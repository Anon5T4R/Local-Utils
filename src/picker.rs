//! Aba Cor: seletor de cor (hex/RGB/HSL) com conta-gotas da tela e uma fileira
//! de cores recentes persistida no config.
//!
//! O conta-gotas reusa a maquinaria da captura (`capture.rs`): congela o
//! desktop virtual ([`capture::Frozen`]) e abre um viewport próprio sem
//! decoração, sempre no topo, do tamanho do desktop virtual. Sobre a tela
//! congelada ele desenha uma LUPA (recorte ampliado da própria textura, com
//! `NEAREST` — o pixel fica quadrado, que é o que se quer inspecionar), um
//! crosshair e a leitura hex no rodapé da lupa. Clique escolhe, Esc cancela; o
//! mapeamento cursor → pixel é por RAZÃO, igual ao recorte da captura.
//!
//! Por que isso não é uma janela de seleção de cor do SO: o app é offline e
//! sem diálogos nativos de cor; e o conta-gotas precisa do MEU overlay pra
//! mostrar a lupa e a leitura em tempo real.

use std::sync::{Arc, Mutex};

use eframe::egui;
use utils_ui::i18n::{self, Key, Lang};

use crate::capture::{self, Frozen};
use crate::clipboard_ui::copy_new_text;
use crate::history::History;
use crate::ui::{self as kit, Skin};

/// Teto dos recentes. O config é um JSON inteiro: 32 cores é uso real e não
/// engorda a gravação.
pub(crate) const RECENT_MAX: usize = 32;
/// Lado da lupa, em points.
const LOUPE_SIDE: f32 = 120.0;
/// Ampliação da lupa (points da lupa por pixel de imagem).
const LOUPE_ZOOM: f32 = 8.0;
/// Distância entre o cursor e a lupa.
const LOUPE_GAP: f32 = 18.0;
/// Altura da barra do hex embaixo da lupa.
const HEX_BAR_H: f32 = 26.0;
/// Título PRÓPRIO de propósito: `winctl::discover` acha a janela principal pelo
/// título exato ("Local Utils") — o overlay não pode se passar por ela.
const OVERLAY_TITLE: &str = "Local Utils — conta-gotas";

/// Id do viewport do conta-gotas (fixo: a mesma abertura reusa a janela).
fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("local-utils-picker")
}

/// Qual dos três campos o usuário editou por último. Serve pra não reescrever
/// embaixo do dedo o campo que está sendo digitado.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Hex,
    Rgb,
    Hsl,
}

impl Format {
    fn key(self) -> Key {
        match self {
            Format::Hex => Key::LabelHex,
            Format::Rgb => Key::LabelRgb,
            Format::Hsl => Key::LabelHsl,
        }
    }
}

// ── estado compartilhado com o viewport ──────────────────────────────────

/// Estado do overlay (o callback do viewport exige `Fn + Send + Sync +
/// 'static`, então vai num `Arc<Mutex<..>>`, como na captura e no popup).
#[derive(Default)]
struct Overlay {
    open: bool,
    /// Retrato congelado do desktop virtual.
    frame: Option<Frozen>,
    /// Textura do retrato: sobe no primeiro frame do viewport.
    tex: Option<egui::TextureHandle>,
    /// O `ViewportCommand::Focus` já foi pedido nesta abertura.
    focus_sent: bool,
    /// Cor escolhida, esperando o app pegar.
    result: Option<egui::Color32>,
}

