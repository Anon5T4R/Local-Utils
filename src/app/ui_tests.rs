//! Testes de UI sem janela (egui_kittest) da casca da suíte: o botão de tema, a engrenagem, a lupa, as
//! Configurações e a paleta. O app de teste não sobe monitor de clipboard, bandeja nem atalho global, e nenhum
//! teste toca o config.json ou o histórico reais nem a rede.

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

use craft_icons::ThemeMode;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use crate::prefs::Prefs;

use super::{Tab, UtilsApp};
use crate::history::History;

/// A menor largura que a janela pode ter na prática (o app não fixa mínimo).
const NARROW: f32 = 340.0;

/// Pasta temporária própria do teste (config.json e history.json de teste vão para lá).
fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("local-utils-ui-{name}-{nanos}"))
}

/// O app inteiro, mas sem as peças do sistema: histórico vazio, sem monitor, sem bandeja, sem atalho global.
fn app(name: &str, cfg: Prefs) -> UtilsApp {
    let (_tx, tray_rx) = channel();
    UtilsApp::assemble(
        cfg,
        scratch(name),
        History::default(),
        None,
        #[cfg(windows)]
        None,
        tray_rx,
        None,
    )
}

fn harness_sized(app: UtilsApp, width: f32) -> Harness<'static, UtilsApp> {
    Harness::builder()
        .with_size(egui::vec2(width, 680.0))
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, app: &mut UtilsApp| {
                egui_extras::install_image_loaders(ui.ctx());
                crate::i18n::set_language(&app.cfg.language);
                crate::theme::apply(ui.ctx(), app.cfg.mode());
                app.frame(ui);
            },
            app,
        )
}

fn harness(app: UtilsApp) -> Harness<'static, UtilsApp> {
    harness_sized(app, 980.0)
}

/// Os botões da barra de cima, da esquerda para a direita (só os pequenos: os de texto das abas ficam de fora).
fn bar_buttons(harness: &Harness<'_, UtilsApp>) -> Vec<egui::Rect> {
    let mut rects: Vec<egui::Rect> =
        harness.get_all_by_role(Role::Button).map(|node| node.rect()).filter(|rect| rect.min.y < 34.0 && rect.width() < 40.0).collect();
    rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    rects
}

fn click_at(harness: &mut Harness<'_, UtilsApp>, pos: egui::Pos2) {
    let push = |harness: &mut Harness<'_, UtilsApp>, event: egui::Event| harness.input_mut().events.push(event);
    push(harness, egui::Event::PointerMoved(pos));
    harness.run_steps(1);
    let button = egui::PointerButton::Primary;
    push(harness, egui::Event::PointerButton { pos, button, pressed: true, modifiers: egui::Modifiers::default() });
    harness.run_steps(1);
    push(harness, egui::Event::PointerButton { pos, button, pressed: false, modifiers: egui::Modifiers::default() });
    harness.run_steps(2);
}

/// Clica num dos três botões da suíte, contando da DIREITA: 0 = engrenagem, 1 = tema, 2 = lupa.
fn click_bar(harness: &mut Harness<'_, UtilsApp>, from_right: usize) {
    let rects = bar_buttons(harness);
    assert!(rects.len() > from_right, "a barra não tem o botão {from_right}: {rects:?}");
    click_at(harness, rects[rects.len() - 1 - from_right].center());
}

const GEAR: usize = 0;
const THEME: usize = 1;
const SEARCH: usize = 2;

fn press(harness: &mut Harness<'_, UtilsApp>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    harness.run_steps(2);
}

fn run_palette(harness: &mut Harness<'_, UtilsApp>, query: &str) {
    press(harness, egui::Key::K, egui::Modifiers::COMMAND);
    harness.input_mut().events.push(egui::Event::Text(query.into()));
    harness.run_steps(2);
    press(harness, egui::Key::Enter, egui::Modifiers::NONE);
}

