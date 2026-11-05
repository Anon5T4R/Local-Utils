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
/// Largura máxima da coluna de conteúdo (o app roda maximizado; 1900px de
/// linha não se lê). O grid de cards vive DENTRO desse teto.
const CONTENT_W: f32 = 1_200.0;
/// Vão entre os cards da grade.
const GUTTER: f32 = 12.0;
/// Largura mínima da coluna para a grade abrir DUAS colunas. Abaixo disso os
/// cards empilham: a janela sem maximizar mostra menos card por linha, mas
/// nenhum corta.
const TWO_COL_MIN_W: f32 = 900.0;
// Alturas do desenho (px).
/// Sparkline do card do CPU (o gráfico principal do painel).
const SPARK_H: f32 = 56.0;
/// Sparkline da rede: o card dela é mais baixo e o gráfico é de apoio.
const SPARK_H_NET: f32 = 36.0;
/// Largura-base do chip de um núcleo (o par "C0 12%" com a barrinha interna):
/// a grade tira a capacidade daqui e depois ESTICA cada chip para preencher a
/// linha do card.
const CHIP_W: f32 = 78.0;
const CHIP_H: f32 = 42.0;
/// Vão entre os chips da grade de núcleos (horizontal e vertical).
const CHIP_GAP: f32 = 6.0;
/// Altura de uma linha da lista de processos.
const PROC_H: f32 = 40.0;
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
        // `max(0.0)`: a PRIMEIRA leitura do sysinfo pode sair NEGATIVA (a janela
        // de amostragem ainda não tem delta; o uso sai -5%) e a UI mostrava
        // "-5%". Histórico, barras e sparkline herdam o clamp daqui.
        self.cpu_hist.push(usage_non_negative(self.sys.global_cpu_usage()));
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

/// Uso de CPU nunca é negativo na tela. A primeira leitura do `sysinfo` (e a de
/// um processo recém-visto) pode voltar negativa por falta de janela de
/// amostragem — artefato de medição, não uso real.
fn usage_non_negative(usage: f32) -> f32 {
    usage.max(0.0)
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

/// Cabeçalho do card: rótulo e valor na MESMA linha, lado a lado. O valor
/// nunca vai pro canto oposto da janela — o card é a unidade de leitura.
fn card_head(ui: &mut egui::Ui, title: &str, value: &str, ink: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        if !value.is_empty() {
            ui.label(egui::RichText::new(value).strong().color(ink));
        }
    });
}

/// Linha de medida: rótulo + valor juntos e a barra do card embaixo.
fn meter_row(ui: &mut egui::Ui, label: &str, value: &str, frac: f32, color: egui::Color32) {
    let skin = Skin::of(ui);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).small().color(skin.muted));
        ui.label(egui::RichText::new(value).small());
    });
    ui.add_space(3.0);
    kit::progress(ui, frac, color);
}

/// Quantos chips de núcleo cabem numa linha do card (o espaçamento entre eles
/// é o `item_spacing.x` do kit, 6px).
fn chip_capacity(inner_w: f32) -> usize {
    (((inner_w + CHIP_GAP) / (CHIP_W + CHIP_GAP)).floor() as usize).max(1)
}

/// Grade dos núcleos: quantas colunas cabem, em quantas linhas e a largura de
/// cada chip. A largura é calculada para a linha PREENCHER o card de ponta a
/// ponta (em vez de sobrar um vão morto à direita).
#[derive(Clone, Copy)]
struct CoreGrid {
    cols: usize,
    rows: usize,
    chip_w: f32,
}

/// Geometria da grade a partir da largura útil do card e da contagem de
/// núcleos. A MESMA conta dimensiona o card ([`cpu_card_h`]) e desenha os
/// chips ([`core_chips`]): os dois nunca discordam.
fn core_grid(inner_w: f32, cores: usize) -> CoreGrid {
    let cols = chip_capacity(inner_w).min(cores.max(1));
    let cols_f = cols as f32;
    let chip_w = ((inner_w - (cols_f - 1.0) * CHIP_GAP) / cols_f).max(1.0);
    let rows = cores.div_ceil(cols).max(1);
    CoreGrid { cols, rows, chip_w }
}