impl Overlay {
    /// Fecha e solta o retrato (um desktop 4K são dezenas de MB). O `result`
    /// NÃO é limpo: é o que o app vem buscar logo depois (o `begin` limpa).
    fn close(&mut self, ctx: &egui::Context) {
        self.open = false;
        self.frame = None;
        self.tex = None;
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Handle do conta-gotas (vive na aba Cor).
#[derive(Clone, Default)]
pub struct Eyedropper {
    state: Arc<Mutex<Overlay>>,
}

impl Eyedropper {
    /// Congela a tela e abre o overlay. Erro = nada abriu (vira status na aba).
    pub fn begin(&self, ctx: &egui::Context) -> Result<(), String> {
        if self.is_open() {
            return Ok(());
        }
        let frame = Frozen::capture(ctx)?;
        let Ok(mut st) = self.state.lock() else {
            return Err("estado do conta-gotas indisponível".into());
        };
        st.frame = Some(frame);
        st.tex = None;
        st.focus_sent = false;
        st.result = None;
        st.open = true;
        ctx.request_repaint();
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.state.lock().is_ok_and(|st| st.open)
    }

    /// Pede foco UMA vez por abertura: sem ele o Esc não chega ao overlay
    /// quando o SO recusa o primeiro plano (mesma razão da captura/popup).
    fn take_focus_request(&self) -> bool {
        match self.state.lock() {
            Ok(mut st) if st.open && !st.focus_sent => {
                st.focus_sent = true;
                true
            }
            _ => false,
        }
    }

    /// Cor escolhida no overlay (o app adota e troca pra aba Cor).
    pub fn take_result(&self) -> Option<egui::Color32> {
        self.state.lock().ok().and_then(|mut st| st.result.take())
    }
}

// ── estado da aba ────────────────────────────────────────────────────────

/// Estado da aba Cor entre frames.
pub struct PickerState {
    current: egui::Color32,
    /// Mais recente primeiro, sem repetição (cap [`RECENT_MAX`]).
    recent: Vec<egui::Color32>,
    hex: String,
    rgb: String,
    hsl: String,
    /// O campo com texto inválido (mostra o aviso em vez de mentir a cor).
    bad: Option<Format>,
    /// O texto editado já virou cor e ainda merece entrar nos recentes quando
    /// o campo perder o foco (digitar não pode gravar a cada tecla).
    pending: bool,
    /// Overlay do conta-gotas.
    hot: Eyedropper,
}

impl PickerState {
    /// Semeia os recentes com o que está no config (`recent_colors`, hex). Cor
    /// inicial = o recente mais novo; sem nenhum, branco.
    pub fn from_config(recent: &[String]) -> Self {
        let mut state = Self {
            current: egui::Color32::WHITE,
            recent: Vec::new(),
            hex: String::new(),
            rgb: String::new(),
            hsl: String::new(),
            bad: None,
            pending: false,
            hot: Eyedropper::default(),
        };
        // Config antigo/corrompido não derruba a aba: entra o que parseia,
        // preservando a ordem (o índice 0 continua sendo o mais novo).
        for text in recent {
            if let Some((r, g, b)) = parse_hex(text) {
                state.seed_recent(egui::Color32::from_rgb(r, g, b));
            }
        }
        state.current = state.recent.first().copied().unwrap_or(egui::Color32::WHITE);
        state.refresh();
        state
    }

    /// Texto (hex) dos recentes, na ordem — é o que vai pro config.
    pub fn recent_hex(&self) -> Vec<String> {
        self.recent.iter().map(|c| hex_text(*c)).collect()
    }

    /// O overlay do conta-gotas está aberto? (o app chama o `show_overlay`)
    pub fn eyedropping(&self) -> bool {
        self.hot.is_open()
    }

    /// Cor escolhida no conta-gotas (só existe uma vez: o resultado é consumido).
    pub fn take_picked(&self) -> Option<egui::Color32> {
        self.hot.take_result()
    }

    /// Adota uma cor vinda de fora do desenho da aba (conta-gotas): vira a
    /// atual, entra nos recentes e os três formatos são reescritos.
    pub fn adopt(&mut self, color: egui::Color32) {
        self.current = color;
        self.bad = None;
        self.pending = false;
        self.refresh();
        self.push_recent(color);
    }

    /// Reescreve os três campos a partir da cor atual.
    fn refresh(&mut self) {
        self.hex = hex_text(self.current);
        self.rgb = rgb_text(self.current);
        self.hsl = hsl_text(self.current);
    }

    /// Cor digitada num campo: atualiza a atual e os OUTROS dois campos (o que
    /// está sendo digitado fica como o usuário escreveu).
    fn apply(&mut self, color: egui::Color32, edited: Format) {
        self.current = color;
        self.bad = None;
        self.pending = true;
        if edited != Format::Hex {
            self.hex = hex_text(color);
        }
        if edited != Format::Rgb {
            self.rgb = rgb_text(color);
        }
        if edited != Format::Hsl {
            self.hsl = hsl_text(color);
        }
    }

    /// Cor escolhida pelo usuário (conta-gotas, recente): vai pro topo dos
    /// recentes, sem duplicar.
    fn push_recent(&mut self, color: egui::Color32) {
        self.recent.retain(|c| *c != color);
        self.recent.insert(0, color);
        self.recent.truncate(RECENT_MAX);
    }

    /// Entrada vinda do config: preserva a ordem (o `push_recent` inverteria a
    /// lista inteira) e descarta duplicata.
    fn seed_recent(&mut self, color: egui::Color32) {
        if self.recent.len() < RECENT_MAX && !self.recent.contains(&color) {
            self.recent.push(color);
        }
    }

    /// Texto do campo (pro botão de copiar).
    fn text(&self, format: Format) -> &str {
        match format {
            Format::Hex => &self.hex,
            Format::Rgb => &self.rgb,
            Format::Hsl => &self.hsl,
        }
    }
}

/// O que a aba devolve pro app.
#[derive(Default)]
pub struct Outcome {
    /// Uma cor foi copiada: cria entrada Text no histórico de clipboard.
    pub history_changed: bool,
    /// Os recentes mudaram: regrava o config.
    pub cfg_changed: bool,
    pub status: Option<String>,
}

// ── aba ──────────────────────────────────────────────────────────────────

/// Desenha a aba Cor. `history` é o histórico de clipboard (copiar um formato
/// cria entrada Text nova, como o "copiar" das outras ferramentas).
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut PickerState, history: &mut History) -> Outcome {
    let mut out = Outcome::default();
    let skin = Skin::of(ui);

    ui.horizontal(|ui| {
        // Swatch grande da cor atual (a leitura que importa, sem precisar ler
        // campo nenhum).
        let (rect, _) = ui.allocate_exact_size(egui::vec2(88.0, 88.0), egui::Sense::hover());
        ui.painter().rect(rect, egui::CornerRadius::same(10), state.current, egui::Stroke::new(1.0_f32, skin.border), egui::StrokeKind::Inside);
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(hex_text(state.current)).size(22.0).strong().color(skin.text));
            ui.add_space(8.0);
            if kit::pill(ui, i18n::t(lang, Key::CaptureColor), true).clicked() {
                if let Err(e) = state.hot.begin(ui.ctx()) {
                    out.status = Some(format!("⚠ {}: {e}", i18n::t(lang, Key::CaptureFailed)));
                }
            }
        });
    });
    ui.add_space(12.0);

    for format in [Format::Hex, Format::Rgb, Format::Hsl] {
        format_row(ui, lang, state, format, history, &mut out);
    }

    ui.add_space(12.0);
    ui.label(egui::RichText::new(i18n::t(lang, Key::RecentColors)).small().color(skin.muted));
    ui.add_space(4.0);

    // Clique num recente adota a cor DEPOIS do laço: adotar mexe na própria
    // lista que está sendo percorrida.
    let mut picked: Option<egui::Color32> = None;
    ui.horizontal_wrapped(|ui| {
        for color in &state.recent {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            ui.painter().rect(rect, egui::CornerRadius::same(5), *color, egui::Stroke::new(1.0_f32, skin.border), egui::StrokeKind::Inside);
            if resp.on_hover_text(hex_text(*color)).clicked() {
                picked = Some(*color);
            }
        }
    });
    if let Some(color) = picked {
        state.adopt(color);
        out.cfg_changed = true;
    }

    out
}

