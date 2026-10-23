//! Live M-VAVE FM-1 units on USB: discovery, firmware detection, firmware flashing and
//! MIDI-clock routing.
//!
//! The protocol is the FM-1's "M-UPGRADE" update path over USB-MIDI, reverse-engineered
//! from thefirmwarehub.com's web installer (sloopDX's fm1pkg.js and fm1ota.js, GPL-3.0):
//!
//! - Detection: send the handshake SysEx `F0 00 32 45 00 00 00 40 7F F7`; the unit
//!   answers with an identity SysEx whose 7-bit-packed payload starts `00 59 11` and
//!   carries a "MODEL_VERSION" string at offset 6 (for example "FM-1_916" = Felucca;
//!   "ota-FM-1_9xx" = the unit is sitting in its update loader).
//! - Flashing: send `F0 22 24 35 7F F7` (UPGRADE) and the device *pulls* the firmware:
//!   it sends read requests (payload `00 59 30 ...`, 7-bit packed, little-endian
//!   address and length, checksummed) and the host answers with data packets. Step 1
//!   (the running firmware stages the update loader) ends with a request for address
//!   0xE000_0000, answered with "success"; the unit reboots into its loader, which
//!   pulls the same image again (ending at 0xF000_0000) and writes the flash. The unit
//!   then reboots into the new firmware, whose identity must equal the package's
//!   product string.
//! - Packages: a .fwsc file embeds its product string in the last byte of each of its
//!   first twenty 48-byte blocks (0x7D = no character, otherwise the character is
//!   `(byte - block - 1) & 0xFF`). The image the device pulls is the package with
//!   those twenty marker bytes removed.
//!
//! Discovery polls the MIDI ports twice a second: midir gives no hot-plug events, and
//! polling a port list is cheap. Only ports whose names look like an FM-1 are ever
//! probed (the hub's own filter), so other MIDI gear is never touched. Everything that
//! talks to a device runs off the GUI thread.

use crate::clock::{Clock, MidiSink};
use crate::routing::Routing;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The handshake query: "who are you?" (fm1ota.js HS_QUERY).
const HS_QUERY: [u8; 10] = [0xF0, 0x00, 0x32, 0x45, 0x00, 0x00, 0x00, 0x40, 0x7F, 0xF7];
/// The upgrade command: the device starts pulling the image (fm1ota.js UPGRADE).
const UPGRADE: [u8; 6] = [0xF0, 0x22, 0x24, 0x35, 0x7F, 0xF7];
/// The largest data packet a device may ask for.
const MAXDATA: usize = 512;
/// The "I am done" addresses: step 1 (staging the loader) and step 2 (writing).
const FINISH_STAGE: u32 = 0xE000_0000;
const FINISH_WRITE: u32 = 0xF000_0000;
/// .fwsc package identity layout: twenty 48-byte blocks, byte 47 of each is a marker.
const PKG_BLOCKS: usize = 20;
const PKG_BLOCK: usize = 0x30;
const PKG_KEEP: usize = 0x2F;

/// Port names the scanner may handshake-probe (the firmware hub's filter, lowercased).
/// Anything else is somebody's MIDI keyboard and is never touched.
const PROBE_NAMES: [&str; 11] = [
    "fm-1",
    "felucca",
    "sloop",
    "clipper",
    "etch",
    "jiant",
    "ota",
    "update",
    "composite",
    "sinco",
    "usb-midi",
];

