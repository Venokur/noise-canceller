//! The processing thread: VoxLock, the residual stages, the DSP chain and the
//! two controllers that keep the output ring in shape.
//!
//! This started as a closure inside `Engine::start` over 36 `let mut` locals,
//! which made `start` 725 lines long. The move is mechanical: every local became
//! a field with the same name and the loop body was copied with a `self.`
//! prefix, so nothing about the audio path changed. `scripts/golden.ps1` and
//! `nc replay` are what prove that.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::ab::ab_append_clean;
use super::helpers::{rms_of, sanitize, InputVoice, Leveler, PeakGuard, Slew};
use super::streams::push_block;
use super::{fade_suppression, Persist, Shared, MODEL_RATE, SILENCE_SKIP_BLOCKS};
use crate::air::Air;
use crate::dsp::{Chain, InputFilter};
use crate::pipeline::Pipeline;
use crate::resample::Resampler;
use crate::settings::{self, Settings};
use crate::speaker::SpeakerGate;
use crate::spectrum::Spectrum;
use crate::state::LEVELER_GAIN_MAX_DB;

/// What one iteration needs to decide how much the output ring can take.
struct Pace {
    max_fill: usize,
    need: usize,
}

/// The learned output-ring size.
///
/// A real buffer miss raises the ring by 20 ms and is persisted for the next
/// session; 90 s without misses lowers it by 10 ms, so latency settles at the
/// smallest value this machine actually sustains. The learned value stays in
/// memory: the processing thread must not read or write `data/`.
struct AutoBuffer {
    enabled: bool,
    ms: f32,
    events: u32,
    at: Instant,
    changed: Instant,
    learned_ms: f32,
}

impl AutoBuffer {
    fn tick(&mut self, shared: &Shared, persist: &Persist, session: Duration) {
        if self.at.elapsed() < Duration::from_secs(4) {
            return;
        }
        self.at = Instant::now();
        if !self.enabled || session <= Duration::from_secs(10) {
            return;
        }
        let events = shared.underrun_events.load(Ordering::Relaxed);
        if events > self.events {
            let delta = events - self.events;
            self.events = events;
            self.ms = (self.ms + 20.0).min(settings::BUFFER_MS_MAX);
            self.learned_ms = self.ms;
            shared.buffer_ms.store(self.ms.to_bits(), Ordering::Relaxed);
            persist.set_auto_buffer(self.ms);
            self.changed = Instant::now();
            crate::log::info(&format!(
                "auto buffer: {:.0} ms (+{delta} buffer miss)",
                self.ms
            ));
        } else if self.changed.elapsed() > Duration::from_secs(90)
            && self.ms > settings::BUFFER_MS_MIN
        {
            self.ms = (self.ms - 10.0).max(settings::BUFFER_MS_MIN);
            self.learned_ms = self.ms;
            shared.buffer_ms.store(self.ms.to_bits(), Ordering::Relaxed);
            persist.set_auto_buffer(self.ms);
            self.changed = Instant::now();
            crate::log::info(&format!(
                "auto buffer: {:.0} ms (90 s without misses)",
                self.ms
            ));
        }
    }
}

/// Rate control on the real ring level.
///
/// Comparing pushed with pulled sample counts jumped by a whole block at every
/// window boundary, and that noise could walk the converter to ±3 % — audible as
/// the voice speeding up and down and the timbre changing. The fill is the
/// integral of the clock error, so a deadband around it is enough; inside it the
/// converter runs at exactly 1.0 and nothing bends the pitch.
struct RateControl {
    at: Instant,
    logs: u32,
    step: f64,
    trace: bool,
}

impl RateControl {
    fn new(trace: bool) -> Self {
        Self {
            at: Instant::now(),
            logs: 0,
            step: 1.0,
            trace,
        }
    }