/// Uma linha de formato: rótulo, campo editável e o botão de copiar.
fn format_row(ui: &mut egui::Ui, lang: Lang, state: &mut PickerState, format: Format, history: &mut History, out: &mut Outcome) {
    let skin = Skin::of(ui);
    ui.horizontal(|ui| {
        ui.add_sized([40.0, 20.0], egui::Label::new(egui::RichText::new(i18n::t(lang, format.key())).color(skin.muted)));

        let field = match format {
            Format::Hex => &mut state.hex,
            Format::Rgb => &mut state.rgb,
            Format::Hsl => &mut state.hsl,
        };
        let resp = ui.add(kit::line(field).desired_width(180.0));

        // Digitou: parseia o campo editado; sucesso atualiza os outros dois,
        // falha deixa o texto como está e marca o aviso (o usuário pode estar
        // no meio de "#FF").
        if resp.changed() {
            let parsed = match format {
                Format::Hex => parse_hex(state.text(format)),
                Format::Rgb => parse_rgb(state.text(format)),
                Format::Hsl => parse_hsl(state.text(format)),
            };
            match parsed {
                Some((r, g, b)) => state.apply(egui::Color32::from_rgb(r, g, b), format),
                None => {
                    state.bad = Some(format);
                    state.pending = false;
                }
            }
        }
        // Saiu do campo com uma cor válida digitada: aí sim entra nos recentes
        // (digitar não pode gravar o config a cada tecla).
        if resp.lost_focus() && state.pending && state.bad != Some(format) {
            let color = state.current;
            state.pending = false;
            state.push_recent(color);
            out.cfg_changed = true;
        }

        if kit::ghost(ui, "📋").on_hover_text(i18n::t(lang, Key::CopyValue)).clicked() {
            let text = state.text(format).to_string();
            let (changed, status) = copy_new_text(history, lang, &text);
            out.history_changed |= changed;
            out.status = Some(status);
        }
        if state.bad == Some(format) {
            ui.label(egui::RichText::new(format!("⚠ {}", i18n::t(lang, Key::InvalidColor))).small().color(skin.danger));
        }
    });
}

