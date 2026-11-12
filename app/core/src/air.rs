//! Optional "air" stage: NovaSR bandwidth extension above 8 kHz.
//!
//! VoxLock works at 16 kHz, so its output has no content above 8 kHz. The
//! resampler stretches what is there to 48 kHz, which is correct but sounds
//! closed next to a raw 48 kHz microphone. This stage runs the tiny NovaSR
//! model on the 16 kHz model output, keeps only the part above 8 kHz and adds
//! it to the 48 kHz signal before the usual DSP, so the tone shelves and the
//! limiter shape it together with the voice.
//!
//! It is deliberately not a denoiser: the 0-8 kHz path is left untouched, and
//! nothing here removes noise or other voices. The stage is off at 0 and is
//! skipped in digital silence, because the model has a small non-zero output
//! for an all-zero input (about -61 dBFS) that would otherwise become audible
//! hiss in pauses.
//!
//! The model is stateless but its conv layers look a little into the future
//! (about one millisecond). Feeding each block on its own with replicated
//! edges made the high band jump at every block boundary - a click every
//! 64 ms. So the stage keeps a short history and a short lookahead and runs
//! the model over a continuous window; only the samples that have real
//! neighbours on both sides are used. The output FIFO absorbs the lookahead,
//! which costs about 8 ms of latency for the synthetic band only.

use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::OnceLock;

use crate::dsp::Biquad;
use crate::resample::Resampler;

const OUT_RATE: usize = 48_000;
/// Input samples kept before the processed part. The model's receptive field is
/// about 20 samples at 16 kHz; 64 keeps the block edges out of the result.
const HISTORY: usize = 64;
/// Input samples kept after the processed part. They are passed to the model
/// but their output is not used until the next call, when they have more real
/// future samples behind them. 72 samples = 4.5 ms.
const LOOKAHEAD: usize = 96;
/// Length of the crossfade between two model windows. The model is almost
/// shift-invariant, but the polyphase filters inside it are not exactly so: a
/// different window offset leaves a small step at the cut. Two milliseconds
/// blend it away without smearing speech.
const CROSSFADE: usize = 96;
/// The stage adds only what is above this. The resampler already carries the
/// 0-8 kHz band, so the two do not overlap much.
const CROSSOVER_HZ: f32 = 8_000.0;
/// Input samples below this are treated as silence by the per-sample air gate.
/// -56 dBFS: quiet syllables still open it, the VAD floor does not.
const GATE_LEVEL: f32 = 0.0016;
const GATE_RELEASE_MS: f32 = 50.0;
const GATE_ENV_ATTACK_MS: f32 = 10.0;
const GATE_ENV_RELEASE_MS: f32 = 50.0;
/// NovaSR's high band is quiet on already-denoised speech; this scales the
/// slider so its middle position matches the level the old Air stage produced.
///
/// 0.5 is the value of the take the owner picked as the best-sounding one; a
/// 13 kHz low-pass and a 10 kHz shelf were tried after it and both took the
/// life out of the voice, so the band is left unfiltered above 8 kHz.
const AIR_GAIN: f32 = 0.5;

/// ONNX Runtime is process-global in `ort` 2.0-rc.10: the first `Air` commits
/// the environment, later ones reuse it. Both the app and `nc replay` can
/// construct an `Air` in the same process.
static ENV: OnceLock<()> = OnceLock::new();

fn environment(dll: &Path) -> Result<()> {
    if ENV.get().is_some() {
        return Ok(());
    }
    let _ = ort::init_from(dll.to_string_lossy()).commit()?;
    let _ = ENV.set(());
    Ok(())
}

/// `ort::Error<SessionBuilder>` is deliberately not `Send` (the builder holds
/// raw pointers), so it cannot be converted into `anyhow::Error` directly.
fn ort_err(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("ONNX Runtime: {err}")
}

/// High-pass, gate and mix: the part that can be tested without a model file.
struct Mixer {
    /// 4th-order Butterworth high-pass at the crossover, run as two biquads
    high1: Biquad,
    high2: Biquad,
    /// Last per-sample gate value, used to fade the tail out in a pause.
    gate: f32,
    gate_release: f32,
}

