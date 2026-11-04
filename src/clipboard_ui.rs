//! Aba Clipboard: lista virtualizada do histórico, busca instantânea, filtro por
//! tipo, recopiar/editar/QR/remover e navegação por teclado (UX herdada do
//! piloto LocalClip v0.3: ↑/↓ move, Enter copia, Del apaga, Esc limpa a busca).
//!
//! Esta camada só DESENHA e aplica ações: o motor (`history.rs`) guarda e
//! persiste as entradas, a ponte com o SO (`clipboard.rs`) lê e regrava. Quem
//! regrava o JSON é o app, olhando o [`Outcome::changed`] — a aba nunca grava
//! em disco por conta própria.

use std::collections::{HashMap, VecDeque};

use eframe::egui;
use utils_ui::i18n::{self, Key, Lang};

use crate::clipboard;
use crate::history::{Entry, History, Kind};
use crate::ui::{self as kit, Skin};

/// Altura fixa de uma linha da lista (o `show_rows` precisa dela pra saber
/// quantas linhas existem sem desenhar todas).
const ROW_H: f32 = 64.0;
/// Lado da etiqueta/miniatura do tipo dentro da linha.
const BADGE: f32 = 48.0;
/// Faixa reservada pras ações da linha (a coluna de texto não invade).
const ACTIONS_W: f32 = 150.0;
/// Quantas texturas de miniatura ficam vivas: a lista pode ter centenas de
/// imagens e cada textura ocupa GPU (a miniatura é 256px, o PNG cheio nunca
/// vira textura).
const THUMB_CACHE: usize = 200;
/// Acima disso o QR fica ilegível num modal de 300px — o botão avisa em vez de
/// abrir.
const QR_MAX_CHARS: usize = 1000;
/// Lado do QR desenhado no modal.
const QR_SIZE: f32 = 300.0;

/// Filtro por tipo (as pills do topo).
#[derive(Clone, Copy, PartialEq, Eq)]
enum KindFilter {
    All,
    Text,
    Images,
    Files,
    Rich,
}

impl KindFilter {
    /// Todas as pills, na ordem da barra.
    const ALL: [KindFilter; 5] = [KindFilter::All, KindFilter::Text, KindFilter::Images, KindFilter::Files, KindFilter::Rich];

    fn label_key(self) -> Key {
        match self {
            KindFilter::All => Key::FilterAll,
            KindFilter::Text => Key::FilterText,
            KindFilter::Images => Key::FilterImages,
            KindFilter::Files => Key::FilterFiles,
            KindFilter::Rich => Key::FilterRich,
        }
    }

    /// A entrada passa no filtro? "Rico" cobre HTML e RTF (os formatos com
    /// marcação, que não são texto puro nem binário).
    fn accepts(self, kind: Kind) -> bool {
        match self {
            KindFilter::All => true,
            KindFilter::Text => kind == Kind::Text,
            KindFilter::Images => kind == Kind::Image,
            KindFilter::Files => kind == Kind::Files,
            KindFilter::Rich => matches!(kind, Kind::Html | Kind::Rtf),
        }
    }
}

/// O que a aba pede de volta pro app depois de desenhar.
#[derive(Default)]
pub struct Outcome {
    /// O histórico mudou (pin/remover/editar): o app regrava o JSON.
    pub changed: bool,
    /// Mensagem pronta pra barra de status (já traduzida).
    pub status: Option<String>,
    /// Pedido de OCR de uma imagem (o app sobe a thread; a leitura é na
    /// `ocr.rs`). O botão só existe no Windows, mas o campo é sempre lido pelo
    /// app — no Linux fica sempre `None`.
    pub ocr: Option<u128>,
}

/// Estado da aba que sobrevive entre frames (fica no `UtilsApp`).
pub struct ClipboardUiState {
    search: String,
    filter: KindFilter,
    /// Entrada com foco de teclado — id, não índice: cada cópia nova reordena
    /// a lista e o foco não pode "pular" pra vizinha errada.
    focus: Option<u128>,
    /// A busca estava com o foco de texto neste frame: as setas/Enter/Del
    /// pertencem ao campo, não à lista.
    search_focused: bool,
    thumbs: ThumbCache,
    /// Diálogo de edição aberto (entrada + rascunho).
    edit: Option<EditDraft>,
    /// Texto do diálogo de QR (`None` = fechado).
    qr: Option<String>,
    /// Texto lido pela OCR (`None` = diálogo fechado). O app semeia com
    /// [`ClipboardUiState::show_ocr`] quando a thread de leitura termina.
    ocr: Option<OcrDialog>,
    /// Confirmação do "Limpar" (só o histórico não-pinado sai).
    confirm_clear: bool,
    /// Offset da lista e altura visível do frame anterior. As setas precisam
    /// saber onde a linha focada está pra trazê-la pra tela — o `show_rows` só
    /// desenha as linhas visíveis.
    scroll_offset: f32,
    view_height: f32,
    /// Linha (índice na lista filtrada) a trazer pra tela no próximo frame.
    scroll_to: Option<usize>,
    /// Lista filtrada em cache (ver [`FilterCache`]).
    cache: FilterCache,
}

