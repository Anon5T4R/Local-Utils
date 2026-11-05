//! Kit mínimo de widgets compartilhados: botões pill/ghost, campos de uma linha
//! e multilinha, cards, segmented control, badges, barras, a moldura dos
//! diálogos modais e as cores derivadas do tema.
//!
//! Molde do `main.rs` do Local-Keys (mesma suíte), adaptado ao fork do egui:
//! aqui não há `impl eframe::App` próprio, só helpers livres que recebem o
//! `&mut egui::Ui` do painel. Regra do kit: **nenhuma cor cravada** espalhada
//! pela UI — quem precisa de cor pede o [`Skin`] do tema vigente.
//!
//! A hierarquia visual da suíte (referência: a aba Clipboard): pills de nível 1
//! → [`segmented`] de nível 2 (menor, separado por [`hairline`]) → [`card`] com
//! o conteúdo → [`badge`] dos valores soltos.

use eframe::egui;

/// Recuo interno de um campo — molde do `FIELD_MARGIN` do local-pdf.
const FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(10, 6);
/// Altura externa mínima de um campo de uma linha.
const FIELD_MIN_HEIGHT: f32 = 30.0;
/// Recuo interno de um card (px, os quatro lados).
const CARD_PAD: i8 = 14;
/// Canto do card.
const CARD_RADIUS: u8 = 8;
/// Folga à direita da barra de progresso dentro do card (o trilho não encosta
/// na borda).
const PROGRESS_RIGHT_INSET: f32 = 3.0;
/// Corpo do [`segmented`] (nível 2) e o recuo horizontal de cada lado.
const SEGMENTED_FONT_SIZE: f32 = 12.0;
const SEGMENTED_PAD_X: f32 = 9.0;

/// Cores derivadas do tema vigente (nada fixo: a suíte tem mais de uma paleta).
#[derive(Clone, Copy)]
pub struct Skin {
    /// Fundo do cartão do diálogo (o "sunken" da paleta).
    pub card: egui::Color32,
    /// Fundo dos campos DENTRO do diálogo (o "base", o inverso do cartão).
    pub field: egui::Color32,
    /// Superfície ELEVADA (cards do conteúdo): um tom acima do fundo do painel
    /// nas duas polaridades — é o `skin.card` dos padrões da suíte.
    pub elevated: egui::Color32,
    /// Trilho das barras de progresso (mais forte que o `hover`, para a barra
    /// aparecer sobre o `elevated` do card).
    pub track: egui::Color32,
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
        // "Um tom acima" só existe como mistura: no escuro o card clareia um
        // pouco o painel, no claro ele vai pro branco. As duas contas saem do
        // `panel_fill`, então qualquer paleta nova funciona sem ajuste.
        let elevated =
            if v.dark_mode { v.panel_fill.lerp_to_gamma(egui::Color32::WHITE, 0.06) } else { v.panel_fill.lerp_to_gamma(egui::Color32::WHITE, 0.68) };
        // O trilho acompanha a polaridade: no escuro clareia (texto é claro),
        // no claro escurece.
        let track = elevated.lerp_to_gamma(text, 0.14);
        Self {
            card: v.extreme_bg_color,
            field: v.panel_fill,
            elevated,
            track,
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
    ghost_tint(ui, label, ink, ink)
}

/// Ação corriqueira de linha (remover, limpar, descartar): tinta do TEMA —
/// branca no escuro, preta no claro — em vez do vermelho. O vermelho fica
/// reservado pro que é destrutivo E confirmado (o `pill_danger` do modal).
/// No hover a tinta sobe pro `skin.text` cheio: o gamma abre um degrau de
/// contraste nas duas polaridades sem depender de cor fixa.
pub fn ghost_action(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let skin = Skin::of(ui);
    ghost_tint(ui, label, skin.text.gamma_multiply(0.82), skin.text)
}

/// Corpo do ghost: aloca a área, pinta o fundo do hover e o glifo na tinta do
/// repouso ou na do hover (`ghost_ink` usa a mesma nas duas).
fn ghost_tint(ui: &mut egui::Ui, label: &str, ink: egui::Color32, hover_ink: egui::Color32) -> egui::Response {
    let skin = Skin::of(ui);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 14.0, 24.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let ink = if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
        hover_ink
    } else {
        ink
    };
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

// ── navegação de nível 2 ─────────────────────────────────────────────────

/// Coluna central de largura limitada. O app roda maximizado: 1900px de linha
/// de conteúdo não se lê, então cada aba escolhe seu teto e o resto vira
/// margem simétrica. Devolve o que o conteúdo devolver.
pub fn column<R>(ui: &mut egui::Ui, max_width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let avail = ui.available_width();
    let width = avail.min(max_width).max(1.0);
    let side = ((avail - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(side);
        let height = ui.available_height().max(0.0);
        ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::top_down(egui::Align::Min), add).inner
    })
    .inner
}

