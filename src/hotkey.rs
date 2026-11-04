//! Atalho global (Windows): registra o atalho no SO e avisa o app quando ele é
//! pressionado, pra mostrar/focar a janela principal.
//!
//! O registro roda numa thread própria porque a crate `global-hotkey` cria uma
//! janela Win32 na thread que chama o gerenciador e precisa que ESSA thread
//! bombeie mensagens (`winctl::run_message_loop`) — só assim o `WM_HOTKEY`
//! chega. O texto do atalho vem do config (`win+alt+v` por padrão); texto
//! inválido não derruba nada: cai no padrão e o app avisa no rodapé.
//!
//! Fora do Windows não há atalho global (decisão da suíte): `spawn` devolve
//! `None` e o app nasce sem o canal.

use std::sync::mpsc::Receiver;

/// O que o atalho avisa pro app.
// No Linux o módulo é stub e nada constrói as variantes — allow daquele alvo.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotkeyEvent {
    /// Registrado no SO — nada a mostrar.
    Registered,
    /// O SO recusou: combinação já usada por outro app.
    Conflict,
    /// O texto do config não é um atalho válido (o padrão foi usado).
    Invalid,
    /// O gerenciador não subiu: esta sessão fica sem atalho.
    Unavailable,
    /// O atalho foi pressionado: mostrar/focar a janela.
    Pressed,
}

/// Sobe a thread do atalho. `None` = sem atalho nesta sessão (fora do Windows
/// ou falha ao criar a thread).
pub fn spawn(spec: String) -> Option<Receiver<HotkeyEvent>> {
    imp::spawn(spec)
}

/// O Win/Super está pressionado AGORA? O egui não carrega essa tecla nas
/// `Modifiers` fora do macOS (`mac_cmd` só vale lá; no Windows o Super some),
/// e a captura do atalho nas opções precisa dela pra montar `win+alt+v`.
#[cfg(windows)]
pub fn win_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_RWIN};
    // SAFETY: GetAsyncKeyState só LÊ o estado do teclado; sem pré-condição.
    // O bit alto (valor negativo) = tecla pressionada neste instante.
    unsafe { GetAsyncKeyState(i32::from(VK_LWIN.0)) < 0 || GetAsyncKeyState(i32::from(VK_RWIN.0)) < 0 }
}

