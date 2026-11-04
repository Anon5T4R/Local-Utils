//! Diálogo de opções do app (a engrenagem da barra de topo): autostart, atalho
//! global, lista de apps sensíveis e export/import do histórico (`.lutils`).
//!
//! Os rascunhos ficam aqui e só viram config no **Salvar** — fechar o diálogo
//! (X, Esc, clique fora) descarta as mudanças de texto/lista. Exceção: o
//! autostart é um toggle do SO e vale na hora (o app grava a chave Run e
//! devolve o cache no config).
//!
//! O atalho não é digitado: o botão **Registrar atalho** entra em modo de
//! captura e o próximo par modificadores+tecla vira o valor, no mesmo formato
//! que o parser do `hotkey.rs` entende (`win+alt+v`). O texto salvo continua
//! sendo esse formato; a tela mostra a versão legível ao lado.

use eframe::egui;
use utils_ui::config::{Config, DEFAULT_HOTKEY};
use utils_ui::i18n::{self, Key, Lang};

use crate::hotkey;
use crate::ui as kit;

/// Teclas que o parser do `hotkey.rs` aceita → o token que ele espera. Tudo o
/// que não está aqui (Enter, Tab, setas…) não serve de atalho global.
const KEY_TOKENS: &[(egui::Key, &str)] = &[
    (egui::Key::A, "a"),
    (egui::Key::B, "b"),
    (egui::Key::C, "c"),
    (egui::Key::D, "d"),
    (egui::Key::E, "e"),
    (egui::Key::F, "f"),
    (egui::Key::G, "g"),
    (egui::Key::H, "h"),
    (egui::Key::I, "i"),
    (egui::Key::J, "j"),
    (egui::Key::K, "k"),
    (egui::Key::L, "l"),
    (egui::Key::M, "m"),
    (egui::Key::N, "n"),
    (egui::Key::O, "o"),
    (egui::Key::P, "p"),
    (egui::Key::Q, "q"),
    (egui::Key::R, "r"),
    (egui::Key::S, "s"),
    (egui::Key::T, "t"),
    (egui::Key::U, "u"),
    (egui::Key::V, "v"),
    (egui::Key::W, "w"),
    (egui::Key::X, "x"),
    (egui::Key::Y, "y"),
    (egui::Key::Z, "z"),
    (egui::Key::Num0, "0"),
    (egui::Key::Num1, "1"),
    (egui::Key::Num2, "2"),
    (egui::Key::Num3, "3"),
    (egui::Key::Num4, "4"),
    (egui::Key::Num5, "5"),
    (egui::Key::Num6, "6"),
    (egui::Key::Num7, "7"),
    (egui::Key::Num8, "8"),
    (egui::Key::Num9, "9"),
    (egui::Key::F1, "f1"),
    (egui::Key::F2, "f2"),
    (egui::Key::F3, "f3"),
    (egui::Key::F4, "f4"),
    (egui::Key::F5, "f5"),
    (egui::Key::F6, "f6"),
    (egui::Key::F7, "f7"),
    (egui::Key::F8, "f8"),
    (egui::Key::F9, "f9"),
    (egui::Key::F10, "f10"),
    (egui::Key::F11, "f11"),
    (egui::Key::F12, "f12"),
];

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
    /// Popup no cursor ligado? Vale na hora — só persiste no config.
    popup_mode: bool,
    /// Modo de captura do atalho: a próxima combinação vira o rascunho.
    capturing: bool,
    /// O Win estava pressionado no frame anterior. O egui não entrega o Super
    /// nas `Modifiers` fora do macOS, então a captura pergunta ao SO
    /// ([`hotkey::win_down`]); este cache cobre o frame em que a tecla chega
    /// com o Win já solto.
    win_held: bool,
}

/// O que o app precisa aplicar depois de desenhar o diálogo.
#[derive(Default)]
pub struct Outcome {
    /// Salvar: aplicar os rascunhos no config e persistir.
    pub save: bool,
    /// O autostart mudou: gravar a chave Run agora.
    pub autostart: Option<bool>,
    /// O modo do popup mudou: persistir agora (o efeito é imediato, mas o
    /// valor que vale é o do config).
    pub popup_mode: Option<bool>,
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
        self.popup_mode = cfg.popup_instead_of_window;
        // Reabrir sempre começa fora do modo de captura.
        self.capturing = false;
        self.win_held = false;
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

