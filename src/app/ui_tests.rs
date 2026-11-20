//! Testes de UI sem janela (egui_kittest) da casca da suíte: o botão de tema, a engrenagem, a lupa, as
//! Configurações e a paleta. O app de teste não sobe monitor de clipboard, bandeja nem atalho global, e nenhum
//! teste toca o config.json ou o histórico reais nem a rede.

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

use crate::prefs::Prefs;
use craft_icons::ThemeMode;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::{Tab, UtilsApp};
use crate::history::History;
use crate::tools::SubTab;

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
    Harness::builder().with_size(egui::vec2(width, 680.0)).with_step_dt(1.0 / 60.0).build_ui_state(
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

// ── Largura mínima, nomes acessíveis e conteúdo de verdade ────────────────────────────────────────────────

/// O app com a lista cheia: textos curtos, longos e sem espaço, rico, arquivos com caminho comprido e um fixado.
fn loaded(name: &str, prefs: Prefs) -> UtilsApp {
    use crate::history::{Entry, Kind};
    let mut app = app(name, prefs);
    let long_path = r"C:\Users\Joao\Documents\Projetos\relatorio-trimestral-com-um-nome-muito-comprido-mesmo-final-v7.xlsx";
    let entries = [
        Entry { id: 1, ts_ms: 1_000, kind: Kind::Text, text: Some("uma cópia curta".into()), ..Default::default() },
        Entry { id: 2, ts_ms: 2_000, kind: Kind::Text, text: Some("a".repeat(240)), pinned: true, ..Default::default() },
        Entry { id: 3, ts_ms: 3_000, kind: Kind::Text, text: Some("várias linhas\nsegunda linha\nterceira".into()), ..Default::default() },
        Entry { id: 4, ts_ms: 4_000, kind: Kind::Html, text: Some("<p>texto <b>rico</b></p>".into()), ..Default::default() },
        Entry { id: 5, ts_ms: 5_000, kind: Kind::Files, files: Some(vec![long_path.into()]), ..Default::default() },
    ];
    for entry in entries {
        app.history.push(entry);
    }
    app
}

/// Nenhum nó da tela (nem de diálogo) passa da borda de uma janela de `width`.
fn assert_fits(harness: &Harness<'_, UtilsApp>, width: f32, what: &str) {
    for node in harness.root().children_recursive() {
        let Some(rect) = node.accesskit_node().bounding_box() else { continue };
        let role = node.accesskit_node().role();
        // O texto de um campo é cortado pelo próprio campo (que é conferido): só o campo conta.
        if rect.width() > 0.0 && role != Role::TextRun {
            let label = node.accesskit_node().label().unwrap_or_default();
            assert!(rect.x0 >= -0.5 && rect.x1 <= f64::from(width) + 0.5, "{what}: {role:?} {label:?} fora da janela de {width}: {rect:?}");
        }
    }
}

type Setup = fn(&mut UtilsApp);

/// Cada aba, e cada sub-aba de Ferramentas, montada com conteúdo.
fn tab_setups() -> Vec<(&'static str, Setup)> {
    vec![
        ("clipboard", |app| app.tab = Tab::Clipboard),
        ("screenshot", |app| {
            app.tab = Tab::Screenshot;
            app.shot = Some(crate::shot_editor::Shot::new(image::RgbaImage::from_pixel(640, 360, image::Rgba([80, 120, 200, 255]))));
        }),
        ("system", |app| app.tab = Tab::Sistema),
        ("tools-clock", |app| {
            app.tab = Tab::Tools;
            app.tools.select(SubTab::Clock);
        }),
        ("tools-calc", |app| {
            app.tab = Tab::Tools;
            app.tools.select(SubTab::Calc);
        }),
        ("tools-units", |app| {
            app.tab = Tab::Tools;
            app.tools.select(SubTab::Units);
        }),
        ("tools-dev", |app| {
            app.tab = Tab::Tools;
            app.tools.select(SubTab::Dev);
        }),
        ("notes", |app| {
            app.tab = Tab::Scratchpad;
            app.cfg.notes = "uma nota com texto\nem duas linhas".into();
        }),
        ("color", |app| app.tab = Tab::Cor),
        ("convert", |app| {
            app.tab = Tab::Converter;
            let dir = scratch("convert-files");
            std::fs::create_dir_all(&dir).expect("pasta");
            let long = dir.join("fotografia-das-ferias-na-praia-com-um-nome-de-arquivo-bem-comprido-demais.png");
            image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 60, 60, 255])).save(&long).expect("png");
            let short = dir.join("foto.png");
            image::RgbaImage::from_pixel(8, 8, image::Rgba([60, 60, 200, 255])).save(&short).expect("png");
            let text = dir.join("notas.txt");
            std::fs::write(&text, "não é imagem").expect("txt");
            app.convert.enqueue([long, short, text]);
        }),
    ]
}