// ── overlay do conta-gotas ───────────────────────────────────────────────

/// Desenha o overlay do conta-gotas (só quando aberto). O app chama isto a
/// cada frame enquanto a aba estiver com o overlay de pé; parar de chamar
/// derruba a janela (mesma mecânica da captura).
pub fn show_overlay(ctx: &egui::Context, lang: Lang, picker: &PickerState) {
    let (pos, size) = match picker.hot.state.lock() {
        Ok(st) if st.open => match st.frame.as_ref() {
            Some(f) => (f.pos_pt, f.size_pt),
            None => return,
        },
        _ => return,
    };
    let state = Arc::clone(&picker.hot.state);
    let hint = i18n::t(lang, Key::PickerHint);
    let builder = egui::ViewportBuilder::default()
        .with_title(OVERLAY_TITLE)
        .with_position(pos)
        .with_inner_size(size)
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        // O desktop virtual pode ser MAIOR que um monitor (mesma razão da
        // captura): sem isto o winit corta a janela no tamanho do monitor.
        .with_clamp_size_to_monitor_size(false)
        .with_active(true);
    ctx.show_viewport_deferred(viewport_id(), builder, move |ui, class| {
        if class == egui::ViewportClass::EmbeddedWindow {
            // Backend sem viewports múltiplos: não há tela congelada em janela
            // própria — cancelar é melhor que virar um retângulo na UI.
            if let Ok(mut st) = state.lock() {
                st.close(ui.ctx());
            }
            return;
        }
        draw(ui, hint, &state);
    });
    // Foco explícito: criar com `active` nem sempre basta no Windows (trava de
    // primeiro plano) e sem foco o Esc não volta pra cá.
    if picker.hot.take_focus_request() {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Focus);
    }
}

