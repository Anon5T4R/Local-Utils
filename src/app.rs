//! Janela principal do Local Utils: barra de topo (título, tema/idioma e
//! check de updates), fileira de abas do roadmap e rodapé de status. A aba
//! Clipboard (a âncora do app) desenha o histórico em `clipboard_ui.rs`; as
//! demais ainda são esqueleto e ganham conteúdo na sua versão do roadmap.

use std::sync::mpsc::Receiver;

use eframe::egui;
use utils_ui::config::{self, Config};
use utils_ui::i18n::{self, Key, Lang};
use utils_ui::theme;

use crate::autostart;
use crate::clipboard::{self, ClipEvent};
use crate::clipboard_ui::{self, ClipboardUiState};
use crate::history;
use crate::hotkey::{self, HotkeyEvent};
use crate::options::{self, OptionsState};
use crate::transfer;
use crate::tray::{self, TrayCmd};

pub const APP_ID: &str = "local-utils";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arquivo do histórico de clipboard: mesma pasta da config do app
/// (`%APPDATA%\local-utils` no Windows, `~/.config/local-utils` fora).
fn history_path() -> std::path::PathBuf {
    config::config_dir(APP_ID).join("history.json")
}

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
    /// Último sequence number processado (dedupe de eventos colados).
    last_seq: u32,
    /// Verificação e instalação de atualização (craft_update): check no
    /// arranque/pedido, prompt egui próprio e self-update em thread própria.
    updater: craft_update::updater::Updater,
    /// Histórico de clipboard (motor em `history.rs`): carregado do disco no
    /// arranque e regravado a cada cópia nova ou mutação da aba.
    history: history::History,
    /// Estado da aba Clipboard que sobrevive entre frames (busca, filtro, foco
    /// de teclado e cache das miniaturas).
    clip_ui: ClipboardUiState,
    /// Eventos do monitor de clipboard (`None` se o monitor não subiu).
    clip_rx: Option<Receiver<ClipEvent>>,
    /// Diálogo de opções (autostart, atalho, exclusões e o `.lutils`).
    options: OptionsState,
    /// Comandos da bandeja: drenados no início do frame, como o `clip_rx`.
    tray_rx: Receiver<TrayCmd>,
    /// Bandeja viva enquanto o app viver (o drop remove o ícone). `None` = sem
    /// bandeja (Linux, ou falha ao criar) — aí o X fecha de verdade.
    #[cfg(windows)]
    tray_icon: Option<tray::Tray>,
    /// Eventos do atalho global (registro e tecla pressionada).
    hotkey_rx: Option<Receiver<HotkeyEvent>>,
    /// Fechar de VERDADE (pedido do updater): sem isto o X só esconde.
    #[cfg(windows)]
    allow_close: bool,
}

impl UtilsApp {
    pub fn new(mut cfg: Config) -> Self {
        // Autostart: o registro é a fonte da verdade — o cache do config pode
        // estar velho se a chave Run foi mexida por fora.
        cfg.autostart_ok = autostart::enabled();

        let mut history = history::load(&history_path());

        // O que já está no clipboard quando o app abre entra no histórico
        // também (o listener só vê cópias a partir do registro — sem isto o
        // boot nasce "0 itens" mesmo com conteúdo na área de transferência;
        // pagou no primeiro QA visual).
        if let Some(entry) = clipboard::read_current() {
            history.push(entry);
            if let Err(e) = history::save(&history_path(), &history) {
                eprintln!("history: {e}");
            }
        }

        // Monitor em thread própria: só avisa "a área de transferência mudou"
        // (a leitura do conteúdo fica com a UI, ver `clipboard.rs`). A lista de
        // exclusão vem do config (editável nas opções, vale no próximo boot).
        let (tx, rx) = std::sync::mpsc::channel();
        let clip_rx = match clipboard::spawn_monitor(tx, cfg.exclude.clone()) {
            // O JoinHandle fica solto de propósito: soltar não mata a thread e
            // o processo termina com ela (o canal morre quando o app fecha).
            Ok(_handle) => Some(rx),
            Err(e) => {
                eprintln!("clipboard: {e}");
                None
            }
        };

        // Bandeja + atalho global. Os rótulos do menu saem no idioma do boot
        // (trocar idioma vale no próximo arranque, como tudo que é do SO).
        let (tray_icon, tray_rx) = tray::spawn(tray::Labels {
            tooltip: i18n::t(cfg.lang, Key::AppTitle).to_string(),
            open: i18n::t(cfg.lang, Key::TrayOpen).to_string(),
            quit: i18n::t(cfg.lang, Key::TrayQuit).to_string(),
        });
        let hotkey_rx = hotkey::spawn(cfg.hotkey.clone());

        Self {
            cfg,
            status: String::new(),
            tab: Tab::Clipboard,
            last_seq: 0,
            updater: craft_update::updater::Updater::new(&craft_update::suite::LOCALUTILS, CURRENT_VERSION),
            history,
            clip_ui: ClipboardUiState::default(),
            clip_rx,
            options: OptionsState::default(),
            tray_rx,
            #[cfg(windows)]
            tray_icon,
            hotkey_rx,
            #[cfg(windows)]
            allow_close: false,
        }
    }