impl Mixer {
    fn new() -> Self {
        let mut mixer = Self {
            high1: Biquad::default(),
            high2: Biquad::default(),
            gate: 0.0,
            gate_release: 1.0 - (-1000.0 / (GATE_RELEASE_MS * OUT_RATE as f32)).exp(),
        };
        mixer.high1.high_pass(OUT_RATE as f32, CROSSOVER_HZ, 0.5412);
        mixer.high2.high_pass(OUT_RATE as f32, CROSSOVER_HZ, 1.3066);
        mixer
    }

    fn reset(&mut self) {
        self.high1.reset();
        self.high2.reset();
        self.gate = 0.0;
    }

    /// Feeds `source` through the high-pass, multiplies it by the per-sample
    /// `gate` and adds it to `dst`.
    ///
    /// `pos`/`gate_pos` are the read positions; samples not yet available are
    /// treated as silence, so a short startup FIFO does not produce a click.
    fn mix_into(
        &mut self,
        source: &[f32],
        pos: &mut usize,
        gate: &[f32],
        gate_pos: &mut usize,
        dst: &mut [f32],
        amount: f32,
    ) {
        for slot in dst.iter_mut() {
            let raw = if *pos < source.len() {
                let value = source[*pos];
                *pos += 1;
                value
            } else {
                0.0
            };
            let g = if *gate_pos < gate.len() {
                let value = gate[*gate_pos];
                *gate_pos += 1;
                value
            } else {
                0.0
            };
            let raw = if raw.is_finite() { raw } else { 0.0 };
            let high = self.high2.process(self.high1.process(raw as f64)) as f32;
            let high = if high.is_finite() { high } else { 0.0 };
            let g = if g.is_finite() {
                g.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.gate = g;
            *slot += high * g * amount;
        }
    }

    /// Feeds silence through the filter and lets the gate close. Used when the
    /// VAD gate has closed: the high band must decay from its last sample
    /// instead of jumping to zero (a click at the end of every phrase).
    fn fade_out(&mut self, dst: &mut [f32], amount: f32) {
        for slot in dst.iter_mut() {
            let high = self.high2.process(self.high1.process(0.0)) as f32;
            let high = if high.is_finite() { high } else { 0.0 };
            self.gate += (0.0 - self.gate) * self.gate_release;
            *slot += high * self.gate * amount;
        }
    }

    /// Whole-slice variant used by the tests: the gate is fully open.
    #[cfg(test)]
    fn mix(&mut self, source: &[f32], dst: &mut [f32], _rms_db: f32, amount: f32) {
        let gate = vec![1.0; source.len()];
        let mut pos = 0;
        let mut gate_pos = 0;
        self.mix_into(source, &mut pos, &gate, &mut gate_pos, dst, amount);
    }
}

pub struct Air {
    session: ort::session::Session,
    input_name: String,
    output_name: String,
    mixer: Mixer,
    /// Input samples not yet passed to the model: the current block plus the
    /// lookahead left over from the previous one.
    pending: Vec<f32>,
    /// `[history, pending]`, reused as the model input.
    window: Vec<f32>,
    history: Vec<f32>,
    /// Model output at 48 kHz that has not reached the output yet.
    fifo: Vec<f32>,
    fifo_pos: usize,
    /// Per-sample gate for the same 48 kHz samples, derived from the 16 kHz
    /// model input. Without it a block that starts with speech and ends in a
    /// pause kept the high band open over the pause.
    gate_fifo: Vec<f32>,
    gate_pos: usize,
    gate_env: f32,
    gate_env_attack: f32,
    gate_env_release: f32,
    /// Model output for the current processed segment, reused as a buffer.
    segment: Vec<f32>,
    /// First samples of the previous window's lookahead output, crossfaded
    /// with the next segment.
    tail: Vec<f32>,
    model_out: Vec<f32>,
    /// Present when the output device is not 48 kHz: the model always returns
    /// 48 kHz and the high band has to be brought to the device rate.
    resampler: Option<Resampler>,
    high_buf: Vec<f32>,
    res_out: Vec<f32>,
    /// Log a model failure once instead of every block.
    failed: bool,
}

impl Air {
    /// Loads `models/bwe/novasr.onnx` with the `onnxruntime.dll` the app already
    /// ships for sherpa-onnx. Returns an error (and never panics) when either
    /// file is missing, so a dev build without the model simply has no air.
    pub fn new(out_rate: u32) -> Result<Self> {
        let model = crate::paths::bwe_model();
        if !model.exists() {
            return Err(anyhow!("air model is missing: {}", model.to_string_lossy()));
        }
        let dll = crate::paths::native_dir().join("onnxruntime.dll");
        if !dll.exists() {
            return Err(anyhow!(
                "onnxruntime.dll is missing: {}",
                dll.to_string_lossy()
            ));
        }
        environment(&dll)?;
        let builder = ort::session::Session::builder().map_err(ort_err)?;
        let builder = builder
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
            .map_err(ort_err)?;
        let builder = builder.with_intra_threads(1).map_err(ort_err)?;
        let session = builder.commit_from_file(&model).map_err(ort_err)?;
        let input_name = session
            .inputs
            .first()
            .map(|o| o.name.clone())
            .ok_or_else(|| anyhow!("air model has no input"))?;
        let output_name = session
            .outputs
            .first()
            .map(|o| o.name.clone())
            .ok_or_else(|| anyhow!("air model has no output"))?;
        Ok(Self {
            session,
            input_name,
            output_name,
            mixer: Mixer::new(),
            pending: Vec::new(),
            window: Vec::new(),
            history: vec![0.0; HISTORY],
            fifo: Vec::new(),
            fifo_pos: 0,
            gate_fifo: Vec::new(),
            gate_pos: 0,
            gate_env: 0.0,
            gate_env_attack: 1.0 - (-1000.0 / (GATE_ENV_ATTACK_MS * 16_000.0)).exp(),
            gate_env_release: 1.0 - (-1000.0 / (GATE_ENV_RELEASE_MS * 16_000.0)).exp(),
            segment: Vec::new(),
            tail: Vec::new(),
            model_out: Vec::new(),
            resampler: if out_rate == OUT_RATE as u32 {
                None
            } else {
                Some(Resampler::new(OUT_RATE as u32, out_rate))
            },
            high_buf: Vec::new(),
            res_out: Vec::new(),
            failed: false,
        })
    }

