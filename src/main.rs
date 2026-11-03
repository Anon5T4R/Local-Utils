//! Local Utils — utilitários do cotidiano 100% locais e offline.
//! Um binário leve, ancorado pelo gerenciador de clipboard residente na
//! bandeja; os demais utilitários são abas on-demand (só inicializam quando
//! abertas). Este scaffold traz o boot, o tema/i18n comuns e o esqueleto das
//! abas — o conteúdo de cada uma entra na sua versão do roadmap.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod history;

use eframe::egui;
use utils_ui::config;
use utils_ui::theme;

/// Ícone da janela (além do .exe cunhado no build.rs — Windows pega os
/// dois; AppImage/desktop só vendo com o PNG do pacote).
const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

/// Decodifica o PNG do ícone pra `IconData` (o `.with_icon` do egui).
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?;
    let rgba = img.to_rgba8().into_raw();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba })
}

fn main() -> eframe::Result<()> {
    // Windows: o helper de update é uma CÓPIA deste programa iniciada com
    // `--finish-update` (craft_update). Roda ANTES de tudo.
    #[cfg(windows)]
    if let Some(result) = craft_update::self_update::finish(&craft_update::suite::LOCALUTILS, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        std::process::exit(i32::from(result.is_err()));
    }

    let cfg = config::load(app::APP_ID);

    let mut viewport = egui::ViewportBuilder::default().with_title("Local Utils").with_inner_size([980.0, 680.0]);
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
