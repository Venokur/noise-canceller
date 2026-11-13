//! Live "is this the enrolled speaker?" confidence.
//!
//! A background thread eats a copy of the 16 kHz microphone stream, embeds a
//! 2 s window once a second with the CAM++ model (`models/speaker`) and compares
//! it to the profile recorded by the user. The audio callback only pushes
//! samples into a ring buffer; no inference happens on the audio path.
//!
//! The number is diagnostic for now: it proves whether a guard is viable on the
//! owner's own recordings before it is allowed to duck anything.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::model::SpeakerEmbedder;
use crate::resample::Resampler;
use crate::wav;

const MODEL_RATE: u32 = 16_000;
/// The profile is embedded from its loudest 3 s, and the live guard uses the
/// same 3 s window: longer files (a 15 s enrolment with pauses) produce a
/// diluted vector that no live window matches.
const WINDOW: usize = MODEL_RATE as usize * 3;
const HOP: usize = (MODEL_RATE as usize * 3) / 2;
const MIN_RMS: f32 = 0.0015;
/// confidence sentinel: no profile, no speech, or the model is missing
pub const UNKNOWN: f32 = -1.0;

/// Cosine similarity of two embeddings; 1.0 is the same vector.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for i in 0..len {
        let x = a[i] as f64;
        let y = b[i] as f64;
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    (dot / (na.sqrt() * nb.sqrt()).max(1e-12)) as f32
}

/// Embedding of a whole wav file (the enrolled profile), resampled to 16 kHz.
/// The loudest 3 s are embedded: a 15 s enrolment has pauses and room tone, and
/// embedding all of it dilutes the vector until live windows no longer match.
pub fn embedding_from_wav(embedder: &mut SpeakerEmbedder, path: &Path) -> Result<Vec<f32>> {
    let (samples, rate) = wav::read_mono(path)?;
    if samples.is_empty() {
        return Err(anyhow!("profile wav is empty"));
    }
    let samples = if rate == MODEL_RATE {
        samples
    } else {
        let mut resampler = Resampler::new(rate, MODEL_RATE);
        let mut out = vec![0.0f32; resampler.max_output(samples.len()) + 64];
        let count = resampler.process(&samples, &mut out);
        out.truncate(count);
        out
    };
    let window = loudest_window(&samples, MODEL_RATE, WINDOW);
    embedder.embed(window, MODEL_RATE)
}

/// The loudest `len` samples, scanned in 100 ms steps. Short files are used as
/// they are.
fn loudest_window(samples: &[f32], rate: u32, len: usize) -> &[f32] {
    if samples.len() <= len {
        return samples;
    }
    let step = (rate as usize / 10).max(1);
    let mut best = 0usize;
    let mut best_rms = 0.0f64;
    let mut start = 0usize;
    while start + len <= samples.len() {
        let window = &samples[start..start + len];
        let rms = (window
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            / len as f64)
            .sqrt();
        if rms > best_rms {
            best_rms = rms;
            best = start;
        }
        start += step;
    }
    &samples[best..best + len]
}

/// Owns the background thread and its stop flag.
pub struct GuardHandle {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl GuardHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for GuardHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the guard. The model and the profile are loaded inside the thread so
/// a slow disk does not delay the engine start.
pub fn spawn(
    model: PathBuf,
    profile: PathBuf,
    mut input: rtrb::Consumer<f32>,
    confidence: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
) -> GuardHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let join = thread::Builder::new()
        .name("nc-speaker".into())
        .spawn(move || {
            let mut embedder = match SpeakerEmbedder::new(&model, 1) {
                Ok(embedder) => embedder,
                Err(err) => {
                    crate::log::info(&format!("speaker: model failed to load: {err}"));
                    confidence.store(UNKNOWN.to_bits(), Ordering::Relaxed);
                    return;
                }
            };
            let enrolled = match embedding_from_wav(&mut embedder, &profile) {
                Ok(vector) => vector,
                Err(err) => {
                    crate::log::info(&format!("speaker: profile failed to load: {err}"));
                    confidence.store(UNKNOWN.to_bits(), Ordering::Relaxed);
                    return;
                }
            };

            let mut buffer: Vec<f32> = Vec::with_capacity(WINDOW * 2);
            let mut silent_windows = 0u32;
            let mut failures = 0u32;
            while running.load(Ordering::Relaxed) && !thread_stop.load(Ordering::Relaxed) {
                while let Ok(sample) = input.pop() {
                    buffer.push(sample);
                }
                // never keep more than 4 s: after a long pause the oldest audio
                // is irrelevant
                if buffer.len() > WINDOW * 2 {
                    let drop = buffer.len() - WINDOW * 2;
                    buffer.drain(..drop);
                }

                if buffer.len() >= WINDOW {
                    let window = &buffer[buffer.len() - WINDOW..];
                    let rms = (window
                        .iter()
                        .map(|v| (*v as f64) * (*v as f64))
                        .sum::<f64>()
                        / window.len() as f64)
                        .sqrt() as f32;
                    if rms >= MIN_RMS {
                        silent_windows = 0;
                        match embedder.embed(window, MODEL_RATE) {
                            Ok(vector) => {
                                let value = cosine(&vector, &enrolled);
                                confidence.store(value.to_bits(), Ordering::Relaxed);
                            }
                            Err(err) => {
                                failures += 1;
                                if failures == 1 {
                                    crate::log::info(&format!("speaker: embed failed: {err}"));
                                }
                            }
                        }
                    } else {
                        silent_windows += 1;
                        if silent_windows >= 3 {
                            confidence.store(UNKNOWN.to_bits(), Ordering::Relaxed);
                        }
                    }
                    buffer.drain(..HOP);
                }

                thread::sleep(Duration::from_millis(20));
            }
        })
        .ok();

    GuardHandle { stop, join }
}

#[cfg(test)]
mod tests {
    use super::cosine;

    #[test]
    fn cosine_is_one_for_the_same_vector_and_low_for_opposites() {
        let a = [1.0f32, 2.0, 3.0];
        let b = [-1.0f32, -2.0, -3.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
        assert!((cosine(&a, &b) + 1.0).abs() < 1e-6);
        assert!(cosine(&a, &[0.0, 0.0, 0.0]).abs() < 1e-6);
    }
}
