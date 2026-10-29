//! fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome.
//! The binary (`src/main.rs`) is a thin CLI + window bootstrap over this library;
//! the modules below are the app. The UI-free machinery (the emulator client, MIDI
//! graph, firmware library, store) lives in the fm1-performer crate. See
//! GPUI_PORT.md for the egui → gpui-kit split.

pub mod app;
mod audio;
mod devices;
mod firmware_picker;
mod flash_dialog;
mod graph;
mod grid;
mod lcd;
mod library_tab;
mod panel;
mod session;
mod settings;
mod sidebar;
mod theme;
mod transport;

#[cfg(test)]
mod tests;