    fn tick(&mut self, shared: &Shared, out_rate: u32, ref_ms: f32, buffer_ms: f32) {
        if self.at.elapsed() < Duration::from_secs(if self.logs == 0 { 8 } else { 4 }) {
            return;
        }
        self.at = Instant::now();
        let fill_ms = f32::from_bits(shared.out_fill.load(Ordering::Relaxed))
            / out_rate.max(1) as f32
            * 1000.0;
        let callback_ms =
            shared.callback_frames.load(Ordering::Relaxed) as f32 / out_rate.max(1) as f32 * 1000.0;
        // where the rate should go, never more than 0.2 % away from 1.0: that is
        // ~3.5 cents, below what anyone hears. Returning to 1.0 is allowed to be
        // faster than the correction itself, so a one-off emergency cannot leave
        // the pitch bent for a minute.
        let err = fill_ms - ref_ms;
        // proportional: the bigger the backlog, the further the read head
        // advances per output sample (step > 1 = fewer samples), so a steady
        // mismatch between the mic and the cable is cancelled at a small
        // constant offset instead of filling the ring for an hour.
        let target: f64 = if fill_ms < 8.0 {
            0.995
        } else if fill_ms > buffer_ms + 120.0 {
            1.005
        } else if err.abs() < 8.0 {
            1.0
        } else {
            (1.0 + 0.0002 * err as f64).clamp(0.995, 1.005)
        };
        let max_step = if (target - 1.0).abs() < 1e-9 {
            0.002
        } else {
            0.001
        };
        self.step += (target - self.step).clamp(-max_step, max_step);
        self.logs += 1;
        if self.logs.is_multiple_of(8) {
            crate::log::info(&format!(
                "clock: fill {fill_ms:.0} ms, ref {ref_ms:.0} ms, cb {callback_ms:.0} ms, step {:.0} ppm, underruns {}, overflows {}, input drops {}",
                (self.step - 1.0) * 1e6,
                shared.underruns.load(Ordering::Relaxed),
                shared.overflows.load(Ordering::Relaxed),
                shared.capture_drops.load(Ordering::Relaxed),
            ));
        }
        if self.trace {
            eprintln!(
                "drift: fill {fill_ms:.0} ms, ref {ref_ms:.0} ms, target {target:.4}, step {:.4}",
                self.step
            );
        }
        shared.slew_ppm.store(
            (((self.step - 1.0) as f32) * 1e6).to_bits(),
            Ordering::Relaxed,
        );
    }
}

pub(super) struct Processor {
    shared: Shared,
    pipeline: Pipeline,
    persist: Persist,
    out_rate: u32,
    chunk_samples: usize,
    chunk_ms: f32,

    chain: Chain,
    up: Resampler,
    up_buf: Vec<f32>,
    block: Vec<f32>,
    input_voice: InputVoice,
    input_filter: InputFilter,
    speaker_in: Option<rtrb::Producer<f32>>,
    speaker_gate: SpeakerGate,
    auto: AutoBuffer,
    max_suppression: f32,
    analyzer: Spectrum,
    chain_seq: u32,
    rtf_window: (usize, Duration),
    leveler: Leveler,
    session_start: Instant,
    saved_level: f32,
    persist_ticks: u32,
    guard: PeakGuard,
    zeros: Vec<f32>,
    silent_run: u32,
    was_speaking: bool,
    onset_blocks: u32,
    speech_hold: u32,
    slew: Slew,
    slew_out: Vec<f32>,
    rate: RateControl,
    air: Option<Air>,
    air_error: bool,
    ab: Arc<Mutex<Option<super::ab::AbBuffer>>>,
    ab_done: Arc<AtomicBool>,
    ab_active: Arc<AtomicBool>,
    ab_folder: Arc<Mutex<Option<String>>>,
    ab_paths: Arc<Mutex<(Option<String>, Option<String>)>>,
}