impl ClipboardUiState {
    /// Descarta o cache da lista filtrada: o histórico mudou por fora da aba
    /// (import de `.lutils`), que pode não mexer no tamanho nem nas pontas.
    pub fn invalidate(&mut self) {
        self.cache.filter = None;
    }

    /// Abre o diálogo com o texto que a OCR devolveu da imagem `id`.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn show_ocr(&mut self, _id: u128, text: String) {
        self.ocr = Some(OcrDialog { text });
    }
}

impl Default for ClipboardUiState {
    fn default() -> Self {
        Self {
            search: String::new(),
            filter: KindFilter::All,
            focus: None,
            search_focused: false,
            thumbs: ThumbCache::default(),
            edit: None,
            qr: None,
            ocr: None,
            confirm_clear: false,
            scroll_offset: 0.0,
            view_height: 0.0,
            scroll_to: None,
            cache: FilterCache::default(),
        }
    }
}

/// Rascunho do diálogo de edição (o histórico só muda no Salvar).
struct EditDraft {
    id: u128,
    text: String,
}

/// Texto que a OCR leu de uma imagem. O diálogo só mostra e oferece copiar —
/// recopiar a imagem original nunca é o que o usuário quer aqui.
struct OcrDialog {
    text: String,
}

/// Ações pedidas pelo desenho deste frame. Ficam separadas porque mutar o
/// histórico no meio da iteração da lista embaralharia os índices.
#[derive(Default)]
struct Actions {
    focus: Option<u128>,
    copy: Option<u128>,
    pin: Option<u128>,
    edit: Option<u128>,
    qr: Option<u128>,
    /// OCR de uma imagem (o botão só existe no Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    ocr: Option<u128>,
    remove: Option<u128>,
    /// Aviso a mostrar no rodapé em vez de executar a ação (ex.: QR comprido).
    warn: Option<Key>,
}

