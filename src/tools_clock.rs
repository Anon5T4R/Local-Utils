//! Relógio da aba Ferramentas: timer, pomodoro, cronômetro e alarmes.
//!
//! Nada aqui usa timer do SO nem thread: o tempo é a DIFERENÇA entre o instante
//! em que o relógio ligou e `history::now_ms()` no frame (mesmo desenho do
//! monitor de clipboard). Quem pede o repaint de 500ms é o app
//! ([`ClockState::wants_repaint`]) — a aba pode estar fechada e o alarme tem
//! que tocar do mesmo jeito.
//!
//! Sem som e sem bandeja (nenhuma dep nova): o aviso é o texto que PISCA no
//! painel, a mensagem no rodapé e o `(0:00)` no rótulo da aba.
//!
//! Os alarmes em si vivem no config (`utils_ui::config::Config::alarms`); este
//! módulo só edita a lista e sinaliza `cfg_changed` — quem grava o JSON é o app.

use utils_ui::i18n::{self, Key, Lang};

use crate::clipboard_ui::{civil_parts, Clock};
use crate::history::now_ms;
use crate::ui as kit;

/// Quanto tempo o aviso do toque fica piscando antes de sumir sozinho.
const FLASH_MS: u64 = 10_000;
/// Meio período do pisca (ligado/desligado).
const BLINK_MS: u64 = 500;

/// Modo do relógio (as pills do topo do painel).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Timer,
    Pomodoro,
    Stopwatch,
    Alarms,
}

impl Mode {
    const ALL: [Mode; 4] = [Mode::Timer, Mode::Pomodoro, Mode::Stopwatch, Mode::Alarms];

    fn label_key(self) -> Key {
        match self {
            Mode::Timer => Key::ClockTimer,
            Mode::Pomodoro => Key::ClockPomodoro,
            Mode::Stopwatch => Key::ClockStopwatch,
            Mode::Alarms => Key::ClockAlarms,
        }
    }
}

/// Fase do pomodoro.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PomoPhase {
    Work,
    Break,
}

/// Contagem regressiva: guarda o que faltava quando parou e o instante em que
/// ligou. Parada, `remaining` é constante; rodando, ele só DIMINUI com o tempo.
#[derive(Default)]
struct Countdown {
    total_ms: u64,
    remaining_ms: u64,
    running_since: Option<u64>,
}

impl Countdown {
    fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    fn remaining(&self, now: u64) -> u64 {
        self.running_since.map_or(self.remaining_ms, |started| self.remaining_ms.saturating_sub(now.saturating_sub(started)))
    }

    fn start(&mut self, now: u64) {
        if self.running_since.is_none() && self.remaining_ms > 0 {
            self.running_since = Some(now);
        }
    }

    fn pause(&mut self, now: u64) {
        if let Some(started) = self.running_since.take() {
            self.remaining_ms = self.remaining_ms.saturating_sub(now.saturating_sub(started));
        }
    }

    /// Zera de vez (o `tick` chama quando o tempo acaba).
    fn finish(&mut self) {
        self.running_since = None;
        self.remaining_ms = 0;
    }

    /// Recarrega a duração e para.
    fn set_duration(&mut self, ms: u64) {
        self.total_ms = ms;
        self.remaining_ms = ms;
        self.running_since = None;
    }

    /// Volta pro total atual (Reset quando o campo não tem valor válido).
    fn reset(&mut self) {
        self.remaining_ms = self.total_ms;
        self.running_since = None;
    }
}

/// Pomodoro: duas durações configuráveis, fases que se emendam sozinhas e a
/// contagem de ciclos de TRABALHO fechados nesta sessão.
struct Pomodoro {
    countdown: Countdown,
    work_min: u32,
    break_min: u32,
    phase: PomoPhase,
    cycles: u32,
}

impl Default for Pomodoro {
    fn default() -> Self {
        let mut pomo = Self { countdown: Countdown::default(), work_min: 25, break_min: 5, phase: PomoPhase::Work, cycles: 0 };
        pomo.load_phase();
        pomo
    }
}

