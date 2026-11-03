//! Autostart no login (Windows): valor na chave
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//!
//! HKCU (não HKLM) de propósito: subir no login do usuário não pede elevação e
//! o MSI é per-machine — o instalador não pode escrever no perfil de ninguém.
//! O registro é a fonte da verdade (`enabled` sempre relê); o `autostart_ok` do
//! config é só cache pra UI abrir coerente.
//!
//! Fora do Windows tudo vira stub (não há chave Run): `set` devolve erro e a
//! UI deixa o toggle desabilitado.

/// O app está configurado pra subir no login?
pub fn enabled() -> bool {
    imp::enabled()
}

/// Liga/desliga o autostart. `false` é idempotente (valor ausente = ok).
pub fn set(on: bool) -> Result<(), String> {
    imp::set(on)
}

#[cfg(windows)]
mod imp {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    /// Nome do valor na chave Run. Estável: é por ele que o app se acha (e se
    /// remove) depois.
    const VALUE_NAME: &str = "LocalUtils";

    /// A chave Run do usuário atual.
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ).and_then(|key| key.get_value::<String, _>(VALUE_NAME)).is_ok()
    }

    pub fn set(on: bool) -> Result<(), String> {
        let key =
            RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE).map_err(|e| format!("abrir {RUN_KEY}: {e}"))?;
        if on {
            let exe = std::env::current_exe().map_err(|e| format!("caminho do executável: {e}"))?;
            // Aspas: o caminho pode ter espaço (Program Files). `--autostart`
            // não existe (o app sobe igual), mas o valor fica auto-explicativo.
            let value = format!("\"{}\"", exe.display());
            key.set_value(VALUE_NAME, &value).map_err(|e| format!("gravar o valor: {e}"))
        } else {
            match key.delete_value(VALUE_NAME) {
                Ok(()) => Ok(()),
                // Já não estava lá: desligar de novo não é erro.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("remover o valor: {e}")),
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<(), String> {
        Err("autostart: só Windows por enquanto".into())
    }
}
