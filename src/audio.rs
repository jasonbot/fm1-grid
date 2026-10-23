//! Mixes the audio of the emulators into one output stream.
//!
//! Every emulator produces 44.1 kHz stereo signed-16-bit samples. Each slot owns an
//! [`AudioRing`]; the emulator thread fills it and the audio callback drains all slots.
//! The callback resamples from 44.1 kHz to whatever rate the sound card uses.

use crate::MAX_SLOTS;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The emulator's native sample rate.
const EMU_RATE: f64 = 44_100.0;
/// Per-slot gain. Several units at full scale would clip without it.
const SLOT_GAIN: f32 = 0.30;
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

/// Open the default output device. Keep the returned stream alive for as long as you
/// want sound; dropping it stops the audio.
pub fn start_audio(rings: [Arc<AudioRing>; MAX_SLOTS]) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default audio output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("cannot query audio output: {e}"))?;
    let config = supported.config();

    match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, rings),
        SampleFormat::F64 => build_stream::<f64>(&device, &config, rings),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, rings),
        SampleFormat::I32 => build_stream::<i32>(&device, &config, rings),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, rings),
        SampleFormat::U8 => build_stream::<u8>(&device, &config, rings),
        other => Err(format!(
            "unsupported audio sample format {other:?} (try changing the system output format)"
        )),
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    rings: [Arc<AudioRing>; MAX_SLOTS],
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let source_per_output = EMU_RATE / config.sample_rate as f64;

    let mut position = 0.0f64;
    let mut current = (0.0f32, 0.0f32);
    let mut next = (0.0f32, 0.0f32);

    let error_callback = |err| eprintln!("audio: {err}");

    let stream = device
        .build_output_stream(
            config.clone(),
            move |out: &mut [T], _| {
                // Locking once per callback is much cheaper than once per sample.
                let mut queues: [std::sync::MutexGuard<'_, VecDeque<i16>>; MAX_SLOTS] =
                    std::array::from_fn(|i| rings[i].q.lock().unwrap());
                let mut starved = [false; MAX_SLOTS];

                for frame in out.chunks_mut(channels) {
                    position += source_per_output;
                    while position >= 1.0 {
                        position -= 1.0;
                        current = next;

                        let mut left = 0.0f32;
                        let mut right = 0.0f32;
                        for (i, q) in queues.iter_mut().enumerate() {
                            let a = q.pop_front();
                            let b = q.pop_front();
                            match (a, b) {
                                (Some(l), Some(r)) => {
                                    left += l as f32 / 32768.0;
                                    right += r as f32 / 32768.0;
                                }
                                _ => starved[i] = true,
                            }
                        }

                        next = (
                            (left * SLOT_GAIN).clamp(-1.0, 1.0),
                            (right * SLOT_GAIN).clamp(-1.0, 1.0),
                        );
                    }

                    // Linear interpolation between source frames when the card is not 44.1 kHz.
                    let f = position as f32;
                    let l = current.0 + (next.0 - current.0) * f;
                    let r = current.1 + (next.1 - current.1) * f;
                    for (c, sample) in frame.iter_mut().enumerate() {
                        *sample = T::from_sample(if c % 2 == 0 { l } else { r });
                    }
                }

                for (i, dry) in starved.iter().enumerate() {
                    if *dry && rings[i].started.load(Ordering::Relaxed) {
                        rings[i].underruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
            },
            error_callback,
            None,
        )
        .map_err(|e| format!("cannot open audio stream: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("cannot start audio stream: {e}"))?;
    Ok(stream)
}