impl Pomodoro {
    /// Duração da fase atual em ms (nunca zero: os minutos são 1..=180).
    fn phase_ms(&self) -> u64 {
        let minutes = match self.phase {
            PomoPhase::Work => self.work_min,
            PomoPhase::Break => self.break_min,
        };
        u64::from(minutes.max(1)) * 60_000
    }

    /// Recarrega a contagem da fase atual (parado).
    fn load_phase(&mut self) {
        let ms = self.phase_ms();
        self.countdown.set_duration(ms);
    }

    /// Volta pro começo da sessão: fase de trabalho, ciclo zerado.
    fn reset(&mut self, now: u64) {
        self.countdown.pause(now);
        self.phase = PomoPhase::Work;
        self.cycles = 0;
        self.load_phase();
    }
}

/// Cronômetro: acumulado de quem já parou + o pedaço da volta corrente.
#[derive(Default)]
struct Stopwatch {
    accumulated_ms: u64,
    running_since: Option<u64>,
}

impl Stopwatch {
    fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    fn elapsed(&self, now: u64) -> u64 {
        self.accumulated_ms.saturating_add(self.running_since.map_or(0, |started| now.saturating_sub(started)))
    }

    fn toggle(&mut self, now: u64) {
        match self.running_since.take() {
            Some(started) => self.accumulated_ms = self.accumulated_ms.saturating_add(now.saturating_sub(started)),
            None => self.running_since = Some(now),
        }
    }

    fn reset(&mut self) {
        self.accumulated_ms = 0;
        self.running_since = None;
    }
}

/// Estado do relógio entre frames (fica no `ToolsState`).
pub struct ClockState {
    mode: Mode,
    timer: Countdown,
    /// Texto do campo do timer (`hh:mm:ss`) — vira duração no Play/Reset.
    timer_text: String,
    /// O timer zerou e ninguém mexeu: o rótulo da aba mostra `(0:00)`.
    timer_done: bool,
    pomo: Pomodoro,
    stop: Stopwatch,
    /// Campos da linha de "adicionar alarme".
    alarm_time: String,
    alarm_label: String,
    /// `(hh:mm, minuto local)` que já tocaram — um alarme por minuto.
    fired: Vec<(String, i64)>,
    /// Aviso que pisca (texto pronto + instante em que entrou).
    flash: Option<(String, u64)>,
}

impl Default for ClockState {
    fn default() -> Self {
        let mut timer = Countdown::default();
        timer.set_duration(5 * 60_000);
        Self {
            mode: Mode::Timer,
            timer,
            timer_text: "00:05:00".to_string(),
            timer_done: false,
            pomo: Pomodoro::default(),
            stop: Stopwatch::default(),
            alarm_time: String::new(),
            alarm_label: String::new(),
            fired: Vec::new(),
            flash: None,
        }
    }
}

impl ClockState {
    /// Play do timer. Parado do zero, o CAMPO manda na duração; uma pausa no
    /// meio continua de onde parou (voltar a duração cheia seria perder a
    /// pausa). Devolve a mensagem de erro, quando o texto não for válido.
    fn start_timer(&mut self, lang: Lang, now: u64) -> Option<String> {
        let paused = self.timer.remaining_ms > 0 && self.timer.remaining_ms < self.timer.total_ms;
        if !paused {
            match parse_hhmmss(&self.timer_text) {
                Some(seconds) if seconds > 0 => self.timer.set_duration(seconds.saturating_mul(1_000)),
                _ => return Some(format!("⚠ {}", i18n::t(lang, Key::ClockTimeInvalid))),
            }
        }
        self.timer_done = false;
        self.timer.start(now);
        None
    }

    /// Reset do timer: recarrega a duração do campo (ou o total antigo, se o
    /// texto não presta — e avisa).
    fn reset_timer(&mut self, lang: Lang) -> Option<String> {
        self.timer_done = false;
        match parse_hhmmss(&self.timer_text) {
            Some(seconds) if seconds > 0 => {
                self.timer.set_duration(seconds.saturating_mul(1_000));
                None
            }
            _ => {
                self.timer.reset();
                Some(format!("⚠ {}", i18n::t(lang, Key::ClockTimeInvalid)))
            }
        }
    }

