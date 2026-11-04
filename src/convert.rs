//! Aba Converter: converte imagens entre PNG/JPEG/WebP numa fila com progresso.
//!
//! A UX é UMA zona de soltar (o `egui` entrega os arquivos no `raw_input` da
//! janela) + o botão nativo de seleção múltipla (só Windows: o `rfd` é
//! `cfg(windows)` no `Cargo.toml` — no Linux a fila entra pelo drop, que é
//! cross-platform). Cada arquivo vira uma linha com tipo DETECTADO PELO HEADER
//! (a extensão mente: renomear `.png` pra `.jpg` é comum) e um destino.
//!
//! A conversão roda numa thread própria com canal de progresso (mesmo desenho
//! da OCR): decodifica, re-encoda e devolve um evento por arquivo. O arquivo de
//! saída vai pra MESMA pasta do original com a extensão do destino; colisão
//! ganha o sufixo ` (1)`, ` (2)`… — nada é sobrescrito.
//!
//! **Metadado nunca sobrevive**: o re-encode parte só dos pixels. EXIF/XMP/ICC
//! do original ficam para trás de propósito (privacidade — mesmo princípio do
//! local-image, que também não propaga metadado ao re-salvar).
//!
//! **JXL só como ORIGEM**: não existe encoder JPEG XL maduro em Rust (o
//! `jxl-oxide` é decodificador). Por isso o destino não oferece JXL — e a aba
//! diz isso na tela.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use eframe::egui;
use image::{DynamicImage, ExtendedColorType, ImageEncoder};
use utils_ui::i18n::{self, Key, Lang};

use crate::ui::{self as kit, Skin};

/// Extensões oferecidas no seletor nativo (o tipo de verdade é o header).
/// Windows only: é a lista do `rfd`, que só existe lá.
#[cfg(windows)]
const PICK_FILTER: &[&str] = &["png", "jpg", "jpeg", "webp", "jxl"];
/// Qualidade padrão do JPEG (0–100).
const DEFAULT_QUALITY: u8 = 90;
/// Quantos caracteres do nome cabem na linha (o resto vira "…").
const NAME_CHARS: usize = 34;
/// Teto do sufixo de colisão (`foto (9999).png`): depois disso o arquivo é
/// recusado em vez de girar pra sempre.
const MAX_SUFFIX: u32 = 9_999;

// ── formatos ─────────────────────────────────────────────────────────────

/// Formato que o app sabe DECODIFICAR.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Png,
    Jpeg,
    Webp,
    Jxl,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Png => "PNG",
            Source::Jpeg => "JPEG",
            Source::Webp => "WebP",
            Source::Jxl => "JXL",
        }
    }
}

/// Formato de SAÍDA. JXL não está aqui de propósito (não existe encoder maduro
/// em Rust; ver o doc do módulo).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Png,
    Jpeg,
    Webp,
}

impl Target {
    const ALL: [Target; 3] = [Target::Png, Target::Jpeg, Target::Webp];

    fn label(self) -> &'static str {
        match self {
            Target::Png => "PNG",
            Target::Jpeg => "JPEG",
            Target::Webp => "WebP",
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Target::Png => "png",
            Target::Jpeg => "jpg",
            Target::Webp => "webp",
        }
    }
}

// ── fila e worker ────────────────────────────────────────────────────────

/// Estado de uma linha da fila.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Status {
    Waiting,
    /// Não é imagem suportada: fica na linha com aviso e sem alvo (v0.5 só
    /// converte imagem).
    Unsupported,
    Done {
        name: String,
    },
    Failed {
        error: String,
    },
}

/// Uma linha da fila.
struct Item {
    /// Correlaciona o evento do worker com a linha (e é o salt do combo).
    id: u64,
    path: PathBuf,
    /// Nome curto já elidido (a linha não precisa do caminho inteiro).
    label: String,
    /// `None` = arquivo que não é imagem suportada.
    src: Option<Source>,
    target: Target,
    status: Status,
}

