//! Relógio da aba Ferramentas: timer, pomodoro, cronômetro e alarmes.
//!
//! Nada aqui usa timer do SO nem thread: o tempo é a DIFERENÇA entre o instante
//! em que o relógio ligou e `history::now_ms()` no frame (mesmo desenho do
//! monitor de clipboard). Quem pede o repaint de 500ms é o app
//! ([`ClockState::wants_repaint`]) — a aba pode estar fechada e o alarme tem
//! que tocar do mesmo jeito.
//!
//! Som: `alarm_sound.rs` toca o `.wav` do config em loop (ou três bipes do SO
//! sem arquivo) quando um alarme dispara. O toque para no ack do usuário
//! (clique, Esc ou a janela voltando ao foco) e quando o aviso expira.
//!
//! Os alarmes em si vivem no config (`utils_ui::config::Config::alarms`); este
//! módulo só edita a lista e sinaliza `cfg_changed` — quem grava o JSON é o app.

use utils_ui::i18n::{self, Key, Lang};

use crate::alarm_sound::AlarmSound;
use crate::clipboard_ui::{civil_parts, Clock};
use crate::history::now_ms;
use crate::ui::{self as kit, Skin};

/// Quanto tempo o aviso do toque fica piscando antes de sumir sozinho.
const FLASH_MS: u64 = 10_000;
/// Meio período do pisca (ligado/desligado).
const BLINK_MS: u64 = 500;
/// Largura máxima do palco (o app roda maximizado; o visor fica no centro).
const CONTENT_W: f32 = 520.0;
/// Corpo do visor de dígitos (§ padrão do palco central).
const DISPLAY_SIZE: f32 = 96.0;
/// Duração dos presets do timer, em minutos.
const PRESETS: [u32; 4] = [1, 5, 10, 30];
/// Largura de um grupo do stepper do alarme (os botões e os dois dígitos).
const STEP_GROUP_W: f32 = 64.0;
/// Largura da coluna do ":" entre os dois grupos.
const COLON_W: f32 = 18.0;
/// Vão entre os grupos e o ":".
const STEP_GAP: f32 = 8.0;
/// Altura do "+"/"−" do stepper (mesma do espaçador do ":").
const STEP_BUTTON_H: f32 = 28.0;

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
    /// Texto do campo do timer (`hh:mm:ss`) — a ÚNICA fonte da duração dele:
    /// o visor mostra o que este campo diz e os presets reescrevem aqui (mata
    /// o par "5:00" vs "00:05:00").
    timer_text: String,
    /// O visor virou campo de edição (clique nele).
    timer_editing: bool,
    /// O timer zerou e ninguém mexeu: o rótulo da aba mostra `(0:00)`.
    timer_done: bool,
    pomo: Pomodoro,
    stop: Stopwatch,
    /// Popover de "+ alarme" aberto + os dois steppers e o rótulo.
    alarm_open: bool,
    alarm_hour: u32,
    alarm_minute: u32,
    alarm_label: String,
    /// Som do toque (`.wav` do config em loop; sem arquivo, 3 bipes do SO).
    sound: AlarmSound,
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
            timer_editing: false,
            timer_done: false,
            pomo: Pomodoro::default(),
            stop: Stopwatch::default(),
            alarm_open: false,
            // Hora cheia da manhã: o valor mais provável de um alarme novo.
            alarm_hour: 7,
            alarm_minute: 0,
            alarm_label: String::new(),
            sound: AlarmSound::default(),
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

    /// Preset de duração: parte do valor do campo (a fonte única) e recarrega
    /// o timer PARADO com `minutes` a mais. Um timer não passa de 24h.
    fn apply_preset(&mut self, minutes: u32) {
        let base = parse_hhmmss(&self.timer_text).unwrap_or(self.timer.total_ms / 1_000);
        let total = base.saturating_add(u64::from(minutes) * 60).min(24 * 3_600);
        self.timer_text = fmt_hhmmss(total);
        self.timer.set_duration(total.saturating_mul(1_000));
        self.timer_done = false;
    }

    /// Tique do relógio: chamado no TOPO do frame (com a aba visível ou não).
    /// Devolve a mensagem pronta pro rodapé quando algo termina ou toca.
    /// `sound_file` é o `.wav` configurado nas Opções (vazio = bipes do SO).
    pub fn tick(&mut self, lang: Lang, clock: &Clock, alarms: &[(String, String)], sound_file: &str) -> Option<String> {
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
            // Toque do alarme: `.wav` do usuário em loop ou as 3 batidas do SO.
            self.sound.start(sound_file, now);
            self.flash = Some((message.clone(), now));
            status = Some(message);
        }

        // Aviso velho sai sozinho (o repaint do app só existe enquanto ele
        // estiver de pé).
        let expired = self.flash.as_ref().is_some_and(|(_, since)| now.saturating_sub(*since) > FLASH_MS);
        if expired {
            self.flash = None;
            // Sem ninguém olhando, o toque também para: um `.wav` em loop não
            // pode seguir tocando depois que o aviso sumiu da tela.
            self.sound.stop();
        } else if self.sound.playing() {
            // Fallback sem arquivo: as batidas saem espaçadas pelo tique.
            self.sound.tick(now);
        }
        status
    }

    /// O alarme está tocando? (o app corta o som na primeira interação)
    pub fn sound_playing(&self) -> bool {
        self.sound.playing()
    }

    /// Para o som na hora (`PlaySoundW(null)`).
    pub fn stop_sound(&mut self) {
        self.sound.stop();
    }

    /// Precisa de frame periódico? Algo rodando, aviso piscando ou alarme
    /// armado — o app pede repaint de 500ms nesse caso.
    pub fn wants_repaint(&self, alarms: &[(String, String)]) -> bool {
        self.timer.is_running()
            || self.pomo.countdown.is_running()
            || self.stop.is_running()
            || self.flash.is_some()
            || self.sound.playing()
            || !alarms.is_empty()
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

    kit::column(ui, CONTENT_W, |ui| {
        // Nível 2 (Temporizador · Pomodoro · Cronômetro · Alarmes): segmented
        // control menor, separado por hairline das pills de nível 1.
        // Centralizado na largura da coluna: encostado à esquerda sobrava
        // metade da fileira morta à direita.
        let gap = ui.spacing().item_spacing.x;
        let mut modes_w = 0.0;
        for mode in Mode::ALL {
            modes_w += kit::segmented_width(ui, i18n::t(lang, mode.label_key())) + gap;
        }
        modes_w -= gap;
        centered_row(ui, modes_w, |ui| {
            for mode in Mode::ALL {
                if kit::segmented(ui, i18n::t(lang, mode.label_key()), state.mode == mode).clicked() {
                    state.mode = mode;
                    // Trocar de painel fecha a edição do visor e o popover do
                    // alarme (os dois são estado do painel que saiu de cena).
                    state.timer_editing = false;
                    state.alarm_open = false;
                }
            }
        });
        ui.add_space(14.0);

        // Aviso do toque piscando: o MESMO texto que foi pro rodapé. A tinta
        // some e volta em vez de o widget desaparecer (senão o layout pula).
        if let Some((text, since)) = state.flash.clone() {
            let skin = Skin::of(ui);
            let visible = (now.saturating_sub(since) / BLINK_MS).is_multiple_of(2);
            let ink = if visible { skin.accent } else { skin.accent.gamma_multiply(0.0) };
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(text).strong().color(ink));
            });
            ui.add_space(10.0);
        }

        match state.mode {
            Mode::Timer => timer_ui(ui, lang, state, now, &mut out),
            Mode::Pomodoro => pomodoro_ui(ui, lang, state, now),
            Mode::Stopwatch => stopwatch_ui(ui, lang, state, now),
            Mode::Alarms => alarms_ui(ui, lang, state, alarms, &mut out),
        }
    });
    out
}

