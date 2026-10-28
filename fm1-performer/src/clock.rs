//! A metronome that sends MIDI clock (0xF8) to every loaded emulator's MIDI IN jack.
//!
//! MIDI clock is 24 ticks per quarter note. A dedicated thread keeps the beat, so the
//! timing does not depend on the GUI's frame rate. The GUI reads the tempo and beat
//! counters from atomics.

use fm1_soc::Action;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The usable tempo range, in beats per minute.
pub const MIN_BPM: u32 = 20;
pub const MAX_BPM: u32 = 300;
pub const DEFAULT_BPM: u32 = 120;

/// A destination for raw MIDI bytes: a live FM-1 on USB. Implemented by the hardware
/// module; the clock only ever pushes single real-time bytes (0xF8 / 0xFA / 0xFC).
pub trait MidiSink: Send + Sync {
    fn send_byte(&self, b: u8);
}

/// One real-time byte to every emulator slot and every USB device on the clock.
fn broadcast(
    targets: &Mutex<Vec<Option<Sender<Action>>>>,
    hw_targets: &Mutex<Vec<Option<Arc<dyn MidiSink>>>>,
    b: u8,
) {
    if let Ok(targets) = targets.lock() {
        for tx in targets.iter().flatten() {
            // An unbounded channel send only fails if the receiver is gone.
            let _ = tx.send(Action::Midi { b });
        }
    }
    if let Ok(hw) = hw_targets.lock() {
        for sink in hw.iter().flatten() {
            sink.send_byte(b);
        }
    }
}

/// The per-slot MIDI targets the clock sends to.
type SlotTargets = Arc<Mutex<Vec<Option<Sender<Action>>>>>;
/// The live USB units on the clock, by device id.
type HwTargets = Arc<Mutex<Vec<Option<Arc<dyn MidiSink>>>>>;

pub struct Clock {
    bpm: Arc<AtomicU32>,
    /// Number of complete quarter notes (beats) since startup.
    beat: Arc<AtomicU64>,
    /// MIDI clock ticks sent so far (24 per beat).
    tick: Arc<AtomicU64>,
    /// Whether the metronome is running. Off by default.
    enabled: Arc<AtomicBool>,
    /// Set to false when the app is shutting down.
    running: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    /// One optional sender per grid slot. Wrapped in a Mutex because slots are reloaded
    /// from the GUI thread while the clock thread is sending.
    targets: SlotTargets,
    /// The live USB devices on the clock, indexed by the hardware module's device ids.
    hw_targets: HwTargets,
    thread: Option<JoinHandle<()>>,
}

impl Clock {
    pub fn start(slot_count: usize) -> Self {
        let slot_count = slot_count.max(1);
        let bpm = Arc::new(AtomicU32::new(DEFAULT_BPM));
        let beat = Arc::new(AtomicU64::new(0));
        let tick = Arc::new(AtomicU64::new(0));
        let enabled = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let quit = Arc::new(AtomicBool::new(false));
        let targets: SlotTargets = Arc::new(Mutex::new(vec![None; slot_count]));
        let hw_targets: HwTargets = Arc::new(Mutex::new(Vec::new()));

        let thread = {
            let bpm = bpm.clone();
            let beat = beat.clone();
            let tick = tick.clone();
            let enabled = enabled.clone();
            let running = running.clone();
            let quit = quit.clone();
            let targets = targets.clone();
            let hw_targets = hw_targets.clone();
            thread::Builder::new()
                .name("fm1-midi-clock".into())
                .spawn(move || {
                    clock_loop(bpm, beat, tick, enabled, running, quit, targets, hw_targets)
                })
                .expect("spawn MIDI clock thread")
        };

        Self {
            bpm,
            beat,
            tick,
            enabled,
            running,
            quit,
            targets,
            hw_targets,
            thread: Some(thread),
        }
    }

    pub fn bpm(&self) -> u32 {
        self.bpm.load(Ordering::Relaxed)
    }

