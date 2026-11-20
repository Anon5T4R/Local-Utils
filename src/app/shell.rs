//! A casca da suíte no Utils: os botões da barra de cima (engrenagem, tema, lupa), as Configurações (as opções
//! do app com idioma, tema e Sobre) e a paleta de comandos. Os comandos rodam o mesmo código dos botões das abas.

use craft_icons::{Icon, ThemeMode};
use craft_palette::Command;
use craft_update::updater::Status;
use egui::{Key, KeyboardShortcut, Modifiers};
use utils_ui::i18n::{self, Key as Text, Lang};
use utils_ui::theme::{self, Appearance};

use super::{options, Tab, UtilsApp};
use crate::tools::SubTab;

/// O modo da suíte que corresponde à preferência salva (os ids são os mesmos: "system", "light", "dark").
pub(crate) fn mode_of(appearance: Appearance) -> ThemeMode {
    ThemeMode::from_id(appearance.code())
}

/// A preferência salva de um modo.
pub(crate) fn appearance_of(mode: ThemeMode) -> Appearance {
    Appearance::from_code(mode.id()).unwrap_or_default()
}

fn shortcut_text(ctx: &egui::Context) -> String {
    ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K))
}

/// Id estável de cada aba na paleta.
fn tab_id(tab: Tab) -> &'static str {
    match tab {
        Tab::Clipboard => "clipboard",
        Tab::Screenshot => "screenshot",
        Tab::Sistema => "system",
        Tab::Tools => "tools",
        Tab::Scratchpad => "notes",
        Tab::Cor => "color",
        Tab::Converter => "convert",
    }
}

/// Id estável de cada sub-aba de Ferramentas na paleta.
fn sub_id(sub: SubTab) -> &'static str {
    match sub {
        SubTab::Clock => "clock",
        SubTab::Calc => "calc",
        SubTab::Units => "units",
        SubTab::Dev => "dev",
    }
}

impl UtilsApp {
    /// Abre as Configurações (a engrenagem e a paleta), com os rascunhos semeados do config atual.
    pub(crate) fn open_settings(&mut self) {
        if !self.options.is_open() {
            self.options.open(&self.cfg);
        }
    }

    /// Troca o modo de tema: aplica e grava.
    pub(crate) fn set_mode(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.cfg.theme = appearance_of(mode);
        theme::apply(ctx, self.cfg.theme);
        self.save_config();
    }

    fn set_lang(&mut self, lang: Lang) {
        self.cfg.lang = lang;
        self.save_config();
    }

    /// Os controles da suíte à direita da barra de cima, num layout da direita para a esquerda: engrenagem
    /// (mais à direita), tema e lupa.
    pub(crate) fn shell_buttons(&mut self, ui: &mut egui::Ui) {
        let lang = self.cfg.lang;
        // Longe da borda da janela, e um dedo de distância entre os botões.
        ui.add_space(8.0);
        if craft_icons::ui::button(ui, Icon::Settings, i18n::tr(lang, "Settings")).clicked() {
            self.open_settings();
        }
        ui.add_space(6.0);
        let mut mode = mode_of(self.cfg.theme);
        let tooltip = i18n::tr_fmt(lang, "Theme: {mode}", &[("mode", i18n::tr(lang, mode.label()))]);
        if craft_icons::ui::theme_button(ui, &mut mode, &tooltip) {
            self.set_mode(ui.ctx(), mode);
        }
        ui.add_space(6.0);
        let search = i18n::tr_fmt(lang, "Search commands ({shortcut})", &[("shortcut", &shortcut_text(ui.ctx()))]);
        if craft_icons::ui::button(ui, Icon::Search, &search).clicked() {
            self.palette.open();
        }
    }

