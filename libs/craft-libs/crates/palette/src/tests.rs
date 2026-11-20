use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::{Command, Labels, Palette, filter};

fn commands() -> Vec<Command> {
    vec![
        Command::new("open", "Abrir…", "Arquivo").shortcut("Ctrl+O"),
        Command::new("save", "Salvar", "Arquivo").shortcut("Ctrl+S"),
        Command::new("settings", "Configurações…", "Editar"),
        Command::new("undo", "Desfazer", "Editar").enabled(false),
        Command::new("zoom-in", "Aumentar zoom", "Exibir"),
    ]
}

#[test]
fn an_empty_query_lists_everything_in_the_apps_order() {
    assert_eq!(filter("", &commands()), vec![0, 1, 2, 3, 4]);
}

#[test]
fn words_match_word_starts_ignoring_case_and_accents() {
    let commands = commands();
    assert_eq!(filter("configuracoes", &commands), vec![2]);
    assert_eq!(filter("CONF", &commands), vec![2]);
    // A word of the group counts too, and every word of the query has to match.
    assert_eq!(filter("arquivo sal", &commands), vec![1]);
    // Inside a word is not a match ("var" is in "Salvar" but starts no word).
    assert!(filter("var", &commands).is_empty());
}

#[test]
fn a_label_that_starts_with_the_query_comes_first() {
    let commands = vec![Command::new("a", "Ajustar zoom", "Exibir"), Command::new("b", "Zoom", "Exibir")];
    assert_eq!(filter("zoom", &commands), vec![1, 0]);
}

struct State {
    palette: Palette,
    commands: Vec<Command>,
    ran: Vec<String>,
}

fn harness() -> Harness<'static, State> {
    let mut palette = Palette::default();
    palette.open();
    let state = State { palette, commands: commands(), ran: Vec::new() };
    Harness::builder().with_size(egui::vec2(800.0, 600.0)).build_ui_state(
        |ui, state: &mut State| {
            let labels = Labels { placeholder: "Buscar comandos", empty: "Nada encontrado" };
            let ctx = ui.ctx().clone();
            if let Some(id) = state.palette.show(&ctx, &labels, &state.commands) {
                state.ran.push(id);
            }
        },
        state,
    )
}

/// Typing filters, Enter runs the first match, and the palette closes.
#[test]
fn typing_and_enter_run_the_first_match() {
    let mut harness = harness();
    harness.run_steps(2);
    harness.get_by_role(egui::accesskit::Role::TextInput).type_text("salv");
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().ran, vec!["save".to_string()]);
    assert!(!harness.state().palette.is_open());
}

/// The arrows move the selection; a disabled command does not run; Escape closes without running anything.
#[test]
fn arrows_move_disabled_commands_do_not_run_and_escape_closes() {
    let mut harness = harness();
    harness.run_steps(2);
    for _ in 0..3 {
        harness.key_press(egui::Key::ArrowDown);
        harness.run_steps(1);
    }
    // The fourth row is "Desfazer", disabled.
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().ran.is_empty());
    assert!(harness.state().palette.is_open());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().palette.is_open());
    assert!(harness.state().ran.is_empty());
}

#[test]
fn nothing_found_says_so() {
    let mut harness = harness();
    harness.run_steps(2);
    harness.get_by_role(egui::accesskit::Role::TextInput).type_text("xyz");
    harness.run_steps(2);
    let _ = harness.get_by_label("Nada encontrado");
}

/// The box has the keyboard as soon as the palette opens, and a long list scrolls: the arrows bring the selected
/// row into the visible part of the list (the wheel scrolls the same area).
#[test]
fn the_box_has_focus_and_a_long_list_scrolls_to_the_selection() {
    let mut palette = Palette::default();
    palette.open();
    let commands: Vec<Command> = (0..60).map(|i| Command::new(format!("c{i}"), format!("Command {i:02}"), "Group")).collect();
    let state = State { palette, commands, ran: Vec::new() };
    let mut harness = Harness::builder().with_size(egui::vec2(800.0, 600.0)).build_ui_state(
        |ui, state: &mut State| {
            let labels = Labels { placeholder: "Buscar comandos", empty: "Nada encontrado" };
            let ctx = ui.ctx().clone();
            if let Some(id) = state.palette.show(&ctx, &labels, &state.commands) {
                state.ran.push(id);
            }
        },
        state,
    );
    harness.run_steps(3);
    assert!(harness.get_by_role(egui::accesskit::Role::TextInput).is_focused(), "the search box takes the keyboard on open");
    let top = harness.get_by_label("Command 00").rect().top();
    for _ in 0..40 {
        harness.key_press(egui::Key::ArrowDown);
        harness.run_steps(1);
    }
    harness.run_steps(10);
    let selected = harness.get_by_label("Command 40").rect();
    let list_bottom = top + 320.0;
    assert!(
        selected.top() >= top - 1.0 && selected.bottom() <= list_bottom + 1.0,
        "row 40 scrolled into view: {selected:?}, list {top}..{list_bottom}"
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().ran, vec!["c40".to_string()]);
}

/// In a window narrower than the palette's measure, the box fits inside it (with the gutter) instead of being cut.
#[test]
fn the_palette_fits_a_narrow_window() {
    let mut palette = Palette::default();
    palette.open();
    let state = State { palette, commands: commands(), ran: Vec::new() };
    let mut harness = Harness::builder().with_size(egui::vec2(340.0, 500.0)).build_ui_state(
        |ui, state: &mut State| {
            let labels = Labels { placeholder: "Buscar comandos", empty: "Nada encontrado" };
            let ctx = ui.ctx().clone();
            let _ = state.palette.show(&ctx, &labels, &state.commands);
        },
        state,
    );
    harness.run_steps(3);
    let input = harness.get_by_role(egui::accesskit::Role::TextInput).rect();
    assert!(input.left() >= 0.0 && input.right() <= 340.0, "the box stays inside a 340 pt window: {input:?}");
}
