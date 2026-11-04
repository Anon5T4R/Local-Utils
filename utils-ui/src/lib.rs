//! utils-ui — infra de UI comum do Local Utils: tema (Light/Dark/System),
//! i18n PT/EN/ES e config JSON por app. Base derivada do kit de UI da suíte.

pub mod config;
pub mod i18n;
pub mod theme;
pub mod workarea;

use config::Config;
use i18n::{Key, Lang};
use theme::Appearance;

/// Um par "rótulo + controle" com o rótulo SEMPRE à esquerda no render final.
///
/// Em layout `left_to_right` isso é só a ordem natural de adição. Já num
/// cluster `right_to_left` (a barra de topo do app), a ordem de adição é
/// ESPELHADA — adicionar rótulo antes do combo desenhava "System ▾ Theme".
/// Por isso o par inverte a ordem de adição junto com o layout, em vez de
/// deixar o espelho brigar com o rótulo.
fn pair(ui: &mut egui::Ui, label: &str, rtl: bool, control: impl FnOnce(&mut egui::Ui)) {
    if rtl {
        control(ui);
        ui.label(label);
    } else {
        ui.label(label);
        control(ui);
    }
}

/// Desenha os combos de idioma + tema (canto de configurações comum aos apps).
/// Devolve `(config_changed, forget_quick_key_clicked)`.
pub fn settings_ui(ui: &mut egui::Ui, cfg: &mut Config, show_forget_quick_key: bool) -> (bool, bool) {
    // O config guarda lista/strings: comparar por clone (barato: só no frame
    // em que a barra de topo é desenhada).
    let before = cfg.clone();
    let mut forget_clicked = false;
    // Direção do layout de QUEM chama: o `horizontal_wrapped` abaixo herda
    // dela, e é ela que decide se a ordem de adição sai espelhada.
    let rtl = ui.layout().prefer_right_to_left();

    ui.horizontal_wrapped(|ui| {
        // Rótulo na mão + combo sem label: o `ComboBox::from_label` do egui
        // desenha o rótulo DEPOIS do controle ("English ▾ Language").
        pair(ui, &format!("{}:", i18n::t(cfg.lang, Key::Language)), rtl, |ui| {
            egui::ComboBox::from_id_salt("utils-language").selected_text(cfg.lang.label()).width(80.0).show_ui(ui, |ui| {
                for l in Lang::ALL {
                    ui.selectable_value(&mut cfg.lang, l, l.label());
                }
            });
        });

        pair(ui, &format!("{}:", i18n::t(cfg.lang, Key::Theme)), rtl, |ui| {
            // O valor mostrado também é traduzido (Sistema/Claro/Escuro).
            let selected = cfg.theme.label_i18n(cfg.lang);
            egui::ComboBox::from_id_salt("utils-theme").selected_text(selected).width(120.0).show_ui(ui, |ui| {
                for th in Appearance::ALL {
                    ui.selectable_value(&mut cfg.theme, th, th.label_i18n(cfg.lang));
                }
            });
        });

        if show_forget_quick_key && ui.button(i18n::t(cfg.lang, Key::ForgetKey)).clicked() {
            forget_clicked = true;
        }
    });

    (*cfg != before, forget_clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Todo texto desenhado no frame, com o retângulo da TINTA de cada um
    /// (`visual_bounding_rect` — o rect do galley inclui a largura justificada
    /// e, no RTL, o offset do `halign`).
    fn textos(out: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        out.shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), cs.shape.visual_bounding_rect())),
                _ => None,
            })
            .collect()
    }

    fn rect_of(textos: &[(String, egui::Rect)], needle: &str) -> egui::Rect {
        textos.iter().find(|(t, _)| t == needle).unwrap_or_else(|| panic!("texto {needle:?} não desenhado")).1
    }

    /// Frame de teste com tela de verdade: sem `screen_rect` o egui nasce sem
    /// largura e o texto truncado não aparece nos shapes.
    fn input() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))), ..Default::default() }
    }

    /// O par "label + combo" sai sempre com o RÓTULO à esquerda — inclusive no
    /// cluster `right_to_left` da barra de topo, onde a ordem de adição é
    /// espelhada. Era o bug do build: "System ▾ Theme" (o combo ficava ANTES
    /// do rótulo).
    #[test]
    fn rotulo_fica_a_esquerda_do_combo_nos_dois_layouts() {
        for rtl in [false, true] {
            let ctx = egui::Context::default();
            let mut cfg = Config::default();
            let out = ctx.run_ui(input(), |ui| {
                let layout = if rtl { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
                ui.with_layout(layout, |ui| {
                    settings_ui(ui, &mut cfg, false);
                });
            });

            let textos = textos(&out);
            let idioma = rect_of(&textos, &format!("{}:", i18n::t(cfg.lang, Key::Language)));
            let combo_idioma = rect_of(&textos, cfg.lang.label());
            assert!(idioma.right() <= combo_idioma.left() + 0.5, "rtl={rtl}: o rótulo {idioma:?} tem que ficar à ESQUERDA do combo {combo_idioma:?}");
            let tema = rect_of(&textos, &format!("{}:", i18n::t(cfg.lang, Key::Theme)));
            let combo_tema = rect_of(&textos, cfg.theme.label_i18n(cfg.lang));
            assert!(tema.right() <= combo_tema.left() + 0.5, "rtl={rtl}: o rótulo {tema:?} tem que ficar à ESQUERDA do combo {combo_tema:?}");
            out.drop_without_applying_deltas();
        }
    }

    /// O seletor de tema mostra o valor TRADUZIDO, não o código em inglês.
    #[test]
    fn combo_de_tema_mostra_o_valor_traduzido() {
        let ctx = egui::Context::default();
        let mut cfg = Config::default();
        let out = ctx.run_ui(input(), |ui| {
            settings_ui(ui, &mut cfg, false);
        });
        let textos = textos(&out);
        assert!(textos.iter().any(|(t, _)| t == "Sistema"), "o valor do tema deveria sair em pt-br: {textos:?}");
        assert!(!textos.iter().any(|(t, _)| t == "System theme"), "o código em inglês não aparece na tela");
        out.drop_without_applying_deltas();
    }
}