/// Progresso vindo da thread de conversão.
enum Event {
    Done {
        id: u64,
        name: String,
    },
    Failed {
        id: u64,
        error: String,
    },
    /// A fila acabou (a thread sai logo depois).
    Finished,
}

/// O que a thread precisa saber de cada arquivo (foto do estado no clique).
struct Job {
    id: u64,
    path: PathBuf,
    src: Source,
    target: Target,
}

/// Estado da aba Converter entre frames.
pub struct ConvertState {
    queue: Vec<Item>,
    /// Qualidade do JPEG (global: uma linha só pra todos, mais simples).
    quality: u8,
    /// Canal da conversão em andamento (`None` = parada).
    rx: Option<Receiver<Event>>,
    next_id: u64,
}

impl Default for ConvertState {
    fn default() -> Self {
        Self { queue: Vec::new(), quality: DEFAULT_QUALITY, rx: None, next_id: 1 }
    }
}

/// O que a aba devolve pro app.
#[derive(Default)]
pub struct Outcome {
    pub status: Option<String>,
}

impl ConvertState {
    /// Tem conversão rodando? O app pede repaint curto enquanto isso (o canal
    /// não acorda a UI sozinho, mesmo motivo da OCR).
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    /// Drena o progresso do worker. Chamado no topo do frame pelo app, como o
    /// `drain_ocr`: a aba pode estar fechada e a fila continua andando.
    pub fn poll(&mut self) {
        loop {
            // Empréstimo separado do `self` mutável do `set_status`.
            let Some(Ok(event)) = self.rx.as_ref().map(|rx| rx.try_recv()) else { return };
            match event {
                Event::Done { id, name } => self.set_status(id, Status::Done { name }),
                Event::Failed { id, error } => self.set_status(id, Status::Failed { error }),
                Event::Finished => {
                    self.rx = None;
                    return;
                }
            }
        }
    }

    fn set_status(&mut self, id: u64, status: Status) {
        if let Some(item) = self.queue.iter_mut().find(|item| item.id == id) {
            item.status = status;
        }
    }

    /// Quantos arquivos têm conversão possível (os não-imagem não contam).
    fn total(&self) -> usize {
        self.queue.iter().filter(|item| item.src.is_some()).count()
    }

    /// Quantos dos conversíveis já terminaram (com sucesso ou com erro).
    fn finished(&self) -> usize {
        self.queue.iter().filter(|item| item.src.is_some() && matches!(item.status, Status::Done { .. } | Status::Failed { .. })).count()
    }

    /// Quantos ainda esperam conversão.
    fn waiting(&self) -> usize {
        self.queue.iter().filter(|item| matches!(item.status, Status::Waiting)).count()
    }

    /// Põe os arquivos na fila: lê o header AGORA (é barato) pra saber o tipo e
    /// se a linha tem alvo. Arquivo ilegível/não-imagem entra com aviso.
    fn enqueue(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            let src = detect_path(&path);
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            let label = elide(&name, NAME_CHARS);
            let status = if src.is_some() { Status::Waiting } else { Status::Unsupported };
            self.queue.push(Item { id: self.next_id, path, label, src, target: Target::Png, status });
            self.next_id = self.next_id.wrapping_add(1);
        }
    }

    /// Sobe a thread da conversão com a fila atual (foto do estado).
    fn start(&mut self) -> Result<(), String> {
        let jobs: Vec<Job> = self
            .queue
            .iter()
            .filter(|item| matches!(item.status, Status::Waiting))
            .filter_map(|item| item.src.map(|src| Job { id: item.id, path: item.path.clone(), src, target: item.target }))
            .collect();
        if jobs.is_empty() {
            return Ok(());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let quality = self.quality;
        let handle = std::thread::Builder::new().name("utils-convert".into()).spawn(move || {
            for job in jobs {
                let event = match convert_one(&job, quality) {
                    Ok(name) => Event::Done { id: job.id, name },
                    Err(error) => Event::Failed { id: job.id, error },
                };
                // Receptor morto = app fechando.
                if tx.send(event).is_err() {
                    return;
                }
            }
            let _ = tx.send(Event::Finished);
        });
        match handle {
            Ok(_) => {
                self.rx = Some(rx);
                Ok(())
            }
            Err(e) => Err(format!("iniciar a thread da conversão: {e}")),
        }
    }
}