    /// Drops the streaming state. Called while the engine is emitting silence,
    /// so the next phrase starts from a clean high band.
    pub fn reset(&mut self) {
        self.mixer.reset();
        self.clear_stream();
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
    }

    /// Drops input/output buffered for the continuation, without touching the
    /// filter and gate state. A pause must not replay the delayed high band of
    /// the previous phrase into the silence, and a short pause must not leave
    /// it for the next phrase either.
    fn clear_stream(&mut self) {
        self.pending.clear();
        self.window.clear();
        self.history.fill(0.0);
        self.fifo.clear();
        self.fifo_pos = 0;
        self.gate_fifo.clear();
        self.gate_pos = 0;
        self.gate_env = 0.0;
        self.segment.clear();
        self.tail.clear();
    }

    /// Adds the synthesized high band of `block16` to `dst48`.
    ///
    /// `dst48` is the already resampled signal (`dst48.len()` is about three
    /// times `block16.len()` at 48 kHz). Only the samples that fit are changed;
    /// the rest of the high band waits in the FIFO. `amount` is 0..1: 0 leaves
    /// the signal bit-exact. `speech` is the same output-VAD verdict that drives
    /// the engine gate; without it the high band would lift a pause above the
    /// DSP gate threshold and the gate would open on its own.
    pub fn add(&mut self, block16: &[f32], dst48: &mut [f32], amount: f32, speech: bool) {
        let amount = amount.clamp(0.0, 1.0) * AIR_GAIN;
        if amount <= 0.0001 {
            // Off: do not keep a half-open gate or a charged filter for the
            // moment the user switches back on.
            self.reset();
            return;
        }
        if block16.is_empty() || dst48.is_empty() {
            return;
        }
        if self.failed {
            return;
        }
        if !speech {
            // The model output is a pause. Drop the delayed high band of the
            // previous phrase and let the last sample decay instead of
            // stopping abruptly.
            self.clear_stream();
            self.mixer.fade_out(dst48, amount);
            if self.mixer.gate < 0.001 {
                self.reset();
            }
            return;
        }
        self.pending.extend_from_slice(block16);
        let processed = self.pending.len().saturating_sub(LOOKAHEAD);
        if processed == 0 {
            return;
        }
        self.window.clear();
        self.window.extend_from_slice(&self.history);
        self.window.extend_from_slice(&self.pending);
        if let Err(err) = self.run_model() {
            if !self.failed {
                self.failed = true;
                crate::log::info(&format!("air: {err}"));
            }
            self.reset();
            return;
        }
        // Build the segment that belongs to the processed input samples.
        let wanted = 3 * processed;
        self.segment.clear();
        self.segment.resize(wanted, 0.0);
        let start = 3 * HISTORY;
        let available = self.model_out.len().saturating_sub(start);
        let filled = wanted.min(available);
        if filled > 0 {
            self.segment[..filled].copy_from_slice(&self.model_out[start..start + filled]);
        }
        // Crossfade the first samples with the previous window's lookahead.
        // Without this the model phase changes at the cut and leaves a click
        // every block; the blend keeps the high band continuous.
        let x = CROSSFADE.min(wanted);
        if x > 0 && self.tail.len() >= x {
            for i in 0..x {
                let w = i as f32 / x as f32;
                self.fifo
                    .push(self.tail[i] * (1.0 - w) + self.segment[i] * w);
            }
            self.fifo.extend_from_slice(&self.segment[x..]);
        } else {
            self.fifo.extend_from_slice(&self.segment);
        }
        // Keep the next window's lookahead output for the next crossfade.
        self.tail.clear();
        let tail_start = start + wanted;
        let tail_end = (tail_start + CROSSFADE).min(self.model_out.len());
        if tail_end > tail_start {
            self.tail
                .extend_from_slice(&self.model_out[tail_start..tail_end]);
        }
        self.tail.resize(CROSSFADE, 0.0);
        // A per-sample gate from the 16 kHz input, expanded to the 48 kHz
        // model output. It follows the speech envelope, so a block that starts
        // with a word and ends in a pause closes the air over the pause instead
        // of playing the delayed high band into it.
        for &sample in &self.pending[..processed] {
            let target = if sample.abs() > GATE_LEVEL { 1.0 } else { 0.0 };
            let coef = if target > self.gate_env {
                self.gate_env_attack
            } else {
                self.gate_env_release
            };
            self.gate_env += (target - self.gate_env) * coef;
            let gate = self.gate_env;
            self.gate_fifo.extend_from_slice(&[gate, gate, gate]);
        }
        // The history for the next call is the `HISTORY` samples that precede
        // its first unprocessed sample.
        self.history
            .copy_from_slice(&self.window[processed..processed + HISTORY]);
        self.pending.drain(..processed);

        if let Some(resampler) = self.resampler.as_mut() {
            let high_len = 3 * block16.len();
            self.high_buf.clear();
            self.high_buf.resize(high_len, 0.0);
            self.mixer.mix_into(
                &self.fifo,
                &mut self.fifo_pos,
                &self.gate_fifo,
                &mut self.gate_pos,
                &mut self.high_buf,
                amount,
            );
            self.res_out.clear();
            self.res_out.resize(dst48.len() + 64, 0.0);
            let produced = resampler.process(&self.high_buf, &mut self.res_out);
            for (slot, value) in dst48.iter_mut().zip(self.res_out[..produced].iter()) {
                *slot += *value;
            }
        } else {
            self.mixer.mix_into(
                &self.fifo,
                &mut self.fifo_pos,
                &self.gate_fifo,
                &mut self.gate_pos,
                dst48,
                amount,
            );
        }
        if self.fifo_pos >= self.fifo.len() {
            self.fifo.clear();
            self.fifo_pos = 0;
        } else if self.fifo_pos > 16_384 {
            self.fifo.drain(..self.fifo_pos);
            self.fifo_pos = 0;
        }
        if self.gate_pos >= self.gate_fifo.len() {
            self.gate_fifo.clear();
            self.gate_pos = 0;
        } else if self.gate_pos > 16_384 {
            self.gate_fifo.drain(..self.gate_pos);
            self.gate_pos = 0;
        }
    }

