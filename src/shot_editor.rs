//! Aba Screenshot: editor de anotação da captura pendente.
//!
//! O modelo é VETORIAL: a imagem base fica intacta e as anotações vivem num
//! `Vec<Annotation>` com desfazer. Recortar NÃO é destrutivo — vira o estado
//! `crop` do [`Edits`], composto só no [`render`] (exportação/preview), e por
//! isso também é desfazível.
//!
//! O canvas desenha o RESULTADO do [`render`] como textura (regerada só quando
//! algo muda, nunca por frame): a prévia fica 1:1 com o arquivo exportado, sem
//! um segundo caminho de desenho pra divergir. O pan/zoom segue o molde do
//! viewer do local-image — roda do mouse ancorada no cursor. A ferramenta
//! Select mexe no que já existe: arrastar SOBRE uma anotação a move (um passo
//! de desfazer por arraste), Del apaga a selecionada; arrastar em área vazia
//! continua sendo pan.
//!
//! O [`render`] é puro (nada de SO, nada de egui além das cores) e é ele que os
//! testes cobrem: tamanho, região afetada por cada tipo de anotação, crop e
//! desfazer.

use eframe::egui;
use image::{imageops, RgbaImage};
use utils_ui::i18n::{self, Key, Lang};

// `ascent`, `h_advance`, `glyph_id` e `outline_glyph` são métodos de TRAIT.
use ab_glyph::{Font, ScaleFont};

use crate::clipboard;
use crate::history::{self, Entry, Kind};
use crate::ui as kit;

/// Fonte do texto anotado: a MESMA Ubuntu-Light que o epaint embute pro app
/// inteiro (fork em `libs/egui`). Lida direto do fork por `include_bytes!` —
/// nenhuma fonte nova entra no repo.
const FONT_TTF: &[u8] = include_bytes!("../libs/egui/crates/epaint_default_fonts/fonts/Ubuntu-Light.ttf");

/// Paleta curta da barra; o resto sai do seletor livre do egui. O branco no fim
/// é o caso "anotar por cima de captura escura" (contraste no fundo escuro).
const PALETTE: [egui::Color32; 7] = [
    egui::Color32::from_rgb(230, 57, 70),
    egui::Color32::from_rgb(244, 162, 97),
    egui::Color32::from_rgb(255, 214, 10),
    egui::Color32::from_rgb(42, 157, 143),
    egui::Color32::from_rgb(69, 123, 157),
    egui::Color32::from_rgb(38, 38, 38),
    egui::Color32::WHITE,
];
/// Passos de desfazer guardados. Cada passo é uma cópia da lista — barata pra
/// o punhado de anotações de uma captura.
const UNDO_MAX: usize = 64;
/// Faixa do zoom do canvas.
const ZOOM_MIN: f32 = 0.05;
const ZOOM_MAX: f32 = 16.0;
/// Espessura e intensidade iniciais.
const THICK_DEFAULT: f32 = 3.0;
const STRENGTH_DEFAULT: u32 = 8;
/// Tamanho inicial do texto anotado (px da imagem).
const TEXT_DEFAULT: f32 = 28.0;
/// Abaixo disto um arraste não vira anotação (clique acidental).
const MIN_DRAG: f32 = 3.0;
/// Tolerância do hit-test em px de TELA: convertida pra px da imagem na escala
/// atual, com piso de 1 px de imagem (no zoom alto o alvo não some).
const HIT_TOL_SCREEN: f32 = 6.0;

/// Ponto em PIXELS da imagem base.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Retângulo em pixels da imagem base (inteiro e normalizado).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Uma anotação. Coordenadas SEMPRE em px da imagem base — o crop é aplicado
/// como deslocamento negativo no [`render`], não reescreve nada.
#[derive(Clone, PartialEq, Debug)]
pub enum Annotation {
    Rect { a: Pt, b: Pt, cor: egui::Color32, espessura: f32 },
    Arrow { a: Pt, b: Pt, cor: egui::Color32, espessura: f32 },
    Text { pos: Pt, cor: egui::Color32, tam: f32, texto: String },
    Pixelate { rect: Rect, forca: u32 },
    Blur { rect: Rect, forca: u32 },
}

impl Annotation {
    /// Cópia deslocada em px da imagem base. Mosaico/desfoque guardam canto
    /// inteiro: anda só o canto e o w/h fica.
    fn translated(&self, dx: f32, dy: f32) -> Annotation {
        match self {
            Annotation::Rect { a, b, cor, espessura } => {
                Annotation::Rect { a: Pt::new(a.x + dx, a.y + dy), b: Pt::new(b.x + dx, b.y + dy), cor: *cor, espessura: *espessura }
            }
            Annotation::Arrow { a, b, cor, espessura } => {
                Annotation::Arrow { a: Pt::new(a.x + dx, a.y + dy), b: Pt::new(b.x + dx, b.y + dy), cor: *cor, espessura: *espessura }
            }
            Annotation::Text { pos, cor, tam, texto } => {
                Annotation::Text { pos: Pt::new(pos.x + dx, pos.y + dy), cor: *cor, tam: *tam, texto: texto.clone() }
            }
            Annotation::Pixelate { rect, forca } => Annotation::Pixelate { rect: rect_shifted(*rect, dx, dy), forca: *forca },
            Annotation::Blur { rect, forca } => Annotation::Blur { rect: rect_shifted(*rect, dx, dy), forca: *forca },
        }
    }

    /// Caixa da anotação em px da base — só pro contorno de seleção.
    fn bounds(&self) -> (Pt, Pt) {
        match self {
            Annotation::Rect { a, b, espessura, .. } | Annotation::Arrow { a, b, espessura, .. } => {
                // O traço é centrado na linha: meia espessura pra fora dela.
                let d = espessura * 0.5 + 1.0;
                (Pt::new(a.x.min(b.x) - d, a.y.min(b.y) - d), Pt::new(a.x.max(b.x) + d, a.y.max(b.y) + d))
            }
            Annotation::Text { pos, tam, texto, .. } => (Pt::new(pos.x, pos.y), Pt::new(pos.x + text_width(texto, *tam), pos.y + *tam)),
            Annotation::Pixelate { rect, .. } | Annotation::Blur { rect, .. } => {
                (Pt::new(rect.x as f32, rect.y as f32), Pt::new((rect.x + rect.w) as f32, (rect.y + rect.h) as f32))
            }
        }
    }