// ── aba ──────────────────────────────────────────────────────────────────

/// Desenha a aba Converter.
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut ConvertState) -> Outcome {
    let mut out = Outcome::default();
    let skin = Skin::of(ui);

    // O drop do egui é da JANELA inteira, não do retângulo da zona: soltar em
    // qualquer lugar da aba alimenta a fila (o desenho tracejado só comunica).
    let dropped = take_dropped(ui.ctx());
    if !dropped.is_empty() {
        state.enqueue(dropped);
    }

    drop_zone(ui, lang);
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        // rfd é `cfg(windows)` no Cargo.toml: no Linux a fila entra pelo drop.
        #[cfg(windows)]
        {
            if kit::pill(ui, i18n::t(lang, Key::SelectFiles), true).clicked() {
                if let Some(paths) = pick_files(lang) {
                    state.enqueue(paths);
                }
            }
        }
        ui.add_space(8.0);
        ui.label(i18n::t(lang, Key::Quality));
        let mut quality = i32::from(state.quality);
        if ui.add(egui::Slider::new(&mut quality, 1..=100).show_value(true)).on_hover_text(i18n::t(lang, Key::QualityHint)).changed() {
            state.quality = quality.clamp(1, 100) as u8;
        }
        ui.add_space(8.0);
        // Converter: rótulo da aba e do botão são a MESMA palavra no catálogo.
        let enabled = !state.running() && state.waiting() > 0;
        ui.add_enabled_ui(enabled, |ui| {
            if kit::pill(ui, i18n::t(lang, Key::TabConvert), true).clicked() {
                if let Err(e) = state.start() {
                    out.status = Some(format!("⚠ {e}"));
                }
            }
        });
    });

    // JXL só como origem (não há encoder): o aviso fica junto da fila, não
    // escondido num tooltip.
    ui.add_space(4.0);
    ui.label(egui::RichText::new(i18n::t(lang, Key::JxlInputOnly)).small().weak());

    let total = state.total();
    if total > 0 {
        let done = state.finished();
        ui.add_space(6.0);
        ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_width(ui.available_width()).text(format!("{done} / {total}")));
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // Barra de ações da lista: limpar o que já terminou.
    ui.horizontal(|ui| {
        let done = state.finished();
        ui.add_enabled_ui(done > 0, |ui| {
            if kit::ghost(ui, i18n::t(lang, Key::ClearDone)).clicked() {
                state.queue.retain(|item| item.status == Status::Waiting);
            }
        });
    });

    let mut remove: Option<usize> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (index, item) in state.queue.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized([260.0, 22.0], egui::Label::new(egui::RichText::new(item.label.as_str()).color(skin.text)).truncate())
                    .on_hover_text(item.path.display().to_string());
                let kind = item.src.map_or_else(|| i18n::t(lang, Key::UnsupportedType).to_string(), |src| src.label().to_string());
                ui.add_sized([64.0, 22.0], egui::Label::new(egui::RichText::new(kind).small().weak()).truncate());

                if item.src.is_some() {
                    egui::ComboBox::from_id_salt(("convert-target", item.id)).selected_text(item.target.label()).width(78.0).show_ui(ui, |ui| {
                        for target in Target::ALL {
                            ui.selectable_value(&mut item.target, target, target.label());
                        }
                    });
                } else {
                    ui.add_space(78.0);
                }

                let status = match &item.status {
                    Status::Waiting => (format!("⏳ {}", i18n::t(lang, Key::ConvertWaiting)), skin.muted),
                    Status::Unsupported => (format!("⚠ {}", i18n::t(lang, Key::UnsupportedType)), skin.danger),
                    Status::Done { name } => (format!("✓ {}", i18n::t(lang, Key::ConvertedTo).replace("{name}", name)), skin.text),
                    Status::Failed { error } => (format!("⚠ {}: {error}", i18n::t(lang, Key::ConvertFailed)), skin.danger),
                };
                ui.add_sized([360.0, 22.0], egui::Label::new(egui::RichText::new(status.0.as_str()).small().color(status.1)).truncate())
                    .on_hover_text(status.0.clone());

                if kit::ghost(ui, "✕").on_hover_text(i18n::t(lang, Key::Remove)).clicked() {
                    remove = Some(index);
                }
            });
        }
    });
    // Remoção fora do laço: mexer na fila durante o `iter_mut` não compila.
    if let Some(index) = remove {
        state.queue.remove(index);
    }

    out
}

