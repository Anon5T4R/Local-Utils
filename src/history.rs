//! Motor do histórico de clipboard — **puro**: nenhuma chamada de SO entra
//! aqui, então tudo é testável sem Windows e sem abrir a área de transferência.
//!
//! O fluxo é: `clipboard.rs` lê a cópia e monta uma [`Entry`] → `History::push`
//! (dedupe por conteúdo + retenção) → `save` no JSON versionado. A UI (lote 3)
//! só consome `iter()`/`latest()`.
//!
//! Persistência: JSON `{"version":1,"entries":[...]}` gravado com tmp + rename
//! (mesmo molde do `vault.rs` do Local-Keys) — uma gravação interrompida não
//! corrompe o histórico. Arquivo corrompido vira histórico NOVO e **fica no
//! disco** pra diagnóstico; nunca apagamos o que não conseguimos ler.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use image::ImageEncoder;
use serde::{Deserialize, Serialize};

/// Retenção padrão: 500 itens…
pub const MAX_ITEMS_DEFAULT: usize = 500;
/// …e no máximo 256 MiB de imagens (texto e lista de arquivos não contam).
pub const MAX_BYTES_DEFAULT: u64 = 256 * 1024 * 1024;
/// Lado máximo da miniatura (quadrada na maior dimensão) usada pela lista.
pub const THUMB_MAX: u32 = 256;

/// Versão do arquivo salvo. Arquivo de versão desconhecida é ignorado (vira
/// histórico vazio, arquivo preservado) até existir migração.
const SAVE_VERSION: u32 = 1;

/// Que tipo de conteúdo a entrada carrega. Decide como recopiar e como a lista
/// desenha; `Html`/`Rtf` guardam o conteúdo rico cru em `Entry::text`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Kind {
    /// Texto puro (CF_UNICODETEXT).
    #[default]
    Text,
    /// CF_HTML — fragmento HTML, do jeito que veio da origem.
    Html,
    /// "Rich Text Format" — RTF cru.
    Rtf,
    /// CF_BITMAP já convertido pra PNG (`image_png`) + miniatura.
    Image,
    /// CF_HDROP — caminhos copiados do Explorer.
    Files,
}

/// Uma cópia. `id` é estável (persistido) pra UI referenciar sem depender da
/// posição na lista; `ts_ms` é epoch em milissegundos.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub id: u128,
    pub ts_ms: u64,
    pub kind: Kind,
    /// Fixada pelo usuário: a retenção automática nunca remove.
    pub pinned: bool,
    /// Texto da cópia (ou o conteúdo rico cru, em `Html`/`Rtf`).
    pub text: Option<String>,
    /// Caminhos da cópia (`Files`).
    pub files: Option<Vec<String>>,
    /// PNG da imagem original (`Image`).
    pub image_png: Option<Vec<u8>>,
    /// PNG da miniatura (`Image`) — o que a lista mostra sem decodificar o
    /// original; entra na conta de bytes junto com `image_png`.
    pub thumb_png: Option<Vec<u8>>,
}

/// Bytes que a entrada ocupa na cota: só imagem conta. Texto e caminhos são
/// baratos e limitados naturalmente pela cota de itens.
fn entry_bytes(e: &Entry) -> u64 {
    let png = e.image_png.as_ref().map_or(0, |b| b.len() as u64);
    let thumb = e.thumb_png.as_ref().map_or(0, |b| b.len() as u64);
    png.saturating_add(thumb)
}

/// Discriminante barato do conteúdo, usado antes do hash: só vale hashear
/// entradas com a mesma cara (kind + tamanhos). Sem isso toda cópia de texto
/// re-hashearia todas as imagens guardadas.
fn content_signature(e: &Entry) -> (u8, usize, usize, usize) {
    let text = e.text.as_ref().map_or(0, String::len);
    let files = e.files.as_ref().map_or(0, Vec::len);
    let image = e.image_png.as_ref().map_or(0, Vec::len);
    (e.kind as u8, text, files, image)
}

/// Hash do CONTEÚDO (kind + texto/caminhos/bytes). `pinned` e `ts_ms` de fora:
/// a mesma cópia feita de novo é a MESMA entrada, não uma nova.
fn content_key(e: &Entry) -> u64 {
    let mut h = DefaultHasher::new();
    (e.kind as u8).hash(&mut h);
    e.text.hash(&mut h);
    e.files.hash(&mut h);
    e.image_png.hash(&mut h);
    h.finish()
}

/// Histórico em memória. As entradas ficam da mais recente (índice 0) pra mais
/// antiga — a retenção corta sempre pelo fim.
#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<Entry>,
    max_items: usize,
    max_bytes: u64,
    /// Cache da soma de `entry_bytes` — mantido por todos os mutadores pra
    /// retenção não varrer a lista a cada cópia.
    cur_bytes: u64,
}

