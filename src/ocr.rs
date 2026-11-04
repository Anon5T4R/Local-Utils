//! OCR nativo das imagens do histórico (`Windows.Media.Ocr`): lê o texto de um
//! print copiado e devolve pro app mostrar no modal e copiar.
//!
//! A leitura roda numa thread própria, como o monitor do clipboard: o motor
//! WinRT exige apartamento COM na thread que chama e o reconhecimento é
//! bloqueante até o resultado (`.join()` do `IAsyncOperation` — a doc pública
//! chama de `.get()`, esta versão da crate expõe com outro nome). O app pede,
//! o canal avisa e a UI desenha; nada de travar o frame.
//!
//! Fora do Windows não há motor: o botão nem aparece (`clipboard_ui` está sob
//! `cfg(windows)`) e o pedido volta como erro se alguém chamar mesmo assim.

use std::sync::mpsc::Receiver;

use crate::history::{Entry, Kind};

/// Resultado de uma leitura.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug)]
pub enum OcrEvent {
    /// Texto reconhecido. Vazio = a imagem não tinha texto.
    Done { id: u128, text: String },
    /// A leitura não rodou (idioma ausente, motor indisponível, PNG quebrado).
    Failed { error: String },
}

/// Estado do OCR no app: o canal da leitura em andamento e o cache do último
/// resultado (pedir de novo a MESMA imagem devolve na hora).
#[derive(Default)]
pub struct Ocr {
    rx: Option<Receiver<OcrEvent>>,
    /// Entrada que está sendo lida agora.
    pending: Option<u128>,
    /// Último resultado por id (cache simples, um só).
    cache: Option<(u128, String)>,
}

impl Ocr {
    /// Pede a leitura da imagem. `Ok(Some(texto))` = saiu do cache (nada a
    /// esperar); `Ok(None)` = thread rodando, o texto vem no [`Ocr::poll`].
    pub fn request(&mut self, entry: &Entry) -> Result<Option<String>, String> {
        if entry.kind != Kind::Image {
            return Err("a entrada não é uma imagem".into());
        }
        if let Some((id, text)) = &self.cache {
            if *id == entry.id {
                return Ok(Some(text.clone()));
            }
        }
        // Uma leitura por vez: pedir de novo enquanto roda não empilha threads.
        if self.pending.is_some() {
            return Ok(None);
        }
        let png = entry.image_png.clone().ok_or_else(|| "a imagem não tem bytes".to_string())?;
        let id = entry.id;
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::Builder::new().name("utils-ocr".into()).spawn(move || {
            let event = match imp::read(&png) {
                Ok(text) => OcrEvent::Done { id, text },
                Err(error) => OcrEvent::Failed { error },
            };
            // Receptor morto = app fechando: nada a fazer.
            let _ = tx.send(event);
        });
        match handle {
            Ok(_) => {
                self.pending = Some(id);
                self.rx = Some(rx);
                Ok(None)
            }
            Err(e) => Err(format!("iniciar a thread do OCR: {e}")),
        }
    }

    /// Drena o canal. `None` = leitura em andamento (ou nenhuma).
    pub fn poll(&mut self) -> Option<OcrEvent> {
        let event = self.rx.as_ref()?.try_recv().ok()?;
        if let OcrEvent::Done { id, text } = &event {
            self.cache = Some((*id, text.clone()));
        }
        self.pending = None;
        self.rx = None;
        Some(event)
    }

    /// Tem leitura rodando? O app usa pra pedir repaint até o resultado chegar
    /// (o canal não acorda a UI sozinho).
    pub fn running(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(windows)]
mod imp {
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    /// Texto da imagem (PNG). Roda na thread dedicada do [`super::Ocr`].
    pub fn read(png: &[u8]) -> Result<String, String> {
        // WinRT exige o apartamento COM inicializado NA THREAD que chama: sem
        // isto a ativação falha com CO_E_NOTINITIALIZED. MTA porque esta thread
        // não tem janela nem loop de mensagens.
        // SAFETY: RoInitialize só mexe no estado COM da thread atual.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| format!("inicializar o WinRT: {e}"))?;

        let image = image::load_from_memory(png).map_err(|e| format!("decodificar a imagem: {e}"))?;
        let rgba = image.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        if w == 0 || h == 0 {
            return Err("imagem vazia".into());
        }
        let width = i32::try_from(w).map_err(|_| "imagem larga demais".to_string())?;
        let height = i32::try_from(h).map_err(|_| "imagem alta demais".to_string())?;

        // RGBA8 → IBuffer → SoftwareBitmap: o motor do OCR come bitmap WinRT,
        // não PNG. O formato Rgba8 existe no enum, então não há troca de canal.
        let writer = DataWriter::new().map_err(|e| format!("buffer do OCR: {e}"))?;
        writer.WriteBytes(rgba.as_raw()).map_err(|e| format!("gravar o buffer do OCR: {e}"))?;
        let buffer = writer.DetachBuffer().map_err(|e| format!("pegar o buffer do OCR: {e}"))?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Rgba8, width, height)
            .map_err(|e| format!("montar o bitmap do OCR: {e}"))?;

        // Sem idioma de OCR instalado o factory devolve erro — vira ⚠ no rodapé.
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| format!("nenhum idioma de OCR instalado: {e}"))?;
        let operation = engine.RecognizeAsync(&bitmap).map_err(|e| format!("iniciar o reconhecimento: {e}"))?;
        // Bloqueia esta thread até o WinRT terminar (por isso a thread dedicada).
        let result = operation.join().map_err(|e| format!("reconhecer o texto: {e}"))?;
        let text = result.Text().map_err(|e| format!("ler o resultado do OCR: {e}"))?;
        Ok(crate::ocr::normalize(&text.to_string()))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn read(_png: &[u8]) -> Result<String, String> {
        Err("OCR: só Windows por enquanto".into())
    }
}

/// Normaliza o texto do motor: quebras de linha do Windows viram `\n`, espaço
/// sobrando no fim de cada linha sai e as pontas em branco somem. Pura — o
/// teste dela roda em qualquer SO.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn normalize(raw: &str) -> String {
    let unified = raw.replace("\r\n", "\n").replace('\r', "\n");
    unified.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_do_ocr_e_normalizado() {
        assert_eq!(normalize("linha 1\r\nlinha 2  \r\n\r\n"), "linha 1\nlinha 2");
        assert_eq!(normalize("  \r\n  só uma linha  "), "só uma linha");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("a\rb"), "a\nb", "CR solto também vira quebra");
    }
}