    /// Configurações: idioma e tema no topo, as opções do app no meio e a seção Sobre (versão, repositório,
    /// atualizações) no fim.
    pub(crate) fn settings_modal(&mut self, ctx: &egui::Context) {
        let was_open = self.options.is_open();
        let lang = self.cfg.lang;
        let t = |source: &'static str| i18n::tr(lang, source);
        let mut check = false;
        let mut new_lang = lang;
        let mut mode = mode_of(self.cfg.theme);
        let status = self.updater.status().clone();
        let general = |ui: &mut egui::Ui| {
            egui::Grid::new("settings-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(i18n::t(lang, Text::Language));
                egui::ComboBox::from_id_salt("settings-lang").selected_text(lang.label()).show_ui(ui, |ui| {
                    for choice in Lang::ALL {
                        ui.selectable_value(&mut new_lang, choice, choice.label());
                    }
                });
                ui.end_row();
                ui.label(i18n::t(lang, Text::Theme));
                ui.horizontal_wrapped(|ui| {
                    for choice in ThemeMode::EVERY {
                        ui.selectable_value(&mut mode, choice, t(choice.label()));
                    }
                });
                ui.end_row();
            });
        };
        let about = |ui: &mut egui::Ui| {
            ui.strong(t("About Local Utils"));
            ui.label(i18n::tr_fmt(lang, "version {version}", &[("version", env!("CARGO_PKG_VERSION"))]));
            ui.hyperlink_to("github.com/Anon5T4R/Local-Utils", env!("CARGO_PKG_REPOSITORY"));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let checking = matches!(status, Status::Checking);
                if ui.add_enabled(!checking, egui::Button::new(t("Check for Updates…"))).clicked() {
                    check = true;
                }
                match &status {
                    Status::Checking => {
                        ui.spinner();
                    }
                    Status::UpToDate => {
                        ui.label(t("You are on the newest version."));
                    }
                    Status::Failed(reason) => {
                        ui.label(format!("⚠ {reason}"));
                    }
                    Status::Idle => {}
                }
            });
        };
        let opts = options::show(ctx, lang, &mut self.options, general, about);
        if new_lang != lang {
            self.set_lang(new_lang);
        }
        if mode_of(self.cfg.theme) != mode {
            self.set_mode(ctx, mode);
        }
        if check {
            self.updater.check_now(ctx);
        }
        self.apply_options(ctx, opts);
        if was_open && !self.options.is_open() {
            self.updater.dismiss_status();
        }
    }

    /// Encerra DE VERDADE (o X da janela só esconde na bandeja): grava o histórico e fecha pelo caminho limpo,
    /// o mesmo do updater.
    pub(crate) fn quit_for_real(&mut self, ctx: &egui::Context) {
        self.quit = false;
        self.save_history();
        #[cfg(windows)]
        {
            self.allow_close = true;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// O que a paleta lista agora: cada utilitário, as ações das abas, as configurações, atualizações e Sobre.
    pub(crate) fn commands(&self) -> Vec<Command> {
        let lang = self.cfg.lang;
        let tr = |source: &'static str| i18n::tr(lang, source);
        let (utilities, file, settings, help, app) = (tr("Utilities"), tr("File"), tr("Settings"), tr("Help"), "Local Utils");
        let mut list = Vec::new();
        for tab in Tab::ALL {
            list.push(Command::new(format!("tab.{}", tab_id(tab)), tab.label(lang), utilities));
        }
        for sub in SubTab::ALL {
            let label = format!("{}: {}", Tab::Tools.label(lang), i18n::t(lang, sub.label_key()));
            list.push(Command::new(format!("tools.{}", sub_id(sub)), label, utilities));
        }
        let capture = Command::new("capture.screen", tr("Capture screen"), utilities).enabled(cfg!(windows));
        list.push(capture.shortcut(options::pretty_hotkey(&self.cfg.hotkey_screenshot)));
        list.push(Command::new("color.pick", tr("Pick a color from the screen"), utilities).enabled(cfg!(windows)));
        list.push(Command::new("convert.add", tr("Add images to convert…"), file));
        list.push(Command::new("history.export", tr("Export history…"), file));
        list.push(Command::new("history.import", tr("Import history…"), file));
        list.push(Command::new("settings.open", tr("Settings"), settings));
        for mode in ThemeMode::EVERY {
            let label = i18n::tr_fmt(lang, "Theme: {mode}", &[("mode", i18n::tr(lang, mode.label()))]);
            list.push(Command::new(format!("settings.theme.{}", mode.id()), label, settings));
        }
        let checking = matches!(self.updater.status(), Status::Checking);
        list.extend([
            Command::new("help.updates", tr("Check for Updates…"), help).enabled(!checking),
            Command::new("help.about", tr("About Local Utils"), help),
            Command::new("app.exit", tr("Exit"), app),
        ]);
        list
    }

    /// Roda o que a paleta devolveu, pelo mesmo código dos botões das abas.
    pub(crate) fn run_command(&mut self, ctx: &egui::Context, id: &str) {
        match id {
            "capture.screen" => self.start_capture(ctx),
            "color.pick" => {
                if let Err(e) = self.picker.start_eyedropper(ctx) {
                    self.set_status(format!("⚠ {}: {e}", i18n::t(self.cfg.lang, Text::CaptureFailed)));
                }
            }
            "convert.add" => {
                self.tab = Tab::Converter;
                self.ask_convert_files(ctx);
            }
            "history.export" => self.apply_options(ctx, options::Outcome { export: true, ..Default::default() }),
            "history.import" => self.apply_options(ctx, options::Outcome { import: true, ..Default::default() }),
            "settings.open" | "help.about" => self.open_settings(),
            "help.updates" => {
                // O resultado aparece nas Configurações, como no botão de lá.
                self.open_settings();
                self.updater.check_now(ctx);
            }
            "app.exit" => self.quit = true,
            _ => {
                if let Some(mode) = id.strip_prefix("settings.theme.") {
                    self.set_mode(ctx, ThemeMode::from_id(mode));
                } else if let Some(name) = id.strip_prefix("tab.") {
                    if let Some(tab) = Tab::ALL.into_iter().find(|t| tab_id(*t) == name) {
                        self.tab = tab;
                    }
                } else if let Some(name) = id.strip_prefix("tools.") {
                    if let Some(sub) = SubTab::ALL.into_iter().find(|s| sub_id(*s) == name) {
                        self.tab = Tab::Tools;
                        self.tools.select(sub);
                    }
                }
            }
        }
        ctx.request_repaint();
    }

    /// A paleta, quando aberta.
    pub(crate) fn palette_ui(&mut self, ctx: &egui::Context) {
        if !self.palette.is_open() {
            return;
        }
        let lang = self.cfg.lang;
        let commands = self.commands();
        let labels = craft_palette::Labels { placeholder: i18n::tr(lang, "Search commands"), empty: i18n::tr(lang, "No command matches.") };
        if let Some(id) = self.palette.show(ctx, &labels, &commands) {
            self.run_command(ctx, &id);
        }
    }
}
