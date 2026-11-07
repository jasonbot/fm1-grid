//! Shared machinery for both apps: the MIDI clock, the routing graph, the physical
//! keyboard poller, the key tables and the on-disk store. No firmware or hardware
//! flashing lives here.

pub mod clock;
pub mod keys;
pub mod midi_keys;
pub mod package;
pub mod routing;
pub mod store;
