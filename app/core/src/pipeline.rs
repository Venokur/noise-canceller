//! The 16 kHz model chain shared by the live engine and `nc replay`.
//!
//! The order is: GTCRN before (optional) → VoxLock → dropout guard and its
//! 200 ms pre-roll → suppression cap → VAD gate → GTCRN after (optional).
//! Both call sites used to spell this out separately, and the copies drifted:
//! `replay` measured the model output with a different fallback and never
//! warmed up the dropout guard, so the regression could measure a chain the app
//! does not produce.

use std::path::Path;

use anyhow::Result;

use crate::engine::{cap_dry_mix, rms_of, DropoutGuard, FixedStage, LeakGuard};
use crate::model::{Denoiser, StreamDenoiser, Vad};
use crate::paths;
use crate::settings::Settings;

/// Pre-roll replayed into the model after a guard reset, samples at 16 kHz.
const PREROLL: usize = 3200; // 200 ms

/// Output gate floor: pauses are 60 dB down instead of digital silence. A hard
/// zero reads as an abrupt cut, and the close ramp hides the tail of the last
/// word when the detector lets go a block early.
const VAD_FLOOR: f32 = 0.001;

/// How long the gate stays open after the detector releases, seconds.
const VAD_HOLD_SEC: f32 = 0.2;

pub struct Pipeline {
    isolated: Option<Denoiser>,
    residual_before: Option<StreamDenoiser>,
    residual_after: Option<StreamDenoiser>,
    vad: Option<Vad>,
    input_vad: Option<Vad>,
    dropout: DropoutGuard,
    leak: LeakGuard,
    isolate_stage: FixedStage,
    before_stage: FixedStage,
    after_stage: FixedStage,
    isolate_out: Vec<f32>,
    before_out: Vec<f32>,
    after_out: Vec<f32>,
    model_out: Vec<f32>,
    res16: Vec<f32>,
    preroll: Vec<f32>,
    mix: Vec<f32>,
    wet: Vec<f32>,
    chunk: usize,
    max_suppression: f32,
    cap_mix: f32,
    vad_gain: f32,
    vad_attack: f32,
    vad_release: f32,
    /// samples left before the gate may close again
    vad_hold: u32,
    /// attenuation and input level of the previous block, for the leak guard
    last_atten_db: f32,
    last_in_db: f32,
}

/// What one block produced, for the caller's meters, logs and DSP.
pub struct BlockOut<'a> {
    /// 16 kHz output after every model stage, exactly one block long.
    pub wet: &'a [f32],
    /// RMS of what the model really returned (`None` when it returned nothing).
    pub model_out_rms: Option<f32>,
    pub model_in_len: usize,
    pub model_out_len: usize,
    pub in_rms: f32,
    pub out_rms: f32,
    /// how much the model cut this block, positive dB
    pub atten_db: f32,
    pub cap_active: bool,
    /// true when the input level looks like speech (the meters use this)
    pub speech_in: bool,
    /// true when the *model input* is speech: the cap may act on this block
    pub cap_voice: bool,
    /// AGC decision before the VAD gate, exactly as the engine used to set it.
    pub target_model: bool,
    /// AGC decision from the output VAD, when it is enabled.
    pub target_vad: Option<bool>,
    /// The model was started over before its first word after a silence.
    pub cold_reset: bool,
    /// The model had stopped isolating and was started over while the user was
    /// silent (an audiobook or a television leaked through).
    pub leak_reset: bool,
    /// `(input dB, output dB, resets so far)` when the dropout guard fired.
    pub guard_reset: Option<(f32, f32, u32)>,
}

