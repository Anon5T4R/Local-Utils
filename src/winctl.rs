//! Controle Win32 da janela, FORA do ciclo de pintura do egui (porte do
//! `winctl.rs` do local-keys).
//!
//! Por que existe: no Windows, janela oculta não recebe `WM_PAINT`, o winit não
//! entrega `RedrawRequested` e o eframe para de rodar os frames. Num app
//! residente na bandeja, mostrar/esconder têm de falar com o SO direto — a
//! thread da bandeja e a do atalho global chamam `show()`/`hide()` daqui, e é
//! isso que acorda a janela depois de escondida.
//!
//! `run_message_loop` bombeia as mensagens da thread do atalho global: sem ele
//! o `WM_HOTKEY` não chega (a crate registra o atalho numa janela da própria
//! thread).

#![cfg(windows)]

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetMessageW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
    TranslateMessage, MSG, SW_HIDE, SW_SHOW,
};

/// Título exato da janela do eframe (o mesmo `with_title` do `main.rs`).
const WINDOW_TITLE: &str = "Local Utils";

/// HWND da janela egui, descoberto no 1º frame e cacheado (0 = ainda não achou).
static HWND_CACHE: AtomicIsize = AtomicIsize::new(0);

/// `Context` do egui, guardado no 1º frame: é o que permite acordar a UI de
/// fora do ciclo de pintura (thread da bandeja e do atalho global) sem mostrar
/// janela nenhuma — ver [`wake`].
static REPAINT: OnceLock<eframe::egui::Context> = OnceLock::new();

struct EnumCtx {
    pid: u32,
    found: isize,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == ctx.pid && IsWindowVisible(hwnd).as_bool() {
        let mut buf = [0u16; 64];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        // A janela do eframe tem o título exato; o console do debug (build sem
        // `windows_subsystem`) tem o caminho do exe — não bate.
        if title == WINDOW_TITLE {
            ctx.found = hwnd.0 as isize;
            return BOOL(0); // para de enumerar
        }
    }
    BOOL(1)
}

/// Descobre e cacheia o HWND (chamado a cada frame; depois do 1º é um load).
pub fn discover() {
    if HWND_CACHE.load(Ordering::Relaxed) != 0 {
        return;
    }
    let mut ctx = EnumCtx { pid: std::process::id(), found: 0 };
    // SAFETY: o callback só escreve no `EnumCtx` que passamos por ponteiro e
    // vive até o `EnumWindows` retornar.
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    if ctx.found != 0 {
        HWND_CACHE.store(ctx.found, Ordering::Relaxed);
    }
}

fn hwnd() -> Option<HWND> {
    let v = HWND_CACHE.load(Ordering::Relaxed);
    if v != 0 {
        Some(HWND(v as *mut core::ffi::c_void))
    } else {
        None
    }
}

/// Mostra e traz pra frente. Acordar a janela oculta por aqui é o ponto: não
/// passa pelo pipeline de repaint (congelado enquanto oculta).
pub fn show() {
    if let Some(h) = hwnd() {
        // SAFETY: `h` veio do `EnumWindows` e é uma janela viva do processo.
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
    }
}

/// Esconde pra bandeja (o X da janela). O processo continua vivo.
pub fn hide() {
    if let Some(h) = hwnd() {
        // SAFETY: idem `show`.
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

/// Guarda o `Context` do egui (chamado a cada frame; só o primeiro vale).
pub fn install_repaint(ctx: &eframe::egui::Context) {
    let _ = REPAINT.set(ctx.clone());
}

/// Acorda a UI **sem mostrar janela nenhuma**: o `request_repaint` atravessa o
/// event loop (UserEvent) e, com a janela principal escondida, o eframe deste
/// fork roda o frame e pinta viewports invisíveis direto. É o que permite o
/// popup do atalho nascer com a janela principal na bandeja.
pub fn wake() {
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

/// Posição do cursor em POINTS (px do SO ÷ `pixels_per_point`). O popup é o
/// único interessado: ele nasce colado no ponteiro, que pode estar sobre outra
/// janela — fora do nosso viewport, o egui não tem evento nenhum.
pub fn cursor_pos(ppp: f32) -> Option<eframe::egui::Pos2> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    if ppp <= 0.0 {
        return None;
    }
    let mut pt = POINT::default();
    // SAFETY: GetCursorPos só escreve na struct local.
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    Some(eframe::egui::pos2(pt.x as f32 / ppp, pt.y as f32 / ppp))
}

/// Bombeia mensagens do Win32 até o loop terminar. **Bloqueia**: roda na thread
/// do atalho global, que é a dona da janela que recebe o `WM_HOTKEY`.
pub fn run_message_loop() {
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` é local e vive durante toda a chamada; a thread é nossa.
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 <= 0 {
            // 0 = WM_QUIT, -1 = erro: nos dois casos não há mais o que bombear.
            break;
        }
        // SAFETY: mensagem válida devolvida pelo `GetMessageW`.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