    /// O ponto cai na anotação? `tol` já em px da imagem; a espessura do
    /// próprio traço entra por cima (é o que o usuário vê pintado).
    fn hit(&self, p: Pt, tol: f32) -> bool {
        match self {
            Annotation::Rect { a, b, espessura, .. } => {
                let t = tol + espessura * 0.5;
                let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
                let (x1, y1) = (a.x.max(b.x), a.y.max(b.y));
                let cantos = [Pt::new(x0, y0), Pt::new(x1, y0), Pt::new(x1, y1), Pt::new(x0, y1)];
                [(0_usize, 1_usize), (1, 2), (2, 3), (3, 0)].iter().any(|&(i, j)| dist_to_segment(p, cantos[i], cantos[j]) <= t)
            }
            Annotation::Arrow { a, b, espessura, .. } => {
                let t = tol + espessura * 0.5;
                if dist_to_segment(p, *a, *b) <= t {
                    return true;
                }
                // A ponta pinta duas linhas (mesma geometria do `arrow`).
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = dx.hypot(dy);
                if len < 1.0 {
                    return false;
                }
                let head = (len * 0.25).clamp(12.0, 60.0) + espessura;
                let base = dy.atan2(dx);
                [-1.0_f32, 1.0].iter().any(|s| {
                    let th = base + s * 25.0_f32.to_radians();
                    dist_to_segment(p, *b, Pt::new(b.x - head * th.cos(), b.y - head * th.sin())) <= t
                })
            }
            Annotation::Text { pos, tam, texto, .. } => {
                // APROXIMAÇÃO: o desenho real (ab_glyph) usa o avanço de cada
                // glifo; aqui a caixa é estimada em 0.6*tam por caractere e
                // `tam` de altura de linha. Só o hit-test usa isto — nenhum
                // pixel exportado depende da estimativa.
                let w = text_width(texto, *tam);
                p.x >= pos.x - tol && p.x <= pos.x + w + tol && p.y >= pos.y - tol && p.y <= pos.y + tam + tol
            }
            Annotation::Pixelate { rect, .. } | Annotation::Blur { rect, .. } => {
                let (x0, y0) = (rect.x as f32, rect.y as f32);
                let (x1, y1) = (x0 + rect.w as f32, y0 + rect.h as f32);
                p.x >= x0 - tol && p.x <= x1 + tol && p.y >= y0 - tol && p.y <= y1 + tol
            }
        }
    }
}

/// Largura ESTIMADA do texto (ver [`Annotation::hit`]).
fn text_width(texto: &str, tam: f32) -> f32 {
    texto.chars().count() as f32 * 0.6 * tam
}

/// Distância de um ponto ao segmento (0 quando cai dentro dele).
fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let (px, py) = (p.x - a.x, p.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 <= f32::EPSILON { 0.0 } else { ((px * vx + py * vy) / len2).clamp(0.0, 1.0) };
    (px - vx * t).hypot(py - vy * t)
}

/// Canto do retângulo deslocado e arredondado (w/h intactos).
fn rect_shifted(r: Rect, dx: f32, dy: f32) -> Rect {
    let x = (r.x as f32 + dx).round();
    let y = (r.y as f32 + dy).round();
    Rect { x: x.max(0.0) as u32, y: y.max(0.0) as u32, w: r.w, h: r.h }
}

/// Lista de anotações + crop + pilha de desfazer. Puro: nenhum tipo de SO.
#[derive(Default, Clone)]
pub struct Edits {
    list: Vec<Annotation>,
    crop: Option<Rect>,
    /// Cada passo guarda a lista e o crop de ANTES da mutação.
    undo: Vec<(Vec<Annotation>, Option<Rect>)>,
}

impl Edits {
    pub fn annotations(&self) -> &[Annotation] {
        &self.list
    }

    pub fn crop(&self) -> Option<Rect> {
        self.crop
    }

    pub fn push(&mut self, annotation: Annotation) {
        self.snapshot();
        self.list.push(annotation);
    }

    pub fn set_crop(&mut self, crop: Option<Rect>) {
        self.snapshot();
        self.crop = crop;
    }

    /// Tira todas as anotações (o recorte fica: são coisas diferentes).
    pub fn clear(&mut self) {
        if self.list.is_empty() {
            return;
        }
        self.snapshot();
        self.list.clear();
    }

    /// Desfaz o último passo. `false` = nada pra desfazer.
    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some((list, crop)) => {
                self.list = list;
                self.crop = crop;
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Anotação sob o ponto (px da base), da MAIS RECENTE pra mais antiga: o
    /// que está por cima é o que o clique pega.
    pub fn hit_test(&self, p: Pt, tol: f32) -> Option<usize> {
        self.list.iter().enumerate().rev().find(|(_, an)| an.hit(p, tol)).map(|(idx, _)| idx)
    }

    /// Fecha um passo de desfazer sem mudar nada: o arraste inteiro vira UM
    /// passo, aberto no primeiro deslocamento (não por frame).
    pub fn checkpoint(&mut self) {
        self.snapshot();
    }

    /// Repõe a anotação `idx` como `origem` deslocada de (dx, dy) — o delta é
    /// TOTAL desde o começo do arraste; assim o arredondamento da região não
    /// acumula (com delta por frame o arraste travaria no zoom alto).
    pub fn translate(&mut self, idx: usize, origem: &Annotation, dx: f32, dy: f32) -> bool {
        match self.list.get_mut(idx) {
            Some(slot) => {
                *slot = origem.translated(dx, dy);
                true
            }
            None => false,
        }
    }

    /// Remove uma anotação (desfazível). `false` = índice fora da lista.
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= self.list.len() {
            return false;
        }
        self.snapshot();
        self.list.remove(idx);
        true
    }

    fn snapshot(&mut self) {
        self.undo.push((self.list.clone(), self.crop));
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
    }
}

/// Compõe o resultado final: recorte + anotações. PURA e testável.
pub fn render(base: &RgbaImage, crop: Option<Rect>, annotations: &[Annotation]) -> RgbaImage {
    let (mut out, ox, oy) = match crop {
        Some(c) => {
            let c = clamp_rect(base, c);
            if c.w == 0 || c.h == 0 {
                (base.clone(), 0.0, 0.0)
            } else {
                (imageops::crop_imm(base, c.x, c.y, c.w, c.h).to_image(), c.x as f32, c.y as f32)
            }
        }
        None => (base.clone(), 0.0, 0.0),
    };
    for annotation in annotations {
        match annotation {
            Annotation::Rect { a, b, cor, espessura } => rect_outline(&mut out, shift(*a, ox, oy), shift(*b, ox, oy), *espessura, rgba(*cor)),
            Annotation::Arrow { a, b, cor, espessura } => arrow(&mut out, shift(*a, ox, oy), shift(*b, ox, oy), *espessura, rgba(*cor)),
            Annotation::Text { pos, cor, tam, texto } => draw_text(&mut out, shift(*pos, ox, oy), *tam, rgba(*cor), texto),
            Annotation::Pixelate { rect, forca } => {
                let (x, y, w, h) = shift_rect(*rect, ox, oy);
                pixelate(&mut out, x, y, w, h, *forca);
            }
            Annotation::Blur { rect, forca } => {
                let (x, y, w, h) = shift_rect(*rect, ox, oy);
                blur(&mut out, x, y, w, h, *forca);
            }
        }
    }
    out
}

/// Ferramenta ativa da barra.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    /// Seleciona e arrasta anotações existentes; arrastar em área vazia dá
    /// pan. A roda dá zoom em qualquer ferramenta.
    #[default]
    Select,
    Rect,
    Arrow,
    Text,
    Pixelate,
    Blur,
    Crop,
}

impl Tool {
    fn label(self) -> Key {
        match self {
            Tool::Select => Key::ToolSelect,
            Tool::Rect => Key::ToolRect,
            Tool::Arrow => Key::ToolArrow,
            Tool::Text => Key::ToolText,
            Tool::Pixelate => Key::ToolPixelate,
            Tool::Blur => Key::ToolBlur,
            Tool::Crop => Key::ToolCrop,
        }
    }

    /// Todas, na ordem da barra.
    const ALL: [Tool; 7] = [Tool::Select, Tool::Rect, Tool::Arrow, Tool::Text, Tool::Pixelate, Tool::Blur, Tool::Crop];
}

/// Arraste de uma anotação em andamento (ferramenta Select).
struct Moving {
    idx: usize,
    /// Cópia de ANTES do arraste: o deslocamento é sempre medido a partir
    /// dela, nunca acumulado frame a frame.
    origem: Annotation,
    /// Ponto da imagem onde o arraste começou.
    inicio: Pt,
    /// O passo de desfazer só abre no primeiro deslocamento real.
    snapshot: bool,
}