    /// Tique do relógio: chamado no TOPO do frame (com a aba visível ou não).
    /// Devolve a mensagem pronta pro rodapé quando algo termina ou toca.
    pub fn tick(&mut self, lang: Lang, clock: &Clock, alarms: &[(String, String)]) -> Option<String> {
        let now = now_ms();
        let mut status = None;

        // Timer zerou: para e avisa UMA vez (o `timer_done` segura o rótulo da
        // aba até alguém dar Play/Reset).
        if self.timer.is_running() && self.timer.remaining(now) == 0 {
            self.timer.finish();
            self.timer_done = true;
            let message = format!("⏰ {}", i18n::t(lang, Key::ClockTimeUp));
            self.flash = Some((message.clone(), now));
            status = Some(message);
        }

        // Pomodoro: fechou a fase, conta o ciclo de trabalho e emenda na
        // seguinte (quem usa pomodoro não quer clicar a cada 25 minutos).
        if self.pomo.countdown.is_running() && self.pomo.countdown.remaining(now) == 0 {
            let ended = self.pomo.phase;
            if ended == PomoPhase::Work {
                self.pomo.cycles = self.pomo.cycles.saturating_add(1);
            }
            self.pomo.phase = match ended {
                PomoPhase::Work => PomoPhase::Break,
                PomoPhase::Break => PomoPhase::Work,
            };
            self.pomo.load_phase();
            self.pomo.countdown.start(now);
            let next = match self.pomo.phase {
                PomoPhase::Work => Key::ClockPomodoroWork,
                PomoPhase::Break => Key::ClockPomodoroBreak,
            };
            let message = format!("⏰ {}", i18n::t(lang, next));
            self.flash = Some((message.clone(), now));
            status = Some(message);
        }

        // Alarmes: compara a hora local do frame com a lista do config. As
        // batidas guardadas são só do minuto corrente — assim o mesmo alarme
        // toca de novo amanhã (e nunca duas vezes no mesmo minuto).
        let local = clock.local_now_ms();
        let (_, _, _, hour, minute) = civil_parts(local);
        let now_minute = local.div_euclid(60_000);
        self.fired.retain(|(_, batida)| *batida == now_minute);
        let stamp = format!("{hour:02}:{minute:02}");
        for (time, label) in alarms {
            if time != &stamp || self.fired.iter().any(|(fired, _)| fired == time) {
                continue;
            }
            self.fired.push((time.clone(), now_minute));
            let message = if label.is_empty() {
                format!("⏰ {}", i18n::t(lang, Key::ClockAlarms))
            } else {
                format!("⏰ {}", i18n::t(lang, Key::ClockAlarmRinging).replace("{label}", label))
            };
            self.flash = Some((message.clone(), now));
            status = Some(message);
        }

        // Aviso velho sai sozinho (o repaint do app só existe enquanto ele
        // estiver de pé).
        let expired = self.flash.as_ref().is_some_and(|(_, since)| now.saturating_sub(*since) > FLASH_MS);
        if expired {
            self.flash = None;
        }
        status
    }

    /// Precisa de frame periódico? Algo rodando, aviso piscando ou alarme
    /// armado — o app pede repaint de 500ms nesse caso.
    pub fn wants_repaint(&self, alarms: &[(String, String)]) -> bool {
        self.timer.is_running() || self.pomo.countdown.is_running() || self.stop.is_running() || self.flash.is_some() || !alarms.is_empty()
    }

    /// Sufixo do rótulo da aba: `(0:00)` com o timer zerado e um ⏰ piscando
    /// enquanto o aviso (alarme/pomodoro/tempo esgotado) estiver de pé — é o
    /// "a aba pisca" que sobrevive à troca de aba.
    pub fn tab_badge(&self) -> Option<String> {
        if self.timer_done {
            return Some("(0:00)".to_string());
        }
        let (_, since) = self.flash.as_ref()?;
        // Mesma paridade de meio segundo do aviso dentro do painel.
        if (now_ms().saturating_sub(*since) / BLINK_MS).is_multiple_of(2) {
            Some("⏰".to_string())
        } else {
            None
        }
    }
}