/// Cada diálogo e janela do app.
fn dialog_setups() -> Vec<(&'static str, Setup)> {
    vec![
        ("settings", |app| app.open_settings()),
        ("clip-edit", |app| app.clip_ui.open_edit(2, &"a".repeat(240))),
        ("clip-qr", |app| app.clip_ui.open_qr("https://exemplo.com/um/caminho/qualquer")),
        ("clip-ocr", |app| app.clip_ui.show_ocr(1, "texto lido da imagem pelo OCR, numa linha só bem comprida para forçar a quebra".into())),
        ("clip-clear", |app| app.clip_ui.open_clear()),
        ("alarm-new", |app| {
            app.tab = Tab::Tools;
            app.tools.open_alarm();
        }),
        ("end-process", |app| {
            app.tab = Tab::Sistema;
            app.system.ask_kill(4242, "um-processo-com-nome-bem-comprido-mesmo.exe");
        }),
        ("update-note", |app| app.update_note = Some("atualizado (0.6.13)".into())),
    ]
}

/// Toda aba cabe na largura mínima, em português e espanhol (os textos mais longos), com conteúdo.
#[test]
fn every_tab_fits_the_narrow_window() {
    for (code, mode) in [("pt-br", ThemeMode::Light), ("es", ThemeMode::Dark)] {
        for (name, setup) in tab_setups() {
            let mut app = loaded("tab-narrow", Prefs::new(code, mode));
            setup(&mut app);
            let mut harness = harness_sized(app, NARROW);
            harness.run_steps(4);
            shot(&mut harness, &format!("tab-{name}-{code}-340"));
            assert_fits(&harness, NARROW, &format!("{name} ({code})"));
        }
    }
}

/// Todo diálogo cabe na largura mínima: nenhum controle passa da borda da janela.
#[test]
fn every_dialog_fits_the_narrow_window() {
    for (code, mode) in [("pt-br", ThemeMode::Dark), ("es", ThemeMode::Light)] {
        for (name, setup) in dialog_setups() {
            let mut app = loaded("dialog-narrow", Prefs::new(code, mode));
            setup(&mut app);
            let mut harness = harness_sized(app, NARROW);
            harness.run_steps(4);
            shot(&mut harness, &format!("dialog-{name}-{code}-340"));
            assert_fits(&harness, NARROW, &format!("{name} ({code})"));
        }
    }
}

/// Todo campo de texto, caixa de combinação, valor arrastável e controle deslizante se anuncia por um nome
/// (leitor de tela), em cada aba e em cada diálogo.
#[test]
fn every_field_has_a_name() {
    let roles = [Role::TextInput, Role::MultilineTextInput, Role::PasswordInput, Role::ComboBox, Role::SpinButton, Role::Slider];
    let all = tab_setups().into_iter().chain(dialog_setups());
    for (name, setup) in all {
        let mut app = loaded("names", Prefs::new("en", ThemeMode::Light));
        setup(&mut app);
        let mut harness = harness(app);
        harness.run_steps(4);
        for node in harness.root().children_recursive() {
            let role = node.accesskit_node().role();
            if roles.contains(&role) {
                let label = node.accesskit_node().label().unwrap_or_default();
                assert!(!label.is_empty(), "{name}: {role:?} sem nome");
            }
        }
    }
}

/// A barra de cima e a paleta cabem na largura mínima, com a lista cheia por baixo.
#[test]
fn the_bar_and_the_palette_fit_the_narrow_window_over_content() {
    for width in [NARROW, 480.0] {
        let mut harness = harness_sized(loaded("palette-narrow", Prefs::new("pt-br", ThemeMode::Dark)), width);
        harness.run_steps(2);
        assert_fits(&harness, width, "barra");
        click_bar(&mut harness, SEARCH);
        harness.run_steps(2);
        assert!(harness.state().palette.is_open());
        let rows: Vec<egui::Rect> = harness.get_all_by_role(Role::ListBoxOption).map(|node| node.rect()).collect();
        assert!(!rows.is_empty());
        assert_fits(&harness, width, "paleta");
    }
}