/// Desenha a aba inteira. Devolve o que o app precisa aplicar (gravar o JSON e
/// mostrar um status).
pub fn show(ui: &mut egui::Ui, lang: Lang, history: &mut History, state: &mut ClipboardUiState) -> Outcome {
    let mut out = Outcome::default();
    let ctx = ui.ctx().clone();
    let clock = Clock::now();

    // ── topo: busca + botão de limpar ─────────────────────────────────
    ui.horizontal(|ui| {
        let clear_w = if state.search.is_empty() { 0.0 } else { 28.0 };
        let width = (ui.available_width() - clear_w).max(120.0);
        let resp = ui.add(kit::line(&mut state.search).hint_text(i18n::t(lang, Key::Search)).desired_width(width));
        state.search_focused = resp.has_focus();
        // "×" (U+00D7, presente nas fontes) limpa a busca; o "✕" (U+2715)
        // saía vazio no Ubuntu/Hack.
        if !state.search.is_empty() && kit::ghost(ui, "×").clicked() {
            state.search.clear();
        }
    });
    ui.add_space(4.0);

    // ── filtro por tipo (pills) + "Limpar" ────────────────────────────
    ui.horizontal(|ui| {
        for filter in KindFilter::ALL {
            if kit::pill(ui, i18n::t(lang, filter.label_key()), state.filter == filter).clicked() {
                state.filter = filter;
            }
        }
        // Ação destrutiva CORRIQUEIRA: ghost na tinta do TEMA (o vermelho fica
        // pro modal que confirma). Encostado à direita da MESMA linha dos
        // filtros; o modal confirma antes de mexer.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = kit::ghost_action(ui, i18n::t(lang, Key::ClearHistory)).on_hover_text(i18n::t(lang, Key::ClearExplain));
            if clear.clicked() {
                state.confirm_clear = true;
            }
        });
    });
    ui.add_space(6.0);

    // Lista filtrada em cache: com busca ativa, rebaixar o texto de centenas de
    // entradas a cada frame custaria caro (HTML grande).
    state.cache.refresh(history, &state.search, state.filter);
    let ids = state.cache.ids.clone();

    let mut actions = Actions::default();
    let modal_open = state.edit.is_some() || state.qr.is_some() || state.ocr.is_some() || state.confirm_clear;

    // ── teclado ───────────────────────────────────────────────────────
    // Esc limpa a busca mesmo com o campo focado (no piloto Esc fechava a
    // janela; aqui a janela é a aba principal, então só a busca).
    if !modal_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.search.clear();
    }
    // Setas/Enter/Del só quando nenhum CAMPO tem foco — digitar texto não pode
    // mover a seleção da lista. (Botão com foco não conta: `search_focused` é
    // específico do campo de busca e dos diálogos, que bloqueiam acima.)
    if !modal_open && !state.search_focused {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            if let Some((row, id)) = move_focus(history, &ids, state.focus, 1) {
                state.focus = Some(id);
                state.scroll_to = Some(row);
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            if let Some((row, id)) = move_focus(history, &ids, state.focus, -1) {
                state.focus = Some(id);
                state.scroll_to = Some(row);
            }
        }
        if state.focus.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            actions.copy = state.focus;
        }
        if state.focus.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            actions.remove = state.focus;
        }
    }

    // ── lista virtualizada ────────────────────────────────────────────
    if !ids.is_empty() {
        let focus = state.focus;
        let thumbs = &mut state.thumbs;
        // Altura visível: a do frame anterior (a mesma área, só que medida
        // depois do cabeçalho) — no primeiro frame cai no espaço disponível.
        let view = if state.view_height > 0.0 { state.view_height } else { ui.available_height() };
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(row) = state.scroll_to.take() {
            // Só rola quando a linha focada saiu da área visível: subir/descer
            // dentro dela não move a lista.
            let top = row as f32 * (ROW_H + ui.spacing().item_spacing.y);
            let bottom = top + ROW_H;
            if top < state.scroll_offset {
                area = area.vertical_scroll_offset(top);
            } else if bottom > state.scroll_offset + view {
                area = area.vertical_scroll_offset(bottom - view);
            }
        }
        let out = area.show_rows(ui, ROW_H, ids.len(), |ui, range| {
            for i in range {
                let Some(entry) = ids.get(i).and_then(|idx| history.entries().get(*idx)) else { continue };
                entry_row(ui, lang, &clock, entry, focus == Some(entry.id), thumbs, &mut actions);
            }
        });
        state.scroll_offset = out.state.clamped_offset().y;
        state.view_height = view;
    } else {
        // Empty-state: área central morta não ajuda ninguém — o usuário sem
        // histórico precisa saber o que o app faz, e quem busca sem achar
        // precisa saber que o filtro é o culpado.
        let msg = if history.entries().is_empty() { Key::EmptyList } else { Key::NoResults };
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(i18n::t(lang, msg)).weak());
        });
    }

    // ── aplica as ações (fora do desenho) ─────────────────────────────
    if let Some(id) = actions.focus {
        state.focus = Some(id);
    }
    if let Some(id) = actions.copy {
        let msg = recopy(history, lang, id);
        if msg.is_some() {
            state.focus = Some(id);
        }
        out.status = msg;
    }
    if let Some(id) = actions.pin {
        history.toggle_pin(id);
        out.changed = true;
        let pinned = history.entries().iter().find(|e| e.id == id).is_some_and(|e| e.pinned);
        out.status = Some(i18n::t(lang, if pinned { Key::Pinned } else { Key::Unpinned }).to_string());
    }
    // Só Texto edita: HTML/RTF são fragmentos com marcação, editá-los como
    // texto puro corromperia o formato.
    if let Some(entry) = actions.edit.and_then(|id| history.entries().iter().find(|e| e.id == id)).filter(|e| e.kind == Kind::Text) {
        state.edit = Some(EditDraft { id: entry.id, text: entry.text.clone().unwrap_or_default() });
    }
    if let Some(text) = actions.qr.and_then(|id| history.entries().iter().find(|e| e.id == id)).and_then(qr_source) {
        state.qr = Some(text.to_string());
    }
    if let Some(id) = actions.ocr {
        out.ocr = Some(id);
    }
    if let Some(key) = actions.warn {
        out.status = Some(i18n::t(lang, key).to_string());
    }
    if let Some(id) = actions.remove {
        history.remove(id);
        out.changed = true;
        state.focus = state.focus.filter(|focused| *focused != id);
    }

    // ── diálogos (por cima da lista) ──────────────────────────────────
    edit_modal(&ctx, lang, state, history, &mut out);
    qr_modal(&ctx, lang, state);
    ocr_modal(&ctx, lang, state, history, &mut out);
    clear_modal(&ctx, lang, state, history, &mut out);

    // Mutação (pin/remover/editar) pode mudar o que casa com a busca/filtro
    // sem mexer no tamanho da lista: força o recálculo no próximo frame.
    if out.changed {
        state.cache.filter = None;
    }

    out
}

/// Recopia a entrada e devolve a mensagem do rodapé (✓ ou ⚠ + motivo do SO).
fn recopy(history: &History, lang: Lang, id: u128) -> Option<String> {
    let entry = history.entries().iter().find(|e| e.id == id)?;
    Some(match clipboard::write_back(entry) {
        Ok(()) => i18n::t(lang, Key::CopiedOk).to_string(),
        Err(e) => format!("⚠ {}: {e}", i18n::t(lang, Key::CopyFailed)),
    })
}

/// Copia um texto NOVO: cria a entrada `Text`, empilha no histórico e põe na
/// área de transferência. É o caminho do "Copiar texto" do OCR e o que a aba
/// Ferramentas usa (conta, conversão, hash, UUID): o resultado vira algo colável
/// de novo, com entrada própria na lista.
///
/// Devolve `(o histórico mudou, status pronto pro rodapé)`.
pub(crate) fn copy_new_text(history: &mut History, lang: Lang, text: &str) -> (bool, String) {
    let entry = Entry {
        id: crate::history::new_id(),
        ts_ms: crate::history::now_ms(),
        kind: Kind::Text,
        pinned: false,
        text: Some(text.to_string()),
        files: None,
        image_png: None,
        thumb_png: None,
    };
    history.push(entry.clone());
    let status = match clipboard::write_back(&entry) {
        Ok(()) => i18n::t(lang, Key::CopiedOk).to_string(),
        Err(e) => format!("⚠ {}: {e}", i18n::t(lang, Key::CopyFailed)),
    };
    (true, status)
}

