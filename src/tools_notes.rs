//! Aba Notas (scratchpad): um campo multilinha gigante, persistido no config
//! (`utils_ui::config::Config::notes`).
//!
//! Autosave DEBOUNCED: digitar marca "sujo" com o instante; o app grava quando o
//! usuário para por [`AUTOSAVE_MS`] — nenhuma gravação por tecla (o config é um
//! JSON inteiro). O prazo corre fora da aba também ([`NotesState::due`]): trocar
//! de aba no meio da digitação não pode deixar o texto só na memória.

use utils_ui::i18n::{self, Key, Lang};

use crate::history::now_ms;
use crate::ui as kit;

/// Quanto tempo sem digitar até o autosave.
pub const AUTOSAVE_MS: u64 = 1_000;

/// Estado da aba entre frames.
#[derive(Default)]
pub struct NotesState {
    /// Instante da última tecla (`None` = nada pendente).
    dirty_since: Option<u64>,
}

impl NotesState {
    /// Marca que o texto mudou (chamado a cada edição do campo).
    fn touch(&mut self, now: u64) {
        self.dirty_since = Some(now);
    }

    /// Tem gravação pendente? (o app insiste num repaint curto enquanto isso)
    pub fn pending(&self) -> bool {
        self.dirty_since.is_some()
    }

    /// Já passou o debounce? (o app grava o config e chama [`NotesState::saved`])
    pub fn due(&self, now: u64) -> bool {
        self.dirty_since.is_some_and(|since| now.saturating_sub(since) >= AUTOSAVE_MS)
    }

    /// A gravação aconteceu (ou o texto voltou ao que está em disco).
    pub fn saved(&mut self) {
        self.dirty_since = None;
    }
}

/// Desenha a aba Notas. `text` é `cfg.notes` (o campo edita direto).
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut NotesState, text: &mut String) {
    let now = now_ms();
    // O campo ocupa tudo menos a linha do rodapé (contagem), que precisa caber
    // na janela mínima do app.
    let height = (ui.available_height() - 26.0).max(120.0);
    let resp = ui.add_sized(egui::vec2(ui.available_width(), height), kit::block(text).hint_text(i18n::t(lang, Key::NotesHint)));
    if resp.changed() {
        state.touch(now);
    }
    ui.add_space(4.0);

    // Char e linha no rodapé da aba (singular/plural na mão: o catálogo não tem
    // forma plural ainda).
    let chars = text.chars().count();
    let lines = text.lines().count();
    let counts = format!(
        "{} · {}",
        i18n::t(lang, Key::NotesChars).replace("{n}", &chars.to_string()),
        i18n::t(lang, Key::NotesLines).replace("{n}", &lines.to_string())
    );
    ui.label(egui::RichText::new(counts).small().weak());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_do_autosave() {
        let mut state = NotesState::default();
        assert!(!state.pending(), "aba recém-aberta não tem nada pendente");
        assert!(!state.due(10_000));

        state.touch(1_000);
        assert!(state.pending());
        assert!(!state.due(1_500), "ainda dentro da janela do debounce");
        assert!(state.due(2_000), "parou de digitar por um segundo: hora de gravar");

        state.saved();
        assert!(!state.pending());
        assert!(!state.due(9_999));
    }

    #[test]
    fn contagem_de_chars_e_linhas() {
        // Mesma conta do rodapé (chars unicode contam 1; linha vazia no fim não
        // cria linha nova).
        let text = "ação\nsegunda";
        assert_eq!(text.chars().count(), 12);
        assert_eq!(text.lines().count(), 2);
        assert_eq!("".lines().count(), 0);
    }
}
