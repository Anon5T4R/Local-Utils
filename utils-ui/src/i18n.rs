//! i18n via craft-i18n (padrão storytold): catálogos TSV em `i18n/locales/` embutidos
//! pelo `build.rs` (fonte: `scripts/gen_i18n.py` converte este enum). O enum `Key`
//! fica como API tipada (compilador cobre completude); o lookup é o motor do
//! craft-i18n, com fallback `pt-br → pt → en` e ausência de tradução = inglês.

use std::sync::LazyLock;

use craft_i18n::I18n;

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// Códigos dos idiomas oferecidos (mesmos do enum `Lang`).
const OFFERED: &[&str] = &["en", "pt-br", "es"];

static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let (i18n, problems) = I18n::new(CRAFT_I18N, OFFERED);
    for p in problems {
        eprintln!("utils-ui i18n: {p}");
    }
    i18n
});

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    Pt,
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Pt, Lang::En, Lang::Es];

    /// Nome no seletor — sempre endônimo (regra da suíte).
    pub fn label(self) -> &'static str {
        match self {
            Lang::Pt => "Português",
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    // compartilhadas
    AppTitle,
    Language,
    Theme,
    ForgetKey,
    // verificação de atualização (o prompt é o dialog do craft_update)
    CheckUpdates,
    UpToDate,
    // abas do app
    TabClipboard,
    TabScreenshot,
    TabSystem,
    TabTools,
    TabScratchpad,
    // aviso do esqueleto: a aba ainda não tem conteúdo
    TabWip,
}

/// Todas as chaves — usado pelo teste de completude (fora de teste o match
/// exaustivo do `t()` já cobre; o const fica silenciado pra não warnar na
/// build da lib).
#[cfg_attr(not(test), allow(dead_code))]
const KEYS: &[Key] = &[
    Key::AppTitle,
    Key::Language,
    Key::Theme,
    Key::ForgetKey,
    Key::CheckUpdates,
    Key::UpToDate,
    Key::TabClipboard,
    Key::TabScreenshot,
    Key::TabSystem,
    Key::TabTools,
    Key::TabScratchpad,
    Key::TabWip,
];

pub fn t(lang: Lang, key: Key) -> &'static str {
    let code = match lang {
        Lang::Pt => "pt-br",
        Lang::En => "en",
        Lang::Es => "es",
    };
    // A fonte inglesa é a chave de lookup do catálogo (padrão craft-i18n).
    let source = match key {
        Key::AppTitle => "Local Utils",
        Key::Language => "Language",
        Key::Theme => "Theme",
        Key::ForgetKey => "Forget key",
        Key::CheckUpdates => "Check for updates",
        Key::UpToDate => "up to date",
        Key::TabClipboard => "Clipboard",
        Key::TabScreenshot => "Screenshot",
        Key::TabSystem => "System",
        Key::TabTools => "Tools",
        Key::TabScratchpad => "Scratchpad",
        Key::TabWip => "Under construction",
    };
    let craft_lang = I18N.from_code(code).unwrap_or_else(|| I18N.from_code("en").expect("en é builtin"));
    I18N.t(craft_lang, source)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equivalente do "tsc força completude": toda chave tem tradução nos
    /// 3 idiomas (match exaustivo + não-vazio).
    #[test]
    fn todas_as_chaves_tem_traducao() {
        for lang in Lang::ALL {
            for key in KEYS {
                let s = t(lang, *key);
                assert!(!s.is_empty(), "chave {key:?} vazia em {lang:?}");
            }
        }
    }

    /// O motor craft-i18n está ligado: pt-br sai do catálogo TSV, não do inglês.
    #[test]
    fn craft_i18n_busca_no_catalogo() {
        assert_eq!(t(Lang::Pt, Key::TabWip), "Em construção");
        assert_eq!(t(Lang::Es, Key::TabWip), "En construcción");
        assert_eq!(t(Lang::En, Key::TabWip), "Under construction");
        assert_eq!(t(Lang::Pt, Key::CheckUpdates), "Verificar atualizações");
        assert_eq!(t(Lang::Es, Key::CheckUpdates), "Buscar actualizaciones");
        // Marca não traduz.
        assert_eq!(t(Lang::Pt, Key::AppTitle), "Local Utils");
    }
}
