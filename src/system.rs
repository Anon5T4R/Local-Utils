//! Aba Sistema: monitor de recursos ao vivo (CPU, memória, discos, rede,
//! bateria, uptime, temperaturas e top processos).
//!
//! **Nada roda com a aba fechada.** O [`SystemState`] só cria o
//! `sysinfo::System` (e os coletores de disco/rede/temperatura) quando a aba
//! está VISÍVEL: o app chama [`SystemState::tick`] a cada frame com a
//! visibilidade na mão e, ao fechar a aba, o estado larga os coletores — mesmo
//! desenho do relógio da aba Ferramentas, que só pede repaint com algo de pé.
//!
//! **O refresh é síncrono no frame**, em cadência de 1s: o `sysinfo` bloqueia
//! por poucos milissegundos e uma thread própria exigiria `Arc<Mutex<System>>`
//! mais canal só para isso — custo e superfície de bug sem ganho.
//!
//! **Temperaturas são best-effort**: vêm do `sysinfo::Components`, que no Linux
//! lê hwmon e no Windows consulta o WMI `MSAcpi_ThermalZoneTemperature` por
//! dentro (a crate já fala COM/WMI). Sem driver, sem admin. Máquina sem zona
//! térmica exposta (VM e boa parte dos desktops) mostra "indisponível" em vez
//! de zero. Como o WMI custa dezenas de ms, a temperatura tem cadência própria
//! (5s) em vez do 1s do resto.
//!
//! **Bateria não está no `sysinfo`**: no Windows vem de
//! `GetSystemPowerStatus` (crate `windows`, feature `Win32_System_Power`) — um
//! P/Invoke, zero crate nova. Fora do Windows a linha mostra "indisponível".

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::{Components, DiskKind, Disks, InterfaceOperationalState, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use utils_ui::i18n::{self, Key, Lang};

use crate::ui as kit;
use crate::ui::Skin;

/// Cadência do refresh (as leituras são baratas; 1s casa com o resto do app).
const TICK_MS: u64 = 1_000;
/// Cadência da temperatura: no Windows cada leitura é uma consulta WMI.
const TEMP_TICK_MS: u64 = 5_000;
/// Amostras guardadas por série: 60 × 1s = o último minuto no sparkline.
const HISTORY: usize = 60;
/// Linhas da tabela de processos.
const TOP_N: usize = 12;
/// Interfaces de rede listadas (as de maior tráfego no último segundo).
const INTERFACES: usize = 4;
/// Alturas do desenho (px).
const SPARK_H: f32 = 34.0;
const BAR_H: f32 = 14.0;
const CORE_BAR_H: f32 = 26.0;
/// Faixa aceita de temperatura (°C). Fora dela o valor é lixo de sensor (o WMI
/// devolve zero/negativo em algumas máquinas) e não entra na sessão.
// Faixa plausível de temperatura em operação (sensor de CPU/GPU real não
// desce de 1°C nem passa de 150): fora disso é leitura morta (zero do WMI
// sem driver, ou lixo) e não entra na média/máxima.
const TEMP_MIN: f32 = 1.0;
const TEMP_MAX: f32 = 150.0;

// ── histórico circular ───────────────────────────────────────────────────

/// Janela deslizante de amostras (a mais velha cai quando a janela enche).
#[derive(Default)]
struct Hist {
    samples: VecDeque<f32>,
}

impl Hist {
    fn push(&mut self, value: f32) {
        if self.samples.len() == HISTORY {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        self.samples.iter().copied()
    }

    fn max(&self) -> f32 {
        self.iter().fold(0.0, f32::max)
    }
}

// ── temperatura da sessão ────────────────────────────────────────────────

/// Acumulado da sessão (não morre quando a aba fecha): média e máxima viram
/// "desde que o app abriu", não "desde que a aba abriu".
#[derive(Default)]
struct TempSession {
    sum: f64,
    count: u64,
    max: Option<f32>,
    last: Option<f32>,
}

impl TempSession {
    /// Registra uma amostra. Valor fora da faixa aceita é descartado: sensor
    /// ausente costuma voltar como zero, e zero puxaria a média pra baixo.
    fn record(&mut self, celsius: f32) {
        if !(TEMP_MIN..=TEMP_MAX).contains(&celsius) {
            return;
        }
        self.sum += f64::from(celsius);
        self.count = self.count.saturating_add(1);
        self.max = Some(self.max.map_or(celsius, |max| max.max(celsius)));
        self.last = Some(celsius);
    }

    fn average(&self) -> Option<f32> {
        if self.count == 0 {
            return None;
        }
        // count > 0 e valores na faixa 0..=150: a divisão não estoura f32.
        Some((self.sum / self.count as f64) as f32)
    }
}

// ── estado ───────────────────────────────────────────────────────────────

/// Ordenação da tabela de processos.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Cpu,
    Mem,
}

/// "Finalizar processo" pendente de confirmação.
struct Confirm {
    pid: u32,
    name: String,
}

/// Uma linha da tabela de processos (cópia dos valores: a UI não segura o
/// empréstimo do `sysinfo` enquanto desenha).
struct ProcRow {
    pid: u32,
    name: String,
    cpu: f32,
    mem: u64,
}

/// Leituras vivas. Existe só enquanto a aba está visível.
struct Live {
    sys: System,
    disks: Option<Disks>,
    nets: Option<Networks>,
    comps: Option<Components>,
    last_tick: Instant,
    last_temp: Instant,
    cpu_hist: Hist,
    down_hist: Hist,
    up_hist: Hist,
    procs: Vec<ProcRow>,
}

impl Live {
    /// Primeira leitura. Falha só se o `sysinfo` não subir (raro); disco, rede e
    /// temperatura são opcionais e degradam para "indisponível".
    fn new() -> Option<Self> {
        let mut sys = System::new().ok()?;
        // Primeira passada: CPU e processos precisam de DUAS leituras para
        // medir (a diferença é o uso). A primeira sai zerada e o tique de 1s
        // corrige — melhor um "0%" por um segundo do que travar o frame
        // esperando `MINIMUM_CPU_UPDATE_INTERVAL`.
        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory());
        let now = Instant::now();
        Some(Self {
            sys,
            disks: Disks::new_with_refreshed_list().ok(),
            nets: Networks::new_with_refreshed_list().ok(),
            comps: Components::new_with_refreshed_list().ok(),
            last_tick: now,
            last_temp: now,
            cpu_hist: Hist::default(),
            down_hist: Hist::default(),
            up_hist: Hist::default(),
            procs: Vec::new(),
        })
    }

    fn refresh(&mut self, sort: SortBy, temps: &mut TempSession) {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        // `refresh_processes` sem `specifics` só traz PID/nome — CPU e memória
        // exigem o kind explícito.
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory());
        if let Some(disks) = self.disks.as_mut() {
            disks.refresh(true);
        }
        if let Some(nets) = self.nets.as_mut() {
            nets.refresh(true);
        }
        // Temperatura em cadência própria (WMI no Windows não é leitura de ms).
        if self.last_temp.elapsed() >= Duration::from_millis(TEMP_TICK_MS) {
            if let Some(comps) = self.comps.as_mut() {
                comps.refresh(true);
            }
            self.last_temp = Instant::now();
            if let Some(hottest) = self.comps.as_ref().and_then(|comps| comps.into_iter().filter_map(|c| c.temperature()).reduce(f32::max)) {
                // Uma amostra por tique = a zona mais quente da máquina (as
                // demais variam junto; a média por zona diluiria o alerta).
                temps.record(hottest);
            }
        }
        self.cpu_hist.push(self.sys.global_cpu_usage());
        let (down, up) = net_deltas(self.nets.as_ref());
        self.down_hist.push(down);
        self.up_hist.push(up);
        self.procs = collect_procs(&self.sys, sort);
        self.last_tick = Instant::now();
    }
}

