//! Hostwick desktop: terminals, SFTP and tunnels in one native GPU window.
// No console window behind the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// GPUI code idiomatically writes `Context<Self>`, not `Context<'_, Self>`.
#![allow(elided_lifetimes_in_paths)]

#[macro_use]
mod i18n;

mod a11y;
mod app;
mod backend;
mod editors;
mod files;
mod firstrun;
mod fuzzy;
mod hostform;
mod hostops;
mod layout;
mod logging;
mod notify;
mod settings;
mod settingsform;
mod shortcuts;
mod terminal;
mod tools;
mod util;
mod vault;
mod vaultui;

use std::sync::Arc;

use gpui_kit::*;

// Icons beyond the component set (tools menu, vault indicator).
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Wrench,
        LockOpen,
        CircleQuestionMark,
        FolderPlus,
        LayoutGrid,
        Clock,
        ScanSearch,
        Command,
    ]
);

/// The component icons plus the few extra ones the app uses.
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn main() {
    let (prompts_tx, prompts_rx) = futures::channel::mpsc::unbounded();
    let backend = match backend::Backend::new(prompts_tx) {
        Ok(b) => Arc::new(b),
        Err(e) => {
            eprintln!("hostwick: {e:#}");
            std::process::exit(1);
        }
    };
    let settings = settings::Settings::load(&backend.store);
    logging::init(&backend.paths.logs, &settings.log_level);
    let lang = i18n::set_language(&settings.language);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), data = %backend.paths.data_dir.display(), lang, "starting");

    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            settings::apply_theme(settings.theme, cx);
            cx.set_global(settings);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(800.0)), cx)),
                // Matches StartupWMClass in hostwick.desktop (Linux taskbar icon).
                app_id: Some("hostwick".into()),
                titlebar: Some(TitlebarOptions {
                    title: Some("Hostwick".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app::HostwickApp::new(backend, prompts_rx, window, cx))
            });
            if let Err(e) = opened {
                eprintln!("hostwick: cannot open window: {e:#}");
                cx.quit();
            }
            cx.activate(true);
        });
}
