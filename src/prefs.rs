//! As preferências do app num JSON pequeno na pasta de config: `%APPDATA%\localutils` no Windows,
//! `$XDG_CONFIG_HOME/localutils` ou `~/.config/localutils` fora dele. Idioma e tema moram ao lado do resto
//! (atalhos, alarmes, notas…).
//!
//! Ler é tolerante: arquivo ausente ou ilegível dá o padrão, campo ausente cai no padrão do campo, e os valores
//! gravados pelas versões do `utils-ui` (`"lang": "Pt"`, ids de tema velhos) caem no equivalente: nunca
//! impedem o app de abrir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n;

/// Processos cujo CONTEÚDO nunca entra no histórico: gerenciadores de senha.
/// Comparação pelo nome do executável, em minúsculas (sem caminho). É a lista
/// que nasce no primeiro boot; a versão editada nas Opções vai pro config.
pub const DEFAULT_EXCLUDE: &[&str] = &["local-keys.exe", "localkeys.exe", "keepass.exe", "keepassxc.exe", "1password.exe", "bitwarden.exe"];

/// Atalho global padrão (`win+alt+v`).
///
/// Só vale pra quem ainda NÃO tem atalho gravado: o campo `hotkey` do config é
/// persistido no primeiro Salvar, e `serde(default)` só entra quando a chave
/// não existe (config nova). Config antiga com `win+shift+v` salvo continua com
/// o atalho dela — mudar o padrão não mexe no que o usuário já escolheu.
pub const DEFAULT_HOTKEY: &str = "win+alt+v";

/// Atalho global de CAPTURA (`win+alt+s`).
///
/// Segunda hotkey do app, independente da primeira: registrada no mesmo
/// gerenciador (`hotkey.rs`) e só entra em conflito se outro app já a tiver.
/// Mesma regra do atalho principal — `serde(default)` só vale pra config nova.
pub const DEFAULT_HOTKEY_SCREENSHOT: &str = "win+alt+s";

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
// `default` no container: config de versão anterior (sem os campos novos) é
// LIDA em vez de descartada — cada campo ausente cai no `Prefs::default()`.
#[serde(default)]
pub struct Prefs {
    /// `system` (segue o sistema), `light` ou `dark`: os ids de `craft_icons::ThemeMode`.
    pub theme: String,
    /// `auto` (segue o sistema) ou um código de `i18n::LANGUAGES`. `lang` era o nome no `utils-ui`.
    #[serde(alias = "lang")]
    pub language: String,
    /// Atalho global que mostra a janela. Falha de parse cai no padrão
    /// (`DEFAULT_HOTKEY`) e o app avisa no rodapé.
    pub hotkey: String,
    /// Atalho global que abre a captura de tela (`capture.rs`).
    /// Falha de parse cai no padrão (`DEFAULT_HOTKEY_SCREENSHOT`).
    pub hotkey_screenshot: String,
    /// Cache do estado lido do registro (HKCU\...\Run). A fonte da verdade é o
    /// registro; isto só evita a UI abrir com o checkbox mentindo antes do
    /// primeiro refresh.
    pub autostart_ok: bool,
    /// Apps sensíveis: cópias vindas deles não entram no histórico. Vale no
    /// próximo arranque (o monitor guarda a lista na subida).
    pub exclude: Vec<String>,
    /// O atalho global abre o POPUP rápido no cursor em vez da janela
    /// principal (`popup.rs`). Vale na hora: quem prefere a janela inteira
    /// desmarca nas opções.
    pub popup_instead_of_window: bool,
    /// Alarmes do relógio (aba Ferramentas): `(hh:mm, rótulo)`. Persistem entre
    /// sessões — o painel do relógio só edita a lista e avisa o app, que grava.
    pub alarms: Vec<(String, String)>,
    /// `.wav` tocado quando um alarme dispara (`alarm_sound.rs`). Vazio = sem
    /// arquivo: o aviso são três bipes do esquema de som do Windows.
    pub alarm_sound: String,
    /// Texto da aba Notas (scratchpad). Gravado pelo autosave debounced da
    /// própria aba (1s depois da última tecla).
    pub notes: String,
    /// Recentes da aba Cor, em hex `#RRGGBB`, mais novo primeiro. O cap (32) e
    /// o dedupe ficam na aba (`picker.rs`); aqui é só a persistência.
    pub recent_colors: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: craft_icons::ThemeMode::System.id().to_string(),
            language: i18n::AUTO.to_string(),
            hotkey: DEFAULT_HOTKEY.to_string(),
            hotkey_screenshot: DEFAULT_HOTKEY_SCREENSHOT.to_string(),
            autostart_ok: false,
            exclude: DEFAULT_EXCLUDE.iter().copied().map(str::to_string).collect(),
            popup_instead_of_window: true,
            alarms: Vec::new(),
            alarm_sound: String::new(),
            notes: String::new(),
            recent_colors: Vec::new(),
        }
    }
}