/// Move o foco na lista visível e devolve (índice na lista, id). Sem foco
/// atual, ↓ começa no topo e ↑ no fim; nas pontas o foco fica parado (nada de
/// dar a volta, que confunde).
fn move_focus(history: &History, ids: &[usize], focus: Option<u128>, delta: isize) -> Option<(usize, u128)> {
    let entries = history.entries();
    let current = focus.and_then(|id| ids.iter().position(|i| entries.get(*i).is_some_and(|e| e.id == id)));
    let last = ids.len().checked_sub(1)?;
    let next = match current {
        Some(pos) => pos.saturating_add_signed(delta).min(last),
        None if delta > 0 => 0,
        None => last,
    };
    ids.get(next).and_then(|i| entries.get(*i)).map(|e| (next, e.id))
}

/// Texto que pode virar QR: só o conteúdo textual (o caso de uso é URL/trecho
/// curto; imagem e lista de arquivos não têm o que codificar).
fn qr_source(entry: &Entry) -> Option<&str> {
    match entry.kind {
        Kind::Text | Kind::Html => entry.text.as_deref(),
        _ => None,
    }
}

/// Uma linha da lista. Altura fixa: o `show_rows` só desenha as visíveis.
fn entry_row(ui: &mut egui::Ui, lang: Lang, clock: &Clock, entry: &Entry, focused: bool, thumbs: &mut ThumbCache, actions: &mut Actions) {
    let skin = Skin::of(ui);
    let width = ui.available_width().max(1.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    if resp.clicked() {
        actions.focus = Some(entry.id);
    }
    // Clique direito: as mesmas ações das ghost buttons (descoberta).
    let _ = resp.context_menu(|ui| row_menu(ui, lang, entry, actions));

    // Fundo: foco > hover > nada (o "ghost" da suíte — a lista fica limpa).
    let bg = if focused {
        skin.accent.gamma_multiply(0.18)
    } else if resp.hovered() {
        skin.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    let body = rect.shrink2(egui::vec2(0.0, 2.0));
    ui.painter().rect_filled(body, egui::CornerRadius::same(8), bg);
    if focused {
        ui.painter().rect_stroke(body, egui::CornerRadius::same(8), egui::Stroke::new(1.0_f32, skin.accent), egui::StrokeKind::Inside);
    }
    if !ui.is_rect_visible(rect) {
        return;
    }

    let inner = rect.shrink2(egui::vec2(10.0, 6.0));
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));

    // Etiqueta do tipo / miniatura.
    let (badge, _) = row.allocate_exact_size(egui::vec2(BADGE, BADGE), egui::Sense::hover());
    draw_badge(&row, badge, entry, thumbs);
    row.add_space(10.0);

    // Coluna do meio: tipo + carimbo em cima, preview embaixo.
    let text_w = (row.available_width() - ACTIONS_W).max(80.0);
    row.allocate_ui_with_layout(egui::vec2(text_w, BADGE), egui::Layout::top_down(egui::Align::LEFT), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(kind_label(lang, entry.kind)).small().color(skin.muted));
            ui.label(egui::RichText::new("·").small().color(skin.muted));
            ui.label(egui::RichText::new(clock.stamp(lang, entry.ts_ms)).small().color(skin.muted));
        });
        ui.add(egui::Label::new(egui::RichText::new(preview_text(entry)).color(skin.text)).truncate());
    });

    // Direita: ★ sempre visível (fixado ou não — o pin é a ação que mais se
    // usa); o resto só no hover/foco.
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let star = if entry.pinned { "★" } else { "☆" };
        // Preenchida + accent quando fixada; contorno em tinta forte (bem mais
        // legível que o `muted` de antes) quando não.
        let ink = if entry.pinned { skin.accent } else { skin.text.gamma_multiply(0.75) };
        let tip = if entry.pinned { Key::Unpin } else { Key::Pin };
        if kit::ghost_ink(ui, star, ink).on_hover_text(i18n::t(lang, tip)).clicked() {
            actions.pin = Some(entry.id);
        }
        if !(resp.hovered() || focused) {
            return;
        }
        if kit::ghost_action(ui, "🗑").on_hover_text(i18n::t(lang, Key::Remove)).clicked() {
            actions.remove = Some(entry.id);
        }
        if let Some(text) = qr_source(entry) {
            // Acima do limite o botão não abre o QR: avisa no rodapé. Fica
            // "apagado" (tinta fraca) mas continua clicável — desabilitado de
            // verdade, o egui não mostra tooltip.
            let too_long = text.chars().count() > QR_MAX_CHARS;
            let ink = if too_long { skin.muted.gamma_multiply(0.5) } else { skin.muted };
            let tip = if too_long { Key::QrTooLong } else { Key::QRCode };
            // "QR" por extenso: o glifo ▦ (U+25A6) não existe no Ubuntu e
            // renderizava vazio (mesma classe de bug do ✕ do popup).
            if kit::ghost_ink(ui, "QR", ink).on_hover_text(i18n::t(lang, tip)).clicked() {
                if too_long {
                    actions.warn = Some(Key::QrTooLong);
                } else {
                    actions.qr = Some(entry.id);
                }
            }
        }
        if entry.kind == Kind::Text && kit::ghost(ui, "✏").on_hover_text(i18n::t(lang, Key::Edit)).clicked() {
            actions.edit = Some(entry.id);
        }
        // OCR só faz sentido em imagem e só existe no Windows (motores do SO).
        #[cfg(windows)]
        {
            if entry.kind == Kind::Image && kit::ghost(ui, "OCR").on_hover_text(i18n::t(lang, Key::OcrButton)).clicked() {
                actions.ocr = Some(entry.id);
            }
        }
    });
}