    /// Drena a bandeja. "Sair" encerra DE VERDADE: salva o histórico e mata o
    /// processo (o `ViewportCommand::Close` passaria pelo X, que esconde).
    fn drain_tray(&mut self, ctx: &egui::Context) {
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::Show => ctx.request_repaint(),
                TrayCmd::Quit => {
                    // Gravação atômica antes de derrubar: o exit não roda Drop.
                    self.save_history();
                    std::process::exit(0);
                }
            }
        }
    }

    /// Drena o atalho global: registro (aviso de conflito no rodapé) e a tecla
    /// pressionada (mostrar/focar a janela).
    fn drain_hotkey(&mut self, ctx: &egui::Context) {
        let mut eventos = Vec::new();
        while let Some(event) = self.hotkey_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            eventos.push(event);
        }
        for event in eventos {
            match event {
                HotkeyEvent::Pressed => {
                    // Janela oculta já foi acordada pela thread do atalho
                    // (winctl); aqui só garantimos o frame seguinte.
                    #[cfg(windows)]
                    {
                        crate::winctl::show();
                    }
                    ctx.request_repaint();
                }
                HotkeyEvent::Conflict => self.status = format!("⚠ {}", i18n::t(self.cfg.lang, Key::HotkeyConflict)),
                HotkeyEvent::Invalid => self.status = format!("⚠ {}", i18n::t(self.cfg.lang, Key::HotkeyInvalid)),
                HotkeyEvent::Unavailable => self.status = format!("⚠ {}", i18n::t(self.cfg.lang, Key::HotkeyUnavailable)),
                HotkeyEvent::Registered => {}
            }
        }
    }

    /// Drena os eventos do monitor e grava o histórico quando entra cópia
    /// nova. Chamado no início do frame; a gravação é síncrona.
    fn drain_clipboard(&mut self) {
        let mut changed = false;
        while let Some(event) = self.clip_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match event {
                ClipEvent::Changed { seq } => {
                    // O monitor já deduplica por `seq`; conferir de novo aqui
                    // custa nada e evita reler o conteúdo se dois avisos
                    // colarem no mesmo frame.
                    if seq == self.last_seq {
                        continue;
                    }
                    self.last_seq = seq;
                    if let Some(entry) = clipboard::read_current() {
                        self.history.push(entry);
                        changed = true;
                    }
                }
                // Falha do monitor: reporta e segue (o rodapé já mostra o
                // status; a aba segue utilizável com o que está em memória).
                ClipEvent::Error(e) => eprintln!("clipboard: {e}"),
            }
        }
        // Sem cópia nova não há o que regravar.
        if changed {
            self.save_history();
        }
    }

    /// Grava o histórico no disco (tmp + rename). Ponto ÚNICO de gravação: o
    /// drain do monitor e a aba Clipboard (pin/remover/editar) passam por aqui.
    /// Erro vira status no rodapé em vez de sumir.
    fn save_history(&mut self) {
        if let Err(e) = history::save(&history_path(), &self.history) {
            self.status = format!("⚠ {e}");
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

    /// Abre o diálogo de opções com os rascunhos semeados do config atual.
    fn open_options(&mut self) {
        self.options.open(&self.cfg);
    }

    /// Aplica o que o diálogo de opções devolveu: autostart (na hora), config
    /// (no Salvar) e export/import do histórico.
    fn apply_options(&mut self, ctx: &egui::Context, opts: options::Outcome) {
        if let Some(on) = opts.autostart {
            match autostart::set(on) {
                Ok(()) => {
                    self.cfg.autostart_ok = on;
                    if let Err(e) = config::save(APP_ID, &self.cfg) {
                        eprintln!("config: {e}");
                    }
                }
                Err(e) => self.status = format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Key::AutostartFailed)),
            }
        }
        if opts.save {
            let (hotkey_spec, exclude) = self.options.saved();
            self.cfg.hotkey = hotkey_spec;
            self.cfg.exclude = exclude;
            match config::save(APP_ID, &self.cfg) {
                // Atalho e exclusões são lidos na subida (registro do atalho e
                // monitor do clipboard): avisa em vez de fingir que já valeu.
                Ok(()) => self.status = i18n::t(self.cfg.lang, Key::RestartApplies).to_string(),
                Err(e) => self.status = format!("⚠ {e}"),
            }
        }
        if opts.export {
            if let Some(path) = transfer::pick_export(&transfer::export_name()) {
                self.status = match transfer::export_to(&path, &self.history) {
                    Ok(()) => i18n::t(self.cfg.lang, Key::Exported).to_string(),
                    Err(e) => format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Key::ExportFailed)),
                };
            }
        }
        if opts.import {
            if let Some(path) = transfer::pick_open() {
                self.status = match transfer::import_into(&path, &mut self.history) {
                    Ok(()) => {
                        self.save_history();
                        // O import pode mexer na lista sem mudar o tamanho: o
                        // cache da lista filtrada precisa ser descartado.
                        self.clip_ui.invalidate();
                        ctx.request_repaint();
                        i18n::t(self.cfg.lang, Key::Imported).to_string()
                    }
                    Err(e) => format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Key::ImportFailed)),
                };
            }
        }
    }
}