/// O que pode ser longo (caminho de arquivo, texto sem espaço) corta com reticências ANTES do ★ e dos botões.
#[test]
fn long_clipboard_text_stops_before_the_buttons() {
    let mut harness = harness_sized(loaded("long-text", Prefs::new("en", ThemeMode::Light)), NARROW);
    harness.run_steps(4);
    let stars: Vec<egui::Rect> = harness.get_all_by_label_contains("☆").chain(harness.get_all_by_label_contains("★")).map(|n| n.rect()).collect();
    assert!(!stars.is_empty());
    let star_left = stars.iter().map(|r| r.min.x).fold(f32::MAX, f32::min);
    for needle in ["aaaaaaaa", "relatorio-trimestral"] {
        let text = harness.get_by_label_contains(needle).rect();
        assert!(text.max.x <= star_left + 0.5, "{needle}: o texto {text:?} passa por baixo do ★ em {star_left}");
    }
}

/// Todo caractere que a interface pode desenhar (literais do código e catálogos) existe nas fontes: sem glyph a
/// tela mostraria a "caixinha".
#[test]
fn every_glyph_of_the_interface_exists() {
    let mut harness = harness(app("glyphs", Prefs::new("en", ThemeMode::Light)));
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<(char, String)> = Vec::new();
    for code in ["pt-br", "es"] {
        let path = root.join(format!("i18n/locales/{code}/localutils.tsv"));
        let text = std::fs::read_to_string(&path).expect("catálogo");
        // As linhas `@id` (o nome do idioma) não são texto de tela e já estão em fontes comuns; o resto é.
        for line in text.lines().filter(|line| !line.starts_with('@')) {
            found.extend(line.chars().filter(|c| !c.is_ascii()).map(|c| (c, format!("{code}: {line}"))));
        }
    }
    fn walk(dir: &Path, found: &mut Vec<(char, String)>) {
        for entry in std::fs::read_dir(dir).expect("src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path).expect("fonte");
                for literal in string_literals(&source) {
                    found.extend(literal.chars().filter(|c| !c.is_ascii()).map(|c| (c, format!("{}: {literal}", path.display()))));
                }
            }
        }
    }
    walk(&root.join("src"), &mut found);

    let font = egui::FontId::proportional(14.0);
    let mut missing: Vec<String> = Vec::new();
    for (c, origin) in found {
        // Espaços e controles (nbsp, tab…) não têm glyph próprio e não aparecem como caixinha.
        if c.is_whitespace() || c.is_control() {
            continue;
        }
        if !ctx.fonts_mut(|fonts| fonts.has_glyph(&font, c)) {
            missing.push(format!("{c} (U+{:04X}) em {origin}", u32::from(c)));
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "glyphs ausentes:\n{}", missing.join("\n"));
}

/// Os literais de texto de um arquivo-fonte, sem comentários (`//`) e sem os literais de caractere.
fn string_literals(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let mut chars = line.chars().peekable();
        let mut current: Option<String> = None;
        while let Some(c) = chars.next() {
            match (&mut current, c) {
                (Some(text), '\\') => {
                    if let Some(next) = chars.next() {
                        text.push(next);
                    }
                }
                (Some(_), '"') => out.extend(current.take()),
                (Some(text), other) => text.push(other),
                (None, '"') => current = Some(String::new()),
                (None, '/') if chars.peek() == Some(&'/') => break,
                (None, '\'') => {
                    // Literal de caractere (`'"'`, `'\n'`) ou lifetime: pula até o fecho quando houver.
                    let rest: String = chars.clone().take(3).collect();
                    if rest.starts_with("\"'") {
                        chars.next();
                        chars.next();
                    }
                }
                _ => {}
            }
        }
    }
    out
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
    // A tela principal com conteúdo e cada diálogo, nas duas larguras e nos dois temas.
    for (width, label) in [(NARROW, "340"), (1100.0, "1100")] {
        for (mode, name) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            for (what, setup) in tab_setups().into_iter().chain(dialog_setups()) {
                let mut app = loaded("shots-content", Prefs::new("en", mode));
                setup(&mut app);
                let mut harness = harness_sized(app, width);
                harness.run_steps(4);
                shot(&mut harness, &format!("{what}-{name}-{label}"));
            }
        }
    }
}
