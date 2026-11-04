//! Captura de tela: congela o desktop, deixa o usuário arrastar um retângulo e
//! entrega o recorte pro editor da aba Screenshot.
//!
//! Fluxo: o atalho de captura (`hotkey.rs`, `PressedShot`) chama
//! [`Capture::begin`], que tira um retrato de TODOS os monitores e compõe um
//! canvas do desktop virtual (`xcap`, Windows). O app então abre um viewport
//! DEFERRED sem decoração, sempre no topo e do tamanho do desktop virtual, que
//! mostra o retrato como textura — a "tela congelada". Arrastar define a
//! seleção e o mouse-up confirma (Enter confirma pelo teclado, Esc cancela). O
//! recorte sai por [`Capture::take_result`]; o viewport é destruído quando o app
//! deixa de chamar [`show`] (mesma mecânica do popup, ver `popup.rs`).
//!
//! Por que compor TODOS os monitores em vez de capturar só o do cursor: a
//! seleção pode atravessar a borda entre dois monitores, e a tela congelada tem
//! de mostrar o que está do outro lado. O canvas é montado pelas posições que o
//! `xcap` reporta (px físicos), então monitores lado a lado ficam contíguos sem
//! consultar as métricas do Win32 à parte.
//!
//! **Limitação de multi-DPI**: o viewport é posicionado/dimensionado em POINTS
//! (px ÷ `pixels_per_point` do app). Com monitores de DPI DIFERENTE o Windows
//! pode desenhar o overlay num tamanho que não casa exatamente com o canvas — o
//! alinhamento visual sai torto, mas o recorte NÃO sofre disso: o mapeamento da
//! seleção é por RAZÃO (largura da imagem ÷ largura desenhada), nunca por
//! `ppp`. Setup de DPI uniforme (inclusive um monitor só) fica exato.

use std::sync::{Arc, Mutex};

use eframe::egui;
use image::RgbaImage;
use utils_ui::i18n::{self, Key, Lang};

/// Título PRÓPRIO de propósito: `winctl::discover` acha a janela principal pelo
/// título exato ("Local Utils") — o overlay não pode se passar por ela.
const OVERLAY_TITLE: &str = "Local Utils — captura";
/// Menor seleção válida, em px da imagem: clique acidental não vira captura
/// vazia.
const MIN_SIDE: u32 = 8;

/// Id do viewport. Fixo: a mesma captura reusa a mesma janela.
fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("local-utils-capture")
}

/// O retrato congelado do desktop virtual + onde o viewport nasce.
struct Frozen {
    image: RgbaImage,
    /// Canto do desktop virtual em points (o `with_position` do viewport).
    pos_pt: egui::Pos2,
    /// Tamanho do desktop virtual em points.
    size_pt: egui::Vec2,
}

/// Estado compartilhado entre o app e o callback do viewport (o callback exige
/// `Fn + Send + Sync + 'static`, então vai num `Arc<Mutex<..>>`).
#[derive(Default)]
struct State {
    open: bool,
    frame: Option<Frozen>,
    /// Textura do retrato: sobe no primeiro frame do viewport.
    tex: Option<egui::TextureHandle>,
    /// Arraste em points, LOCAIS ao viewport (0,0 = canto do overlay).
    sel: Option<(egui::Pos2, egui::Pos2)>,
    /// Enter pediu confirmação (o mouse-up já confirma sozinho).
    confirm: bool,
    /// O botão estava pressionado no frame anterior: é o que distingue
    /// "mouse-up depois de arrastar" de "mouse parado".
    pressing: bool,
    /// O `ViewportCommand::Focus` já foi pedido nesta abertura (o SO pode
    /// recusar; insistir a cada frame viraria loop de repaint).
    focus_sent: bool,
    /// Recorte confirmado, esperando o app pegar.
    result: Option<RgbaImage>,
}

