//! One emulated FM-1 per grid slot.
//!
//! Each emulator owns a thread, a [`Soc`], and its cores. The thread runs the firmware
//! paced to the wall clock, pushes audio into the slot's [`AudioRing`], and publishes the
//! LCD framebuffer, front-panel LED brightness and a one-line status for the GUI.

use crate::audio::{AudioRing, MAX_QUEUED};
use fm1_core::{Bus, Cpu};
use fm1_soc::{machine, Action, Board, Soc, CPU_HZ, RAM_BASE};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The FM-1's app binary is mapped here; this is also the usual entry point for raw images.
const ENTRY: u32 = 0x0200_0120;
const RAM_SIZE: usize = 1 << 20;
/// How far emulated time runs ahead of the wall clock before audio starts.
const LEAD_MS: f64 = 60.0;
/// Once the firmware streams audio, run whenever less than this much sound is queued.
const AUDIO_TARGET_MS: f64 = 50.0;
/// The SPL's boot-parameter block.
const PARAMS: u32 = RAM_BASE + 0x7fc00;

/// 44.1 kHz stereo i16 is 88.2 samples per millisecond.
const SAMPLES_PER_MS: f64 = 88.2;
/// The dropout health window: only drops inside the trailing five seconds count.
const DROP_WINDOW: Duration = Duration::from_secs(5);

pub const LCD_W: usize = 240;
pub const LCD_H: usize = 240;

/// The framebuffer shared with the GUI, RGB565.
pub type SharedLcd = Arc<Mutex<Vec<u16>>>;

struct Machine {
    cpu: Cpu,
    /// The second core, from when cpu0 releases it.
    cpu1: Option<Cpu>,
    soc: Soc,
    pending_tick: bool,
}

impl Machine {
    /// Build a machine from a `.fwsc` update package (a community firmware or M-VAVE's own).
    fn from_package(bytes: &[u8]) -> Result<Self, String> {
        let stock = fm1_soc::stock_from_package(bytes, None)?;
        if stock.board != Board::Fm1 {
            return Err(
                "this package is for the SMK-37; only FM-1 firmware can be emulated here".into(),
            );
        }
        let mut soc = Soc::new(RAM_SIZE, stock.xip.clone());
        soc.periph.hz = CPU_HZ;
        soc.periph.skip_idle = true;

        let mut cpu = Cpu::new(stock.entry);
        cpu.jit_on = true;
        // The SPL hands cpu0 over in user mode (as fm1-live does).
        cpu.user = true;
        soc.write32(PARAMS, PARAMS + 0x40);
        cpu.regs[0] = fm1_soc::stock_boot(&mut soc, &stock.flash, PARAMS, stock.board);
        Ok(Self { cpu, cpu1: None, soc, pending_tick: false })
    }

    /// Build a machine from a raw app image (`.bin` / `.xip`), mapped at XIP_BASE.
    fn from_raw(image: Vec<u8>) -> Result<Self, String> {
        if image.is_empty() {
            return Err("the firmware image is empty".into());
        }
        let mut soc = Soc::new(RAM_SIZE, image);
        soc.periph.hz = CPU_HZ;
        soc.periph.skip_idle = true;

        let mut cpu = Cpu::new(ENTRY);
        cpu.jit_on = true;
        cpu.user = true;
        soc.write32(PARAMS, PARAMS + 0x40);
        cpu.regs[0] = PARAMS;
        Ok(Self { cpu, cpu1: None, soc, pending_tick: false })
    }

    /// Run `n` instructions of cpu0, and cpu1 alongside once it is running.
    fn run(&mut self, n: u32) -> Result<(), String> {
        let jit = self.cpu.jit_on;
        let Self { cpu, cpu1, soc, pending_tick } = self;
        machine::run_dual(cpu1, soc, n, RAM_SIZE, jit, |soc, k| {
            machine::run_blocks(cpu, soc, k, pending_tick)
        })
    }
}

/// A running emulator. Dropping the handle stops its thread.
pub struct EmulatorHandle {
    pub lcd: SharedLcd,
    pub tx: Sender<Action>,
    /// Brightness (0..1) of each of the 41 front-panel LEDs, as the firmware drives them.
    leds: Arc<Mutex<[f32; 41]>>,
    status: Arc<Mutex<String>>,
    /// Audio dropouts inside the trailing five-second window, refreshed twice a second.
    drops_5s: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    /// The SPI flash's contents once the thread has stopped (the emulated device's
    /// persistent storage, for pinned slots).
    snapshot: Arc<Mutex<Option<Vec<u8>>>>,
}