/// Menu de contexto da linha (clique direito).
fn row_menu(ui: &mut egui::Ui, lang: Lang, entry: &Entry, actions: &mut Actions) {
    if ui.button(i18n::t(lang, Key::CopyAgain)).clicked() {
        actions.copy = Some(entry.id);
        ui.close();
    }
    if entry.kind == Kind::Text && ui.button(i18n::t(lang, Key::Edit)).clicked() {
        actions.edit = Some(entry.id);
        ui.close();
    }
    if qr_source(entry).is_some() && ui.button(i18n::t(lang, Key::QRCode)).clicked() {
        actions.qr = Some(entry.id);
        ui.close();
    }
    if ui.button(i18n::t(lang, Key::Remove)).clicked() {
        actions.remove = Some(entry.id);
        ui.close();
    }
}

/// Etiqueta do tipo: miniatura (imagem) ou o glifo do formato.
fn draw_badge(ui: &egui::Ui, rect: egui::Rect, entry: &Entry, thumbs: &mut ThumbCache) {
    let skin = Skin::of(ui);
    ui.painter().rect_filled(rect, egui::CornerRadius::same(8), skin.hover);
    if paint_thumb(ui, rect, entry, thumbs) {
        return;
    }
    let glyph = match entry.kind {
        Kind::Text => "T",
        Kind::Html => "</>",
        Kind::Rtf => "RTF",
        Kind::Image => "🖼",
        Kind::Files => "📁",
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, font, skin.muted);
}

/// Pinta a miniatura encaixada na etiqueta. `false` = não há miniatura (entrada
/// que não é imagem, PNG corrompido ou textura vazia): cai no glifo.
fn paint_thumb(ui: &egui::Ui, rect: egui::Rect, entry: &Entry, thumbs: &mut ThumbCache) -> bool {
    if entry.kind != Kind::Image {
        return false;
    }
    let Some(tex) = thumbs.texture(ui.ctx(), entry) else { return false };
    let size = tex.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return false;
    }
    // Encaixa sem distorcer: a proporção é da imagem, não da etiqueta.
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let draw = egui::Rect::from_center_size(rect.shrink(2.0).center(), size * scale);
    ui.painter().image(tex.id(), draw, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    true
}

/// Rótulo traduzido do tipo (linha de cima da entrada). Compartilhado com o
/// popup rápido, que desenha a linha em versão slim.
pub(crate) fn kind_label(lang: Lang, kind: Kind) -> &'static str {
    let key = match kind {
        Kind::Text => Key::TypeText,
        Kind::Html => Key::TypeHtml,
        Kind::Rtf => Key::TypeRtf,
        Kind::Image => Key::TypeImage,
        Kind::Files => Key::TypeFiles,
    };
    i18n::t(lang, key)
}