impl State {
    /// Fecha e solta o retrato. Um desktop 4K são dezenas de MB — segurar
    /// depois de confirmar/cancelar não tem propósito.
    fn close(&mut self, ctx: &egui::Context) {
        self.open = false;
        self.frame = None;
        self.tex = None;
        self.sel = None;
        self.confirm = false;
        self.pressing = false;
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Handle da captura (vive no app).
#[derive(Clone, Default)]
pub struct Capture {
    state: Arc<Mutex<State>>,
}

impl Capture {
    /// Congela a tela e abre o overlay. Erro = nada abriu (vira status no
    /// rodapé). Chamada repetida enquanto aberto é no-op.
    pub fn begin(&self, ctx: &egui::Context) -> Result<(), String> {
        if self.is_open() {
            return Ok(());
        }
        // A captura roda na thread da UI: são ~1 frame de trabalho e evita
        // corrida com o próprio overlay abrindo.
        let (image, origin) = imp::capture_all()?;
        let ppp = ctx.pixels_per_point().max(0.1);
        let Ok(mut st) = self.state.lock() else {
            return Err("estado da captura indisponível".into());
        };
        st.frame = Some(Frozen {
            pos_pt: egui::pos2(origin.0 as f32 / ppp, origin.1 as f32 / ppp),
            size_pt: egui::vec2(image.width() as f32 / ppp, image.height() as f32 / ppp),
            image,
        });
        st.tex = None;
        st.sel = None;
        st.confirm = false;
        st.pressing = false;
        st.focus_sent = false;
        st.result = None;
        st.open = true;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.state.lock().is_ok_and(|st| st.open)
    }

    /// Pede foco UMA vez por abertura: sem ele o Esc não chega ao overlay
    /// quando o SO recusa o primeiro plano (mesma razão do popup).
    fn take_focus_request(&self) -> bool {
        match self.state.lock() {
            Ok(mut st) if st.open && !st.focus_sent => {
                st.focus_sent = true;
                true
            }
            _ => false,
        }
    }

    /// Recorte confirmado (o app troca pra aba Screenshot com ele).
    pub fn take_result(&self) -> Option<RgbaImage> {
        self.state.lock().ok().and_then(|mut st| st.result.take())
    }
}

/// Desenha o overlay (só quando aberto). O app chama isto a cada frame; parar
/// de chamar derruba a janela.
pub fn show(ctx: &egui::Context, lang: Lang, hotkey: &str, cap: &Capture) {
    let (pos, size) = match cap.state.lock() {
        Ok(st) if st.open => match st.frame.as_ref() {
            Some(f) => (f.pos_pt, f.size_pt),
            None => return,
        },
        _ => return,
    };
    let state = Arc::clone(&cap.state);
    let hint = i18n::t(lang, Key::CaptureHint).replace("{hotkey}", &crate::options::pretty_hotkey(hotkey));
    let builder = egui::ViewportBuilder::default()
        .with_title(OVERLAY_TITLE)
        .with_position(pos)
        .with_inner_size(size)
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        // O desktop virtual pode ser MAIOR que um monitor: sem isto o winit
        // corta a janela no tamanho do monitor e sobra área sem overlay.
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
        draw(ui, &hint, &state);
    });
    // Foco explícito: criar com `active` nem sempre basta no Windows (trava de
    // primeiro plano) e sem foco o Esc não volta pra cá.
    if cap.take_focus_request() {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Focus);
    }
}