/// O que o relógio devolve pro app.
#[derive(Default)]
pub struct Outcome {
    /// A lista de alarmes mudou: o app regrava o config.
    pub cfg_changed: bool,
    pub status: Option<String>,
}

/// Desenha o relógio. `alarms` é a lista persistida no config (editada aqui).
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, alarms: &mut Vec<(String, String)>) -> Outcome {
    let mut out = Outcome::default();
    let now = now_ms();

    ui.horizontal(|ui| {
        for mode in Mode::ALL {
            if kit::pill(ui, i18n::t(lang, mode.label_key()), state.mode == mode).clicked() {
                state.mode = mode;
            }
        }
    });
    ui.add_space(8.0);

    // Aviso do toque piscando: o MESMO texto que foi pro rodapé. A tinta some e
    // volta em vez de o widget desaparecer (senão o layout pula).
    if let Some((text, since)) = state.flash.clone() {
        let skin = crate::ui::Skin::of(ui);
        let visible = (now.saturating_sub(since) / BLINK_MS).is_multiple_of(2);
        let ink = if visible { skin.accent } else { skin.accent.gamma_multiply(0.0) };
        ui.label(egui::RichText::new(text).strong().color(ink));
        ui.add_space(2.0);
    }

    match state.mode {
        Mode::Timer => timer_ui(ui, lang, state, now, &mut out),
        Mode::Pomodoro => pomodoro_ui(ui, lang, state, now),
        Mode::Stopwatch => stopwatch_ui(ui, lang, state, now),
        Mode::Alarms => alarms_ui(ui, lang, state, alarms, &mut out),
    }
    out
}

/// Painel do timer regressivo.
fn timer_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, now: u64, out: &mut Outcome) {
    let skin = crate::ui::Skin::of(ui);
    ui.label(egui::RichText::new(fmt_clock(state.timer.remaining(now))).size(38.0).strong().color(skin.text));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(i18n::t(lang, Key::ClockTimerHint));
        ui.add(kit::line(&mut state.timer_text).desired_width(120.0));
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let running = state.timer.is_running();
        let label = if running { Key::ClockPause } else { Key::ClockStart };
        if kit::pill(ui, i18n::t(lang, label), true).clicked() {
            out.status = if running {
                state.timer.pause(now);
                None
            } else {
                state.start_timer(lang, now)
            };
        }
        if kit::pill(ui, i18n::t(lang, Key::ClockReset), false).clicked() {
            out.status = state.reset_timer(lang);
        }
    });
}

/// Painel do pomodoro.
fn pomodoro_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, now: u64) {
    let skin = crate::ui::Skin::of(ui);
    let phase_key = match state.pomo.phase {
        PomoPhase::Work => Key::ClockPomodoroWork,
        PomoPhase::Break => Key::ClockPomodoroBreak,
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(i18n::t(lang, phase_key)).strong().color(skin.accent));
        let cycles = i18n::t(lang, Key::ClockPomodoroCycles).replace("{n}", &state.pomo.cycles.to_string());
        ui.label(egui::RichText::new(cycles).small().weak());
    });
    ui.label(egui::RichText::new(fmt_clock(state.pomo.countdown.remaining(now))).size(38.0).strong().color(skin.text));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(i18n::t(lang, Key::ClockPomodoroWorkMin));
        let mut work = state.pomo.work_min;
        if ui.add(egui::DragValue::new(&mut work).range(1..=180)).changed() {
            state.pomo.work_min = work;
            // Parado, mudar o tempo da fase recarrega a contagem na hora.
            if !state.pomo.countdown.is_running() && state.pomo.phase == PomoPhase::Work {
                state.pomo.load_phase();
            }
        }
        ui.label(i18n::t(lang, Key::ClockPomodoroBreakMin));
        let mut rest = state.pomo.break_min;
        if ui.add(egui::DragValue::new(&mut rest).range(1..=180)).changed() {
            state.pomo.break_min = rest;
            if !state.pomo.countdown.is_running() && state.pomo.phase == PomoPhase::Break {
                state.pomo.load_phase();
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let running = state.pomo.countdown.is_running();
        let label = if running { Key::ClockPause } else { Key::ClockStart };
        if kit::pill(ui, i18n::t(lang, label), true).clicked() {
            if running {
                state.pomo.countdown.pause(now);
            } else {
                state.pomo.countdown.start(now);
            }
        }
        if kit::pill(ui, i18n::t(lang, Key::ClockReset), false).clicked() {
            state.pomo.reset(now);
        }
    });
}

