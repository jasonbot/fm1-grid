use super::*;

/// A SysEx queue, filled by the MIDI-input callback thread.
use fm1_base::clock::Clock;
use fm1_base::routing::Routing;

/// How long a silent port waits before another read-only handshake.
const PROBE_RETRY: Duration = Duration::from_secs(30);

pub(crate) type Queue = Arc<Mutex<VecDeque<Vec<u8>>>>;

/// The input callback's data: the SysEx queue for the update protocol, plus the MIDI
/// graph's router, so every message the device sends also flows through the graph.
pub(crate) struct CallbackData {
    queue: Queue,
    router: Routing,
    name: String,
}

/// One open MIDI in/out pair with a live device. Shared by the clock thread (single
/// real-time bytes), the flash thread (packets), the scanner (handshakes) and the MIDI
/// graph (whole messages, both directions).
pub struct DeviceLink {
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
    pub(crate) fn read(&self, timeout: Duration) -> Option<Vec<u8>> {
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

    pub(crate) fn drain(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }
}

impl MidiSink for DeviceLink {
    fn send_byte(&self, b: u8) {
        self.send(&[b]);
    }

    fn send(&self, bytes: &[u8]) {
        DeviceLink::send(self, bytes);
    }
}

pub(crate) fn scan_loop(
    devices: Arc<Mutex<Vec<Device>>>,
    clock: Arc<Clock>,
    router: Routing,
    board: Board,
    commands: Receiver<Command>,
    rescue: Arc<AtomicBool>,
) {
    let mut next_hw_id = 0usize;
    // Ports that opened but did not answer the handshake. Probed once per appearance;
    // a replug clears the entry.
    let mut failed: HashMap<String, Instant> = HashMap::new();
    loop {
        // Commands first, so a clock toggle never waits behind a scan.
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::RescueFlash { name, bytes } => {
                    // One rescue at a time: while it runs the scanner registers no
                    // new devices, so no clock bytes or graph routes hit the port
                    // mid-update (the unit reboots under names nobody knows).
                    if rescue.swap(true, Ordering::SeqCst) {
                        continue; // a rescue flash is already running
                    }
                    let board = board.clone();
                    let router = router.clone();
                    let active = rescue.clone();
                    thread::Builder::new()
                        .name("fm1-usb-rescue".into())
                        .spawn(move || {
                            rescue::rescue_task(&name, bytes, &board, &router);
                            active.store(false, Ordering::SeqCst);
                        })
                        .expect("spawn rescue flash thread");
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
                                    device.flash.clone(),
                                    device.outcome.clone(),
                                )
                            })
                    });
                    let Some((hw_id, link, identity, flash, outcome)) = found else {
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
            log: Vec::new(),
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
                                devices, board, link, identity, flash, outcome, clock,
                                hw_id, bytes,
                            )
                        })
                        .expect("spawn flash thread");
                }
            }
        }

        scan(&devices, &clock, &router, &mut next_hw_id, &rescue, &mut failed);
        thread::sleep(Duration::from_millis(500));
    }
}

