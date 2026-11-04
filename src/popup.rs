//! Popup rápido no cursor: o atalho global abre ESTE viewport em vez da janela
//! principal (config `popup_instead_of_window`, ligado por padrão).
//!
//! Por que um viewport separado: a janela principal é a "casa" do app (abas,
//! opções, updater) e não deve aparecer pra quem só quer recolar a última
//! cópia. O popup nasce sem decoração, sempre no topo, colado no cursor e fora
//! da barra de tarefas (a bandeja continua sendo a da janela principal).
//!
//! Mecânica: `show_viewport_deferred` cria uma janela nativa do MESMO app — o
//! processo pode estar só na bandeja, que a janela principal continua escondida
//! (o eframe deste fork roda o frame e pinta viewports invisíveis direto; ver
//! `check_redraw_requests` no `run.rs` do eframe). O callback do viewport é
//! `Fn + Send + Sync + 'static`, então o estado que o campo de busca e a lista
//! compartilham com o app vive num `Arc<Mutex<..>>` ([`Popup`]).
//!
//! O app é o dono da decisão: ele chama [`Popup::sync`] (retrato da lista) e
//! [`show`] a cada frame enquanto o popup estiver aberto, e aplica o comando que
//! o popup deixou ([`Popup::take_cmd`]) — recopiar e colar é do app, que tem o
//! histórico.

use std::sync::{Arc, Mutex};

use eframe::egui;
use utils_ui::i18n::{self, Key, Lang};
use utils_ui::workarea;

use crate::clipboard_ui::{self, Clock};
use crate::history::{Entry, History, Kind};
use crate::ui::{self as kit, Skin};

/// Tamanho do popup (points). Sem decoração, o outer é o próprio conteúdo.
const POPUP_W: f32 = 560.0;
const POPUP_H: f32 = 460.0;
/// Distância entre o cursor e o canto do popup.
const CURSOR_GAP: f32 = 10.0;
/// Altura da linha compacta (só tipo + preview + carimbo). Folga interna
/// generosa: texto apertado em linha de 40px cansava (feedback do João).
const ROW_H: f32 = 50.0;
/// Quantas cópias o popup lista (as mais recentes que casam com a busca).
const MAX_ROWS: usize = 12;
/// Quantas entradas entram no retrato da lista. A busca do popup só enxerga o
/// que está aqui: 200 cobre o uso real e evita rebaixar o texto de um histórico
/// de 500 itens a cada cópia nova.
const MAX_SNAPSHOT: usize = 200;
/// Lado do botão de fechar (área de clique folgada) e a largura reservada pra
/// ele na fileira da busca.
const CLOSE_BTN: f32 = 30.0;
/// Quanto do texto entra na busca. Um Ctrl+V com megabytes de texto não pode
/// virar 200 MB rebaixados no retrato (buscar além disto não é uso real).
const MAX_HAYSTACK: usize = 4096;
/// Título PRÓPRIO de propósito: `winctl::discover` acha a janela principal pelo
/// título exato ("Local Utils") — o popup não pode se passar por ela.
const POPUP_TITLE: &str = "Local Utils — popup";

/// Id do viewport. Fixo: o mesmo popup reusa a mesma janela enquanto aberto.
fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("local-utils-popup")
}

/// O que o popup pede pro app depois de desenhar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    /// Enter/clique: recopiar a entrada e colar na janela que estava em foco.
    Paste(u128),
}

/// Uma linha do retrato: o mínimo que a lista slim desenha + o texto da busca.
#[derive(Clone)]
struct Row {
    id: u128,
    kind: Kind,
    preview: String,
    ts_ms: u64,
    /// Texto pesquisável já normalizado (`clipboard_ui::search_haystack`).
    haystack: String,
}

