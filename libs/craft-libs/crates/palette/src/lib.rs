//! The suite's command palette: a search box over the app's own commands, the same in every app.
//!
//! The app lists its commands each frame (id, label, group, shortcut, enabled) and runs the id the palette returns;
//! the palette owns only the box, the filter and the keyboard. Opening it is the app's call: Ctrl+K
//! ([`Palette::shortcut_pressed`]) where the app has not given Ctrl+K another meaning, and always from the interface
//! (the search icon of the top bar).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// One command the app offers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Command {
    /// What the app gets back when the user runs it.
    pub id: String,
    /// The command's name, in the app's language (the menu item's text).
    pub label: String,
    /// Where it lives (the menu's name), shown beside the label and searched too.
    pub group: String,
    /// The shortcut as the app shows it in menus, or empty.
    pub shortcut: String,
    /// A disabled command is listed (so the user learns it exists) but cannot run.
    pub enabled: bool,
}

impl Command {
    pub fn new(id: impl Into<String>, label: impl Into<String>, group: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), group: group.into(), shortcut: String::new(), enabled: true }
    }

    #[must_use]
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = shortcut.into();
        self
    }

    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// The palette's words, in the app's language.
pub struct Labels<'a> {
    /// The box's placeholder ("Search commands").
    pub placeholder: &'a str,
    /// Shown when nothing matches.
    pub empty: &'a str,
}

/// The palette's state; keep one in the app.
#[derive(Debug, Default)]
pub struct Palette {
    open: bool,
    query: String,
    /// Index into the filtered list.
    selected: usize,
    /// Focus the box on the first frame after opening.
    focus: bool,
}

impl Palette {
    /// The width of the box: one measure in every app.
    pub const WIDTH: f32 = 520.0;

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens on an empty query, with the box focused.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn toggle(&mut self) {
        if self.open { self.close() } else { self.open() }
    }

    /// Ctrl+K (Cmd+K on macOS) was pressed this frame; the key is consumed. Only for apps where Ctrl+K is free.
    pub fn shortcut_pressed(ctx: &egui::Context) -> bool {
        ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K))
    }

    /// Draws the palette (when open) and returns the id of the command the user ran, if any. The palette closes after
    /// a run, on Escape and on a click outside it.
    pub fn show(&mut self, ctx: &egui::Context, labels: &Labels<'_>, commands: &[Command]) -> Option<String> {
        if !self.open {
            return None;
        }
        let found = filter(&self.query, commands);
        self.selected = self.selected.min(found.len().saturating_sub(1));

        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if escape {
            self.close();
            return None;
        }
        if down && !found.is_empty() {
            self.selected = (self.selected + 1) % found.len();
        }
        if up && !found.is_empty() {
            self.selected = self.selected.checked_sub(1).unwrap_or(found.len() - 1);
        }
        let mut ran = None;
        if enter
            && let Some(command) = found.get(self.selected).and_then(|&index| commands.get(index))
            && command.enabled
        {
            ran = Some(command.id.clone());
        }

        // One measure in every app, but never wider than the window (a narrow window keeps a 16 pt gutter each side).
        let width = Self::WIDTH.min(ctx.content_rect().width() - 32.0).max(160.0);
        let area = egui::Area::new(egui::Id::unique("craft-palette"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    let edit = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(labels.placeholder).desired_width(f32::INFINITY));
                    if std::mem::take(&mut self.focus) {
                        edit.request_focus();
                    }
                    if edit.changed() {
                        self.selected = 0;
                    }
                    ui.add_space(4.0);
                    if found.is_empty() {
                        ui.label(egui::RichText::new(labels.empty).weak());
                        return;
                    }
                    egui::ScrollArea::vertical().max_height(320.0).id_salt("craft-palette-list").show(ui, |ui| {
                        for (row, &index) in found.iter().enumerate() {
                            let Some(command) = commands.get(index) else { continue };
                            let selected = row == self.selected;
                            let response = command_row(ui, command, selected);
                            if selected && (up || down) {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() && command.enabled {
                                ran = Some(command.id.clone());
                            }
                            // The mouse picks a row only when it moves, so a resting pointer does not undo the arrows.
                            if response.hovered() && ui.input(|i| i.pointer.is_moving()) {
                                self.selected = row;
                            }
                        }
                    });
                });
            });
        let clicked_outside =
            ctx.input(|i| i.pointer.any_pressed()) && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pos| !area.response.rect.contains(pos));
        if ran.is_some() || clicked_outside {
            self.close();
        }
        ran
    }
}

/// One row: the label, its group, and the shortcut at the right; the selected row is highlighted.
fn command_row(ui: &mut egui::Ui, command: &Command, selected: bool) -> egui::Response {
    let height = ui.spacing().interact_size.y.max(22.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    // The row is painted, so it says what it is: screen readers (and the tests) read the command's name.
    response.widget_info(|| egui::WidgetInfo::selected(egui::accesskit::Role::ListBoxOption, command.enabled, selected, &command.label));
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        if selected {
            ui.painter().rect_filled(rect, visuals.widgets.hovered.corner_radius, visuals.selection.bg_fill.gamma_multiply(0.35));
        }
        let text = if command.enabled { visuals.text_color() } else { visuals.weak_text_color() };
        let weak = visuals.weak_text_color();
        let body = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let left = rect.left_center() + egui::vec2(6.0, 0.0);
        let label = ui.painter().text(left, egui::Align2::LEFT_CENTER, &command.label, body, text);
        if !command.group.is_empty() {
            ui.painter().text(label.right_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, &command.group, small.clone(), weak);
        }
        if !command.shortcut.is_empty() {
            ui.painter().text(rect.right_center() - egui::vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, &command.shortcut, small, weak);
        }
    }
    response
}

/// The commands that match `query`, best first: every word of the query must start a word of the label or the
/// group (case and accents ignored); a label that starts with the query comes first, then the app's own order.
pub fn filter(query: &str, commands: &[Command]) -> Vec<usize> {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    let mut found: Vec<(u8, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            let label = fold(&command.label);
            let haystack = format!("{label} {}", fold(&command.group));
            let targets: Vec<&str> = haystack.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
            let all = words.iter().all(|word| targets.iter().any(|target| target.starts_with(word.as_str())));
            all.then(|| (u8::from(!label.starts_with(words.first().map_or("", String::as_str))), index))
        })
        .collect();
    found.sort_by_key(|&(rank, index)| (rank, index));
    found.into_iter().map(|(_, index)| index).collect()
}

/// Lower case, Latin accents removed (so "configuracoes" finds "Configurações").
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests;