/// Zona de soltar: grade tracejada fraca com a dica. Fica em accent quando há
/// arquivo pairando sobre a janela.
fn drop_zone(ui: &mut egui::Ui, lang: Lang) {
    let skin = Skin::of(ui);
    let hovered = ui.input(|i| !i.raw.hovered_files.is_empty());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 92.0), egui::Sense::hover());
    if hovered {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), skin.hover);
    }
    let stroke = egui::Stroke::new(1.0_f32, if hovered { skin.accent } else { skin.border });
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    let mut shapes = Vec::new();
    for pair in corners.windows(2) {
        shapes.extend(egui::Shape::dashed_line(pair, stroke, 7.0, 5.0));
    }
    ui.painter().extend(shapes);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, i18n::t(lang, Key::DropImagesHint), egui::FontId::proportional(15.0), skin.muted);
}

/// Arquivos largados na janela neste frame (o `egui` não limpa sozinho porque
/// cada frame traz um `RawInput` novo).
fn take_dropped(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|i| i.raw.dropped_files.iter().map(|file| file.path().to_path_buf()).collect())
}

/// Seletor nativo de múltiplos arquivos (só Windows: o `rfd` é `cfg(windows)`).
#[cfg(windows)]
fn pick_files(lang: Lang) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new().add_filter(i18n::t(lang, Key::FilterImages), PICK_FILTER).pick_files()
}

// ── detecção, nomes e conversão ──────────────────────────────────────────

/// Tipo pelo HEADER (magic numbers), não pela extensão.
pub(crate) fn detect(bytes: &[u8]) -> Option<Source> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Source::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Source::Jpeg);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        return Some(Source::Webp);
    }
    // JXL: codestream puro (`FF 0A`) ou container ISO-BMFF (a caixa "JXL "
    // com a assinatura `0D 0A 87 0A`).
    if bytes.starts_with(&[0xFF, 0x0A]) || bytes.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A]) {
        return Some(Source::Jxl);
    }
    None
}

/// Lê o começo do arquivo e detecta o tipo pelo HEADER; a extensão é só plano B
/// (arquivo travado por outro processo, header ilegível). Header ilegível E
/// extensão desconhecida = linha com aviso, nunca some sem explicação.
fn detect_path(path: &Path) -> Option<Source> {
    use std::io::Read as _;
    let mut head = [0u8; 16];
    let mut filled = 0;
    if let Ok(mut file) = std::fs::File::open(path) {
        while filled < head.len() {
            match file.read(&mut head[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => break,
            }
        }
    }
    detect(&head[..filled]).or_else(|| from_extension(path))
}

/// Tipo pela extensão (plano B do [`detect_path`]).
pub(crate) fn from_extension(path: &Path) -> Option<Source> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some(Source::Png),
        "jpg" | "jpeg" => Some(Source::Jpeg),
        "webp" => Some(Source::Webp),
        "jxl" => Some(Source::Jxl),
        _ => None,
    }
}

