//! Entradas de build do binário, tudo embutido nele:
//! 1. o ícone no .exe (Windows) — sem isso o Explorer mostra o ícone genérico de executável;
//! 2. `craft_i18n.rs`: os catálogos `i18n/locales/<code>/localutils.tsv` que o `src/i18n.rs` inclui.
//!    `CRAFT_I18N_DIR` troca a pasta; `CRAFT_I18N_REQUIRED=1` (releases) faz catálogo faltante virar erro.
//!    Sem catálogos, a interface fica em inglês.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        res.compile().expect("winresource");
    }
    embed_catalogs();
}

fn embed_catalogs() {
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_REQUIRED");
    let required = std::env::var("CRAFT_I18N_REQUIRED").is_ok_and(|v| v.trim() == "1");
    let default_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n");
    let dir = std::env::var_os("CRAFT_I18N_DIR").filter(|d| !d.is_empty()).map(std::path::PathBuf::from).unwrap_or(default_dir);
    let locales_dir = dir.join("locales");
    let level = if required { "error" } else { "warning" };
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_i18n.rs");
    let mut entries: Vec<(String, std::path::PathBuf)> = Vec::new();
    match std::fs::read_dir(&locales_dir) {
        Ok(list) => {
            println!("cargo::rerun-if-changed={}", locales_dir.display());
            for entry in list.flatten() {
                let path = entry.path();
                let Some(code) = path.file_name().and_then(std::ffi::OsStr::to_str).map(str::to_string) else { continue };
                let app = path.join("localutils.tsv");
                if app.is_file() {
                    println!("cargo::rerun-if-changed={}", app.display());
                    entries.push((code, app));
                }
            }
        }
        Err(e) => println!("cargo::{level}=i18n catalogs: {e}; a interface fica em inglês"),
    }
    if entries.is_empty() {
        println!("cargo::{level}=i18n catalogs: nenhum localutils.tsv; a interface fica em inglês");
    }
    entries.sort();
    // Sem canonicalize: no Windows ele dá `\\?\C:\...`, que o `include_str!` não lê.
    let mut o = String::from("/// (locale, catálogo compartilhado, catálogo do app), um por locale.\n");
    o.push_str("pub static CRAFT_I18N: &craft_i18n::Embedded = &[\n");
    for (code, app) in entries {
        o.push_str(&format!("    ({code:?}, \"\", include_str!({app:?})),\n"));
    }
    o.push_str("];\n");
    if let Err(e) = std::fs::write(&out, o) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}