/// Estado compartilhado entre o app e o callback do viewport.
#[derive(Default)]
struct State {
    open: bool,
    /// Busca: vive aqui porque o campo roda dentro do viewport.
    search: String,
    /// Posição do foco na lista FILTRADA (`None` = lista vazia).
    focus: Option<usize>,
    /// Pede scroll-into-view da linha focada — só quando o foco VEIO DAS SETAS.
    /// Rolá-la a cada frame brigava com a roda do usuário (bug pago: a lista
    /// "voltava pro topo" ao tentar descer com o scroll).
    scroll_to: Option<usize>,
    rows: Vec<Row>,
    /// Canto do popup nesta abertura. Calculado no [`Popup::open`]: se o cursor
    /// decidisse a cada frame, o popup seguiria o mouse enquanto aberto.
    pos: Option<egui::Pos2>,
    /// Assinatura do histórico com que `rows` foi montado (ver [`Popup::sync`]).
    sig: Option<(usize, Option<u128>, Option<u128>)>,
    /// Comando pendente pro app.
    cmd: Option<Cmd>,
    /// O campo de busca já pediu foco nesta abertura.
    focus_requested: bool,
    /// O `ViewportCommand::Focus` já foi pedido nesta abertura.
    focus_sent: bool,
    /// O viewport já esteve em primeiro plano alguma vez: só depois disso
    /// "perder o foco" quer dizer que o usuário clicou fora.
    had_focus: bool,
}