/// Duas células da grade lado a lado ou empilhadas, conforme a largura. Nas
/// duas colunas as células da MESMA linha compartilham `min_h`; empilhadas a
/// altura mínima é descartada (cada card cresce o que precisa).
fn card_pair(ui: &mut egui::Ui, two_cols: bool, card_w: f32, min_h: f32, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    if two_cols {
        ui.horizontal_top(|ui| {
            kit::card_h(ui, card_w, min_h, left);
            ui.add_space(GUTTER);
            kit::card_h(ui, card_w, min_h, right);
        });
    } else {
        kit::card(ui, card_w, left);
        ui.add_space(GUTTER);
        kit::card(ui, card_w, right);
    }
}

/// Núcleos como chips numa grade que preenche o card: as linhas são explícitas
/// (`chunks`), não um wrap implícito — nenhum chip escapa da borda e a altura
/// do card é exatamente a desta grade.
fn core_chips(ui: &mut egui::Ui, usages: &[f32], grid: CoreGrid) {
    let skin = Skin::of(ui);
    ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, CHIP_GAP);
    for (row, chunk) in usages.chunks(grid.cols).enumerate() {
        ui.horizontal(|ui| {
            for (col, usage) in chunk.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(grid.chip_w, CHIP_H), egui::Sense::hover());
                paint_core_chip(ui, rect, row * grid.cols + col, *usage, skin);
            }
        });
    }
}

/// Um chip de núcleo: "C0" e o percentual em cima, barrinha interna embaixo.
fn paint_core_chip(ui: &egui::Ui, rect: egui::Rect, index: usize, usage: f32, skin: Skin) {
    // Mesmo clamp da leitura global: núcleo recém-medido pode vir negativo.
    let usage = usage_non_negative(usage);
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    let font = egui::FontId::proportional(10.0);
    painter.text(egui::pos2(rect.left() + 7.0, rect.top() + 5.0), egui::Align2::LEFT_TOP, format!("C{index}"), font.clone(), skin.muted);
    painter.text(egui::pos2(rect.right() - 7.0, rect.top() + 5.0), egui::Align2::RIGHT_TOP, format!("{usage:.0}%"), font, skin.text);
    let track = egui::Rect::from_min_size(egui::pos2(rect.left() + 7.0, rect.bottom() - 10.0), egui::vec2(rect.width() - 14.0, 4.0));
    painter.rect_filled(track, egui::CornerRadius::same(2), skin.track);
    let frac = (usage / 100.0).clamp(0.0, 1.0);
    if frac > 0.0 {
        let filled = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * frac, track.height()));
        painter.rect_filled(filled, egui::CornerRadius::same(2), skin.accent);
    }
}

/// Componente mais quente da máquina (as zonas variam junto; a média diluiria).
fn hottest(comps: &Components) -> Option<f32> {
    comps.into_iter().filter_map(|component| component.temperature()).reduce(f32::max)
}

/// Card do CPU: cabeçalho com o uso, sparkline do último minuto e os núcleos.
fn cpu_card(ui: &mut egui::Ui, lang: Lang, live: &Live, cores: &[f32], grid: CoreGrid) {
    let skin = Skin::of(ui);
    let cpu = usage_non_negative(live.sys.global_cpu_usage());
    card_head(ui, i18n::t(lang, Key::SysCpu), &format!("· {cpu:.0}%"), skin.accent);
    ui.add_space(6.0);
    sparkline(ui, &live.cpu_hist, 100.0, skin.accent, SPARK_H);
    ui.add_space(10.0);
    ui.label(egui::RichText::new(i18n::t(lang, Key::SysPerCore)).small().color(skin.muted));
    ui.add_space(4.0);
    core_chips(ui, cores, grid);
}

