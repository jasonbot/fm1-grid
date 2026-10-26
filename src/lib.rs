//! fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome.
//! The binary (`src/main.rs`) is a thin CLI + window bootstrap over this library;
//! the modules below are the app. See GPUI_PORT.md for the egui → gpui-kit split.


mod affinity;
pub mod app;
mod audio;
pub mod builtin;
pub mod clock;
mod devices;
mod emulator;
pub mod firmware_picker;
mod graph;
mod grid;
pub mod hardware;
pub mod keys;
mod lcd;
#[cfg(not(target_arch = "wasm32"))]
pub mod library;
pub mod midi_keys;
mod panel;
pub mod routing;
mod sidebar;
mod flash_dialog;
mod library_tab;
pub mod store;
mod theme;
mod transport;
mod usbmidi;

#[cfg(test)]
mod tests;

/// The 2x2 grid of emulator panes. The arrays in the app, audio mixer and clock are sized
/// for this. Panes are powered off by default, so idle panes cost nothing.
pub const MAX_SLOTS: usize = 4;
