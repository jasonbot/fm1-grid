//! CLI and window bootstrap; the app itself lives in the `fm1_grid` library.

use fm1_grid::MAX_SLOTS;
use fm1_grid::app::GridApp;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::path::PathBuf;

const USAGE: &str = "\
fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome

usage: fm1-grid [FIRMWARE ...]

Each FIRMWARE argument (up to four) is loaded into a pane, left to right, top to bottom. A
firmware is either a .fwsc update package or a raw .bin/.xip image (entry 0x02000120). With
no firmware arguments the embedded firmwares load into the first panes. Panes are powered
off at startup; the power switch in the device sidebar starts and stops each pane's
emulator.

The device sidebar's dropdown offers the embedded firmwares and \"Other…\" for a file
picker. Click a pane (or its display) to route the computer keyboard there.
A W S E D F T G Y H U J K O L P ; ' play C4..F5; C V B N M Q I play F3..B3;
Z/X octave, Space PLAY, R REC, 1..0 functions; ←/→ PRESETS, ↑/↓ ALGORITHM, ,/. SELECT.
";

fn usage_error(message: &str) -> ! {
    eprintln!("fm1-grid: {message}");
    eprint!("{USAGE}");
    std::process::exit(2);
}

fn main() {
    let mut initial = Vec::new();
    for arg in std::env::args_os().skip(1) {
        if arg == "-h" || arg == "--help" {
            print!("{USAGE}");
            return;
        }
        let path = PathBuf::from(arg);
        if path.to_string_lossy().starts_with('-') {
            usage_error(&format!("unknown option {}", path.display()));
        }
        initial.push(path);
    }

    if initial.len() > MAX_SLOTS {
        usage_error(&format!(
            "{} firmware paths were given, but the grid only has {MAX_SLOTS} panes",
            initial.len()
        ));
    }

    // Room for two card rows, the device sidebar and the transport bar.
    application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            init(cx);
            let bounds = Bounds::centered(None, size(px(1180.0), px(1160.0)), cx);
            let (_, app) = open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // Two 1:1 cards across, the sidebar at its 1x floor, the tab bar
                    // and the transport bar. Below this the layout degrades.
                    window_min_size: Some(size(px(840.0), px(950.0))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("FM-1 Grid".into()),
                        ..TitleBar::title_bar_options()
                    }),
                    // TitleBar::window_options: the in-app title bar owns dragging and
                    // draws the window controls where the platform lacks them (GNOME).
                    ..TitleBar::window_options()
                },
                cx,
                move |window, cx| cx.new(|cx| GridApp::new(window, cx, &initial)),
            )
            .expect("failed to open window");

        // Going away: pinned slots snapshot their device storage to the store.
        cx.on_app_quit(move |cx| {
            app.update(cx, |app, _| app.save_pinned());
            async {}
        })
        .detach();
    });
}