/// Estado da aba Sistema entre frames.
pub struct SystemState {
    live: Option<Live>,
    /// A criação do `sysinfo` falhou nesta passada visível: não insiste a cada
    /// frame (só re-tenta quando a aba fecha e abre de novo).
    failed: bool,
    sort: SortBy,
    confirm: Option<Confirm>,
    /// Acumulado de temperatura da sessão do APP (sobrevive a esconder a aba).
    temps: TempSession,
}

impl Default for SystemState {
    fn default() -> Self {
        Self { live: None, failed: false, sort: SortBy::Cpu, confirm: None, temps: TempSession::default() }
    }
}

impl SystemState {
    /// Tique do monitor. Chamado no topo do frame com a visibilidade da aba na
    /// mão: fechada, larga os coletores e não pede repaint nenhum.
    pub fn tick(&mut self, ctx: &egui::Context, visible: bool) {
        if !visible {
            // Nada roda com a aba fechada — inclusive o custo de manter o
            // `System` vivo e o repaint de 1s.
            self.live = None;
            self.failed = false;
            return;
        }
        let now = Instant::now();
        if self.live.is_none() && !self.failed {
            match Live::new() {
                Some(live) => self.live = Some(live),
                None => self.failed = true,
            }
        }
        if let Some(live) = self.live.as_mut() {
            let due = now.saturating_duration_since(live.last_tick) >= Duration::from_millis(TICK_MS);
            if due {
                live.refresh(self.sort, &mut self.temps);
            }
        }
        // Repaint de 1s: só a aba visível paga o tique; fechada, nada acorda a UI.
        ctx.request_repaint_after(Duration::from_millis(TICK_MS));
    }