/// Corta o nome no limite de caracteres, com "…" no fim (caracteres, não
/// bytes: nome acentuado não pode virar meio caractere).
pub(crate) fn elide(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut out: String = name.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Primeiro nome livre: `foto.png`, `foto (1).png`, `foto (2).png`… `exists`
/// é injetado pra a sequência ser testável sem tocar no disco.
pub(crate) fn free_name(stem: &str, ext: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let first = format!("{stem}.{ext}");
    if !exists(&first) {
        return Some(first);
    }
    for suffix in 1..=MAX_SUFFIX {
        let candidate = format!("{stem} ({suffix}).{ext}");
        if !exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Caminho de saída: mesma pasta do original, extensão do destino, sem
/// sobrescrever nada.
fn output_path(src: &Path, target: Target) -> Result<PathBuf, String> {
    let dir = src.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = src.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| "imagem".to_string());
    let ext = target.ext();
    free_name(&stem, ext, |name| dir.join(name).exists()).map(|name| dir.join(name)).ok_or_else(|| format!("sem nome livre para {stem}.{ext}"))
}

/// Decodifica o original. JXL passa pelo `jxl-oxide` (o `image` não tem JXL);
/// o resto é sniffado pelo conteúdo, não pela extensão.
fn decode(path: &Path, source: Source) -> Result<DynamicImage, String> {
    match source {
        // O decoder do jxl-oxide implementa `image::ImageDecoder`: o
        // `DynamicImage` resolve orientação, profundidade de bits e canais.
        Source::Jxl => {
            let file = std::fs::File::open(path).map_err(|e| format!("abrir {}: {e}", path.display()))?;
            let decoder = jxl_oxide::integration::JxlDecoder::new(std::io::BufReader::new(file)).map_err(|e| format!("ler o JXL: {e}"))?;
            DynamicImage::from_decoder(decoder).map_err(|e| format!("decodificar o JXL: {e}"))
        }
        _ => {
            let bytes = std::fs::read(path).map_err(|e| format!("ler {}: {e}", path.display()))?;
            image::load_from_memory(&bytes).map_err(|e| format!("decodificar a imagem: {e}"))
        }
    }
}

/// Re-encoda SÓ os pixels (metadado do original fica para trás de propósito —
/// privacidade). A saída é sempre 8 bits por canal.
fn encode(image: &DynamicImage, path: &Path, target: Target, quality: u8) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("criar {}: {e}", path.display()))?;
    let writer = std::io::BufWriter::new(file);
    let (width, height) = (image.width(), image.height());
    match target {
        Target::Png => {
            let rgba = image.to_rgba8();
            image::codecs::png::PngEncoder::new(writer)
                .write_image(rgba.as_raw(), width, height, ExtendedColorType::Rgba8)
                .map_err(|e| format!("gravar o PNG: {e}"))
        }
        Target::Jpeg => {
            // JPEG não tem canal alfa: o alfa é descartado na conversão pra RGB.
            let rgb = image.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(writer, quality)
                .write_image(rgb.as_raw(), width, height, ExtendedColorType::Rgb8)
                .map_err(|e| format!("gravar o JPEG: {e}"))
        }
        Target::Webp => {
            let rgba = image.to_rgba8();
            // O encoder do `image` é VP8L (lossless): a qualidade do slider não
            // se aplica ao WebP — está dito no tooltip do slider.
            image::codecs::webp::WebPEncoder::new_lossless(writer)
                .write_image(rgba.as_raw(), width, height, ExtendedColorType::Rgba8)
                .map_err(|e| format!("gravar o WebP: {e}"))
        }
    }
}