/// Altura EXTERNA do card do CPU, montada com a MESMA grade que desenha os
/// chips (cabeçalho + sparkline + rótulo + linhas de chip). Era a conta
/// divergente do wrap implícito que deixava a última fileira cortada.
fn cpu_card_h(ui: &egui::Ui, inner_w: f32, cores: usize) -> f32 {
    let grid = core_grid(inner_w, cores);
    // O layout vertical cobra o `item_spacing.y` depois de cada WIDGET (o
    // `add_space` anda o cursor exato, sem folga extra): a conta repete a
    // sequência exata de [`cpu_card`].
    let spacing = ui.spacing().item_spacing.y;
    // Linha do cabeçalho: `ui.horizontal` reserva o `interact_size.y` do tema.
    let head = ui.spacing().interact_size.y;
    // Rótulo "por núcleo": `RichText::small()`.
    let small = ui.text_style_height(&egui::TextStyle::Small);
    let rows = grid.rows as f32;
    let inner = (head + spacing)
        + 6.0
        + (SPARK_H + spacing)
        + 10.0
        + (small + spacing)
        + 4.0
        + rows * CHIP_H
        + (grid.rows.saturating_sub(1)) as f32 * CHIP_GAP;
    kit::card_outer_height(inner)
}

/// Card da memória: RAM e swap com rótulo e valor juntos, barra do card.
/// `inner_h` é a altura ÚTIL que a fileira impõe (a do card do CPU, mais alto):
/// os dois medidores se dividem nela em vez de o painel terminar no meio e
/// deixar um vazio embaixo. `0.0` = altura natural (cards empilhados).
fn memory_card(ui: &mut egui::Ui, lang: Lang, live: &Live, inner_h: f32) {
    let skin = Skin::of(ui);
    let total = live.sys.total_memory();
    let used = live.sys.used_memory();
    let frac = if total == 0 { 0.0 } else { used as f32 / total as f32 };
    card_head(ui, i18n::t(lang, Key::SysMemory), &format!("· {:.0}%", frac * 100.0), skin.accent);
    ui.add_space(8.0);
    // Sem arquivo de paginação o swap é 0/0: mostra "0 B / 0 B" (não é erro).
    let total_swap = live.sys.total_swap();
    let used_swap = live.sys.used_swap();
    let swap_frac = if total_swap == 0 { 0.0 } else { used_swap as f32 / total_swap as f32 };
    let ram = |ui: &mut egui::Ui| {
        let value = format!("{} / {}", format_bytes(used), format_bytes(total));
        meter_row(ui, "RAM", &value, frac, skin.accent);
    };
    let swap = |ui: &mut egui::Ui| {
        let value = format!("{} / {}", format_bytes(used_swap), format_bytes(total_swap));
        meter_row(ui, i18n::t(lang, Key::SysSwap), &value, swap_frac, skin.muted);
    };
    // O que sobra depois do cabeçalho vira duas metades iguais: o medidor fica
    // no topo de cada uma (distribuídos, não empilhados no canto de cima). O
    // `item_spacing` entre as duas metades entra na conta, senão o card fica
    // mais alto que o do CPU e a fileira desalinha.
    let consumed = ui.cursor().top() - ui.max_rect().top();
    let rest = (inner_h - consumed).max(0.0);
    let gap = ui.spacing().item_spacing.y;
    if rest > gap {
        let half = (rest - gap) / 2.0;
        let width = ui.available_width();
        ui.allocate_ui_with_layout(egui::vec2(width, half), egui::Layout::top_down(egui::Align::Min), ram);
        ui.allocate_ui_with_layout(egui::vec2(width, half), egui::Layout::top_down(egui::Align::Min), swap);
    } else {
        ram(ui);
        ui.add_space(12.0);
        swap(ui);
    }
}

/// Card dos discos: um bloco por volume (nome · tipo, uso e barra).
fn disks_card(ui: &mut egui::Ui, lang: Lang, live: &Live) {
    let skin = Skin::of(ui);
    let mut any = false;
    card_head(ui, i18n::t(lang, Key::SysDisks), "", skin.text);
    ui.add_space(8.0);
    if let Some(disks) = live.disks.as_ref() {
        for disk in disks {
            let total = disk.total_space();
            if total == 0 {
                continue;
            }
            any = true;
            let used = total.saturating_sub(disk.available_space());
            let kind = disk_kind_label(disk.kind()).unwrap_or_else(|| i18n::t(lang, Key::SysUnknown));
            let name = format!("{} · {kind}", disk.mount_point().display());
            let value = format!("{} / {}", format_bytes(used), format_bytes(total));
            meter_row(ui, &name, &value, used as f32 / total as f32, skin.accent);
            ui.add_space(10.0);
        }
    }
    if !any {
        ui.label(egui::RichText::new(i18n::t(lang, Key::SysUnavailable)).small().color(skin.muted));
    }
}

