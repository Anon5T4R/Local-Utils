//! Tema do Local Utils: o modo da suíte (`craft_icons::ThemeMode` — Automático, Claro, Escuro) + `Tokens` (as
//! cores da identidade do app) aplicados por `set_visuals_of` nos DOIS temas; no Automático o egui troca sozinho
//! quando o SO muda. Widgets nunca hard-codam cor: leem `ui.visuals()` (o `Skin` do app) ou os Tokens.

use craft_icons::ThemeMode;
use egui::Color32;

/// A preferência do egui de um modo: o Automático continua automático (não resolve para fixo).
fn preference(mode: ThemeMode) -> egui::ThemePreference {
    match mode {
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::System => egui::ThemePreference::System,
    }
}

/// As cores da identidade do Local Utils; widgets nunca hard-codam cor.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    /// Fundo de painel/janela.
    pub base: Color32,
    /// Fundo "afundado": campos, cards, gráficos.
    pub input: Color32,
    /// Bordas sutis e fortes.
    pub border: Color32,
    pub border_strong: Color32,
    /// Texto principal.
    pub text: Color32,
    /// Hover/pressionado sobre superfícies.
    pub hover: Color32,
    pub pressed: Color32,
    /// Destaque (seleção, links, foco).
    pub accent: Color32,
    /// Sombras.
    pub shadow: Color32,
}

impl Tokens {
    pub fn light() -> Self {
        Tokens {
            dark: false,
            base: Color32::from_rgb(0xf2, 0xf3, 0xf5),
            input: Color32::from_rgb(0xff, 0xff, 0xff),
            border: Color32::from_black_alpha(25),
            border_strong: Color32::from_black_alpha(55),
            text: Color32::from_rgb(0x1f, 0x24, 0x2a),
            hover: Color32::from_black_alpha(14),
            pressed: Color32::from_black_alpha(28),
            accent: Color32::from_rgb(0x2f, 0x62, 0xb8),
            shadow: Color32::from_black_alpha(45),
        }
    }

    pub fn dark() -> Self {
        Tokens {
            dark: true,
            base: Color32::from_rgb(0x15, 0x1a, 0x1e),
            input: Color32::from_rgb(0x0e, 0x12, 0x16),
            border: Color32::from_white_alpha(30),
            border_strong: Color32::from_white_alpha(65),
            text: Color32::from_rgb(0xdf, 0xe5, 0xee),
            hover: Color32::from_white_alpha(16),
            pressed: Color32::from_white_alpha(30),
            accent: Color32::from_rgb(0x6e, 0x9b, 0xf0),
            shadow: Color32::from_black_alpha(140),
        }
    }
}

/// Aplica AMBOS os temas (`set_visuals_of` Light e Dark) + o modo. No Automático o egui alterna sozinho quando
/// o SO muda.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_visuals_of(egui::Theme::Light, visuals(&Tokens::light()));
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&Tokens::dark()));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size = egui::vec2(24.0, 22.0);
        s.spacing.menu_margin = egui::Margin::same(6);
    });
    ctx.options_mut(|o| {
        o.fallback_theme = egui::Theme::Dark;
        o.theme_preference = preference(mode);
    });
}

/// Visuals de um tema a partir dos Tokens (molde do local-office).
fn visuals(t: &Tokens) -> egui::Visuals {
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.base;
    v.window_fill = t.input;
    v.extreme_bg_color = t.input;
    v.faint_bg_color = t.base;
    v.code_bg_color = t.input;
    v.window_stroke = egui::Stroke::new(1.0, t.border_strong);
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    v.selection.bg_fill = t.accent.gamma_multiply(0.35);
    v.selection.stroke = egui::Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.inactive.bg_fill = t.input;
    v.widgets.inactive.weak_bg_fill = t.input;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, t.border_strong);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    v.widgets.active.bg_fill = t.pressed;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.active.corner_radius = egui::CornerRadius::same(4);
    v.widgets.open.bg_fill = t.pressed;
    v.widgets.open.weak_bg_fill = t.pressed;
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow };
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: t.shadow };
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_maps_to_its_egui_preference() {
        assert_eq!(preference(ThemeMode::System), egui::ThemePreference::System);
        assert_eq!(preference(ThemeMode::Light), egui::ThemePreference::Light);
        assert_eq!(preference(ThemeMode::Dark), egui::ThemePreference::Dark);
    }

    /// Contraste WCAG entre duas cores opacas.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn luminance(c: Color32) -> f32 {
            let channel = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        let (hi, lo) = (luminance(a).max(luminance(b)), luminance(a).min(luminance(b)));
        (hi + 0.05) / (lo + 0.05)
    }

    /// O texto do pill primário (sobre o accent CHEIO) e o accent como link/ícone sobre o fundo passam de 4,5:1
    /// (AA) nos dois temas.
    #[test]
    fn the_accent_is_readable_in_both_themes() {
        for tokens in [Tokens::light(), Tokens::dark()] {
            let skin = crate::ui::Skin::from_visuals(&visuals(&tokens));
            assert_eq!(skin.accent, tokens.accent, "o Skin usa o accent cheio");
            let on_pill = contrast(skin.accent, skin.on_accent);
            let on_base = contrast(skin.accent, tokens.base);
            assert!(on_pill >= 4.5, "dark={}: texto sobre o accent {on_pill:.2}:1", tokens.dark);
            assert!(on_base >= 4.5, "dark={}: accent sobre o fundo {on_base:.2}:1", tokens.dark);
        }
    }
}