/// Estado da captura pendente (v0.3 guarda UMA: captura nova substitui).
pub struct Shot {
    base: RgbaImage,
    edits: Edits,
    tool: Tool,
    /// Anotação selecionada (só existe com `Tool::Select`).
    selected: Option<usize>,
    /// Arraste de anotação em andamento.
    moving: Option<Moving>,
    cor: egui::Color32,
    espessura: f32,
    forca: u32,
    texto: String,
    tam: f32,
    /// Resultado do [`render`] como textura. Regerado só quando `dirty`.
    preview: Option<egui::TextureHandle>,
    dirty: bool,
    /// Pan/zoom do canvas (molde do viewer do local-image).
    zoom: f32,
    pan: egui::Vec2,
    fit: bool,
    /// Arraste em andamento, em px da imagem base.
    drag: Option<(Pt, Pt)>,
    /// O botão estava pressionado no frame anterior: fecha a anotação no
    /// mouse-up mesmo com o limiar de drag do egui (mesma razão da captura).
    pressing: bool,
}

impl Shot {
    pub fn new(base: RgbaImage) -> Self {
        Self {
            base,
            edits: Edits::default(),
            tool: Tool::Select,
            selected: None,
            moving: None,
            cor: PALETTE[0],
            espessura: THICK_DEFAULT,
            forca: STRENGTH_DEFAULT,
            texto: String::new(),
            tam: TEXT_DEFAULT,
            preview: None,
            dirty: true,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit: true,
            drag: None,
            pressing: false,
        }
    }

    /// Regera a textura da prévia quando o modelo mudou.
    fn sync_preview(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        let img = render(&self.base, self.edits.crop(), self.edits.annotations());
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return;
        }
        let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
        self.preview = Some(ctx.load_texture("shot-preview", color, egui::TextureOptions::NEAREST));
        self.dirty = false;
        ctx.request_repaint();
    }
}

/// O que a aba devolve pro app (mensagem pronta pro rodapé).
#[derive(Default)]
pub struct Outcome {
    pub status: Option<String>,
}

/// Desenha a aba. Sem captura pendente, mostra a dica do atalho.
pub fn show(ui: &mut egui::Ui, lang: Lang, hotkey: &str, shot: Option<&mut Shot>) -> Outcome {
    let mut out = Outcome::default();
    let Some(shot) = shot else {
        let hint = i18n::t(lang, Key::CaptureHint).replace("{hotkey}", &crate::options::pretty_hotkey(hotkey));
        let _ = ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(hint).weak());
        });
        return out;
    };
    shot.sync_preview(ui.ctx());

    // Ctrl+Z (Command no macOS) desfaz. Consumido antes da barra pra que
    // nenhum campo de texto reclame a tecla.
    if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) && shot.edits.undo() {
        shot.selected = None;
        shot.moving = None;
        shot.dirty = true;
        shot.sync_preview(ui.ctx());
    }

    // Del apaga a anotação selecionada. Só há seleção na ferramenta Select,
    // que não mostra campo de texto: a tecla não é roubada de ninguém. Com um
    // arraste em andamento o índice do `selected` dançaria — ignora.
    if shot.tool == Tool::Select && shot.moving.is_none() {
        if let Some(idx) = shot.selected {
            if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) && shot.edits.remove(idx) {
                shot.selected = None;
                shot.dirty = true;
            }
        }
    }

    toolbar(ui, lang, shot, &mut out);
    // A seleção só existe na ferramenta Select: trocou de ferramenta, limpou.
    if shot.tool != Tool::Select {
        shot.selected = None;
        shot.moving = None;
    }
    ui.separator();
    canvas(ui, shot);
    // Mudou o modelo neste frame? A prévia é regerada no começo do PRÓXIMO —
    // sem o pedido de repaint ela só apareceria no próximo evento do usuário.
    if shot.dirty {
        ui.ctx().request_repaint();
    }
    out
}

/// Barra de ferramentas: ferramentas, cor, espessura, texto e ações.
fn toolbar(ui: &mut egui::Ui, lang: Lang, shot: &mut Shot, out: &mut Outcome) {
    let t = |key: Key| i18n::t(lang, key);
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::ALL {
            ui.selectable_value(&mut shot.tool, tool, t(tool.label()));
        }
        ui.separator();

        for cor in PALETTE {
            if swatch(ui, cor, shot.cor == cor).clicked() {
                shot.cor = cor;
            }
        }
        egui::color_picker::color_edit_button_srgba(ui, &mut shot.cor, egui::color_picker::Alpha::Opaque).on_hover_text(t(Key::Color));

        ui.separator();
        ui.label(t(Key::Thickness));
        ui.add(egui::DragValue::new(&mut shot.espessura).range(1.0..=24.0).speed(0.2));
        if matches!(shot.tool, Tool::Pixelate | Tool::Blur) {
            ui.label(t(Key::Strength));
            ui.add(egui::DragValue::new(&mut shot.forca).range(1..=48));
        }
        if shot.tool == Tool::Text {
            ui.add(egui::DragValue::new(&mut shot.tam).range(8.0..=200.0).suffix(" px"));
            ui.add(kit::line(&mut shot.texto).hint_text(t(Key::ShotText)).desired_width(160.0));
        }

        ui.separator();
        let undo = ui.add_enabled_ui(shot.edits.can_undo(), |ui| kit::pill(ui, t(Key::Undo), false)).inner;
        if undo.clicked() && shot.edits.undo() {
            shot.selected = None;
            shot.moving = None;
            shot.dirty = true;
        }
        if kit::pill(ui, t(Key::ClearAnnotations), false).clicked() {
            shot.edits.clear();
            shot.selected = None;
            shot.moving = None;
            shot.dirty = true;
        }
        if shot.edits.crop().is_some() && kit::pill(ui, t(Key::CropReset), false).clicked() {
            shot.edits.set_crop(None);
            shot.dirty = true;
        }
        ui.separator();
        if kit::pill(ui, t(Key::CopyShot), false).clicked() {
            out.status = Some(copy_to_clipboard(shot, lang));
        }
        if kit::pill(ui, t(Key::SaveShot), true).clicked() {
            out.status = Some(save_png(shot, lang));
        }
    });
}

/// Amostra de cor clicável da paleta.
fn swatch(ui: &mut egui::Ui, cor: egui::Color32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    // O anel de seleção usa a cor de destaque: um anel branco sumiria dentro
    // do branco da paleta.
    let stroke = if selected {
        egui::Stroke::new(2.0_f32, ui.visuals().selection.stroke.color)
    } else {
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(60))
    };
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(4), cor);
    painter.rect_stroke(rect, egui::CornerRadius::same(4), stroke, egui::StrokeKind::Inside);
    resp
}

/// Canvas: prévia com pan/zoom e o desenho das anotações.
/// Quanto da imagem (em px de tela) fica sempre visível por eixo: o pan pode
/// levar a imagem pro canto, nunca pra fora.
const MIN_VISIBLE: f32 = 48.0;