/// Painel do cronômetro.
fn stopwatch_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, now: u64) {
    let skin = crate::ui::Skin::of(ui);
    let elapsed = state.stop.elapsed(now);
    ui.label(egui::RichText::new(fmt_stopwatch(elapsed)).size(38.0).strong().color(skin.text));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let running = state.stop.is_running();
        let label = if running { Key::ClockStop } else { Key::ClockStart };
        if kit::pill(ui, i18n::t(lang, label), true).clicked() {
            state.stop.toggle(now);
        }
        if kit::pill(ui, i18n::t(lang, Key::ClockReset), false).clicked() {
            state.stop.reset();
        }
    });
}

/// Painel dos alarmes: lista + linha de adicionar.
fn alarms_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, alarms: &mut Vec<(String, String)>, out: &mut Outcome) {
    if alarms.is_empty() {
        ui.label(egui::RichText::new(i18n::t(lang, Key::ClockAlarmsEmpty)).weak());
    }
    let mut remove = None;
    for (index, (time, label)) in alarms.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(time).strong());
            if !label.is_empty() {
                ui.label(label);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let skin = crate::ui::Skin::of(ui);
                if kit::ghost_ink(ui, "🗑", skin.danger).on_hover_text(i18n::t(lang, Key::Remove)).clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        alarms.remove(index);
        out.cfg_changed = true;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add(kit::line(&mut state.alarm_time).hint_text(i18n::t(lang, Key::ClockAlarmTime)).desired_width(130.0));
        ui.add(kit::line(&mut state.alarm_label).hint_text(i18n::t(lang, Key::ClockAlarmLabel)).desired_width(200.0));
        if kit::pill(ui, i18n::t(lang, Key::ExcludeAdd), true).clicked() {
            match parse_hhmm(&state.alarm_time) {
                Some((hour, minute)) => {
                    alarms.push((format!("{hour:02}:{minute:02}"), state.alarm_label.trim().to_string()));
                    state.alarm_time.clear();
                    state.alarm_label.clear();
                    out.cfg_changed = true;
                }
                None => out.status = Some(format!("⚠ {}", i18n::t(lang, Key::ClockAlarmInvalid))),
            }
        }
    });
}