/// Fora do Windows não há atalho global: nunca há "win" pra capturar.
#[cfg(not(windows))]
pub fn win_down() -> bool {
    false
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Mutex;

    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

    use super::HotkeyEvent;
    use crate::winctl;
    use utils_ui::config::DEFAULT_HOTKEY;

    pub fn spawn(spec: String) -> Option<Receiver<HotkeyEvent>> {
        let (tx, rx) = channel();
        let fallback = tx.clone();
        let handle = std::thread::Builder::new().name("utils-hotkey".into()).spawn(move || run(&spec, tx));
        match handle {
            Ok(_) => Some(rx),
            Err(e) => {
                eprintln!("hotkey: thread: {e}");
                // Ainda devolve o canal: o app recebe o aviso e segue sem atalho.
                let _ = fallback.send(HotkeyEvent::Unavailable);
                Some(rx)
            }
        }
    }

    /// Tudo que toca o SO acontece nesta thread: gerenciador, registro e o
    /// pump de mensagens que entrega o `WM_HOTKEY`.
    fn run(spec: &str, tx: Sender<HotkeyEvent>) {
        // Config quebrada não derruba o app: cai no padrão e avisa.
        let (mods, code) = match parse(spec) {
            Some(hk) => hk,
            None => {
                let _ = tx.send(HotkeyEvent::Invalid);
                match parse(DEFAULT_HOTKEY) {
                    Some(hk) => hk,
                    // O padrão é uma constante do repo: chegar aqui é bug, e
                    // sem atalho é melhor que panic.
                    None => return,
                }
            }
        };

        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("hotkey: gerenciador: {e}");
                let _ = tx.send(HotkeyEvent::Unavailable);
                return;
            }
        };
        let hotkey = HotKey::new(Some(mods), code);
        if let Err(e) = manager.register(hotkey) {
            eprintln!("hotkey: registrar {spec}: {e}");
            let _ = tx.send(HotkeyEvent::Conflict);
            return;
        }
        let _ = tx.send(HotkeyEvent::Registered);

        // O handler global é `Fn + Send + Sync`: o `Sender` do std não é Sync,
        // então vai embrulhado num Mutex (o envio é raro e curtíssimo).
        let events = Mutex::new(tx);
        GlobalHotKeyEvent::set_event_handler(Some(move |ev: GlobalHotKeyEvent| {
            if ev.state != HotKeyState::Pressed {
                return;
            }
            // Janela oculta não roda frame: acorda o winit pelo SO (lição do
            // winctl) e avisa o app pelo canal.
            winctl::show();
            if let Ok(tx) = events.lock() {
                let _ = tx.send(HotkeyEvent::Pressed);
            }
        }));

        // Bloqueia pra sempre: sem este pump o WM_HOTKEY nunca chega. O
        // `manager` vive até o fim da thread (o Drop destroi a janela).
        winctl::run_message_loop();
    }

    /// `"win+alt+v"` → combinação da crate. Mods: `ctrl`/`alt`/`shift`/`win`
    /// (aceita `control`, `super` e `meta` como apelidos), e a ÚLTIMA parte é a
    /// tecla: letra, dígito ou F1–F12. Qualquer outra coisa = `None`.
    fn parse(spec: &str) -> Option<(Modifiers, Code)> {
        let mut mods = Modifiers::empty();
        let mut key = None;
        for part in spec.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= Modifiers::CONTROL,
                "alt" => mods |= Modifiers::ALT,
                "shift" => mods |= Modifiers::SHIFT,
                "win" | "super" | "meta" => mods |= Modifiers::SUPER,
                other => {
                    // Duas teclas na mesma combinação não é atalho: recusa.
                    if key.is_some() {
                        return None;
                    }
                    key = Some(key_code(other)?);
                }
            }
        }
        Some((mods, key?))
    }

    /// Tecla final do atalho (já em minúsculas).
    fn key_code(word: &str) -> Option<Code> {
        Some(match word {
            "a" => Code::KeyA,
            "b" => Code::KeyB,
            "c" => Code::KeyC,
            "d" => Code::KeyD,
            "e" => Code::KeyE,
            "f" => Code::KeyF,
            "g" => Code::KeyG,
            "h" => Code::KeyH,
            "i" => Code::KeyI,
            "j" => Code::KeyJ,
            "k" => Code::KeyK,
            "l" => Code::KeyL,
            "m" => Code::KeyM,
            "n" => Code::KeyN,
            "o" => Code::KeyO,
            "p" => Code::KeyP,
            "q" => Code::KeyQ,
            "r" => Code::KeyR,
            "s" => Code::KeyS,
            "t" => Code::KeyT,
            "u" => Code::KeyU,
            "v" => Code::KeyV,
            "w" => Code::KeyW,
            "x" => Code::KeyX,
            "y" => Code::KeyY,
            "z" => Code::KeyZ,
            "0" => Code::Digit0,
            "1" => Code::Digit1,
            "2" => Code::Digit2,
            "3" => Code::Digit3,
            "4" => Code::Digit4,
            "5" => Code::Digit5,
            "6" => Code::Digit6,
            "7" => Code::Digit7,
            "8" => Code::Digit8,
            "9" => Code::Digit9,
            _ => return function_key(word),
        })
    }

    /// `f1`…`f12`.
    fn function_key(word: &str) -> Option<Code> {
        Some(match word {
            "f1" => Code::F1,
            "f2" => Code::F2,
            "f3" => Code::F3,
            "f4" => Code::F4,
            "f5" => Code::F5,
            "f6" => Code::F6,
            "f7" => Code::F7,
            "f8" => Code::F8,
            "f9" => Code::F9,
            "f10" => Code::F10,
            "f11" => Code::F11,
            "f12" => Code::F12,
            _ => return None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parse_aceita_o_padrao_e_as_formas_usuais() {
            let padrao = parse(DEFAULT_HOTKEY);
            assert_eq!(padrao, Some((Modifiers::ALT | Modifiers::SUPER, Code::KeyV)));

            // Caixa alta, espaço e ordem dos mods não importam.
            assert_eq!(parse(" Ctrl + Alt + F9 "), Some((Modifiers::CONTROL | Modifiers::ALT, Code::F9)));
            assert_eq!(parse("win+1"), Some((Modifiers::SUPER, Code::Digit1)));
            assert_eq!(parse("shift+meta+z"), Some((Modifiers::SUPER | Modifiers::SHIFT, Code::KeyZ)));
        }

        #[test]
        fn parse_recusa_texto_que_nao_e_atalho() {
            assert_eq!(parse(""), None);
            assert_eq!(parse("win+shift"), None, "sem tecla final não é atalho");
            assert_eq!(parse("ctrl+v+b"), None, "duas teclas não é atalho");
            assert_eq!(parse("win+ab"), None, "token que não é tecla");
            assert_eq!(parse("win+f13"), None, "fora da faixa F1–F12");
            assert_eq!(parse("hyper+v"), None, "modificador desconhecido");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::mpsc::Receiver;

    use super::HotkeyEvent;

    /// Fora do Windows não há atalho global: o app nasce sem o canal.
    pub fn spawn(_spec: String) -> Option<Receiver<HotkeyEvent>> {
        None
    }
}