impl Pipeline {
    pub fn new(
        settings: &Settings,
        voice: Option<&Path>,
        threads: u16,
        chunk: usize,
        enroll: &Path,
        chunk_model: &Path,
    ) -> Result<Self> {
        let isolated = if settings.voice_isolation {
            Some(Denoiser::new(enroll, chunk_model, voice, threads)?)
        } else {
            None
        };

        let residual_kind = settings.residual_denoise.to_lowercase();
        let order_before = settings.residual_order.eq_ignore_ascii_case("before");
        let mut residual_before = None;
        let mut residual_after = None;
        if residual_kind == "gtcrn" && paths::gtcrn_model().exists() {
            if let Ok(model) = Denoiser::new_gtcrn(&paths::gtcrn_model(), 2) {
                let stream = StreamDenoiser::new(model);
                if order_before {
                    residual_before = Some(stream);
                } else {
                    residual_after = Some(stream);
                }
            }
        }

        let vad = if settings.vad_gate && paths::vad_model().exists() {
            Vad::new(&paths::vad_model(), 0.5).ok()
        } else {
            None
        };
        // a second detector on the model input: the output-side one follows
        // VoxLock, so while the model is cold it reports silence exactly when
        // the speaker is talking. The cap and the cold reset need the input.
        let input_vad = Vad::new(&paths::vad_model(), 0.5).ok();

        Ok(Self {
            isolated,
            residual_before,
            residual_after,
            vad,
            input_vad,
            dropout: DropoutGuard::new(),
            leak: LeakGuard::new(),
            isolate_stage: FixedStage::new(),
            before_stage: FixedStage::new(),
            after_stage: FixedStage::new(),
            isolate_out: vec![0.0; chunk],
            before_out: vec![0.0; chunk],
            after_out: vec![0.0; chunk],
            model_out: Vec::with_capacity(chunk * 2),
            res16: Vec::with_capacity(chunk * 2),
            preroll: Vec::with_capacity(PREROLL * 2),
            mix: vec![0.0; chunk],
            wet: vec![0.0; chunk],
            chunk,
            max_suppression: settings.max_suppression_db.clamp(0.0, 48.0),
            cap_mix: 0.0,
            vad_gain: 1.0,
            vad_attack: 1.0 - (-1000.0f32 / 5.0 / 16_000.0).exp(),
            vad_release: 1.0 - (-1000.0f32 / 450.0 / 16_000.0).exp(),
            vad_hold: 0,
            last_atten_db: 0.0,
            last_in_db: -120.0,
        })
    }

    /// Settings that can change without rebuilding the models.
    pub fn set_max_suppression(&mut self, db: f32) {
        self.max_suppression = db.clamp(0.0, 48.0);
    }

    /// A block that never reached the model (a quiet room): keep the pre-roll
    /// current, leave the gate open and tell the guard it saw nothing, exactly
    /// as the engine does on its silence path.
    pub fn skip_silence(&mut self, block: &[f32]) {
        self.preroll.extend_from_slice(block);
        if self.preroll.len() > PREROLL {
            let drop = self.preroll.len() - PREROLL;
            self.preroll.drain(..drop);
        }
        self.dropout.skipped();
        self.vad_gain = 1.0;
        self.vad_hold = 0;
        // the model did not see this block, so the leak guard must not judge it
        self.last_atten_db = 0.0;
        self.last_in_db = -120.0;
    }

    /// Runs the chain on one 16 kHz block (already leveled and peak-limited).
    ///
    /// `wet` is the isolation strength, `warm` says the session is older than
    /// one second (the dropout guard stays out until the model has loaded),
    /// `speech_hint` comes from the caller's `InputVoice`, which must be fed the
    /// *raw* microphone block before the leveler.
    pub fn process(
        &mut self,
        block: &mut [f32],
        wet: f32,
        warm: bool,
        speech_hint: bool,
    ) -> BlockOut<'_> {
        let Self {
            isolated,
            residual_before,
            residual_after,
            vad,
            input_vad,
            dropout,
            leak,
            isolate_stage,
            before_stage,
            after_stage,
            isolate_out,
            before_out,
            after_out,
            model_out,
            res16,
            preroll,
            mix,
            wet: wet_out,
            chunk,
            max_suppression,
            cap_mix,
            vad_gain,
            vad_attack,
            vad_release,
            vad_hold,
            last_atten_db,
            last_in_db,
        } = self;

