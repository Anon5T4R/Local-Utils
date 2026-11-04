//! Aba Ferramentas: as sub-abas na mesma linha de pills dos filtros da aba
//! Clipboard (Relógio · Calculadora · Unidades · Dev) e o despacho do desenho.
//!
//! O estado dos quatro painéis mora aqui e sobrevive à troca de sub-aba; o
//! relógio é o único que também roda com a aba FECHADA — por isso o
//! [`ToolsState::tick`] é chamado no topo do frame pelo app, não aqui.

use utils_ui::i18n::{self, Key, Lang};

use crate::clipboard_ui::Clock;
use crate::history::History;
use crate::tools_calc::{self, CalcState};
use crate::tools_clock::{self, ClockState};
use crate::tools_dev::{self, DevState};
use crate::tools_units::{self, UnitsState};
use crate::ui as kit;

/// Sub-aba aberta dentro de Ferramentas.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SubTab {
    Clock,
    Calc,
    Units,
    Dev,
}

impl SubTab {
    const ALL: [SubTab; 4] = [SubTab::Clock, SubTab::Calc, SubTab::Units, SubTab::Dev];

    fn label_key(self) -> Key {
        match self {
            SubTab::Clock => Key::ToolsClock,
            SubTab::Calc => Key::CalcTab,
            SubTab::Units => Key::UnitsTab,
            SubTab::Dev => Key::DevTab,
        }
    }
}

/// Estado da aba Ferramentas entre frames.
pub struct ToolsState {
    sub: SubTab,
    clock: ClockState,
    calc: CalcState,
    units: UnitsState,
    dev: DevState,
}

impl Default for ToolsState {
    fn default() -> Self {
        Self { sub: SubTab::Clock, clock: ClockState::default(), calc: CalcState::default(), units: UnitsState::default(), dev: DevState::default() }
    }
}

/// O que a aba devolve pro app.
#[derive(Default)]
pub struct Outcome {
    /// Lista de alarmes mudou: regrava o config.
    pub cfg_changed: bool,
    /// Um resultado foi copiado: regrava o histórico de clipboard.
    pub history_changed: bool,
    pub status: Option<String>,
}

impl ToolsState {
    /// Tique do relógio/alarmes. Chamado no TOPO do frame, com a aba aberta ou
    /// não (o alarme tem que tocar do mesmo jeito). Devolve a mensagem pronta.
    pub fn tick(&mut self, lang: Lang, clock: &Clock, alarms: &[(String, String)]) -> Option<String> {
        self.clock.tick(lang, clock, alarms)
    }

    /// Precisa de frame periódico? (algo rodando, aviso piscando ou alarme
    /// armado — o app pede repaint de 500ms nesse caso)
    pub fn wants_repaint(&self, alarms: &[(String, String)]) -> bool {
        self.clock.wants_repaint(alarms)
    }

    /// Sufixo do rótulo da aba: `(0:00)` com o timer zerado, ⏰ piscando
    /// enquanto um aviso do relógio estiver de pé.
    pub fn tab_badge(&self) -> Option<String> {
        self.clock.tab_badge()
    }
}

/// Desenha a aba Ferramentas inteira. `texts` é o histórico de clipboard (os
/// "copiar" dos resultados criam entrada nova, como no recopiar da aba
/// Clipboard); `alarms` é a lista persistida no config.
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut ToolsState, texts: &mut History, alarms: &mut Vec<(String, String)>) -> Outcome {
    let mut out = Outcome::default();

    ui.horizontal(|ui| {
        for sub in SubTab::ALL {
            if kit::pill(ui, i18n::t(lang, sub.label_key()), state.sub == sub).clicked() {
                state.sub = sub;
            }
        }
    });
    ui.add_space(6.0);

    match state.sub {
        SubTab::Clock => {
            let clock = tools_clock::show(ui, lang, &mut state.clock, alarms);
            out.cfg_changed |= clock.cfg_changed;
            out.status = clock.status.or(out.status);
        }
        SubTab::Calc => {
            let calc = tools_calc::show(ui, lang, &mut state.calc, texts);
            out.history_changed |= calc.history_changed;
            out.status = calc.status.or(out.status);
        }
        SubTab::Units => {
            let units = tools_units::show(ui, lang, &mut state.units, texts);
            out.history_changed |= units.history_changed;
            out.status = units.status.or(out.status);
        }
        SubTab::Dev => {
            let dev = tools_dev::show(ui, lang, &mut state.dev, texts);
            out.history_changed |= dev.history_changed;
            out.status = dev.status.or(out.status);
        }
    }
    out
}
