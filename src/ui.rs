//! Kit mínimo de widgets compartilhados: botões pill/ghost, campos de uma linha
//! e multilinha, a moldura dos diálogos modais e as cores derivadas do tema.
//!
//! Molde do `main.rs` do Local-Keys (mesma suíte), adaptado ao fork do egui:
//! aqui não há `impl eframe::App` próprio, só helpers livres que recebem o
//! `&mut egui::Ui` do painel. Regra do kit: **nenhuma cor cravada** espalhada
//! pela UI — quem precisa de cor pede o [`Skin`] do tema vigente.

use eframe::egui;

/// Recuo interno de um campo — molde do `FIELD_MARGIN` do local-pdf.
const FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(10, 6);
/// Altura externa mínima de um campo de uma linha.
const FIELD_MIN_HEIGHT: f32 = 30.0;

/// Cores derivadas do tema vigente (nada fixo: a suíte tem mais de uma paleta).
#[derive(Clone, Copy)]
pub struct Skin {
    /// Fundo do cartão do diálogo (o "sunken" da paleta).
    pub card: egui::Color32,
    /// Fundo dos campos DENTRO do diálogo (o "base", o inverso do cartão).
    pub field: egui::Color32,
    /// Destaque SÓLIDO do tema (estrela de pin, foco da linha, pill primário).
    /// Vem do `hyperlink_color` — o `selection.bg_fill` é o accent lavado
    /// (alpha baixo) e sumia como ícone/contorno (pagou no QA do v0.1.0).
    pub accent: egui::Color32,
    /// Texto sobre `accent` (pill primário).
    pub on_accent: egui::Color32,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub border: egui::Color32,
    /// Lavagem de hover (ghost buttons e linha da lista).
    pub hover: egui::Color32,
    /// Tom de ação destrutiva (do próprio tema).
    pub danger: egui::Color32,
}

impl Skin {
    /// Cores do tema do `ui` corrente.
    pub fn of(ui: &egui::Ui) -> Self {
        Self::from_visuals(ui.visuals())
    }

    /// Cores de um `Visuals` qualquer (o `modal_frame` recebe só o `Context`).
    pub fn from_visuals(v: &egui::Visuals) -> Self {
        let text = v.override_text_color.unwrap_or(v.widgets.noninteractive.fg_stroke.color);
        let accent = v.hyperlink_color;
        let danger = v.error_fg_color;
        Self {
            card: v.extreme_bg_color,
            field: v.panel_fill,
            accent,
            on_accent: ink_on(accent),
            text,
            muted: text.gamma_multiply(0.6),
            border: if v.dark_mode { egui::Color32::from_white_alpha(30) } else { egui::Color32::from_black_alpha(30) },
            hover: if v.dark_mode { egui::Color32::from_white_alpha(18) } else { egui::Color32::from_black_alpha(14) },
            danger,
        }
    }
}

/// Tinta legível sobre `fill` (claro em fundo escuro e vice-versa).
fn ink_on(fill: egui::Color32) -> egui::Color32 {
    if luminance(fill) > 150.0 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    }
}

/// Luminância perceptual (0–255) — decide texto claro ou escuro sobre o fundo.
fn luminance(c: egui::Color32) -> f32 {
    0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())
}

/// Campo de uma linha com altura de clique decente.
pub fn line(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(FIELD_MARGIN).min_size(egui::vec2(0.0, FIELD_MIN_HEIGHT))
}

/// Campo multilinha (edição de texto longo) com o mesmo recuo horizontal.
pub fn block(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(text).margin(FIELD_MARGIN)
}

/// Moldura do diálogo: cartão com canto de 10, borda sutil e a sombra do tema.
pub fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    let style = ctx.style_of(ctx.theme());
    let skin = Skin::from_visuals(&style.visuals);
    egui::Frame::NONE
        .fill(skin.card)
        .stroke(egui::Stroke::new(1.0_f32, skin.border))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(20, 16))
        .shadow(style.visuals.window_shadow)
}

/// Visuals de dentro do diálogo: campos com contorno e no tom "base" (o cartão
/// é o "sunken" — sem isso o campo some no fundo do modal).
pub fn begin_dialog(ui: &mut egui::Ui) {
    let skin = Skin::of(ui);
    let v = ui.visuals_mut();
    v.extreme_bg_color = skin.field;
    v.selection.stroke = egui::Stroke::new(1.5_f32, skin.accent);
    let w = &mut v.widgets;
    w.inactive.bg_stroke = egui::Stroke::new(1.0_f32, skin.border);
    w.inactive.weak_bg_fill = skin.field;
    w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, skin.muted);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = egui::CornerRadius::same(4);
    }
    ui.spacing_mut().interact_size.y = FIELD_MIN_HEIGHT;
}

/// Botão pill: `primary` preenche com o accent, o secundário fica no contorno.
/// Desenhado à mão porque o `egui::Button` não tem raio/altura própria.
pub fn pill(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let skin = Skin::of(ui);
    if primary {
        pill_colored(ui, label, skin.accent, skin.on_accent, false)
    } else {
        pill_colored(ui, label, egui::Color32::TRANSPARENT, skin.text, true)
    }
}

/// Pill de ação destrutiva (vermelho do tema — legível nas duas polaridades).
pub fn pill_danger(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let skin = Skin::of(ui);
    pill_colored(ui, label, skin.danger, ink_on(skin.danger), false)
}

/// Pill com cores explícitas; `outline` desenha só a borda.
fn pill_colored(ui: &mut egui::Ui, label: &str, fill: egui::Color32, ink: egui::Color32, outline: bool) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    let ink = if enabled { ink } else { skin.muted };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 28.0, 28.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let hovered = resp.hovered() && enabled;
    let fill = if !enabled {
        fill.gamma_multiply(0.45)
    } else if hovered {
        fill.gamma_multiply(1.12)
    } else {
        fill.gamma_multiply(0.9)
    };
    let fill = if outline {
        if hovered {
            skin.hover
        } else {
            egui::Color32::TRANSPARENT
        }
    } else {
        fill
    };
    let stroke = if outline { egui::Stroke::new(1.0_f32, skin.border) } else { egui::Stroke::NONE };
    ui.painter().rect(rect, egui::CornerRadius::same(14), fill, stroke, egui::StrokeKind::Inside);
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

/// Ação de linha: transparente até o hover (o "ghost button" do local-pdf).
pub fn ghost(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ghost_ink(ui, label, Skin::of(ui).muted)
}

/// [`ghost`] com tinta própria (o ★ de pin usa o accent quando ativo).
pub fn ghost_ink(ui: &mut egui::Ui, label: &str, ink: egui::Color32) -> egui::Response {
    let skin = Skin::of(ui);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 14.0, 24.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}