/// Corpo do overlay: tela congelada, crosshair, lupa e leitura hex.
fn draw(ui: &mut egui::Ui, hint: &str, state: &Arc<Mutex<Overlay>>) {
    let ctx = ui.ctx().clone();
    let Ok(mut st) = state.lock() else { return };
    if !st.open {
        return;
    }

    // Alt+F4 / fechamento pelo SO: o estado tem de acompanhar, senão o app
    // recria o viewport no frame seguinte.
    if ctx.input(|i| i.viewport().close_requested()) {
        st.close(&ctx);
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        st.close(&ctx);
        return;
    }

    // Textura do retrato: sobe uma vez por captura (o `ColorImage` sai num
    // passo à parte pra não segurar o empréstimo de `st.frame` junto com o de
    // `st.tex`).
    let fresh = match (st.frame.as_ref(), st.tex.is_some()) {
        (Some(f), false) => Some(egui::ColorImage::from_rgba_unmultiplied([f.image.width() as usize, f.image.height() as usize], f.image.as_raw())),
        _ => None,
    };
    if let Some(color) = fresh {
        st.tex = Some(ctx.load_texture("picker-frozen", color, egui::TextureOptions::NEAREST));
    }

    // Cor escolhida neste frame (aplicada depois de soltar o empréstimo do
    // painel, quando já podemos fechar o overlay).
    let mut chosen: Option<egui::Color32> = None;
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(egui::Color32::BLACK)).show(ui, |ui| {
        let full = ui.max_rect();
        let painter = ui.painter_at(full);
        let Some(frame) = st.frame.as_ref() else { return };
        let image_rect = frame.image_rect(full);
        let Some(tex) = st.tex.as_ref() else { return };
        painter.image(tex.id(), image_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);

        let resp = ui.interact(full, ui.scope_id().with("picker-pick"), egui::Sense::click());
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let pick = match pointer {
            Some(p) => capture::pixel_at(image_rect, &frame.image, p).map(|(x, y)| {
                let px = frame.image.get_pixel(x, y);
                (p, x, y, egui::Color32::from_rgb(px[0], px[1], px[2]))
            }),
            None => None,
        };

        if let Some((p, x, y, color)) = pick {
            // Crosshair de tela inteira: orienta o ponteiro longe da lupa.
            let stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(150));
            painter.line_segment([egui::pos2(full.min.x, p.y), egui::pos2(full.max.x, p.y)], stroke);
            painter.line_segment([egui::pos2(p.x, full.min.y), egui::pos2(p.x, full.max.y)], stroke);
            loupe(&painter, full, Picked { tex: tex.id(), frame, at: (x, y), color, cursor: p });
            if resp.clicked() {
                chosen = Some(color);
            }
        }

        // Dica + Esc, no alto (mesmo desenho da captura).
        let text = format!("{hint}  ·  Esc");
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(14.0), egui::Color32::WHITE);
        let pad = egui::vec2(12.0, 6.0);
        let box_rect = egui::Rect::from_center_size(
            egui::pos2(full.center().x, full.min.y + 8.0 + (galley.size().y + pad.y * 2.0) / 2.0),
            galley.size() + pad * 2.0,
        );
        painter.rect_filled(box_rect, egui::CornerRadius::same(6), egui::Color32::from_black_alpha(190));
        painter.galley(box_rect.min + pad, galley, egui::Color32::WHITE);
    });

    if let Some(color) = chosen {
        st.result = Some(color);
        st.close(&ctx);
    }
}

/// O que a lupa precisa desenhar num frame: a textura do retrato, o pixel alvo
/// e onde o cursor está.
struct Picked<'a> {
    tex: egui::TextureId,
    frame: &'a Frozen,
    at: (u32, u32),
    color: egui::Color32,
    cursor: egui::Pos2,
}