/// Nome de um caminho copiado (sem a pasta).
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Preview de uma linha: primeira linha do texto, `arquivo +n` ou nada (a
/// imagem fala pela miniatura). O popup rápido mostra o mesmo preview.
pub(crate) fn preview_text(entry: &Entry) -> String {
    match entry.kind {
        Kind::Image => String::new(),
        Kind::Files => {
            let files = entry.files.as_deref().unwrap_or_default();
            let first = files.first().map_or("", |f| file_name(f));
            match files.len().checked_sub(1) {
                Some(rest) if rest > 0 => format!("{first}  +{rest}"),
                _ => first.to_string(),
            }
        }
        Kind::Text | Kind::Html | Kind::Rtf => {
            entry.text.as_deref().map(|t| t.trim_start().lines().next().unwrap_or_default().to_string()).unwrap_or_default()
        }
    }
}

/// Deslocamento do fuso local (ms). Usa o relógio do SO uma vez por frame: um
/// item copiado do outro lado de uma virada de horário de verão pode mostrar 1h
/// de erro (o deslocamento aplicado é o de agora).
#[cfg(windows)]
fn local_offset_ms() -> i64 {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime devolve a struct por valor; sem ponteiro nem
    // pré-condição.
    let st = unsafe { GetLocalTime() };
    let local = days_from_civil(i64::from(st.wYear), i64::from(st.wMonth), i64::from(st.wDay)) * 86_400_000
        + i64::from(st.wHour) * 3_600_000
        + i64::from(st.wMinute) * 60_000
        + i64::from(st.wSecond) * 1_000;
    local.saturating_sub(i64::try_from(crate::history::now_ms()).unwrap_or(0))
}

#[cfg(not(windows))]
fn local_offset_ms() -> i64 {
    // Fora do Windows o histórico ainda nem funciona: UTC serve.
    0
}

/// Relógio do frame: agora no fuso local e o deslocamento a aplicar nas
/// entradas. Lido uma vez por frame (o fuso não muda no meio da lista). O
/// popup rápido usa o mesmo carimbo da lista.
#[derive(Clone, Copy)]
pub(crate) struct Clock {
    now_local_ms: i64,
    offset_ms: i64,
}

impl Clock {
    pub(crate) fn now() -> Self {
        let utc = i64::try_from(crate::history::now_ms()).unwrap_or(0);
        let offset_ms = local_offset_ms();
        Self { now_local_ms: utc.saturating_add(offset_ms), offset_ms }
    }

    /// Agora no fuso local (epoch-ms). A aba Ferramentas (alarmes, timestamps)
    /// usa o MESMO deslocamento dos carimbos da lista.
    pub(crate) fn local_now_ms(&self) -> i64 {
        self.now_local_ms
    }

    /// Deslocamento do fuso local (ms) — converter epoch ↔ hora civil fora dos
    /// carimbos da lista.
    pub(crate) fn offset_ms(&self) -> i64 {
        self.offset_ms
    }

    /// Carimbo relativo: agora / H:mm / ontem / dd/mm.
    pub(crate) fn stamp(&self, lang: Lang, ts_ms: u64) -> String {
        let ts = i64::try_from(ts_ms).unwrap_or(0).saturating_add(self.offset_ms);
        let (y, m, d, hh, mm) = civil_parts(ts);
        let (ny, nm, nd, _, _) = civil_parts(self.now_local_ms);
        if (y, m, d) == (ny, nm, nd) {
            if self.now_local_ms.saturating_sub(ts) < 60_000 {
                return i18n::t(lang, Key::TsNow).to_string();
            }
            return i18n::t(lang, Key::TsTime).replace("{h}", &hh.to_string()).replace("{mm}", &format!("{mm:02}"));
        }
        if days_of(ts).saturating_add(1) == days_of(self.now_local_ms) {
            return i18n::t(lang, Key::TsYesterday).to_string();
        }
        i18n::t(lang, Key::TsDate).replace("{d}", &format!("{d:02}")).replace("{month}", &format!("{m:02}"))
    }
}

/// Dia civil (epoch-ms já local) → número de dias desde a época.
fn days_of(ms: i64) -> i64 {
    ms.div_euclid(86_400_000)
}

/// Decomposição civil de um epoch-ms local: (ano, mês, dia, hora, minuto). O
/// algoritmo `civil_from_days` é o do Hinnant (o mesmo que o chrono usa) — o
/// app não puxa crate de data só pra isso. `pub(crate)`: a aba Ferramentas
/// (alarmes e timestamps) decompõe com o mesmo algoritmo.
pub(crate) fn civil_parts(ms: i64) -> (i64, i64, i64, i64, i64) {
    let days = days_of(ms);
    let rem = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, rem / 3_600_000, (rem % 3_600_000) / 60_000)
}

/// Dia (epoch-days) → (ano, mês, dia). `pub(crate)`: o export do `.lutils`
/// carimba a data do nome do arquivo com o mesmo algoritmo.
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Só o Windows precisa disto (o offset local vem do relógio do SO); no Linux
/// o histórico ainda é stub, então a fn não existe e não sobra código morto.
#[cfg(windows)]
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Cache das texturas de miniatura. O `thumb_png` (256px) é decodificado na
/// primeira vez que a entrada aparece e evicted por LRU quando o cache enche —
/// o PNG original nunca é decodificado pra lista (RAM manda).
#[derive(Default)]
struct ThumbCache {
    /// `None` = já tentamos e o PNG não decodificou (não tenta de novo a cada
    /// frame).
    map: HashMap<u128, Option<egui::TextureHandle>>,
    /// Ordem de recência: o fim é o mais novo.
    order: VecDeque<u128>,
}

impl ThumbCache {
    /// Textura da miniatura da entrada (carregando se for a primeira vez).
    fn texture(&mut self, ctx: &egui::Context, entry: &Entry) -> Option<&egui::TextureHandle> {
        if !self.map.contains_key(&entry.id) {
            let tex = entry.thumb_png.as_deref().and_then(|png| decode_thumb(ctx, entry.id, png));
            self.evict();
            self.map.insert(entry.id, tex);
        }
        // Toca a entrada (LRU): a miniatura visível não é a próxima a sair.
        if let Some(pos) = self.order.iter().position(|id| *id == entry.id) {
            self.order.remove(pos);
        }
        self.order.push_back(entry.id);
        self.map.get(&entry.id).and_then(|tex| tex.as_ref())
    }

    /// Abre espaço antes de inserir: descarta a miniatura usada há mais tempo.
    fn evict(&mut self) {
        while self.map.len() >= THUMB_CACHE {
            let Some(old) = self.order.pop_front() else { break };
            self.map.remove(&old);
        }
    }
}

/// Decodifica a miniatura e sobe a textura (só roda uma vez por entrada).
fn decode_thumb(ctx: &egui::Context, id: u128, png: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    // A textura sobe no fim deste frame: sem o repaint a miniatura só
    // apareceria no próximo evento do usuário.
    ctx.request_repaint();
    Some(ctx.load_texture(format!("clip-thumb-{id}"), color, egui::TextureOptions::LINEAR))
}