/// Limita o pan: com a imagem menor que o canvas, centraliza (pan ali só
/// deslocaria); maior, deixa no máximo (tamanho − MIN_VISIBLE) fora de cada
/// lado — a imagem nunca some ("arrastar às cegas", feedback do João).
fn clamp_pan(rect: egui::Rect, size: egui::Vec2, pan: &mut egui::Vec2) {
    if size.x <= rect.width() {
        pan.x = (rect.width() - size.x) / 2.0;
    } else {
        pan.x = pan.x.clamp(MIN_VISIBLE - size.x, rect.width() - MIN_VISIBLE);
    }
    if size.y <= rect.height() {
        pan.y = (rect.height() - size.y) / 2.0;
    } else {
        pan.y = pan.y.clamp(MIN_VISIBLE - size.y, rect.height() - MIN_VISIBLE);
    }
}

fn canvas(ui: &mut egui::Ui, shot: &mut Shot) {
    let avail = ui.available_size();
    if avail.x < 1.0 || avail.y < 1.0 {
        return;
    }
    let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    let size = match shot.preview.as_ref() {
        Some(tex) => tex.size_vec2(),
        None => {
            // Sem prévia ainda (primeiro frame): pinta o fundo do painel —
            // área alocada e sem pintura deixa pixel velho de outra aba
            // (fantasma de repaint, pago no primeiro QA do editor).
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, ui.visuals().panel_fill);
            return;
        }
    };
    if size.x < 1.0 || size.y < 1.0 {
        return;
    }

    // Base de encaixe: "fit" até a primeira interação, como o viewer do
    // local-image (o primeiro scroll/arraste materializa a escala).
    let fit_base = |shot: &Shot| {
        if shot.fit {
            let s = (rect.width() / size.x).min(rect.height() / size.y).min(1.0);
            (s, egui::vec2((rect.width() - size.x * s) / 2.0, (rect.height() - size.y * s) / 2.0))
        } else {
            (1.0, egui::Vec2::ZERO)
        }
    };

    // Roda do mouse: zoom ancorado no cursor (o ponto sob o ponteiro fica lá).
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            let (base_scale, base_off) = fit_base(shot);
            if shot.fit {
                shot.pan = base_off;
                shot.zoom *= base_scale;
                shot.fit = false;
            }
            let factor = if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 };
            let new_zoom = (shot.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
            if let Some(m) = resp.hover_pos() {
                let m = m - rect.min;
                shot.pan = m - (m - shot.pan) / shot.zoom * new_zoom;
            }
            shot.zoom = new_zoom;
        }
    }

    let (base_scale, base_off) = fit_base(shot);
    let scale = base_scale * shot.zoom;
    if !shot.fit {
        // Pan limitado: a imagem nunca some do canvas (feedback do João —
        // arrastar "meio às cegas" pra achar a imagem de novo era ruim). Com
        // a imagem menor que o canvas, centraliza.
        clamp_pan(rect, size * scale, &mut shot.pan);
    }
    let dest = egui::Rect::from_min_size(rect.min + base_off + shot.pan, size * scale);
    // Origem do recorte em px da base: a prévia JÁ está recortada, então
    // devolver a coordenada da base exige somar de volta o canto do crop.
    let (ox, oy) = match shot.edits.crop() {
        Some(c) => (c.x as f32, c.y as f32),
        None => (0.0, 0.0),
    };
    let to_view = |p: Pt| egui::pos2(dest.min.x + (p.x - ox) * scale, dest.min.y + (p.y - oy) * scale);
    let to_img = |p: egui::Pos2| Pt::new((p.x - dest.min.x) / scale.max(0.0001) + ox, (p.y - dest.min.y) / scale.max(0.0001) + oy);

    // Índice que sobrou de um desfazer/crop antigo não pode apontar pra fora.
    shot.selected = shot.selected.filter(|i| *i < shot.edits.annotations().len());

    // Cursor: sobre uma anotação a Select oferece "mover"; arrastando, "agarrando".
    let cursor = match shot.tool {
        Tool::Select if shot.moving.is_some() => egui::CursorIcon::Grabbing,
        Tool::Select => {
            let sobre = resp.hover_pos().map(to_img).and_then(|p| shot.edits.hit_test(p, hit_tol(scale)));
            if sobre.is_some() {
                egui::CursorIcon::Move
            } else {
                egui::CursorIcon::Grab
            }
        }
        Tool::Text => egui::CursorIcon::Text,
        _ => egui::CursorIcon::Crosshair,
    };
    let resp = resp.on_hover_cursor(cursor);

    let painter = ui.painter_at(rect);
    if let Some(tex) = shot.preview.as_ref() {
        painter.image(tex.id(), dest, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }

    match shot.tool {
        // Selecionar/mover: apertar SOBRE uma anotação a move; arrastar em área
        // vazia dá pan (e a roda dá zoom). Sem botão do meio: o clique vazio já
        // cobre o pan e não disputa com o arraste de anotação.
        Tool::Select => {
            // Soltar encerra o arraste da anotação.
            if resp.drag_stopped() {
                shot.moving = None;
            }
            // Onde o arraste nasceu decide: anotação → mover; vazio → pan. O
            // alvo é medido no ponto do APERTO — quando `drag_started` dispara o
            // ponteiro já andou o limiar e pode ter saído do traço.
            if resp.drag_started() {
                let aperto = ui.input(|i| i.pointer.press_origin());
                let hit = aperto.map(to_img).and_then(|p| shot.edits.hit_test(p, hit_tol(scale)));
                match hit {
                    Some(idx) => {
                        let origem = shot.edits.annotations().get(idx).cloned();
                        if let (Some(origem), Some(p)) = (origem, resp.interact_pointer_pos()) {
                            shot.selected = Some(idx);
                            shot.moving = Some(Moving { idx, origem, inicio: to_img(p), snapshot: false });
                        }
                    }
                    None => shot.selected = None,
                }
            }
            // Clique parado só seleciona (no vazio, desmarca).
            if resp.clicked() {
                shot.selected = resp.interact_pointer_pos().map(to_img).and_then(|p| shot.edits.hit_test(p, hit_tol(scale)));
            }
            // Arrastando uma anotação: o deslocamento é o TOTAL desde o começo
            // (a região é u32 e o arredondamento por frame travaria no zoom alto).
            let movendo = shot.moving.as_ref().map(|mv| (mv.idx, mv.origem.clone(), mv.inicio, mv.snapshot));
            if let Some((idx, origem, inicio, snapshot)) = movendo {
                if let Some(p) = resp.interact_pointer_pos() {
                    let atual = to_img(p);
                    let (dx, dy) = (atual.x - inicio.x, atual.y - inicio.y);
                    if dx.abs() > 0.01 || dy.abs() > 0.01 {
                        // Um passo de desfazer pro arraste INTEIRO.
                        if !snapshot {
                            shot.edits.checkpoint();
                            if let Some(mv) = shot.moving.as_mut() {
                                mv.snapshot = true;
                            }
                        }
                        if shot.edits.translate(idx, &origem, dx, dy) {
                            shot.dirty = true;
                        }
                    }
                }
            } else if resp.dragged() {
                let (base_scale, base_off) = fit_base(shot);
                if shot.fit {
                    shot.pan = base_off;
                    shot.zoom *= base_scale;
                    shot.fit = false;
                }
                shot.pan += resp.drag_delta();
            }
        }
        _ => {
            if resp.is_pointer_button_down_on() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let ip = to_img(p);
                    match shot.drag.as_mut() {
                        Some(drag) => drag.1 = ip,
                        None => shot.drag = Some((ip, ip)),
                    }
                }
                shot.pressing = true;
            } else if shot.pressing {
                shot.pressing = false;
                if let Some((a, b)) = shot.drag.take() {
                    commit(shot, a, b);
                }
            }
            // Texto é CLIQUE (não arraste): a posição é o canto de entrada.
            let place_text = resp.clicked() && shot.tool == Tool::Text;
            if let Some(p) = resp.interact_pointer_pos().filter(|_| place_text) {
                let texto = shot.texto.trim().to_string();
                if !texto.is_empty() {
                    let pos = to_img(p);
                    let (cor, tam) = (shot.cor, shot.tam);
                    shot.edits.push(Annotation::Text { pos, cor, tam, texto });
                    shot.dirty = true;
                }
            }
        }
    }

    // Prévia do arraste em andamento (a anotação de verdade só entra ao soltar).
    let live = shot.tool;
    if let Some((a, b)) = shot.drag.filter(|_| matches!(live, Tool::Rect | Tool::Arrow | Tool::Pixelate | Tool::Blur | Tool::Crop)) {
        let (pa, pb) = (to_view(a), to_view(b));
        let cor = if live == Tool::Crop { egui::Color32::from_rgb(90, 170, 255) } else { shot.cor };
        let stroke = egui::Stroke::new((shot.espessura * scale).clamp(1.0, 8.0), cor);
        if live == Tool::Arrow {
            painter.line_segment([pa, pb], stroke);
            let dir = pb - pa;
            let head = (dir.length() * 0.25).clamp(8.0, 48.0);
            let ang = dir.angle();
            for s in [-1.0_f32, 1.0] {
                let th = ang + std::f32::consts::PI + s * 25.0_f32.to_radians();
                painter.line_segment([pb, pb + egui::vec2(th.cos() * head, th.sin() * head)], stroke);
            }
        } else {
            painter.rect_stroke(egui::Rect::from_two_pos(pa, pb), egui::CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);
        }
    }

    // Seleção: contorno tracejado fino em volta da anotação escolhida.
    if shot.tool == Tool::Select {
        if let Some(an) = shot.selected.and_then(|i| shot.edits.annotations().get(i)) {
            let (p0, p1) = an.bounds();
            let r = egui::Rect::from_two_pos(to_view(p0), to_view(p1));
            let stroke = egui::Stroke::new(1.0_f32, ui.visuals().selection.stroke.color);
            // `StrokeKind` só existe pro `rect_stroke`: pro tracejado ficar
            // "Outside" a caixa é expandida meia espessura + 1 px de folga.
            dashed_outline(&painter, r.expand(stroke.width * 0.5 + 1.0), stroke);
        }
    }
}