/// Lupa: recorte ampliado (`NEAREST`) em volta do pixel sob o cursor, com
/// crosshair central e a leitura hex no rodapé.
fn loupe(painter: &egui::Painter, full: egui::Rect, picked: Picked<'_>) {
    let (x, y) = picked.at;
    let (w, h) = (picked.frame.image.width() as f32, picked.frame.image.height() as f32);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let rect = loupe_rect(full, picked.cursor);

    // Recorte da imagem (± meio lado, em px de imagem). As coordenadas podem
    // sair de [0,1] nas bordas: o sampler repete a borda e o pixel continua
    // CENTRADO na lupa (é o que importa pra mirar).
    let half = (LOUPE_SIDE / LOUPE_ZOOM) / 2.0;
    let cx = x as f32 + 0.5;
    let cy = y as f32 + 0.5;
    let uv = egui::Rect::from_min_max(egui::pos2((cx - half) / w, (cy - half) / h), egui::pos2((cx + half) / w, (cy + half) / h));

    painter.rect_filled(rect, egui::CornerRadius::same(4), egui::Color32::from_black_alpha(210));
    painter.image(picked.tex, rect, uv, egui::Color32::WHITE);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(220)),
        egui::StrokeKind::Outside,
    );

    // Crosshair + a célula de UM pixel de imagem (o alvo exato), em preto e
    // branco pra aparecer sobre qualquer cor.
    let center = rect.center();
    let hair = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(170));
    painter.line_segment([egui::pos2(rect.min.x, center.y), egui::pos2(rect.max.x, center.y)], hair);
    painter.line_segment([egui::pos2(center.x, rect.min.y), egui::pos2(center.x, rect.max.y)], hair);
    let cell = egui::Rect::from_center_size(center, egui::vec2(LOUPE_ZOOM, LOUPE_ZOOM));
    painter.rect_stroke(cell, egui::CornerRadius::ZERO, egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(200)), egui::StrokeKind::Outside);
    painter.rect_stroke(cell, egui::CornerRadius::ZERO, egui::Stroke::new(1.0_f32, egui::Color32::WHITE), egui::StrokeKind::Inside);

    // Rodapé: quadradinho da cor + hex.
    let bar = egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.max.y + 4.0), egui::vec2(rect.width(), HEX_BAR_H));
    painter.rect_filled(bar, egui::CornerRadius::same(4), egui::Color32::from_black_alpha(210));
    let chip = egui::Rect::from_min_size(bar.min + egui::vec2(5.0, 5.0), egui::vec2(HEX_BAR_H - 10.0, HEX_BAR_H - 10.0));
    painter.rect(
        chip,
        egui::CornerRadius::same(3),
        picked.color,
        egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(90)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(chip.max.x + 8.0, bar.center().y),
        egui::Align2::LEFT_CENTER,
        hex_text(picked.color),
        egui::FontId::monospace(13.0),
        egui::Color32::WHITE,
    );
}

/// Onde a lupa nasce: ao lado do cursor, invertendo perto das bordas da tela
/// pra nunca sair do overlay (a barra do hex conta na conta).
fn loupe_rect(full: egui::Rect, cursor: egui::Pos2) -> egui::Rect {
    let total_h = LOUPE_SIDE + 4.0 + HEX_BAR_H;
    let mut min = cursor + egui::vec2(LOUPE_GAP, LOUPE_GAP);
    if min.x + LOUPE_SIDE > full.max.x - 4.0 {
        min.x = cursor.x - LOUPE_GAP - LOUPE_SIDE;
    }
    if min.y + total_h > full.max.y - 4.0 {
        min.y = cursor.y - LOUPE_GAP - total_h;
    }
    // Nunca fora do overlay (cursor colado no canto superior esquerdo).
    let min = egui::pos2(min.x.max(full.min.x + 4.0), min.y.max(full.min.y + 4.0));
    egui::Rect::from_min_size(min, egui::vec2(LOUPE_SIDE, LOUPE_SIDE))
}

// ── texto e conversão de cor (puro, testável) ────────────────────────────

