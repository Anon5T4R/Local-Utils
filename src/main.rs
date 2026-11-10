//! Local Utils — utilitários do cotidiano 100% locais e offline.
//! Um binário leve, ancorado pelo gerenciador de clipboard residente na
//! bandeja; os demais utilitários são abas on-demand (só inicializam quando
//! abertas). Este scaffold traz o boot, o tema/i18n comuns e o esqueleto das
//! abas — o conteúdo de cada uma entra na sua versão do roadmap.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alarm_sound;
mod app;
mod autostart;
mod capture;
mod clipboard;
mod clipboard_ui;
mod convert;
mod history;
mod hotkey;
mod ocr;
mod options;
mod paste;
mod picker;
mod pickers;
mod popup;
mod shot_editor;
mod system;
mod tools;
mod tools_calc;
mod tools_clock;
mod tools_dev;
mod tools_notes;
mod tools_units;
mod transfer;
mod tray;
mod ui;

#[cfg(windows)]
mod winctl;

use eframe::egui;
use utils_ui::config;
use utils_ui::theme;

/// Ícone da janela (além do .exe cunhado no build.rs — Windows pega os
/// dois; AppImage/desktop só vendo com o PNG do pacote). Também é o ícone da
/// bandeja (`tray.rs`).
pub(crate) const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

/// Decodifica o PNG do ícone pra `IconData` (o `.with_icon` do egui).
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?;
    let rgba = img.to_rgba8().into_raw();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba })
}

fn main() -> eframe::Result<()> {
    // Windows: o helper de update é uma CÓPIA deste programa iniciada com
    // `--finish-update` (craft_update). Roda ANTES de tudo — inclusive da
    // guarda de instância única abaixo, que o helper não pode bloquear (ele
    // espera o app sair e instala por conta própria).
    #[cfg(windows)]
    if let Some(result) = craft_update::self_update::finish(&craft_update::suite::LOCALUTILS, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        std::process::exit(i32::from(result.is_err()));
    }

    // ── single-instance (só Windows — é lá que o X esconde pra bandeja) ──
    // Mesmo desenho do local-keys: lock de ARQUIVO (`File::try_lock` — o
    // kernel libera sozinho em crash) + flag "mostra-te" consumida pela
    // thread da bandeja. Sem isso, abrir pelo ícone de novo cria uma segunda
    // instância (e segunda bandeja) em vez de acordar a que já existe.
    #[cfg(windows)]
    {
        let dir = config::config_dir(app::APP_ID);
        let _ = std::fs::create_dir_all(&dir);
        let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("instance.lock"));
        let segunda = match &lock {
            Ok(f) => f.try_lock().is_err(),
            // Sem conseguir abrir o lock: segue SEM guarda — melhor rodar
            // sem single-instance do que não rodar.
            Err(_) => false,
        };
        if segunda {
            let _ = std::fs::write(dir.join("show.flag"), b"1");
            return Ok(()); // a bandeja do primeiro atende a flag e mostra
        }
        if let Ok(f) = lock {
            std::mem::forget(f); // lock vive até o processo morrer
        }
    }

    let cfg = config::load(app::APP_ID);

    let mut viewport = egui::ViewportBuilder::default().with_title("Local Utils").with_inner_size([980.0, 680.0]);
    // Soltar arquivos na janela (aba Converter): o winit já liga por padrão no
    // Windows, mas explicitamos — é requisito da aba, não um detalhe do SO.
    viewport = viewport.with_drag_and_drop(true);
    // Ícone da janela (o .exe já cunha o mesmo no build.rs — Resources).
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        app::APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            let mut utils = app::UtilsApp::new(cfg);
            // Check de update no arranque: achou release nova, o prompt do
            // updater abre sozinho (craft_update::dialog).
            utils.check_at_start(&cc.egui_ctx);
            Ok(Box::new(utils))
        }),
    )
}

/// Abre a página da release no navegador — ação "Download" do prompt do
/// updater, para instalações que não se auto-atualizam (zip solto, dev build).
pub(crate) fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb = HSTRING::from("open");
        let file = HSTRING::from(url);
        // > 32 = lançou.
        let res = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if res.0 as usize <= 32 {
            eprintln!("update: ShellExecute ao abrir {url}: código {}", res.0 as usize);
        }
    }
    #[cfg(not(windows))]
    {
        if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
            eprintln!("update: xdg-open {url}: {e}");
        }
    }
}
