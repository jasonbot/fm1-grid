//! The per-emulator audio queue.
//!
//! Every emulator produces 44.1 kHz stereo signed-16-bit samples. Each slot owns an
//! [`AudioRing`]; the emulator thread fills it and the UI's audio callback drains all
//! slots (the cpal output stream lives in the fm1-grid crate).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Keep at most this much audio queued per slot (stereo i16 samples, 250 ms).
pub(crate) const MAX_QUEUED: usize = 44_100 * 2 / 4;

pub struct AudioRing {
    /// Interleaved stereo: L, R, L, R, ...
    pub q: Mutex<VecDeque<i16>>,
    /// Set once the emulator has actually produced audio.
    pub started: AtomicBool,
    /// Count of callbacks that ran dry after audio had started.
    pub underruns: AtomicU64,
}

impl AudioRing {
    pub fn new() -> Self {
        Self {
            q: Mutex::new(VecDeque::with_capacity(MAX_QUEUED)),
            started: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.q.lock().unwrap().clear();
        self.started.store(false, Ordering::Relaxed);
        self.underruns.store(0, Ordering::Relaxed);
    }
}

impl Default for AudioRing {
    fn default() -> Self {
        Self::new()
    }
}
