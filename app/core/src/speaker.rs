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

/// How uniform the enrolment is: the profile is split into thirds, the loudest
/// 3 s of each third are embedded, and the pairwise cosine is collapsed to a
/// median. A low value means the take mixed voices, distances or a lot of noise.
pub fn profile_consistency(embedder: &mut SpeakerEmbedder, path: &Path) -> Result<f32> {
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
    if samples.len() < WINDOW * 2 {
        return Err(anyhow!("profile is shorter than 6 seconds"));
    }

    let third = samples.len() / 3;
    let mut vectors: Vec<Vec<f32>> = Vec::new();
    for part in 0..3 {
        let slice = &samples[part * third..(part + 1) * third];
        let len = WINDOW.min(slice.len());
        if len < MODEL_RATE as usize {
            continue;
        }
        let window = loudest_window(slice, MODEL_RATE, len);
        let rms = (window
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            / window.len() as f64)
            .sqrt() as f32;
        if rms < MIN_RMS {
            continue;
        }
        if let Ok(vector) = embedder.embed(window, MODEL_RATE) {
            vectors.push(vector);
        }
    }
    if vectors.len() < 2 {
        return Err(anyhow!("not enough speech to compare"));
    }

    let mut values = Vec::new();
    for i in 0..vectors.len() {
        for j in (i + 1)..vectors.len() {
            values.push(cosine(&vectors[i], &vectors[j]));
        }
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(values[values.len() / 2])
}

/// Similarity of every speech window in a recording to one profile.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Similarity {
    pub windows: u32,
    pub min: f32,
    pub median: f32,
    pub max: f32,
}

/// Slides a 3 s window over a recording and compares each speech window with
/// the profile. Used by the A/B card to collect real "own / other" numbers
/// before a confidence threshold is allowed to duck anything.
pub fn similarity_to_profile(
    embedder: &mut SpeakerEmbedder,
    profile: &Path,
    samples: &[f32],
    rate: u32,
) -> Result<Similarity> {
    let enrolled = embedding_from_wav(embedder, profile)?;
    similarity_to_enrolled(embedder, &enrolled, samples, rate)
}

/// Same as [`similarity_to_profile`], but the enrolled vector is passed in so a
/// batch can embed the profile once instead of once per take.
pub fn similarity_to_enrolled(
    embedder: &mut SpeakerEmbedder,
    enrolled: &[f32],
    samples: &[f32],
    rate: u32,
) -> Result<Similarity> {
    let samples = if rate == MODEL_RATE {
        samples.to_vec()
    } else {
        let mut resampler = Resampler::new(rate, MODEL_RATE);
        let mut out = vec![0.0f32; resampler.max_output(samples.len()) + 64];
        let count = resampler.process(samples, &mut out);
        out.truncate(count);
        out
    };

    let mut values: Vec<f32> = Vec::new();
    if samples.len() < WINDOW {
        return Err(anyhow!("recording is shorter than 3 seconds"));
    }
    let mut start = 0usize;
    while start + WINDOW <= samples.len() {
        let window = &samples[start..start + WINDOW];
        let rms = (window
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            / window.len() as f64)
            .sqrt() as f32;
        if rms >= MIN_RMS {
            if let Ok(vector) = embedder.embed(window, MODEL_RATE) {
                values.push(cosine(&vector, enrolled));
            }
        }
        start += HOP;
    }
    if values.is_empty() {
        return Err(anyhow!("no speech windows found"));
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = values[values.len() / 2];
    Ok(Similarity {
        windows: values.len() as u32,
        min: *values.first().unwrap_or(&0.0),
        median,
        max: *values.last().unwrap_or(&0.0),
    })
}

/// Suggested guard threshold from the user's own A/B takes.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Recommendation {
    pub takes: u32,
    /// fifth percentile of the per-take medians
    pub p5: f32,
    pub median: f32,
    pub threshold: f32,
}

/// Embeds the profile once, measures every take, and suggests a threshold
/// 0.15 below the fifth percentile. With no labelled foreign voice this is the
/// honest ceiling: it keeps the guard off the owner's quiet takes, and the user
/// can still lower it manually.
pub fn recommend_threshold(
    embedder: &mut SpeakerEmbedder,
    profile: &Path,
    takes: &[PathBuf],
) -> Result<Recommendation> {
    let enrolled = embedding_from_wav(embedder, profile)?;
    let mut medians: Vec<f32> = Vec::new();
    for path in takes {
        let Ok((samples, rate)) = wav::read_mono(path) else {
            continue;
        };
        if let Ok(similarity) = similarity_to_enrolled(embedder, &enrolled, &samples, rate) {
            medians.push(similarity.median);
        }
    }
    if medians.is_empty() {
        return Err(anyhow!("no usable takes"));
    }
    medians.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p5 = medians[(medians.len().saturating_sub(1)) * 5 / 100];
    let median = medians[medians.len() / 2];
    Ok(Recommendation {
        takes: medians.len() as u32,
        p5,
        median,
        threshold: (p5 - 0.15).clamp(0.25, 0.6),
    })
}

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

/// Smooth ducking driven by the confidence number.
///
/// The confidence updates once every 1.5 s, so a single low window must not cut
/// a word: the gate waits 2.5 s of sustained "not the profile" before it ducks,
/// and releases in 300 ms when the speaker comes back.
pub struct SpeakerGate {
    gain: f32,
    low_ms: f32,
    attack: f32,
    release: f32,
}

impl SpeakerGate {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            gain: 1.0,
            low_ms: 0.0,
            attack: 1.0 - (-1000.0 / 10.0 / sample_rate).exp(),
            release: 1.0 - (-1000.0 / 300.0 / sample_rate).exp(),
        }
    }

    pub fn process(
        &mut self,
        block: &mut [f32],
        confidence: f32,
        threshold: f32,
        enabled: bool,
        block_ms: f32,
    ) {
        let low = enabled && confidence >= 0.0 && confidence < threshold;
        if low {
            self.low_ms = (self.low_ms + block_ms).min(5_000.0);
        } else {
            self.low_ms = (self.low_ms - block_ms * 2.0).max(0.0);
        }
        let target = if low && self.low_ms >= 2_500.0 {
            0.125
        } else {
            1.0
        };
        let coef = if target < self.gain {
            self.attack
        } else {
            self.release
        };
        for sample in block.iter_mut() {
            self.gain += (target - self.gain) * coef;
            *sample *= self.gain;
        }
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
        self.low_ms = 0.0;
    }
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

    #[test]
    fn the_gate_waits_before_ducking_and_releases() {
        let mut gate = super::SpeakerGate::new(48_000.0);
        let mut last = 1.0f32;
        for _ in 0..30 {
            let mut block = vec![1.0f32; 4_800];
            gate.process(&mut block, 0.1, 0.35, true, 100.0);
            last = block[block.len() - 1];
        }
        assert!(last < 0.25, "a foreign voice was not ducked: {last}");

        for _ in 0..10 {
            let mut block = vec![1.0f32; 4_800];
            gate.process(&mut block, 0.9, 0.35, true, 100.0);
            last = block[block.len() - 1];
        }
        assert!(last > 0.9, "the gate did not release: {last}");
    }
}