    pub fn set_bpm(&self, bpm: u32) {
        self.bpm
            .store(bpm.clamp(MIN_BPM, MAX_BPM), Ordering::Relaxed);
    }

    /// True on every other beat: the little on/off light in the toolbar.
    pub fn beat_light(&self) -> bool {
        self.enabled.load(Ordering::Relaxed) && self.beat.load(Ordering::Relaxed) % 2 == 1
    }

    /// Whether the metronome is currently sending MIDI clock.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Start or stop the metronome. The clock thread sends MIDI Start (`0xFA`) or Stop
    /// (`0xFC`) to every connected slot on the next pass.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// The MIDI clock tick counter (24 per beat), handy for debugging.
    #[allow(dead_code)]
    pub fn ticks(&self) -> u64 {
        self.tick.load(Ordering::Relaxed)
    }

    /// Point one grid slot at an emulator. `None` unplugs it.
    ///
    /// When a new emulator is connected we send MIDI Start (0xFA) once. Some firmwares
    /// use Start/Stop to reset their sequencer; others only look at the 0xF8 ticks.
    pub fn set_target(&self, slot: usize, target: Option<Sender<Action>>) {
        let mut targets = self.targets.lock().unwrap();
        if targets.len() <= slot {
            targets.resize(slot + 1, None);
        }
        if let Some(tx) = &target {
            // A newly plugged emulator joins the running clock immediately; a stopped clock
            // sends Start when it is switched on.
            if self.enabled() {
                let _ = tx.send(Action::Midi { b: 0xFA });
            }
        }
        targets[slot] = target;
    }

    /// Plug a live USB device into (or out of) the clock. Like an emulator, a newly
    /// plugged device joins a running clock with MIDI Start (0xFA).
    pub fn set_hw_target(&self, id: usize, target: Option<Arc<dyn MidiSink>>) {
        let mut hw = self.hw_targets.lock().unwrap();
        if hw.len() <= id {
            hw.resize(id + 1, None);
        }
        if let Some(sink) = &target {
            if self.enabled() {
                sink.send_byte(0xFA);
            }
        }
        hw[id] = target;
    }

    /// Stop the clock thread and send MIDI Stop (0xFC) to every still-connected slot.
    pub fn stop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        self.running.store(false, Ordering::Relaxed);
        broadcast(&self.targets, &self.hw_targets, 0xFC);
        self.quit.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn clock_loop(
    bpm: Arc<AtomicU32>,
    beat: Arc<AtomicU64>,
    tick: Arc<AtomicU64>,
    enabled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    targets: SlotTargets,
    hw_targets: HwTargets,
) {
    let mut next = Instant::now();
    let mut was_enabled = false;
    while !quit.load(Ordering::Relaxed) {
        if !running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let on = enabled.load(Ordering::Relaxed);
        if on != was_enabled {
            was_enabled = on;
            // A start always begins on beat one; stop leaves the light dark.
            beat.store(0, Ordering::Relaxed);
            tick.store(0, Ordering::Relaxed);
            next = Instant::now();
            let message = if on { 0xFA } else { 0xFC };
            broadcast(&targets, &hw_targets, message);
            if !on {
                continue;
            }
        }
        if !on {
            // Idle cheaply; the toggle is picked up within a few milliseconds.
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let tempo = bpm.load(Ordering::Relaxed).clamp(MIN_BPM, MAX_BPM);
        // 24 ticks per quarter note, tempo in beats per minute.
        let interval = Duration::from_secs_f64(60.0 / tempo as f64 / 24.0);
        next += interval;

        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else if now.duration_since(next) > Duration::from_millis(100) {
            // The machine was suspended or the thread was descheduled for a long
            // time. Resync instead of trying to catch up with a burst of ticks.
            next = now;
        }

        // The switch may have been flipped while we slept.
        if !enabled.load(Ordering::Relaxed) || quit.load(Ordering::Relaxed) {
            continue;
        }

        broadcast(&targets, &hw_targets, 0xF8);

        let count = tick.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(24) {
            beat.fetch_add(1, Ordering::Relaxed);
        }
    }
}
