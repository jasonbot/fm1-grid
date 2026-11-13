//! fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome.
//! The binary (`src/main.rs`) is a thin window bootstrap over this library;
//! the modules below are the app. The UI-free machinery (the emulator client, MIDI
//! graph, firmware library, store) lives in the fm1-performer crate. See
//! GPUI_PORT.md for the egui → gpui-kit split.

/// The window title, shared by the native title bar and the in-app one.
pub const APP_TITLE: &str = "Synth Firmware Zoo";

pub mod app;
mod about;
mod audio;
mod devices;
mod firmware_picker;
mod flash_dialog;
mod graph;
mod grid;
mod lcd;
mod library_tab;
mod panel;
mod recovery;
mod session;
mod settings;
mod sidebar;
mod theme;
mod themes;
mod transport;
mod updates;

#[cfg(test)]
mod tests;
