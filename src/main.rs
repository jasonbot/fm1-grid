mod app;
mod audio;
mod builtin;
mod clock;
mod emulator;

use eframe::egui;
use std::path::PathBuf;

/// The largest number of emulator slots the app can run. The arrays in the app, audio
/// mixer and clock are sized for this, while the actual count is chosen at startup.
pub const MAX_SLOTS: usize = 4;
/// The default number of slots. Two keeps the CPU load and memory use down.
pub const DEFAULT_SLOTS: usize = 2;

const USAGE: &str = "\
fm1-grid — up to four FM-1 emulators with a shared MIDI-clock metronome

usage: fm1-grid [--slots N] [FIRMWARE ...]

  --slots N    number of emulator slots to run, 1..4 (default 2)

Each FIRMWARE argument is loaded into a slot, left to right. A firmware is either a .fwsc
update package or a raw .bin/.xip image (entry 0x02000120). With no firmware arguments the
first N embedded firmwares (SLOOP and Felucca by default) start in the N slots.

Each slot's dropdown offers all four embedded firmwares and \"Other…\" for a file picker.
Click a slot's radio button (or its display) to route the computer keyboard there.
A W S E D F T G Y H U J K O L P ; ' play C4..F5; Z/X octave, Space PLAY, R REC, 1..0 functions.
";

fn usage_error(message: &str) -> ! {
    eprintln!("fm1-grid: {message}");
    eprint!("{USAGE}");
    std::process::exit(2);
}

fn parse_slots(value: &std::ffi::OsStr) -> usize {
    let Some(text) = value.to_str() else {
        usage_error("--slots expects a number");
    };
    match text.parse::<usize>() {
        Ok(n) if (1..=MAX_SLOTS).contains(&n) => n,
        _ => usage_error(&format!("--slots must be between 1 and {MAX_SLOTS}")),
    }
}

fn main() -> eframe::Result {
    let mut initial = Vec::new();
    let mut slot_count = DEFAULT_SLOTS;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "-h" || arg == "--help" {
            print!("{USAGE}");
            return Ok(());
        }
        if arg == "--slots" {
            let Some(value) = args.next() else {
                usage_error("--slots needs a value");
            };
            slot_count = parse_slots(&value);
            continue;
        }
        if let Some(text) = arg.to_str() {
            if let Some(value) = text.strip_prefix("--slots=") {
                slot_count = parse_slots(std::ffi::OsStr::new(value));
                continue;
            }
        }
        let path = PathBuf::from(arg);
        if path.to_string_lossy().starts_with('-') {
            usage_error(&format!("unknown option {}", path.display()));
        }
        initial.push(path);
    }

    if initial.len() > slot_count {
        usage_error(&format!(
            "{} firmware paths were given, but --slots {slot_count} only runs {slot_count}",
            initial.len()
        ));
    }

    // Give the card row enough room to show all slots at a comfortable size.
    let default_width = match slot_count {
        1 => 900.0,
        2 => 1100.0,
        3 => 1320.0,
        _ => 1540.0,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FM-1 Grid")
            .with_inner_size([default_width, 780.0])
            .with_min_inner_size([1000.0, 740.0]),
        ..Default::default()
    };

    eframe::run_native(
        "FM-1 Grid",
        options,
        Box::new(move |cc| Ok(Box::new(app::GridApp::new(cc, &initial, slot_count)))),
    )
}
