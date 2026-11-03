//! Ponte com a área de transferência do SO.
//!
//! Duas metades de propósito:
//!
//! - [`spawn_monitor`] — thread dedicada que só ESCUTA (janela message-only +
//!   `WM_CLIPBOARDUPDATE`) e avisa pelo canal. Ela não lê o conteúdo: ler na
//!   thread de mensagens acoplaria a leitura (que pode ser lenta — imagem
//!   grande) ao bombeamento de mensagens do Win32.
//! - [`read_current`] / [`write_back`] — leitura e regravação, chamadas pela
//!   UI quando o evento chega (e nos botões de recopiar do lote 3).
//!
//! **Exclusão de apps sensíveis**: antes de reportar a cópia, o monitor olha a
//! janela em primeiro plano e descarta o evento se o processo estiver na lista
//! (gerenciador de senhas copiando a senha não entra no histórico).
//!
//! Fora do Windows tudo é stub — este lote é Windows-only e não puxa dep
//! cross-platform nenhuma.

use std::sync::mpsc::Sender;

use crate::history::Entry;

/// Evento da thread de monitoramento.
#[derive(Clone, Debug)]
pub enum ClipEvent {
    /// A área de transferência mudou. `seq` é o número de sequência do
    /// Windows (`GetClipboardSequenceNumber`), útil pra descartar repetição.
    Changed { seq: u32 },
    /// Falha fatal do monitor (a thread encerra depois de avisar).
    Error(String),
}

/// Processos cujo CONTEÚDO nunca entra no histórico: gerenciadores de senha.
/// Comparação pelo nome do executável, em minúsculas (sem caminho).
pub const DEFAULT_EXCLUDE: &[&str] = &["local-keys.exe", "localkeys.exe", "keepass.exe", "keepassxc.exe", "1password.exe", "bitwarden.exe"];

/// Sobe a thread de monitoramento. O `Sender` é do app: quando o receptor
/// morre (app fechando), a thread percebe no envio e encerra.
///
/// `exclude` é a lista de executáveis ignorados (ver [`DEFAULT_EXCLUDE`]).
pub fn spawn_monitor(tx: Sender<ClipEvent>, exclude: Vec<String>) -> Result<std::thread::JoinHandle<()>, String> {
    imp::spawn_monitor(tx, exclude)
}

/// Lê a cópia atual e monta a entrada: **uma só**, do formato mais rico pro
/// mais simples (imagem > arquivos > HTML > RTF > texto). `None` se a área de
/// transferência está vazia, tem só formatos que não nos interessam ou está
/// travada por outro processo.
pub fn read_current() -> Option<Entry> {
    imp::read_current()
}

/// Repõe a entrada na área de transferência (botão de recopiar). Texto vira
/// CF_UNICODETEXT, imagem vira CF_BITMAP, arquivos viram CF_HDROP e HTML/RTF
/// tentam o formato rico registrado.
pub fn write_back(entry: &Entry) -> Result<(), String> {
    imp::write_back(entry)
}

#[cfg(windows)]
mod imp {
    //! Implementação Windows: `windows` (Win32 cru) para a janela/processos e
    //! `clipboard-win` para o conteúdo (mesma crate do Local-Keys).

    use std::sync::mpsc::Sender;

