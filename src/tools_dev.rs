//! Ferramentas de dev da aba Ferramentas: Base64, URL, hash (SHA-256/MD5),
//! UUID v4 e data ISO ↔ epoch.
//!
//! Tudo **puro** e testável: Base64 e percent-encoding são escritos aqui (são
//! ~30 linhas cada e não valem um crate novo); o hash usa as crates do
//! RustCrypto (`sha2`/`md-5`, já no grafo do app); o UUID é derivado de
//! [`crate::history::new_id`] — **único, mas NÃO criptográfico** (não há RNG do
//! SO aqui de propósito: id de correlação, não segredo); e a data civil reusa o
//! `civil_from_days` do Hinnant que a lista de clipboard já usa.

use md5::Md5;
use sha2::{Digest, Sha256};
use utils_ui::i18n::{self, Key, Lang};

use crate::clipboard_ui::{civil_parts, copy_new_text, Clock};
use crate::history::History;
use crate::ui as kit;

/// Alfabeto do Base64 (RFC 4648, padrão).
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
/// Dígitos hexa em maiúsculas (percent-encoding) e minúsculas (hash/UUID).
const HEX_UP: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOW: &[u8; 16] = b"0123456789abcdef";

/// Erro das ferramentas (cada variante tem uma chave do catálogo).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevError {
    /// Entrada malformada (base64 inválido, `%` sem dois dígitos hexa…).
    BadInput,
    /// O base64/percent-decode não formou UTF-8 válido.
    BadUtf8,
    /// Data no formato errado.
    BadIso,
}

impl DevError {
    fn key(self) -> Key {
        match self {
            DevError::BadInput => Key::DevBadInput,
            DevError::BadUtf8 => Key::DevBadUtf8,
            DevError::BadIso => Key::DevBadIso,
        }
    }
}

// ── Base64 ───────────────────────────────────────────────────────────────

/// Codifica bytes em Base64 (com padding `=`).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Valor de um caractere no alfabeto Base64.
fn b64_value(c: char) -> Option<u32> {
    let byte = u8::try_from(u32::from(c)).ok()?;
    B64.iter().position(|b| *b == byte).map(|i| i as u32)
}

/// Decodifica Base64. Espaços/quebras de linha são ignorados (texto colado);
/// padding no meio, caractere fora do alfabeto ou sobra de 1 sexteto = erro.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, DevError> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut padding = false;
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        if c == '=' {
            padding = true;
            continue;
        }
        if padding {
            return Err(DevError::BadInput);
        }
        let value = b64_value(c).ok_or(DevError::BadInput)?;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // Sobrou UM sexteto: 6 bits não formam byte nenhum — base64 inválido.
    if bits == 6 {
        return Err(DevError::BadInput);
    }
    Ok(out)
}

// ── URL (percent-encoding) ───────────────────────────────────────────────

/// Percent-encoding RFC 3986: só o "não reservado" (`A-Z a-z 0-9 - _ . ~`)
/// escapa de ser codificado — o resto vira `%XX` do byte UTF-8.
pub fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(*byte));
        } else {
            out.push('%');
            out.push(char::from(HEX_UP[(byte >> 4) as usize]));
            out.push(char::from(HEX_UP[(byte & 0x0F) as usize]));
        }
    }
    out
}

/// Valor de um dígito hexa ASCII.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decodifica percent-encoding. `+` vira espaço (convenção de query/form, que é
/// de onde o texto colado quase sempre vem) e `%` sem dois dígitos hexa é erro.
/// O resultado precisa ser UTF-8 válido.
pub fn url_decode(text: &str) -> Result<String, DevError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                let hi = bytes.get(i + 1).copied().and_then(hex_value).ok_or(DevError::BadInput)?;
                let lo = bytes.get(i + 2).copied().and_then(hex_value).ok_or(DevError::BadInput)?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| DevError::BadUtf8)
}

// ── Hash ─────────────────────────────────────────────────────────────────

/// Bytes → hexa minúsculo.
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX_LOW[(byte >> 4) as usize]));
        out.push(char::from(HEX_LOW[(byte & 0x0F) as usize]));
    }
    out
}

/// SHA-256 do texto (UTF-8), em hexa minúsculo.
pub fn sha256_hex(text: &str) -> String {
    hex_lower(Sha256::digest(text.as_bytes()).as_slice())
}

/// MD5 do texto (UTF-8), em hexa minúsculo — checksum de conveniência, não
/// segurança.
pub fn md5_hex(text: &str) -> String {
    hex_lower(Md5::digest(text.as_bytes()).as_slice())
}

// ── UUID ─────────────────────────────────────────────────────────────────

