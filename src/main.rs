mod affinity;
mod app;
mod audio;
mod builtin;
mod clock;
mod devices;
mod emulator;
mod graph;
mod grid;
mod hardware;
mod keys;
mod lcd;
mod midi_keys;
mod panel;
mod routing;
mod sidebar;
mod theme;
mod transport;

use eframe::egui;
use std::path::PathBuf;

/// The 3x3 grid of emulator panes. The arrays in the app, audio mixer and clock are sized
/// for this. Panes are powered off by default, so idle panes cost nothing.
pub const MAX_SLOTS: usize = 9;

const USAGE: &str = "\
fm1-grid — a 3x3 grid of FM-1 emulators with a shared MIDI-clock metronome

usage: fm1-grid [FIRMWARE ...]

Each FIRMWARE argument (up to nine) is loaded into a pane, left to right, top to bottom. A
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

fn main() -> eframe::Result {
    let mut initial = Vec::new();
    for arg in std::env::args_os().skip(1) {
        if arg == "-h" || arg == "--help" {
            print!("{USAGE}");
            return Ok(());
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

    // Room for three card rows, the device sidebar and the transport bar.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FM-1 Grid")
            .with_inner_size([1180.0, 1160.0])
            .with_min_inner_size([900.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "FM-1 Grid",
        options,
        Box::new(move |cc| Ok(Box::new(app::GridApp::new(cc, &initial)))),
    )
}
