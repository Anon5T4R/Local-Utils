//! Seletor nativo que nunca bloqueia o frame.
//!
//! Molde: `local-pdf/crates/ui-egui/src/pickers.rs` — e o `local-keys` (mesma
//! versão completa). O app tem várias continuações distintas (fila do
//! conversor, "salvar como" do editor de captura, export/import do `.lutils`),
//! então a resposta chega como closure `Then`, executada num frame posterior
//! com o app mutável. Um seletor por vez; cancelamento = paths vazios (a
//! closure decide o que fazer — silêncio, ou o campo de caminho digitado).
//!
//! Backends do rfd 0.17 (defaults): Windows = diálogo nativo; Linux = portal
//! XDG via zbus (Rust puro, fallback Zenity). macOS não é alvo dos pilotos; se
//! um dia for, criar o future na main thread antes de spawnar (ver local-pdf).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::app::UtilsApp;

/// Qual painel nativo mostrar.
pub(crate) enum Ask {
    /// Escolher um arquivo existente.
    File(rfd::AsyncFileDialog),
    /// Escolher vários arquivos existentes (multi-seleção).
    Files(rfd::AsyncFileDialog),
    /// Escolher onde salvar (nome sugerido já no diálogo).
    Save(rfd::AsyncFileDialog),
}

/// O que fazer com os caminhos escolhidos, num frame posterior.
/// Paths vazios = usuário cancelou.
pub(crate) type Then = Box<dyn FnOnce(&mut UtilsApp, Vec<PathBuf>) + Send>;

/// Uma escolha pronta, esperando o frame.
pub(crate) struct Picked {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) then: Then,
}

/// Seletor em andamento + escolhas prontas.
#[derive(Default)]
pub(crate) struct Pickers {
    showing: Arc<AtomicBool>,
    done: Arc<Mutex<Vec<Picked>>>,
}

impl Pickers {
    /// Um seletor está aberto (ou a resposta ainda espera o frame)?
    pub(crate) fn busy(&self) -> bool {
        self.showing.load(Ordering::SeqCst)
    }

    /// Mostra `ask` sem bloquear o frame; `then` roda num frame posterior com
    /// a escolha (vazia = cancelou). `false` = já há um seletor aberto (ou a
    /// thread não subiu) — o clique é ignorado, como hoje.
    pub(crate) fn ask(&self, ctx: &egui::Context, ask: Ask, then: Then) -> bool {
        if self.busy() {
            return false;
        }
        self.showing.store(true, Ordering::SeqCst);
        let showing = self.showing.clone();
        let done = self.done.clone();
        let ctx = ctx.clone(); // Arc interno: barato; a thread exige 'static
        std::thread::Builder::new()
            .name("file-picker".into())
            .spawn(move || {
                let paths = pollster::block_on(async {
                    match ask {
                        Ask::File(d) => d.pick_file().await.map(|h| vec![h.path().to_path_buf()]).unwrap_or_default(),
                        Ask::Files(d) => d.pick_files().await.map(|hs| hs.into_iter().map(|h| h.path().to_path_buf()).collect()).unwrap_or_default(),
                        Ask::Save(d) => d.save_file().await.map(|h| vec![h.path().to_path_buf()]).unwrap_or_default(),
                    }
                });
                done.lock().unwrap_or_else(PoisonError::into_inner).push(Picked { paths, then });
                showing.store(false, Ordering::SeqCst);
                ctx.request_repaint(); // acorda o frame que consome a resposta
            })
            .is_ok()
    }

    /// Escolhas prontas — o frame que consome limpa a fila. A closure recebe
    /// o app mutável (chamada tipo: `for p in self.pickers.take() { (p.then)(self, p.paths); }`).
    pub(crate) fn take(&self) -> Vec<Picked> {
        std::mem::take(&mut *self.done.lock().unwrap_or_else(PoisonError::into_inner))
    }
}