/// 128 bits espalhados a partir do relógio + contador do processo: único na
/// prática, mas **não criptográfico** (o aviso está na UI).
fn uuid_bits() -> u128 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let id = crate::history::new_id();
    let mut high = DefaultHasher::new();
    id.hash(&mut high);
    0u8.hash(&mut high);
    let mut low = DefaultHasher::new();
    id.hash(&mut low);
    1u8.hash(&mut low);
    (u128::from(high.finish()) << 64) | u128::from(low.finish())
}

/// UUID no formato v4 (`xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`).
pub fn uuid_v4() -> String {
    let mut bytes = uuid_bits().to_be_bytes();
    // Nibble de versão e os dois bits de variante (RFC 4122) — o miolo continua
    // vindo do hash.
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from(HEX_LOW[(byte >> 4) as usize]));
        out.push(char::from(HEX_LOW[(byte & 0x0F) as usize]));
    }
    out
}

// ── Data civil ───────────────────────────────────────────────────────────

/// Dia (epoch-days) → (ano, mês, dia) já vem do `civil_from_days`; este é o
/// inverso (o mesmo Hinnant usado pelo `chrono`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Epoch-ms LOCAL → `2026-11-04T14:30:00`.
pub fn epoch_to_iso(local_ms: i64) -> String {
    let (year, month, day, hour, minute) = civil_parts(local_ms);
    let second = local_ms.rem_euclid(60_000) / 1_000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Um número do texto ISO; campo vazio ou não numérico = data inválida.
fn iso_num(part: Option<&str>) -> Result<i64, DevError> {
    part.ok_or(DevError::BadIso)?.trim().parse().map_err(|_| DevError::BadIso)
}

/// `2026-11-04T14:30:00` (ou com espaço, ou só a data) → epoch-ms LOCAL.
pub fn iso_to_local_ms(text: &str) -> Result<i64, DevError> {
    let text = text.trim();
    let (date, time) = match text.split_once(['T', ' ']) {
        Some(pair) => pair,
        // Só a data: meia-noite.
        None => (text, "00:00:00"),
    };
    let mut date_parts = date.split('-');
    let year = iso_num(date_parts.next())?;
    let month = iso_num(date_parts.next())?;
    let day = iso_num(date_parts.next())?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(DevError::BadIso);
    }
    let mut time_parts = time.split(':');
    let hour = iso_num(time_parts.next())?;
    let minute = iso_num(time_parts.next())?;
    // Segundos são opcionais (`14:30` vale).
    let second = match time_parts.next() {
        Some(part) => iso_num(Some(part))?,
        None => 0,
    };
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=59).contains(&second) {
        return Err(DevError::BadIso);
    }
    Ok(days_from_civil(year, month, day) * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000)
}

// ── UI ───────────────────────────────────────────────────────────────────

/// Estado das ferramentas de dev entre frames.
#[derive(Default)]
pub struct DevState {
    b64_in: String,
    b64_out: String,
    b64_error: Option<Key>,
    url_in: String,
    url_out: String,
    url_error: Option<Key>,
    hash_in: String,
    uuid: String,
    iso_in: String,
    epoch_in: String,
    ts_error: Option<Key>,
}

/// O que a aba devolve pro app (histórico de clipboard + status do rodapé).
#[derive(Default)]
pub struct Outcome {
    pub history_changed: bool,
    pub status: Option<String>,
}

/// Título de uma seção.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong());
}

/// Linha de resultado: campo somente-leitura + "Copiar" (cria uma entrada Text
/// nova no histórico de clipboard, o mesmo caminho do recopiar da aba).
fn output_row(ui: &mut egui::Ui, lang: Lang, value: &str, texts: &mut History, out: &mut Outcome) {
    ui.horizontal(|ui| {
        let mut shown = value.to_string();
        let copy_w = 96.0 + ui.spacing().item_spacing.x;
        ui.add(kit::line(&mut shown).interactive(false).desired_width((ui.available_width() - copy_w).max(80.0)));
        if !value.is_empty() && kit::pill(ui, i18n::t(lang, Key::CopyValue), false).clicked() {
            let (changed, status) = copy_new_text(texts, lang, value);
            out.history_changed |= changed;
            out.status = Some(status);
        }
    });
}

/// Mensagem de erro em vermelho do tema (nada de inglês cru na tela).
fn error_line(ui: &mut egui::Ui, lang: Lang, key: Key) {
    let skin = crate::ui::Skin::of(ui);
    ui.label(egui::RichText::new(format!("⚠ {}", i18n::t(lang, key))).color(skin.danger));
}

