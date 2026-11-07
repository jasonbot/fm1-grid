//! A physical MIDI keyboard driving the on-screen virtual keyboard: the transport bar's
//! selector picks a system MIDI input port; this module keeps that port open on its own
//! thread and maps its note on/off messages onto the FM-1's 27 note keys (F3..G5, MIDI
//! notes 53..79 → key ids 14..40; notes outside that range are ignored — the panel's
//! OCT−/OCT+ buttons shift octaves on the machine itself).
//!
//! The app merges [`MidiKeys::held`] into the same key state the computer keyboard and
//! the panel mouse feed, so the panel lights up and the emulator sees one source. The
//! port list for the selector comes from [`crate::routing`]'s poller (no second
//! enumerator); the port itself is opened here, which ALSA seq allows alongside the
//! graph's own connection.

use midir::{MidiInput, MidiInputConnection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// MIDI note of the panel's lowest key (F3) and the FM-1 key id it plays.
const MIDI_F3: u8 = 53;
const KEY_F3: u8 = 14;
const NOTES: u8 = 27;

enum Command {
    Select(Option<String>),
}

/// Handle to the keyboard driver: the GUI selects a port and reads the held notes; the
/// poller thread owns the port connection. Cloning is not needed; it lives on the app.
pub struct MidiKeys {
    held: Arc<[AtomicBool; 41]>,
    selected: Arc<Mutex<Option<String>>>,
    commands: Sender<Command>,
}

impl MidiKeys {
    /// Start the keyboard poller thread.
    pub fn start() -> Self {
        let held: Arc<[AtomicBool; 41]> = Arc::new(std::array::from_fn(|_| AtomicBool::new(false)));
        let selected = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel::<Command>();
        {
            let held = held.clone();
            let selected = selected.clone();
            thread::Builder::new()
                .name("fm1-grid-keys".into())
                .spawn(move || poll_loop(held, selected, rx))
                .expect("spawn MIDI keyboard thread");
        }
        Self { held, selected, commands: tx }
    }

    /// Pick the port to listen to (None for off); the poller opens it within ½ s.
    pub fn select(&self, port: Option<String>) {
        let _ = self.commands.send(Command::Select(port));
    }

    /// The port the poller is holding open, once the selection has taken effect.
    pub fn selected(&self) -> Option<String> {
        self.selected.lock().ok().and_then(|selected| selected.clone())
    }

    /// The note keys the physical keyboard is holding, for the app's key-state merge.
    pub fn held(&self) -> [bool; 41] {
        std::array::from_fn(|id| self.held[id].load(Ordering::Relaxed))
    }
}

fn clear(held: &[AtomicBool; 41]) {
    for flag in held {
        flag.store(false, Ordering::Relaxed);
    }
}

fn poll_loop(
    held: Arc<[AtomicBool; 41]>,
    selected: Arc<Mutex<Option<String>>>,
    commands: Receiver<Command>,
) {
    let mut connection: Option<MidiInputConnection<Arc<[AtomicBool; 41]>>> = None;
    let mut open: Option<String> = None;
    loop {
        while let Ok(Command::Select(port)) = commands.try_recv() {
            if let Ok(mut selected) = selected.lock() {
                *selected = port;
            }
            // Reopen below; a new (or no) port must not leave notes stuck down.
            open = None;
            clear(&held);
        }
        let want = selected.lock().ok().and_then(|s| s.clone());
        if open != want {
            open = None;
            clear(&held);
        }
        // An unplugged keyboard drops its dead connection; when it returns, the poller
        // reopens it (the selection persists).
        if open.is_some() && !port_alive(want.as_deref()) {
            connection.take();
            open = None;
            clear(&held);
        }
        if let Some(name) = &want {
            if open.is_none() {
                // Assigning drops any previous connection, which closes its port.
                connection = open_port(name, &held);
                open = connection.is_some().then(|| name.clone());
            }
        } else {
            connection.take();
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Whether the named system input port is currently plugged (enumeration is cheap; the
/// graph's poller does it at the same rhythm).
fn port_alive(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    MidiInput::new("fm1-grid-keys").is_ok_and(|input| {
        input
            .ports()
            .iter()
            .any(|port| input.port_name(port).is_ok_and(|n| n == *name))
    })
}

/// Open the named system input port and map its notes onto the held array. A fresh
/// MidiInput per connection: connect() consumes it.
fn open_port(name: &str, held: &Arc<[AtomicBool; 41]>) -> Option<MidiInputConnection<Arc<[AtomicBool; 41]>>> {
    let input = MidiInput::new("fm1-grid-keys").ok()?;
    let port = input
        .ports()
        .into_iter()
        .find(|port| input.port_name(port).is_ok_and(|n| n == *name))?;
    input
        .connect(
            &port,
            "fm1-grid-keys",
            |_time, message, held: &mut Arc<[AtomicBool; 41]>| {
                // Note on/off on any channel; running status and system messages are
                // ignored (a keyboard's notes always arrive as full messages).
                let (note, on) = match *message {
                    [status, note, velocity] if status & 0xF0 == 0x90 => (note, velocity > 0),
                    [status, note, _] if status & 0xF0 == 0x80 => (note, false),
                    _ => return,
                };
                if !(MIDI_F3..MIDI_F3 + NOTES).contains(&note) {
                    return;
                }
                held[(note - MIDI_F3 + KEY_F3) as usize].store(on, Ordering::Relaxed);
            },
            held.clone(),
        )
        .ok() // busy or gone: the poller retries every tick
}