        // "before" mode: clean the input so VoxLock extracts the voice from a
        // quieter signal
        if let Some(model) = residual_before.as_mut() {
            res16.clear();
            model.process(block, res16);
            before_stage.take(res16, before_out, Some(block));
            block.copy_from_slice(before_out);
        }

        // The cap exists to keep the *speaker* audible when VoxLock mistakes
        // them for noise. Silero alone calls music, a television and an
        // audiobook speech, and then the cap mixes the very noise the model has
        // just removed straight back into the output: at a 10 dB limit the
        // audiobook sat 10 dB below the voice and was clearly audible. The
        // mic-side speaker detector (a level jump over the room) is what tells
        // the two apart, so it has the last word and Silero may only agree.
        let vad_in = input_vad.as_mut().map(|v| v.push(block));
        let cap_voice = speech_hint && vad_in.unwrap_or(true);

        // after a long stretch of the model suppressing everything, its speaker
        // state is stale: start it over before it sees the first word. The
        // verdict comes from the mic, and only for a phrase: a drum beat is one
        // block, a sentence is not, so the model no longer restarts on music.
        let cold_reset = dropout.cold_onset(speech_hint);
        // the opposite failure: the model stopped isolating and is passing the
        // room through (an audiobook in the background) while the user is silent
        let leak_reset = leak.observe(*last_atten_db, *last_in_db, speech_hint);
        if (cold_reset || leak_reset) && isolated.is_some() {
            if let Some(model) = isolated.as_mut() {
                model.reset();
            }
            isolate_stage.reset();
            if let Some(model) = residual_after.as_mut() {
                model.reset();
            }
            after_stage.reset();
            if let Some(model) = residual_before.as_mut() {
                model.reset();
            }
            before_stage.reset();
            if leak_reset {
                // the pre-roll holds the audio that leaked, so replaying it
                // would feed the model the same thing again
                preroll.clear();
                dropout.restart();
            }
        }

        let mut model_out_rms = None;
        let model_in_len = block.len();
        let mut model_out_len = 0usize;
        match isolated.as_mut() {
            Some(model) => {
                model.run_into(block, model_out);
                model_out_len = model_out.len();
                // measure what the model really returned: the padded tail is
                // silence and would look like a 40 dB cut
                model_out_rms = if model_out.is_empty() {
                    None
                } else {
                    Some(rms_of(model_out))
                };
                isolate_stage.take(model_out, isolate_out, None);
            }
            // isolation off: the input goes straight on to the noise stage
            None => {
                isolate_stage.reset();
                isolate_out.copy_from_slice(block);
            }
        }

        // VoxLock loses the speaker now and then and cuts 40+ dB for seconds.
        // Reset it and replay the last 200 ms of the input so it locks again;
        // the output of the replay is thrown away, only the state is rebuilt.
        let mut guard_reset = None;
        if let Some(model) = isolated.as_mut() {
            let in_now = rms_of(block);
            let in_db = 20.0 * in_now.max(1e-9).log10();
            // NaN or no output is as dead as a deep cut: the old comparison was
            // false for NaN and the guard stayed blind for the whole episode
            let out_db = match model_out_rms {
                Some(v) if v.is_finite() && v > 0.0 => 20.0 * v.log10(),
                _ => -120.0,
            };
            // with a suppression cap a 50 dB cut is not visible in the output,
            // so the dropout threshold has to stay above the cap
            let cut_db = 26.0f32.max(*max_suppression + 6.0);
            if warm && dropout.observe(in_db, out_db, cap_voice, cut_db) {
                model.reset();
                isolate_stage.reset();
                for part in preroll.chunks(*chunk) {
                    model.run_into(part, model_out);
                }
                model.run_into(block, model_out);
                model_out_len = model_out.len();
                model_out_rms = if model_out.is_empty() {
                    None
                } else {
                    Some(rms_of(model_out))
                };
                isolate_stage.take(model_out, isolate_out, None);
                guard_reset = Some((in_db, out_db, dropout.resets));
            }
            preroll.extend_from_slice(block);
            if preroll.len() > PREROLL {
                let drop = preroll.len() - PREROLL;
                preroll.drain(..drop);
            }
        }

