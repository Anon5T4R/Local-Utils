//! Aba Ferramentas: as sub-abas na mesma linha de pills dos filtros da aba
//! Clipboard (Relógio · Calculadora · Unidades · Dev) e o despacho do desenho.
//!
//! O estado dos quatro painéis mora aqui e sobrevive à troca de sub-aba; o
//! relógio é o único que também roda com a aba FECHADA — por isso o
//! [`ToolsState::tick`] é chamado no topo do frame pelo app, não aqui.

use crate::i18n::tr;

use crate::clipboard_ui::Clock;
use crate::history::History;
use crate::tools_calc::{self, CalcState};
use crate::tools_clock::{self, ClockState};
use crate::tools_dev::{self, DevState};
use crate::tools_units::{self, UnitsState};
use crate::ui as kit;

/// Sub-aba aberta dentro de Ferramentas.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubTab {
    Clock,
    Calc,
    Units,
    Dev,
}

impl SubTab {
    pub(crate) const ALL: [SubTab; 4] = [SubTab::Clock, SubTab::Calc, SubTab::Units, SubTab::Dev];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SubTab::Clock => tr("Clock"),
            SubTab::Calc => tr("Calculator"),
            SubTab::Units => tr("Units"),
            SubTab::Dev => tr("Dev tools"),
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
    /// `sound_file` é o `.wav` do config (vazio = bipes do SO).
    pub fn tick(&mut self, clock: &Clock, alarms: &[(String, String)], sound_file: &str) -> Option<String> {
        self.clock.tick(clock, alarms, sound_file)
    }

    /// Abre o diálogo de novo alarme (os testes de largura).
    #[cfg(test)]
    pub(crate) fn open_alarm(&mut self) {
        self.sub = SubTab::Clock;
        self.clock.open_alarm();
    }

    /// Abre uma sub-aba (a paleta de comandos).
    pub(crate) fn select(&mut self, sub: SubTab) {
        self.sub = sub;
    }

    /// O alarme está tocando? (o app corta na primeira interação)
    pub fn sound_playing(&self) -> bool {
        self.clock.sound_playing()
    }

    /// Para o som do alarme na hora.
    pub fn stop_sound(&mut self) {
        self.clock.stop_sound();
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
pub fn show(ui: &mut egui::Ui, state: &mut ToolsState, texts: &mut History, alarms: &mut Vec<(String, String)>) -> Outcome {
    let mut out = Outcome::default();

    // Nível 1 (Relógio · Calculadora · Unidades · Dev) em pills; o hairline e o
    // respiro separam do nível 2 de cada painel (segmented control).
    ui.horizontal_wrapped(|ui| {
        for sub in SubTab::ALL {
            if kit::pill(ui, sub.label(), state.sub == sub).clicked() {
                state.sub = sub;
            }
        }
    });
    ui.add_space(8.0);
    kit::hairline(ui);
    ui.add_space(10.0);

    match state.sub {
        SubTab::Clock => {
            let clock = scrolling(ui, "clock-scroll", |ui| tools_clock::show(ui, &mut state.clock, alarms));
            out.cfg_changed |= clock.cfg_changed;
            out.status = clock.status.or(out.status);
        }
        SubTab::Calc => {
            let calc = scrolling(ui, "calc-scroll", |ui| tools_calc::show(ui, &mut state.calc, texts));
            out.history_changed |= calc.history_changed;
            out.status = calc.status.or(out.status);
        }
        SubTab::Units => {
            let units = scrolling(ui, "units-scroll", |ui| tools_units::show(ui, &mut state.units, texts));
            out.history_changed |= units.history_changed;
            out.status = units.status.or(out.status);
        }
        SubTab::Dev => {
            let dev = tools_dev::show(ui, &mut state.dev, texts);
            out.history_changed |= dev.history_changed;
            out.status = dev.status.or(out.status);
        }
    }
    out
}

/// Os painéis que não rolam por conta própria (o de dev tools tem o scroll dele) entram num
/// `ScrollArea`: numa janela baixa o corpo rola, em vez de deixar controle fora da tela.
fn scrolling<T>(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui) -> T) -> T {
    egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, body).inner
}