/// `#RRGGBB` (maiúsculas, com `#`).
pub(crate) fn hex_text(color: egui::Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

/// `R, G, B` em 0–255.
pub(crate) fn rgb_text(color: egui::Color32) -> String {
    format!("{}, {}, {}", color.r(), color.g(), color.b())
}

/// `H°, S%, L%` (H 0–360, S/L 0–100), arredondado ao inteiro.
pub(crate) fn hsl_text(color: egui::Color32) -> String {
    let (h, s, l) = rgb_to_hsl(color.r(), color.g(), color.b());
    format!("{h:.0}°, {:.0}%, {:.0}%", s * 100.0, l * 100.0)
}

/// Um dígito hexa ASCII.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `#RGB`, `#RRGGBB` ou sem `#`. Espaços nas pontas são tolerados (texto
/// colado); qualquer outra coisa é erro.
pub(crate) fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let digits = text.trim().trim_start_matches('#').as_bytes();
    match digits.len() {
        // Cada dígito vale por dois: `#F00` = `#FF0000`.
        3 => Some((hex_digit(digits[0])? * 17, hex_digit(digits[1])? * 17, hex_digit(digits[2])? * 17)),
        6 => Some((
            hex_digit(digits[0])? * 16 + hex_digit(digits[1])?,
            hex_digit(digits[2])? * 16 + hex_digit(digits[3])?,
            hex_digit(digits[4])? * 16 + hex_digit(digits[5])?,
        )),
        _ => None,
    }
}

/// Separa um texto em pedaços por vírgula/espaço (símbolos de `°`/`%` viram
/// separador também), descartando pedaços vazios.
fn parts(text: &str) -> Vec<&str> {
    text.split(|c: char| c == ',' || c == '°' || c == '%' || c.is_whitespace()).filter(|s| !s.is_empty()).collect()
}

/// `R, G, B` (ou `R G B`), cada canal 0–255.
pub(crate) fn parse_rgb(text: &str) -> Option<(u8, u8, u8)> {
    let parts = parts(text);
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse::<u8>().ok()?;
    let g = parts[1].parse::<u8>().ok()?;
    let b = parts[2].parse::<u8>().ok()?;
    Some((r, g, b))
}

/// `H°, S%, L%` (aceita sem os símbolos). H 0–360, S/L 0–100.
pub(crate) fn parse_hsl(text: &str) -> Option<(u8, u8, u8)> {
    let parts = parts(text);
    if parts.len() != 3 {
        return None;
    }
    let h = parts[0].parse::<f32>().ok()?;
    let s = parts[1].parse::<f32>().ok()?;
    let l = parts[2].parse::<f32>().ok()?;
    if !(0.0..=360.0).contains(&h) || !(0.0..=100.0).contains(&s) || !(0.0..=100.0).contains(&l) {
        return None;
    }
    Some(hsl_to_rgb(h, s / 100.0, l / 100.0))
}

/// RGB (0–255) → HSL com H em 0–360 e S/L em 0–1.
pub(crate) fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = f32::from(r) / 255.0;
    let gf = f32::from(g) / 255.0;
    let bf = f32::from(b) / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta <= f32::EPSILON {
        // Cinza: matiz indefinido, saturação zero (0 é o valor canônico).
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { delta / (2.0 - max - min) } else { delta / (max + min) };
    let h = if (max - rf).abs() <= f32::EPSILON {
        ((gf - bf) / delta + if gf < bf { 6.0 } else { 0.0 }) * 60.0
    } else if (max - gf).abs() <= f32::EPSILON {
        ((bf - rf) / delta + 2.0) * 60.0
    } else {
        ((rf - gf) / delta + 4.0) * 60.0
    };
    (h, s, l)
}

/// HSL (H 0–360, S/L 0–1) → RGB 0–255.
pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    if s <= 0.0 {
        let v = to_u8(l);
        return (v, v, v);
    }
    let h = h.rem_euclid(360.0) / 360.0;
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    (to_u8(hue(p, q, h + 1.0 / 3.0)), to_u8(hue(p, q, h)), to_u8(hue(p, q, h - 1.0 / 3.0)))
}

