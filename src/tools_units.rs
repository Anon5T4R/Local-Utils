//! Conversor de unidades da aba Ferramentas: comprimento, massa, temperatura,
//! dados e velocidade.
//!
//! Cada unidade tem um FATOR pro padrão da categoria (m, g, byte, m/s) — a
//! conversão é `valor * fator_origem / fator_destino`. Temperatura é a exceção
//! (não é escala, é deslocamento): as três saem de/para Celsius por fórmula.
//!
//! Puro: nenhuma unidade depende do SO nem de locale. Os rótulos das unidades
//! são SÍMBOLOS (`km`, `GiB`, `°C`) — iguais nos três idiomas de propósito, então
//! não passam pelo catálogo i18n; só os nomes das CATEGORIAS passam.

use utils_ui::i18n::{self, Key, Lang};

use crate::clipboard_ui::copy_new_text;
use crate::history::History;
use crate::ui::{self as kit, Skin};

/// Largura da coluna (o conversor é um card estreito e centralizado).
const CONTENT_W: f32 = 560.0;

/// Uma unidade: identificador estável (usado pelo `convert`) e quanto ela vale
/// no padrão da categoria.
#[derive(Clone, Copy)]
pub struct Unit {
    pub id: &'static str,
    /// Quanto 1 unidade vale na unidade-base da categoria (1.0 em temperatura:
    /// lá a conta é por fórmula, não por fator).
    pub factor: f64,
}

/// Categoria de unidades (as pills do topo).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Length,
    Mass,
    Temperature,
    Data,
    Speed,
}

impl Category {
    /// Todas as categorias, na ordem da barra.
    pub const ALL: [Category; 5] = [Category::Length, Category::Mass, Category::Temperature, Category::Data, Category::Speed];

    fn label_key(self) -> Key {
        match self {
            Category::Length => Key::UnitsLength,
            Category::Mass => Key::UnitsMass,
            Category::Temperature => Key::UnitsTemperature,
            Category::Data => Key::UnitsData,
            Category::Speed => Key::UnitsSpeed,
        }
    }

    /// Unidades da categoria, na ordem em que aparecem nos combos.
    fn units(self) -> &'static [Unit] {
        match self {
            Category::Length => &LENGTH,
            Category::Mass => &MASS,
            Category::Temperature => &TEMPERATURE,
            Category::Data => &DATA,
            Category::Speed => &SPEED,
        }
    }
}

/// Comprimento — base: metro.
static LENGTH: [Unit; 8] = [
    Unit { id: "mm", factor: 0.001 },
    Unit { id: "cm", factor: 0.01 },
    Unit { id: "m", factor: 1.0 },
    Unit { id: "km", factor: 1_000.0 },
    Unit { id: "in", factor: 0.0254 },
    Unit { id: "ft", factor: 0.3048 },
    Unit { id: "yd", factor: 0.9144 },
    Unit { id: "mi", factor: 1_609.344 },
];

/// Massa — base: grama.
static MASS: [Unit; 6] = [
    Unit { id: "mg", factor: 0.001 },
    Unit { id: "g", factor: 1.0 },
    Unit { id: "kg", factor: 1_000.0 },
    Unit { id: "t", factor: 1_000_000.0 },
    Unit { id: "oz", factor: 28.349_523_125 },
    Unit { id: "lb", factor: 453.592_37 },
];

/// Dados — base: byte. Decimal (KB/MB/GB) e binário (KiB/MiB/GiB) convivem:
/// são unidades DIFERENTES, não duas formas de escrever a mesma.
static DATA: [Unit; 7] = [
    Unit { id: "B", factor: 1.0 },
    Unit { id: "KB", factor: 1_000.0 },
    Unit { id: "MB", factor: 1_000_000.0 },
    Unit { id: "GB", factor: 1_000_000_000.0 },
    Unit { id: "KiB", factor: 1_024.0 },
    Unit { id: "MiB", factor: 1_048_576.0 },
    Unit { id: "GiB", factor: 1_073_741_824.0 },
];

/// Velocidade — base: metro por segundo.
static SPEED: [Unit; 4] = [
    Unit { id: "m/s", factor: 1.0 },
    Unit { id: "km/h", factor: 1.0 / 3.6 },
    Unit { id: "mph", factor: 0.447_04 },
    Unit { id: "kn", factor: 1_852.0 / 3_600.0 },
];

/// Temperatura — o fator não vale aqui; a conta é por fórmula (base: Celsius).
static TEMPERATURE: [Unit; 3] = [Unit { id: "°C", factor: 1.0 }, Unit { id: "°F", factor: 1.0 }, Unit { id: "K", factor: 1.0 }];

/// Acha a unidade pelo id dentro da categoria.
fn find(cat: Category, id: &str) -> Option<Unit> {
    cat.units().iter().copied().find(|unit| unit.id == id)
}