/// Known product strings and the firmwares that report them (thefirmwarehub.com's
/// catalogue, October 2026). Several projects share one product string; a single exact
/// hit gets a friendly name, a shared one shows how many firmwares it could be.
const KNOWN: &[(&str, &[&str])] = &[
    ("FM-1_003", &["Bubba Machine"]),
    ("FM-1_1500", &["ETCH"]),
    ("FM-1_2123", &["BEETLE"]),
    ("FM-1_337", &["Chipbox"]),
    ("FM-1_600", &["FM1-GM"]),
    ("FM-1_700", &["Tonitor", "Vector37"]),
    ("FM-1_703", &["Fart Machine One"]),
    ("FM-1_714", &["Skiff"]),
    ("FM-1_777", &["Evander's Music Box"]),
    ("FM-1_80023", &["Unholy Union"]),
    ("FM-1_900", &[
        "Hortator",
        "GHOULBOX",
        "SLOOP",
        "Clipper",
        "JIANT FM",
        "winampFM Synth",
        "MYCELIUM",
        "ORBIT",
        "Felucca 8 Patterns",
        "PIGEON",
        "NOIZ DEMONS",
        "MC-1",
        "Tuna",
        "No Man Sky FM",
        "FM1 Quest",
        "Mini-Tone",
        "FMini-1",
        "Golana DZ Edition",
        "Flyfish",
    ]),
    ("FM-1_901", &["DUELL"]),
    ("FM-1_903", &["fm1-chord"]),
    ("FM-1_907", &["FM-106"]),
    ("FM-1_909", &["Felucca [Salt]"]),
    ("FM-1_910", &["Jangada", "WaveLoop FM-1", "MAKU"]),
    ("FM-1_91045", &["Melodee"]),
    ("FM-1_911", &["Rainbow Mode"]),
    ("FM-1_912", &["Golana"]),
    ("FM-1_916", &["Felucca"]),
    ("FM-1_920", &["ChoralRoot"]),
    ("FM-1_924", &["SLOOP Floyd FM"]),
    ("FM-1_925", &["SLOOP DX7 Banks"]),
    ("FM-1_927", &["PurpleMonkey"]),
    ("FM-1_930", &["CoralFlower"]),
    ("FM-1_940", &["sloopDX"]),
    ("FM-1_942", &["Bytemania"]),
    ("FM-1_950", &["CS-1", "OPN-1", "FMEL-1"]),
    ("FM-1_951", &["Ketch"]),
    ("FM-1_955", &["OberVave", "KAWA"]),
    ("FM-1_958", &["Frodhes"]),
    ("FM-1_960", &["SID-1"]),
    ("FM-1_961", &["FOMAJAM"]),
    ("FM-1_962", &["Shine On"]),
    ("FM-1_966", &["GR!MM"]),
    ("FM-1_974", &["zp12"]),
    ("FM-1_981", &["Pocket GammaWave"]),
    ("FM-1_993", &["Dinghy"]),
    ("FM-1_3000500", &["Polly"]),
    ("FM-1_386010002", &["386DX"]),
    ("FM-1_42000300", &["MODELAR"]),
    ("FM-1_4310100", &["HERA"]),
    ("FM-1_5000201", &["MIN-1"]),
    ("FM-1_5000901", &["MONK-1"]),
    ("FM-1_6010000", &["CTL-1"]),
    ("FM-1_6010200", &["ZEN-1"]),
    ("FM-1_7010100", &["FuMi-1"]),
    ("FM-1_7010201", &["Cut-A-Dor"]),
    ("FM-1_7010300", &["AMB-1"]),
    ("FM-1_7010554", &["HYP-1"]),
    ("FM-1_7100002", &["Bunker Evolved"]),
    ("FM-1_710010000", &["Mutable Dreams"]),
    ("FM-1_8000013", &["MORPHEUS"]),
    ("FM-1_8000200", &["Niagara Drift"]),
    ("FM-1_8000400", &["FoMni"]),
    ("FM-1_8000601", &["FiMba-1"]),
    ("FM-1_8010300", &["DUB-1"]),
    ("FM-1_9000300", &["TRIBE"]),
    ("FM-1_9010100", &["X0X"]),
    ("FM-1_987010100", &["X0X-RMX"]),
    ("FM-1_10000601", &["LOOM-8"]),
];

/// Older firmware versions report other numbers: (prefix, digits after it, family
/// name), checked only when no exact product matches (the catalogue's ident patterns).
const FAMILIES: &[(&str, usize, &str)] = &[
    ("FM-1_386", 6, "386DX"),
    ("FM-1_800", 4, "FoMni or FiMba-1"),
    ("FM-1_90", 5, "X0X"),
    ("FM-1_70", 5, "FuMi-1"),
    ("FM-1_97", 1, "zp12"),
    ("FM-1_93", 1, "sloopDX"),
    ("FM-1_95", 1, "OberVave or KAWA"),
    ("FM-1_99", 1, "Dinghy"),
    ("FM-1_91", 1, "Felucca"),
    ("FM-1_71", 1, "Skiff"),
    ("FM-1_3", 6, "Polly"),
];