/// Card da rede: taxa total no cabeçalho, sparkline do download e as
/// interfaces (nome truncado, com o nome inteiro no tooltip).
fn network_card(ui: &mut egui::Ui, lang: Lang, live: &Live) {
    let skin = Skin::of(ui);
    let (down, up) = net_deltas(live.nets.as_ref());
    card_head(ui, i18n::t(lang, Key::SysNetwork), &format!("↓ {} · ↑ {}", format_rate(down as u64), format_rate(up as u64)), skin.accent);
    ui.add_space(6.0);
    sparkline(ui, &live.down_hist, 0.0, skin.accent, SPARK_H_NET);
    ui.add_space(8.0);
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
            adapter_row(ui, name, *down, *up, skin);
        }
    }
}

/// Uma interface: nome truncado (tooltip com o nome inteiro) e os dois badges.
fn adapter_row(ui: &mut egui::Ui, name: &str, down: u64, up: u64, skin: Skin) {
    ui.horizontal(|ui| {
        let name_w = (ui.available_width() - 150.0).max(60.0);
        let label = egui::Label::new(egui::RichText::new(name).small().color(skin.muted)).truncate();
        ui.add_sized(egui::vec2(name_w, 18.0), label).on_hover_text(name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            kit::badge(ui, &format!("↓ {}", format_rate(down)), skin.text);
            ui.add_space(4.0);
            kit::badge(ui, &format!("↑ {}", format_rate(up)), skin.text);
        });
    });
}

/// Card "Energia & sistema": bateria como pill (SEM barra), uptime e
/// temperaturas. As zonas térmicas detalhadas ficam no tooltip.
fn energy_card(ui: &mut egui::Ui, lang: Lang, live: &Live, average: Option<f32>, max: Option<f32>, last: Option<f32>) {
    let skin = Skin::of(ui);
    card_head(ui, i18n::t(lang, Key::SysEnergy), "", skin.text);
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        match battery() {
            Some(info) => {
                let state_key = if info.charging {
                    Key::SysCharging
                } else if info.plugged {
                    Key::SysPlugged
                } else {
                    Key::SysOnBattery
                };
                // Bateria fraca fora da tomada é o único caso de alerta.
                let ink = if info.percent <= 20 && !info.plugged { skin.danger } else { skin.accent };
                kit::badge(ui, &format!("⚡ {}% · {}", info.percent, i18n::t(lang, state_key)), ink);
            }
            None => {
                kit::badge(ui, i18n::t(lang, Key::SysNoBattery), skin.muted);
            }
        }
        ui.add_space(16.0);
        ui.label(egui::RichText::new(i18n::t(lang, Key::SysUptime)).small().color(skin.muted));
        let uptime = System::uptime().map(format_uptime).unwrap_or_else(|_| i18n::t(lang, Key::SysUnavailable).to_string());
        ui.label(egui::RichText::new(uptime).small());
        ui.add_space(16.0);
        ui.label(egui::RichText::new(i18n::t(lang, Key::SysTemperatures)).small().color(skin.muted));
        // Se o WMI parar de responder depois de ter funcionado, a última
        // leitura da sessão continua na tela em vez de "indisponível".
        let current = live.comps.as_ref().and_then(hottest).or(last);
        let text = match current {
            Some(value) => format!("{value:.0} °C"),
            None => i18n::t(lang, Key::SysUnavailable).to_string(),
        };
        let resp = ui.label(egui::RichText::new(text).small());
        let mut detail = String::new();
        if let Some(comps) = live.comps.as_ref() {
            for component in comps {
                if let Some(value) = component.temperature() {
                    detail.push_str(&format!("{} · {value:.0} °C\n", component.label()));
                }
            }
        }
        let average = average.map_or_else(|| "—".to_string(), |value| format!("{value:.0} °C"));
        let maximum = max.map_or_else(|| "—".to_string(), |value| format!("{value:.0} °C"));
        detail.push_str(&format!("{}: {average}   {}: {maximum}", i18n::t(lang, Key::SysAvg), i18n::t(lang, Key::SysMax)));
        resp.on_hover_text(detail);
    });
}

