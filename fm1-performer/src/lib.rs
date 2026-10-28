//! fm1-performer — the UI-free core shared by the fm1-grid gpui app and the fm1-tty
//! terminal companion: the emulator client (one thread + SoC per slot), the USB-MIDI
//! hardware scanner, the MIDI routing graph, the MIDI-clock metronome, the physical
//! MIDI keyboard poller, the firmware library with hub downloads, the embedded
//! firmware table, and the on-disk store.

pub mod affinity;
pub mod audio;
pub mod builtin;
pub mod clock;
pub mod emulator;
pub mod firmware_picker;
pub mod hardware;
pub mod keys;
#[cfg(not(target_arch = "wasm32"))]
pub mod library;
pub mod midi_keys;
pub mod routing;
pub mod store;
pub mod usbmidi;

/// The 2x2 grid of emulator panes. The arrays in the app, audio mixer and clock are sized
/// for this. Panes are powered off by default, so idle panes cost nothing.
pub const MAX_SLOTS: usize = 4;