impl Default for History {
    fn default() -> Self {
        Self { entries: Vec::new(), max_items: MAX_ITEMS_DEFAULT, max_bytes: MAX_BYTES_DEFAULT, cur_bytes: 0 }
    }
}

// Grande parte desta API (iter/latest/toggle_pin/…) só ganha chamador na UI
// do lote 3 — o motor veio primeiro (testado) e a lista vem depois.
#[allow(dead_code)]
impl History {
    /// Histórico vazio com cotas próprias (testes e, no futuro, opções do app).
    pub fn new(max_items: usize, max_bytes: u64) -> Self {
        Self { entries: Vec::new(), max_items, max_bytes, cur_bytes: 0 }
    }

    /// Insere no topo. Dedupe por conteúdo: se a mesma cópia já existe em
    /// qualquer posição (pinada inclusive), a entrada antiga sobe pro topo e
    /// ganha o carimbo novo — o `id` e o `pinned` originais ficam.
    pub fn push(&mut self, entry: Entry) {
        let sig = content_signature(&entry);
        let key = content_key(&entry);
        if let Some(pos) = self.entries.iter().position(|e| content_signature(e) == sig && content_key(e) == key) {
            let mut existing = self.entries.remove(pos);
            existing.ts_ms = entry.ts_ms;
            self.entries.insert(0, existing);
            return;
        }
        self.cur_bytes = self.cur_bytes.saturating_add(entry_bytes(&entry));
        self.entries.insert(0, entry);
        self.enforce_retention();
    }

    /// Corta do FIM (mais antigas) até caber em `max_items` e `max_bytes`.
    /// Entradas pinadas contam pra cota mas nunca são removidas — se só
    /// sobraram pinadas, a lista pode ficar acima do limite (decisão do
    /// usuário vence a retenção).
    pub fn enforce_retention(&mut self) {
        loop {
            let over_items = self.entries.len() > self.max_items;
            let over_bytes = self.cur_bytes > self.max_bytes;
            if !over_items && !over_bytes {
                break;
            }
            // A cópia mais recente (índice 0) NUNCA é cortada: com a cota
            // cheia de pinadas, cortar a nova faria a cópia de agora "sumir
            // na hora" — a lista fica acima do limite, decisão do usuário
            // vence a retenção (pagou teste: `retencao_respeita_pinadas`).
            let Some(pos) = self.entries.iter().skip(1).rposition(|e| !e.pinned).map(|p| p + 1) else { break };
            let removed = self.entries.remove(pos);
            self.cur_bytes = self.cur_bytes.saturating_sub(entry_bytes(&removed));
        }
    }

    /// Fixa/desafixa. Pinada não é removida pela retenção; nada mais muda.
    pub fn toggle_pin(&mut self, id: u128) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.pinned = !entry.pinned;
        }
    }

    /// Remove a entrada (a UI do lote 3 chama no botão de lixeira).
    #[allow(dead_code)]
    pub fn remove(&mut self, id: u128) {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            let removed = self.entries.remove(pos);
            self.cur_bytes = self.cur_bytes.saturating_sub(entry_bytes(&removed));
        }
    }

    /// Da mais recente pra mais antiga.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// A cópia mais recente (atalho da UI/recopiar).
    pub fn latest(&self) -> Option<&Entry> {
        self.entries.first()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bytes de imagem guardados (a cota que a retenção persegue).
    pub fn total_bytes(&self) -> u64 {
        self.cur_bytes
    }
}

/// Id único sem depender de RNG: nanos desde a época nos bits altos + contador
/// do processo nos baixos (duas cópias no mesmo nanossegundo não colidem).
pub fn new_id() -> u128 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let seq = u128::from(COUNTER.fetch_add(1, Ordering::Relaxed));
    (nanos << 16) | (seq & 0xFFFF)
}

/// Carimbo em milissegundos desde a época (0 se o relógio estiver fora da
/// faixa — entrada sem data é melhor que entrada nenhuma).
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|d| u64::try_from(d.as_millis()).ok()).unwrap_or(0)
}

/// Encoda RGBA cru em PNG. `None` se o buffer não casar com as dimensões.
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(rgba, width, height, image::ExtendedColorType::Rgba8).ok()?;
    Some(png)
}

/// Miniatura PNG com a maior dimensão em `max` (nunca aumenta a imagem).
/// Downsampling da própria `image` (box filter do `thumbnail`) — rápido e
/// suficiente pra lista; a imagem cheia fica em `image_png`.
pub fn make_thumb(rgba: &[u8], width: u32, height: u32, max: u32) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let (tw, th) = thumb_dims(width, height, max.max(1));
    let thumb = image::imageops::thumbnail(&img, tw, th);
    encode_png(thumb.as_raw(), tw, th)
}