        // ── popup no cursor (vale na hora) ──────────────────────────
        // Quem prefere a janela inteira no atalho desmarca aqui: o popup só
        // troca o que o atalho abre, nada mais do app muda.
        let mut popup_mode = state.popup_mode;
        if ui.checkbox(&mut popup_mode, t(Key::TogglePopupMode)).changed() {
            state.popup_mode = popup_mode;
            out.popup_mode = Some(popup_mode);
        }
        ui.add_space(10.0);

        // ── atalho global (vale no próximo arranque) ──────────────────
        ui.horizontal(|ui| {
            ui.label(t(Key::HotkeyGlobal));
            // Captura ANTES do campo: o `TextEdit` com foco consome as teclas
            // do frame, e a leitura das teclas tem que acontecer primeiro.
            if state.capturing {
                capture(ui, state);
            }
            // Campo read-only de verdade: o valor só muda pela captura. O
            // buffer é DESCARTÁVEL (recriado a cada frame), então o que o
            // usuário digitar nele não mexe no rascunho.
            let mut shown = if state.capturing { t(Key::PressKeysHint).to_string() } else { state.hotkey.clone() };
            let field = ui.add_enabled(windows, kit::line(&mut shown).hint_text(DEFAULT_HOTKEY).desired_width(200.0));
            if state.capturing && !field.has_focus() {
                // Foco é o que dá "no campo" à captura (o egui entrega as
                // teclas pelo `Context`, não pelo widget).
                field.request_focus();
            }
            let label = if state.capturing { t(Key::Cancel) } else { t(Key::RegisterHotkey) };
            let button = ui.add_enabled_ui(windows, |ui| kit::pill(ui, label, false)).inner;
            if button.clicked() {
                state.capturing = !state.capturing;
                state.win_held = false;
            }
            // O atalho atual em texto legível, ao lado do campo.
            ui.label(egui::RichText::new(pretty_hotkey(&state.hotkey)).small().weak());
            if !state.capturing {
                button.on_hover_text(t(if windows { Key::RestartApplies } else { Key::WindowsOnly }));
            }
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
        state.capturing = false;
    } else if close || resp.should_close() {
        state.open = false;
        state.capturing = false;
    }
    out
}

/// Modo de captura do atalho: lê as teclas DESTE frame e, no primeiro par
/// modificadores+tecla válido, grava o rascunho no formato do parser
/// (`win+alt+v`). Esc cancela.
///
/// Modificadores: Ctrl/Alt/Shift (das `Modifiers` do egui) e o Win, perguntado
/// ao SO — o egui não expõe o Super fora do macOS. Tecla sem modificador é
/// ignorada; F1–F12 sozinhas passam (já são atalhos legítimos).
fn capture(ui: &egui::Ui, state: &mut OptionsState) {
    // Esc: consumido aqui pra que o Modal NÃO feche junto — o `should_close`
    // dele também escuta Esc, e cancelar a captura não é fechar as opções.
    if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.capturing = false;
        state.win_held = false;
        return;
    }

    let win_now = hotkey::win_down();
    let win = win_now || state.win_held;
    state.win_held = win_now;

    let events = ui.ctx().input(|i| i.events.clone());
    for event in &events {
        let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = event else { continue };
        if let Some(spec) = hotkey_spec(*modifiers, win, *key) {
            state.hotkey = spec;
            state.capturing = false;
            state.win_held = false;
            return;
        }
    }
}

/// Monta o texto do atalho (o formato que o parser do `hotkey.rs` lê) a partir
/// da tecla e dos modificadores. `None` = combinação que não serve: tecla que o
/// parser não conhece ou tecla sozinha (que não é F1–F12).
fn hotkey_spec(modifiers: egui::Modifiers, win: bool, key: egui::Key) -> Option<String> {
    let token = key_token(key)?;
    let mut parts: Vec<&str> = Vec::new();
    if win {
        parts.push("win");
    }
    if modifiers.ctrl {
        parts.push("ctrl");
    }
    if modifiers.alt {
        parts.push("alt");
    }
    if modifiers.shift {
        parts.push("shift");
    }
    if parts.is_empty() && !is_function_key(key) {
        return None;
    }
    parts.push(token);
    Some(parts.join("+"))
}