    /// Finaliza um processo. `false` = sem permissão, PID já morto ou leitura
    /// indisponível (o chamador avisa no rodapé).
    fn kill(&mut self, pid: u32) -> bool {
        self.live.as_ref().and_then(|live| live.sys.process(Pid::from_u32(pid))).map(|process| process.kill()).unwrap_or(false)
    }
}

// ── coleta pura ──────────────────────────────────────────────────────────

/// Top processos, já ordenados e copiados para a UI.
fn collect_procs(sys: &System, sort: SortBy) -> Vec<ProcRow> {
    let own_pid = std::process::id();
    let mut rows: Vec<ProcRow> = sys
        .processes()
        .values()
        // PID 0 é o "idle" do Windows: não tem nome nem uso útil.
        // O próprio app fica fora: finalizá-lo mataria o histórico sem salvar.
        .filter(|process| process.pid().as_u32() != 0 && process.pid().as_u32() != own_pid)
        .map(|process| ProcRow {
            pid: process.pid().as_u32(),
            name: process.name().to_string_lossy().into_owned(),
            cpu: process.cpu_usage(),
            mem: process.memory(),
        })
        .collect();
    match sort {
        SortBy::Cpu => rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu)),
        SortBy::Mem => rows.sort_by_key(|r| std::cmp::Reverse(r.mem)),
    }
    rows.truncate(TOP_N);
    rows
}

/// Bytes/s somados nas interfaces `Up`. O `refresh` roda a 1 Hz, então o delta
/// desde a última leitura é a taxa por segundo.
fn net_deltas(nets: Option<&Networks>) -> (f32, f32) {
    let Some(nets) = nets else { return (0.0, 0.0) };
    let mut down: u64 = 0;
    let mut up: u64 = 0;
    for (_name, data) in nets {
        if data.operational_state() == InterfaceOperationalState::Up {
            down = down.saturating_add(data.received());
            up = up.saturating_add(data.transmitted());
        }
    }
    (down as f32, up as f32)
}

/// Rótulo do tipo de disco. `None` = o SO não soube dizer (a UI traduz).
fn disk_kind_label(kind: DiskKind) -> Option<&'static str> {
    match kind {
        DiskKind::HDD => Some("HDD"),
        DiskKind::SSD => Some("SSD"),
        _ => None,
    }
}

// ── formatação (pura, testada) ───────────────────────────────────────────

/// Unidades de base 1024, do byte ao tebibyte.
const BYTE_UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

/// Bytes → "512 B", "1.5 KiB", "3.0 GiB" (1 casa a partir do KiB; B é inteiro).
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < BYTE_UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", BYTE_UNITS[unit])
}

/// Taxa de transferência (o delta de 1s já é bytes/s).
fn format_rate(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

/// Segundos → "12m", "1h 01m", "1d 01h 01m". Letras de unidade são símbolos
/// (como "%" e "GB"), não palavras — nada a traduzir.
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours:02}h {minutes:02}m")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

// ── bateria (Windows) ────────────────────────────────────────────────────

/// Estado da bateria lido do SO.
struct Battery {
    percent: u8,
    charging: bool,
    plugged: bool,
}

#[cfg(windows)]
fn battery() -> Option<Battery> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: 0,
        BatteryFlag: 0,
        BatteryLifePercent: 255,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    // SAFETY: `GetSystemPowerStatus` escreve na struct que passamos e não
    // guarda o ponteiro; nenhum pré-requisito além do buffer válido.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    // 128 = "no system battery", 255 = flag/percentual desconhecido.
    if status.BatteryFlag == 128 || status.BatteryLifePercent == 255 {
        return None;
    }
    Some(Battery {
        percent: status.BatteryLifePercent.min(100),
        // Bit 8 (0x08) = carregando.
        charging: status.BatteryFlag & 8 != 0,
        // 1 = na tomada.
        plugged: status.ACLineStatus == 1,
    })
}