/// `valor` na unidade `from` convertido pra `to`. `None` = id desconhecido.
pub fn convert(cat: Category, value: f64, from: &str, to: &str) -> Option<f64> {
    if cat == Category::Temperature {
        let celsius = to_celsius(value, from)?;
        return from_celsius(celsius, to);
    }
    let from = find(cat, from)?;
    let to = find(cat, to)?;
    Some(value * from.factor / to.factor)
}

/// Qualquer escala de temperatura → Celsius.
fn to_celsius(value: f64, unit: &str) -> Option<f64> {
    match unit {
        "°C" => Some(value),
        "°F" => Some((value - 32.0) * 5.0 / 9.0),
        "K" => Some(value - 273.15),
        _ => None,
    }
}

/// Celsius → qualquer escala de temperatura.
fn from_celsius(celsius: f64, unit: &str) -> Option<f64> {
    match unit {
        "°C" => Some(celsius),
        "°F" => Some(celsius * 9.0 / 5.0 + 32.0),
        "K" => Some(celsius + 273.15),
        _ => None,
    }
}

/// Número legível: sem zeros à toa, notação científica pros extremos (converter
/// 1 byte pra GiB cairia num "0" enganoso).
pub fn fmt_value(value: f64) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.abs() >= 1e15 || value.abs() < 1e-6 {
        return format!("{value:.6e}");
    }
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Estado do conversor entre frames.
pub struct UnitsState {
    cat: Category,
    /// O valor digitado (texto: o usuário pode estar no meio de "1.5").
    value: String,
    /// Índices nas unidades da categoria (mudar de categoria reseta os dois).
    from: usize,
    to: usize,
}

impl Default for UnitsState {
    fn default() -> Self {
        Self { cat: Category::Length, value: "1".to_string(), from: 2, to: 3 }
    }
}

impl UnitsState {
    /// Unidade de origem escolhida (id).
    fn origin_id(&self) -> Option<&'static str> {
        self.cat.units().get(self.from).map(|unit| unit.id)
    }

    /// Unidade de destino escolhida (id).
    fn target_id(&self) -> Option<&'static str> {
        self.cat.units().get(self.to).map(|unit| unit.id)
    }

    /// Resultado pronto pra tela: valor formatado ou a mensagem de erro.
    fn result(&self) -> Result<String, Key> {
        let value: f64 = self.value.trim().replace(',', ".").parse().map_err(|_| Key::UnitsInvalid)?;
        let (Some(from), Some(to)) = (self.origin_id(), self.target_id()) else { return Err(Key::UnitsInvalid) };
        let converted = convert(self.cat, value, from, to).ok_or(Key::UnitsInvalid)?;
        Ok(fmt_value(converted))
    }
}

/// O que o conversor devolve pro app.
#[derive(Default)]
pub struct Outcome {
    pub history_changed: bool,
    pub status: Option<String>,
}

/// Um lado da conversão: rótulo pequeno ("De"/"Para") e o combo das unidades
/// da categoria corrente.
fn unit_combo(ui: &mut egui::Ui, id: &str, label: &str, units: &'static [Unit], index: &mut usize, width: f32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).small().color(Skin::of(ui).muted));
        egui::ComboBox::from_id_salt(id).selected_text(units.get(*index).map_or("", |unit| unit.id)).width(width).show_ui(ui, |ui| {
            for (i, unit) in units.iter().enumerate() {
                ui.selectable_value(index, i, unit.id);
            }
        });
    });
}