/// A friendly name for an identity text: "Felucca (FM-1_916)", "the update loader
/// (ota-FM-1_916)", "the official firmware V15", or the raw text when unknown.
pub(crate) fn friendly(text: &str) -> String {
    if let Some(rest) = text
        .strip_prefix("ota-")
        .or_else(|| text.strip_prefix("OTA-"))
    {
        return format!("the update loader ({rest})");
    }
    match text {
        "FM-1_015" => return "the official firmware V15".to_string(),
        "FM-1_016" => return "the official firmware V16".to_string(),
        _ => {}
    }
    if let Some((_, names)) = KNOWN.iter().find(|(product, _)| *product == text) {
        return match names.len() {
            1 => format!("{} ({text})", names[0]),
            2 => format!("{} or {} ({text})", names[0], names[1]),
            n => format!("one of {n} firmwares ({text})"),
        };
    }
    if let Some((_, _, name)) = FAMILIES.iter().find(|(prefix, digits, _)| {
        let Some(rest) = text.strip_prefix(*prefix) else {
            return false;
        };
        rest.len() == *digits && rest.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return format!("{name} ({text})");
    }
    format!("firmware {text}")
}

/// The package identity ("FM-1_916"): the marker byte after each of the first twenty
/// 48-byte blocks. None when the file is not an FM-1 package.
fn product_of(fwsc: &[u8]) -> Option<String> {
    if fwsc.len() < PKG_BLOCKS * PKG_BLOCK {
        return None;
    }
    let mut s = String::new();
    for i in 0..PKG_BLOCKS {
        let m = fwsc[i * PKG_BLOCK + PKG_KEEP];
        if m != 0x7D {
            s.push((m.wrapping_sub(i as u8).wrapping_sub(1)) as char);
        }
    }
    (!s.is_empty()).then_some(s)
}

/// The image the device pulls during an update: the package without the twenty marker
/// bytes.
fn logical_image(fwsc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(fwsc.len().saturating_sub(PKG_BLOCKS));
    for i in 0..PKG_BLOCKS {
        out.extend_from_slice(&fwsc[i * PKG_BLOCK..i * PKG_BLOCK + PKG_KEEP]);
    }
    out.extend_from_slice(&fwsc[PKG_BLOCKS * PKG_BLOCK..]);
    out
}

/// Pack 8-bit data into SysEx 7-bit words (a little-endian bit stream), as the update
/// protocol uses.
fn pack7(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 8 / 7 + 1);
    let (mut acc, mut nb) = (0u32, 0u32);
    for &b in data {
        acc |= (b as u32) << nb;
        nb += 8;
        while nb >= 7 {
            out.push((acc & 0x7F) as u8);
            acc >>= 7;
            nb -= 7;
        }
    }
    if nb > 0 {
        out.push((acc & 0x7F) as u8);
    }
    out
}

fn unpack7(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 7 / 8 + 1);
    let (mut acc, mut nb) = (0u32, 0u32);
    for &b in data {
        acc |= ((b & 0x7F) as u32) << nb;
        nb += 7;
        while nb >= 8 {
            out.push((acc & 0xFF) as u8);
            acc >>= 8;
            nb -= 8;
        }
    }
    out
}

/// A data packet answering a read request: header `00 59 30`, the total length, the
/// request's flag, the address, the data length, the data and a checksum, 7-bit packed
/// into a SysEx.
fn response(addr: u32, data: &[u8], fl: u8) -> Vec<u8> {
    let len = data.len();
    let mut body = Vec::with_capacity(15 + len);
    body.extend_from_slice(&[
        0x00,
        0x59,
        0x30,
        ((len + 8) & 0xFF) as u8,
        (((len + 8) >> 8) & 0xFF) as u8,
        0,
        fl,
        (addr & 0xFF) as u8,
        ((addr >> 8) & 0xFF) as u8,
        ((addr >> 16) & 0xFF) as u8,
        ((addr >> 24) & 0xFF) as u8,
        (len & 0xFF) as u8,
        ((len >> 8) & 0xFF) as u8,
        0,
    ]);
    body.extend_from_slice(data);
    let sum: u32 = body[6..].iter().map(|&b| b as u32).sum();
    body.push((!sum & 0xFF) as u8);
    let mut sysex = Vec::with_capacity(body.len() + 2);
    sysex.push(0xF0);
    sysex.extend_from_slice(&pack7(&body));
    sysex.push(0xF7);
    sysex
}

/// The identity an answered handshake reports, from its SysEx packet.
fn parse_identity(pkt: &[u8]) -> Option<String> {
    if pkt.first() != Some(&0xF0) || pkt.last() != Some(&0xF7) {
        return None;
    }
    let d = unpack7(&pkt[1..pkt.len() - 1]);
    if d.len() != 34 || d[0] != 0 || d[1] != 0x59 || d[2] != 0x11 {
        return None;
    }
    let text: String = d[6..33]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    // "MODEL_VERSION", like "FM-1_916" or "ota-FM-1_904".
    let valid = text
        .split_once('_')
        .is_some_and(|(model, version)| !model.is_empty() && !version.is_empty());
    valid.then_some(text)
}

/// One read request from the device during an update.
struct Request {
    fl: u8,
    addr: u32,
    len: u32,
}

fn parse_request(pkt: &[u8]) -> Option<Request> {
    if pkt.first() != Some(&0xF0) || pkt.last() != Some(&0xF7) {
        return None;
    }
    let u = unpack7(&pkt[1..pkt.len() - 1]);
    if u.len() != 15 || u[0] != 0 || u[1] != 0x59 || u[2] != 0x30 {
        return None;
    }
    let sum: u32 = u[6..14].iter().map(|&b| b as u32).sum();
    if (!sum & 0xFF) as u8 != u[14] {
        return None;
    }
    Some(Request {
        fl: u[6],
        addr: u[7] as u32 | (u[8] as u32) << 8 | (u[9] as u32) << 16 | (u[10] as u32) << 24,
        len: u[11] as u32 | (u[12] as u32) << 8 | (u[13] as u32) << 16,
    })
}