impl State {
    /// Fecha o popup pedindo um frame do app: é o app que decide parar de
    /// mostrar o viewport (o `show_viewport_deferred` deixa de ser chamado e o
    /// eframe derruba a janela).
    fn close_with(&mut self, ctx: &egui::Context, cmd: Option<Cmd>) {
        self.open = false;
        self.cmd = cmd;
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Handle do popup que vive no app (`Arc` porque o callback do viewport precisa
/// de `'static`).
#[derive(Clone, Default)]
pub struct Popup {
    state: Arc<Mutex<State>>,
}

impl Popup {
    /// Abre (chamado quando o atalho dispara): busca e foco zerados, lista
    /// remontada no próximo `sync` e canto calculado no cursor de AGORA.
    pub fn open(&self, ctx: &egui::Context) {
        let pos = anchored(ctx);
        if let Ok(mut st) = self.state.lock() {
            st.open = true;
            st.search.clear();
            st.focus = None;
            st.scroll_to = None;
            st.cmd = None;
            st.focus_requested = false;
            st.focus_sent = false;
            st.had_focus = false;
            st.sig = None;
            st.pos = Some(pos);
        }
    }

    pub fn is_open(&self) -> bool {
        self.state.lock().is_ok_and(|st| st.open)
    }

    /// Fecha sem colar (o app fecha quando o comando chega).
    pub fn close(&self) {
        if let Ok(mut st) = self.state.lock() {
            st.open = false;
            st.cmd = None;
        }
    }

    /// Comando deixado pelo popup (Enter/clique/Esc/clique fora).
    pub fn take_cmd(&self) -> Option<Cmd> {
        self.state.lock().ok().and_then(|mut st| st.cmd.take())
    }

    /// Canto combinado na abertura.
    fn position(&self, ctx: &egui::Context) -> egui::Pos2 {
        self.state.lock().ok().and_then(|st| st.pos).unwrap_or_else(|| anchored(ctx))
    }

    /// Pede foco UMA vez por abertura (o SO pode recusar quando o app está em
    /// segundo plano; insistir a cada frame viraria loop de repaint).
    fn take_focus_request(&self) -> bool {
        match self.state.lock() {
            Ok(mut st) if st.open && !st.focus_sent => {
                st.focus_sent = true;
                true
            }
            _ => false,
        }
    }

    /// Remonta o retrato da lista quando o histórico mudou (cópia nova, pin,
    /// remoção). Mesma assinatura do `FilterCache` da aba: tamanho + pontas.
    pub fn sync(&self, history: &History) {
        let entries = history.entries();
        let sig = (entries.len(), entries.first().map(|e| e.id), entries.last().map(|e| e.id));
        let Ok(mut st) = self.state.lock() else { return };
        if st.sig == Some(sig) {
            return;
        }
        st.rows = entries.iter().take(MAX_SNAPSHOT).map(row_of).collect();
        st.sig = Some(sig);
    }
}

/// Recorte de uma entrada pro popup.
fn row_of(entry: &Entry) -> Row {
    let mut haystack = clipboard_ui::search_haystack(entry.text.as_deref(), entry.files.as_deref());
    if haystack.chars().count() > MAX_HAYSTACK {
        haystack = haystack.chars().take(MAX_HAYSTACK).collect();
    }
    Row { id: entry.id, kind: entry.kind, preview: clipboard_ui::preview_text(entry), ts_ms: entry.ts_ms, haystack }
}

/// Índices (na ordem do retrato = mais recente primeiro) das linhas que casam
/// com a busca, no máximo [`MAX_ROWS`]. Pura e com o MESMO critério da aba
/// Clipboard — é ela que o teste cobre.
fn matching(rows: &[Row], search: &str) -> Vec<usize> {
    let needle = clipboard_ui::normalize_search(search);
    rows.iter().enumerate().filter(|(_, row)| clipboard_ui::text_matches(&row.haystack, &needle)).map(|(i, _)| i).take(MAX_ROWS).collect()
}

/// Desenha o viewport do popup (só quando aberto).
pub fn show(ctx: &egui::Context, lang: Lang, popup: &Popup) {
    if !popup.is_open() {
        return;
    }
    let state = Arc::clone(&popup.state);
    // Canto fixado na abertura (ver `State::pos`); o fallback cobre o caso de o
    // app ter perdido o lock no meio.
    let pos = popup.position(ctx);
    let builder = egui::ViewportBuilder::default()
        .with_title(POPUP_TITLE)
        .with_inner_size([POPUP_W, POPUP_H])
        .with_position(pos)
        .with_decorations(false)
        .with_resizable(false)
        // Fora da barra de tarefas: o popup é uma sobreposição, não uma janela
        // do app (a bandeja continua sendo a da janela principal).
        .with_taskbar(false)
        .with_always_on_top()
        // Nasce em primeiro plano: o campo de busca precisa do teclado.
        .with_active(true);
    ctx.show_viewport_deferred(viewport_id(), builder, move |ui, class| {
        if class == egui::ViewportClass::EmbeddedWindow {
            // Backend sem viewports múltiplos: não há como flutuar no cursor.
            // Fechar é melhor que virar uma janela dentro da janela.
            if let Ok(mut st) = state.lock() {
                st.close_with(ui.ctx(), None);
            }
            return;
        }
        draw(ui, lang, &state);
    });
    // Foco explicitamente pedido ao SO: criar com `active` nem sempre basta
    // quando o app está na bandeja (o Windows tem trava de primeiro plano).
    if popup.take_focus_request() {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Focus);
    }
}

/// Corpo do popup: campo de busca, lista slim e teclado.
fn draw(ui: &mut egui::Ui, lang: Lang, state: &Arc<Mutex<State>>) {
    let ctx = ui.ctx().clone();
    let clock = Clock::now();
    let focused = ctx.input(|i| i.viewport().focused);

    let Ok(mut st) = state.lock() else { return };
    if !st.open {
        return;
    }

    // Foco do viewport: perder o foco = clicou fora = fecha. O `had_focus` cobre
    // o primeiro frame (a janela nasce sem foco e não pode se fechar sozinha) e
    // `None` (plataforma que não informa) nunca fecha.
    match focused {
        Some(true) => st.had_focus = true,
        Some(false) if st.had_focus => {
            st.close_with(&ctx, None);
            return;
        }
        _ => {}
    }

    // Esc fecha. Consumido antes dos campos pra que nada abaixo o reaproveite.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        st.close_with(&ctx, None);
        return;
    }

    let hits = matching(&st.rows, &st.search);
    // Foco padrão no primeiro que casa: Enter sem setas cola o mais recente.
    st.focus = match st.focus {
        Some(i) if i < hits.len() => Some(i),
        _ if hits.is_empty() => None,
        _ => Some(0),
    };

    // ↑/↓ navegam MESMO com o campo de busca focado (é um popup de teclado: o
    // usuário digita e navega sem tirar o foco do texto). Por isso as teclas são
    // consumidas antes de o `TextEdit` ver o frame.
    if !hits.is_empty() {
        let last = hits.len().saturating_sub(1);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            st.focus = Some(st.focus.unwrap_or(0).saturating_add(1).min(last));
            st.scroll_to = st.focus;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            st.focus = Some(st.focus.unwrap_or(0).saturating_sub(1));
            st.scroll_to = st.focus;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            if let Some(row) = st.focus.and_then(|i| hits.get(i)).and_then(|i| st.rows.get(*i)) {
                let id = row.id;
                st.close_with(&ctx, Some(Cmd::Paste(id)));
                return;
            }
        }
    }

