//! Colar direto na janela em foco (Windows): o popup devolve o primeiro plano à
//! janela de origem e dispara Ctrl+V nela.
//!
//! O HWND é capturado ANTES de o popup abrir ([`remember_foreground`]) — depois
//! disso o primeiro plano passa a ser o próprio popup, e colar nele não faz
//! sentido. A janela de destino fica num `AtomicIsize` (handle cru): o app só
//! chama isto no fluxo do popup, nunca na lista da janela principal.
//!
//! Fora do Windows não há como colar em outro app (nem atalho global): o stub
//! devolve erro e o app mostra no rodapé.

// Fora do Windows ninguém chama as funções (o fluxo do popup é Windows-only).
#![cfg_attr(not(windows), allow(dead_code))]

/// Guarda o HWND do primeiro plano AGORA (o app chama no instante do atalho).
pub fn remember_foreground() {
    imp::remember();
}

/// Devolve o foco à janela capturada e manda Ctrl+V. `Err` = motivo pronto pro
/// rodapé (o app já recopiou; só a colagem falhou).
pub fn paste_into_focused() -> Result<(), String> {
    imp::paste()
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::time::Duration;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    /// HWND da janela de destino (0 = nenhuma capturada ainda).
    static TARGET: AtomicIsize = AtomicIsize::new(0);

    /// Pausa entre devolver o foco e mandar o Ctrl+V: a janela de destino
    /// precisa voltar a ser a dona do teclado antes do `SendInput`, senão as
    /// teclas caem no app errado (ou em ninguém).
    const FOCUS_SETTLE: Duration = Duration::from_millis(40);

    pub fn remember() {
        // SAFETY: GetForegroundWindow só lê o estado do SO; sem pré-condição.
        let hwnd = unsafe { GetForegroundWindow() };
        let raw = if hwnd.is_invalid() { 0 } else { hwnd.0 as isize };
        TARGET.store(raw, Ordering::Relaxed);
    }

    pub fn paste() -> Result<(), String> {
        let raw = TARGET.load(Ordering::Relaxed);
        if raw == 0 {
            return Err("nenhuma janela de destino capturada".into());
        }
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        // SAFETY: o handle veio do SO na captura; se a janela já morreu, o
        // SetForegroundWindow devolve FALSE e o erro sobe pro rodapé.
        let brought = unsafe { SetForegroundWindow(hwnd) };
        if !brought.as_bool() {
            return Err("o Windows recusou trazer a janela de destino pra frente".into());
        }
        std::thread::sleep(FOCUS_SETTLE);
        send_ctrl_v()
    }

    /// Os quatro eventos do atalho, SEPARADOS: o Windows não trata "Ctrl+V num
    /// evento só" como atalho.
    fn send_ctrl_v() -> Result<(), String> {
        let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 },
            },
        };
        let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
        // SAFETY: o slice é local e vive durante toda a chamada.
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err(format!("SendInput entregou {sent} de {} eventos", inputs.len()));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn remember() {}

    pub fn paste() -> Result<(), String> {
        Err("colar na janela em foco: só Windows por enquanto".into())
    }
}