/// midir's ALSA port names look like "FM-1:32 FM-1 MIDI 1:0": client name and number,
/// then port name and number. The parts the device row shows.
fn split_port_name(name: &str) -> (String, Option<u32>) {
    let (client, rest) = name.split_once(' ').unwrap_or((name, ""));
    let (client_name, number) = match client.split_once(':') {
        Some((client_name, number)) => (client_name, number.parse().ok()),
        None => (client, None),
    };
    let port = rest.rsplit_once(':').map(|(port, _)| port).unwrap_or(rest);
    let port = if port.is_empty() { client_name } else { port };
    (port.to_string(), number)
}

/// The device's USB bus-port path ("1-2.4"), best effort: the ALSA card whose id is
/// the port's client name, and its usbbus file. Two units with the same card id share
/// one path here; the row still tells them apart by their ALSA client number.
fn usb_path(port_name: &str) -> Option<String> {
    let card = port_name.split([':', ' ']).next()?;
    let cards = std::fs::read_to_string("/proc/asound/cards").ok()?;
    for line in cards.lines() {
        let Some((number, rest)) = line.trim().split_once('[') else {
            continue;
        };
        let Some(id) = rest.split(']').next() else {
            continue;
        };
        if id.trim() != card {
            continue;
        }
        let bus =
            std::fs::read_to_string(format!("/proc/asound/card{}/usbbus", number.trim())).ok()?;
        return Some(bus.trim().to_string());
    }
    None
}

/// A SysEx queue, filled by the MIDI-input callback thread.
type Queue = Arc<Mutex<VecDeque<Vec<u8>>>>;

/// The input callback's data: the SysEx queue for the update protocol, plus the MIDI
/// graph's router, so every message the device sends also flows through the graph.
struct CallbackData {
    queue: Queue,
    router: Routing,
    name: String,
}

/// One open MIDI in/out pair with a live device. Shared by the clock thread (single
/// real-time bytes), the flash thread (packets), the scanner (handshakes) and the MIDI
/// graph (whole messages, both directions).
pub(crate) struct DeviceLink {
    out: Mutex<MidiOutputConnection>,
    queue: Queue,
    /// Kept alive: dropping it closes the input port and its callback.
    _input: Mutex<MidiInputConnection<CallbackData>>,
}

impl DeviceLink {
    pub(crate) fn send(&self, bytes: &[u8]) -> bool {
        self.out.lock().is_ok_and(|mut out| out.send(bytes).is_ok())
    }