impl EmulatorHandle {
    /// Start a firmware image. `package` selects the `.fwsc` loader; otherwise the bytes are
    /// treated as a raw XIP image.
    pub fn start(
        name: String,
        bytes: Vec<u8>,
        package: bool,
        ring: Arc<AudioRing>,
        audio_enabled: bool,
        pin_cpu: Option<usize>,
        resume_flash: Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let mut engine = if package {
            Machine::from_package(&bytes)?
        } else {
            Machine::from_raw(bytes)?
        };
        if let Some(saved) = resume_flash {
            // A pinned slot's device storage comes back: boot the firmware fresh, but
            // with its flash chip's contents from the last run.
            engine.soc.flash.mem = saved;
        }

        let lcd: SharedLcd = Arc::new(Mutex::new(vec![0u16; LCD_W * LCD_H]));
        let leds = Arc::new(Mutex::new([0.0f32; 41]));
        let status = Arc::new(Mutex::new("booting".to_string()));
        let drops_5s = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel::<Action>();
        let snapshot = Arc::new(Mutex::new(None));

        let thread = {
            let lcd = lcd.clone();
            let leds = leds.clone();
            let status = status.clone();
            let drops_5s = drops_5s.clone();
            let running = running.clone();
            let snapshot = snapshot.clone();
            thread::Builder::new()
                .name(format!("fm1-{name}"))
                .spawn(move || {
                    // Pin each emulator to its own physical core when Linux exposes the
                    // topology; the scheduler otherwise tends to stack two on one core.
                    if let Some(cpu) = pin_cpu {
                        crate::affinity::pin_current_thread(cpu);
                    }
                    run_emulator(
                        engine,
                        rx,
                        ring,
                        lcd,
                        leds,
                        status,
                        drops_5s,
                        running,
                        audio_enabled,
                        snapshot,
                    )
                })
                .map_err(|e| format!("cannot start emulator thread: {e}"))?
        };

        Ok(Self {
            lcd,
            tx,
            leds,
            status,
            drops_5s,
            running,
            join: Some(thread),
            snapshot,
        })
    }

    /// Stop the thread and take the emulated device's flash contents (None if the
    /// thread never produced one — e.g. it halted at boot).
    pub fn stop_with_snapshot(&mut self) -> Option<Vec<u8>> {
        self.stop();
        self.snapshot.lock().ok()?.take()
    }

    pub fn send(&self, action: Action) {
        // The channel only fails once the thread has gone away.
        let _ = self.tx.send(action);
    }