/// Token do parser para uma tecla do egui (`Key::V` → `"v"`, `Key::Num1` →
/// `"1"`, `Key::F9` → `"f9"`). `None` = tecla fora da lista do `hotkey.rs`.
fn key_token(key: egui::Key) -> Option<&'static str> {
    KEY_TOKENS.iter().find(|(k, _)| *k == key).map(|(_, token)| *token)
}

/// F1–F12 servem sozinhas como atalho (o parser aceita sem modificador).
fn is_function_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::F1
            | egui::Key::F2
            | egui::Key::F3
            | egui::Key::F4
            | egui::Key::F5
            | egui::Key::F6
            | egui::Key::F7
            | egui::Key::F8
            | egui::Key::F9
            | egui::Key::F10
            | egui::Key::F11
            | egui::Key::F12
    )
}

/// Versão legível do texto salvo: `win+alt+v` → `Win + Alt + V`. O config
/// continua no formato do parser; isto é só o que a tela mostra.
fn pretty_hotkey(spec: &str) -> String {
    let parts: Vec<String> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).map(pretty_token).collect();
    parts.join(" + ")
}

/// Nome legível de uma parte do atalho (modificador conhecido ou a tecla em
/// caixa alta).
fn pretty_token(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => "Ctrl".to_string(),
        "alt" => "Alt".to_string(),
        "shift" => "Shift".to_string(),
        "win" | "super" | "meta" => "Win".to_string(),
        key => key.to_ascii_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Modifiers` só com os três que o egui entrega no Windows.
    fn mods(ctrl: bool, alt: bool, shift: bool) -> egui::Modifiers {
        egui::Modifiers { ctrl, alt, shift, ..egui::Modifiers::NONE }
    }

    /// O caso do padrão novo: Win+Alt+V tem que sair no formato do parser.
    #[test]
    fn captura_monta_o_atalho_no_formato_do_parser() {
        assert_eq!(hotkey_spec(mods(false, true, false), true, egui::Key::V), Some("win+alt+v".to_string()));
        assert_eq!(hotkey_spec(mods(true, false, true), false, egui::Key::Num1), Some("ctrl+shift+1".to_string()));
        // Sem Win e sem Ctrl/Alt/Shift: só F1–F12 valem sozinhas.
        assert_eq!(hotkey_spec(mods(false, false, false), false, egui::Key::F9), Some("f9".to_string()));
        assert_eq!(hotkey_spec(mods(false, false, false), false, egui::Key::V), None, "tecla sozinha não é atalho global");
        // Tecla que o parser não conhece nunca vira atalho (nem com modificador).
        assert_eq!(hotkey_spec(mods(false, true, false), false, egui::Key::Space), None);
        assert_eq!(hotkey_spec(mods(true, false, false), true, egui::Key::Enter), None);
    }

    /// O padrão do app sai do mesmo caminho (Win perguntado ao SO + Alt).
    #[test]
    fn captura_reproduz_o_padrao_do_config() {
        let configurado = DEFAULT_HOTKEY;
        assert_eq!(hotkey_spec(mods(false, true, false), true, egui::Key::V).as_deref(), Some(configurado));
    }

    #[test]
    fn texto_legivel_do_atalho() {
        assert_eq!(pretty_hotkey("win+alt+v"), "Win + Alt + V");
        assert_eq!(pretty_hotkey(" ctrl + F9 "), "Ctrl + F9");
        assert_eq!(pretty_hotkey("super+shift+1"), "Win + Shift + 1");
        assert_eq!(pretty_hotkey(""), "");
    }

    /// O que a captura grava e o que a tela mostra não divergem: todo token da
    /// tabela aparece legível (sem ficar minúsculo ou vazio).
    #[test]
    fn toda_tecla_da_tabela_tem_token_legivel() {
        for (key, token) in KEY_TOKENS {
            assert!(!token.is_empty(), "{key:?} sem token");
            assert_eq!(pretty_token(token), token.to_ascii_uppercase(), "token {token:?} no texto legível");
        }
    }
}