fn shot(harness: &mut Harness<'_, UtilsApp>, name: &str) {
    if std::env::var_os("LOCALUTILS_QA_RENDER").is_none() {
        return;
    }
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-casca");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    if let Ok(image) = harness.render() {
        let _ = image.save(directory.join(format!("{name}.png")));
    }
}

/// O botão de tema da barra gira Automático -> Claro -> Escuro -> Automático e grava a escolha.
#[test]
fn the_theme_button_cycles_the_three_modes() {
    let mut harness = harness(app("theme", Prefs::default()));
    assert_eq!(harness.state().cfg.mode(), ThemeMode::System);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().cfg.mode(), ThemeMode::Light);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().cfg.mode(), ThemeMode::Dark);
    click_bar(&mut harness, THEME);
    assert_eq!(harness.state().cfg.mode(), ThemeMode::System);
    // Cada troca foi gravada: o arquivo de teste (nunca o real) guarda o modo atual.
    let saved = crate::prefs::load_from(&harness.state().config_file());
    assert_eq!(saved.mode(), ThemeMode::System);
}

/// Uma preferência de tema gravada por versões antigas (id que a casca não conhece) abre no Automático.
#[test]
fn an_old_theme_id_falls_back_to_automatic() {
    let dir = scratch("old-theme");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("config.json");
    std::fs::write(&file, r#"{"theme":"PunkPrincess","lang":"En"}"#).expect("write config");
    let cfg = crate::prefs::load_from(&file);
    assert_eq!(cfg.mode(), ThemeMode::System);
    assert_eq!(cfg.language, "en");
}

/// A engrenagem abre as Configurações; lá o tema vira pelas três opções e o idioma troca.
#[test]
fn the_gear_opens_settings_with_language_and_theme() {
    let mut harness = harness(app("gear", Prefs::new("en", ThemeMode::Dark)));
    click_bar(&mut harness, GEAR);
    // O diálogo se centraliza depois da primeira medida: espera o layout assentar antes de ler posições.
    harness.run_steps(4);
    assert!(harness.state().options.is_open());
    harness.get_by_label("About Local Utils");
    shot(&mut harness, "settings-dark");
    let light = harness.get_by_label("Light").rect();
    click_at(&mut harness, light.center());
    assert_eq!(harness.state().cfg.mode(), ThemeMode::Light);
    let automatic = harness.get_by_label("Automatic").rect();
    click_at(&mut harness, automatic.center());
    assert_eq!(harness.state().cfg.mode(), ThemeMode::System);
    // O idioma sai da barra e mora aqui: o combo troca para o português.
    harness.get_by_role(Role::ComboBox).click();
    harness.run_steps(2);
    harness.get_by_label("Português (Brasil)").click();
    harness.run_steps(2);
    assert_eq!(harness.state().cfg.language, "pt-br");
    harness.get_by_label("Sobre o Local Utils");
    // Esc fecha a janela.
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().options.is_open());
}

/// A lupa e o Ctrl+K abrem a paleta; Enter roda o comando escolhido.
#[test]
fn the_search_icon_and_ctrl_k_open_the_palette_and_run_a_command() {
    let mut harness = harness(app("palette", Prefs::new("en", ThemeMode::Light)));
    assert!(!harness.state().palette.is_open());
    click_bar(&mut harness, SEARCH);
    assert!(harness.state().palette.is_open());
    press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!harness.state().palette.is_open());

    run_palette(&mut harness, "about");
    assert!(!harness.state().palette.is_open());
    assert!(harness.state().options.is_open(), "About abre as Configurações");
}

/// Um comando da paleta troca de aba e outro abre uma sub-aba de Ferramentas.
#[test]
fn palette_commands_open_tabs_and_tools() {
    let mut harness = harness(app("tabs", Prefs::new("en", ThemeMode::Dark)));
    assert!(harness.state().tab == Tab::Clipboard);
    run_palette(&mut harness, "color");
    assert!(harness.state().tab == Tab::Cor);
    run_palette(&mut harness, "calculator");
    assert!(harness.state().tab == Tab::Tools);
    run_palette(&mut harness, "Theme: Dark");
    assert_eq!(harness.state().cfg.mode(), ThemeMode::Dark);
}