    /// The next SysEx in the queue, waiting up to `timeout` for one.
    fn read(&self, timeout: Duration) -> Option<Vec<u8>> {
        let end = Instant::now() + timeout;
        loop {
            if let Ok(mut queue) = self.queue.lock() {
                if let Some(pkt) = queue.pop_front() {
                    return Some(pkt);
                }
            }
            if Instant::now() >= end {
                return None;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn drain(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }
}

impl MidiSink for DeviceLink {
    fn send_byte(&self, b: u8) {
        self.send(&[b]);
    }
}

/// What a running flash is doing right now, for the device's row: the phase, and how
/// far the image transfer has got (None until the device starts pulling).
#[derive(Clone)]
pub(crate) struct FlashState {
    pub step: String,
    pub progress: Option<f32>,
    /// Whether the state is a failure message rather than a running phase.
    pub error: bool,
}

/// The pane-level flash status. A flash outlives the row it started on: the unit
/// reboots through its update loader and comes back as a new port, so the rows come and
/// go while the flash is running. The board carries the state across that churn.
type Board = Arc<Mutex<Option<FlashState>>>;

/// One live FM-1, as the devices pane shows it.
#[derive(Clone)]
pub(crate) struct DeviceRow {
    /// The MIDI port name; the device's key for [`Hardware::flash`] and
    /// [`Hardware::set_clock`].
    pub name: String,
    /// The port's short name ("FM-1 MIDI 1").
    pub port: String,
    /// The ALSA client number, the closest thing to a device number ALSA gives.
    pub number: Option<u32>,
    /// The USB bus-port path ("1-2.4"), when ALSA knows it.
    pub usb: Option<String>,
    /// What the row shows: the friendly firmware name, or a state.
    pub display: String,
    /// Whether the metronome's MIDI clock is routed to the device.
    pub clock: bool,
    /// What the running flash is doing, if one is running.
    pub flash: Option<FlashState>,
    /// The last flash's outcome, until the next flash.
    pub outcome: Option<String>,
}

struct Device {
    name: String,
    port: String,
    number: Option<u32>,
    usb: Option<String>,
    /// The stable id the clock uses for this device.
    hw_id: usize,
    link: Arc<DeviceLink>,
    identity: Arc<Mutex<Option<String>>>,
    clock_on: Arc<AtomicBool>,
    flash: Arc<Mutex<Option<FlashState>>>,
    outcome: Arc<Mutex<Option<String>>>,
}

enum Command {
    Flash { name: String, bytes: Vec<u8> },
    SetClock { name: String, on: bool },
}

/// The live USB devices: a background scanner thread owns discovery, the GUI reads the
/// row list and posts commands.
pub(crate) struct Hardware {
    devices: Arc<Mutex<Vec<Device>>>,
    commands: Sender<Command>,
    /// The pane-level flash status, alive across the reboots of a flash.
    board: Board,
    /// Why MIDI could not be initialised at all, if it could not.
    pub error: Option<String>,
}

impl Hardware {
    /// Start the port scanner. It polls the MIDI ports twice a second and probes the
    /// ones that look like an FM-1.
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
        {
            let devices = devices.clone();
            let board = board.clone();
            thread::Builder::new()
                .name("fm1-usb-scan".into())
                .spawn(move || scan_loop(devices, clock, router, board, rx))
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
                    display,
                    clock: device.clock_on.load(Ordering::Relaxed),
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

    /// Route the metronome's MIDI clock to the named device, or stop routing it. The
    /// row's switch flips immediately; the scanner thread does the clock plumbing.
    pub fn set_clock(&self, name: &str, on: bool) {
        if let Ok(list) = self.devices.lock() {
            if let Some(device) = list.iter().find(|device| device.name == name) {
                device.clock_on.store(on, Ordering::Relaxed);
            }
        }
        let _ = self.commands.send(Command::SetClock {
            name: name.to_string(),
            on,
        });
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

fn scan_loop(
    devices: Arc<Mutex<Vec<Device>>>,
    clock: Arc<Clock>,
    router: Routing,
    board: Board,
    commands: Receiver<Command>,
) {
    let mut next_hw_id = 0usize;
    loop {
        // Commands first, so a clock toggle never waits behind a scan.
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::SetClock { name, on } => {
                    let found = devices.lock().ok().and_then(|list| {
                        list.iter()
                            .find(|device| device.name == name)
                            .map(|device| {
                                (
                                    device.hw_id,
                                    device.clock_on.clone(),
                                    device.flash.clone(),
                                    device.link.clone(),
                                )
                            })
                    });
                    if let Some((hw_id, flag, flash, link)) = found {
                        flag.store(on, Ordering::Relaxed);
                        // Never route clock bytes into a running update; the flash
                        // thread re-plugs the device when it is done.
                        let busy = flash.lock().ok().is_some_and(|state| state.is_some());
                        if !busy {
                            let target: Option<Arc<dyn MidiSink>> =
                                on.then_some(link as Arc<dyn MidiSink>);
                            clock.set_hw_target(hw_id, target);
                        }
                    }
                }
                Command::Flash { name, bytes } => {
                    let found = devices.lock().ok().and_then(|list| {
                        list.iter()
                            .find(|device| device.name == name)
                            .map(|device| {
                                (
                                    device.hw_id,
                                    device.link.clone(),
                                    device.identity.clone(),
                                    device.clock_on.clone(),
                                    device.flash.clone(),
                                    device.outcome.clone(),
                                )
                            })
                    });
                    let Some((hw_id, link, identity, clock_on, flash, outcome)) = found else {
                        continue;
                    };
                    {
                        // Claim the device: a state means a flash is already running.
                        let Ok(mut state) = flash.lock() else {
                            continue;
                        };
                        if state.is_some() {
                            continue; // already flashing
                        }
                        *state = Some(FlashState {
                            step: "reading the package…".to_string(),
                            progress: None,
                            error: false,
                        });
                    }
                    if let Ok(mut outcome) = outcome.lock() {
                        *outcome = None;
                    }
                    let clock = clock.clone();
                    let devices = devices.clone();
                    let board = board.clone();
                    thread::Builder::new()
                        .name("fm1-usb-flash".into())
                        .spawn(move || {
                            flash_task(
                                devices, board, link, identity, clock_on, flash, outcome, clock,
                                hw_id, bytes,
                            )
                        })
                        .expect("spawn flash thread");
                }
            }
        }

        scan(&devices, &clock, &router, &mut next_hw_id);
        thread::sleep(Duration::from_millis(500));
    }
}

/// One poll: drop the devices whose port is gone, probe the new FM-1-looking ones.
fn scan(
    devices: &Mutex<Vec<Device>>,
    clock: &Clock,
    router: &Routing,
    next_hw_id: &mut usize,
) {
    let (Ok(input), Ok(output)) = (
        MidiInput::new("fm1-grid-scan"),
        MidiOutput::new("fm1-grid-scan"),
    ) else {
        return; // no MIDI (yet); try again next tick
    };

    let mut seen = Vec::new();
    let mut candidates = Vec::new();
    for port in input.ports() {
        let Ok(name) = input.port_name(&port) else {
            continue;
        };
        seen.push(name.clone());
        let lower = name.to_lowercase();
        if PROBE_NAMES.iter().any(|needle| lower.contains(needle)) {
            candidates.push((name, port));
        }
    }

    // Drop the devices whose port is gone; unplugging also unplugs the clock and the
    // MIDI graph.
    if let Ok(mut list) = devices.lock() {
        list.retain(|device| {
            if seen.iter().any(|name| *name == device.name) {
                true
            } else {
                clock.set_hw_target(device.hw_id, None);
                router.device_gone(&device.name);
                false
            }
        });
    }

    let out_ports = output.ports();
    for (name, in_port) in candidates {
        let known = devices
            .lock()
            .ok()
            .is_some_and(|list| list.iter().any(|device| device.name == name));
        if known {
            continue;
        }
        // Pair the input with the output port of the same name; fall back to the only
        // output there is (the hub's rule).
        let out_port = out_ports
            .iter()
            .find(|port| output.port_name(port).is_ok_and(|n| n == name))
            .or_else(|| (out_ports.len() == 1).then_some(&out_ports[0]));
        let Some(out_port) = out_port else { continue };

        let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
        // connect() consumes its MidiInput/MidiOutput, and ports stay valid across
        // instances (they are ALSA addresses), so each device gets a fresh pair.
        let (Ok(out_mid), Ok(in_mid)) = (
            MidiOutput::new("fm1-grid"),
            MidiInput::new("fm1-grid"),
        ) else {
            continue;
        };
        let Ok(out_conn) = out_mid.connect(out_port, "fm1-grid") else {
            continue; // busy: another app (or the browser flasher) has it
        };
        let Ok(in_conn) = in_mid.connect(
            &in_port,
            "fm1-grid",
            |_time, message, data: &mut CallbackData| {
                if message.first() == Some(&0xF0) {
                    if let Ok(mut queue) = data.queue.lock() {
                        queue.push_back(message.to_vec());
                    }
                }
                data.router.route_from_device(&data.name, message);
            },
            CallbackData {
                queue: queue.clone(),
                router: router.clone(),
                name: name.clone(),
            },
        ) else {
            continue;
        };
        let link = Arc::new(DeviceLink {
            out: Mutex::new(out_conn),
            queue,
            _input: Mutex::new(in_conn),
        });
        // Hand the open link to the MIDI graph: it routes to and from the device
        // through this pair, never opening the port itself.
        router.device_added(&name, link.clone());
        let identity = probe(&link);
        let (port, number) = split_port_name(&name);
        let usb = usb_path(&name);
        let hw_id = *next_hw_id;
        *next_hw_id += 1;
        if let Ok(mut list) = devices.lock() {
            list.push(Device {
                name,
                port,
                number,
                usb,
                hw_id,
                link,
                identity: Arc::new(Mutex::new(identity)),
                clock_on: Arc::new(AtomicBool::new(false)),
                flash: Arc::new(Mutex::new(None)),
                outcome: Arc::new(Mutex::new(None)),
            });
        }
    }
}

/// Ask a freshly opened device who it is: the handshake query, twice, one second each.
fn probe(link: &DeviceLink) -> Option<String> {
    for _ in 0..2 {
        link.drain();
        if !link.send(&HS_QUERY) {
            return None;
        }
        let end = Instant::now() + Duration::from_secs(1);
        while Instant::now() < end {
            let Some(pkt) = link.read(end - Instant::now()) else {
                break;
            };
            if let Some(text) = parse_identity(&pkt) {
                return Some(text);
            }
        }
    }
    None
}

/// The whole update on its own thread, mirroring the firmware hub's one-shot flow:
/// step 1 on the running firmware, wait for the update loader the unit reboots into,
/// step 2 on the loader, wait for the unit to come back, verify the new identity. The
/// phases go to the device's row and to the pane-level board (rows come and go as the
/// unit reboots); the outcome lands on the row of whatever device is left at the end.
#[allow(clippy::too_many_arguments)]
fn flash_task(
    devices: Arc<Mutex<Vec<Device>>>,
    board: Board,
    link: Arc<DeviceLink>,
    identity: Arc<Mutex<Option<String>>>,
    clock_on: Arc<AtomicBool>,
    flash: Arc<Mutex<Option<FlashState>>>,
    outcome: Arc<Mutex<Option<String>>>,
    clock: Arc<Clock>,
    hw_id: usize,
    bytes: Vec<u8>,
) {
    let result = flash_flow(&devices, &board, &link, &identity, &clock, hw_id, bytes, &flash);
    if let Ok(mut state) = flash.lock() {
        *state = None;
    }
    // The device is never clocked during an update; put the routing back afterwards,
    // but only while the original device is still the one registered under this link.
    let registered = devices
        .lock()
        .ok()
        .is_some_and(|list| list.iter().any(|device| Arc::ptr_eq(&device.link, &link)));
    if registered && clock_on.load(Ordering::Relaxed) {
        let target: Option<Arc<dyn MidiSink>> = Some(link.clone() as Arc<dyn MidiSink>);
        clock.set_hw_target(hw_id, target);
    }
    if let Ok(mut outcome) = outcome.lock() {
        *outcome = Some(match &result {
            Ok(product) => format!("flashed: {}", friendly(product)),
            Err(message) => message.clone(),
        });
    }
    // Success clears the board (the returned device's row carries the outcome);
    // failures stay on the board, since the row they belong to may be gone.
    if let Ok(mut state) = board.lock() {
        *state = match &result {
            Ok(_) => None,
            Err(message) => Some(FlashState {
                step: message.clone(),
                progress: None,
                error: true,
            }),
        };
    }
}

/// Publish a phase to the row's flash state (if that row still exists) and the board.
fn publish(
    flash: &Mutex<Option<FlashState>>,
    board: &Board,
    text: &str,
    progress: Option<f32>,
) {
    if let Ok(mut state) = flash.lock() {
        if let Some(state) = state.as_mut() {
            state.step = text.to_string();
            state.progress = progress;
        }
    }
    if let Ok(mut state) = board.lock() {
        *state = Some(FlashState {
            step: text.to_string(),
            progress,
            error: false,
        });
    }
}

/// Publish just the progress (the phase text stays as it is).
fn publish_progress(
    flash: &Mutex<Option<FlashState>>,
    board: &Board,
    progress: f32,
) {
    if let Ok(mut state) = flash.lock() {
        if let Some(state) = state.as_mut() {
            state.progress = Some(progress);
        }
    }
    if let Ok(mut state) = board.lock() {
        if let Some(state) = state.as_mut() {
            state.progress = Some(progress);
        }
    }
}

/// Wait for a device the scanner (re-)registered whose identity matches `pred`, up to
/// `timeout`. Returns its handles, claimed for flashing (row state set, clock unplugged).
#[allow(clippy::too_many_arguments)]
fn wait_for_device(
    devices: &Mutex<Vec<Device>>,
    clock: &Clock,
    board: &Board,
    timeout: Duration,
    pred: impl Fn(&str) -> bool,
) -> Option<(
    Arc<DeviceLink>,
    Arc<Mutex<Option<String>>>,
    Arc<Mutex<Option<FlashState>>>,
    Arc<Mutex<Option<String>>>,
)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(list) = devices.lock() {
            for device in list.iter() {
                let text = device.identity.lock().ok().and_then(|i| i.clone());
                if !text.as_deref().is_some_and(&pred) {
                    continue;
                }
                // Claim it: the row shows the flash state and no clock bytes go in.
                if let Ok(mut state) = device.flash.lock() {
                    *state = Some(FlashState {
                        step: "flashing… do not unplug".to_string(),
                        progress: None,
                        error: false,
                    });
                }
                clock.set_hw_target(device.hw_id, None);
                if let Ok(mut state) = board.lock() {
                    if let Some(state) = state.as_mut() {
                        state.progress = None;
                    }
                }
                return Some((
                    device.link.clone(),
                    device.identity.clone(),
                    device.flash.clone(),
                    device.outcome.clone(),
                ));
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn flash_flow(
    devices: &Mutex<Vec<Device>>,
    board: &Board,
    link: &DeviceLink,
    identity: &Mutex<Option<String>>,
    clock: &Clock,
    hw_id: usize,
    bytes: Vec<u8>,
    flash: &Mutex<Option<FlashState>>,
) -> Result<String, String> {
    // Clock bytes would confuse both the running firmware and the loader.
    clock.set_hw_target(hw_id, None);

    let Some(product) = product_of(&bytes) else {
        return Err("not an FM-1 package: no product identity in the file".to_string());
    };
    let model = product.split_once('_').map(|(model, _)| model).unwrap_or("FM-1");
    let image = logical_image(&bytes);
    let current = identity.lock().ok().and_then(|i| i.clone());
    let is_ota = current
        .as_deref()
        .is_some_and(|text| text.to_lowercase().starts_with("ota-"));
    let current_model = current
        .as_deref()
        .and_then(|text| text.split_once('_'))
        .map(|(model, _)| model.to_string());
    fn strip_ota(text: &str) -> &str {
        text.strip_prefix("ota-")
            .or_else(|| text.strip_prefix("OTA-"))
            .unwrap_or(text)
    }

    if !is_ota {
        // Step 1: the running firmware verifies the package and stages the loader.
        publish(flash, board, "checking the device's firmware…", None);
        let Some(current) = current else {
            return Err(
                "the device did not answer the handshake, so its firmware cannot be \
                 checked: nothing was sent"
                    .to_string(),
            );
        };
        if current_model.as_deref() != Some(model) {
            return Err(format!(
                "the device is {current}, the package is for {product}: nothing was sent"
            ));
        }
        publish(flash, board, "sending the UPGRADE command…", None);
        link.send(&UPGRADE);
        thread::sleep(Duration::from_secs(2));
        publish(flash, board, "staging the update loader…", None);
        let served = serve(link, &image, FINISH_STAGE, Duration::from_secs(8), flash, board)?;
        if !served.finished {
            return Err(format!(
                "the device stopped after {} requests: nothing was written",
                served.count
            ));
        }
        // The device reboots into its update loader and comes back as a new port; the
        // scanner registers it and this thread picks it up (the hub waits 30 s).
        publish(flash, board, "step 1 done: waiting for the update loader…", None);
        if let Ok(mut identity) = identity.lock() {
            *identity = None;
        }
        let loader = wait_for_device(devices, clock, board, Duration::from_secs(30), |text| {
            text.to_lowercase().starts_with("ota-") && strip_ota(text).split_once('_').map(|(m, _)| m) == Some(model)
        });
        let Some((loader_link, _, loader_flash, _)) = loader else {
            return Err(
                "the update loader did not appear within 30 s: replug the USB cable and \
                 press Flash again — the device stays in update mode until the update is \
                 finished"
                    .to_string(),
            );
        };
        let result = write_flash(
            clock, devices, board, &loader_link, &loader_flash, &image, &product,
        );
        return result;
    }

    // Resume: the device is already sitting in its update loader (the user pressed
    // Flash on the loader's row). Step 2 straight away.
    publish(flash, board, "checking the update loader…", None);
    let loader_model = current_model.as_deref().map(strip_ota);
    if loader_model != Some(model) {
        return Err(format!(
            "the loader is for another device than {product}: nothing was written"
        ));
    }
    write_flash(clock, devices, board, link, flash, &image, &product)
}

/// Step 2 on the loader: pull the whole image into the flash, then wait for the device
/// to reboot into the new firmware and verify its identity (the hub waits 40 s).
fn write_flash(
    clock: &Clock,
    devices: &Mutex<Vec<Device>>,
    board: &Board,
    link: &DeviceLink,
    flash: &Mutex<Option<FlashState>>,
    image: &[u8],
    product: &str,
) -> Result<String, String> {
    publish(flash, board, "sending the UPGRADE command…", None);
    link.send(&UPGRADE);
    thread::sleep(Duration::from_secs(2));
    publish(flash, board, "writing the flash…", None);
    let served = serve(link, image, FINISH_WRITE, Duration::from_secs(180), flash, board)?;
    if !served.finished {
        return Err(format!(
            "the loader stopped after {} requests: replug and press Flash again to resume",
            served.count
        ));
    }
    // The write is done; the device reboots into the new firmware and comes back as a
    // new port. Wait for it and verify the identity the handshake reports.
    publish(flash, board, "written: waiting for the device to come back…", None);
    let back = wait_for_device(devices, clock, board, Duration::from_secs(40), |text| {
        !text.to_lowercase().starts_with("ota-")
    });
    match back {
        Some((_, back_identity, back_flash, back_outcome)) => {
            // The claim protected the returning device while it was being verified;
            // it is not flashing, so release the row.
            if let Ok(mut state) = back_flash.lock() {
                *state = None;
            }
            let text = back_identity.lock().ok().and_then(|i| i.clone());
            if text.as_deref() == Some(product) {
                if let Ok(mut outcome) = back_outcome.lock() {
                    *outcome = Some(format!("flashed: {}", friendly(product)));
                }
                Ok(product.to_string())
            } else {
                Err(format!(
                    "the device came back as {}, not {product}: the update may have failed",
                    text.unwrap_or_else(|| "nothing the handshake could read".to_string())
                ))
            }
        }
        None => Err(format!(
            "the device did not come back within 40 s: it should be running {product} — \
             check its row, and replug if it stays dark"
        )),
    }
}

struct Served {
    count: u32,
    finished: bool,
}

/// Answer the device's read requests until it asks for one of the finish addresses,
/// publishing the served fraction of the image to the row and the board.
fn serve(
    link: &DeviceLink,
    image: &[u8],
    finish_addr: u32,
    idle: Duration,
    flash: &Mutex<Option<FlashState>>,
    board: &Board,
) -> Result<Served, String> {
    let mut count = 0u32;
    let mut served_bytes = 0usize;
    let mut last_pct = u16::MAX;
    let mut last = Instant::now();
    loop {
        let Some(pkt) = link.read(Duration::from_secs(1)) else {
            if last.elapsed() > idle {
                return Ok(Served { count, finished: false });
            }
            continue;
        };
        let Some(request) = parse_request(&pkt) else {
            continue;
        };
        last = Instant::now();
        if request.addr == FINISH_STAGE || request.addr == FINISH_WRITE {
            link.send(&response(request.addr, b"success\0", 0));
            if request.addr == finish_addr {
                return Ok(Served { count, finished: true });
            }
            continue;
        }
        let len = request.len as usize;
        let start = request.addr as usize;
        if len > MAXDATA || start + len > image.len() {
            return Err(format!("bad request {:x}+{len}", request.addr));
        }
        // The hub's pacing: let the device settle before each packet.
        thread::sleep(Duration::from_millis(10));
        if !link.send(&response(request.addr, &image[start..start + len], request.fl)) {
            return Err("the device was disconnected mid-update".to_string());
        }
        count += 1;
        served_bytes += len;
        // Publish on whole percent steps only; the pane repaints anyway.
        let pct = (served_bytes * 100 / image.len().max(1)) as u16;
        if pct != last_pct {
            last_pct = pct;
            publish_progress(flash, board, (pct as f32 / 100.0).min(1.0));
        }
    }
}