/// One poll: drop the devices whose port is gone, ask every new port who it is, and
/// register only the ones that answer the FM-1 handshake. While a rescue flash runs,
/// no new devices are registered: the rescue target reboots through its update loader
/// and a freshly registered device gets clocked — clock bytes would corrupt the update.
pub(crate) fn scan(
    devices: &Mutex<Vec<Device>>,
    clock: &Clock,
    router: &Routing,
    next_hw_id: &mut usize,
    rescue: &AtomicBool,
    failed: &mut HashMap<String, Instant>,
) {
    let Ok(input) = MidiInput::new("fm1-grid-scan") else {
        return; // no MIDI (yet); try again next tick
    };

    let mut seen = Vec::new();
    let mut candidates = Vec::new();
    for port in input.ports() {
        let Ok(name) = input.port_name(&port) else {
            continue;
        };
        seen.push(name.clone());
        if !rescue::ours(&name) && !failed.contains_key(&name) {
            candidates.push(name);
        }
    }
    // Keep names that are still present and still cooling down; a slow device gets
    // another handshake after the retry interval instead of staying invisible until
    // it is unplugged.
    failed.retain(|name, at| seen.contains(name) && at.elapsed() < PROBE_RETRY);

    // Drop the devices whose port is gone; unplugging also unplugs the clock and the
    // MIDI graph.
    if let Ok(mut list) = devices.lock() {
        list.retain(|device| {
            if seen.contains(&device.name) {
                true
            } else {
                clock.set_hw_target(device.hw_id, None);
                router.device_gone(&device.name);
                false
            }
        });
    }

    if rescue.load(Ordering::Relaxed) {
        return; // a rescue flash owns every new port for its duration
    }
    for name in candidates {
        let known = devices
            .lock()
            .ok()
            .is_some_and(|list| list.iter().any(|device| device.name == name));
        if known {
            continue;
        }
        let Some(link) = open_link(&name, router, false) else {
            continue; // busy: another app (or the browser flasher) has it
        };
        let Some(identity) = probe(&link) else {
            // Not an FM-1/SMK: keep the port, but do not add a row, route or clock.
            failed.insert(name, Instant::now());
            continue;
        };
        // Hand the open link to the MIDI graph: it routes to and from the device
        // through this pair, never opening the port itself.
        router.device_added(&name, link.clone());
        let (port, number) = split_port_name(&name);
        let usb = usb_path(&name);
        let hw_id = *next_hw_id;
        *next_hw_id += 1;
        // Every connected unit gets the metronome's clock (the clock thread gates on
        // the metronome's switch); a flash unplugs it for the update's duration.
        clock.set_hw_target(hw_id, Some(link.clone()));
        if let Ok(mut list) = devices.lock() {
            list.push(Device {
                name,
                port,
                number,
                usb,
                hw_id,
                link,
                identity: Arc::new(Mutex::new(Some(identity))),
                flash: Arc::new(Mutex::new(None)),
                outcome: Arc::new(Mutex::new(None)),
            });
        }
    }
}

/// Open the in/out pair for one MIDI port, by name. The input pairs with the output
/// port of the same name, falling back to the only output there is (the hub's rule).
/// The returned link is not registered anywhere: the caller decides whether it joins
/// the MIDI graph (the scanner does) or stays a private flash channel (rescue does).
pub(crate) fn open_link(name: &str, router: &Routing, fallback: bool) -> Option<Arc<DeviceLink>> {
    let (lookup_in, lookup_out) = (
        MidiInput::new("fm1-grid-scan").ok()?,
        MidiOutput::new("fm1-grid-scan").ok()?,
    );
    let in_port = lookup_in
        .ports()
        .into_iter()
        .find(|port| lookup_in.port_name(port).is_ok_and(|n| n == name))?;
    let out_ports = lookup_out.ports();
    let out_port = out_ports
        .iter()
        .find(|port| lookup_out.port_name(port).is_ok_and(|n| n == name))
        .or_else(|| (fallback && out_ports.len() == 1).then_some(&out_ports[0]))?
        .clone();

    let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
    // connect() consumes its MidiInput/MidiOutput, and ports stay valid across
    // instances (they are ALSA addresses), so each device gets a fresh pair.
    let (out_mid, in_mid) = (
        MidiOutput::new("fm1-grid").ok()?,
        MidiInput::new("fm1-grid").ok()?,
    );
    let out_conn = out_mid.connect(&out_port, "fm1-grid").ok()?;
    let in_conn = in_mid
        .connect(
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
                name: name.to_string(),
            },
        )
        .ok()?;
    Some(Arc::new(DeviceLink {
        out: Mutex::new(out_conn),
        queue,
        _input: Mutex::new(in_conn),
    }))
}

/// Ask a freshly opened device who it is: the handshake query, twice, one second each.
pub(crate) fn probe(link: &DeviceLink) -> Option<String> {
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