/// Fileira centralizada: o egui não centraliza `horizontal` sozinho, então a
/// largura vem calculada de fora.
fn centered_row<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        add(ui)
    })
    .inner
}

/// Rótulo pequeno em caixa alta sobre o visor (o "smallcaps" do palco).
fn stage_label(ui: &mut egui::Ui, text: &str) {
    let skin = Skin::of(ui);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text.to_uppercase()).small().color(skin.muted));
    });
}

/// Largura do visor de dígitos (para centralizar e para o clique acertar).
fn display_width(ui: &egui::Ui, text: &str) -> f32 {
    kit::text_width(ui, text, egui::FontId::monospace(DISPLAY_SIZE))
}

/// Visor de dígitos do palco: mono tabular (o número não "dança" a cada
/// segundo) e clicável quando há edição por trás.
fn stage_display(ui: &mut egui::Ui, text: &str, hint: Option<&str>) -> egui::Response {
    let skin = Skin::of(ui);
    let label = egui::Label::new(egui::RichText::new(text).font(egui::FontId::monospace(DISPLAY_SIZE)).color(skin.text)).sense(egui::Sense::click());
    let resp = ui.add(label);
    if resp.hovered() && hint.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    match hint {
        Some(hint) => resp.on_hover_text(hint),
        None => resp,
    }
}