/// Diálogo de edição do texto. Salvar só mexe no histórico: recopiar é ação
/// explícita (o usuário pode estar editando justamente pra não recopiar).
fn edit_modal(ctx: &egui::Context, lang: Lang, state: &mut ClipboardUiState, history: &mut History, out: &mut Outcome) {
    // O rascunho sai do estado enquanto o modal está aberto (o app não pode
    // segurar um `&mut` enquanto a UI roda) e volta no fim do frame.
    let Some(mut draft) = state.edit.take() else { return };
    let mut save = false;
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("clip-edit")).frame(kit::modal_frame(ctx)).show(ctx, |ui| {
        kit::begin_dialog(ui);
        ui.set_width(440.0);
        ui.heading(i18n::t(lang, Key::Edit));
        ui.add_space(10.0);
        egui::ScrollArea::vertical().id_salt("clip-edit-scroll").max_height(320.0).show(ui, |ui| {
            ui.add(kit::block(&mut draft.text).desired_width(400.0).desired_rows(10));
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::pill(ui, i18n::t(lang, Key::Save), true).clicked() {
                    save = true;
                }
                if kit::pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                    cancel = true;
                }
            });
        });
    });
    if save {
        // Silencioso de propósito: a lista já mostra o texto novo (e o status
        // do rodapé fica livre pro resultado de um recopiar).
        let written = history.set_text(draft.id, draft.text);
        out.changed |= written;
    } else if !(cancel || resp.should_close()) {
        state.edit = Some(draft);
    }
}

/// Diálogo do QR: a matriz é desenhada como retângulos (sem dep de imagem).
fn qr_modal(ctx: &egui::Context, lang: Lang, state: &mut ClipboardUiState) {
    let Some(text) = state.qr.take() else { return };
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("clip-qr")).frame(kit::modal_frame(ctx)).show(ctx, |ui| {
        kit::begin_dialog(ui);
        ui.set_width(QR_SIZE + 40.0);
        ui.heading(i18n::t(lang, Key::QRCode));
        ui.add_space(10.0);
        draw_qr(ui, lang, &text);
        ui.add_space(10.0);
        // O começo do conteúdo em texto, pra conferir que é o item certo.
        ui.add(egui::Label::new(egui::RichText::new(text.lines().next().unwrap_or_default()).small().weak()).truncate());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                    cancel = true;
                }
            });
        });
    });
    if !(cancel || resp.should_close()) {
        state.qr = Some(text);
    }
}

/// Diálogo do OCR: o texto que o motor do Windows leu da imagem. "Copiar
/// texto" grava o texto como uma entrada NOVA do histórico (é o resultado que o
/// usuário quer reusar) e já o põe no clipboard.
fn ocr_modal(ctx: &egui::Context, lang: Lang, state: &mut ClipboardUiState, history: &mut History, out: &mut Outcome) {
    let Some(dialog) = state.ocr.take() else { return };
    let mut copy = false;
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("clip-ocr")).frame(kit::modal_frame(ctx)).show(ctx, |ui| {
        kit::begin_dialog(ui);
        ui.set_width(460.0);
        ui.heading(i18n::t(lang, Key::OcrButton));
        ui.add_space(10.0);
        egui::ScrollArea::vertical().id_salt("clip-ocr-scroll").max_height(320.0).show(ui, |ui| {
            // Read-only de verdade: o texto lido é resultado, não rascunho —
            // editar aqui não teria onde gravar.
            let mut text = dialog.text.clone();
            ui.add(kit::block(&mut text).interactive(false).desired_width(420.0).desired_rows(12));
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::pill(ui, i18n::t(lang, Key::CopyText), true).clicked() {
                    copy = true;
                }
                if kit::pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                    cancel = true;
                }
            });
        });
    });
    if copy {
        // Entrada nova de texto: o histórico é a lista de "coisas que dá pra
        // colar de novo", e o OCR é exatamente isso daqui pra frente.
        let entry = Entry {
            id: crate::history::new_id(),
            ts_ms: crate::history::now_ms(),
            kind: Kind::Text,
            pinned: false,
            text: Some(dialog.text.clone()),
            files: None,
            image_png: None,
            thumb_png: None,
        };
        history.push(entry.clone());
        out.changed = true;
        out.status = Some(match clipboard::write_back(&entry) {
            Ok(()) => i18n::t(lang, Key::CopiedOk).to_string(),
            Err(e) => format!("⚠ {}: {e}", i18n::t(lang, Key::CopyFailed)),
        });
    } else if !(cancel || resp.should_close()) {
        state.ocr = Some(dialog);
    }
}

/// Confirmação do "Limpar": só o histórico NÃO fixado sai; as entradas fixadas
/// ficam (o modal diz isso antes de o usuário confirmar).
fn clear_modal(ctx: &egui::Context, lang: Lang, state: &mut ClipboardUiState, history: &mut History, out: &mut Outcome) {
    if !state.confirm_clear {
        return;
    }
    let mut confirm = false;
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("clip-clear")).frame(kit::modal_frame(ctx)).show(ctx, |ui| {
        kit::begin_dialog(ui);
        ui.set_width(360.0);
        ui.heading(i18n::t(lang, Key::ClearAll));
        ui.add_space(10.0);
        ui.label(i18n::t(lang, Key::ClearExplain));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::pill_danger(ui, i18n::t(lang, Key::ClearAll)).clicked() {
                    confirm = true;
                }
                if kit::pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                    cancel = true;
                }
            });
        });
    });
    if confirm {
        history.clear_unpinned();
        out.changed = true;
        // O foco pode ter ficado numa entrada que acabou de sair da lista.
        state.focus = None;
        out.status = Some(i18n::t(lang, Key::HistoryCleared).to_string());
        state.confirm_clear = false;
    } else if cancel || resp.should_close() {
        state.confirm_clear = false;
    }
}