/// Card dos top processos, no padrão da aba Clipboard: nome com tooltip,
/// CPU/RAM como badges, × só no hover e hairline entre as linhas.
fn processes_card(ui: &mut egui::Ui, lang: Lang, live: &Live, sort: SortBy, sort_choice: &mut Option<SortBy>, kill: &mut Option<(u32, String)>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(i18n::t(lang, Key::SysProcesses)).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::segmented(ui, "RAM", sort == SortBy::Mem).clicked() {
                *sort_choice = Some(SortBy::Mem);
            }
            if kit::segmented(ui, "CPU", sort == SortBy::Cpu).clicked() {
                *sort_choice = Some(SortBy::Cpu);
            }
        });
    });
    ui.add_space(6.0);
    kit::hairline(ui);
    for row in &live.procs {
        process_row(ui, lang, row, kill);
        kit::hairline(ui);
    }
}

/// Uma linha da lista de processos.
fn process_row(ui: &mut egui::Ui, lang: Lang, row: &ProcRow, kill: &mut Option<(u32, String)>) {
    let skin = Skin::of(ui);
    let width = ui.available_width().max(1.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, PROC_H), egui::Sense::hover());
    if resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    let inner = rect.shrink2(egui::vec2(8.0, 0.0));
    let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    // Nome truncado; o tooltip devolve o nome inteiro e o PID.
    let name_w = (inner.width() - 190.0).max(80.0);
    let label = egui::Label::new(egui::RichText::new(&row.name).color(skin.text)).truncate();
    row_ui.add_sized(egui::vec2(name_w, 20.0), label).on_hover_text(format!("{} · PID {}", row.name, row.pid));
    row_ui.add_space(6.0);
    row_ui.label(egui::RichText::new(format!("#{}", row.pid)).small().color(skin.muted));
    row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // Ação destrutiva só aparece com o mouse em cima da linha. "×" (U+00D7)
        // e tinta do TEMA: o "✕" do v0.6 não existe nas fontes e o vermelho
        // ficava reservado pro modal de confirmação.
        // Sem hover-dependência: o botão bloqueia o hover da linha e "sumia"
        // sob o próprio ponteiro (mesmo bug pago nos alarmes).
        if kit::ghost_action(ui, "×").on_hover_text(i18n::t(lang, Key::SysKill)).clicked() {
            *kill = Some((row.pid, row.name.clone()));
        }
        kit::badge(ui, &format_bytes(row.mem), skin.muted);
        ui.add_space(4.0);
        let cpu = usage_non_negative(row.cpu);
        let cpu_ink = if cpu >= 50.0 { skin.accent } else { skin.text };
        kit::badge(ui, &format!("{cpu:.0}%"), cpu_ink);
    });
}