/// Fileira de presets (`+1 min`, `+5 min`…): alimenta o display.
fn presets_row(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState) {
    let labels: Vec<String> = PRESETS.iter().map(|minutes| i18n::t(lang, Key::ClockPresetMin).replace("{n}", &minutes.to_string())).collect();
    let font = egui::TextStyle::Button.resolve(ui.style());
    let spacing = ui.spacing().item_spacing.x;
    let mut total = 0.0;
    for label in &labels {
        total += kit::text_width(ui, label, font.clone()) + 28.0 + spacing;
    }
    total -= spacing;
    centered_row(ui, total, |ui| {
        for (label, minutes) in labels.iter().zip(PRESETS) {
            if kit::pill(ui, label, false).clicked() {
                state.apply_preset(minutes);
            }
        }
    });
}

/// Painel do timer regressivo: palco central com o visor de dígitos.
fn timer_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, now: u64, out: &mut Outcome) {
    stage_label(ui, i18n::t(lang, Key::ClockTimer));
    ui.add_space(4.0);
    if state.timer_editing {
        // Campo fino no lugar do visor: é o MESMO valor que o display mostra
        // (a duração do timer), só que editável.
        let width = 200.0;
        let resp = centered_row(ui, width, |ui| {
            ui.allocate_ui_with_layout(egui::vec2(width, DISPLAY_SIZE), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(kit::line(&mut state.timer_text).desired_width(width))
            })
            .inner
        });
        if resp.changed() {
            // Aplicar ao vivo mantém display e campo contando a mesma coisa.
            if let Some(seconds) = parse_hhmmss(&state.timer_text).filter(|seconds| *seconds > 0) {
                state.timer.set_duration(seconds.saturating_mul(1_000));
                state.timer_done = false;
            }
        }
        let done = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
        if done || resp.lost_focus() {
            state.timer_editing = false;
        }
    } else {
        let text = fmt_clock(state.timer.remaining(now));
        let hint = i18n::t(lang, Key::ClockEditHint);
        let width = display_width(ui, &text);
        let resp = centered_row(ui, width, |ui| stage_display(ui, &text, Some(hint)));
        if resp.clicked() {
            state.timer_editing = true;
        }
    }
    ui.add_space(16.0);
    presets_row(ui, lang, state);
    ui.add_space(14.0);
    let running = state.timer.is_running();
    let label = if running { Key::ClockPause } else { Key::ClockStart };
    centered_row(ui, 288.0, |ui| {
        if kit::big(ui, i18n::t(lang, label), true, 150.0).clicked() {
            out.status = if running {
                state.timer.pause(now);
                None
            } else {
                state.start_timer(lang, now)
            };
        }
        if kit::big(ui, i18n::t(lang, Key::ClockReset), false, 130.0).clicked() {
            out.status = state.reset_timer(lang);
        }
    });
}