    use clipboard_win::{formats, Clipboard};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HINSTANCE};
    use windows::Win32::System::DataExchange::{AddClipboardFormatListener, GetClipboardSequenceNumber};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, RegisterClassW, HWND_MESSAGE,
        MSG, WINDOW_EX_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW, WS_OVERLAPPED,
    };

    use super::ClipEvent;
    use crate::history::{self, Entry, Kind};

    /// Formatos registrados (nomes fixos da convenção do Windows).
    const RTF_FORMAT: &str = "Rich Text Format";

    /// Nome da classe da nossa janela message-only.
    const CLASS_NAME: &str = "LocalUtilsClipListener";

    /// WndProc só pra satisfazer a ABI "system" do campo — o comportamento é
    /// o default do Win32 (nada é processado aqui; o loop lê direto do queue).
    unsafe extern "system" fn wnd_proc(
        hwnd: windows::Win32::Foundation::HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// UTF-16 com NUL final — o `PCWSTR` que as APIs pedem.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn spawn_monitor(tx: Sender<ClipEvent>, exclude: Vec<String>) -> Result<std::thread::JoinHandle<()>, String> {
        std::thread::Builder::new()
            .name("clip-monitor".into())
            .spawn(move || {
                // Erro só UMA vez: a thread para e o app recebe a causa (nada
                // de spammar o canal a cada volta).
                if let Err(e) = listen(&tx, &exclude) {
                    let _ = tx.send(ClipEvent::Error(e));
                }
            })
            .map_err(|e| format!("iniciar a thread do clipboard: {e}"))
    }

    /// Cria a janela message-only, assina `WM_CLIPBOARDUPDATE` e bombeia
    /// mensagens até o app fechar (ou até o Win32 falhar).
    fn listen(tx: &Sender<ClipEvent>, exclude: &[String]) -> Result<(), String> {
        // Todo o corpo é FFI do Win32; os handles criados aqui vivem até o fim
        // do loop (o processo morre com eles).
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| format!("módulo do processo: {e}"))?;
            let class_name = wide(CLASS_NAME);
            // Janela de verdade não serve: escondida (HWND_MESSAGE) ela só
            // recebe as mensagens que assinamos.
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(instance.0),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err("registrar a classe da janela de monitoramento falhou".into());
            }

            let title = wide("Local Utils — monitor de clipboard");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(HINSTANCE(instance.0)),
                None,
            )
            .map_err(|e| format!("criar a janela de monitoramento: {e}"))?;

            AddClipboardFormatListener(hwnd).map_err(|e| format!("assinar WM_CLIPBOARDUPDATE: {e}"))?;

            let mut last_seq = 0u32;
            let mut msg = MSG::default();
            loop {
                let ret = GetMessageW(&mut msg, None, 0, 0);
                if ret.0 == 0 {
                    // WM_QUIT.
                    break;
                }
                if ret.0 == -1 {
                    return Err("GetMessageW falhou".into());
                }
                if msg.message == WM_CLIPBOARDUPDATE && !is_excluded(exclude) {
                    let seq = GetClipboardSequenceNumber();
                    // O Windows pode repetir o aviso pra mesma cópia; `seq`
                    // idêntico já foi reportado.
                    if seq != last_seq {
                        last_seq = seq;
                        // Receptor sumiu = app fechando: encerra a thread.
                        if tx.send(ClipEvent::Changed { seq }).is_err() {
                            break;
                        }
                    }
                }
                DispatchMessageW(&msg);
            }
            Ok(())
        }
    }

    /// A cópia veio de um app da lista de exclusão?
    fn is_excluded(exclude: &[String]) -> bool {
        match foreground_process_name() {
            Some(name) => exclude.iter().any(|e| e.eq_ignore_ascii_case(&name)),
            None => false,
        }
    }

    /// Nome (minúsculo, sem caminho) do executável em primeiro plano.
    fn foreground_process_name() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            if pid == 0 {
                return None;
            }

            // PROCESS_QUERY_LIMITED_INFORMATION é o mínimo pra ler o nome de
            // outro processo — não exige elevação nem Win32_Security.
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let res = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size);
            let _ = CloseHandle(process);
            res.ok()?;
            if size == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..size as usize]);
            let name = path.rsplit(['\\', '/']).next()?;
            if name.is_empty() {
                return None;
            }
            Some(name.to_ascii_lowercase())
        }
    }

    /// Entrada já normalizada (id/carimbo gerados aqui, `pinned` começa false).
    fn entry_of(
        id: u128,
        ts_ms: u64,
        kind: Kind,
        text: Option<String>,
        files: Option<Vec<String>>,
        image: Option<Vec<u8>>,
        thumb: Option<Vec<u8>>,
    ) -> Entry {
        Entry { id, ts_ms, kind, pinned: false, text, files, image_png: image, thumb_png: thumb }
    }

    pub fn read_current() -> Option<Entry> {
        // UMA abertura pra todas as leituras: elas enxergam o mesmo instante
        // (e uma cópia nova no meio do caminho não mistura formatos).
        let _clip = Clipboard::new_attempts(10).ok()?;

        let ts_ms = history::now_ms();
        let id = history::new_id();

        // Prioridade do mais rico pro mais simples: um Entry só por cópia.
        if let Some((png, thumb)) = read_image() {
            return Some(entry_of(id, ts_ms, Kind::Image, None, None, Some(png), thumb));
        }
        if let Some(files) = read_files() {
            return Some(entry_of(id, ts_ms, Kind::Files, None, Some(files), None, None));
        }
        if let Some(html) = read_html() {
            return Some(entry_of(id, ts_ms, Kind::Html, Some(html), None, None, None));
        }
        if let Some(rtf) = read_rtf() {
            return Some(entry_of(id, ts_ms, Kind::Rtf, Some(rtf), None, None, None));
        }
        read_text().map(|text| entry_of(id, ts_ms, Kind::Text, Some(text), None, None, None))
    }

    /// CF_BITMAP → PNG cheio + miniatura. O `get_bitmap` do clipboard-win
    /// devolve um BMP completo (header + pixels), então quem decodifica é a
    /// `image`.
    fn read_image() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        let bmp = clipboard_win::get::<Vec<u8>, _>(formats::Bitmap).ok()?;
        if bmp.is_empty() {
            return None;
        }
        let img = image::load_from_memory(&bmp).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let mut rgba = img.into_raw();
        // DIB de 32 bits do Windows costuma vir com alpha 0 (o CF_BITMAP não
        // guarda transparência de verdade): sem esse ajuste o PNG sai
        // invisível na lista.
        let (pixels, _) = rgba.as_chunks::<4>();
        if pixels.iter().all(|px| px[3] == 0) {
            for px in rgba.as_chunks_mut::<4>().0 {
                px[3] = 255;
            }
        }
        let png = history::encode_png(&rgba, w, h)?;
        let thumb = history::make_thumb(&rgba, w, h, history::THUMB_MAX);
        Some((png, thumb))
    }

    /// CF_HDROP — caminhos copiados do Explorer.
    fn read_files() -> Option<Vec<String>> {
        let files = clipboard_win::get::<Vec<String>, _>(formats::FileList).ok()?;
        if files.is_empty() {
            None
        } else {
            Some(files)
        }
    }

    /// CF_HTML — o getter do clipboard-win já devolve só o FRAGMENTO (tira o
    /// cabeçalho StartHTML/EndFragment).
    fn read_html() -> Option<String> {
        let format = formats::Html::new()?;
        let html = clipboard_win::get::<String, _>(format).ok()?;
        if html.trim().is_empty() {
            None
        } else {
            Some(html)
        }
    }

    /// "Rich Text Format" — formato registrado, sem getter próprio na crate:
    /// bytes crus e NULs de padding do GlobalAlloc aparados.
    fn read_rtf() -> Option<String> {
        let format = clipboard_win::register_format(RTF_FORMAT)?;
        let mut bytes = Vec::new();
        clipboard_win::raw::get_vec(format.get(), &mut bytes).ok()?;
        while bytes.last() == Some(&0) {
            bytes.pop();
        }
        if bytes.is_empty() {
            return None;
        }
        // RTF na prática é ASCII/latin-1; se não for UTF-8, ignora (o texto
        // puro da mesma cópia cobre o caso).
        String::from_utf8(bytes).ok()
    }

    /// CF_UNICODETEXT.
    fn read_text() -> Option<String> {
        let text = clipboard_win::get::<String, _>(formats::Unicode).ok()?;
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    // Chamado pela aba Clipboard (botão/Enter de recopiar).
    pub fn write_back(entry: &Entry) -> Result<(), String> {
        // Uma abertura só pra limpar+gravar (o `_clip` fecha no fim).
        let _clip = Clipboard::new_attempts(10).map_err(|e| format!("abrir o clipboard: {e}"))?;
        match entry.kind {
            Kind::Text => {
                let text = entry.text.as_deref().ok_or_else(|| "entrada de texto sem texto".to_string())?;
                clipboard_win::raw::set_string(text).map_err(|e| format!("gravar o texto: {e}"))
            }
            Kind::Html => {
                let html = entry.text.as_deref().ok_or_else(|| "entrada HTML sem conteúdo".to_string())?;
                let format = formats::Html::new().ok_or_else(|| "formato CF_HTML indisponível".to_string())?;
                // Não existe plain separado na Entry (o rico mora em `text`):
                // grava só o CF_HTML depois de limpar a área de transferência.
                clipboard_win::raw::empty().map_err(|e| format!("limpar o clipboard: {e}"))?;
                clipboard_win::raw::set_html(format.code(), html).map_err(|e| format!("gravar o HTML: {e}"))
            }
            Kind::Rtf => {
                let rtf = entry.text.as_deref().ok_or_else(|| "entrada RTF sem conteúdo".to_string())?;
                let format = clipboard_win::register_format(RTF_FORMAT).ok_or_else(|| "formato RTF indisponível".to_string())?;
                clipboard_win::raw::empty().map_err(|e| format!("limpar o clipboard: {e}"))?;
                clipboard_win::raw::set(format.get(), rtf.as_bytes()).map_err(|e| format!("gravar o RTF: {e}"))
            }
            Kind::Image => {
                let png = entry.image_png.as_deref().ok_or_else(|| "entrada de imagem sem bytes".to_string())?;
                let rgb = image::load_from_memory(png).map_err(|e| format!("decodificar o PNG da entrada: {e}"))?.to_rgb8();
                let (w, h) = rgb.dimensions();
                let mut bmp = Vec::new();
                image::codecs::bmp::BmpEncoder::new(&mut bmp)
                    .encode(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
                    .map_err(|e| format!("codificar o BMP pro clipboard: {e}"))?;
                clipboard_win::raw::empty().map_err(|e| format!("limpar o clipboard: {e}"))?;
                clipboard_win::raw::set_bitmap(&bmp).map_err(|e| format!("gravar a imagem: {e}"))
            }
            Kind::Files => {
                let files = entry.files.as_deref().ok_or_else(|| "entrada de arquivos vazia".to_string())?;
                if files.is_empty() {
                    return Err("entrada de arquivos vazia".to_string());
                }
                clipboard_win::raw::empty().map_err(|e| format!("limpar o clipboard: {e}"))?;
                clipboard_win::raw::set_file_list(files).map_err(|e| format!("gravar a lista de arquivos: {e}"))
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    //! Stub: este lote é Windows-only. Sem dep cross-platform nova.

    use std::sync::mpsc::Sender;

    use super::ClipEvent;
    use crate::history::Entry;

    pub fn spawn_monitor(_tx: Sender<ClipEvent>, _exclude: Vec<String>) -> Result<std::thread::JoinHandle<()>, String> {
        Err("monitor de clipboard: só Windows por enquanto".into())
    }

    pub fn read_current() -> Option<Entry> {
        None
    }

    pub fn write_back(_entry: &Entry) -> Result<(), String> {
        Err("escrever no clipboard: só Windows por enquanto".into())
    }
}