#[cfg(not(windows))]
fn battery() -> Option<Battery> {
    None
}

// ── UI ───────────────────────────────────────────────────────────────────

/// O que a aba devolve pro app (mensagem do rodapé).
#[derive(Default)]
pub struct Outcome {
    pub status: Option<String>,
}

/// Título de uma seção.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong());
}

/// Barra de progresso com título (esquerda) e valor (direita) na linha de cima.
fn meter(ui: &mut egui::Ui, title: &str, value: &str, frac: f32, color: egui::Color32, skin: Skin) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).small());
        });
    });
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), BAR_H), egui::Sense::hover());
    ui.painter().rect(rect, egui::CornerRadius::same(4), skin.hover, egui::Stroke::new(1.0_f32, skin.border), egui::StrokeKind::Inside);
    let frac = frac.clamp(0.0, 1.0);
    if frac > 0.0 {
        let filled = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac, rect.height()));
        ui.painter().rect_filled(filled, egui::CornerRadius::same(4), color);
    }
}

/// Sparkline do histórico. `scale` = teto fixo da série (100 no CPU); zero
/// significa escala automática pela maior amostra da janela.
fn sparkline(ui: &mut egui::Ui, hist: &Hist, scale: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), SPARK_H), egui::Sense::hover());
    if hist.len() < 2 {
        return;
    }
    let scale = if scale > 0.0 { scale } else { hist.max().max(1.0) };
    let painter = ui.painter();
    let step = rect.width() / (HISTORY - 1) as f32;
    let mut previous: Option<egui::Pos2> = None;
    for (index, value) in hist.iter().enumerate() {
        let x = rect.left() + index as f32 * step;
        let y = rect.bottom() - (value / scale).clamp(0.0, 1.0) * rect.height();
        let point = egui::pos2(x, y);
        if let Some(previous) = previous {
            painter.line_segment([previous, point], egui::Stroke::new(1.5_f32, color));
        }
        previous = Some(point);
    }
}

/// Barras finas, uma por núcleo, preenchendo a largura disponível.
fn core_bars(ui: &mut egui::Ui, usages: &[f32], skin: Skin) {
    if usages.is_empty() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), CORE_BAR_H), egui::Sense::hover());
    let gap = 2.0;
    let count = usages.len() as f32;
    let bar_w = ((rect.width() - gap * (count - 1.0)) / count).max(2.0);
    for (index, usage) in usages.iter().enumerate() {
        let x = rect.left() + index as f32 * (bar_w + gap);
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(bar_w, rect.height()));
        ui.painter().rect_filled(cell, egui::CornerRadius::same(1), skin.hover);
        let frac = (*usage / 100.0).clamp(0.0, 1.0);
        if frac > 0.0 {
            let filled = egui::Rect::from_min_max(cell.left_bottom() - egui::vec2(0.0, cell.height() * frac), cell.right_bottom());
            ui.painter().rect_filled(filled, egui::CornerRadius::same(1), skin.accent);
        }
    }
}