/// Tolerância do hit-test em px da IMAGEM: o alvo confortável de ~6 px de tela
/// vira isso na escala atual (nunca menor que 1 px da imagem).
fn hit_tol(scale: f32) -> f32 {
    (HIT_TOL_SCREEN / scale.max(0.0001)).max(1.0)
}

/// Contorno tracejado de um retângulo: o egui só tem `rect_stroke` sólido,
/// então as quatro arestas viram um caminho fechado com `Shape::dashed_line`.
fn dashed_outline(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let caminho = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    painter.extend(egui::Shape::dashed_line(&caminho, stroke, 4.0, 3.0));
}

/// Fecha o arraste numa anotação (ou num crop), respeitando o mínimo.
fn commit(shot: &mut Shot, a: Pt, b: Pt) {
    let big_enough = (a.x - b.x).abs() >= MIN_DRAG || (a.y - b.y).abs() >= MIN_DRAG;
    // Cores por valor: mexer em `shot.edits` e ler outros campos de `shot` na
    // mesma expressão só funciona por empréstimo de campo — assim é explícito.
    let (cor, espessura, forca) = (shot.cor, shot.espessura, shot.forca);
    match shot.tool {
        Tool::Rect if big_enough => {
            shot.edits.push(Annotation::Rect { a, b, cor, espessura });
            shot.dirty = true;
        }
        Tool::Arrow if (a.x - b.x).hypot(a.y - b.y) >= MIN_DRAG => {
            shot.edits.push(Annotation::Arrow { a, b, cor, espessura });
            shot.dirty = true;
        }
        Tool::Pixelate => {
            if let Some(rect) = rect_of(a, b) {
                shot.edits.push(Annotation::Pixelate { rect, forca });
                shot.dirty = true;
            }
        }
        Tool::Blur => {
            if let Some(rect) = rect_of(a, b) {
                shot.edits.push(Annotation::Blur { rect, forca });
                shot.dirty = true;
            }
        }
        Tool::Crop => {
            if let Some(rect) = rect_of(a, b) {
                let rect = clamp_rect(&shot.base, rect);
                shot.edits.set_crop(Some(rect));
                shot.dirty = true;
            }
        }
        _ => {}
    }
}

/// Copia o resultado pro clipboard como entrada de imagem (espelho do botão
/// "recopiar" da aba Clipboard — é o caminho mais barato de copiar imagem).
fn copy_to_clipboard(shot: &Shot, lang: Lang) -> String {
    let img = render(&shot.base, shot.edits.crop(), shot.edits.annotations());
    let (w, h) = img.dimensions();
    let Some(png) = history::encode_png(img.as_raw(), w, h) else {
        return format!("⚠ {}", i18n::t(lang, Key::CopyFailed));
    };
    let entry = Entry {
        id: history::new_id(),
        ts_ms: history::now_ms(),
        kind: Kind::Image,
        pinned: false,
        text: None,
        files: None,
        thumb_png: history::make_thumb(img.as_raw(), w, h, history::THUMB_MAX),
        image_png: Some(png),
    };
    match clipboard::write_back(&entry) {
        Ok(()) => i18n::t(lang, Key::CopiedOk).to_string(),
        Err(e) => format!("⚠ {}: {e}", i18n::t(lang, Key::CopyFailed)),
    }
}

/// "Salvar como" (PNG). Cancelar o diálogo não deixa mensagem no rodapé.
#[cfg(windows)]
fn save_png(shot: &Shot, lang: Lang) -> String {
    let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(default_name()).save_file() else {
        return String::new();
    };
    let img = render(&shot.base, shot.edits.crop(), shot.edits.annotations());
    let (w, h) = img.dimensions();
    match image::save_buffer(&path, img.as_raw(), w, h, image::ExtendedColorType::Rgba8) {
        Ok(()) => i18n::t(lang, Key::ShotSaved).to_string(),
        Err(e) => format!("⚠ {}: {e}", i18n::t(lang, Key::ShotSaveFailed)),
    }
}

#[cfg(not(windows))]
fn save_png(_shot: &Shot, lang: Lang) -> String {
    format!("⚠ {}", i18n::t(lang, Key::WindowsOnly))
}

/// Nome sugerido do arquivo: `screenshot-AAAAMMDD-hhmmss.png`.
#[cfg(windows)]
fn default_name() -> String {
    let ms = history::now_ms();
    let days = i64::try_from(ms / 86_400_000).unwrap_or(0);
    let (y, m, d) = crate::clipboard_ui::civil_from_days(days);
    let secs = (ms % 86_400_000) / 1000;
    format!("screenshot-{y:04}{m:02}{d:02}-{:02}{:02}{:02}.png", secs / 3600, (secs / 60) % 60, secs % 60)
}

// ───────────────────────── desenho no RgbaImage ─────────────────────────

/// RGBA da cor do egui.
fn rgba(cor: egui::Color32) -> [u8; 4] {
    [cor.r(), cor.g(), cor.b(), cor.a()]
}

/// Aplica o deslocamento do recorte (o recorte é sempre um canto positivo).
fn shift(p: Pt, ox: f32, oy: f32) -> Pt {
    Pt::new(p.x - ox, p.y - oy)
}