/// Um arquivo, do começo ao fim (roda na thread do worker).
fn convert_one(job: &Job, quality: u8) -> Result<String, String> {
    let image = decode(&job.path, job.src)?;
    let out = output_path(&job.path, job.target)?;
    encode(&image, &out, job.target, quality)?;
    Ok(out.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| out.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// Pasta descartável por teste (same pid + nome do arquivo).
    fn temp_path(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("local-utils-convert-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(name);
        (dir, path)
    }

    #[test]
    fn tipo_pelo_header_ignora_a_extensao() {
        assert_eq!(detect(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00]), Some(Source::Png));
        assert_eq!(detect(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Source::Jpeg));
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, b'W', b'E', b'B', b'P']);
        assert_eq!(detect(&webp), Some(Source::Webp));
        assert_eq!(detect(&[0xFF, 0x0A, 0x00]), Some(Source::Jxl), "codestream puro");
        assert_eq!(detect(&[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A]), Some(Source::Jxl), "container ISO-BMFF");
        // GIF, texto e arquivo vazio não são imagem suportada.
        assert_eq!(detect(b"GIF89a"), None);
        assert_eq!(detect(b"hello world"), None);
        assert_eq!(detect(&[]), None);
    }

    #[test]
    fn extensao_serve_de_plano_b() {
        assert_eq!(from_extension(Path::new("a/B.JPEG")), Some(Source::Jpeg), "case-insensitive");
        assert_eq!(from_extension(Path::new("foto.webp")), Some(Source::Webp));
        assert_eq!(from_extension(Path::new("nota.txt")), None);
        assert_eq!(from_extension(Path::new("sem_extensao")), None);
    }

    #[test]
    fn header_manda_e_extensao_e_so_plano_b() {
        // Extensão mente, header fala: um JPEG salvo como .png é JPEG.
        let (dir, path) = temp_path("mentira.png");
        std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xE0]).ok();
        assert_eq!(detect_path(&path), Some(Source::Jpeg));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nome_elidido_conta_caracteres() {
        assert_eq!(elide("foto.png", 20), "foto.png");
        assert_eq!(elide("uma-foto-com-nome-longo.png", 12), "uma-foto-co…");
        // Conta CARACTERES, não bytes: acento não vira meio caractere.
        assert_eq!(elide("ação", 4), "ação");
        assert_eq!(elide("ação", 3), "aç…");
        assert_eq!(elide("abc", 1), "…");
    }

    #[test]
    fn colisao_gera_sufixo_em_sequencia() {
        // Ocupados: `foto.png` e `foto (1).png` → o próximo livre é `(2)`.
        let taken = ["foto.png", "foto (1).png"];
        let exists = |name: &str| taken.contains(&name);
        assert_eq!(free_name("foto", "png", exists).as_deref(), Some("foto (2).png"));
        // Pasta limpa usa o nome cru.
        assert_eq!(free_name("foto", "png", |_| false).as_deref(), Some("foto.png"));
        // Nada livre até o teto = desiste (erro tratado na linha).
        assert_eq!(free_name("foto", "png", |_| true), None);
    }

    #[test]
    fn png_ida_e_volta_exata() {
        let img = RgbaImage::from_fn(4, 4, |x, y| Rgba([(x * 60) as u8, (y * 60) as u8, 7, 255]));
        let (dir, path) = temp_path("rt.png");
        encode(&DynamicImage::ImageRgba8(img.clone()), &path, Target::Png, 90).expect("gravar PNG");
        let back = image::open(&path).expect("reabrir PNG").to_rgba8();
        assert_eq!(back.dimensions(), (4, 4));
        assert_eq!(back.get_pixel(1, 2), img.get_pixel(1, 2), "PNG é sem perda");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn webp_lossless_preserva_os_pixels() {
        let img = RgbaImage::from_fn(4, 4, |x, y| Rgba([255 - (x * 30) as u8, (y * 40) as u8, 90, 200]));
        let (dir, path) = temp_path("rt.webp");
        encode(&DynamicImage::ImageRgba8(img.clone()), &path, Target::Webp, 90).expect("gravar WebP");
        let back = image::open(&path).expect("reabrir WebP").to_rgba8();
        assert_eq!(back.get_pixel(2, 1), img.get_pixel(2, 1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jpeg_encoda_e_decodifica_de_novo() {
        let img = RgbaImage::from_fn(8, 8, |x, _| Rgba([(x * 30) as u8, 10, 200, 255]));
        let (dir, path) = temp_path("rt.jpg");
        encode(&DynamicImage::ImageRgba8(img), &path, Target::Jpeg, DEFAULT_QUALITY).expect("gravar JPEG");
        let back = image::open(&path).expect("reabrir JPEG");
        assert_eq!((back.width(), back.height()), (8, 8));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saida_fica_na_pasta_do_original_com_outra_extensao() {
        let (dir, path) = temp_path("entrada.png");
        std::fs::write(&path, [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).ok();
        let out = output_path(&path, Target::Jpeg).expect("caminho de saída");
        assert_eq!(out.parent(), Some(dir.as_path()));
        assert_eq!(out.file_name().and_then(|n| n.to_str()), Some("entrada.jpg"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
