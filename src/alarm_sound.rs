//! Som do alarme do relógio (`tools_clock.rs`).
//!
//! Duas fontes, uma ordem: o `.wav` configurado pelo usuário nas Opções toca em
//! LOOP (`PlaySoundW` com `SND_ASYNC | SND_LOOP`) enquanto o aviso estiver de
//! pé; sem arquivo (ou com caminho que não existe), o fallback são três bipes
//! do esquema de som do SO (`MessageBeep`), espaçados pelo tique do relógio.
//!
//! Quem para o som: o aviso que expira (`ClockState::tick`) e o ack do usuário
//! (`app.rs`: clique, Esc ou a janela voltando ao foco) — o toque é um alerta,
//! não uma sirene que só desliga fechando o processo. Parar é `PlaySoundW(null)`.
//!
//! Só o Windows toca: no Linux o módulo compila e não faz nada (stub
//! consciente — a suíte ainda não escolheu o backend de áudio do AppImage).

use std::path::Path;

/// Intervalo entre os bipes do fallback (ms) — o mesmo tanto que separa três
/// toques de campainha de celular.
const BEEP_GAP_MS: u64 = 700;
/// Quantos bipes o fallback toca por alarme.
const BEEPS: u8 = 3;

/// Estado do som entre frames (vive no `ClockState`).
#[derive(Default)]
pub struct AlarmSound {
    /// Quando o toque começou (ms). `None` = nada tocando.
    started_ms: Option<u64>,
    /// Instante do próximo bipe do fallback.
    next_beep_ms: u64,
    /// Bipes que ainda faltam (só no fallback).
    beeps_left: u8,
    /// O `PlaySoundW` está em loop de arquivo: exige um `stop` explícito.
    looping: bool,
}

impl AlarmSound {
    /// Está tocando (loop de arquivo ou bipes do fallback ainda pendentes)?
    pub fn playing(&self) -> bool {
        self.started_ms.is_some() || self.looping
    }

    /// Começa o toque de um alarme. Um toque novo sempre mata o anterior —
    /// dois alarmes no mesmo minuto não podem sobrepor áudio.
    pub fn start(&mut self, file: &str, now: u64) {
        self.stop();
        self.started_ms = Some(now);
        self.next_beep_ms = now;
        self.beeps_left = BEEPS;
        self.looping = play_file(file);
    }

    /// Tique do relógio: dispara as batidas do fallback. Com arquivo em loop
    /// não faz nada (o `PlaySoundW` cuida da repetição).
    pub fn tick(&mut self, now: u64) {
        if self.looping || self.beeps_left == 0 {
            return;
        }
        if now < self.next_beep_ms {
            return;
        }
        beep();
        self.beeps_left -= 1;
        self.next_beep_ms = now.saturating_add(BEEP_GAP_MS);
        if self.beeps_left == 0 {
            // Cumpriu as três batidas: não há mais nada pendente.
            self.started_ms = None;
        }
    }

    /// Para tudo na hora (`PlaySoundW(null)` no Windows).
    pub fn stop(&mut self) {
        stop_playback();
        self.looping = false;
        self.beeps_left = 0;
        self.started_ms = None;
    }
}

/// Teste das Opções: toca o arquivo UMA vez; sem arquivo válido, um bipe do SO
/// (o mesmo som do fallback, então "Testar" sempre dá retorno audível).
pub fn preview(file: &str) {
    match file_path(file) {
        Some(path) => play_once(path),
        None => beep(),
    }
}

/// Caminho do `.wav` que existe de verdade. Vazio ou inexistente = `None`
/// (cai no fallback).
fn file_path(file: &str) -> Option<&Path> {
    let trimmed = file.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    path.is_file().then_some(path)
}

#[cfg(windows)]
fn play_file(file: &str) -> bool {
    match file_path(file) {
        Some(path) => {
            play_looping(path);
            true
        }
        None => false,
    }
}

#[cfg(not(windows))]
fn play_file(_file: &str) -> bool {
    false
}

#[cfg(windows)]
fn play_looping(path: &Path) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_LOOP, SND_NODEFAULT};

    let buf = wide(path);
    // SAFETY: `buf` é NUL-terminado e vive até o fim da chamada. O modo
    // assíncrono só solta a reprodução — quem lê o caminho é o `winmm` na
    // hora da chamada, não um ponteiro guardado.
    unsafe {
        let _ = PlaySoundW(windows::core::PCWSTR(buf.as_ptr()), None, SND_ASYNC | SND_FILENAME | SND_LOOP | SND_NODEFAULT);
    }
}

#[cfg(windows)]
fn play_once(path: &Path) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};

    let buf = wide(path);
    // SAFETY: mesmo contrato do `play_looping`, sem o `SND_LOOP`.
    unsafe {
        let _ = PlaySoundW(windows::core::PCWSTR(buf.as_ptr()), None, SND_ASYNC | SND_FILENAME | SND_NODEFAULT);
    }
}

#[cfg(not(windows))]
fn play_once(_path: &Path) {}

#[cfg(windows)]
fn stop_playback() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_PURGE};

    // SAFETY: `PCWSTR::null()` + `SND_PURGE` corta o som do processo (o
    // "PlaySoundW(null)" do requisito).
    unsafe {
        let _ = PlaySoundW(windows::core::PCWSTR::null(), None, SND_PURGE);
    }
}

#[cfg(not(windows))]
fn stop_playback() {}

#[cfg(windows)]
fn beep() {
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::MB_ICONEXCLAMATION;

    // SAFETY: `MessageBeep` só toca um alias do esquema de som do SO.
    unsafe {
        let _ = MessageBeep(MB_ICONEXCLAMATION);
    }
}

#[cfg(not(windows))]
fn beep() {}

/// Caminho → UTF-16 NUL-terminado (o `PCWSTR` do `PlaySoundW`).
#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sem arquivo válido, o fallback agenda exatamente três bipes e termina.
    #[test]
    fn fallback_toca_tres_bipes() {
        let mut sound = AlarmSound::default();
        sound.start("", 0);
        assert!(sound.playing());

        sound.tick(0);
        assert_eq!(sound.beeps_left, BEEPS - 1);
        sound.tick(BEEP_GAP_MS - 1);
        assert_eq!(sound.beeps_left, BEEPS - 1, "antes da hora não toca de novo");
        sound.tick(BEEP_GAP_MS);
        sound.tick(BEEP_GAP_MS * 2);
        assert_eq!(sound.beeps_left, 0);
        assert!(!sound.playing(), "as três batidas encerram o toque");
    }

    /// Parar zera o estado e é idempotente (chamado a cada interação).
    #[test]
    fn parar_zera_o_estado() {
        let mut sound = AlarmSound::default();
        sound.start("", 0);
        sound.stop();
        assert!(!sound.playing());
        assert_eq!(sound.beeps_left, 0);
        sound.stop();
        assert!(!sound.playing());
    }

    /// Caminho vazio ou inexistente nunca vira arquivo (cai no fallback).
    #[test]
    fn caminho_invalido_cai_no_fallback() {
        assert_eq!(file_path(""), None);
        assert_eq!(file_path("   "), None);
        assert_eq!(file_path("C:/caminho/que/nao/existe.wav"), None);
    }
}