impl eframe::App for UtilsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();

        // Janela residente na bandeja: o X ESCONDE em vez de sair. Exceções: o
        // updater pediu fechamento de verdade (`allow_close`) ou não há
        // bandeja (Linux/falha) — aí o X segue o caminho normal do winit.
        // Descobrir o HWND já no 1º frame: a thread da bandeja precisa dele
        // pra acordar a janela oculta (ShowWindow direto, ver `winctl.rs`).
        #[cfg(windows)]
        {
            crate::winctl::discover();
            if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.tray_icon.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::winctl::hide();
                return; // não processa o resto do frame
            }
        }

        // Bandeja e atalho: canais drenados no início do frame (mesmo padrão do
        // `clip_rx`). "Sair" da bandeja encerra o processo daqui, depois de
        // salvar o histórico.
        self.drain_tray(ctx);
        self.drain_hotkey(ctx);

        // Clipboard: drena as cópias que chegaram desde o último frame. O
        // monitor não tem como acordar a UI (não recebe o `Context`), então
        // pedimos um repaint periódico — o lote 3 troca por repaint disparado
        // no próprio evento.
        if self.clip_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        self.drain_clipboard();

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
                    // Engrenagem: opções do app (autostart, atalho, exclusões,
                    // export/import).
                    let gear = egui::Button::new(egui::RichText::new("⚙").weak()).frame(false);
                    if ui.add(gear).on_hover_text(i18n::t(self.cfg.lang, Key::Options)).clicked() {
                        self.open_options();
                    }
                });
            });
        });

        egui::containers::Panel::bottom("rodape").show(ui, |ui| {
            ui.horizontal(|ui| {
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).small().weak());
                }
                // Contagem do histórico no rodapé. O texto vem do catálogo com
                // `{n}` preenchido à mão — plural real entra quando existir a
                // chave plural no catálogo.
                // Singular/plural na mão (sem plumbing de plural do i18n
                // ainda): 1 item, 2 itens.
                let key = if self.history.len() == 1 { Key::ItemOne } else { Key::ItemsCount };
                let count = i18n::t(self.cfg.lang, key).replace("{n}", &self.history.len().to_string());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(count).small().weak());
                });
            });
        });

        let mut outcome = clipboard_ui::Outcome::default();
        egui::CentralPanel::default().show(ui, |ui| {
            // Fileira de abas: `selectable_value` marca a ativa e troca o
            // painel na hora (sem estado extra).
            ui.horizontal(|ui| {
                for tab in Tab::ALL {
                    ui.selectable_value(&mut self.tab, tab, tab.label(self.cfg.lang));
                }
            });
            ui.separator();
            match self.tab {
                // Clipboard é a âncora: lista virtualizada do histórico.
                Tab::Clipboard => {
                    outcome = clipboard_ui::show(ui, self.cfg.lang, &mut self.history, &mut self.clip_ui);
                }
                // As demais abas ainda são esqueleto: aviso traduzido no centro.
                _ => {
                    let _ = ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new(i18n::t(self.cfg.lang, Key::TabWip)).weak());
                    });
                }
            }
        });
        // Mutação da aba (pin/remover/editar) → regrava o histórico; a ação
        // também pode ter deixado uma mensagem pro rodapé.
        if outcome.changed {
            self.save_history();
        }
        if let Some(status) = outcome.status {
            self.status = status;
            // O rodapé já foi desenhado neste frame: sem o pedido de repaint o
            // "copiado ✓" só apareceria no próximo evento do usuário.
            ctx.request_repaint();
        }

        // Opções (engrenagem): por cima do painel central e abaixo do prompt de
        // update. O app aplica o que o diálogo devolveu.
        let opts = options::show(ctx, self.cfg.lang, &mut self.options);
        self.apply_options(ctx, opts);

        // Prompt de update (craft_update): por cima de tudo. "Atualizar e
        // reiniciar" pede o fechamento pelo caminho limpo (ViewportCommand::
        // Close) — o helper do craft_update instala e relança depois que o
        // app sai.
        match self.updater.show(ctx) {
            craft_update::updater::Action::None => {}
            craft_update::updater::Action::OpenUrl(url) => crate::open_url(&url),
            craft_update::updater::Action::Close => {
                // Fechar de VERDADE: sem isto o X esconderia pra bandeja e o
                // updater nunca instalaria (ele espera o processo sair).
                #[cfg(windows)]
                {
                    self.allow_close = true;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