impl Processor {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        shared: Shared,
        settings: &Settings,
        pipeline: Pipeline,
        persist: Persist,
        out_rate: u32,
        buffer_ms: f32,
        learned_buffer_ms: f32,
        saved_gain_db: f32,
        speaker_in: Option<rtrb::Producer<f32>>,
    ) -> Self {
        let chunk_samples = super::block_samples(settings.chunk_ms);
        let chunk_ms = super::block_ms(chunk_samples);
        let mut chain = Chain::new();
        chain.configure(settings, out_rate);
        let up = Resampler::new(MODEL_RATE, out_rate);
        let up_buf = vec![0.0f32; up.max_output(chunk_samples) + 64];
        let saved_gain = saved_gain_db.clamp(0.0, LEVELER_GAIN_MAX_DB);
        let ab = shared.ab.clone();
        let ab_done = shared.ab_done.clone();
        let ab_active = shared.ab_active.clone();
        let ab_folder = shared.ab_folder.clone();
        let ab_paths = shared.ab_paths.clone();

        Self {
            shared,
            pipeline,
            persist,
            out_rate,
            chunk_samples,
            chunk_ms,
            chain,
            up,
            up_buf,
            block: vec![0.0f32; chunk_samples],
            input_voice: InputVoice::new(),
            input_filter: InputFilter::new(MODEL_RATE as f32),
            speaker_in,
            speaker_gate: SpeakerGate::new(out_rate as f32),
            auto: AutoBuffer {
                enabled: settings.buffer_auto,
                ms: buffer_ms,
                events: 0,
                at: Instant::now(),
                changed: Instant::now(),
                learned_ms: learned_buffer_ms,
            },
            max_suppression: settings.max_suppression_db.clamp(0.0, 48.0),
            analyzer: Spectrum::new(1024, out_rate as f32),
            chain_seq: 0,
            rtf_window: (0, Duration::ZERO),
            leveler: Leveler::new(saved_gain),
            session_start: Instant::now(),
            saved_level: saved_gain,
            persist_ticks: 0,
            guard: PeakGuard::new(MODEL_RATE as f32),
            zeros: vec![0.0f32; chunk_samples],
            silent_run: 0,
            was_speaking: false,
            onset_blocks: 0,
            speech_hold: 0,
            slew: Slew::new(),
            slew_out: Vec::with_capacity(chunk_samples * 4),
            rate: RateControl::new(std::env::var("NC_TRACE").is_ok()),
            air: None,
            air_error: false,
            ab,
            ab_done,
            ab_active,
            ab_folder,
            ab_paths,
        }
    }

    pub(super) fn run(
        mut self,
        mut in_cons: rtrb::Consumer<f32>,
        mut out_prod: rtrb::Producer<f32>,
    ) {
        while self.shared.running.load(Ordering::Relaxed) {
            self.apply_settings();

            if in_cons.slots() < self.chunk_samples {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }

            let pace = self.pace();
            if self.shared.out_raw.load(Ordering::Relaxed) as usize + pace.need > pace.max_fill {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }

            for slot in self.block.iter_mut() {
                *slot = in_cons.pop().unwrap_or(0.0);
            }
            // DC and subsonic rumble never reach the leveler or the model: a
            // desk thump used to lift the gain and clip the model input.
            self.input_filter.process(&mut self.block);
            if let Some(producer) = self.speaker_in.as_mut() {
                // best effort: the guard is diagnostic, a full ring must never
                // hold up the audio path
                for sample in self.block.iter() {
                    let _ = producer.push(*sample);
                }
            }
            let in_voice = self.input_voice.update(&self.block);

            // silence costs nothing: skip the model and emit a clean zero block
            // once the room has been quiet for a moment
            let raw_rms = rms_of(&self.block);
            if raw_rms < 6e-4 {
                self.silent_run += 1;
            } else {
                self.silent_run = 0;
            }

            if self.silent_run > SILENCE_SKIP_BLOCKS {
                self.emit_silence(&mut out_prod);
                continue;
            }

            let leveling = self.shared.leveling.load(Ordering::Relaxed);
            let allow_up =
                self.session_start.elapsed() > Duration::from_millis(1500) || self.speech_hold > 2;
            // only the speaker's own blocks may move the level: a steady bed
            // (music, a television) used to pull the gain around and the voice
            // "caught up" after it stopped
            self.leveler
                .process(&mut self.block, leveling, allow_up, in_voice);
            self.shared
                .leveler_gain
                .store(self.leveler.gain_db().to_bits(), Ordering::Relaxed);
            self.guard.process(&mut self.block);

            self.persist_ticks += 1;
            if self.persist_ticks >= 40 {
                self.persist_ticks = 0;
                let gain = self.leveler.gain_db();
                if (gain - self.saved_level).abs() > 2.0 {
                    self.saved_level = gain;
                    self.persist.set_leveler_gain(gain);
                }
            }

            let wet = f32::from_bits(self.shared.wet.load(Ordering::Relaxed));
            // VoxLock needs a moment to lock on after a start
            let warm = self.session_start.elapsed() > Duration::from_secs(1);
            let started = Instant::now();
            let out = self.pipeline.process(
                &mut self.block,
                wet,
                warm,
                in_voice,
                self.input_voice.voiced(),
            );
            if out.cold_reset {
                crate::log::info(&format!(
                    "cold start: VoxLock reset while the model was closed (in {:.0} dB)",
                    20.0 * raw_rms.max(1e-9).log10()
                ));
            }
            if out.leak_reset {
                crate::log::info(&format!(
                    "leak guard: VoxLock reset, the model was passing the room (in {:.0} dB, atten {:.0} dB)",
                    20.0 * out.in_rms.max(1e-9).log10(),
                    out.atten_db
                ));
            }
            if let Some((in_db, out_db, resets)) = out.guard_reset {
                self.shared.guard_resets.fetch_add(1, Ordering::Relaxed);
                crate::log::info(&format!(
                    "dropout guard: VoxLock reset + 200 ms pre-roll (in {in_db:.0} dB, out {out_db:.0} dB, reset #{resets})"
                ));
            }
            self.shared.speech.store(out.speech_in, Ordering::Relaxed);

            let cap_voice = out.cap_voice;
            let cap_active = out.cap_active;
            let out_rms = out.out_rms;

            // the card shows the model's attenuation of *speech*, and never more
            // than the cap (a 50 dB number only scares)
            if (cap_voice || cap_active) && out_rms > 1e-8 {
                let shown = if self.max_suppression > 0.0 {
                    out.atten_db.min(self.max_suppression)
                } else {
                    out.atten_db
                };
                let current = f32::from_bits(self.shared.suppression.load(Ordering::Relaxed));
                let smoothed = current + (shown - current) * 0.1;
                self.shared
                    .suppression
                    .store(smoothed.to_bits(), Ordering::Relaxed);
            } else {
                // nothing is being suppressed in a pause: let the readout fall
                // back instead of freezing at the last phrase, which made the
                // app look like it kept cutting the room
                fade_suppression(&self.shared);
            }

            // a block the model cut hard is not the target speaker: do not lift
            // its level, unless the cap keeps the speaker audible anyway. This
            // is decided further down, once the output detector has had its say.
            let mut peak = 0.0f32;
            for sample in self.block.iter() {
                peak = peak.max(sample.abs());
            }
            let block_rms = rms_of(&self.block);
            let crest_db = 20.0 * ((peak + 1e-9) / (block_rms + 1e-9)).log10();
            let rms_db = 20.0 * (block_rms + 1e-9).log10();
            // hold the speech state for ~0.4 s so syllables do not retrigger the
            // onset boost on every word
            if block_rms > 1.8e-3 {
                self.speech_hold = 7;
            } else {
                self.speech_hold = self.speech_hold.saturating_sub(1);
            }
            let speaking = self.speech_hold > 0;
            // a knock is a click in an otherwise quiet block
            let knock = crest_db > 19.0 && rms_db < -50.0;
            let onset_now = (speaking && !self.was_speaking) || knock;
            if onset_now {
                self.onset_blocks = 5; // ~0.3 s at 60 ms blocks
            }
            self.was_speaking = speaking;
            self.onset_blocks = self.onset_blocks.saturating_sub(1);

            // the model lets the level fall 10-20 dB between syllables; a gain
            // lift was tried here and **reverted**: it raised the residual noise
            // and other voices along with the quiet syllables.
            // Whose block is this? The output detector says "speech" for an
            // audiobook as readily as for the speaker, so the AGC used to lift
            // the background to the target. Both sides have to agree: the mic
            // heard a level jump (the user is talking) and the detector did not
            // call the block silent.
            let vad = out.target_vad.unwrap_or(true);
            let vad_ok = vad && out.target_model;
            let speaker = (in_voice && vad_ok) || cap_active;
            self.chain.set_target_present(speaker);

            let produced = self.up.process(out.wet, &mut self.up_buf);

            if sanitize(&mut self.up_buf[..produced]) {
                let seen = self.shared.bad_blocks.fetch_add(1, Ordering::Relaxed) + 1;
                if seen == 1 {
                    crate::log::info("model returned NaN/Inf — block replaced with silence");
                }
            }
            // Optional air above 8 kHz. It goes in before the DSP so the tone
            // shelves and the limiter shape it like the rest of the voice; the
            // 0-8 kHz path itself is unchanged.
            let air_amount = f32::from_bits(self.shared.air.load(Ordering::Relaxed));
            if air_amount > 0.0 && self.air.is_none() && !self.air_error {
                match Air::new(self.out_rate) {
                    Ok(stage) => self.air = Some(stage),
                    Err(err) => {
                        self.air_error = true;
                        crate::log::info(&format!("air: {err}"));
                    }
                }
            }
            if let Some(stage) = self.air.as_mut() {
                // only while the speaker talks: the air stage lifts whatever the
                // model passed, and 8-12 kHz is exactly what makes a leaked
                // television voice intelligible
                stage.add(out.wet, &mut self.up_buf[..produced], air_amount, speaker);
            }
            self.chain.process(&mut self.up_buf[..produced]);
            let confidence = f32::from_bits(self.shared.speaker_confidence.load(Ordering::Relaxed));
            let threshold = f32::from_bits(self.shared.speaker_threshold.load(Ordering::Relaxed));
            let guard = self.shared.speaker_guard.load(Ordering::Relaxed);
            if guard {
                self.speaker_gate.process(
                    &mut self.up_buf[..produced],
                    confidence,
                    threshold,
                    true,
                    produced as f32 / self.out_rate.max(1) as f32 * 1000.0,
                );
            } else {
                self.speaker_gate.reset();
            }
            self.shared
                .lufs
                .store(self.chain.lufs().to_bits(), Ordering::Relaxed);
            self.shared
                .true_peak
                .store(self.chain.true_peak_db().to_bits(), Ordering::Relaxed);

            if self.rate.trace {
                eprintln!(
                    "blk in {:.0} out {:.0} atten {:.0} cap {} voice {} vad {} agc {:.1} level {:.1}",
                    20.0 * out.in_rms.max(1e-9).log10(),
                    20.0 * out.out_rms.max(1e-9).log10(),
                    out.atten_db,
                    u8::from(cap_active),
                    u8::from(in_voice),
                    u8::from(out.target_vad.unwrap_or(false)),
                    self.chain.agc_gain_db(),
                    self.leveler.gain_db(),
                );
            }

            // the model needs about a second to lock onto the speaker; lift its
            // output for that second so the first words are not swallowed (the
            // ramp makes the return inaudible)
            if self.onset_blocks > 0 {
                let boost_db = 3.0 * (self.onset_blocks as f32 / 5.0);
                let lin = 10f32.powf(boost_db / 20.0);
                for sample in self.up_buf[..produced].iter_mut() {
                    *sample = (*sample * lin).clamp(-1.0, 1.0);
                }
            }

            ab_append_clean(
                &self.ab,
                &self.ab_done,
                &self.ab_active,
                &self.ab_folder,
                &self.ab_paths,
                &self.up_buf[..produced],
            );
            self.publish(&mut out_prod, produced);

            let elapsed = started.elapsed();
            self.rtf_window.0 += self.chunk_samples;
            self.rtf_window.1 += elapsed;
            if self.rtf_window.0 >= MODEL_RATE as usize {
                let rtf = self.rtf_window.1.as_secs_f32()
                    / (self.rtf_window.0 as f32 / MODEL_RATE as f32);
                self.shared.rtf.store(rtf.to_bits(), Ordering::Relaxed);
                self.rtf_window = (0, Duration::ZERO);
            }
        }
    }

    /// Pick up a settings change without a restart. Runs in the processing
    /// thread: `configure` allocates, so it must not happen on the audio path.
    fn apply_settings(&mut self) {
        let seq = self.shared.settings_seq.load(Ordering::Relaxed);
        if seq == self.chain_seq {
            return;
        }
        if let Ok(current) = self.shared.settings.lock() {
            self.chain.configure(&current, self.out_rate);
            self.max_suppression = current.max_suppression_db.clamp(0.0, 48.0);
            self.pipeline.set_max_suppression(self.max_suppression);
            self.pipeline.set_residual_mix(current.residual_mix);
            if current.buffer_auto {
                if !self.auto.enabled {
                    // the learned value, kept in memory: this thread does not
                    // touch `data/`
                    self.auto.ms = self.auto.learned_ms;
                    self.shared
                        .buffer_ms
                        .store(self.auto.ms.to_bits(), Ordering::Relaxed);
                    self.auto.events = self.shared.underrun_events.load(Ordering::Relaxed);
                    self.auto.changed = Instant::now();
                }
                self.auto.enabled = true;
            } else {
                self.auto.enabled = false;
            }
        }
        self.chain_seq = seq;
    }

    /// How much the output ring can hold this iteration, and the two controllers
    /// that run on the same 4-second cadence.
    fn pace(&mut self) -> Pace {
        let chunk_out = (self.out_rate as f32 * self.chunk_ms / 1000.0) as usize;
        let buffer_ms = f32::from_bits(self.shared.buffer_ms.load(Ordering::Relaxed));
        // a virtual cable can ask for 200 ms in one callback; the ring has to
        // hold at least one callback plus a block, otherwise every callback
        // drains it and the render starves
        let callback_ms = self.shared.callback_frames.load(Ordering::Relaxed) as f32
            / self.out_rate.max(1) as f32
            * 1000.0;
        let block_ms = self.chunk_samples as f32 / MODEL_RATE as f32 * 1000.0;
        // the ring must hold the user buffer, one whole model block and one
        // device callback; `buffer + max(half block, cb)` left 18 ms of slack at
        // the default and a 65 ms model reset always emptied it
        let ref_ms = buffer_ms + block_ms + callback_ms;
        let max_fill = (self.out_rate as f32 * ref_ms / 1000.0) as usize + chunk_out + 64;
        // leave room for a whole block plus the resampler's tail: pushing into a
        // nearly full ring silently dropped the rest of the block, which is a
        // hole in the middle of a word
        let need = chunk_out + 64;

        self.auto
            .tick(&self.shared, &self.persist, self.session_start.elapsed());
        self.rate
            .tick(&self.shared, self.out_rate, ref_ms, buffer_ms);

        Pace { max_fill, need }
    }

    /// Digital silence: keep the pre-roll current, reset the air stage and push
    /// a clean zero block without touching the model.
    fn emit_silence(&mut self, out_prod: &mut rtrb::Producer<f32>) {
        // keep the pre-roll current (after a long gap it used to hold audio from
        // before the silence) and leave the gate open: the detector has not seen
        // audio for a while and would take ~300 ms to notice the next word
        self.pipeline.skip_silence(&self.block);
        if let Some(air) = self.air.as_mut() {
            air.reset();
        }
        fade_suppression(&self.shared);
        self.shared
            .lufs
            .store((-120.0f32).to_bits(), Ordering::Relaxed);
        self.shared
            .true_peak
            .store((-120.0f32).to_bits(), Ordering::Relaxed);
        let produced = self.up.process(&self.zeros, &mut self.up_buf);
        for sample in self.up_buf[..produced].iter_mut() {
            *sample = 0.0;
        }
        ab_append_clean(
            &self.ab,
            &self.ab_done,
            &self.ab_active,
            &self.ab_folder,
            &self.ab_paths,
            &self.up_buf[..produced],
        );
        self.publish(out_prod, produced);
    }

    /// A/B capture, spectrum and the slow rate converter, in the order the
    /// engine has always used.
    fn publish(&mut self, out_prod: &mut rtrb::Producer<f32>, produced: usize) {
        self.analyzer.push(&self.up_buf[..produced]);
        if let Ok(mut slot) = self.shared.spectrum.lock() {
            *slot = self.analyzer.bands();
        }
        self.slew_out.clear();
        self.slew.step = self.rate.step;
        self.slew
            .process(&self.up_buf[..produced], &mut self.slew_out);
        push_block(out_prod, &self.slew_out, &self.shared);
    }
}
