//! fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome.
//! The binary (`src/main.rs`) is a thin CLI + window bootstrap over this library;
//! the modules below are the app. See GPUI_PORT.md for the egui → gpui-kit split.


mod affinity;
pub mod app;
mod audio;
mod builtin;
mod clock;
mod devices;
mod emulator;
mod firmware_picker;
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
mod usbmidi;

#[cfg(test)]
mod tests;

/// The 2x2 grid of emulator panes. The arrays in the app, audio mixer and clock are sized
/// for this. Panes are powered off by default, so idle panes cost nothing.
pub const MAX_SLOTS: usize = 4;
