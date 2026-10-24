//! The MIDI graph: connects MIDI sources to any number of MIDI sinks.
//!
//! Sources are the system MIDI ports that deliver bytes (keyboards, sequencers, …) and
//! the live FM-1 units on USB (their USB-MIDI port is bidirectional; bytes they send are
//! picked up by the input callback the hardware module already owns, so no port is ever
//! opened twice). Sinks are the system MIDI ports, the MIDI IN jacks of the running
//! emulator panes (`Action::Midi`, one byte at a time, exactly like the metronome feeds
//! them) and the live FM-1 units.
//!
//! An emulated pane is a sink only here, but that is an emulator limit, not a firmware
//! one: the FM-1's TRS jack is MIDI-in alone and its firmwares instead talk MIDI both
//! ways over USB-C — their images carry a USB MIDIStreaming interface (see
//! [`crate::usbmidi`]), which is how the live units below get their bidirectional ports.
//! The emulated SoC stubs the USB controller at register level (and UART1 TX is not
//! wired either), so nothing a pane's firmware sends ever leaves it. The graph UI says
//! so where a running pane advertises USB-MIDI.
//!
//! A poller thread refreshes the system port list twice a second (the same rhythm the
//! hardware scanner uses) and keeps one MIDI input connection open per system source
//! that has links; those connections' callbacks do the routing itself: one lock, then a
//! send per sink. The metronome's clock routing is separate from this graph; a target
//! in both simply receives the merged bytes.

use crate::hardware::DeviceLink;
use fm1_soc::Action;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// One endpoint of the graph.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum Port {
    /// A system MIDI port, by its midir (ALSA) name.
    System(String),
    /// A live FM-1 unit, by the hardware module's port name; both a source and a sink.
    Device(String),
    /// An emulator pane's MIDI IN jack; a sink only.
    Pane(usize),
}

/// What the graph UI shows about one endpoint.
#[derive(Clone)]
pub(crate) struct Endpoint {
    pub port: Port,
    pub label: String,
    /// Whether bytes from this endpoint can be routed somewhere.
    pub source: bool,
    /// Whether something can be routed to this endpoint.
    pub sink: bool,
}

/// One open system source: the connection whose callback routes its bytes.
struct OpenSource {
    name: String,
    /// Kept alive: dropping it closes the port and stops the callback.
    _connection: MidiInputConnection<(Routing, String)>,
}

struct Inner {
    /// The graph: source -> sinks, in click order.
    links: HashMap<Port, Vec<Port>>,
    /// The registered live FM-1 units, by port name (the hardware module registers them).
    devices: HashMap<String, Arc<DeviceLink>>,
    /// The running emulator panes: slot -> (label, MIDI IN).
    panes: HashMap<usize, (String, Sender<Action>)>,
    /// The system ports that deliver bytes, from the poller.
    system_ins: Vec<String>,
    /// The system ports that accept bytes, from the poller.
    system_outs: Vec<String>,
    /// Open output connections to system sinks, shared with the routing callbacks.
    outputs: HashMap<String, Arc<Mutex<MidiOutputConnection>>>,
}

enum Command {
    Connect { source: Port, sink: Port },
    Disconnect { source: Port, sink: Port },
}

/// Handle to the MIDI graph: the GUI reads the endpoints and links and posts
/// connect/disconnect commands; the hardware module registers devices and the app
/// registers emulator panes. Cloning is cheap.
#[derive(Clone)]
pub(crate) struct Routing {
    inner: Arc<Mutex<Inner>>,
    commands: Sender<Command>,
}