/// Pasta de config do app (`%APPDATA%\<id>` no Windows, `~/.config/<id>` fora).
pub fn config_dir(app_id: &str) -> PathBuf {
    if let Ok(roaming) = std::env::var("APPDATA") {
        return PathBuf::from(roaming).join(app_id);
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join(app_id);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join(app_id)
}

pub fn load(app_id: &str) -> Prefs {
    load_from(&config_dir(app_id).join("config.json"))
}

/// Lê o arquivo (ausente ou ilegível = padrão) e normaliza os valores antigos.
pub fn load_from(path: &Path) -> Prefs {
    let mut prefs: Prefs = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    prefs.language = match prefs.language.as_str() {
        "Pt" => "pt-br".into(),
        "En" => "en".into(),
        "Es" => "es".into(),
        code if code == i18n::AUTO || i18n::LANGUAGES.contains(&code) => prefs.language,
        _ => i18n::AUTO.to_string(),
    };
    prefs.theme = prefs.mode().id().to_string();
    prefs
}

pub fn save_to(path: &Path, prefs: &Prefs) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(prefs)?)
}

impl Prefs {
    /// Preferências de um idioma e um modo (os testes montam o app com elas).
    #[cfg(test)]
    pub fn new(language: &str, theme: craft_icons::ThemeMode) -> Self {
        Self { language: language.to_string(), theme: theme.id().to_string(), ..Self::default() }
    }

    /// O modo de tema salvo; id desconhecido cai no Automático.
    pub fn mode(&self) -> craft_icons::ThemeMode {
        craft_icons::ThemeMode::from_id(&self.theme)
    }
}

#[cfg(test)]
mod tests {
    use craft_icons::ThemeMode;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("local-utils-prefs-{name}-{nanos}")).join("config.json")
    }

    #[test]
    fn a_saved_choice_comes_back() {
        let path = scratch("roundtrip");
        let prefs = Prefs::new("es", ThemeMode::Dark);
        save_to(&path, &prefs).expect("grava");
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        assert_eq!(load_from(Path::new("/caminho/que/nao/existe.json")), Prefs::default());
        let path = scratch("broken");
        save_to(&path, &Prefs::default()).expect("grava");
        std::fs::write(&path, "{ isso não é json").expect("estraga");
        assert_eq!(load_from(&path), Prefs::default());
    }

    /// O config.json do `utils-ui` (`"lang": "Pt"`, tema por id) abre com o mesmo idioma e tema; valor
    /// desconhecido cai no padrão do campo, e o resto do arquivo continua valendo.
    #[test]
    fn the_utils_ui_file_keeps_its_choices() {
        let path = scratch("utils-ui");
        std::fs::create_dir_all(path.parent().expect("pasta")).expect("pasta");
        std::fs::write(&path, r#"{"theme":"dark","lang":"Pt","hotkey":"ctrl+shift+v"}"#).expect("grava");
        let prefs = load_from(&path);
        assert_eq!((prefs.language.as_str(), prefs.mode()), ("pt-br", ThemeMode::Dark));
        assert_eq!(prefs.hotkey, "ctrl+shift+v", "o resto do arquivo é lido");
        std::fs::write(&path, r#"{"theme":"PunkPrincess","lang":"Klingon"}"#).expect("grava");
        assert_eq!(load_from(&path), Prefs::default());
        std::fs::write(&path, r#"{"theme":"light","lang":"En"}"#).expect("grava");
        assert_eq!(load_from(&path), Prefs::new("en", ThemeMode::Light));
    }

    /// Config da versão anterior (só tema e idioma) continua sendo LIDA: os campos novos entram com o default em
    /// vez de jogar tudo fora.
    #[test]
    fn an_old_config_gets_the_new_fields() {
        let path = scratch("old");
        std::fs::create_dir_all(path.parent().expect("pasta")).expect("pasta");
        std::fs::write(&path, r#"{"theme":"dark","lang":"En"}"#).expect("grava");
        let prefs = load_from(&path);
        assert_eq!(prefs.mode(), ThemeMode::Dark);
        assert_eq!(prefs.language, "en");
        assert_eq!(prefs.hotkey, DEFAULT_HOTKEY);
        assert_eq!(prefs.hotkey_screenshot, DEFAULT_HOTKEY_SCREENSHOT);
        assert!(!prefs.autostart_ok);
        assert_eq!(prefs.exclude.len(), DEFAULT_EXCLUDE.len());
        assert!(prefs.popup_instead_of_window, "o popup no cursor nasce ligado");
        assert!(prefs.alarms.is_empty(), "alarmes nascem sem nenhum");
        assert!(prefs.alarm_sound.is_empty(), "o som do alarme nasce vazio (bipes do SO)");
        assert!(prefs.notes.is_empty(), "o scratchpad nasce vazio");
        assert!(prefs.recent_colors.is_empty(), "os recentes de cor nascem vazios");
    }
}