/// Painel do pomodoro: a fase é o dado principal (badge grande), o visor
/// abaixo e as durações num par de campos.
fn pomodoro_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, now: u64) {
    let skin = Skin::of(ui);
    let phase_key = match state.pomo.phase {
        PomoPhase::Work => Key::ClockPomodoroWork,
        PomoPhase::Break => Key::ClockPomodoroBreak,
    };
    let phase = i18n::t(lang, phase_key);
    let phase_w = kit::text_width(ui, phase, egui::FontId::proportional(14.0)) + 24.0;
    centered_row(ui, phase_w, |ui| {
        kit::badge_lg(ui, phase, skin.accent);
    });
    ui.add_space(8.0);
    let text = fmt_clock(state.pomo.countdown.remaining(now));
    let width = display_width(ui, &text);
    centered_row(ui, width, |ui| {
        stage_display(ui, &text, None);
    });
    ui.add_space(6.0);
    let cycles = i18n::t(lang, Key::ClockPomodoroCycles).replace("{n}", &state.pomo.cycles.to_string());
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(cycles).small().color(skin.muted));
    });
    ui.add_space(14.0);
    let labels_w = 340.0;
    centered_row(ui, labels_w, |ui| {
        ui.label(egui::RichText::new(i18n::t(lang, Key::ClockPomodoroWorkMin)).small());
        let mut work = state.pomo.work_min;
        if ui.add(egui::DragValue::new(&mut work).range(1..=180)).changed() {
            state.pomo.work_min = work;
            // Parado, mudar o tempo da fase recarrega a contagem na hora.
            if !state.pomo.countdown.is_running() && state.pomo.phase == PomoPhase::Work {
                state.pomo.load_phase();
            }
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new(i18n::t(lang, Key::ClockPomodoroBreakMin)).small());
        let mut rest = state.pomo.break_min;
        if ui.add(egui::DragValue::new(&mut rest).range(1..=180)).changed() {
            state.pomo.break_min = rest;
            if !state.pomo.countdown.is_running() && state.pomo.phase == PomoPhase::Break {
                state.pomo.load_phase();
            }
        }
    });
    ui.add_space(14.0);
    let running = state.pomo.countdown.is_running();
    let label = if running { Key::ClockPause } else { Key::ClockStart };
    centered_row(ui, 288.0, |ui| {
        if kit::big(ui, i18n::t(lang, label), true, 150.0).clicked() {
            if running {
                state.pomo.countdown.pause(now);
            } else {
                state.pomo.countdown.start(now);
            }
        }
        if kit::big(ui, i18n::t(lang, Key::ClockReset), false, 130.0).clicked() {
            state.pomo.reset(now);
        }
    });
}

/// Painel do cronômetro: visor + os dois botões (sem voltas, por desenho).
fn stopwatch_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, now: u64) {
    stage_label(ui, i18n::t(lang, Key::ClockStopwatch));
    ui.add_space(4.0);
    let text = fmt_stopwatch(state.stop.elapsed(now));
    let width = display_width(ui, &text);
    centered_row(ui, width, |ui| {
        stage_display(ui, &text, None);
    });
    ui.add_space(18.0);
    let running = state.stop.is_running();
    let label = if running { Key::ClockStop } else { Key::ClockStart };
    centered_row(ui, 288.0, |ui| {
        if kit::big(ui, i18n::t(lang, label), true, 150.0).clicked() {
            state.stop.toggle(now);
        }
        if kit::big(ui, i18n::t(lang, Key::ClockReset), false, 130.0).clicked() {
            state.stop.reset();
        }
    });
}