/// Segmented control: a navegação de nível 2 (Temporizador/Pomodoro…), menor e
/// mais discreta que as [`pill`] de nível 1. Fica SEMPRE separada delas por um
/// [`hairline`] — duas fileiras iguais coladas viram ruído.
pub fn segmented(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    let ink = if selected { skin.text } else { skin.muted };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), segmented_font(), ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 2.0 * SEGMENTED_PAD_X, 24.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, selected, label));
    let fill = if selected {
        skin.hover
    } else if resp.hovered() && enabled {
        skin.hover.gamma_multiply(0.6)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    resp
}

/// Fonte do [`segmented`].
fn segmented_font() -> egui::FontId {
    egui::FontId::proportional(SEGMENTED_FONT_SIZE)
}

/// Largura que um [`segmented`] vai ocupar — o chamador mede a fileira antes de
/// centralizá-la (o layout do egui não mede por nós).
pub fn segmented_width(ui: &egui::Ui, label: &str) -> f32 {
    text_width(ui, label, segmented_font()) + 2.0 * SEGMENTED_PAD_X
}

/// Fio de separação entre os dois níveis de navegação (e entre seções).
pub fn hairline(ui: &mut egui::Ui) {
    let skin = Skin::of(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(0), skin.border);
}

// ── superfícies ──────────────────────────────────────────────────────────

/// Card do conteúdo: superfície elevada, canto 8, borda sutil e recuo interno
/// de 14. `width` é a largura EXTERNA — o corpo já recebe o recuo descontado.
pub fn card<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_h(ui, width, 0.0, add)
}

/// Largura ÚTIL do corpo de um card de largura externa `width` (recuo dos dois
/// lados + a borda de 1px). Quem calcula layout dentro do card parte daqui.
pub fn card_inner_width(width: f32) -> f32 {
    (width - 2.0 * f32::from(CARD_PAD) - 2.0).max(1.0)
}

/// Altura ÚTIL do corpo de um card de altura externa `height`.
pub fn card_inner_height(height: f32) -> f32 {
    (height - 2.0 * f32::from(CARD_PAD) - 2.0).max(0.0)
}

/// Altura EXTERNA de um card cujo corpo mede `inner_height` — o inverso de
/// [`card_inner_height`], para quem dimensiona o card a partir do conteúdo.
pub fn card_outer_height(inner_height: f32) -> f32 {
    inner_height + 2.0 * f32::from(CARD_PAD) + 2.0
}

/// [`card`] com altura mínima externa: as grades de cards (2×N) precisam das
/// células da mesma linha com a mesma altura.
pub fn card_h<R>(ui: &mut egui::Ui, width: f32, min_height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let skin = Skin::of(ui);
    // Borda de 1px dos dois lados entra na conta da largura externa.
    let inner_w = card_inner_width(width);
    let inner_h = card_inner_height(min_height);
    egui::Frame::NONE
        .fill(skin.elevated)
        .stroke(egui::Stroke::new(1.0_f32, skin.border))
        .corner_radius(egui::CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(CARD_PAD))
        .show(ui, |ui| {
            // O ui de um Frame HERDA o layout do pai (egui `new_child`:
            // `layout.unwrap_or(*self.layout())`): criado dentro de um
            // `horizontal_top` (as grades 2×N de cards), o corpo do card
            // fluía na HORIZONTAL — conteúdo emfileirado, chips numa linha
            // só transbordando e o card vizinho empurrado pra fora. Todo
            // card é coluna, aqui e sempre.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(inner_w);
                if inner_h > 0.0 {
                    ui.set_min_height(inner_h);
                }
                add(ui)
            })
            .inner
        })
        .inner
}

// ── valores soltos ───────────────────────────────────────────────────────

/// Chip-pill de valor (CPU/RAM de um processo, bateria, ⏰ de um alarme): fundo
/// `hover`, forma de pill e tinta própria. Não é clicável, mas aceita tooltip.
pub fn badge(ui: &mut egui::Ui, text: &str, ink: egui::Color32) -> egui::Response {
    chip(ui, text, ink, 11.0, 7.0, 20.0)
}