    let skin = Skin::of(ui);
    let frame = egui::Frame::NONE.fill(skin.card).inner_margin(egui::Margin::same(14));
    let mut pick: Option<u128> = None;
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        // Campo de busca auto-focado na abertura + fechar com o mouse (×):
        // só Esc/clique-fora (foco) era pouco (feedback do João). Só pede foco
        // uma vez: pedir todo frame roubaria o foco de volta do clique.
        let mut want_close = false;
        ui.horizontal(|ui| {
            // O botão de fechar reserva a largura ANTES do campo: com
            // `desired_width(INFINITY)` o campo comia a linha inteira e o botão
            // saía pela borda direita.
            let field = ui.add(
                kit::line(&mut st.search).hint_text(i18n::t(lang, Key::Search)).desired_width((ui.available_width() - CLOSE_BTN - 8.0).max(120.0)),
            );
            if !st.focus_requested {
                field.request_focus();
                st.focus_requested = true;
            }
            if close_button(ui, skin).on_hover_text(i18n::t(lang, Key::Close)).clicked() {
                want_close = true;
            }
        });
        if want_close {
            st.close_with(&ctx, None);
            return;
        }
        ui.add_space(8.0);

        if hits.is_empty() {
            ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                ui.label(egui::RichText::new(i18n::t(lang, Key::PopupNoResults)).weak());
            });
            return;
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Respiro entre as linhas (feedback: texto muito colado no outro).
            ui.spacing_mut().item_spacing.y = 3.0;
            let mut wanted = st.scroll_to.take();
            for (pos, index) in hits.iter().enumerate() {
                let Some(row) = st.rows.get(*index) else { continue };
                let focused = st.focus == Some(pos);
                let resp = row_ui(ui, lang, &clock, row, focused);
                // Só rola quando o foco veio das SETAS (ver `State::scroll_to`):
                // a cada frame brigava com a roda do usuário.
                if focused && wanted == Some(pos) {
                    ui.scroll_to_rect(resp.rect, None);
                    wanted = None;
                }
                if resp.clicked() {
                    pick = Some(row.id);
                }
            }
        });
    });

    if let Some(id) = pick {
        st.close_with(&ctx, Some(Cmd::Paste(id)));
    }
}

/// Botão de fechar do popup: "×" (U+00D7, presente nas fontes do app) com
/// tinta do tema e área de clique de 30px. O "✕" (U+2715) do v0.2 não existe
/// no Ubuntu/Hack e saía VAZIO.
fn close_button(ui: &mut egui::Ui, skin: Skin) -> egui::Response {
    let font = egui::FontId::proportional(18.0);
    let galley = ui.painter().layout_no_wrap("×".to_owned(), font, skin.text);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CLOSE_BTN, CLOSE_BTN), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, skin.text);
    resp
}

/// Uma linha da lista slim: tipo + carimbo em cima, preview embaixo. Sem pin e
/// sem ações — o popup só serve pra escolher e colar.
fn row_ui(ui: &mut egui::Ui, lang: Lang, clock: &Clock, row: &Row, focused: bool) -> egui::Response {
    let skin = Skin::of(ui);
    let width = ui.available_width().max(1.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    let bg = if focused {
        skin.accent.gamma_multiply(0.18)
    } else if resp.hovered() {
        skin.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), bg);
    if focused {
        ui.painter().rect_stroke(rect, egui::CornerRadius::same(6), egui::Stroke::new(1.0_f32, skin.accent), egui::StrokeKind::Inside);
    }
    if !ui.is_rect_visible(rect) {
        return resp;
    }

    let inner = rect.shrink2(egui::vec2(10.0, 6.0));
    let mut line = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::LEFT)));
    line.spacing_mut().item_spacing.y = 2.0;
    line.horizontal(|ui| {
        ui.label(egui::RichText::new(clipboard_ui::kind_label(lang, row.kind)).small().color(skin.muted));
        ui.label(egui::RichText::new("·").small().color(skin.muted));
        ui.label(egui::RichText::new(clock.stamp(lang, row.ts_ms)).small().color(skin.muted));
    });
    line.add(egui::Label::new(egui::RichText::new(&row.preview).color(skin.text)).truncate());
    resp
}