/// Corpo do overlay: retrato, crosshair, moldura da seleção e confirmação.
fn draw(ui: &mut egui::Ui, hint: &str, state: &Arc<Mutex<State>>) {
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

    // Textura do retrato: sobe uma vez por captura. O `ColorImage` sai num
    // passo à parte pra não segurar o empréstimo de `st.frame` junto com o de
    // `st.tex`.
    let fresh = match (st.frame.as_ref(), st.tex.is_some()) {
        (Some(f), false) => Some(egui::ColorImage::from_rgba_unmultiplied([f.image.width() as usize, f.image.height() as usize], f.image.as_raw())),
        _ => None,
    };
    if let Some(color) = fresh {
        st.tex = Some(ctx.load_texture("shot-frozen", color, egui::TextureOptions::NEAREST));
    }

    // Preenchidos dentro do painel (é lá que o retângulo útil existe) e lidos
    // na hora de confirmar.
    let mut full_rect = egui::Rect::NOTHING;
    let mut released = false;
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(egui::Color32::BLACK)).show(ui, |ui| {
        let full = ui.max_rect();
        full_rect = full;
        let painter = ui.painter_at(full);
        if let Some(tex) = st.tex.as_ref() {
            painter.image(tex.id(), full, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }

        let resp = ui.interact(full, ui.scope_id().with("shot-capture"), egui::Sense::click_and_drag());

        // Arraste: a seleção nasce no PRESS (não no "virou drag" do egui) e
        // fecha no mouse-up — com o limiar de drag do egui um arraste curto
        // nunca viraria `drag_started` e o usuário ficaria com uma moldura
        // morta na tela.
        if resp.is_pointer_button_down_on() {
            if let Some(p) = resp.interact_pointer_pos() {
                match st.sel.as_mut() {
                    Some(sel) => sel.1 = p,
                    None => st.sel = Some((p, p)),
                }
            }
            st.pressing = true;
        } else if st.pressing {
            st.pressing = false;
            released = true;
        }

        let sel_rect = st.sel.map(|(a, b)| egui::Rect::from_two_pos(a, b));
        let has_selection = sel_rect.is_some_and(|r| r.width() >= 1.0 && r.height() >= 1.0);
        let accent = egui::Color32::from_rgb(90, 170, 255);
        // Escala imagem ÷ desenho, só pra etiqueta de medidas sair em px de
        // IMAGEM (é o tamanho que o recorte vai ter).
        let scale = st
            .frame
            .as_ref()
            .map(|f| (f.image.width() as f32 / full.width().max(1.0), f.image.height() as f32 / full.height().max(1.0)))
            .unwrap_or((1.0, 1.0));
        if let Some(r) = sel_rect.filter(|_| has_selection) {
            // Quatro bandas: o miolo da seleção fica sem o véu.
            let bands = [
                egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, r.min.y)),
                egui::Rect::from_min_max(egui::pos2(full.min.x, r.max.y), full.max),
                egui::Rect::from_min_max(egui::pos2(full.min.x, r.min.y), egui::pos2(r.min.x, r.max.y)),
                egui::Rect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(full.max.x, r.max.y)),
            ];
            let veil = egui::Color32::from_black_alpha(120);
            for band in bands {
                painter.rect_filled(band, egui::CornerRadius::ZERO, veil);
            }
            painter.rect_stroke(r, egui::CornerRadius::ZERO, egui::Stroke::new(1.0_f32, accent), egui::StrokeKind::Inside);
            painter.text(
                r.min + egui::vec2(4.0, -18.0),
                egui::Align2::LEFT_TOP,
                measure(r, scale),
                egui::FontId::monospace(12.0),
                egui::Color32::WHITE,
            );
        } else {
            // Sem seleção a tela inteira leva um véu leve: deixa claro que o
            // retrato está congelado e inerte.
            painter.rect_filled(full, egui::CornerRadius::ZERO, egui::Color32::from_black_alpha(40));
        }

        // Crosshair no cursor enquanto não há arraste (orienta o canto).
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|_| !has_selection) {
            let stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(150));
            painter.line_segment([egui::pos2(full.min.x, p.y), egui::pos2(full.max.x, p.y)], stroke);
            painter.line_segment([egui::pos2(p.x, full.min.y), egui::pos2(p.x, full.max.y)], stroke);
        }

        // Dica do atalho + Esc, no alto.
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

    // Confirmação: mouse-up (fluxo normal) ou Enter.
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    if enter {
        st.confirm = true;
    }
    if released || st.confirm {
        st.confirm = false;
        let cropped = match (st.frame.as_ref(), st.sel) {
            (Some(f), Some(sel)) => crop_selection(full_rect, &f.image, sel),
            _ => None,
        };
        match cropped {
            Some(img) => {
                st.result = Some(img);
                st.close(&ctx);
            }
            // Clique/drag curto demais: descarta e deixa o overlay aberto
            // (Esc sai).
            None => st.sel = None,
        }
    }
}

/// Etiqueta de medidas da seleção, em px DA IMAGEM (é o tamanho que o recorte
/// vai ter). `scale` = imagem ÷ área desenhada em cada eixo.
fn measure(r: egui::Rect, (sx, sy): (f32, f32)) -> String {
    format!("{:.0} × {:.0}", r.width() * sx, r.height() * sy)
}