/// [`badge`] grande (a fase do pomodoro, que é o dado principal do painel).
pub fn badge_lg(ui: &mut egui::Ui, text: &str, ink: egui::Color32) -> egui::Response {
    chip(ui, text, ink, 14.0, 12.0, 28.0)
}

fn chip(ui: &mut egui::Ui, text: &str, ink: egui::Color32, font_size: f32, pad_x: f32, height: f32) -> egui::Response {
    let skin = Skin::of(ui);
    let font = egui::FontId::proportional(font_size);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 2.0 * pad_x, height), egui::Sense::hover());
    // Raio = metade da altura: a forma é pill nas duas variantes.
    ui.painter().rect_filled(rect, egui::CornerRadius::same((height / 2.0) as u8), skin.hover);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    resp
}

/// Barra de progresso do card: trilho arredondado de 8px e preenchimento cheio
/// na largura disponível (dentro do card, nunca a janela toda).
pub fn progress(ui: &mut egui::Ui, frac: f32, color: egui::Color32) {
    let skin = Skin::of(ui);
    // Folga à direita: o preenchimento não pode encostar na borda do card
    // (pago no print da aba Sistema — a barra ia até o limite do conteúdo).
    let width = (ui.available_width() - PROGRESS_RIGHT_INSET).max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), egui::Sense::hover());
    let radius = egui::CornerRadius::same(4);
    ui.painter().rect_filled(rect, radius, skin.track);
    let frac = frac.clamp(0.0, 1.0);
    if frac > 0.0 {
        let filled = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac, rect.height()));
        ui.painter().rect_filled(filled, radius, color);
    }
}

// ── botões de palco ──────────────────────────────────────────────────────

/// Botão de ação do palco central (altura 44, forma de pill, largura fixa).
/// `primary` preenche com o accent; o secundário fica só no contorno.
pub fn big(ui: &mut egui::Ui, label: &str, primary: bool, width: f32) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    let (fill, ink, outline) = if primary { (skin.accent, skin.on_accent, false) } else { (egui::Color32::TRANSPARENT, skin.text, true) };
    let ink = if enabled { ink } else { skin.muted };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width.max(galley.size().x + 32.0), 44.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let hovered = resp.hovered() && enabled;
    let fill = if outline {
        if hovered {
            skin.hover
        } else {
            egui::Color32::TRANSPARENT
        }
    } else if hovered {
        fill.gamma_multiply(1.12)
    } else {
        fill
    };
    let stroke = if outline { egui::Stroke::new(1.0_f32, skin.border) } else { egui::Stroke::NONE };
    ui.painter().rect(rect, egui::CornerRadius::same(22), fill, stroke, egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    resp
}

/// Natureza de uma tecla do teclado da calculadora: dígito (superfície clara),
/// operador (superfície funda) ou a tecla de resultado (accent).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PadKey {
    Digit,
    Op,
    Accent,
}

/// Tecla do keypad da calculadora (altura 52, canto 8). Só EDITA a string da
/// expressão — o parser nunca vê um caminho diferente do campo de texto.
pub fn pad(ui: &mut egui::Ui, label: &str, kind: PadKey, width: f32) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    let (fill, ink) = match kind {
        // Dígitos mais claros que os operadores: a diferença é o `lerp` pro
        // preto, que escurece nas duas polaridades (no claro o dígito é branco).
        PadKey::Digit => (skin.elevated, skin.text),
        PadKey::Op => (skin.elevated.lerp_to_gamma(egui::Color32::BLACK, 0.16), skin.text),
        PadKey::Accent => (skin.accent, skin.on_accent),
    };
    let ink = if enabled { ink } else { skin.muted };
    let font = egui::FontId::proportional(17.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width.max(galley.size().x + 12.0), 52.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let hovered = resp.hovered() && enabled;
    let fill = if hovered { fill.gamma_multiply(1.12) } else { fill };
    ui.painter().rect(rect, egui::CornerRadius::same(8), fill, egui::Stroke::new(1.0_f32, skin.border), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    resp
}

/// Largura que um [`pill`]/[`segmented`]/[`badge`] vai ocupar — para centralizar
/// uma fileira antes de desenhá-la (o layout do egui não mede por nós).
pub fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE).size().x
}