/// Desenha a matriz do QR (preto sobre branco: as duas cores são do CÓDIGO, não
/// do tema — o leitor precisa de contraste puro).
fn draw_qr(ui: &mut egui::Ui, lang: Lang, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(QR_SIZE, QR_SIZE), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), egui::Color32::WHITE);
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let msg = i18n::t(lang, Key::QrFailed);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, msg, font, egui::Color32::from_gray(60));
        return;
    };
    let modules = code.width();
    if modules == 0 {
        return;
    }
    let colors = code.to_colors();
    // Margem de silêncio (4 módulos) que todo leitor espera em volta.
    let quiet = 4usize;
    let total = modules + quiet * 2;
    let side = (rect.width() / total as f32).floor().max(1.0);
    let drawn = side * total as f32;
    let origin = egui::pos2(rect.center().x - drawn / 2.0, rect.center().y - drawn / 2.0);
    for (y, line) in colors.chunks(modules).enumerate().take(modules) {
        // Módulos escuros vizinhos viram UM retângulo (bem menos shapes).
        let mut run: Option<usize> = None;
        for (x, color) in line.iter().enumerate() {
            if *color == qrcode::types::Color::Dark {
                run.get_or_insert(x);
            } else if let Some(start) = run.take() {
                paint_qr_run(ui, origin, side, quiet, start, x, y);
            }
        }
        if let Some(start) = run {
            paint_qr_run(ui, origin, side, quiet, start, line.len(), y);
        }
    }
}

/// Um trecho horizontal escuro do QR, já deslocado pela margem de silêncio.
fn paint_qr_run(ui: &egui::Ui, origin: egui::Pos2, side: f32, quiet: usize, x0: usize, x1: usize, y: usize) {
    let min = egui::pos2(origin.x + (x0 + quiet) as f32 * side, origin.y + (y + quiet) as f32 * side);
    let rect = egui::Rect::from_min_size(min, egui::vec2((x1 - x0) as f32 * side, side));
    ui.painter().rect_filled(rect, egui::CornerRadius::same(0), egui::Color32::BLACK);
}

/// A lista filtrada, recomputada só quando algo muda (busca, filtro ou o começo
/// e o fim do histórico) — rebaixar o texto de tudo a cada frame pesaria.
#[derive(Default)]
struct FilterCache {
    /// Busca (crua) com que a lista foi montada.
    search: String,
    filter: Option<KindFilter>,
    len: usize,
    first: Option<u128>,
    last: Option<u128>,
    /// Índices das entradas visíveis, na ordem do histórico.
    ids: Vec<usize>,
}

impl FilterCache {
    fn refresh(&mut self, history: &History, search: &str, filter: KindFilter) {
        let entries = history.entries();
        let len = entries.len();
        let first = entries.first().map(|e| e.id);
        let last = entries.last().map(|e| e.id);
        if self.filter == Some(filter) && self.search == search && self.len == len && self.first == first && self.last == last {
            return;
        }
        let needle = normalize_search(search);
        self.ids = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| filter.accepts(entry.kind))
            .filter(|(_, entry)| needle.is_empty() || entry_matches(entry, &needle))
            .map(|(i, _)| i)
            .collect();
        self.search = search.to_string();
        self.filter = Some(filter);
        self.len = len;
        self.first = first;
        self.last = last;
    }
}

/// A busca normalizada (minúsculas, sem espaço nas pontas) — o critério é o
/// mesmo na aba Clipboard e no popup rápido (`popup.rs`), então a conta mora
/// aqui.
pub(crate) fn normalize_search(search: &str) -> String {
    search.trim().to_lowercase()
}

/// Texto pesquisável de uma cópia, já em minúsculas: o conteúdo textual mais
/// os NOMES dos arquivos copiados (a pasta não entra — ninguém busca por
/// `C:\Users`). Quem guarda o retrato da lista (o popup) chama isto uma vez por
/// entrada em vez de rebaixar o texto a cada frame.
pub(crate) fn search_haystack(text: Option<&str>, files: Option<&[String]>) -> String {
    let mut hay = text.map(str::to_lowercase).unwrap_or_default();
    if let Some(files) = files {
        for file in files {
            hay.push('\n');
            hay.push_str(&file_name(file).to_lowercase());
        }
    }
    hay
}

/// O texto pesquisável casa com a busca já normalizada?
pub(crate) fn text_matches(haystack: &str, needle: &str) -> bool {
    needle.is_empty() || haystack.contains(needle)
}

/// A entrada casa com a busca? Delega pro critério compartilhado com o popup.
fn entry_matches(entry: &Entry, needle: &str) -> bool {
    text_matches(&search_haystack(entry.text.as_deref(), entry.files.as_deref()), needle)
}
