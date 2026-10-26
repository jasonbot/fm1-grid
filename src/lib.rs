//! fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome.
//! The binary (`src/main.rs`) is a thin CLI + window bootstrap over this library;
//! the modules below are the app. See GPUI_PORT.md for the egui → gpui-kit split.


// The `gui` feature (default) gates everything that draws with gpui-kit; the
// fm1-tty binary builds with `--no-default-features` and skips the whole GUI
// stack (gpui-kit, cpal, image, rfd, hqx — most of the dependency tree).
#[cfg(feature = "gui")]
mod affinity;
#[cfg(feature = "gui")]
pub mod app;
#[cfg(feature = "gui")]
mod audio;
pub mod builtin;
pub mod clock;
#[cfg(feature = "gui")]
mod devices;
#[cfg(feature = "gui")]
mod emulator;
pub mod firmware_picker;
#[cfg(feature = "gui")]
mod graph;
#[cfg(feature = "gui")]
mod grid;
pub mod hardware;
pub mod keys;
#[cfg(feature = "gui")]
mod lcd;
#[cfg(not(target_arch = "wasm32"))]
pub mod library;
pub mod midi_keys;
#[cfg(feature = "gui")]
mod panel;
pub mod routing;
#[cfg(feature = "gui")]
mod sidebar;
#[cfg(feature = "gui")]
mod flash_dialog;
#[cfg(feature = "gui")]
mod library_tab;
pub mod store;
#[cfg(feature = "gui")]
mod theme;
#[cfg(feature = "gui")]
mod transport;
mod usbmidi;

#[cfg(all(test, feature = "gui"))]
mod tests;

/// The 2x2 grid of emulator panes. The arrays in the app, audio mixer and clock are sized
/// for this. Panes are powered off by default, so idle panes cost nothing.
pub const MAX_SLOTS: usize = 4;
