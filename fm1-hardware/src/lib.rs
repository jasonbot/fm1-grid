//! Live M-VAVE FM-1 and SMK-37 units on USB: discovery and firmware flashing.
//!
//! The protocol, package layout, identity rules, scanner and flash flow live in
//! separate modules; this file keeps the public handles and the command channel.

use fm1_base::clock::{Clock, MidiSink};
use fm1_base::routing::Routing;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};


mod flash;
mod rescue;
mod identity;
mod package;
mod protocol;
mod scan;
#[cfg(test)]
mod tests;

pub(crate) use flash::*;
pub use identity::{device_type, package_fits};
pub(crate) use identity::*;
pub(crate) use package::*;
pub(crate) use protocol::*;
pub(crate) use scan::*;


/// What a running flash is doing right now, for the device's row: the phase, and how
/// far the image transfer has got (None until the device starts pulling).
#[derive(Clone)]
pub struct FlashState {
    pub step: String,
    pub progress: Option<f32>,
    /// Whether the state is a failure message rather than a running phase.
    pub error: bool,
    /// Every phase message so far (the flash dialog's terminal shows the tail).
    /// Progress-only updates don't log.
    pub log: Vec<String>,
}

/// The pane-level flash status. A flash outlives the row it started on: the unit
/// reboots through its update loader and comes back as a new port, so the rows come and
/// go while the flash is running. The board carries the state across that churn.
pub(crate) type Board = Arc<Mutex<Option<FlashState>>>;

/// One live FM-1, as the devices pane shows it.
#[derive(Clone)]
pub struct DeviceRow {
    /// The MIDI port name; the device's key for [`Hardware::flash`] and
    /// [`Hardware::set_clock`].
    pub name: String,
    /// The port's short name ("FM-1 MIDI 1").
    pub port: String,
    /// The ALSA client number, the closest thing to a device number ALSA gives.
    pub number: Option<u32>,
    /// The USB bus-port path ("1-2.4"), when ALSA knows it.
    pub usb: Option<String>,
    /// The raw identity the handshake reported, for target/model checks.
    pub identity: Option<String>,
    /// What the row shows: the friendly firmware name, or a state.
    pub display: String,
    /// What the running flash is doing, if one is running.
    pub flash: Option<FlashState>,
    /// The last flash's outcome, until the next flash.
    pub outcome: Option<String>,
}

pub(crate) struct Device {
    pub(crate) name: String,
    pub(crate) port: String,
    pub(crate) number: Option<u32>,
    pub(crate) usb: Option<String>,
    /// The stable id the clock uses for this device.
    pub(crate) hw_id: usize,
    pub(crate) link: Arc<DeviceLink>,
    pub(crate) identity: Arc<Mutex<Option<String>>>,
    pub(crate) flash: Arc<Mutex<Option<FlashState>>>,
    pub(crate) outcome: Arc<Mutex<Option<String>>>,
}

pub(crate) enum Command {
    Flash { name: String, bytes: Vec<u8> },
    /// Flash a port the scanner never registered (a unit too bricked to answer to an
    /// FM-1-looking name). The name is any MIDI port on the system.
    RescueFlash { name: String, bytes: Vec<u8> },
}

/// The live USB devices: a background scanner thread owns discovery, the GUI reads the
/// row list and posts commands.
pub struct Hardware {
    devices: Arc<Mutex<Vec<Device>>>,
    commands: Sender<Command>,
    /// The pane-level flash status, alive across the reboots of a flash.
    board: Board,
    /// Why MIDI could not be initialised at all, if it could not.
    pub error: Option<String>,
}