/// "Exit" na paleta pede o encerramento (o quadro seguinte responde com Close).
#[test]
fn exit_in_the_palette_asks_to_quit() {
    let mut harness = harness(app("exit", Prefs::new("en", ThemeMode::Dark)));
    run_palette(&mut harness, "exit");
    // O pedido já foi atendido no quadro seguinte: o flag volta a falso e o fechamento real foi liberado.
    harness.run_steps(2);
    assert!(!harness.state().quit);
}

/// Os comandos que dependem do sistema só ficam ligados onde existem (captura e conta-gotas: Windows).
#[test]
fn system_commands_follow_the_platform() {
    let app = app("platform", Prefs::default());
    for command in app.commands() {
        if matches!(command.id.as_str(), "capture.screen" | "color.pick") {
            assert_eq!(command.enabled, cfg!(windows), "{}", command.id);
        } else {
            assert!(command.enabled, "{} deveria estar ligado", command.id);
        }
    }
}

/// Com a paleta aberta a lista do Clipboard fica quieta: o Delete não remove a entrada com foco.
#[test]
fn the_clipboard_list_stays_quiet_while_the_palette_is_open() {
    let mut app = app("quiet", Prefs::new("en", ThemeMode::Light));
    app.history.push(crate::history::Entry {
        id: 1,
        ts_ms: 1,
        kind: crate::history::Kind::Text,
        text: Some("uma cópia".into()),
        ..Default::default()
    });
    let mut harness = harness(app);
    harness.run_steps(2);
    let before = harness.state().history.len();
    click_bar(&mut harness, SEARCH);
    assert!(harness.state().palette.is_open());
    press(&mut harness, egui::Key::ArrowDown, egui::Modifiers::NONE);
    press(&mut harness, egui::Key::Delete, egui::Modifiers::NONE);
    assert_eq!(harness.state().history.len(), before);
}

/// Os três botões da suíte ficam inteiros, na ordem certa e dentro da janela, na largura estreita e na larga.
#[test]
fn the_top_bar_buttons_fit_narrow_and_wide_windows() {
    for width in [NARROW, 600.0, 1100.0] {
        let mut harness = harness_sized(app("narrow", Prefs::new("pt-br", ThemeMode::Light)), width);
        harness.run_steps(2);
        let rects = bar_buttons(&harness);
        assert_eq!(rects.len(), 3, "{width}: a barra tem lupa, tema e engrenagem: {rects:?}");
        for pair in rects.windows(2) {
            assert!(pair[0].max.x <= pair[1].min.x + 0.5, "{width}: botões se sobrepõem: {pair:?}");
        }
        assert!(rects.iter().all(|r| r.min.x >= 0.0 && r.max.x <= width), "{width}: botão fora da janela: {rects:?}");
    }
}

/// As capturas de revisão: duas larguras (mínima e ~1100), nos dois temas, com a barra, as Configurações e a paleta.
#[test]
fn review_shots() {
    for (width, label) in [(NARROW, "340"), (1100.0, "1100")] {
        for (mode, name) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            let mut harness = harness_sized(app("shots", Prefs::new("en", mode)), width);
            harness.run_steps(2);
            shot(&mut harness, &format!("bar-{name}-{label}"));
            click_bar(&mut harness, GEAR);
            harness.run_steps(4);
            shot(&mut harness, &format!("settings-{name}-{label}"));
            press(&mut harness, egui::Key::Escape, egui::Modifiers::NONE);
            click_bar(&mut harness, SEARCH);
            harness.run_steps(4);
            shot(&mut harness, &format!("palette-{name}-{label}"));
        }
    }
}