/// Canto do popup: junto ao cursor, preso na área útil do monitor (taskbar
/// descontada). Perto da borda direita/inferior o popup abre pra
/// esquerda/cima — o clamp é o que garante isso.
fn anchored(ctx: &egui::Context) -> egui::Pos2 {
    let (avail, origin) = workarea::work_area(ctx);
    let cursor = cursor_pos(ctx).unwrap_or_else(|| origin + avail / 2.0);
    let max_x = (origin.x + avail.x - POPUP_W).max(origin.x);
    let max_y = (origin.y + avail.y - POPUP_H).max(origin.y);
    let x = (cursor.x + CURSOR_GAP).clamp(origin.x, max_x);
    let y = (cursor.y + CURSOR_GAP).clamp(origin.y, max_y);
    egui::pos2(x, y)
}

/// Posição do cursor em points. No Windows vem do SO (`winctl`; o ponteiro
/// pode estar fora da janela do popup, onde o egui não tem evento nenhum); nas
/// demais plataformas o popup nem abre, e o ponteiro do egui serve de fallback.
#[cfg(windows)]
fn cursor_pos(ctx: &egui::Context) -> Option<egui::Pos2> {
    crate::winctl::cursor_pos(ctx.pixels_per_point())
}

#[cfg(not(windows))]
fn cursor_pos(ctx: &egui::Context) -> Option<egui::Pos2> {
    ctx.input(|i| i.pointer.latest_pos())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: Kind, text: Option<&str>, files: Option<&[&str]>, ts: u64) -> Entry {
        Entry {
            id: crate::history::new_id(),
            ts_ms: ts,
            kind,
            pinned: false,
            text: text.map(str::to_string),
            files: files.map(|f| f.iter().map(|s| (*s).to_string()).collect()),
            image_png: None,
            thumb_png: None,
        }
    }

    fn retrato(entries: &[Entry]) -> Vec<Row> {
        entries.iter().map(row_of).collect()
    }

    /// O popup filtra pelo MESMO critério da aba Clipboard (texto e nome de
    /// arquivo, sem caixa) e corta em [`MAX_ROWS`].
    #[test]
    fn filtro_do_popup_usa_o_criterio_da_aba() {
        let rows = retrato(&[
            entry(Kind::Text, Some("Relatório do mês"), None, 5),
            entry(Kind::Files, None, Some(&["C:\\Users\\ana\\Notas.txt"]), 4),
            entry(Kind::Image, None, None, 3),
            entry(Kind::Text, Some("nada a ver"), None, 2),
        ]);

        // Busca vazia: tudo, na ordem do histórico.
        assert_eq!(matching(&rows, ""), vec![0, 1, 2, 3]);
        // Texto, sem caixa e sem espaço nas pontas.
        assert_eq!(matching(&rows, "  RELATÓRIO "), vec![0]);
        // Nome do arquivo (a pasta não conta).
        assert_eq!(matching(&rows, "notas"), vec![1]);
        assert!(matching(&rows, "users").is_empty(), "a pasta não entra na busca");
        assert!(matching(&rows, "inexistente").is_empty());
    }

    /// O popup nunca lista mais que [`MAX_ROWS`] — e sempre os mais recentes.
    #[test]
    fn filtro_do_popup_corta_nas_mais_recentes() {
        let entries: Vec<Entry> = (0..(MAX_ROWS + 5)).map(|i| entry(Kind::Text, Some(&format!("item {i}")), None, i as u64)).collect();
        let rows = retrato(&entries);
        let hits = matching(&rows, "item");
        assert_eq!(hits.len(), MAX_ROWS);
        assert_eq!(hits.first().copied(), Some(0), "a primeira é a mais recente");
    }
}