impl Routing {
    /// Start the graph's poller thread.
    pub fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            links: HashMap::new(),
            devices: HashMap::new(),
            panes: HashMap::new(),
            system_ins: Vec::new(),
            system_outs: Vec::new(),
            outputs: HashMap::new(),
        }));
        let (tx, rx) = mpsc::channel::<Command>();
        let routing = Self { inner, commands: tx };
        {
            let routing = routing.clone();
            thread::Builder::new()
                .name("fm1-midi-graph".into())
                .spawn(move || poll_loop(routing, rx))
                .expect("spawn MIDI graph thread");
        }
        routing
    }

    /// Route one message from a source to every sink it is connected to. Called from
    /// the MIDI input callbacks (system sources and live devices).
    fn route(&self, source: &Port, bytes: &[u8]) {
        let Ok(inner) = self.inner.lock() else {
            return;
        };
        let Some(sinks) = inner.links.get(source) else {
            return;
        };
        for sink in sinks {
            match sink {
                Port::System(name) => {
                    if let Some(out) = inner.outputs.get(name) {
                        if let Ok(mut out) = out.lock() {
                            let _ = out.send(bytes);
                        }
                    }
                }
                Port::Device(name) => {
                    if let Some(link) = inner.devices.get(name) {
                        link.send(bytes);
                    }
                }
                Port::Pane(slot) => {
                    if let Some((_, tx)) = inner.panes.get(slot) {
                        for &b in bytes {
                            let _ = tx.send(Action::Midi { b });
                        }
                    }
                }
            }
        }
    }

    /// Route one message from a live FM-1 unit; called by the hardware module's input
    /// callback, so device bytes flow through the graph without a second open port.
    pub fn route_from_device(&self, name: &str, bytes: &[u8]) {
        self.route(&Port::Device(name.to_string()), bytes);
    }

    /// Connect a source to one more sink (on the poller thread, which owns the system
    /// port connections).
    pub fn connect(&self, source: Port, sink: Port) {
        if source == sink {
            return;
        }
        let _ = self.commands.send(Command::Connect { source, sink });
    }

    pub fn disconnect(&self, source: Port, sink: Port) {
        let _ = self.commands.send(Command::Disconnect { source, sink });
    }

    /// Register (or with None unregister) a running emulator pane as a MIDI IN sink.
    /// Links to an unregistered pane are dropped.
    pub fn set_pane(&self, slot: usize, sink: Option<(String, Sender<Action>)>) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        match sink {
            Some(sink) => {
                inner.panes.insert(slot, sink);
            }
            None => {
                inner.panes.remove(&slot);
                prune(&mut inner, &Port::Pane(slot));
            }
        }
    }

    /// Register a live FM-1 unit, so the graph can route to it and from it without
    /// opening its port a second time.
    pub fn device_added(&self, name: &str, link: Arc<DeviceLink>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.devices.insert(name.to_string(), link);
        }
    }

    /// Unregister a live FM-1 unit; links from and to it are dropped.
    pub fn device_gone(&self, name: &str) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.devices.remove(name);
        prune(&mut inner, &Port::Device(name.to_string()));
    }

    /// Every endpoint the graph can currently use, for the UI's dropdowns.
    pub fn endpoints(&self) -> Vec<Endpoint> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in &inner.system_ins {
            out.push(Endpoint {
                port: Port::System(name.clone()),
                label: label_of(name),
                source: true,
                sink: false,
            });
        }
        for name in &inner.system_outs {
            out.push(Endpoint {
                port: Port::System(name.clone()),
                label: label_of(name),
                source: false,
                sink: true,
            });
        }
        for (slot, (label, _)) in &inner.panes {
            out.push(Endpoint {
                port: Port::Pane(*slot),
                label: label.clone(),
                source: false,
                sink: true,
            });
        }
        for name in inner.devices.keys() {
            out.push(Endpoint {
                port: Port::Device(name.clone()),
                label: format!("FM-1 · {}", label_of(name)),
                source: true,
                sink: true,
            });
        }
        out
    }

    /// The graph as (source, sink) pairs, for the UI's rows.
    pub fn links(&self) -> Vec<(Port, Port)> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (source, sinks) in &inner.links {
            for sink in sinks {
                out.push((source.clone(), sink.clone()));
            }
        }
        out
    }
}

/// Drop every link from or to a dead endpoint.
fn prune(inner: &mut Inner, dead: &Port) {
    inner.links.retain(|source, sinks| {
        if source == dead {
            return false;
        }
        sinks.retain(|sink| sink != dead);
        true
    });
}

/// midir's ALSA port names are "CLIENT:CNUM PORT:PNUM"; the friendly part is the client
/// and port names without the numbers.
pub(crate) fn label_of(name: &str) -> String {
    let mut parts = name.split(' ');
    let client = parts.next().unwrap_or(name);
    let client = client.split(':').next().unwrap_or(client);
    match parts.next() {
        Some(port) => {
            let port = port.rsplit(':').next().unwrap_or(port);
            format!("{client} · {port}")
        }
        None => client.to_string(),
    }
}

/// Whether a port name is one of this app's own midir clients; the poller skips those.
fn is_own_client(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("fm1-grid") || lower.contains("fm1-midi-graph")
}

fn poll_loop(routing: Routing, commands: Receiver<Command>) {
    let mut open: Vec<OpenSource> = Vec::new();
    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Connect { source, sink } => {
                    if let Ok(mut inner) = routing.inner.lock() {
                        let sinks = inner.links.entry(source).or_default();
                        if !sinks.contains(&sink) {
                            sinks.push(sink);
                        }
                    }
                }
                Command::Disconnect { source, sink } => {
                    if let Ok(mut inner) = routing.inner.lock() {
                        if let Some(sinks) = inner.links.get_mut(&source) {
                            sinks.retain(|s| *s != sink);
                            if sinks.is_empty() {
                                inner.links.remove(&source);
                            }
                        }
                    }
                }
            }
        }

        poll(&routing, &mut open);
        thread::sleep(Duration::from_millis(500));
    }
}