fn shift_rect(r: Rect, ox: f32, oy: f32) -> (i32, i32, u32, u32) {
    (r.x as i32 - ox as i32, r.y as i32 - oy as i32, r.w, r.h)
}

/// Recorta o retângulo nos limites da imagem.
fn clamp_rect(img: &RgbaImage, r: Rect) -> Rect {
    let (w, h) = img.dimensions();
    let x = r.x.min(w);
    let y = r.y.min(h);
    Rect { x, y, w: r.w.min(w.saturating_sub(x)), h: r.h.min(h.saturating_sub(y)) }
}

/// Retângulo normalizado a partir de dois pontos (mínimo de 2 px de lado).
fn rect_of(a: Pt, b: Pt) -> Option<Rect> {
    let x0 = a.x.min(b.x).max(0.0);
    let y0 = a.y.min(b.y).max(0.0);
    let x1 = a.x.max(b.x).max(0.0);
    let y1 = a.y.max(b.y).max(0.0);
    let (w, h) = ((x1 - x0).round() as u32, (y1 - y0).round() as u32);
    if w < 2 || h < 2 {
        return None;
    }
    Some(Rect { x: x0.round() as u32, y: y0.round() as u32, w, h })
}

/// Alpha blend de um pixel (a imagem base é sempre opaca).
fn blend(img: &mut RgbaImage, x: i32, y: i32, px: [u8; 4], alpha: f32) {
    let (w, h) = img.dimensions();
    if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
        return;
    }
    let a = (f32::from(px[3]) / 255.0) * alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let dst = img.get_pixel_mut(x as u32, y as u32);
    let rgb = [px[0], px[1], px[2]];
    for (canal, src) in dst.0.iter_mut().zip(rgb) {
        let fundo = f32::from(*canal);
        *canal = (f32::from(src) * a + fundo * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    }
    dst.0[3] = 255;
}

/// Pincel circular.
fn dot(img: &mut RgbaImage, cx: f32, cy: f32, raio: f32, cor: [u8; 4]) {
    let r = raio.max(0.5);
    let span = r.ceil() as i32;
    let (ix, iy) = (cx.round() as i32, cy.round() as i32);
    for dy in -span..=span {
        for dx in -span..=span {
            if (dx * dx + dy * dy) as f32 > r * r {
                continue;
            }
            blend(img, ix + dx, iy + dy, cor, 1.0);
        }
    }
}

/// Linha por Bresenham com pincel da espessura (sem dep de desenho: é pouco
/// código e fica exato).
fn line(img: &mut RgbaImage, a: Pt, b: Pt, espessura: f32, cor: [u8; 4]) {
    let raio = espessura.max(1.0) / 2.0;
    let (mut x, mut y) = (a.x.round() as i32, a.y.round() as i32);
    let (x1, y1) = (b.x.round() as i32, b.y.round() as i32);
    let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
    let (sx, sy) = (if x < x1 { 1 } else { -1 }, if y < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        dot(img, x as f32, y as f32, raio, cor);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Retângulo vazado: quatro linhas.
fn rect_outline(img: &mut RgbaImage, a: Pt, b: Pt, espessura: f32, cor: [u8; 4]) {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (a.x.max(b.x), a.y.max(b.y));
    line(img, Pt::new(x0, y0), Pt::new(x1, y0), espessura, cor);
    line(img, Pt::new(x1, y0), Pt::new(x1, y1), espessura, cor);
    line(img, Pt::new(x1, y1), Pt::new(x0, y1), espessura, cor);
    line(img, Pt::new(x0, y1), Pt::new(x0, y0), espessura, cor);
}

/// Seta: haste + duas linhas a ±25° na ponta.
fn arrow(img: &mut RgbaImage, a: Pt, b: Pt, espessura: f32, cor: [u8; 4]) {
    line(img, a, b, espessura, cor);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return;
    }
    let head = (len * 0.25).clamp(12.0, 60.0) + espessura;
    let base = dy.atan2(dx);
    for s in [-1.0_f32, 1.0] {
        let th = base + s * 25.0_f32.to_radians();
        line(img, b, Pt::new(b.x - head * th.cos(), b.y - head * th.sin()), espessura, cor);
    }
}

/// Mosaico: reduz a região (box filter) e devolve com vizinho-mais-próximo.
fn pixelate(img: &mut RgbaImage, x: i32, y: i32, w: u32, h: u32, forca: u32) {
    let Some((x, y, w, h)) = region(img, x, y, w, h) else { return };
    if w < 2 || h < 2 {
        return;
    }
    let block = forca.clamp(2, 64);
    let region_img = imageops::crop_imm(img, x, y, w, h).to_image();
    let (sw, sh) = ((w / block).max(1), (h / block).max(1));
    let small = imageops::thumbnail(&region_img, sw, sh);
    let big = imageops::resize(&small, w, h, imageops::FilterType::Nearest);
    blit(img, &big, x, y);
}

/// Desfoque de caixa separável (horizontal + vertical) só na região.
fn blur(img: &mut RgbaImage, x: i32, y: i32, w: u32, h: u32, forca: u32) {
    let Some((x, y, w, h)) = region(img, x, y, w, h) else { return };
    if w < 2 || h < 2 {
        return;
    }
    let raio = forca.clamp(1, 32) as i32;
    let src = imageops::crop_imm(img, x, y, w, h).to_image();
    let (wu, hu) = (w as usize, h as usize);
    let raw = src.as_raw();
    let mut tmp = vec![0_u8; raw.len()];
    let mut out = vec![0_u8; raw.len()];

    for yy in 0..hu {
        for xx in 0..wu {
            let mut acc = [0_u32; 4];
            let mut n = 0_u32;
            for k in -raio..=raio {
                let sx = (xx as i32 + k).clamp(0, wu as i32 - 1) as usize;
                let i = (yy * wu + sx) * 4;
                for (soma, amostra) in acc.iter_mut().zip(&raw[i..i + 4]) {
                    *soma += u32::from(*amostra);
                }
                n += 1;
            }
            let o = (yy * wu + xx) * 4;
            for (destino, soma) in tmp[o..o + 4].iter_mut().zip(acc) {
                *destino = (soma / n) as u8;
            }
        }
    }
    for yy in 0..hu {
        for xx in 0..wu {
            let mut acc = [0_u32; 4];
            let mut n = 0_u32;
            for k in -raio..=raio {
                let sy = (yy as i32 + k).clamp(0, hu as i32 - 1) as usize;
                let i = (sy * wu + xx) * 4;
                for (soma, amostra) in acc.iter_mut().zip(&tmp[i..i + 4]) {
                    *soma += u32::from(*amostra);
                }
                n += 1;
            }
            let o = (yy * wu + xx) * 4;
            for (destino, soma) in out[o..o + 4].iter_mut().zip(acc) {
                *destino = (soma / n) as u8;
            }
        }
    }

    if let Some(big) = RgbaImage::from_raw(w, h, out) {
        blit(img, &big, x, y);
    }
}

/// Texto com contorno escuro de 1 px, rasterizado pelo `ab_glyph` (a fonte é a
/// Ubuntu-Light embutida pelo epaint).
fn draw_text(img: &mut RgbaImage, pos: Pt, tam: f32, cor: [u8; 4], texto: &str) {
    if texto.is_empty() {
        return;
    }
    let Ok(font) = ab_glyph::FontVec::try_from_vec(FONT_TTF.to_vec()) else { return };
    let scale = ab_glyph::PxScale::from(tam.clamp(6.0, 400.0));
    let scaled = font.as_scaled(scale);
    let baseline = pos.y + scaled.ascent();
    let mut pen = pos.x;

    // Cobertura primeiro, pintura depois: duas passadas garantem que o
    // contorno de um glifo não coma o preenchimento do seguinte.
    let mut covered: Vec<(i32, i32, f32)> = Vec::new();
    for ch in texto.chars() {
        let gid = font.glyph_id(ch);
        let advance = scaled.h_advance(gid);
        if ch != ' ' {
            let glyph = gid.with_scale_and_position(scale, ab_glyph::point(pen, baseline));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                let (bx, by) = (bounds.min.x.round() as i32, bounds.min.y.round() as i32);
                outline.draw(|gx, gy, cov| {
                    if cov > 0.01 {
                        covered.push((bx + gx as i32, by + gy as i32, cov));
                    }
                });
            }
        }
        pen += advance;
    }

    let contorno = [0_u8, 0, 0, 235];
    for &(x, y, cov) in &covered {
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                blend(img, x + dx, y + dy, contorno, cov);
            }
        }
    }
    for &(x, y, cov) in &covered {
        blend(img, x, y, cor, cov);
    }
}