        let count = isolate_out.len().min(block.len());
        let in_rms = rms_of(block);
        let out_rms = model_out_rms.unwrap_or_else(|| rms_of(&isolate_out[..count]));
        let atten_db = if in_rms > 1e-6 && out_rms > 1e-8 {
            -20.0 * (out_rms / in_rms).log10()
        } else {
            0.0
        };
        let speech_in = in_rms > 1.8e-3;

        // Maximum suppression: if the model cut a speech block by more than the
        // limit, mix the (leveled, peak-limited) model input back until the
        // output is exactly `limit` down. This is the only guarantee that the
        // speaker stays audible when VoxLock mistakes them for noise.
        let need = if *max_suppression > 0.0 && cap_voice && in_rms > 2e-3 {
            cap_dry_mix(in_rms, out_rms, *max_suppression)
        } else {
            0.0
        };
        // attack fast (the hole starts now) and let go quickly as well: a cap
        // that lingers after the phrase keeps mixing the room back in, and a
        // phone playing an audiobook was audible for half a second after every
        // sentence
        let coef = if need > *cap_mix { 0.5 } else { 0.35 };
        *cap_mix += (need - *cap_mix) * coef;
        let cap_active = *cap_mix > 0.01;
        // `cap_mix` is the fraction of the model input to mix back, so it puts
        // a floor under how far `wet` may go
        let wet_eff = if *cap_mix > 0.0 {
            wet.clamp(0.0, 1.0).min(1.0 - *cap_mix)
        } else {
            wet.clamp(0.0, 1.0)
        };

        mix[..count].copy_from_slice(&isolate_out[..count]);
        for (i, sample) in mix.iter_mut().enumerate().take(count) {
            *sample = block[i] * (1.0 - wet_eff) + *sample * wet_eff;
        }
        mix[count..].copy_from_slice(&block[count..]);

        // Silero VAD gate: close the output in pauses, keep every syllable.
        let mut target_vad = None;
        if let Some(detector) = vad.as_mut() {
            let speech = detector.push(mix);
            target_vad = Some(speech);
            // Do not shut on the exact block where the detector lets go: the
            // close ramp used to trim the last syllable, which is heard as a
            // swallowed ending. Hold the gate open for a moment instead.
            if speech {
                *vad_hold = (VAD_HOLD_SEC * 16_000.0) as u32;
            } else {
                *vad_hold = vad_hold.saturating_sub(mix.len() as u32);
            }
            let open = speech || *vad_hold > 0;
            let target = if open { 1.0f32 } else { VAD_FLOOR };
            let coef = if target > *vad_gain {
                *vad_attack
            } else {
                *vad_release
            };
            for sample in mix.iter_mut() {
                *vad_gain += (target - *vad_gain) * coef;
                *sample *= *vad_gain;
            }
        }

        // GTCRN works at 16 kHz, right after VoxLock
        match residual_after.as_mut() {
            Some(model) => {
                res16.clear();
                model.process(mix, res16);
                after_stage.take(res16, after_out, Some(mix));
                wet_out.copy_from_slice(after_out);
            }
            None => wet_out.copy_from_slice(mix),
        }

        // what the next block's leak guard has to judge
        *last_in_db = 20.0 * in_rms.max(1e-9).log10();
        *last_atten_db = atten_db;

        BlockOut {
            wet: wet_out,
            model_out_rms,
            model_in_len,
            model_out_len,
            in_rms,
            out_rms,
            atten_db,
            cap_active,
            speech_in,
            cap_voice,
            target_model: atten_db < 8.0 || cap_active,
            target_vad,
            cold_reset,
            leak_reset,
            guard_reset,
        }
    }
}
