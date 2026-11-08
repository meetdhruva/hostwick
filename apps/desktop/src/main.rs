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
mod desknotify;
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
mod zoom;

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

/// Where the first window was last time.
fn layout_bounds(backend: &backend::Backend, cx: &App) -> Option<layout::SavedBounds> {
    if !settings::Settings::get(cx).restore_tabs {
        return None;
    }
    let json = backend
        .store
        .setting(layout::TABS_KEY)
        .ok()
        .flatten()
        .filter(|j| !j.is_empty())?;
    layout::SavedSession::from_json(&json).bounds
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
    zoom::set_ui_scale(settings.ui_scale);
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
            // Windows closing because the app quits keep their tabs in the
            // saved layout.
            cx.on_app_quit(|cx| {
                app::mark_quitting(cx);
                async {}
            })
            .detach();
            // Prompts from the SSH layer (host keys, passwords) go to the
            // window in front, one at a time.
            cx.spawn(async move |cx| {
                use futures::StreamExt as _;
                let mut rx = prompts_rx;
                while let Some(p) = rx.next().await {
                    cx.update(|cx| app::route_prompt(p, cx));
                }
            })
            .detach();

            let saved = layout_bounds(&backend, cx);
            let extra = app::saved_extra(&backend, cx);
            let options = app::options_for(saved, cx);
            let first = backend.clone();
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| app::HostwickApp::new(first, window, cx, app::Init::First))
            });
            if let Err(e) = opened {
                eprintln!("hostwick: cannot open window: {e:#}");
                cx.quit();
            }
            app::restore_extra_windows(&backend, extra, cx);
            cx.activate(true);
        });
}