    fn run_model(&mut self) -> Result<()> {
        let input =
            ort::value::Tensor::from_array(([1usize, 1, self.window.len()], self.window.clone()))?;
        let outputs = self
            .session
            .run(ort::inputs![self.input_name.as_str() => input])?;
        let (_, data) = outputs[self.output_name.as_str()].try_extract_tensor::<f32>()?;
        self.model_out.clear();
        self.model_out.extend_from_slice(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / OUT_RATE as f32).sin())
            .collect()
    }

    #[test]
    fn a_biquad_pair_removes_the_band_below_the_crossover() {
        let mut low = Biquad::default();
        let mut high = Biquad::default();
        low.high_pass(OUT_RATE as f32, CROSSOVER_HZ, 0.5412);
        high.high_pass(OUT_RATE as f32, CROSSOVER_HZ, 1.3066);
        let mut run = |freq: f32| {
            let input = sine(freq, 0.5, OUT_RATE / 2);
            let mut out = Vec::with_capacity(input.len());
            for x in input {
                out.push(high.process(low.process(x as f64)));
            }
            let rms =
                (out[1000..].iter().map(|v| v * v).sum::<f64>() / (out.len() - 1000) as f64).sqrt();
            20.0 * (rms as f32 + 1e-12).log10()
        };
        let low_db = run(1_000.0);
        let high_db = run(12_000.0);
        assert!(
            high_db - low_db > 20.0,
            "high band should pass and low band should not: 1 kHz {low_db:.1} dB, 12 kHz {high_db:.1} dB"
        );
    }

