//! Hostwick desktop: terminals, SFTP and tunnels in one native GPU window.
// No console window behind the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// GPUI code idiomatically writes `Context<Self>`, not `Context<'_, Self>`.
#![allow(elided_lifetimes_in_paths)]

mod app;
mod backend;
mod files;
mod terminal;
mod util;

use std::sync::Arc;

use gpui_kit::*;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("HOSTWICK_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,hostwick=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();

    let (prompts_tx, prompts_rx) = futures::channel::mpsc::unbounded();
    let backend = match backend::Backend::new(prompts_tx) {
        Ok(b) => Arc::new(b),
        Err(e) => {
            eprintln!("hostwick: {e:#}");
            std::process::exit(1);
        }
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(800.0)), cx)),
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