/// Recorta a seleção (points locais ao viewport) da imagem congelada. `None`
/// quando o retângulo é menor que [`MIN_SIDE`] px em qualquer eixo.
fn crop_selection(full: egui::Rect, img: &RgbaImage, (a, b): (egui::Pos2, egui::Pos2)) -> Option<RgbaImage> {
    let (w, h) = img.dimensions();
    if full.width() <= 0.0 || full.height() <= 0.0 || w == 0 || h == 0 {
        return None;
    }
    // Razão imagem ÷ viewport (não `ppp`): a seleção é em points do que está
    // DESENHADO, então funciona igual em qualquer escala.
    let sx = w as f32 / full.width();
    let sy = h as f32 / full.height();
    let r = egui::Rect::from_two_pos(a, b).intersect(full);
    let x0 = (((r.min.x - full.min.x) * sx).floor().max(0.0) as u32).min(w);
    let y0 = (((r.min.y - full.min.y) * sy).floor().max(0.0) as u32).min(h);
    let x1 = (((r.max.x - full.min.x) * sx).round().max(0.0) as u32).min(w);
    let y1 = (((r.max.y - full.min.y) * sy).round().max(0.0) as u32).min(h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    if cw < MIN_SIDE || ch < MIN_SIDE {
        return None;
    }
    Some(image::imageops::crop_imm(img, x0, y0, cw, ch).to_image())
}

#[cfg(windows)]
mod imp {
    //! Retrato do desktop virtual pelo `xcap` (WGC no Windows).

    use image::RgbaImage;

    /// Teto de memória da composição (4 bytes/px). Acima disso a captura é
    /// recusada com um erro claro em vez de tentar alocar: dois 4K ficam em
    /// ~66 MB, três em ~100 MB.
    const MAX_PIXELS: u64 = 120_000_000;

    pub fn capture_all() -> Result<(RgbaImage, (i32, i32)), String> {
        let monitors = xcap::Monitor::all().map_err(|e| format!("listar monitores: {e}"))?;
        if monitors.is_empty() {
            return Err("nenhum monitor disponível".into());
        }

        // Limites do desktop virtual e a pegada de memória, antes de alocar.
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let mut budget = 0_u64;
        for m in &monitors {
            let (x, y, w, h) = geom(m)?;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x.saturating_add(w as i32));
            max_y = max_y.max(y.saturating_add(h as i32));
            budget = budget.saturating_add(u64::from(w).saturating_mul(u64::from(h)));
        }
        if budget > MAX_PIXELS {
            return Err(format!("área de captura grande demais ({budget} px)"));
        }
        let (cw, ch) = (max_x.saturating_sub(min_x), max_y.saturating_sub(min_y));
        if cw <= 0 || ch <= 0 {
            return Err("desktop virtual vazio".into());
        }

        let mut canvas = RgbaImage::new(cw as u32, ch as u32);
        for m in &monitors {
            let (x, y, _, _) = geom(m)?;
            let shot = m.capture_image().map_err(|e| format!("capturar o monitor: {e}"))?;
            blit(&mut canvas, &shot, x - min_x, y - min_y);
        }
        Ok((canvas, (min_x, min_y)))
    }

    /// Posição e tamanho do monitor em px físicos (`dmPosition`/`dmPels*`).
    fn geom(m: &xcap::Monitor) -> Result<(i32, i32, u32, u32), String> {
        let x = m.x().map_err(|e| format!("posição do monitor: {e}"))?;
        let y = m.y().map_err(|e| format!("posição do monitor: {e}"))?;
        let w = m.width().map_err(|e| format!("tamanho do monitor: {e}"))?;
        let h = m.height().map_err(|e| format!("tamanho do monitor: {e}"))?;
        Ok((x, y, w, h))
    }

    /// Copia `src` para dentro de `dst` em (dx, dy). O `imageops::replace` já
    /// corta o que passa da borda (é o motivo de a composição inteira ser
    /// feita com ele: monitores encostados e coordenadas negativas acontecem).
    fn blit(dst: &mut RgbaImage, src: &RgbaImage, dx: i32, dy: i32) {
        image::imageops::replace(dst, src, i64::from(dx), i64::from(dy));
    }
}

#[cfg(not(windows))]
mod imp {
    //! Stub: a composição do desktop usa o `xcap` só no Windows neste lote.

    use image::RgbaImage;

    pub fn capture_all() -> Result<(RgbaImage, (i32, i32)), String> {
        Err("captura de tela: só Windows por enquanto".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O mapeamento seleção → imagem é por RAZÃO: metade do tamanho desenhado
    /// devolve o dobro em px de imagem.
    #[test]
    fn recorte_mapeia_por_razao() {
        let img = RgbaImage::from_pixel(200, 100, image::Rgba([0, 0, 0, 255]));
        let full = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 50.0));
        let got = crop_selection(full, &img, (egui::pos2(10.0, 10.0), egui::pos2(60.0, 40.0))).expect("recorte válido");
        assert_eq!(got.dimensions(), (100, 60));
    }

    /// Clique/drag curto demais não vira captura (dimensão mínima de 8 px).
    #[test]
    fn recorte_recusa_selecao_minuscula_e_vazia() {
        let img = RgbaImage::from_pixel(200, 100, image::Rgba([0, 0, 0, 255]));
        let full = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 50.0));
        // 2 points de lado = 4 px na imagem: abaixo do mínimo.
        assert!(crop_selection(full, &img, (egui::pos2(0.0, 0.0), egui::pos2(2.0, 2.0))).is_none());
        // Retângulo invertido (arrastou pra cima/esquerda) continua válido.
        assert!(crop_selection(full, &img, (egui::pos2(60.0, 40.0), egui::pos2(10.0, 10.0))).is_some());
        // Seleção fora do viewport (arraste pra fora) é cortada, não estoura.
        let clipped = crop_selection(full, &img, (egui::pos2(-50.0, -50.0), egui::pos2(500.0, 500.0))).expect("recorte cortado");
        assert_eq!(clipped.dimensions(), (200, 100));
    }
}
