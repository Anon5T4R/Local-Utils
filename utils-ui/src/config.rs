//! Config por app em JSON, no mesmo espírito do padrão Tauri da suíte
//! (theme + lang persistidos), só que resolvido à mão: %APPDATA% no Windows,
//! XDG/~/.config no resto. Load tolerante: ausente ou corrompido = default.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::theme::Appearance;

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
// LIDA em vez de descartada — cada campo ausente cai no `Config::default()`.
#[serde(default)]
pub struct Config {
    pub theme: Appearance,
    pub lang: Lang,
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
    /// Texto da aba Notas (scratchpad). Gravado pelo autosave debounced da
    /// própria aba (1s depois da última tecla).
    pub notes: String,
    /// Recentes da aba Cor, em hex `#RRGGBB`, mais novo primeiro. O cap (32) e
    /// o dedupe ficam na aba (`picker.rs`); aqui é só a persistência.
    pub recent_colors: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Appearance::System,
            lang: Lang::Pt,
            hotkey: DEFAULT_HOTKEY.to_string(),
            hotkey_screenshot: DEFAULT_HOTKEY_SCREENSHOT.to_string(),
            autostart_ok: false,
            exclude: DEFAULT_EXCLUDE.iter().copied().map(str::to_string).collect(),
            popup_instead_of_window: true,
            alarms: Vec::new(),
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

pub fn load(app_id: &str) -> Config {
    load_from(&config_dir(app_id).join("config.json"))
}

pub fn save(app_id: &str, cfg: &Config) -> std::io::Result<()> {
    save_to(&config_dir(app_id).join("config.json"), cfg)
}

pub fn load_from(path: &Path) -> Config {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_to(path: &Path, cfg: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let dir = std::env::temp_dir()
            .join(format!("utils-ui-test-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("config.json");

        let cfg = Config { theme: Appearance::Dark, lang: Lang::Es, ..Config::default() };
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path), cfg);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ausente_ou_corrompido_vira_default() {
        assert_eq!(load_from(Path::new("/caminho/que/nao/existe.json")), Config::default());
        let dir = std::env::temp_dir()
            .join(format!("utils-ui-test-bad-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("config.json");
        save_to(&path, &Config::default()).unwrap();
        std::fs::write(&path, "{ isso não é json").unwrap();
        assert_eq!(load_from(&path), Config::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Config da versão anterior (só tema e idioma) continua sendo LIDA: os
    /// campos novos entram com o default em vez de jogar tudo fora.
    #[test]
    fn config_antiga_ganha_os_campos_novos() {
        let dir = std::env::temp_dir()
            .join(format!("utils-ui-test-old-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"theme":"dark","lang":"En"}"#).unwrap();

        let cfg = load_from(&path);
        assert_eq!(cfg.theme, Appearance::Dark);
        assert_eq!(cfg.lang, Lang::En);
        assert_eq!(cfg.hotkey, DEFAULT_HOTKEY);
        assert_eq!(cfg.hotkey_screenshot, DEFAULT_HOTKEY_SCREENSHOT);
        assert!(!cfg.autostart_ok);
        assert_eq!(cfg.exclude.len(), DEFAULT_EXCLUDE.len());
        assert!(cfg.popup_instead_of_window, "o popup no cursor nasce ligado");
        assert!(cfg.alarms.is_empty(), "alarmes nascem sem nenhum");
        assert!(cfg.notes.is_empty(), "o scratchpad nasce vazio");
        assert!(cfg.recent_colors.is_empty(), "os recentes de cor nascem vazios");

        std::fs::remove_dir_all(&dir).ok();
    }
}