    pub fn status(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Audio dropouts inside the trailing five-second window.
    pub fn drops_5s(&self) -> u64 {
        self.drops_5s.load(Ordering::Relaxed)
    }

    /// The latest front-panel LED brightness values (0..1, one per key id).
    pub fn leds(&self) -> [f32; 41] {
        self.leds.lock().map(|l| *l).unwrap_or([0.0; 41])
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.join.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for EmulatorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_emulator(
    mut m: Machine,
    rx: mpsc::Receiver<Action>,
    ring: Arc<AudioRing>,
    lcd: SharedLcd,
    leds: Arc<Mutex<[f32; 41]>>,
    status: Arc<Mutex<String>>,
    drops_5s: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    audio_enabled: bool,
    snapshot: Arc<Mutex<Option<Vec<u8>>>>,
) {
    let start = Instant::now();
    // Wall-clock time given up after falling too far behind.
    let mut offset_ms = 0.0f64;
    let mut last_lcd = Instant::now() - Duration::from_millis(16);
    let mut last_stat = Instant::now();
    let (mut stat_emu, mut stat_wall) = (0u64, Instant::now());
    let mut lcd_buf = vec![0u16; LCD_W * LCD_H];
    // Samples of the from-zero underrun counter, kept to the trailing DROP_WINDOW.
    let mut drop_log: VecDeque<(Instant, u64)> = VecDeque::new();

    while running.load(Ordering::Relaxed) {
        while let Ok(action) = rx.try_recv() {
            m.soc.periph.apply(&action);
        }

        let real = start.elapsed().as_secs_f64() * 1000.0 - offset_ms;
        let emu_ms = m.soc.periph.cycles as f64 * 1000.0 / m.soc.periph.hz as f64;
        if real - emu_ms > 250.0 {
            // Too far behind (suspend, debugging, a busy machine): resync, do not race.
            offset_ms += real - emu_ms - LEAD_MS;
        }

        let buffered = ring.q.lock().map(|q| q.len()).unwrap_or(0) as f64 / SAMPLES_PER_MS;
        let behind = if audio_enabled && m.soc.periph.audio_halves > 0 {
            buffered < AUDIO_TARGET_MS
        } else {
            emu_ms < real + LEAD_MS
        };

        if behind {
            if let Err(e) = m.run(20_000) {
                *status.lock().unwrap() = format!("halted: {e}");
                running.store(false, Ordering::Relaxed);
                break;
            }
        } else {
            thread::sleep(Duration::from_millis(1));
        }

        if !m.soc.periph.audio.is_empty() {
            if let Ok(mut q) = ring.q.lock() {
                q.extend(m.soc.periph.audio.drain(..));
                ring.started.store(true, Ordering::Relaxed);
                if q.len() > MAX_QUEUED {
                    let extra = q.len() - MAX_QUEUED;
                    q.drain(..extra);
                }
            } else {
                m.soc.periph.audio.clear();
            }
        }

        if last_lcd.elapsed() >= Duration::from_millis(16) {
            lcd_buf.copy_from_slice(&m.soc.lcd.fb);
            if let Ok(mut shared) = lcd.lock() {
                shared.copy_from_slice(&lcd_buf);
            }
            let panel_leds = m.soc.periph.led_take();
            if let Ok(mut shared) = leds.lock() {
                *shared = panel_leds;
            }
            last_lcd = Instant::now();
        }

        if last_stat.elapsed() >= Duration::from_millis(500) {
            let now = m.soc.periph.ms();
            let speed = (now - stat_emu) as f64 / stat_wall.elapsed().as_secs_f64() / 1000.0;
            let buffered = ring.q.lock().map(|q| q.len()).unwrap_or(0) as f64 / SAMPLES_PER_MS;
            let underruns = ring.underruns.load(Ordering::Relaxed);
            // The underrun counter only grows from boot; fold it into the trailing
            // window so the status and the health dot show the recent past, not the
            // whole run.
            let wall = Instant::now();
            if drop_log.back().is_some_and(|&(_, total)| total > underruns) {
                drop_log.clear(); // the counter reset with the ring
            }
            drop_log.push_back((wall, underruns));
            while drop_log
                .front()
                .is_some_and(|&(t, _)| wall.duration_since(t) > DROP_WINDOW)
            {
                drop_log.pop_front();
            }
            let drops = underruns - drop_log.front().map(|&(_, total)| total).unwrap_or(underruns);
            drops_5s.store(drops, Ordering::Relaxed);
            *status.lock().unwrap() = format!(
                "{speed:.2}x · {:.0} ms audio · {drops} drops/5s",
                buffered.max(0.0)
            );
            stat_emu = now;
            stat_wall = Instant::now();
            last_stat = Instant::now();
        }
    }

    // The device is off: its flash chip's contents are the slot's snapshot, for the
    // pinned-slot restore on the next boot.
    if let Ok(mut snapshot) = snapshot.lock() {
        *snapshot = Some(m.soc.flash.mem.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioRing;
    use crate::builtin::BUILTINS;

    /// Boot a bundled firmware, stop it (snapshot taken), boot again with the saved
    /// flash — the pinned-slot cycle. The restored device must run and draw.
    #[test]
    fn snapshot_restore_cycle() {
        let firmware = &BUILTINS[0];
        let ring = Arc::new(AudioRing::new());
        let mut first = EmulatorHandle::start(
            firmware.label(),
            firmware.bytes.to_vec(),
            true,
            ring.clone(),
            false,
            None,
            None,
        )
        .expect("first boot");
        std::thread::sleep(Duration::from_millis(1500));
        let saved = first.stop_with_snapshot().expect("a flash snapshot");
        assert!(!saved.is_empty(), "the flash snapshot is empty");

        let mut second = EmulatorHandle::start(
            firmware.label(),
            firmware.bytes.to_vec(),
            true,
            ring,
            false,
            None,
            Some(saved),
        )
        .expect("restored boot");
        std::thread::sleep(Duration::from_millis(1500));
        let lcd = second.lcd.lock().unwrap().clone();
        second.stop();
        assert!(
            lcd.iter().any(|&px| px != 0),
            "the restored device drew nothing"
        );
        assert!(second.status().contains('x'), "status: {}", second.status());
    }
}