/// Desenha a aba inteira. O refresh NÃO acontece aqui: quem tica é [`SystemState::tick`].
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut SystemState) -> Outcome {
    let mut out = Outcome::default();
    let ctx = ui.ctx().clone();

    // Sem leituras ainda (primeiro frame depois de abrir a aba, ou sysinfo que
    // não subiu): um aviso curto e o pedido de frame seguinte.
    let Some(live) = state.live.as_ref() else {
        let _ = ui.centered_and_justified(|ui| {
            if state.failed {
                ui.label(egui::RichText::new(i18n::t(lang, Key::SysUnavailable)).weak());
            } else {
                // "…" é símbolo, não texto traduzível.
                ui.label(egui::RichText::new("…").weak());
            }
        });
        ctx.request_repaint();
        return out;
    };

    let skin = Skin::of(ui);
    let mut sort_choice: Option<SortBy> = None;
    let mut kill_request: Option<(u32, String)> = None;
    let sort = state.sort;
    // Acumulado de temperatura copiado para fora: o closure do painel não
    // precisa (e não deve) emprestar `state` enquanto `live` está emprestado.
    let temps_average = state.temps.average();
    let temps_max = state.temps.max;
    let temps_last = state.temps.last;

    egui::ScrollArea::vertical().id_salt("system-scroll").auto_shrink([false, false]).show(ui, |ui| {
        // ── CPU ──────────────────────────────────────────────────────
        let cpu = live.sys.global_cpu_usage();
        heading(ui, i18n::t(lang, Key::SysCpu));
        // Título vazio: o número já está à direita e o cabeçalho acima diz o resto.
        meter(ui, "", &format!("{cpu:.0}%"), cpu / 100.0, skin.accent, skin);
        sparkline(ui, &live.cpu_hist, 100.0, skin.accent);
        ui.add_space(2.0);
        ui.label(egui::RichText::new(i18n::t(lang, Key::SysPerCore)).small().weak());
        let cores: Vec<f32> = live.sys.cpus().iter().map(|core| core.usage()).collect();
        core_bars(ui, &cores, skin);
        ui.add_space(10.0);
        ui.separator();

        // ── Memória e swap ───────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::SysMemory));
        let total_ram = live.sys.total_memory();
        let used_ram = live.sys.used_memory();
        let ram_frac = if total_ram == 0 { 0.0 } else { used_ram as f32 / total_ram as f32 };
        meter(ui, "RAM", &format!("{} / {}", format_bytes(used_ram), format_bytes(total_ram)), ram_frac, skin.accent, skin);
        let total_swap = live.sys.total_swap();
        let used_swap = live.sys.used_swap();
        // Sem arquivo de paginação o swap é 0/0: mostra "0 B / 0 B" (não é erro).
        let swap_frac = if total_swap == 0 { 0.0 } else { used_swap as f32 / total_swap as f32 };
        meter(ui, i18n::t(lang, Key::SysSwap), &format!("{} / {}", format_bytes(used_swap), format_bytes(total_swap)), swap_frac, skin.muted, skin);
        ui.add_space(10.0);
        ui.separator();

        // ── Discos por volume ────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::SysDisks));
        let mut any_disk = false;
        if let Some(disks) = live.disks.as_ref() {
            for disk in disks {
                let total = disk.total_space();
                if total == 0 {
                    continue;
                }
                any_disk = true;
                let used = total.saturating_sub(disk.available_space());
                let kind = disk_kind_label(disk.kind()).unwrap_or_else(|| i18n::t(lang, Key::SysUnknown));
                let name = format!("{} · {kind}", disk.mount_point().display());
                let frac = used as f32 / total as f32;
                meter(ui, &name, &format!("{} / {}", format_bytes(used), format_bytes(total)), frac, skin.accent, skin);
            }
        }
        if !any_disk {
            ui.label(egui::RichText::new(i18n::t(lang, Key::SysUnavailable)).small().weak());
        }
        ui.add_space(10.0);
        ui.separator();

        // ── Rede ─────────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::SysNetwork));
        let (down, up) = net_deltas(live.nets.as_ref());
        ui.label(egui::RichText::new(format!("↓ {}   ↑ {}", format_rate(down as u64), format_rate(up as u64))).small());
        sparkline(ui, &live.down_hist, 0.0, skin.accent);
        if let Some(nets) = live.nets.as_ref() {
            let mut rows: Vec<(String, u64, u64)> = nets
                .into_iter()
                .filter(|(_name, data)| {
                    data.operational_state() == InterfaceOperationalState::Up && (data.total_received() > 0 || data.total_transmitted() > 0)
                })
                .map(|(name, data)| (name.to_string_lossy().into_owned(), data.received(), data.transmitted()))
                .collect();
            rows.sort_by_key(|(_, down, up)| std::cmp::Reverse(down.saturating_add(*up)));
            for (name, down, up) in rows.iter().take(INTERFACES) {
                ui.label(egui::RichText::new(format!("{name}   ↓ {}   ↑ {}", format_rate(*down), format_rate(*up))).small().weak());
            }
        }
        ui.add_space(10.0);
        ui.separator();

        // ── Bateria ──────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::SysBattery));
        match battery() {
            Some(info) => {
                let state_key = if info.charging {
                    Key::SysCharging
                } else if info.plugged {
                    Key::SysPlugged
                } else {
                    Key::SysOnBattery
                };
                let frac = f32::from(info.percent) / 100.0;
                // Bateria fraca fora da tomada é o único caso que pede cor de
                // alerta (o resto é leitura normal).
                let color = if info.percent <= 20 && !info.plugged { skin.danger } else { skin.accent };
                meter(ui, "", &format!("{}% · {}", info.percent, i18n::t(lang, state_key)), frac, color, skin);
            }
            None => {
                ui.label(egui::RichText::new(i18n::t(lang, Key::SysNoBattery)).small().weak());
            }
        }
        ui.add_space(10.0);
        ui.separator();

        // ── Uptime ───────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::SysUptime));
        let uptime = System::uptime().map(format_uptime).unwrap_or_else(|_| i18n::t(lang, Key::SysUnavailable).to_string());
        ui.label(egui::RichText::new(uptime).small());

        // ── Temperaturas (best-effort) ───────────────────────────────
        // Se o WMI parar de responder depois de ter funcionado, a última
        // leitura da sessão continua no cabeçalho em vez de "indisponível".
        let current = live.comps.as_ref().and_then(|comps| comps.into_iter().filter_map(|c| c.temperature()).reduce(f32::max)).or(temps_last);
        let header = match current {
            Some(value) => format!("{} · {value:.0} °C", i18n::t(lang, Key::SysTemperatures)),
            None => i18n::t(lang, Key::SysTemperatures).to_string(),
        };
        egui::CollapsingHeader::new(egui::RichText::new(header).strong()).id_salt("system-temps").show(ui, |ui| {
            let mut any = false;
            if let Some(comps) = live.comps.as_ref() {
                for component in comps {
                    if let Some(value) = component.temperature() {
                        any = true;
                        ui.label(egui::RichText::new(format!("{} · {value:.0} °C", component.label())).small());
                    }
                }
            }
            if !any && current.is_none() {
                ui.label(egui::RichText::new(i18n::t(lang, Key::SysUnavailable)).small().weak());
            }
            let average = temps_average.map_or_else(|| "—".to_string(), |value| format!("{value:.0} °C"));
            let maximum = temps_max.map_or_else(|| "—".to_string(), |value| format!("{value:.0} °C"));
            ui.label(
                egui::RichText::new(format!("{}: {average}   {}: {maximum}", i18n::t(lang, Key::SysAvg), i18n::t(lang, Key::SysMax))).small().weak(),
            );
        });
        ui.add_space(10.0);
        ui.separator();

        // ── Top processos ────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::SysProcesses));
        ui.horizontal(|ui| {
            if kit::pill(ui, "CPU", sort == SortBy::Cpu).clicked() {
                sort_choice = Some(SortBy::Cpu);
            }
            if kit::pill(ui, "RAM", sort == SortBy::Mem).clicked() {
                sort_choice = Some(SortBy::Mem);
            }
        });
        ui.add_space(4.0);
        egui::Grid::new("system-procs").num_columns(5).striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
            ui.label(egui::RichText::new(i18n::t(lang, Key::SysProcesses)).small().weak());
            ui.label(egui::RichText::new("PID").small().weak());
            ui.label(egui::RichText::new("CPU").small().weak());
            ui.label(egui::RichText::new("RAM").small().weak());
            ui.label("");
            ui.end_row();
            for row in &live.procs {
                ui.add(egui::Label::new(egui::RichText::new(&row.name).small()).truncate());
                ui.label(egui::RichText::new(row.pid.to_string()).small().weak());
                ui.label(egui::RichText::new(format!("{:.0}%", row.cpu)).small());
                ui.label(egui::RichText::new(format_bytes(row.mem)).small().weak());
                if kit::ghost(ui, "✕").on_hover_text(i18n::t(lang, Key::SysKill)).clicked() {
                    kill_request = Some((row.pid, row.name.clone()));
                }
                ui.end_row();
            }
        });
    });

    // Ordenação trocada: reordena o cache na hora (o refresh seguinte repetiria
    // a mesma conta em 1s — quem clicou quer o resultado já).
    if let Some(sort) = sort_choice {
        state.sort = sort;
        if let Some(live) = state.live.as_mut() {
            // Reordena a cópia que a UI já tem, sem tocar no `sysinfo`.
            match sort {
                SortBy::Cpu => live.procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu)),
                SortBy::Mem => live.procs.sort_by_key(|p| std::cmp::Reverse(p.mem)),
            }
        }
    }

    // Pedido de finalização: abre o modal de confirmação (nada é morto sem OK).
    if let Some((pid, name)) = kill_request {
        state.confirm = Some(Confirm { pid, name });
    }

    // Modal de confirmação (por cima do painel, como o das opções). O `take`
    // evita dois empréstimos de `state` no mesmo frame: o modal devolve o
    // pedido intacto se o usuário não confirmar.
    if let Some(confirm) = state.confirm.take() {
        let mut close = false;
        let mut do_kill = false;
        egui::Window::new(i18n::t(lang, Key::SysKill))
            .id(egui::Id::unique("system-kill"))
            .frame(kit::modal_frame(&ctx))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(380.0)
            .show(&ctx, |ui| {
                ui.set_width(380.0);
                let pid = confirm.pid.to_string();
                let question = i18n::t(lang, Key::SysKillConfirm).replace("{name}", &confirm.name).replace("{pid}", &pid);
                ui.label(question);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if kit::pill_danger(ui, i18n::t(lang, Key::SysKill)).clicked() {
                        do_kill = true;
                    }
                    if kit::pill(ui, i18n::t(lang, Key::Cancel), false).clicked() {
                        close = true;
                    }
                });
            });
        if do_kill {
            let ok = state.kill(confirm.pid);
            out.status = Some(if ok { i18n::t(lang, Key::SysKilled).to_string() } else { format!("⚠ {}", i18n::t(lang, Key::SysKillFailed)) });
        } else if !close {
            state.confirm = Some(confirm);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_com_uma_casa_a_partir_do_kib() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
        // O topo da escala é o TiB: acima disso o número cresce, a unidade não.
        assert_eq!(format_bytes(u64::MAX), "16777216.0 TiB");
    }

    #[test]
    fn historico_guarda_a_janela_do_ultimo_minuto() {
        let mut hist = Hist::default();
        for sample in 0..(HISTORY + 10) {
            hist.push(sample as f32);
        }
        assert_eq!(hist.len(), HISTORY, "a janela não cresce além do teto");
        assert_eq!(hist.iter().next(), Some(10.0), "as amostras velhas caem pela frente");
        assert_eq!(hist.iter().last(), Some((HISTORY + 9) as f32));
        assert_eq!(hist.max(), (HISTORY + 9) as f32);
    }

    #[test]
    fn janela_vazia_nao_estoura() {
        let hist = Hist::default();
        assert_eq!(hist.len(), 0);
        assert_eq!(hist.max(), 0.0);
        assert_eq!(hist.iter().next(), None);
    }

    #[test]
    fn temperatura_media_e_maxima_da_sessao() {
        let mut temps = TempSession::default();
        assert_eq!(temps.average(), None);
        assert_eq!(temps.max, None);

        temps.record(40.0);
        temps.record(60.0);
        temps.record(50.0);
        assert_eq!(temps.average(), Some(50.0));
        assert_eq!(temps.max, Some(60.0));
        assert_eq!(temps.last, Some(50.0));

        // Sensor que devolve lixo (zero/negativo/acima da faixa) não entra.
        temps.record(0.0);
        temps.record(-5.0);
        temps.record(250.0);
        assert_eq!(temps.average(), Some(50.0));
        assert_eq!(temps.max, Some(60.0));
        assert_eq!(temps.count, 3);
    }

    #[test]
    fn uptime_em_dias_horas_e_minutos() {
        assert_eq!(format_uptime(0), "0m");
        assert_eq!(format_uptime(59), "0m");
        assert_eq!(format_uptime(61), "1m");
        assert_eq!(format_uptime(3_661), "1h 01m");
        assert_eq!(format_uptime(90_061), "1d 01h 01m");
        assert_eq!(format_uptime(86_400), "1d 00h 00m");
    }

    #[test]
    fn taxa_reaproveita_o_formatador_de_bytes() {
        assert_eq!(format_rate(0), "0 B/s");
        assert_eq!(format_rate(2048), "2.0 KiB/s");
    }

    #[test]
    fn tipo_de_disco_traduzivel() {
        assert_eq!(disk_kind_label(DiskKind::HDD), Some("HDD"));
        assert_eq!(disk_kind_label(DiskKind::SSD), Some("SSD"));
        assert_eq!(disk_kind_label(DiskKind::Unknown(7)), None, "o SO não soube: a UI escreve 'desconhecido'");
    }
}
