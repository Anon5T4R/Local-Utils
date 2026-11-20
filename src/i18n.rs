//! Idiomas da interface, sobre o `craft-i18n` (vendorizado em `libs/craft-libs/crates/i18n`), no mesmo desenho
//! do Local Mind.
//!
//! O texto no código fica em inglês e é a chave de busca; os catálogos em `i18n/locales/<code>/localutils.tsv`
//! dão o texto mostrado. Sem tradução, sai o inglês: uma tradução que falta nunca quebra build, teste nem
//! release (o teste `every_text_is_translated` aponta o que falta). O `build.rs` embute os catálogos
//! (`CRAFT_I18N_DIR` troca a pasta; as releases ligam `CRAFT_I18N_REQUIRED=1`).
//!
//! Idioma novo: o `localutils.tsv` em `i18n/locales/<code>/` e uma linha em [`LANGUAGES`].

use std::cell::Cell;
use std::sync::LazyLock;

use craft_i18n::I18n;

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// O valor da preferência de idioma que segue o idioma do sistema.
pub const AUTO: &str = "auto";

/// Os idiomas oferecidos (código BCP 47 em minúsculas, que é também o valor salvo). Inglês primeiro: é a fonte e
/// o recurso final.
pub static LANGUAGES: [&str; 3] = ["en", "pt-br", "es"];

/// Os catálogos embutidos, com exatamente os idiomas de [`LANGUAGES`].
static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let (i18n, problems) = I18n::new(CRAFT_I18N, &LANGUAGES);
    for problem in problems {
        eprintln!("catálogos da interface: {problem}");
    }
    i18n
});

thread_local! {
    /// O idioma do quadro atual. A interface roda na thread principal, então cada busca dispensa argumento.
    static CURRENT: Cell<craft_i18n::Lang> = const { Cell::new(craft_i18n::Lang::EN) };
}

/// O idioma que a preferência pede: `auto` (o do sistema; inglês se não houver catálogo) ou um código de
/// [`LANGUAGES`]. Valor desconhecido cai no inglês.
pub fn resolve(setting: &str) -> craft_i18n::Lang {
    if setting == AUTO {
        return I18N.system_lang();
    }
    I18N.match_tag(setting).or_else(|| I18N.from_code(setting)).unwrap_or(craft_i18n::Lang::EN)
}

/// Fixa o idioma dos próximos quadros (no início e a cada troca da preferência).
pub fn set_language(setting: &str) {
    let lang = resolve(setting);
    CURRENT.with(|current| current.set(lang));
}

fn current() -> craft_i18n::Lang {
    CURRENT.with(Cell::get)
}

/// `source` (inglês) no idioma atual; sem tradução, o inglês.
pub fn tr(source: &'static str) -> &'static str {
    I18N.t(current(), source)
}

/// [`tr`] com os `{nome}` preenchidos (`tr_fmt("version {version}", &[("version", "1.0")])`).
pub fn tr_fmt(source: &'static str, args: &[(&str, &str)]) -> String {
    craft_i18n::fmt(tr(source), args)
}

/// O nome de um idioma no próprio idioma (a linha `@id language.name` do catálogo; senão, o código).
pub fn name_of(code: &str) -> &'static str {
    I18N.from_code(code).map(|lang| I18N.name(lang)).unwrap_or("English")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_parses() {
        let problems = I18N.check_all();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn english_is_the_fallback() {
        set_language("pt-br");
        assert_eq!(tr("Clipboard"), "Área de transferência");
        assert_eq!(tr("a string no catalog has"), "a string no catalog has");
        set_language("es");
        assert_eq!(tr("Tools"), "Herramientas");
        set_language("en");
        assert_eq!(tr("Clipboard"), "Clipboard");
        assert_eq!(resolve("tlh"), craft_i18n::Lang::EN);
    }

    #[test]
    fn every_language_has_its_own_name() {
        assert_eq!(name_of("pt-br"), "Português (Brasil)");
        assert_eq!(name_of("es"), "Español");
        assert_eq!(name_of("en"), "English");
    }

    /// Chaves com `{nome}` chegam preenchidas por `tr_fmt`, em cada idioma.
    #[test]
    fn placeholders_are_filled() {
        set_language("pt-br");
        assert_eq!(tr_fmt("{n} items", &[("n", "4")]), "4 itens");
        set_language("es");
        assert_eq!(tr_fmt("{n} items", &[("n", "4")]), "4 elementos");
        set_language("en");
        assert_eq!(tr_fmt("{n} items", &[("n", "4")]), "4 items");
    }

    /// Todo `tr("…")`/`tr_fmt("…"` do código tem tradução em pt-br e es (o lugar do antigo enum `Key`, que
    /// garantia a completude pelo compilador). Lê as fontes do app, então texto novo sem tradução quebra aqui.
    #[test]
    fn every_text_is_translated() {
        let mut sources = Vec::new();
        collect(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut sources);
        let mut missing = Vec::new();
        for code in ["pt-br", "es"] {
            set_language(code);
            for text in sources.iter().flat_map(|source| literals(source)) {
                // `Box::leak`: o catálogo pede `&'static str`, e o teste vive pouco.
                let text: &'static str = Box::leak(text.into_boxed_str());
                if I18N.t(current(), text) == text && !untranslated_on_purpose(code, text) {
                    missing.push(format!("{code}: {text}"));
                }
            }
        }
        set_language("en");
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "sem tradução:\n{}", missing.join("\n"));
    }

    /// Os textos que ficam iguais ao inglês naquele idioma (siglas, nomes de formato, nomes que as línguas
    /// compartilham, formatos de hora e data).
    fn untranslated_on_purpose(code: &str, text: &str) -> bool {
        let everywhere = matches!(
            text,
            "OCR" | "QR" | "Hex" | "RGB" | "HSL" | "CPU" | "Swap" | "UUID v4" | "Base64" | "HTML" | "RTF" | "URL" | "Hash" | "Pomodoro" | "Local Utils"
                | "+{n} min" | "{h}:{mm}" | "{d}/{month}"
        );
        everywhere || (code == "es" && text == "Color") || (code == "pt-br" && text == "{n} item")
    }

    /// Todos os `.rs` de uma pasta (recursivo), exceto este arquivo, cujos testes citam `tr("…")` de exemplo.
    fn collect(dir: &std::path::Path, into: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("pasta src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, into);
            } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "i18n.rs") {
                into.push(std::fs::read_to_string(&path).expect("lê a fonte"));
            }
        }
    }

    /// Os literais passados a `tr(` e `tr_fmt(` num arquivo-fonte (com os escapes `\"`, `\\` e `\n` resolvidos).
    fn literals(source: &str) -> Vec<String> {
        let mut found = Vec::new();
        for opener in ["tr(\"", "tr_fmt(\""] {
            let mut rest = source;
            while let Some(at) = rest.find(opener) {
                // `from_str("` também contém `tr("`: só vale com o nome inteiro.
                let whole = rest[..at].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
                let after = &rest[at + opener.len()..];
                let mut text = String::new();
                let mut chars = after.char_indices();
                let mut end = None;
                while let Some((i, c)) = chars.next() {
                    match c {
                        '"' => {
                            end = Some(i);
                            break;
                        }
                        '\\' => match chars.next() {
                            Some((_, 'n')) => text.push('\n'),
                            Some((_, other)) => text.push(other),
                            None => break,
                        },
                        other => text.push(other),
                    }
                }
                let Some(end) = end else { break };
                if whole {
                    found.push(text);
                }
                rest = &after[end..];
            }
        }
        found
    }
}