    #[test]
    fn the_gate_closes_on_silence_and_opens_on_speech() {
        let mut mixer = Mixer::new();
        // 12 kHz carrier in the model output: if the gate leaks, the output moves
        let model_out = sine(12_000.0, 0.5, 3 * 1024);
        let gate_on = vec![1.0f32; model_out.len()];
        let gate_off = vec![0.0f32; model_out.len()];
        let mut silent = vec![0.0f32; 3 * 1024];
        for _ in 0..40 {
            let mut pos = 0;
            let mut gate_pos = 0;
            mixer.mix_into(
                &model_out,
                &mut pos,
                &gate_off,
                &mut gate_pos,
                &mut silent,
                1.0,
            );
        }
        assert!(
            silent.iter().all(|v| v.abs() < 1e-6),
            "gate stayed open in silence"
        );
        let mut speech = vec![0.0f32; 3 * 1024];
        for _ in 0..40 {
            let mut pos = 0;
            let mut gate_pos = 0;
            mixer.mix_into(
                &model_out,
                &mut pos,
                &gate_on,
                &mut gate_pos,
                &mut speech,
                1.0,
            );
        }
        assert!(
            speech.iter().any(|v| v.abs() > 1e-3),
            "gate did not open on speech"
        );
    }

    #[test]
    fn zero_amount_is_a_no_op() {
        let mut mixer = Mixer::new();
        let model_out = sine(12_000.0, 0.5, 3 * 1024);
        let mut dst = vec![0.25f32; 3 * 1024];
        mixer.mix(&model_out, &mut dst, -20.0, 0.0);
        assert!(dst.iter().all(|v| (*v - 0.25).abs() < 1e-9));
    }

    #[test]
    fn a_non_finite_model_output_does_not_poison_the_mix() {
        let mut mixer = Mixer::new();
        let mut model_out = sine(12_000.0, 0.5, 3 * 1024);
        for (i, v) in model_out.iter_mut().enumerate() {
            if i % 7 == 0 {
                *v = f32::NAN;
            }
        }
        let mut dst = vec![0.0f32; 3 * 1024];
        for _ in 0..8 {
            mixer.mix(&model_out, &mut dst, -20.0, 1.0);
        }
        assert!(dst.iter().all(|v| v.is_finite()));
    }

    /// Needs `models/bwe/novasr.onnx` and `native/onnxruntime.dll`; run it
    /// explicitly when changing the model:
    /// `cargo test -p noise-canceller-core the_model -- --nocapture`
    #[test]
    fn the_model_adds_a_high_band() {
        let model = crate::paths::bwe_model();
        let dll = crate::paths::native_dir().join("onnxruntime.dll");
        if !model.exists() || !dll.exists() {
            eprintln!("skip: model or onnxruntime.dll is missing");
            return;
        }
        let mut air = Air::new(OUT_RATE as u32).expect("air model should load");
        let block: Vec<f32> = (0..1024)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 6_000.0 * i as f32 / 16_000.0).sin())
            .collect();
        let mut dst = vec![0.0f32; 3 * 1024];
        for _ in 0..8 {
            air.add(&block, &mut dst, 1.0, true);
        }
        let peak = dst.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak > 1e-3, "the high band is silent (peak {peak})");
    }
}