/// Sparkline do histórico. `scale` = teto fixo da série (100 no CPU); zero
/// significa escala automática pela maior amostra da janela. `height` é a
/// altura da faixa (cada card escolhe a sua).
fn sparkline(ui: &mut egui::Ui, hist: &Hist, scale: f32, color: egui::Color32, height: f32) {
    // Largura do CARD (o chamador está dentro dele): a série ocupa a linha
    // inteira, nunca um pedaço pequeno no canto.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    if hist.len() < 2 {
        return;
    }
    let scale = if scale > 0.0 { scale } else { hist.max().max(1.0) };
    let painter = ui.painter();
    // Passo pelas amostras EXISTENTES: no começo a janela tem 5 pontos e a
    // linha esticava só o comecinho do card (parecia "mini no canto").
    let step = rect.width() / (hist.len() - 1) as f32;
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

    let mut sort_choice: Option<SortBy> = None;
    let mut kill_request: Option<(u32, String)> = None;
    let sort = state.sort;
    // Acumulado de temperatura copiado para fora: o closure do painel não
    // precisa (e não deve) emprestar `state` enquanto `live` está emprestado.
    let temps_average = state.temps.average();
    let temps_max = state.temps.max;
    let temps_last = state.temps.last;

    // Barra de rolagem DESTA área: a padrão do egui é FLUTUANTE (não reserva
    // espaço) e pinta o thumb com o `fg_stroke` do tema — quase branco no
    // escuro. Resultado: um pino claro boiando POR CIMA dos cards. Sólida, o
    // espaço é reservado no conteúdo e o thumb fica na tinta atenuada do tema.
    let skin = Skin::of(ui);
    let mut scroll = ui.spacing().scroll;
    scroll.floating = false;
    scroll.bar_width = 8.0;
    scroll.bar_inner_margin = 6.0;
    scroll.bar_outer_margin = 2.0;
    scroll.foreground_color = true;
    ui.spacing_mut().scroll = scroll;
    // Repouso na tinta atenuada; hover/pressão sobem pro texto cheio.
    ui.visuals_mut().widgets.inactive.fg_stroke.color = skin.muted;

    egui::ScrollArea::vertical().id_salt("system-scroll").auto_shrink([false, false]).show(ui, |ui| {
        kit::column(ui, CONTENT_W, |ui| {
            let col_w = ui.available_width();
            // O espaçamento do kit separa os cards dentro da linha: descontar as
            // duas folgas evita a grade estourar a coluna.
            let gap = ui.spacing().item_spacing.x;
            // Reflui com a largura: duas colunas só quando cada card ainda tem
            // largura legível; abaixo disso empilha (janela pequena mostra
            // menos card, nunca um card cortado).
            let two_cols = col_w >= TWO_COL_MIN_W;
            let card_w = if two_cols { ((col_w - GUTTER - 2.0 * gap) / 2.0).floor() } else { col_w };

            // Grade 2×N: CPU | Memória · Discos | Rede. A altura da primeira
            // linha vem da grade de núcleos (a mesma que desenha os chips) e as
            // duas células a compartilham — a memória distribui o que sobra.
            let cores: Vec<f32> = live.sys.cpus().iter().map(|core| usage_non_negative(core.usage())).collect();
            let inner_w = kit::card_inner_width(card_w);
            let grid = core_grid(inner_w, cores.len());
            let cpu_h = cpu_card_h(ui, inner_w, cores.len());
            let mem_inner_h = if two_cols { kit::card_inner_height(cpu_h) } else { 0.0 };

            card_pair(ui, two_cols, card_w, cpu_h, |ui| cpu_card(ui, lang, live, &cores, grid), |ui| memory_card(ui, lang, live, mem_inner_h));
            ui.add_space(GUTTER);
            card_pair(ui, two_cols, card_w, 210.0, |ui| disks_card(ui, lang, live), |ui| network_card(ui, lang, live));
            ui.add_space(GUTTER);
            kit::card(ui, col_w, |ui| energy_card(ui, lang, live, temps_average, temps_max, temps_last));
            ui.add_space(GUTTER);
            kit::card(ui, col_w, |ui| processes_card(ui, lang, live, sort, &mut sort_choice, &mut kill_request));
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
    fn grade_de_nucleos_preenche_o_card_sem_estourar() {
        // Larguras reais de card (empilhado e duas colunas) × contagens típicas.
        for inner_w in [120.0_f32, 240.0, 408.0, 558.0, 770.0] {
            for cores in [1_usize, 4, 8, 12, 16, 24, 32] {
                let grid = core_grid(inner_w, cores);
                assert!(grid.cols >= 1, "sempre cabe pelo menos um chip");
                assert_eq!(grid.rows, cores.div_ceil(grid.cols).max(1));
                let used = grid.cols as f32 * grid.chip_w + (grid.cols - 1) as f32 * CHIP_GAP;
                assert!(used <= inner_w + 0.01, "a linha estoura o card ({used} > {inner_w})");
                assert!(grid.chip_w >= CHIP_W - 0.01, "o chip não encolhe abaixo da largura-base");
            }
        }
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
