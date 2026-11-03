//! Janela principal do Local Utils: barra de topo (título, tema/idioma e
//! check de updates), fileira de abas do roadmap e rodapé de status. As abas
//! nascem como esqueleto — cada uma ganha conteúdo na sua versão do roadmap.

use eframe::egui;
use utils_ui::config::{self, Config};
use utils_ui::i18n::{self, Key, Lang};
use utils_ui::theme;

pub const APP_ID: &str = "local-utils";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Aba aberta no painel central. Clipboard é a âncora (a home do app).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Clipboard,
    Screenshot,
    Sistema,
    Tools,
    Scratchpad,
}

impl Tab {
    /// Todas as abas, na ordem do roadmap.
    const ALL: [Tab; 5] = [Tab::Clipboard, Tab::Screenshot, Tab::Sistema, Tab::Tools, Tab::Scratchpad];

    fn label(self, lang: Lang) -> &'static str {
        let key = match self {
            Tab::Clipboard => Key::TabClipboard,
            Tab::Screenshot => Key::TabScreenshot,
            Tab::Sistema => Key::TabSystem,
            Tab::Tools => Key::TabTools,
            Tab::Scratchpad => Key::TabScratchpad,
        };
        i18n::t(lang, key)
    }
}

pub struct UtilsApp {
    cfg: Config,
    /// Mensagem da barra de status (vazia = nada a mostrar).
    status: String,
    tab: Tab,
    /// Verificação e instalação de atualização (craft_update): check no
    /// arranque/pedido, prompt egui próprio e self-update em thread própria.
    updater: craft_update::updater::Updater,
}

impl UtilsApp {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            status: String::new(),
            tab: Tab::Clipboard,
            updater: craft_update::updater::Updater::new(&craft_update::suite::LOCALUTILS, CURRENT_VERSION),
        }
    }

    /// Check de update no arranque (chamado antes do primeiro frame): achou
    /// release nova, o prompt do updater abre sozinho.
    pub fn check_at_start(&mut self, ctx: &egui::Context) {
        self.updater.check_at_start(ctx);
    }

    /// Verifica atualizações agora (botão "Verificar atualizações"). O
    /// resultado sai na barra de status do rodapé; release nova abre o
    /// prompt do próprio updater.
    fn check_updates(&mut self, ctx: &egui::Context) {
        self.updater.check_now(ctx);
    }
}

impl eframe::App for UtilsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();

        // Updater (craft_update): drena o check/instalação pendente (o tick
        // acorda a UI por si). Resultado de um check PEDIDO sai na barra de
        // status do rodapé; release nova abre o prompt do updater.
        self.updater.tick(ctx);
        match self.updater.status().clone() {
            craft_update::updater::Status::UpToDate => {
                self.status = i18n::t(self.cfg.lang, Key::UpToDate).to_string();
                self.updater.dismiss_status();
            }
            craft_update::updater::Status::Failed(e) => {
                self.status = format!("⚠ {e}");
                self.updater.dismiss_status();
            }
            _ => {}
        }

        egui::containers::Panel::top("topo").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(i18n::t(self.cfg.lang, Key::AppTitle));
                // Direita→esquerda: configurações e o check de update (o
                // mesmo par da toolbar dos irmãos da suíte).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (cfg_changed, _) = utils_ui::settings_ui(ui, &mut self.cfg, false);
                    if cfg_changed {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                    // Ghost button: o check de update é ação secundária, não
                    // pede o peso visual de um botão com moldura.
                    let check = egui::Button::new(egui::RichText::new(i18n::t(self.cfg.lang, Key::CheckUpdates)).weak()).frame(false);
                    if ui.add(check).clicked() {
                        self.check_updates(ctx);
                    }
                });
            });
        });

        egui::containers::Panel::bottom("rodape").show(ui, |ui| {
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).small().weak());
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            // Fileira de abas: `selectable_value` marca a ativa e troca o
            // painel na hora (sem estado extra).
            ui.horizontal(|ui| {
                for tab in Tab::ALL {
                    ui.selectable_value(&mut self.tab, tab, tab.label(self.cfg.lang));
                }
            });
            ui.separator();
            // Todas as abas ainda são esqueleto: aviso traduzido no centro.
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::TabWip)).weak());
            });
        });

        // Prompt de update (craft_update): por cima de tudo. "Atualizar e
        // reiniciar" pede o fechamento pelo caminho limpo (ViewportCommand::
        // Close) — o helper do craft_update instala e relança depois que o
        // app sai.
        match self.updater.show(ctx) {
            craft_update::updater::Action::None => {}
            craft_update::updater::Action::OpenUrl(url) => crate::open_url(&url),
            craft_update::updater::Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}
