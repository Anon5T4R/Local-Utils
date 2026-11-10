//! Export/import do histórico em arquivo próprio `.lutils` (molde do `.tkeys`
//! do Local-Keys).
//!
//! O arquivo é o MESMO JSON do `history.json` (`{"version":1,"entries":[...]}`)
//! gravado com tmp + rename: exportar é só gravar o histórico atual num caminho
//! escolhido. Importar lê o arquivo com validação (versão + JSON) e MESCLA no
//! histórico de agora — dedupe por `id` (fica a entrada mais nova por `ts_ms`)
//! e por conteúdo (a mesma cópia com id diferente vira uma só).
//!
//! A lógica é cross-platform; o diálogo é o seletor nativo assíncrono de
//! `src/pickers.rs` (Windows: nativo; Linux: portal XDG).

use std::path::Path;

use crate::history::{self, History};

/// Nome sugerido no "Salvar como": `history-export-<data>.lutils`.
pub fn export_name() -> String {
    format!("history-export-{}.lutils", today_stamp())
}

/// Data de hoje (UTC) pro nome do arquivo. Local exigiria API de fuso; aqui o
/// carimbo é só pra ordenar exportações — UTC serve.
fn today_stamp() -> String {
    let days = i64::try_from(history::now_ms() / 86_400_000).unwrap_or(0);
    let (y, m, d) = crate::clipboard_ui::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Diálogo "Salvar como" do export, com o nome sugerido. Quem espera a
/// resposta é a thread de `src/pickers.rs`; paths vazios de lá = cancelado.
pub fn export_dialog(name: &str) -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new().add_filter("Histórico do Local Utils (.lutils)", &["lutils"]).set_file_name(name)
}

/// Diálogo "Abrir" do import.
pub fn open_dialog() -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new().add_filter("Histórico do Local Utils (.lutils)", &["lutils"])
}

/// Grava o histórico no caminho escolhido (atômico: tmp + rename).
pub fn export_to(path: &Path, h: &History) -> Result<(), String> {
    history::save(path, h).map_err(|e| format!("gravar {}: {e}", path.display()))
}

/// Lê o `.lutils` e mescla no histórico atual.
pub fn import_into(path: &Path, h: &mut History) -> Result<(), String> {
    let outro = history::load_import(path)?;
    h.merge(outro);
    Ok(())
}