/// Canal de matiz auxiliar do HSL → RGB.
fn hue(p: f32, q: f32, t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// 0–1 → 0–255 com arredondamento.
fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_aceita_tres_e_seis_digitos() {
        assert_eq!(parse_hex("#FF0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex("00ff7f"), Some((0, 255, 127)));
        assert_eq!(parse_hex(" #F0A "), Some((255, 0, 170)));
        assert_eq!(parse_hex("#12345"), None, "5 dígitos não é hex de cor");
        assert_eq!(parse_hex("#GG0000"), None);
        assert_eq!(parse_hex(""), None);
        // Multibyte não pode estourar o slice (o parser olha bytes).
        assert_eq!(parse_hex("#ção"), None);
    }

    #[test]
    fn rgb_aceita_virgula_espaco_e_recusa_excesso() {
        assert_eq!(parse_rgb("255, 0, 0"), Some((255, 0, 0)));
        assert_eq!(parse_rgb("12 34 56"), Some((12, 34, 56)));
        assert_eq!(parse_rgb("255,0"), None);
        assert_eq!(parse_rgb("256, 0, 0"), None, "canal fora de 0–255");
        assert_eq!(parse_rgb("-1, 0, 0"), None);
        assert_eq!(parse_rgb("1, 2, 3, 4"), None);
    }

    #[test]
    fn hsl_ida_e_volta_nas_cores_primarias() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), (0, 0, 255));
        // Cinza: matiz 0 e saturação 0.
        assert_eq!(rgb_to_hsl(128, 128, 128), (0.0, 0.0, 128.0 / 255.0));
        // Texto gerado volta a ser aceito (o usuário pode copiar/colar o campo).
        // O texto é arredondado ao inteiro, então a volta não é exata.
        let text = hsl_text(egui::Color32::from_rgb(30, 120, 200));
        let (r, g, b) = parse_hsl(&text).expect("o texto que a aba gera parseia");
        assert!(r.abs_diff(30) <= 8 && g.abs_diff(120) <= 8 && b.abs_diff(200) <= 8, "{text} → {r},{g},{b}");
    }

    #[test]
    fn formatos_de_texto_sao_estaveis() {
        let c = egui::Color32::from_rgb(10, 20, 30);
        assert_eq!(hex_text(c), "#0A141E");
        assert_eq!(rgb_text(c), "10, 20, 30");
        assert_eq!(parse_hex(&hex_text(c)), Some((10, 20, 30)));
        assert_eq!(parse_rgb(&rgb_text(c)), Some((10, 20, 30)));
    }

    #[test]
    fn recentes_sem_duplicata_com_cap_e_ordem_nova_primeiro() {
        let mut state = PickerState::from_config(&[]);
        assert!(state.recent.is_empty());
        state.push_recent(egui::Color32::RED);
        state.push_recent(egui::Color32::GREEN);
        state.push_recent(egui::Color32::RED);
        assert_eq!(state.recent, vec![egui::Color32::RED, egui::Color32::GREEN], "o repetido volta pro topo, sem duplicar");
        for i in 0..RECENT_MAX + 5 {
            state.push_recent(egui::Color32::from_rgb(i as u8, 0, 0));
        }
        assert_eq!(state.recent.len(), RECENT_MAX);
    }

    #[test]
    fn config_semeia_os_recentes_na_ordem_salva() {
        let state = PickerState::from_config(&["#FF0000".to_string(), "lixo".to_string(), "#00FF00".to_string()]);
        assert_eq!(state.recent, vec![egui::Color32::RED, egui::Color32::GREEN], "ordem preservada e lixo descartado");
        assert_eq!(hex_text(state.current), "#FF0000", "a cor inicial é a mais recente");
        assert_eq!(state.hex, "#FF0000");
        assert_eq!(state.rgb, "255, 0, 0");
    }
}
