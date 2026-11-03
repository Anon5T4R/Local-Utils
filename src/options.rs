//! Diálogo de opções do app (a engrenagem da barra de topo): autostart, atalho
//! global, lista de apps sensíveis e export/import do histórico (`.lutils`).
//!
//! Os rascunhos ficam aqui e só viram config no **Salvar** — fechar o diálogo
//! (X, Esc, clique fora) descarta as mudanças de texto/lista. Exceção: o
//! autostart é um toggle do SO e vale na hora (o app grava a chave Run e
//! devolve o cache no config).

use eframe::egui;
use utils_ui::config::{Config, DEFAULT_HOTKEY};
use utils_ui::i18n::{self, Key, Lang};

use crate::ui as kit;

/// Estado do diálogo (vive no app entre frames).
#[derive(Default)]
pub struct OptionsState {
    open: bool,
    /// Rascunho do atalho global.
    hotkey: String,
    /// Rascunho da lista de apps sensíveis.
    exclude: Vec<String>,
    /// Campo "novo app" (o texto que ainda não entrou na lista).
    new_exclude: String,
    /// Rascunho do autostart (espelha o registro até o próximo toggle).
    autostart: bool,
}

/// O que o app precisa aplicar depois de desenhar o diálogo.
#[derive(Default)]
pub struct Outcome {
    /// Salvar: aplicar os rascunhos no config e persistir.
    pub save: bool,
    /// O autostart mudou: gravar a chave Run agora.
    pub autostart: Option<bool>,
    pub export: bool,
    pub import: bool,
}

impl OptionsState {
    /// Abre o diálogo com os rascunhos semeados do config atual.
    pub fn open(&mut self, cfg: &Config) {
        self.open = true;
        self.hotkey = cfg.hotkey.clone();
        self.exclude = cfg.exclude.clone();
        self.new_exclude.clear();
        self.autostart = cfg.autostart_ok;
    }

    /// Valores prontos pro config no Salvar: atalho aparado (vazio volta ao
    /// padrão) e lista normalizada — minúsculas, sem vazios e sem repetição.
    pub fn saved(&self) -> (String, Vec<String>) {
        let hotkey = self.hotkey.trim();
        let hotkey = if hotkey.is_empty() { DEFAULT_HOTKEY.to_string() } else { hotkey.to_string() };
        let mut exclude: Vec<String> = Vec::new();
        for app in &self.exclude {
            let name = app.trim().to_ascii_lowercase();
            if !name.is_empty() && !exclude.contains(&name) {
                exclude.push(name);
            }
        }
        (hotkey, exclude)
    }
}

/// Desenha o diálogo (se aberto) e devolve o que aplicar.
pub fn show(ctx: &egui::Context, lang: Lang, state: &mut OptionsState) -> Outcome {
    let mut out = Outcome::default();
    if !state.open {
        return out;
    }
    let windows = cfg!(windows);
    let t = |key: Key| i18n::t(lang, key);
    let mut save = false;
    let mut close = false;

    let resp = egui::Modal::new(egui::Id::unique("utils-options")).frame(kit::modal_frame(ctx)).show(ctx, |ui| {
        kit::begin_dialog(ui);
        ui.set_width(520.0);
        ui.heading(t(Key::Options));
        ui.add_space(12.0);

        // ── autostart (vale na hora) ──────────────────────────────────
        let mut autostart = state.autostart;
        let check = ui.add_enabled(windows, egui::Checkbox::new(&mut autostart, t(Key::Autostart)));
        if windows {
            if check.changed() {
                state.autostart = autostart;
                out.autostart = Some(autostart);
            }
        } else {
            check.on_hover_text(t(Key::WindowsOnly));
        }
        ui.add_space(10.0);

        // ── atalho global (vale no próximo arranque) ──────────────────
        ui.horizontal(|ui| {
            ui.label(t(Key::HotkeyGlobal));
            let field = ui.add_enabled(windows, kit::line(&mut state.hotkey).hint_text(DEFAULT_HOTKEY).desired_width(180.0));
            let tip = if windows { Key::RestartApplies } else { Key::WindowsOnly };
            field.on_hover_text(t(tip));
        });
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // ── apps sensíveis (vale no próximo arranque) ─────────────────
        ui.label(egui::RichText::new(t(Key::ExcludeApps)).strong());
        ui.label(egui::RichText::new(t(Key::RestartApplies)).small().weak());
        ui.add_space(6.0);
        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("utils-options-exclude").max_height(150.0).show(ui, |ui| {
            for (i, app) in state.exclude.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(kit::line(app).desired_width(360.0));
                    if kit::ghost(ui, "🗑").on_hover_text(t(Key::Remove)).clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some(i) = remove {
            state.exclude.remove(i);
        }
        ui.horizontal(|ui| {
            ui.add(kit::line(&mut state.new_exclude).hint_text(t(Key::ExcludeHint)).desired_width(360.0));
            if kit::pill(ui, t(Key::ExcludeAdd), false).clicked() {
                let name = state.new_exclude.trim().to_ascii_lowercase();
                if !name.is_empty() && !state.exclude.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
                    state.exclude.push(name);
                    state.new_exclude.clear();
                }
            }
        });
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // ── export/import do histórico ────────────────────────────────
        ui.horizontal(|ui| {
            let export = ui.add_enabled_ui(windows, |ui| kit::pill(ui, t(Key::ExportHistory), false)).inner;
            let import = ui.add_enabled_ui(windows, |ui| kit::pill(ui, t(Key::ImportHistory), false)).inner;
            if export.clicked() {
                out.export = true;
            }
            if import.clicked() {
                out.import = true;
            }
            if !windows {
                export.on_hover_text(t(Key::WindowsOnly));
                import.on_hover_text(t(Key::WindowsOnly));
            }
        });

        // ── rodapé ───────────────────────────────────────────────────
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::pill(ui, t(Key::Save), true).clicked() {
                    save = true;
                }
                if kit::pill(ui, t(Key::Close), false).clicked() {
                    close = true;
                }
            });
        });
    });

    if save {
        out.save = true;
        state.open = false;
    } else if close || resp.should_close() {
        state.open = false;
    }
    out
}