/// Painel dos alarmes: lista no padrão Clipboard (hora mono, rótulo, ⏰ e a
/// remoção SEMPRE visível) dentro de um card + o botão "+ alarme", que abre o
/// popover de escolha da hora.
fn alarms_ui(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, alarms: &mut Vec<(String, String)>, out: &mut Outcome) {
    let mut remove = None;
    // Com o popover aberto o card NÃO desenha conteúdo: o modal fica sobre o
    // centro da janela e a lista por baixo aparecia truncada nas beiradas
    // ("Nenhu…", "+ ad…") — o backdrop do kit escurece, mas não esconde.
    if !state.alarm_open {
        let skin = Skin::of(ui);
        let width = ui.available_width();
        kit::card(ui, width, |ui| {
            if alarms.is_empty() {
                ui.label(egui::RichText::new(i18n::t(lang, Key::ClockAlarmsEmpty)).color(skin.muted));
            }
            for (index, (time, label)) in alarms.iter().enumerate() {
                let width = ui.available_width().max(1.0);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
                }
                let inner = rect.shrink2(egui::vec2(8.0, 0.0));
                let mut row = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
                row.label(egui::RichText::new(time).font(egui::FontId::monospace(16.0)).color(skin.text));
                row.add_space(10.0);
                if !label.is_empty() {
                    row.add(egui::Label::new(egui::RichText::new(label).color(skin.text)).truncate());
                }
                row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Remover SEMPRE visível, em ghost discreto e tinta do tema. O
                    // botão só no hover da linha piscava: quando o ponteiro chegava
                    // nele, o próprio botão bloqueava o hover da LINHA e o "×"
                    // sumia — clique e visibilidade brigavam pelo mesmo pixel.
                    if kit::ghost_action(ui, "×").on_hover_text(i18n::t(lang, Key::Remove)).clicked() {
                        remove = Some(index);
                    }
                    kit::badge(ui, "⏰", skin.accent);
                });
                kit::hairline(ui);
            }
            ui.add_space(8.0);
            if kit::pill(ui, i18n::t(lang, Key::ClockAlarmAdd), true).clicked() {
                state.alarm_open = true;
            }
        });
    }
    if let Some(index) = remove {
        alarms.remove(index);
        out.cfg_changed = true;
    }
    alarm_popover(ui, lang, state, alarms, out);
}

/// Popover de novo alarme: hora (0–23) e minuto (0–59) em steppers, o rótulo e
/// o Adicionar. Nada de digitar `hh:mm` à mão.
fn alarm_popover(ui: &mut egui::Ui, lang: Lang, state: &mut ClockState, alarms: &mut Vec<(String, String)>, out: &mut Outcome) {
    if !state.alarm_open {
        return;
    }
    let mut add = false;
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("clock-alarm-new")).frame(kit::modal_frame(ui.ctx())).show(ui.ctx(), |ui| {
        kit::begin_dialog(ui);
        ui.set_width(320.0);
        ui.heading(i18n::t(lang, Key::ClockAlarms));
        ui.add_space(12.0);
        // Fileira dos steppers: rótulos ACIMA de cada grupo, os dois números no
        // MESMO eixo e o ":" no meio. Cada grupo tem largura EXPLÍCITA — um
        // `ui.vertical` dentro de um `horizontal` herda a largura toda, empurrava
        // o resto da fileira pra fora e o "Minuto" virava texto vertical.
        let mut hour = state.alarm_hour;
        let mut minute = state.alarm_minute;
        let row_w = 2.0 * STEP_GROUP_W + COLON_W + 2.0 * STEP_GAP;
        ui.horizontal_top(|ui| {
            // Vão explícito entre os grupos: com o `item_spacing.x` zerado a
            // conta de centralização não depende das folgas automáticas.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
            if stepper_group(ui, "clock-alarm-hour", i18n::t(lang, Key::ClockHour), &mut hour, 23) {
                state.alarm_hour = hour;
            }
            ui.add_space(STEP_GAP);
            stepper_colon(ui);
            ui.add_space(STEP_GAP);
            if stepper_group(ui, "clock-alarm-minute", i18n::t(lang, Key::ClockMinute), &mut minute, 59) {
                state.alarm_minute = minute;
            }
        });
        ui.add_space(12.0);
        ui.add(kit::line(&mut state.alarm_label).hint_text(i18n::t(lang, Key::ClockAlarmLabel)).desired_width(f32::INFINITY));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if kit::pill(ui, i18n::t(lang, Key::ExcludeAdd), true).clicked() {
                add = true;
            }
            if kit::pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                cancel = true;
            }
        });
    });
    if add {
        let hour = state.alarm_hour;
        let minute = state.alarm_minute;
        alarms.push((format!("{hour:02}:{minute:02}"), state.alarm_label.trim().to_string()));
        state.alarm_label.clear();
        state.alarm_open = false;
        out.cfg_changed = true;
    } else if cancel || resp.should_close() {
        state.alarm_open = false;
    }
}