impl Hardware {
    /// Start the port scanner. It polls the MIDI ports twice a second and asks every
    /// new port who it is; only units that answer the FM-1 handshake become rows.
    pub fn start(clock: Arc<Clock>, router: Routing) -> Self {
        let devices: Arc<Mutex<Vec<Device>>> = Arc::new(Mutex::new(Vec::new()));
        let board: Board = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel::<Command>();
        let error = match (
            MidiInput::new("fm1-grid-check"),
            MidiOutput::new("fm1-grid-check"),
        ) {
            (Ok(_), Ok(_)) => None,
            (Err(e), _) | (_, Err(e)) => Some(format!("MIDI unavailable: {e}")),
        };
        let rescue = Arc::new(AtomicBool::new(false));
        {
            let devices = devices.clone();
            let board = board.clone();
            thread::Builder::new()
                .name("fm1-usb-scan".into())
                .spawn(move || {
                    scan_loop(devices, clock, router, board, rx, rescue)
                })
                .expect("spawn USB scanner thread");
        }
        Self { devices, commands: tx, board, error }
    }

    /// The pane-level flash status: the phase of the running flash (which may be
    /// between rows, waiting for a rebooting device), or the last failure.
    pub fn flash_state(&self) -> Option<FlashState> {
        self.board.lock().ok().and_then(|state| state.clone())
    }

    /// The live devices, one row each, for the devices pane.
    pub fn devices(&self) -> Vec<DeviceRow> {
        let Ok(list) = self.devices.lock() else {
            return Vec::new();
        };
        list.iter()
            .map(|device| {
                let identity = device.identity.lock().ok().and_then(|i| i.clone());
                let flash = device.flash.lock().ok().and_then(|f| f.clone());
                let display = if flash.is_some() {
                    String::from("flashing… do not unplug")
                } else {
                    match &identity {
                        Some(text) => friendly(text),
                        None => String::from("connected · no reply to the handshake"),
                    }
                };
                DeviceRow {
                    name: device.name.clone(),
                    port: device.port.clone(),
                    number: device.number,
                    usb: device.usb.clone(),
                    identity: identity.clone(),
                    display,
                    flash,
                    outcome: device.outcome.lock().ok().and_then(|o| o.clone()),
                }
            })
            .collect()
    }

    /// Flash a .fwsc package to the named device, on its own thread. A device that is
    /// already flashing ignores the request.
    pub fn flash(&self, name: &str, bytes: Vec<u8>) {
        let _ = self.commands.send(Command::Flash {
            name: name.to_string(),
            bytes,
        });
    }

    /// Flash a .fwsc package to any MIDI port, registered or not: the rescue path for
    /// a unit so bricked it no longer answers to an FM-1-looking name. Runs on its own
    /// thread; a second request while one runs is ignored.
    pub fn rescue_flash(&self, name: &str, bytes: Vec<u8>) {
        let _ = self.commands.send(Command::RescueFlash {
            name: name.to_string(),
            bytes,
        });
    }

    /// Every MIDI port a rescue flash could go to: all the system's ports with a
    /// pairable output, minus this process's own clients and the registered devices
    /// (those flash through [`Hardware::flash`]).
    pub fn midi_ports(&self) -> Vec<String> {
        let (Ok(input), Ok(output)) = (
            MidiInput::new("fm1-grid-scan"),
            MidiOutput::new("fm1-grid-scan"),
        ) else {
            return Vec::new();
        };
        let out_ports = output.ports();
        let registered: Vec<String> = self
            .devices
            .lock()
            .map(|list| list.iter().map(|device| device.name.clone()).collect())
            .unwrap_or_default();
        input
            .ports()
            .iter()
            .filter_map(|port| input.port_name(port).ok())
            .filter(|name| !rescue::ours(name) && !registered.contains(name))
            .filter(|name| {
                out_ports
                    .iter()
                    .any(|port| output.port_name(port).is_ok_and(|n| &n == name))
                    || out_ports.len() == 1
            })
            .collect()
    }

    /// Show a message on the pane-level flash board: the outcome slot for rescue
    /// targets, which have no device row to carry it.
    pub fn report_board(&self, message: String) {
        settle_board(&self.board, &Err(message));
    }

    /// Show a message on the device's row (a file that could not be read, say).
    pub fn report(&self, name: &str, message: String) {
        if let Ok(list) = self.devices.lock() {
            if let Some(device) = list.iter().find(|device| device.name == name) {
                if let Ok(mut outcome) = device.outcome.lock() {
                    *outcome = Some(message);
                }
            }
        }
    }
}