/// Desenha as ferramentas de dev. `texts` é o histórico de clipboard.
pub fn show(ui: &mut egui::Ui, lang: Lang, state: &mut DevState, texts: &mut History) -> Outcome {
    let mut out = Outcome::default();

    egui::ScrollArea::vertical().id_salt("dev-scroll").auto_shrink([false, false]).show(ui, |ui| {
        // ── Base64 ───────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::DevBase64));
        ui.add(kit::line(&mut state.b64_in).hint_text(i18n::t(lang, Key::DevInput)).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if kit::pill(ui, i18n::t(lang, Key::DevEncode), true).clicked() {
                state.b64_out = base64_encode(state.b64_in.as_bytes());
                state.b64_error = None;
            }
            if kit::pill(ui, i18n::t(lang, Key::DevDecode), false).clicked() {
                match base64_decode(&state.b64_in).and_then(|bytes| String::from_utf8(bytes).map_err(|_| DevError::BadUtf8)) {
                    Ok(text) => {
                        state.b64_out = text;
                        state.b64_error = None;
                    }
                    Err(e) => {
                        state.b64_out.clear();
                        state.b64_error = Some(e.key());
                    }
                }
            }
        });
        if let Some(key) = state.b64_error {
            error_line(ui, lang, key);
        }
        output_row(ui, lang, &state.b64_out, texts, &mut out);
        ui.add_space(10.0);
        ui.separator();

        // ── URL ──────────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::DevUrl));
        ui.add(kit::line(&mut state.url_in).hint_text(i18n::t(lang, Key::DevInput)).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if kit::pill(ui, i18n::t(lang, Key::DevEncode), true).clicked() {
                state.url_out = url_encode(&state.url_in);
                state.url_error = None;
            }
            if kit::pill(ui, i18n::t(lang, Key::DevDecode), false).clicked() {
                match url_decode(&state.url_in) {
                    Ok(text) => {
                        state.url_out = text;
                        state.url_error = None;
                    }
                    Err(e) => {
                        state.url_out.clear();
                        state.url_error = Some(e.key());
                    }
                }
            }
        });
        if let Some(key) = state.url_error {
            error_line(ui, lang, key);
        }
        output_row(ui, lang, &state.url_out, texts, &mut out);
        ui.add_space(10.0);
        ui.separator();

        // ── Hash ─────────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::DevHash));
        ui.add(kit::line(&mut state.hash_in).hint_text(i18n::t(lang, Key::DevInput)).desired_width(f32::INFINITY));
        // Duas passadas pequenas por frame: texto de ferramenta é curto.
        let hash = (sha256_hex(&state.hash_in), md5_hex(&state.hash_in));
        ui.label(egui::RichText::new("SHA-256").small().weak());
        output_row(ui, lang, &hash.0, texts, &mut out);
        ui.label(egui::RichText::new("MD5").small().weak());
        output_row(ui, lang, &hash.1, texts, &mut out);
        ui.add_space(10.0);
        ui.separator();

        // ── UUID ─────────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::DevUuid));
        if state.uuid.is_empty() {
            state.uuid = uuid_v4();
        }
        ui.horizontal(|ui| {
            if kit::pill(ui, i18n::t(lang, Key::DevGenerate), true).clicked() {
                state.uuid = uuid_v4();
            }
            ui.label(egui::RichText::new(i18n::t(lang, Key::DevUuidNote)).small().weak());
        });
        output_row(ui, lang, &state.uuid, texts, &mut out);
        ui.add_space(10.0);
        ui.separator();

        // ── Timestamp ────────────────────────────────────────────────
        heading(ui, i18n::t(lang, Key::DevTimestamp));
        ui.add(kit::line(&mut state.iso_in).hint_text(i18n::t(lang, Key::DevIsoHint)).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if kit::pill(ui, i18n::t(lang, Key::DevToEpoch), true).clicked() {
                // A data digitada é LOCAL; o epoch mostrado é UTC.
                match iso_to_local_ms(&state.iso_in) {
                    Ok(local_ms) => {
                        let offset = Clock::now().offset_ms();
                        state.epoch_in = local_ms.saturating_sub(offset).div_euclid(1_000).to_string();
                        state.ts_error = None;
                    }
                    Err(e) => state.ts_error = Some(e.key()),
                }
            }
            if kit::pill(ui, i18n::t(lang, Key::DevNow), false).clicked() {
                state.iso_in = epoch_to_iso(Clock::now().local_now_ms());
                state.ts_error = None;
            }
        });
        ui.add_space(4.0);
        ui.add(kit::line(&mut state.epoch_in).hint_text(i18n::t(lang, Key::DevEpoch)).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if kit::pill(ui, i18n::t(lang, Key::DevToIso), true).clicked() {
                match state.epoch_in.trim().parse::<i64>() {
                    Ok(seconds) => {
                        let offset = Clock::now().offset_ms();
                        state.iso_in = epoch_to_iso(seconds.saturating_mul(1_000).saturating_add(offset));
                        state.ts_error = None;
                    }
                    Err(_) => state.ts_error = Some(Key::DevBadInput),
                }
            }
        });
        if let Some(key) = state.ts_error {
            error_line(ui, lang, key);
        }
    });

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_ida_e_volta() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        // Acentuação passa como bytes UTF-8.
        assert_eq!(base64_encode("ação".as_bytes()), "YcOnw6Nv");

        assert_eq!(base64_decode("Zm9v"), Ok(b"foo".to_vec()));
        assert_eq!(base64_decode("aGVsbG8="), Ok(b"hello".to_vec()));
        assert_eq!(base64_decode(" YcOnw6Nv\n"), Ok("ação".as_bytes().to_vec()), "espaço e quebra de linha são ignorados");
        // Sem padding também decodifica (texto colado de qualquer lugar).
        assert_eq!(base64_decode("Zm8"), Ok(b"fo".to_vec()));
    }

    #[test]
    fn base64_invalido_e_erro_visivel() {
        assert_eq!(base64_decode("Zm9v*"), Err(DevError::BadInput));
        assert_eq!(base64_decode("Z"), Err(DevError::BadInput), "um sexteto não forma byte");
        assert_eq!(base64_decode("Zg==Zg=="), Err(DevError::BadInput), "padding no meio");
    }

    #[test]
    fn url_ida_e_volta() {
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("ação"), "a%C3%A7%C3%A3o");
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e", "não reservado fica como está");
        assert_eq!(url_encode("a&b=c?d"), "a%26b%3Dc%3Fd");

        assert_eq!(url_decode("a%20b").as_deref(), Ok("a b"));
        assert_eq!(url_decode("a+b").as_deref(), Ok("a b"), "'+' é espaço (convenção de query)");
        assert_eq!(url_decode("a%C3%A7%C3%A3o").as_deref(), Ok("ação"));
        assert_eq!(url_decode("a%2"), Err(DevError::BadInput), "% sem dois dígitos hexa");
        assert_eq!(url_decode("a%ZZ"), Err(DevError::BadInput));
        assert_eq!(url_decode("%FF"), Err(DevError::BadUtf8), "byte solto não é UTF-8");
    }

    #[test]
    fn hash_contra_os_vetores_conhecidos() {
        assert_eq!(sha256_hex(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn uuid_no_formato_v4_e_sempre_diferente() {
        let first = uuid_v4();
        let second = uuid_v4();
        assert_eq!(first.len(), 36);
        assert_eq!(first.chars().filter(|c| *c == '-').count(), 4);
        let bytes: Vec<char> = first.chars().collect();
        assert_eq!(bytes[14], '4', "nibble de versão");
        assert!(matches!(bytes[19], '8' | '9' | 'a' | 'b'), "bits de variante RFC 4122: {}", bytes[19]);
        assert_ne!(first, second);
        assert!(first.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn data_civil_ida_e_volta() {
        // O inverso do `civil_from_days` fecha em qualquer data do intervalo.
        for (year, month, day) in [(1970, 1, 1), (2000, 2, 29), (2026, 11, 4), (1900, 12, 31)] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_parts(days * 86_400_000), (year, month, day, 0, 0), "{year}-{month}-{day}");
        }
    }

    #[test]
    fn iso_e_epoch_nos_dois_sentidos() {
        assert_eq!(epoch_to_iso(0), "1970-01-01T00:00:00");
        assert_eq!(epoch_to_iso(1_000_000_000_000), "2001-09-09T01:46:40");
        assert_eq!(iso_to_local_ms("1970-01-02T00:00:00"), Ok(86_400_000));
        assert_eq!(iso_to_local_ms("1970-01-02 00:00:00"), Ok(86_400_000), "espaço no lugar do T");
        assert_eq!(iso_to_local_ms("1970-01-02"), Ok(86_400_000), "só a data = meia-noite");
        assert_eq!(iso_to_local_ms("2026-11-04T14:30"), Ok(days_from_civil(2026, 11, 4) * 86_400_000 + 14 * 3_600_000 + 30 * 60_000));

        // Ida e volta com hora cheia.
        let local = iso_to_local_ms("2026-11-04T14:30:00");
        assert_eq!(local.map(epoch_to_iso).as_deref(), Ok("2026-11-04T14:30:00"));

        assert_eq!(iso_to_local_ms("04/11/2026"), Err(DevError::BadIso));
        assert_eq!(iso_to_local_ms("2026-13-01"), Err(DevError::BadIso));
        assert_eq!(iso_to_local_ms("2026-11-04T25:00"), Err(DevError::BadIso));
        assert_eq!(iso_to_local_ms(""), Err(DevError::BadIso));
    }
}