/// One poll: refresh the system port lists, drop the links and connections of ports
/// that are gone, and keep one input connection open per system source with links.
fn poll(routing: &Routing, open: &mut Vec<OpenSource>) {
    // Enumeration instances are short-lived; ports are ALSA addresses and stay valid
    // for the connections made below.
    let (Ok(input), Ok(output)) = (
        MidiInput::new("fm1-midi-graph"),
        MidiOutput::new("fm1-midi-graph"),
    ) else {
        return; // no MIDI (yet); try again next tick
    };
    let mut system_ins = Vec::new();
    let mut system_outs = Vec::new();
    {
        let Ok(inner) = routing.inner.lock() else {
            return;
        };
        for port in input.ports() {
            let Ok(name) = input.port_name(&port) else {
                continue;
            };
            if is_own_client(&name) || inner.devices.contains_key(&name) {
                continue; // our own clients, and the live FM-1s (Device endpoints)
            }
            system_ins.push(name);
        }
        for port in output.ports() {
            let Ok(name) = output.port_name(&port) else {
                continue;
            };
            if is_own_client(&name) || inner.devices.contains_key(&name) {
                continue;
            }
            system_outs.push(name);
        }
    }

    if let Ok(mut inner) = routing.inner.lock() {
        // Drop the links of ports that are gone (plugged out, or closed by their app).
        let gone = |port: &Port| match port {
            Port::System(name) => {
                !system_ins.contains(name)
                    && !system_outs.contains(name)
                    && !inner.devices.contains_key(name)
            }
            Port::Device(name) => !inner.devices.contains_key(name),
            Port::Pane(slot) => !inner.panes.contains_key(slot),
        };
        let mut dead: Vec<Port> = Vec::new();
        for (source, sinks) in inner.links.iter() {
            if gone(source) {
                dead.push(source.clone());
            }
            for sink in sinks {
                if gone(sink) {
                    dead.push(sink.clone());
                }
            }
        }
        for dead in &dead {
            prune(&mut inner, dead);
        }
        // Drop the output connections of sinks that are gone or unlinked.
        let wanted: Vec<String> = inner
            .links
            .values()
            .flatten()
            .filter_map(|sink| match sink {
                Port::System(name) => Some(name.clone()),
                _ => None,
            })
            .collect();
        inner.outputs.retain(|name, _| wanted.contains(name));
        // Open the output connections that new system sinks need.
        for name in &wanted {
            if inner.outputs.contains_key(name) || !system_outs.contains(name) {
                continue;
            }
            if let Some(port) = output
                .ports()
                .into_iter()
                .find(|port| output.port_name(port).is_ok_and(|n| n == *name))
            {
                // A fresh MidiOutput per connection: connect() consumes it.
                if let Ok(fresh) = MidiOutput::new("fm1-midi-graph") {
                    if let Ok(connection) = fresh.connect(&port, "fm1-midi-graph") {
                        inner
                            .outputs
                            .insert(name.clone(), Arc::new(Mutex::new(connection)));
                    }
                }
            }
        }
        inner.system_ins = system_ins.clone();
        inner.system_outs = system_outs.clone();
    }

    // Close the input connections of sources that no longer have links.
    let wanted_sources: Vec<String> = routing
        .inner
        .lock()
        .map(|inner| {
            inner
                .links
                .keys()
                .filter_map(|source| match source {
                    Port::System(name) => Some(name.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    open.retain(|source| wanted_sources.contains(&source.name));
    for name in &wanted_sources {
        if open.iter().any(|source| &source.name == name) || !system_ins.contains(name) {
            continue;
        }
        let Some(port) = input
            .ports()
            .into_iter()
            .find(|port| input.port_name(port).is_ok_and(|n| n == *name))
        else {
            continue;
        };
        // A fresh MidiInput per connection: connect() consumes it.
        let Ok(fresh) = MidiInput::new("fm1-midi-graph") else {
            continue;
        };
        let callback_data = (routing.clone(), name.clone());
        let Ok(connection) = fresh.connect(
            &port,
            "fm1-midi-graph",
            |_time, message, (routing, name): &mut (Routing, String)| {
                routing.route(&Port::System(name.clone()), message);
            },
            callback_data,
        ) else {
            continue; // busy: another app has the port
        };
        open.push(OpenSource { name: name.clone(), _connection: connection });
    }
}

/// A short label for an emulator pane's MIDI IN, for the graph UI.
pub(crate) fn pane_label(slot: usize, label: Option<&String>) -> String {
    format!("Pane {} · {}", slot + 1, label.map_or("no firmware".into(), |l| l.clone()))
}