/// Proporção da miniatura, arredondada pra cima e nunca abaixo de 1px.
fn thumb_dims(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (max, max);
    }
    if width <= max && height <= max {
        return (width, height);
    }
    let scale = f64::from(max) / f64::from(width.max(height));
    let tw = (f64::from(width) * scale).round().max(1.0) as u32;
    let th = (f64::from(height) * scale).round().max(1.0) as u32;
    (tw, th)
}

/// Formato do arquivo em disco. `version` PRIMEIRO pra um arquivo de versão
/// futura poder ser reconhecido sem tentar interpretar as entradas.
#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    entries: Vec<Entry>,
}

/// Grava o histórico com tmp + rename (atômico em Windows e Unix). O arquivo
/// anterior é substituído só quando o novo está inteiro no disco.
pub fn save(path: &Path, h: &History) -> std::io::Result<()> {
    let file = SaveFile { version: SAVE_VERSION, entries: h.entries.clone() };
    let bytes = serde_json::to_vec(&file)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Lê o histórico. Ausente, ilegível, corrompido ou de versão desconhecida →
/// histórico NOVO (vazio); o arquivo fica intacto pra diagnóstico.
pub fn load(path: &Path) -> History {
    let Ok(bytes) = std::fs::read(path) else { return History::default() };
    let Ok(file) = serde_json::from_slice::<SaveFile>(&bytes) else { return History::default() };
    if file.version != SAVE_VERSION {
        return History::default();
    }
    let mut h = History { entries: file.entries, ..History::default() };
    // Cota recalculada do arquivo (não confia em cache que não foi salvo) e
    // retenção aplicada: cotas podem ter mudado desde a última gravação.
    h.cur_bytes = h.entries.iter().map(entry_bytes).sum();
    h.enforce_retention();
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Nota: nada aqui mexe em estado global, então os testes rodam em
    // paralelo sem mutex — a regra da suíte (travar o módulo) só vale pra
    // teste que muda o compartilhado.

    fn text_entry(text: &str, ts: u64) -> Entry {
        Entry { id: new_id(), ts_ms: ts, kind: Kind::Text, pinned: false, text: Some(text.into()), ..Entry::default() }
    }

    /// Imagem sintética; `fill` diferente = conteúdo diferente no dedupe.
    fn image_entry(fill: u8, bytes: usize, ts: u64) -> Entry {
        Entry {
            id: new_id(),
            ts_ms: ts,
            kind: Kind::Image,
            pinned: false,
            text: None,
            files: None,
            image_png: Some(vec![fill; bytes]),
            thumb_png: Some(vec![fill; 8]),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("local-utils-history-{tag}-{nanos}"))
    }

    #[test]
    fn dedupe_sobe_pro_topo_sem_duplicar() {
        let mut h = History::default();
        h.push(text_entry("primeiro", 1));
        h.push(text_entry("segundo", 2));
        h.push(text_entry("terceiro", 3));
        assert_eq!(h.len(), 3);

        // A cópia mais antiga é repetida: não pode nascer uma quarta entrada,
        // e ela volta pro topo com o carimbo novo.
        h.push(text_entry("primeiro", 99));
        assert_eq!(h.len(), 3);
        assert_eq!(h.latest().and_then(|e| e.text.as_deref()), Some("primeiro"));
        assert_eq!(h.latest().map(|e| e.ts_ms), Some(99));
        let restantes: Vec<&str> = h.iter().skip(1).filter_map(|e| e.text.as_deref()).collect();
        assert_eq!(restantes, ["terceiro", "segundo"]);
    }

    #[test]
    fn dedupe_nao_duplica_entrada_pinada() {
        let mut h = History::default();
        h.push(text_entry("senha do wifi", 1));
        let id = h.latest().map(|e| e.id).unwrap_or(0);
        h.toggle_pin(id);
        h.push(text_entry("outra coisa", 2));
        h.push(text_entry("senha do wifi", 3));

        assert_eq!(h.len(), 2, "a cópia repetida não pode criar entrada nova");
        assert_eq!(h.latest().and_then(|e| e.text.as_deref()), Some("senha do wifi"));
        assert_eq!(h.latest().map(|e| e.pinned), Some(true), "o pin da entrada original fica");
        assert_eq!(h.latest().map(|e| e.id), Some(id), "o id original é preservado");
    }

    #[test]
    fn retencao_respeita_pinadas() {
        let mut h = History::new(2, MAX_BYTES_DEFAULT);
        h.push(text_entry("antiga", 1));
        let pinada = h.latest().map(|e| e.id).unwrap_or(0);
        h.toggle_pin(pinada);

        h.push(text_entry("media", 2));
        h.push(text_entry("nova", 3));
        // Limite de 2: a "antiga" sobrou só porque está pinada? Não — sai a
        // mais antiga NÃO pinada (a "media"), então cabem nova + antiga.
        assert_eq!(h.len(), 2);
        assert!(h.iter().any(|e| e.text.as_deref() == Some("antiga") && e.pinned), "pinada não sai");
        assert!(h.iter().any(|e| e.text.as_deref() == Some("nova")));
        assert!(!h.iter().any(|e| e.text.as_deref() == Some("media")), "a não pinada mais antiga sai");

        // Só pinadas e ainda acima da cota: a retenção para (não há o que cortar).
        // Pinar ANTES da pressão da cota — pinar depois não testa nada: a
        // entrada não-pinada já saiu no push seguinte (pagou na primeira
        // versão deste teste, que pinava entradas já despejadas).
        let mut so_pinadas = History::new(1, MAX_BYTES_DEFAULT);
        so_pinadas.push(text_entry("a", 1));
        so_pinadas.toggle_pin(so_pinadas.entries[0].id);
        so_pinadas.push(text_entry("b", 2));
        so_pinadas.toggle_pin(so_pinadas.entries[0].id);
        so_pinadas.enforce_retention();
        assert_eq!(so_pinadas.len(), 2, "com tudo pinado a cota de itens não corta nada");
    }

    #[test]
    fn retencao_conta_itens_e_bytes() {
        // Cota de bytes: cada imagem rende 68 bytes (60 + 8 da thumb).
        let mut h = History::new(100, 100);
        h.push(image_entry(1, 60, 1));
        h.push(image_entry(2, 60, 2));
        assert_eq!(h.len(), 1, "a segunda imagem passa da cota de bytes e derruba a primeira");
        assert_eq!(h.total_bytes(), 68);
        assert_eq!(h.latest().map(|e| e.image_png.as_ref().map(|b| b[0])), Some(Some(2)));

        // Cota de itens, com espaço de sobra em bytes.
        let mut itens = History::new(2, MAX_BYTES_DEFAULT);
        itens.push(text_entry("a", 1));
        itens.push(text_entry("b", 2));
        itens.push(text_entry("c", 3));
        assert_eq!(itens.len(), 2);
        assert_eq!(itens.total_bytes(), 0, "texto não conta bytes");
        let sobreviventes: Vec<&str> = itens.iter().filter_map(|e| e.text.as_deref()).collect();
        assert_eq!(sobreviventes, ["c", "b"]);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("history.json");

        let mut h = History::default();
        let pinada = text_entry("texto pinado", 10);
        h.push(pinada.clone());
        h.toggle_pin(pinada.id);
        h.push(image_entry(7, 32, 20));

        assert!(save(&path, &h).is_ok(), "save deveria gravar: {path:?}");
        let back = load(&path);

        assert_eq!(back.len(), 2);
        assert_eq!(back.total_bytes(), h.total_bytes());
        assert_eq!(back.iter().map(|e| e.id).collect::<Vec<_>>(), h.iter().map(|e| e.id).collect::<Vec<_>>());
        assert_eq!(back.iter().map(|e| e.kind).collect::<Vec<_>>(), [Kind::Image, Kind::Text]);
        assert!(back.iter().any(|e| e.id == pinada.id && e.pinned), "o pin sobrevive à gravação");
        assert_eq!(back.iter().find(|e| e.id == pinada.id).and_then(|e| e.text.as_deref()), Some("texto pinado"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_corrompido_vira_vazio_sem_apagar_arquivo() {
        let dir = temp_dir("corrompido");
        let path = dir.join("history.json");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(&path, b"{ isto nao e json").ok();

        let h = load(&path);
        assert!(h.is_empty());
        assert_eq!(h.total_bytes(), 0);
        assert!(path.exists(), "o arquivo corrompido fica no disco pra diagnóstico");

        // Versão desconhecida: também não interpreta nem apaga.
        std::fs::write(&path, br#"{"version":99,"entries":[]}"#).ok();
        assert!(load(&path).is_empty());
        assert!(path.exists());

        // Ausente: vazio, sem criar nada.
        assert!(load(&dir.join("nao-existe.json")).is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn thumb_respeita_o_lado_maximo() {
        // 400x200 com max 256 → 256x128, e a imagem pequena não cresce.
        assert_eq!(thumb_dims(400, 200, 256), (256, 128));
        assert_eq!(thumb_dims(100, 50, 256), (100, 50));
        assert_eq!(thumb_dims(0, 0, 256), (256, 256));

        let rgba = vec![255u8; 8 * 4 * 4];
        let thumb = make_thumb(&rgba, 8, 4, 4);
        assert!(thumb.is_some(), "a miniatura deveria encodar em PNG");
        let decodificada = thumb.and_then(|b| image::load_from_memory(&b).ok());
        assert_eq!(decodificada.map(|i| i.width()), Some(4));
    }
}