/// Desenha o conversor. `texts` é o histórico de clipboard (o "copiar" cria uma
/// entrada nova de texto, como no recopiar da aba Clipboard).
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut UnitsState, texts: &mut History) -> Outcome {
    let mut out = Outcome::default();

    kit::column(ui, CONTENT_W, |ui| {
        // Categoria: segmented control (nível 2), abaixo das pills da aba.
        ui.horizontal(|ui| {
            for cat in Category::ALL {
                if kit::segmented(ui, i18n::t(lang, cat.label_key()), state.cat == cat).clicked() && state.cat != cat {
                    state.cat = cat;
                    // Os índices antigos podem não existir na categoria nova.
                    state.from = 0;
                    state.to = state.cat.units().len().saturating_sub(1);
                }
            }
        });
        ui.add_space(12.0);

        let width = ui.available_width();
        kit::card(ui, width, |ui| {
            let skin = Skin::of(ui);

            // ── valor digitado, grande (mono) ────────────────────────
            ui.label(egui::RichText::new(i18n::t(lang, Key::UnitsValue)).small().color(skin.muted));
            let field_w = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut state.value)
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::monospace(30.0))
                    .text_color(skin.text)
                    .margin(egui::Margin::symmetric(0, 4))
                    .desired_width(field_w),
            );
            ui.add_space(4.0);
            kit::hairline(ui);
            ui.add_space(12.0);

            // ── De / ⇄ / Para ────────────────────────────────────────
            let units = state.cat.units();
            let combo_w = ((ui.available_width() - 100.0) / 2.0).max(120.0);
            ui.horizontal(|ui| {
                unit_combo(ui, "units-from", i18n::t(lang, Key::UnitsFrom), units, &mut state.from, combo_w);
                ui.add_space(8.0);
                // Troca origem e destino de uma vez (o valor digitado fica).
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    if kit::ghost(ui, "⇄").on_hover_text(i18n::t(lang, Key::UnitsSwap)).clicked() {
                        std::mem::swap(&mut state.from, &mut state.to);
                    }
                });
                ui.add_space(8.0);
                unit_combo(ui, "units-to", i18n::t(lang, Key::UnitsTo), units, &mut state.to, combo_w);
            });
            ui.add_space(14.0);

            // ── resultado herói (accent, copiável) ───────────────────
            match state.result() {
                Ok(text) => {
                    ui.horizontal(|ui| {
                        let label = egui::Label::new(egui::RichText::new(&text).font(egui::FontId::monospace(30.0)).strong().color(skin.accent))
                            .sense(egui::Sense::click());
                        let resp = ui.add(label).on_hover_text(i18n::t(lang, Key::CopyValue));
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if let Some(unit) = units.get(state.to) {
                            ui.label(egui::RichText::new(unit.id).small().color(skin.muted));
                        }
                        if resp.clicked() {
                            let (changed, status) = copy_new_text(texts, lang, &text);
                            out.history_changed |= changed;
                            out.status = Some(status);
                        }
                    });
                    // Linha de referência ("1 ft = 30.48 cm"): o par de unidades
                    // em corpo pequeno, para conferir a escala sem converter.
                    if let (Some(from), Some(to)) = (state.origin_id(), state.target_id()) {
                        if let Some(unit_value) = convert(state.cat, 1.0, from, to) {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(format!("1 {from} = {} {to}", fmt_value(unit_value))).small().color(skin.muted));
                        }
                    }
                }
                Err(key) => {
                    ui.label(egui::RichText::new(format!("⚠ {}", i18n::t(lang, key))).color(skin.danger));
                }
            }
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perto(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Converte e confere com tolerância (o inverso de uma divisão nunca é
    /// exato em f64).
    fn check(cat: Category, value: f64, from: &str, to: &str, want: f64) {
        match convert(cat, value, from, to) {
            Some(got) if perto(got, want) => {}
            other => panic!("{value} {from}→{to}: esperava {want}, veio {other:?}"),
        }
    }

    #[test]
    fn temperatura_nas_duas_direcoes() {
        check(Category::Temperature, 100.0, "°C", "°F", 212.0);
        check(Category::Temperature, 32.0, "°F", "°C", 0.0);
        check(Category::Temperature, 0.0, "°C", "K", 273.15);
        check(Category::Temperature, 300.0, "K", "°C", 26.85);
        check(Category::Temperature, 98.6, "°F", "K", 310.15);
        check(Category::Temperature, -40.0, "°C", "°F", -40.0);
        // Ida e volta fecha no valor original.
        check(Category::Temperature, 37.0, "°C", "°C", 37.0);
    }

    #[test]
    fn dados_decimal_e_binario_sao_unidades_diferentes() {
        check(Category::Data, 1.0, "MB", "KB", 1_000.0);
        check(Category::Data, 1.0, "GB", "MB", 1_000.0);
        check(Category::Data, 1.0, "GiB", "MB", 1_073.741_824);
        check(Category::Data, 1.0, "GiB", "KiB", 1_048_576.0);
        check(Category::Data, 1.0, "MiB", "B", 1_048_576.0);
        check(Category::Data, 1.0, "KiB", "KB", 1.024);
    }

    #[test]
    fn comprimento_massa_e_velocidade() {
        check(Category::Length, 1.0, "km", "m", 1_000.0);
        check(Category::Length, 1.0, "mi", "m", 1_609.344);
        check(Category::Length, 12.0, "in", "cm", 30.48);
        check(Category::Mass, 1.0, "kg", "g", 1_000.0);
        check(Category::Mass, 1.0, "lb", "g", 453.592_37);
        check(Category::Speed, 1.0, "km/h", "m/s", 0.277_777_777);
        check(Category::Speed, 1.0, "kn", "km/h", 1.852);
    }

    #[test]
    fn unidade_desconhecida_nao_converte() {
        assert_eq!(convert(Category::Length, 1.0, "km", "banana"), None);
        assert_eq!(convert(Category::Length, 1.0, "banana", "m"), None);
        // Unidade de OUTRA categoria também não converte.
        assert_eq!(convert(Category::Length, 1.0, "km", "kg"), None);
        assert_eq!(convert(Category::Temperature, 1.0, "°C", "°R"), None);
    }

    #[test]
    fn formatacao_do_valor() {
        assert_eq!(fmt_value(1_000.0), "1000");
        assert_eq!(fmt_value(0.5), "0.5");
        assert_eq!(fmt_value(1.5), "1.5");
        assert_eq!(fmt_value(-40.0), "-40");
        assert_eq!(fmt_value(0.0), "0");
        // 1 byte em GiB: decimal não serve (viraria "0").
        assert!(fmt_value(1.0 / 1_073_741_824.0).contains('e'), "valor minúsculo sai em científica");
    }
}