/// Região pedida, presa aos limites da imagem.
fn region(img: &RgbaImage, x: i32, y: i32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let (iw, ih) = img.dimensions();
    let x0 = x.max(0) as u32;
    let y0 = y.max(0) as u32;
    let x1 = (x.saturating_add(w as i32)).max(0) as u32;
    let y1 = (y.saturating_add(h as i32)).max(0) as u32;
    let (x1, y1) = (x1.min(iw), y1.min(ih));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

/// Copia `src` para dentro de `dst` no canto (x, y) — `imageops::replace` corta
/// o que passar da borda.
fn blit(dst: &mut RgbaImage, src: &RgbaImage, x: u32, y: u32) {
    imageops::replace(dst, src, i64::from(x), i64::from(y));
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255]))
    }

    /// Quantos pixels diferem entre duas imagens do mesmo tamanho.
    fn diff(a: &RgbaImage, b: &RgbaImage) -> usize {
        a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count()
    }

    /// Ponta a e ponta b de uma anotação de linha (Rect/Seta).
    fn pontos(an: &Annotation) -> Option<(Pt, Pt)> {
        match an {
            Annotation::Rect { a, b, .. } | Annotation::Arrow { a, b, .. } => Some((*a, *b)),
            _ => None,
        }
    }

    /// Retângulo de uma anotação de região (Pixelate/Blur).
    fn rect_da_regiao(an: &Annotation) -> Option<Rect> {
        match an {
            Annotation::Pixelate { rect, .. } | Annotation::Blur { rect, .. } => Some(*rect),
            _ => None,
        }
    }

    #[test]
    fn sem_anotacao_o_render_devolve_a_base() {
        let base = solid(64, 32);
        let out = render(&base, None, &[]);
        assert_eq!(out.dimensions(), (64, 32));
        assert_eq!(out, base);
    }

    /// O crop define o tamanho da saída E desloca as anotações: um retângulo na
    /// borda do recorte cai na borda da imagem exportada.
    #[test]
    fn crop_define_o_tamanho_e_desloca_as_anotacoes() {
        let base = solid(100, 80);
        let crop = Rect { x: 20, y: 10, w: 40, h: 30 };
        let sem = render(&base, Some(crop), &[]);
        assert_eq!(sem.dimensions(), (40, 30));

        let an = [Annotation::Rect { a: Pt::new(20.0, 10.0), b: Pt::new(60.0, 40.0), cor: egui::Color32::RED, espessura: 2.0 }];
        let com = render(&base, Some(crop), &an);
        assert_eq!(com.dimensions(), (40, 30));
        assert_ne!(com.get_pixel(0, 0), sem.get_pixel(0, 0), "a borda do retângulo cai no canto do recorte");
        assert_eq!(com.get_pixel(20, 15), sem.get_pixel(20, 15), "o miolo do retângulo não é pintado");
    }

    /// Cada efeito de região mexe DENTRO do retângulo e nada fora dele.
    #[test]
    fn pixelate_e_blur_mudam_so_a_regiao() {
        let base = RgbaImage::from_fn(64, 64, |x, y| Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255]));
        let rect = Rect { x: 16, y: 16, w: 16, h: 16 };
        for an in [Annotation::Pixelate { rect, forca: 8 }, Annotation::Blur { rect, forca: 3 }] {
            let out = render(&base, None, &[an]);
            assert_eq!(out.dimensions(), (64, 64));
            let mut dentro = 0;
            let mut fora = 0;
            for y in 0..64_u32 {
                for x in 0..64_u32 {
                    if out.get_pixel(x, y) != base.get_pixel(x, y) {
                        if (16..32).contains(&x) && (16..32).contains(&y) {
                            dentro += 1;
                        } else {
                            fora += 1;
                        }
                    }
                }
            }
            assert!(dentro > 0, "o efeito mudou alguma coisa");
            assert_eq!(fora, 0, "nada fora da região muda");
        }
    }

    /// Seta: pinta ao longo da haste e não na esquina oposta.
    #[test]
    fn seta_pinta_a_haste_e_pouco_mais() {
        let base = solid(100, 100);
        let an = [Annotation::Arrow { a: Pt::new(10.0, 10.0), b: Pt::new(80.0, 80.0), cor: egui::Color32::WHITE, espessura: 2.0 }];
        let out = render(&base, None, &an);
        assert_ne!(out.get_pixel(45, 45), base.get_pixel(45, 45), "meio da haste pintado");
        assert_eq!(out.get_pixel(95, 5), base.get_pixel(95, 5), "a esquina oposta fica intacta");
    }

    /// Texto: alguma coisa é rasterizada perto da posição pedida.
    #[test]
    fn texto_pinta_na_regiao_da_posicao() {
        let base = solid(160, 60);
        let an = [Annotation::Text { pos: Pt::new(8.0, 8.0), cor: egui::Color32::WHITE, tam: 24.0, texto: "Ab".to_string() }];
        let out = render(&base, None, &an);
        assert_eq!(out.dimensions(), (160, 60));
        let mut mudou = 0;
        for y in 0..60_u32 {
            for x in 0..70_u32 {
                if out.get_pixel(x, y) != base.get_pixel(x, y) {
                    mudou += 1;
                }
            }
        }
        assert!(mudou > 0, "o texto pintou alguma coisa");
        assert_eq!(diff(&out, &base), mudou, "nada mudou depois da área do texto");
    }

    /// Desfazer andou de volta um passo (anotação) e preservou o resto.
    #[test]
    fn undo_remove_a_ultima_anotacao() {
        let mut edits = Edits::default();
        edits.push(Annotation::Blur { rect: Rect { x: 0, y: 0, w: 8, h: 8 }, forca: 4 });
        edits.push(Annotation::Pixelate { rect: Rect { x: 0, y: 0, w: 9, h: 9 }, forca: 4 });
        assert_eq!(edits.annotations().len(), 2);
        assert!(edits.undo());
        assert_eq!(edits.annotations().len(), 1);
        assert!(edits.undo());
        assert!(edits.annotations().is_empty());
        assert!(!edits.undo(), "sem passos, desfazer não faz nada");
        assert!(!edits.can_undo());
    }

    /// O crop é estado desfazível (não destrói a imagem base).
    #[test]
    fn undo_desfaz_o_crop() {
        let mut edits = Edits::default();
        assert_eq!(edits.crop(), None);
        edits.set_crop(Some(Rect { x: 1, y: 1, w: 4, h: 4 }));
        assert_eq!(edits.crop(), Some(Rect { x: 1, y: 1, w: 4, h: 4 }));
        assert!(edits.undo());
        assert_eq!(edits.crop(), None);
    }

    /// O recorte não pode estourar a imagem base (arraste pra fora).
    #[test]
    fn crop_fora_da_imagem_e_preso_nos_limites() {
        let base = solid(40, 20);
        let out = render(&base, Some(Rect { x: 30, y: 10, w: 100, h: 100 }), &[]);
        assert_eq!(out.dimensions(), (10, 10));
    }

    /// Hit-test de cada tipo: em cima da anotação pega, no miolo vazio e fora
    /// dela não. A espessura do traço entra na tolerância.
    #[test]
    fn hit_test_pega_cada_tipo_no_lugar_certo() {
        // Retângulo vazado: as arestas pegam, o miolo não.
        let mut edits = Edits::default();
        edits.push(Annotation::Rect { a: Pt::new(10.0, 10.0), b: Pt::new(60.0, 40.0), cor: egui::Color32::RED, espessura: 2.0 });
        assert_eq!(edits.hit_test(Pt::new(35.0, 10.0), 2.0), Some(0), "aresta de cima");
        assert_eq!(edits.hit_test(Pt::new(60.0, 25.0), 2.0), Some(0), "aresta direita");
        assert_eq!(edits.hit_test(Pt::new(35.0, 25.0), 2.0), None, "miolo vazado");
        assert_eq!(edits.hit_test(Pt::new(35.0, 4.0), 2.0), None, "fora da borda");

        // Seta: haste e linhas da ponta.
        let mut edits = Edits::default();
        edits.push(Annotation::Arrow { a: Pt::new(10.0, 10.0), b: Pt::new(90.0, 90.0), cor: egui::Color32::WHITE, espessura: 2.0 });
        assert_eq!(edits.hit_test(Pt::new(50.0, 52.0), 2.0), Some(0), "quase na haste");
        assert_eq!(edits.hit_test(Pt::new(76.0, 85.0), 2.0), Some(0), "na linha da ponta");
        assert_eq!(edits.hit_test(Pt::new(80.0, 20.0), 2.0), None, "longe da seta");

        // Texto: caixa estimada (0.6*tam por caractere).
        let mut edits = Edits::default();
        edits.push(Annotation::Text { pos: Pt::new(10.0, 10.0), cor: egui::Color32::WHITE, tam: 20.0, texto: "abc".to_string() });
        assert_eq!(edits.hit_test(Pt::new(20.0, 20.0), 0.0), Some(0), "dentro da caixa");
        assert_eq!(edits.hit_test(Pt::new(60.0, 20.0), 0.0), None, "depois do fim do texto");
        assert_eq!(edits.hit_test(Pt::new(48.0, 20.0), 2.0), Some(0), "tolerância na borda direita");

        // Região: dentro do retângulo (não só na borda).
        let mut edits = Edits::default();
        edits.push(Annotation::Pixelate { rect: Rect { x: 10, y: 10, w: 20, h: 10 }, forca: 8 });
        assert_eq!(edits.hit_test(Pt::new(15.0, 15.0), 0.0), Some(0), "dentro da região");
        assert_eq!(edits.hit_test(Pt::new(35.0, 15.0), 0.0), None, "fora da região");
        assert_eq!(edits.hit_test(Pt::new(31.0, 15.0), 2.0), Some(0), "tolerância na borda");
    }

    /// Sobreposição: a MAIS RECENTE ganha (é a que aparece por cima).
    #[test]
    fn hit_test_prefere_a_anotacao_mais_recente() {
        let mut edits = Edits::default();
        edits.push(Annotation::Blur { rect: Rect { x: 0, y: 0, w: 40, h: 40 }, forca: 4 });
        edits.push(Annotation::Rect { a: Pt::new(10.0, 10.0), b: Pt::new(30.0, 30.0), cor: egui::Color32::RED, espessura: 2.0 });
        assert_eq!(edits.hit_test(Pt::new(20.0, 10.0), 2.0), Some(1), "a borda do retângulo está por cima");
        assert_eq!(edits.hit_test(Pt::new(20.0, 20.0), 2.0), Some(0), "só o blur cobre o miolo");
    }

    /// Mover desloca os DOIS pontos e o arraste inteiro é um passo de desfazer.
    #[test]
    fn mover_rect_desloca_os_dois_pontos_em_um_passo_de_undo() {
        let mut edits = Edits::default();
        edits.push(Annotation::Rect { a: Pt::new(10.0, 10.0), b: Pt::new(50.0, 30.0), cor: egui::Color32::RED, espessura: 2.0 });
        let origem = edits.annotations()[0].clone();
        assert_eq!(edits.hit_test(Pt::new(30.0, 10.0), 2.0), Some(0), "a anotação é pegável antes do arraste");

        edits.checkpoint();
        // Três frames do MESMO arraste: o delta é sempre medido desde o começo.
        assert!(edits.translate(0, &origem, 5.0, 0.0));
        assert!(edits.translate(0, &origem, 10.0, 4.0));
        assert!(edits.translate(0, &origem, 10.0, 8.0));
        assert_eq!(pontos(&edits.annotations()[0]), Some((Pt::new(20.0, 18.0), Pt::new(60.0, 38.0))), "os dois pontos andaram");

        assert!(edits.undo());
        assert_eq!(pontos(&edits.annotations()[0]), Some((Pt::new(10.0, 10.0), Pt::new(50.0, 30.0))), "um passo por arraste inteiro");
        assert_eq!(edits.annotations().len(), 1, "mover não cria nem duplica");
    }

    /// Mover a região (u32) mantém w/h e não acumula arredondamento.
    #[test]
    fn mover_pixelate_preserva_o_tamanho() {
        let mut edits = Edits::default();
        edits.push(Annotation::Pixelate { rect: Rect { x: 10, y: 10, w: 24, h: 16 }, forca: 8 });
        let origem = edits.annotations()[0].clone();
        edits.checkpoint();
        assert!(edits.translate(0, &origem, 12.4, 7.6));
        assert_eq!(rect_da_regiao(&edits.annotations()[0]), Some(Rect { x: 22, y: 18, w: 24, h: 16 }));
        assert!(edits.translate(0, &origem, 12.5, 7.5), "trocar o delta usa a origem, não o estado atual");
        assert_eq!(rect_da_regiao(&edits.annotations()[0]), Some(Rect { x: 23, y: 18, w: 24, h: 16 }));
    }

    /// Del apaga a selecionada e isso é desfazível.
    #[test]
    fn remover_a_anotacao_selecionada_e_desfazivel() {
        let mut edits = Edits::default();
        edits.push(Annotation::Blur { rect: Rect { x: 0, y: 0, w: 8, h: 8 }, forca: 4 });
        edits.push(Annotation::Pixelate { rect: Rect { x: 20, y: 20, w: 8, h: 8 }, forca: 4 });
        assert!(edits.remove(1));
        assert_eq!(edits.annotations().len(), 1);
        assert!(matches!(&edits.annotations()[0], Annotation::Blur { .. }), "removeu a anotação certa");
        assert!(edits.undo(), "apagar entra no desfazer");
        assert_eq!(edits.annotations().len(), 2);
        assert!(!edits.remove(9), "índice fora da lista não faz nada");
        assert_eq!(edits.annotations().len(), 2);
    }
}
