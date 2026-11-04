//! Janela principal do Local Utils: barra de topo (só as ações da direita —
//! um tema/idioma, opções e o check de updates; o título é o da titlebar do
//! SO), fileira de abas do roadmap e rodapé de status. A aba Clipboard (a
//! âncora do app) desenha o histórico em `clipboard_ui.rs`; a aba Screenshot
//! edita a captura pendente (`shot_editor.rs`); as demais ainda são esqueleto e
//! ganham conteúdo na sua versão do roadmap.

use std::sync::mpsc::Receiver;

use eframe::egui;
use utils_ui::config::{self, Config};
use utils_ui::i18n::{self, Key, Lang};
use utils_ui::theme;

use crate::autostart;
use crate::capture::{self, Capture};
use crate::clipboard::{self, ClipEvent};
use crate::clipboard_ui::{self, ClipboardUiState};
use crate::history;
use crate::hotkey::{self, HotkeyEvent};
use crate::ocr::{self, Ocr};
use crate::options::{self, OptionsState};
use crate::popup::{self, Popup};
use crate::shot_editor::{self, Shot};
use crate::transfer;
use crate::tray::{self, TrayCmd};
use crate::ui as kit;

pub const APP_ID: &str = "local-utils";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Quanto tempo um status de ação fica no rodapé ("Copiado ✓", "Desafixado"…).
/// Sem isso a última mensagem ficava presa pra sempre (QA viu o "Desafixado"
/// sobrevivendo a várias ações depois).
const STATUS_TTL_MS: u64 = 4_000;

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
    /// Instante (epoch ms) em que o status atual entrou. Status de ação some
    /// sozinho depois de [`STATUS_TTL_MS`] — sem timer extra: a expiração é
    /// uma comparação feita no frame.
    status_since: u64,
    tab: Tab,
    /// Último sequence number processado (dedupe de eventos colados).
    last_seq: u32,
    /// Resultado de check de update manual ("Atualizado ✓ (versão)" / erro):
    /// modal com OK — feedback que não passa despercebido no rodapé.
    update_note: Option<String>,
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
    /// Popup rápido do atalho (viewport próprio, no cursor).
    popup: Popup,
    /// Overlay de seleção da captura de tela (viewport próprio, fullscreen).
    capture: Capture,
    /// Captura pendente no editor da aba Screenshot (v0.3 guarda UMA: uma
    /// captura nova substitui a anterior).
    shot: Option<Shot>,
    /// Colagem pendente do popup: o Ctrl+V roda no frame SEGUINTE, depois que
    /// o viewport já sumiu — o popup não pode estar na frente da janela que vai
    /// receber as teclas.
    popup_paste: Option<u128>,
    /// OCR das imagens: thread de leitura + cache do último resultado.
    ocr: Ocr,
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
        // No Linux o handle do ícone não existe (stub) — só o canal importa.
        #[cfg_attr(not(windows), allow(unused_variables))]
        let (tray_icon, tray_rx) = tray::spawn(tray::Labels {
            tooltip: i18n::t(cfg.lang, Key::AppTitle).to_string(),
            open: i18n::t(cfg.lang, Key::TrayOpen).to_string(),
            quit: i18n::t(cfg.lang, Key::TrayQuit).to_string(),
        });
        let hotkey_rx = hotkey::spawn(cfg.hotkey.clone(), cfg.hotkey_screenshot.clone());

        Self {
            cfg,
            status: String::new(),
            status_since: 0,
            tab: Tab::Clipboard,
            last_seq: 0,
            update_note: None,
            updater: craft_update::updater::Updater::new(&craft_update::suite::LOCALUTILS, CURRENT_VERSION),
            history,
            clip_ui: ClipboardUiState::default(),
            clip_rx,
            options: OptionsState::default(),
            tray_rx,
            #[cfg(windows)]
            tray_icon,
            hotkey_rx,
            popup: Popup::default(),
            capture: Capture::default(),
            shot: None,
            popup_paste: None,
            ocr: Ocr::default(),
            #[cfg(windows)]
            allow_close: false,
        }
    }

    /// Publica uma mensagem no rodapé e carimba o instante dela: o status de
    /// ação expira sozinho depois de [`STATUS_TTL_MS`]. Ponto ÚNICO de escrita
    /// do `status` — atribuir direto furaria a expiração.
    fn set_status(&mut self, message: String) {
        self.status = message;
        self.status_since = history::now_ms();
    }

    /// Tira do rodapé o status vencido e pede o frame em que ele some (sem
    /// timer: a conta é refeita a cada frame).
    fn expire_status(&mut self, ctx: &egui::Context) {
        if self.status.is_empty() {
            return;
        }
        let idade = history::now_ms().saturating_sub(self.status_since);
        if idade >= STATUS_TTL_MS {
            self.status.clear();
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(STATUS_TTL_MS - idade));
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
    /// pressionada. O atalho abre o popup no cursor (`popup_instead_of_window`,
    /// o padrão) ou a janela inteira (quem desmarcou nas opções).
    fn drain_hotkey(&mut self, ctx: &egui::Context) {
        let mut eventos = Vec::new();
        while let Some(event) = self.hotkey_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            eventos.push(event);
        }
        for event in eventos {
            match event {
                HotkeyEvent::Pressed => {
                    if self.cfg.popup_instead_of_window {
                        // Janela de destino capturada ANTES de o popup abrir:
                        // depois disso o primeiro plano é o próprio popup.
                        #[cfg(windows)]
                        {
                            crate::paste::remember_foreground();
                        }
                        self.popup.open(ctx);
                    } else {
                        // Janela oculta: o SO precisa mostrar de novo (o frame
                        // sozinho não pinta a janela escondida).
                        #[cfg(windows)]
                        {
                            crate::winctl::show();
                        }
                    }
                    ctx.request_repaint();
                }
                HotkeyEvent::PressedShot => self.start_capture(ctx),
                HotkeyEvent::Conflict => self.set_status(format!("⚠ {}", i18n::t(self.cfg.lang, Key::HotkeyConflict))),
                HotkeyEvent::Invalid => self.set_status(format!("⚠ {}", i18n::t(self.cfg.lang, Key::HotkeyInvalid))),
                HotkeyEvent::Unavailable => self.set_status(format!("⚠ {}", i18n::t(self.cfg.lang, Key::HotkeyUnavailable))),
                HotkeyEvent::Registered => {}
            }
        }
    }

    /// Abre a captura de tela (atalho de captura): congela o desktop e sobe o
    /// overlay de seleção. Falha vira status no rodapé — no Linux a captura é
    /// stub e o aviso sai traduzido.
    fn start_capture(&mut self, ctx: &egui::Context) {
        if let Err(e) = self.capture.begin(ctx) {
            self.set_status(format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Key::CaptureFailed)));
        }
        ctx.request_repaint();
    }

    /// Recopia a entrada e cola na janela que estava em foco antes do popup
    /// (fluxo do popup: Enter ou clique). Na lista da janela principal nada
    /// disto roda — lá recopiar é só recopiar.
    fn paste_entry(&mut self, id: u128) {
        let lang = self.cfg.lang;
        // Resultado já dono: o empréstimo do histórico morre aqui e o
        // `set_status` pode mexer no app.
        let copied = self.history.entries().iter().find(|e| e.id == id).map(clipboard::write_back);
        match copied {
            // A entrada saiu da lista entre o Enter e a colagem.
            None => {}
            Some(Err(e)) => self.set_status(format!("⚠ {}: {e}", i18n::t(lang, Key::CopyFailed))),
            Some(Ok(())) => {
                #[cfg(windows)]
                {
                    if let Err(e) = crate::paste::paste_into_focused() {
                        self.set_status(format!("⚠ {}: {e}", i18n::t(lang, Key::PasteFailed)));
                    }
                }
            }
        }
    }

    /// Pede a OCR de uma imagem do histórico (botão da linha).
    fn start_ocr(&mut self, id: u128, ctx: &egui::Context) {
        let lang = self.cfg.lang;
        let outcome = {
            // Empréstimos de campos separados: o pedido lê o histórico e mexe
            // no estado do OCR ao mesmo tempo.
            let Some(entry) = self.history.entries().iter().find(|e| e.id == id) else { return };
            self.ocr.request(entry)
        };
        match outcome {
            // Cache: o texto já está pronto, o modal abre na hora.
            Ok(Some(text)) => self.clip_ui.show_ocr(id, text),
            // Thread rodando: o resultado entra pelo `drain_ocr`.
            Ok(None) => self.set_status(i18n::t(lang, Key::OcrReading).to_string()),
            Err(e) => self.set_status(format!("⚠ {}: {e}", i18n::t(lang, Key::OcrFailed))),
        }
        ctx.request_repaint();
    }

    /// Drena o resultado da OCR. Texto vazio não abre modal: avisa no rodapé.
    fn drain_ocr(&mut self, ctx: &egui::Context) {
        let lang = self.cfg.lang;
        while let Some(event) = self.ocr.poll() {
            match event {
                ocr::OcrEvent::Done { text, .. } if text.is_empty() => {
                    self.set_status(format!("⚠ {}", i18n::t(lang, Key::OcrEmpty)));
                }
                ocr::OcrEvent::Done { id, text } => self.clip_ui.show_ocr(id, text),
                ocr::OcrEvent::Failed { error } => {
                    eprintln!("ocr: {error}");
                    self.set_status(format!("⚠ {}: {error}", i18n::t(lang, Key::OcrFailed)));
                }
            }
        }
        // O canal não acorda a UI (mesmo motivo do `clip_rx`): enquanto a
        // leitura roda, insistimos no repaint.
        if self.ocr.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
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
            self.set_status(format!("⚠ {e}"));
        }
    }

    /// Check de update no arranque (chamado antes do primeiro frame): achou
    /// release nova, o prompt do updater abre sozinho.
    pub fn check_at_start(&mut self, ctx: &egui::Context) {
        self.updater.check_at_start(ctx);
    }

    /// Verifica atualizações agora (o ↻ da barra de topo; o tooltip diz o que
    /// ele faz). O resultado sai na barra de status do rodapé; release nova
    /// abre o prompt do próprio updater.
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
                Err(e) => self.set_status(format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Key::AutostartFailed))),
            }
        }
        if opts.save {
            let (hotkey_spec, exclude) = self.options.saved();
            self.cfg.hotkey = hotkey_spec;
            self.cfg.exclude = exclude;
            match config::save(APP_ID, &self.cfg) {
                // Atalho e exclusões são lidos na subida (registro do atalho e
                // monitor do clipboard): avisa em vez de fingir que já valeu.
                Ok(()) => self.set_status(i18n::t(self.cfg.lang, Key::RestartApplies).to_string()),
                Err(e) => self.set_status(format!("⚠ {e}")),
            }
        }
        // Popup no cursor vale NA HORA (nada do SO é relido): só persiste.
        if let Some(on) = opts.popup_mode {
            self.cfg.popup_instead_of_window = on;
            if let Err(e) = config::save(APP_ID, &self.cfg) {
                eprintln!("config: {e}");
            }
        }
        if opts.export {
            if let Some(path) = transfer::pick_export(&transfer::export_name()) {
                self.set_status(match transfer::export_to(&path, &self.history) {
                    Ok(()) => i18n::t(self.cfg.lang, Key::Exported).to_string(),
                    Err(e) => format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Key::ExportFailed)),
                });
            }
        }
        if opts.import {
            if let Some(path) = transfer::pick_open() {
                // Mensagem primeiro, efeitos depois: `set_status` e os
                // efeitos do Ok não podem disputar o borrow do `import_into`.
                let lang = self.cfg.lang;
                let outcome = transfer::import_into(&path, &mut self.history);
                match outcome {
                    Ok(()) => {
                        self.save_history();
                        // O import pode mexer na lista sem mudar o tamanho: o
                        // cache da lista filtrada precisa ser descartado.
                        self.clip_ui.invalidate();
                        ctx.request_repaint();
                        self.set_status(i18n::t(lang, Key::Imported).to_string());
                    }
                    Err(e) => self.set_status(format!("⚠ {}: {e}", i18n::t(lang, Key::ImportFailed))),
                }
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
            // Context guardado pra thread do atalho poder acordar a UI sem
            // mostrar a janela principal (o popup abre com ela escondida).
            crate::winctl::install_repaint(ctx);
            if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.tray_icon.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::winctl::hide();
                return; // não processa o resto do frame
            }
        }

        // Colagem pendente do popup: roda AQUI, no começo do frame — o viewport
        // do popup já não foi mostrado no frame anterior, então o primeiro plano
        // é livre pra ir pra janela de destino antes do Ctrl+V.
        if let Some(id) = self.popup_paste.take() {
            self.paste_entry(id);
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
        // OCR: resultado da thread de leitura (modal com o texto).
        self.drain_ocr(ctx);

        // Updater (craft_update): drena o check/instalação pendente (o tick
        // acorda a UI por si). Resultado de um check PEDIDO sai num MODAL
        // (piscar 4s no rodapé era fácil de perder — "o botão não fez nada");
        // release nova abre o prompt do updater.
        self.updater.tick(ctx);
        match self.updater.status().clone() {
            craft_update::updater::Status::UpToDate => {
                self.update_note = Some(format!("{} ({})", i18n::t(self.cfg.lang, Key::UpToDate), CURRENT_VERSION));
                self.updater.dismiss_status();
            }
            craft_update::updater::Status::Failed(e) => {
                self.update_note = Some(format!("⚠ {e}"));
                self.updater.dismiss_status();
            }
            _ => {}
        }
        // Status de ação é transitório: some ~4s depois de entrar (o "Desafixado"
        // ficava preso no rodapé até a próxima mensagem).
        self.expire_status(ctx);

        // Barra de topo: a titlebar do SO já mostra "Local Utils" — aqui ficam
        // só as ações da direita (⚙ opções, ↻ updates e os combos de
        // idioma/tema), numa faixa mais baixa que o padrão do painel.
        let topo_frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 3));
        egui::containers::Panel::top("topo").frame(topo_frame).show(ui, |ui| {
            // Direita→esquerda: a PRIMEIRA coisa adicionada encosta na borda
            // direita. Engrenagem e ↻ são ícones ghost (mesmo tamanho do kit);
            // os combos de configuração vêm depois, à esquerda deles.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Engrenagem: opções do app (autostart, atalho, exclusões,
                    // export/import).
                    if kit::ghost(ui, "⚙").on_hover_text(i18n::t(self.cfg.lang, Key::Options)).clicked() {
                        self.open_options();
                    }
                    // ↻ ghost: o check de update é ação secundária e o texto longo
                    // ("Verificar atualizações") pesava na faixa — o tooltip diz o
                    // que o ícone faz.
                    if kit::ghost(ui, "↻").on_hover_text(i18n::t(self.cfg.lang, Key::CheckUpdates)).clicked() {
                        self.check_updates(ctx);
                    }
                    let (cfg_changed, _) = utils_ui::settings_ui(ui, &mut self.cfg, false);
                    if cfg_changed {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                });
            });
        });

        egui::containers::Panel::bottom("rodape").show(ui, |ui| {
            ui.horizontal(|ui| {
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).small().weak());
                }
                // Contagem do histórico no rodapé. Singular/plural na mão (o
                // catálogo não tem forma plural ainda): 1 item, 2 itens — as
                // linhas `ItemOne`/`ItemsCount` do TSV são cobertas por teste
                // em `utils-ui` (o QA já viu "4 items" em inglês).
                let key = if self.history.len() == 1 { Key::ItemOne } else { Key::ItemsCount };
                let count = i18n::t(self.cfg.lang, key).replace("{n}", &self.history.len().to_string());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(count).small().weak());
                });
            });
        });

        let mut outcome = clipboard_ui::Outcome::default();
        let mut shot_status: Option<String> = None;
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
                // Screenshot: editor da captura pendente (ou a dica do atalho).
                Tab::Screenshot => {
                    let shot = shot_editor::show(ui, self.cfg.lang, &self.cfg.hotkey_screenshot, self.shot.as_mut());
                    shot_status = shot.status;
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
            self.set_status(status);
            // O rodapé já foi desenhado neste frame: sem o pedido de repaint o
            // "copiado ✓" só apareceria no próximo evento do usuário.
            ctx.request_repaint();
        }
        // Status do editor da captura (copiar/salvar). Vazio = diálogo
        // cancelado, nada pro rodapé.
        if let Some(status) = shot_status.filter(|s| !s.is_empty()) {
            self.set_status(status);
            ctx.request_repaint();
        }
        // OCR pedido pela linha da imagem: sobe a thread (o resultado entra no
        // `drain_ocr` dos próximos frames).
        if let Some(id) = outcome.ocr {
            self.start_ocr(id, ctx);
        }

        // Popup rápido: retrato da lista + viewport no cursor. O comando que ele
        // deixar (Enter/clique) é aplicado no frame seguinte, com o viewport já
        // fora da frente (ver `popup_paste` no topo do frame). O `take_cmd` vem
        // ANTES do `is_open`: quando o popup escolhe um item ele já se fecha no
        // próprio frame, e o comando não pode ficar preso.
        if let Some(popup::Cmd::Paste(id)) = self.popup.take_cmd() {
            self.popup.close();
            self.popup_paste = Some(id);
            ctx.request_repaint();
        }
        if self.popup.is_open() {
            self.popup.sync(&self.history);
            popup::show(ctx, self.cfg.lang, &self.popup);
        }

        // Captura de tela: o overlay de seleção é um viewport próprio (como o
        // popup). Confirmou? A aba Screenshot abre com o recorte no editor.
        if self.capture.is_open() {
            capture::show(ctx, self.cfg.lang, &self.cfg.hotkey_screenshot, &self.capture);
        }
        if let Some(image) = self.capture.take_result() {
            self.shot = Some(Shot::new(image));
            self.tab = Tab::Screenshot;
            ctx.request_repaint();
        }

        // Opções (engrenagem): por cima do painel central e abaixo do prompt de
        // update. O app aplica o que o diálogo devolveu.
        let opts = options::show(ctx, self.cfg.lang, &mut self.options);
        self.apply_options(ctx, opts);

        // Nota do check manual (Atualizado ✓ / falha): modal simples com OK.
        if let Some(note) = self.update_note.clone() {
            let mut close = false;
            egui::Window::new(i18n::t(self.cfg.lang, Key::CheckUpdates))
                .id(egui::Id::unique("update-note"))
                .frame(crate::ui::modal_frame(ctx))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .default_width(340.0)
                .show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label(&note);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if crate::ui::pill(ui, i18n::t(self.cfg.lang, Key::Close), false).clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.update_note = None;
            }
        }

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
