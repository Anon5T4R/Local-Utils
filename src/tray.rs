//! Bandeja do sistema (Windows): ícone com menu "Abrir"/"Sair" e a thread que
//! atende os comandos (molde do `spawn_tray` do local-keys).
//!
//! A thread é obrigatória: com a janela oculta o eframe congela (sem WM_PAINT
//! não há frame), então os handlers do tray-icon não podem depender do loop de
//! UI — mostram a janela pelo `winctl` (SO direto) e só avisam o app pelo
//! canal, que é drenado no início de cada frame.
//!
//! Fora do Windows não há bandeja (política da suíte: o tray-icon puxaria
//! gtk3 pro AppImage) — o canal nasce desconectado e o X fecha o app.

/// Comando vindo da bandeja (thread própria → app).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
// No Linux o `spawn` é stub e nada constrói as variantes — allow daquele alvo.
#[cfg_attr(not(windows), allow(dead_code))]
pub enum TrayCmd {
    /// Mostrar/focar a janela (clique no ícone ou "Abrir").
    Show,
    /// Sair DE VERDADE: o app salva o histórico e encerra o processo.
    Quit,
}

/// Textos do menu/ícone. Montados uma vez na subida, no idioma do boot (trocar
/// o idioma vale no próximo arranque, como o resto do que é do SO).
// Fora do Windows não há menu (o `spawn` ignora os rótulos): sem isso o lint
// reclamaria dos campos nunca lidos naquele alvo.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Labels {
    pub tooltip: String,
    pub open: String,
    pub quit: String,
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{channel, Receiver};

    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{Labels, TrayCmd};
    use crate::winctl;

    /// A bandeja viva. Segurar isto segura o ícone; o drop o remove.
    pub struct Tray {
        _icon: TrayIcon,
    }

    /// Ícone da bandeja: o MESMO PNG da janela (o .exe cunha o .ico no build).
    fn tray_icon() -> Option<tray_icon::Icon> {
        let img = image::load_from_memory(crate::ICON_PNG).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        tray_icon::Icon::from_rgba(img.into_raw(), w, h).ok()
    }

    /// Sobe a bandeja e a thread que atende os comandos dela. Devolve o ícone
    /// (que o app segura vivo) e o canal que o frame drena.
    pub fn spawn(labels: Labels) -> (Option<Tray>, Receiver<TrayCmd>) {
        let (app_tx, app_rx) = channel::<TrayCmd>();
        let (cmd_tx, cmd_rx) = channel::<TrayCmd>();

        let open = MenuItem::with_id("open", labels.open.as_str(), true, None);
        let quit = MenuItem::with_id("quit", labels.quit.as_str(), true, None);
        let menu = Menu::new();
        let _ = menu.append(&open);
        let _ = menu.append(&quit);

        // Ícone ausente não derruba o app: sobe o menu e segue (o X fecha de
        // verdade quando não há bandeja).
        let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(labels.tooltip);
        if let Some(icon) = tray_icon() {
            builder = builder.with_icon(icon);
        }

        let tray = match builder.build() {
            Ok(t) => {
                // Clique esquerdo no ícone = abrir (não alterna: mostrar).
                let tx = cmd_tx.clone();
                TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        let _ = tx.send(TrayCmd::Show);
                    }
                }));
                let tx = cmd_tx.clone();
                MenuEvent::set_event_handler(Some(move |ev: MenuEvent| match ev.id.0.as_str() {
                    "open" => {
                        let _ = tx.send(TrayCmd::Show);
                    }
                    "quit" => {
                        let _ = tx.send(TrayCmd::Quit);
                    }
                    _ => {}
                }));
                Some(Tray { _icon: t })
            }
            Err(e) => {
                eprintln!("bandeja: não deu para criar: {e}");
                None
            }
        };

        let handle = std::thread::Builder::new().name("utils-tray".into()).spawn(move || {
            // Janela oculta não roda frame: mostrar é aqui, no SO direto, e o
            // evento só avisa o app (que pede repaint/derruba o processo).
            // A mesma thread consome a flag "mostra-te" da segunda instância
            // (single-instance, ver main): o frame não roda com a janela
            // oculta, então não pode ser o app.
            let show_flag = utils_ui::config::config_dir(crate::app::APP_ID).join("show.flag");
            loop {
                match cmd_rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(cmd) => match cmd {
                        TrayCmd::Show => {
                            winctl::show();
                            let _ = app_tx.send(TrayCmd::Show);
                        }
                        TrayCmd::Quit => {
                            // Janela visível primeiro: o "Sair" roda no frame (que
                            // só existe com a janela acordada), salva e encerra.
                            winctl::show();
                            let _ = app_tx.send(TrayCmd::Quit);
                        }
                    },
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if std::fs::remove_file(&show_flag).is_ok() {
                            winctl::show();
                            let _ = app_tx.send(TrayCmd::Show);
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        if let Err(e) = handle {
            eprintln!("bandeja: thread: {e}");
        }

        (tray, app_rx)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::mpsc::{channel, Receiver};

    use super::{Labels, TrayCmd};

    /// Fora do Windows não há bandeja: tipo vazio só pra assinatura bater.
    #[allow(dead_code)] // nunca é construído neste alvo; existe pro tipo do `spawn`.
    pub struct Tray;

    pub fn spawn(_labels: Labels) -> (Option<Tray>, Receiver<TrayCmd>) {
        let (_tx, rx) = channel();
        (None, rx)
    }
}

// Fora do Windows o `Tray` não existe (stub) — só o `spawn` é reexportado.
#[cfg(not(windows))]
pub use imp::spawn;
#[cfg(windows)]
pub use imp::{spawn, Tray};