/// `hh:mm:ss` (ou `m:ss` abaixo de uma hora) a partir de ms.
pub fn fmt_clock(ms: u64) -> String {
    let total = ms / 1_000;
    let hours = total / 3_600;
    let minutes = (total % 3_600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `m:ss.d` — o cronômetro mostra décimos.
pub fn fmt_stopwatch(ms: u64) -> String {
    let total = ms / 1_000;
    format!("{}:{:02}.{}", total / 60, total % 60, (ms % 1_000) / 100)
}

/// Texto do campo do timer → segundos. Aceita `hh:mm:ss`, `mm:ss` e `ss`
/// (só o último pode passar de 59 — "90" é uma hora e meia, não um erro).
pub fn parse_hhmmss(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.trim().split(':').map(str::trim).collect();
    let number = |part: &str| part.parse::<u64>().ok();
    match parts.as_slice() {
        [seconds] => number(seconds),
        [minutes, seconds] => {
            let minutes = number(minutes)?;
            let seconds = number(seconds)?;
            if seconds > 59 {
                return None;
            }
            minutes.checked_mul(60)?.checked_add(seconds)
        }
        [hours, minutes, seconds] => {
            let hours = number(hours)?;
            let minutes = number(minutes)?;
            let seconds = number(seconds)?;
            if minutes > 59 || seconds > 59 {
                return None;
            }
            hours.checked_mul(3_600)?.checked_add(minutes.checked_mul(60)?)?.checked_add(seconds)
        }
        _ => None,
    }
}

/// `hh:mm` de um alarme → (hora, minuto).
pub fn parse_hhmm(text: &str) -> Option<(u32, u32)> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let hours: u32 = hours.trim().parse().ok()?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some((hours, minutes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatacao_do_relogio() {
        assert_eq!(fmt_clock(0), "0:00");
        assert_eq!(fmt_clock(65_000), "1:05");
        assert_eq!(fmt_clock(3_600_000), "1:00:00");
        assert_eq!(fmt_clock(3_661_000), "1:01:01");
        assert_eq!(fmt_stopwatch(0), "0:00.0");
        assert_eq!(fmt_stopwatch(1_234), "0:01.2");
        assert_eq!(fmt_stopwatch(61_900), "1:01.9");
    }

    #[test]
    fn leitura_do_campo_do_timer() {
        assert_eq!(parse_hhmmss("00:01:30"), Some(90));
        assert_eq!(parse_hhmmss("01:30"), Some(90));
        assert_eq!(parse_hhmmss("45"), Some(45));
        assert_eq!(parse_hhmmss("90"), Some(90), "um campo só é sempre segundos");
        assert_eq!(parse_hhmmss(" 1:00:00 "), Some(3_600));
        assert_eq!(parse_hhmmss("1:2:3"), Some(3_723));
        // Inválidos.
        assert_eq!(parse_hhmmss(""), None);
        assert_eq!(parse_hhmmss("abc"), None);
        assert_eq!(parse_hhmmss("00:99"), None, "segundo acima de 59");
        assert_eq!(parse_hhmmss("00:99:00"), None, "minuto acima de 59");
        assert_eq!(parse_hhmmss("1:2:3:4"), None);
    }

    #[test]
    fn leitura_do_horario_do_alarme() {
        assert_eq!(parse_hhmm("08:05"), Some((8, 5)));
        assert_eq!(parse_hhmm(" 23:59 "), Some((23, 59)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("0805"), None);
        assert_eq!(parse_hhmm(""), None);
    }

    #[test]
    fn contagem_regressiva_para_e_retoma() {
        let mut count = Countdown::default();
        count.set_duration(10_000);
        assert_eq!(count.remaining(1_000), 10_000, "parado, o tempo não anda");
        count.start(1_000);
        assert_eq!(count.remaining(4_000), 7_000);
        count.pause(4_000);
        assert_eq!(count.remaining(9_999), 7_000, "pausado, congela no que faltava");
        count.start(9_999);
        assert_eq!(count.remaining(20_000), 0, "zerou");
        count.finish();
        assert!(!count.is_running());
        assert_eq!(count.remaining(20_000), 0);
    }

    #[test]
    fn cronometro_soma_voltas() {
        let mut watch = Stopwatch::default();
        assert_eq!(watch.elapsed(500), 0);
        watch.toggle(1_000);
        assert_eq!(watch.elapsed(2_500), 1_500);
        watch.toggle(2_500);
        assert_eq!(watch.elapsed(9_000), 1_500, "parado não conta");
        watch.toggle(9_000);
        assert_eq!(watch.elapsed(9_500), 2_000, "a volta nova soma na anterior");
        watch.reset();
        assert_eq!(watch.elapsed(9_500), 0);
    }

    #[test]
    fn pomodoro_recarrega_a_fase() {
        let mut pomo = Pomodoro::default();
        assert_eq!(pomo.phase, PomoPhase::Work);
        assert_eq!(pomo.countdown.remaining(0), 25 * 60_000);
        pomo.phase = PomoPhase::Break;
        pomo.load_phase();
        assert_eq!(pomo.countdown.remaining(0), 5 * 60_000);
        pomo.reset(0);
        assert_eq!(pomo.phase, PomoPhase::Work);
        assert_eq!(pomo.cycles, 0);
        assert_eq!(pomo.countdown.remaining(0), 25 * 60_000);
    }
}