/// Stepper de hora/minuto: o número grande em mono com o "+" acima e o "-"
/// abaixo. Devolve `true` quando o valor mudou (o chamador grava no estado).
fn stepper(ui: &mut egui::Ui, id: &str, value: &mut u32, max: u32) -> bool {
    let skin = Skin::of(ui);
    let mut changed = false;
    // Botões ASCII ("+"/"-"): os triângulos ▲▼ não estão garantidos nas fontes
    // do app (foi o que matou o "✕" do popup).
    ui.push_id(id, |ui| {
        ui.vertical_centered(|ui| {
            if step_button(ui, "+").clicked() {
                *value = if *value >= max { 0 } else { *value + 1 };
                changed = true;
            }
            ui.label(egui::RichText::new(format!("{:02}", *value)).font(egui::FontId::monospace(28.0)).color(skin.text));
            if step_button(ui, "-").clicked() {
                *value = if *value == 0 { max } else { *value - 1 };
                changed = true;
            }
        });
    });
    changed
}

/// Grupo do stepper (rótulo em cima, "+", dígitos e "−" embaixo) com largura
/// PRÓPRIA: dentro de um `horizontal`, `ui.vertical` herda a largura toda e o
/// segundo grupo era empurrado pra fora da janela.
fn stepper_group(ui: &mut egui::Ui, id: &str, label: &str, value: &mut u32, max: u32) -> bool {
    let skin = Skin::of(ui);
    let mut changed = false;
    ui.scope(|ui| {
        ui.set_width(STEP_GROUP_W);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(label).small().color(skin.muted));
            changed = stepper(ui, id, value, max);
        });
    });
    changed
}

/// O ":" entre os dois grupos, alinhado ao eixo dos DÍGITOS: as duas linhas de
/// cima são reservadas com a MESMA altura dos grupos (rótulo e "+"), senão ele
/// subia pro nível do rótulo.
fn stepper_colon(ui: &mut egui::Ui) {
    let skin = Skin::of(ui);
    ui.scope(|ui| {
        ui.set_width(COLON_W);
        ui.vertical_centered(|ui| {
            // Mesma altura do rótulo pequeno dos grupos.
            ui.label(egui::RichText::new(" ").small());
            // Mesma altura do "+" (a área é só espaçador, não pinta nada).
            let _ = ui.allocate_exact_size(egui::vec2(COLON_W, STEP_BUTTON_H), egui::Sense::hover());
            ui.label(egui::RichText::new(":").font(egui::FontId::monospace(28.0)).color(skin.muted));
        });
    });
}

/// Botão de 34×28 do stepper (área de clique folgada, tinta do tema).
fn step_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let skin = Skin::of(ui);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, skin.text);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(34.0, STEP_BUTTON_H), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, skin.text);
    resp
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

/// Segundos → `hh:mm:ss` (o formato que o campo do timer aceita — os presets
/// reescrevem o campo com isto, nunca com um formato próprio).
fn fmt_hhmmss(seconds: u64) -> String {
    format!("{:02}:{:02}:{:02}", seconds / 3_600, (seconds % 3_600) / 60, seconds % 60)
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
