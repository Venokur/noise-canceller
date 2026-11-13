//! Post-isolation clean-up: high-pass, expander, AGC, tone shelves, limiter,
//! optional comfort noise. Deterministic, no allocations in `process`.

use crate::settings::Settings;

#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    pub fn high_pass(&mut self, sample_rate: f32, cutoff_hz: f32, q: f64) {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz as f64 / sample_rate as f64;
        let cos = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;

        self.b0 = (1.0 + cos) / 2.0 / a0;
        self.b1 = -(1.0 + cos) / a0;
        self.b2 = (1.0 + cos) / 2.0 / a0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// RBJ low-pass: everything above `cutoff_hz` is rolled off.
    pub fn low_pass(&mut self, sample_rate: f32, cutoff_hz: f32, q: f64) {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz as f64 / sample_rate as f64;
        let cos = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;

        self.b0 = (1.0 - cos) / 2.0 / a0;
        self.b1 = (1.0 - cos) / a0;
        self.b2 = (1.0 - cos) / 2.0 / a0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// RBJ low shelf: everything below `corner_hz` moves by `gain_db`.
    ///
    /// The slope is the steepest one that does not overshoot (`S = 1`), which is
    /// what a tone control wants: a smooth tilt, not a resonant bump around the
    /// corner that would sound like a second voice.
    pub fn low_shelf(&mut self, sample_rate: f32, corner_hz: f32, gain_db: f64) {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * corner_hz as f64 / sample_rate as f64;
        let cos = w0.cos();
        let alpha = w0.sin() / 2.0 * std::f64::consts::SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha;
        self.b0 = a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha) / a0;
        self.b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos) / a0;
        self.b2 = a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha) / a0;
        self.a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos) / a0;
        self.a2 = ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha) / a0;
    }

    /// RBJ high shelf: everything above `corner_hz` moves by `gain_db`.
    pub fn high_shelf(&mut self, sample_rate: f32, corner_hz: f32, gain_db: f64) {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * corner_hz as f64 / sample_rate as f64;
        let cos = w0.cos();
        let alpha = w0.sin() / 2.0 * std::f64::consts::SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha;
        self.b0 = a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha) / a0;
        self.b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos) / a0;
        self.b2 = a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha) / a0;
        self.a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos) / a0;
        self.a2 = ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha) / a0;
    }

    /// RBJ notch: removes a narrow band around `center_hz`.
    pub fn notch(&mut self, sample_rate: f32, center_hz: f32, q: f64) {
        let w0 = 2.0 * std::f64::consts::PI * center_hz as f64 / sample_rate as f64;
        let cos = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = 1.0 / a0;
        self.b1 = -2.0 * cos / a0;
        self.b2 = 1.0 / a0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

pub struct Gate {
    env: f64,
    gain: f64,
    open: bool,
    hold_left: f64,
    threshold_lin: f64,
    pub threshold_db: f64,
    pub ratio: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    pub hold_ms: f64,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            env: 0.0,
            gain: 1.0,
            open: true,
            hold_left: 0.0,
            threshold_lin: 10f64.powf(-60.0 / 20.0),
            threshold_db: -60.0,
            ratio: 2.0,
            attack_ms: 10.0,
            release_ms: 300.0,
            hold_ms: 150.0,
        }
    }
}

impl Gate {
    #[inline]
    pub fn process(&mut self, x: f64, sample_rate: f64) -> f64 {
        let abs = x.abs();
        self.env += (abs - self.env) * if abs > self.env { 0.35 } else { 0.004 };

        let thr = self.threshold_lin;
        let dt = 1.0 / sample_rate;

        // hold keeps the gate open through short gaps between syllables, so
        // endings of words and quiet consonants are not chopped off
        if self.env >= thr {
            self.open = true;
            self.hold_left = self.hold_ms / 1000.0;
        } else if self.hold_left > 0.0 {
            self.hold_left -= dt;
        } else {
            self.open = false;
        }

        let target = if self.open || self.env <= 1e-7 {
            1.0
        } else {
            (self.env / thr).powf((self.ratio - 1.0).max(0.1)).min(1.0)
        };

        let time = if target > self.gain {
            self.attack_ms
        } else {
            self.release_ms
        };
        let coef = 1.0 - (-1000.0 / time.max(1.0) / sample_rate).exp();
        self.gain += (target - self.gain) * coef;
        x * self.gain
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.gain = 1.0;
        self.open = true;
        self.hold_left = 0.0;
    }

    pub fn prepare(&mut self) {
        self.threshold_lin = 10f64.powf(self.threshold_db / 20.0);
    }
}

pub struct Agc {
    /// set by the engine from the model's attenuation: only a block that the
    /// model did not suppress is the target speaker, and only then the level
    /// may be raised
    pub target_present: bool,
    /// this detector window is at the speech level, so the held gain may be
    /// applied. A breath or the room between two words is not: lifting it is
    /// how the gap between words arrived 12 dB louder than it left.
    apply: bool,
    block_sum: f64,
    block_peak: f64,
    block_len: usize,
    block_count: usize,
    rms: f64,
    /// loudest recent block: the reference for "is this block a word at all".
    /// An average drifts down to the room level during a long gap, and then the
    /// room itself counts as speech and gets the gain.
    peak: f64,
    gain_db: f64,
    gain_lin: f64,
    target_lin: f64,
    smooth_coef: f64,
    smooth_rate: f64,
    /// detector windows left in which the gain may travel fast: at start-up the
    /// gain has to reach the target inside the first phrase
    warmup: u32,
    pub target_db: f64,
    pub max_gain_db: f64,
    pub min_gain_db: f64,
}

impl Default for Agc {
    fn default() -> Self {
        Self {
            // start closed: before the detector has an opinion the AGC used to
            // lift whatever was in the room, and the first word came out 8 dB
            // louder than the rest
            target_present: false,
            apply: false,
            block_sum: 0.0,
            block_peak: 0.0,
            block_len: 0,
            block_count: 0,
            rms: 0.0,
            peak: 0.0,
            gain_db: 0.0,
            gain_lin: 1.0,
            target_lin: 1.0,
            smooth_coef: 0.0,
            smooth_rate: 0.0,
            warmup: Self::WARMUP_WINDOWS,
            target_db: -24.0,
            max_gain_db: 12.0,
            // symmetric: a loud phrase has to be pulled down too, otherwise a
            // loud sentence stays 10 dB above the next one
            min_gain_db: -6.0,
        }
    }
}

impl Agc {
    /// gain step per 100 ms detector window once settled: 8 dB/s
    const STEP_DB: f64 = 0.8;
    /// ...and downwards, 4 dB per window (40 dB/s): a loud phrase must come
    /// back to the target within a few hundred ms
    const FALL_DB: f64 = 4.0;
    /// a block this far above the phrase average is a shout or another voice
    const SHOUT_DB: f64 = 6.0;
    /// start-up may travel 4 dB per window (40 dB/s)
    const WARMUP_DB: f64 = 4.0;
    /// ...for the first two seconds
    const WARMUP_WINDOWS: u32 = 20;
    /// blocks this far below the recent speech level are the room, not a word
    const SPEECH_WINDOW_DB: f64 = 22.0;
    /// ...and this is how close a block has to be for the held gain to be applied
    const APPLY_WINDOW_DB: f64 = 12.0;
    /// the loudest-block reference loses 0.5 % per 100 ms window
    const PEAK_DECAY: f64 = 0.995;

    #[inline]
    pub fn process(&mut self, x: f64, sample_rate: f64) -> f64 {
        self.block_sum += x * x;
        self.block_peak = self.block_peak.max(x.abs());
        self.block_count += 1;
        // 200 ms detector: at 100 ms the gain followed syllables and the voice
        // audibly pumped; 200 ms averages a whole word instead
        self.block_len = ((sample_rate * 0.1) as usize).max(64);

        if self.smooth_rate != sample_rate {
            self.smooth_rate = sample_rate;
            // 30 ms one-pole: the gain has to move per sample, otherwise every
            // 50 ms detector step puts a small click into the voice
            self.smooth_coef = 1.0 - (-1000.0 / 30.0 / sample_rate).exp();
        }

        if self.block_count >= self.block_len {
            let block_rms = (self.block_sum / self.block_count as f64).sqrt();
            let block_peak = self.block_peak;
            self.block_sum = 0.0;
            self.block_peak = 0.0;
            self.block_count = 0;

            // A knock is a short loud crack; it must not pull the level down,
            // or the words after it come out quiet for a second or two
            let crest = 20.0 * ((block_peak + 1e-9) / (block_rms + 1e-9)).log10();

            // A block this far below the loudest recent one is the room, a
            // keyboard click or the book in the background, not a word. Lifting
            // it is exactly how a pause arrives 12 dB louder than it left, and
            // why the same room was audible in the A/B takes but not in the raw.
            // The reference decays slowly (0.5 % per window), so a long gap does
            // not drag it down to the room level and turn the room into speech.
            self.peak = (self.peak * Self::PEAK_DECAY).max(block_rms);
            let speech_level = block_rms > self.peak * 10f64.powf(-Self::SPEECH_WINDOW_DB / 20.0);

            // ...and even inside a phrase the held gain is applied only where
            // the block is close to the speech level: a breath between two words
            // used to receive the full +12 dB of a phrase that was quieter.
            self.apply = self.target_present
                && block_rms > 1.8e-3
                && block_rms > self.peak * 10f64.powf(-Self::APPLY_WINDOW_DB / 20.0);

            if speech_level && block_rms > 1.8e-3 && crest < 14.0 && self.target_present {
                self.rms = if self.rms <= 0.0 {
                    block_rms
                } else {
                    self.rms * 0.9 + block_rms * 0.1
                };
                let rms_db = 20.0 * self.rms.log10();
                let level_db = 20.0 * block_rms.log10();
                // A block far above the phrase average is a shout, a word after
                // a pause, or another voice that the model let through: it must
                // not carry the +18 dB a quiet passage asked for. Measure the
                // gain from this block instead, and let it come down fast.
                let wanted = if level_db > rms_db + Self::SHOUT_DB {
                    (self.target_db - level_db).clamp(self.min_gain_db, self.max_gain_db)
                } else {
                    (self.target_db - rms_db).clamp(self.min_gain_db, self.max_gain_db)
                };
                // Rate-limited in both directions. Snapping to a single 100 ms
                // window at the start of every phrase (the old `fresh`) is what
                // made one sentence 7 dB louder than the next: a soft first
                // window asked for +18 dB, and the whole phrase got it. 0.8 dB
                // per window is 8 dB/s upwards — it follows the room, not the
                // syllable. Downwards it may travel 4 dB per window: pulling a
                // loud phrase back cannot lift the noise, and at 8 dB/s the
                // first second after a quiet passage arrived far too loud.
                let limit = if self.warmup > 0 {
                    self.warmup -= 1;
                    Self::WARMUP_DB
                } else {
                    Self::STEP_DB
                };
                let step = (wanted - self.gain_db).clamp(-Self::FALL_DB, limit);
                self.gain_db += step;
                self.target_lin = 10f64.powf(self.gain_db / 20.0);
            }
        }

        self.gain_lin += (self.target_lin - self.gain_lin) * self.smooth_coef;

        x * self.gain_lin
    }

    pub fn reset(&mut self) {
        self.block_sum = 0.0;
        self.block_count = 0;
        self.rms = 0.0;
        self.peak = 0.0;
        self.gain_db = 0.0;
        self.gain_lin = 1.0;
        self.apply = false;
        self.warmup = Self::WARMUP_WINDOWS;
    }
}

/// Removes DC and subsonic rumble *before* the leveler and the model.
///
/// The post-model high-pass cannot help here: a desk thump or wind already
/// moved the leveler and pushed the model input around by the time it runs.
/// 40 Hz, 4th order, is below the owner's 96-110 Hz fundamental: at 96 Hz it is
/// within 0.1 dB, while 20 Hz loses 24 dB. No notch at 100/200 Hz — that
/// experiment cut the fundamental and made vowels hollow.
pub struct InputFilter {
    hp1: Biquad,
    hp2: Biquad,
}

impl InputFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            hp1: Biquad::default(),
            hp2: Biquad::default(),
        };
        filter.configure(sample_rate);
        filter
    }

    pub fn configure(&mut self, sample_rate: f32) {
        self.hp1.high_pass(sample_rate, 40.0, 0.5412);
        self.hp2.high_pass(sample_rate, 40.0, 1.3066);
    }

    pub fn process(&mut self, block: &mut [f32]) {
        for sample in block.iter_mut() {
            let x = *sample as f64;
            *sample = self.hp2.process(self.hp1.process(x)) as f32;
        }
    }

    pub fn reset(&mut self) {
        self.hp1.reset();
        self.hp2.reset();
    }
}

pub struct Limiter {
    gain: f64,
    ceiling: f64,
    pub ceiling_db: f64,
}
impl Default for Limiter {
    fn default() -> Self {
        Self {
            gain: 1.0,
            ceiling: 10f64.powf(-1.0 / 20.0),
            ceiling_db: -1.0,
        }
    }
}

impl Limiter {
    #[inline]
    pub fn process(&mut self, x: f64, sample_rate: f64) -> f64 {
        let abs = x.abs();

        if abs > 1e-9 && abs * self.gain > self.ceiling {
            self.gain = self.gain.min(self.ceiling / abs);
        } else {
            let release = 1.0 - (-1000.0 / 40.0 / sample_rate).exp();
            self.gain += (1.0 - self.gain) * release;
        }

        x * self.gain
    }

    pub fn prepare(&mut self) {
        self.ceiling = 10f64.powf(self.ceiling_db / 20.0);
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
    }
}

pub struct Chain {
    sample_rate: f64,
    high_pass: Biquad,
    high_pass2: Biquad,
    gate: Gate,
    agc: Agc,
    limiter: Limiter,
    /// Final safety limiter after the output volume and comfort noise. The
    /// output gain sits after `limiter`, so a positive volume used to run
    /// straight into the hard `clamp(-1.0, 1.0)`: a loud syllable clipped on the
    /// very last sample. This one is idle (gain 1.0) whenever the signal stays
    /// under the ceiling, so the shipped -3 dB setting is still bit-exact.
    safety: Limiter,
    /// tone control: body shelf at 200 Hz and presence shelf at 3 kHz, both
    /// after the AGC and before the limiter
    tone_low: Biquad,
    tone_high: Biquad,
    /// transient (keyboard click, knock) suppressor state
    click_gain: f64,
    click_hold: u32,
    click_attack: f64,
    click_release: f64,
    silence_env: f64,
    agc_mix: f64,
    agc_mix_coef: f64,

    high_pass_enabled: bool,
    high_pass_hz: f32,
    tone_enabled: bool,
    tone_low_db: f32,
    tone_high_db: f32,
    /// a shelf at 0 dB is the identity filter, so it is skipped entirely: a
    /// settings file without the tone fields keeps the chain bit-exact
    tone_low_active: bool,
    tone_high_active: bool,
    gate_enabled: bool,
    agc_enabled: bool,
    limiter_enabled: bool,
    comfort_enabled: bool,
    comfort_level: f32,
    output_gain_db: f32,
    noise_state: u64,
}

impl Chain {
    /// speech stays under about 14 dB of crest; a keyboard click or a knock is
    /// a 17-25 dB crack and is what "typing in the background" sounds like
    const CLICK_CREST_DB: f64 = 17.0;
    /// a click is not a word: 14 dB down keeps it recognisable but not annoying
    const CLICK_GAIN: f64 = 0.2;
    /// how long the duck lasts after the block that carried the click
    const CLICK_HOLD_MS: f64 = 50.0;
    /// Presence sits at 3 kHz because consonants live at 1-4 kHz and that is
    /// where the model plus the residual denoiser take the most out. Above
    /// 8 kHz there is nothing to lift: the model runs at 16 kHz, so the band
    /// the resampler stretches to 48 kHz is empty.
    const TONE_HIGH_HZ: f32 = 3000.0;
    /// Body sits at 200 Hz. Below the 100 Hz high-pass a boost only brings back
    /// the rumble that filter removed, so the low shelf is mostly a cut.
    const TONE_LOW_HZ: f32 = 200.0;

    pub fn new() -> Self {
        let mut chain = Self {
            sample_rate: 48000.0,
            high_pass: Biquad::default(),
            high_pass2: Biquad::default(),
            gate: Gate::default(),
            agc: Agc::default(),
            limiter: Limiter::default(),
            safety: Limiter::default(),
            tone_low: Biquad::default(),
            tone_high: Biquad::default(),
            click_gain: 1.0,
            click_hold: 0,
            click_attack: 0.0,
            click_release: 0.0,
            silence_env: 0.0,
            // the crossfade starts fully dry: the AGC has no gain yet, so there
            // is nothing to fade in
            agc_mix: 0.0,
            agc_mix_coef: 0.0,
            high_pass_enabled: true,
            high_pass_hz: 85.0,
            tone_enabled: true,
            tone_low_db: 0.0,
            tone_high_db: 0.0,
            tone_low_active: false,
            tone_high_active: false,
            gate_enabled: true,
            agc_enabled: true,
            limiter_enabled: true,
            comfort_enabled: false,
            comfort_level: -66.0,
            output_gain_db: 0.0,
            noise_state: 0x2545F4914F6CDD1D,
        };
        chain.configure(&Settings::default(), 48000);
        chain
    }

    pub fn configure(&mut self, settings: &Settings, sample_rate: u32) {
        self.sample_rate = sample_rate as f64;
        self.high_pass_enabled = settings.high_pass_enabled;
        self.high_pass_hz = settings.high_pass_hz;
        self.tone_enabled = settings.tone_enabled;
        self.tone_low_db = settings.tone_low_db;
        self.tone_high_db = settings.tone_high_db;
        self.gate_enabled = settings.gate_enabled;
        self.agc_enabled = settings.agc_enabled;
        self.limiter_enabled = settings.limiter_enabled;
        self.comfort_enabled = settings.comfort_noise_enabled;
        self.comfort_level = settings.comfort_noise_db;
        self.output_gain_db = settings.output_gain_db;

        self.gate.threshold_db = settings.gate_threshold_db as f64;
        self.gate.ratio = settings.gate_ratio as f64;
        self.gate.attack_ms = settings.gate_attack_ms as f64;
        self.gate.release_ms = settings.gate_release_ms as f64;
        self.gate.hold_ms = settings.gate_hold_ms as f64;
        self.gate.prepare();

        self.agc.target_db = settings.agc_target_db as f64;
        self.agc.max_gain_db = settings.agc_max_gain_db as f64;

        self.limiter.prepare();
        // the final guard: whatever the output volume, click suppressor and
        // comfort noise produced must still leave the chain under the ceiling
        self.safety.prepare();

        // 4th order Butterworth: one biquad alone lets 40-60 Hz rumble through
        self.high_pass
            .high_pass(sample_rate as f32, self.high_pass_hz, 0.5412);
        self.high_pass2
            .high_pass(sample_rate as f32, self.high_pass_hz, 1.3066);
        // No notch at 100/200 Hz. The owner's F0 is 96-110 Hz, so a Q=12
        // notch at 100 Hz cut the fundamental by 18 dB and rotated its phase
        // by several milliseconds against the harmonics: vowels lost their
        // body and read as hollow/resonant. What looked like "mains hum" in
        // the band share was the voice itself; the microphone now has only
        // about -60 dBFS of hum in pauses, which the gate covers.

        // A shelf set to 0 dB is an identity filter; skipping it keeps the
        // output bit-exact for every settings file written before the control
        // existed, so the golden test has nothing to drift on.
        self.tone_low_active = self.tone_enabled && self.tone_low_db.abs() >= 0.05;
        self.tone_high_active = self.tone_enabled && self.tone_high_db.abs() >= 0.05;
        if self.tone_low_active {
            self.tone_low.low_shelf(
                sample_rate as f32,
                Self::TONE_LOW_HZ,
                self.tone_low_db as f64,
            );
        }
        if self.tone_high_active {
            self.tone_high.high_shelf(
                sample_rate as f32,
                Self::TONE_HIGH_HZ,
                self.tone_high_db as f64,
            );
        }
        // 30 ms crossfade between "no level change" and the AGC output
        self.agc_mix_coef = 1.0 - (-1000.0 / 30.0 / sample_rate as f64).exp();
        // a keyboard click is a 1-3 ms crack: duck it within a millisecond and
        // let go in 30 ms, so a word starting right after is not swallowed
        self.click_attack = 1.0 - (-1000.0 / 1.0 / sample_rate as f64).exp();
        self.click_release = 1.0 - (-1000.0 / 30.0 / sample_rate as f64).exp();
    }

    /// The engine tells the chain whether the model thinks the target speaker
    /// is in the current block; the AGC only lifts the level for that speaker,
    /// so residual noise and other voices are not amplified.
    pub fn set_target_present(&mut self, present: bool) {
        self.agc.target_present = present;
    }

    pub fn reset(&mut self) {
        self.high_pass.reset();
        self.high_pass2.reset();
        self.tone_low.reset();
        self.tone_high.reset();
        self.gate.reset();
        self.agc.reset();
        self.limiter.reset();
        self.click_gain = 1.0;
        self.click_hold = 0;
        self.silence_env = 0.0;
    }

    /// Current AGC gain, for diagnostics (`nc replay` prints whether the
    /// control is actually moving or sitting at its limit).
    pub fn agc_gain_db(&self) -> f64 {
        self.agc.gain_db
    }

    /// Processes interleaved-free mono samples in place.
    pub fn process(&mut self, buffer: &mut [f32]) {
        let sr = self.sample_rate;
        let gain = 10f64.powf(self.output_gain_db as f64 / 20.0);
        let comfort_threshold = 10f64.powf(((self.comfort_level + 6.0) as f64) / 20.0);

        // A keyboard click or a knock is a crack of 17-25 dB crest; speech stays
        // under 14 dB even at its loudest syllable. Ducking the block that
        // carries one keeps the click (and the room it excites) out of the way
        // without touching words.
        let peak = buffer.iter().fold(0.0f64, |m, v| m.max((*v as f64).abs()));
        let rms = (buffer
            .iter()
            .map(|v| {
                let x = *v as f64;
                x * x
            })
            .sum::<f64>()
            / buffer.len().max(1) as f64)
            .sqrt();
        let crest = 20.0 * ((peak + 1e-9) / (rms + 1e-9)).log10();
        if crest > Self::CLICK_CREST_DB {
            self.click_hold = (sr * Self::CLICK_HOLD_MS / 1000.0) as u32;
        }

        for sample in buffer.iter_mut() {
            let dry = *sample as f64;
            let abs = dry.abs();
            self.silence_env +=
                (abs - self.silence_env) * if abs > self.silence_env { 0.01 } else { 0.0002 };

            let mut x = dry;
            if self.high_pass_enabled {
                x = self.high_pass2.process(self.high_pass.process(x));
            }
            // gate first, level second: the gate must see the model output at
            // its natural level, otherwise the AGC normalises noise up to the
            // speech level and nothing can separate them afterwards
            if self.gate_enabled {
                x = self.gate.process(x, sr);
            }
            if self.agc_enabled {
                let dry = x;
                let wet = self.agc.process(x, sr);
                // only lift blocks the model considers the target speaker; the
                // rest passes without the held AGC gain, so noise stays down
                let target = if self.agc.apply { 1.0 } else { 0.0 };
                self.agc_mix += (target - self.agc_mix) * self.agc_mix_coef;
                x = dry + (wet - dry) * self.agc_mix;
            }
            // Tone sits after the AGC and before the limiter. In front of the
            // AGC it would be measured as loudness and pulled back within
            // 100 ms, so the slider would look like it does nothing; behind
            // the limiter the boost would clip.
            if self.tone_low_active {
                x = self.tone_low.process(x);
            }
            if self.tone_high_active {
                x = self.tone_high.process(x);
            }
            if self.limiter_enabled {
                x = self.limiter.process(x, sr);
            }

            x *= gain;

            let duck = if self.click_hold > 0 {
                Self::CLICK_GAIN
            } else {
                1.0
            };
            let click_coef = if duck < self.click_gain {
                self.click_attack
            } else {
                self.click_release
            };
            self.click_gain += (duck - self.click_gain) * click_coef;
            self.click_hold = self.click_hold.saturating_sub(1);
            x *= self.click_gain;

            if self.comfort_enabled && self.silence_env < comfort_threshold {
                let ramp = 1.0 - self.silence_env / comfort_threshold.max(1e-9);
                x += self.next_noise() * ramp;
            }

            x = self.safety.process(x, sr);
            *sample = (x as f32).clamp(-1.0, 1.0);
        }
    }

    fn next_noise(&mut self) -> f64 {
        // xorshift + one-pole low-pass: quiet, unobtrusive floor
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 7;
        self.noise_state ^= self.noise_state << 17;
        let white = ((self.noise_state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
        let amplitude = 10f64.powf(self.comfort_level as f64 / 20.0) * 0.5;
        white * amplitude
    }
}

impl Default for Chain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(gate_threshold_db: f64, rms: f32) -> f64 {
        let mut gate = Gate {
            threshold_db: gate_threshold_db,
            ratio: 2.0,
            attack_ms: 10.0,
            release_ms: 300.0,
            hold_ms: 150.0,
            ..Gate::default()
        };
        gate.prepare();

        let amplitude = rms * std::f32::consts::SQRT_2;
        let mut phase = 0.0f64;
        let mut out_rms = 0.0;

        for _ in 0..40 {
            let mut sum = 0.0;
            for _ in 0..4800 {
                phase += 0.05;
                let x = (amplitude as f64) * phase.sin();
                let y = gate.process(x, 48_000.0);
                sum += y * y;
            }
            out_rms = (sum / 4800.0).sqrt();
        }

        out_rms
    }

    #[test]
    fn a_narrow_notch_removes_its_center_frequency() {
        let run = |freq: f64| {
            let mut n1 = Biquad::default();
            n1.notch(48_000.0, 100.0, 12.0);
            let mut n2 = Biquad::default();
            n2.notch(48_000.0, 200.0, 18.0);
            let n = 48_000;
            let mut rms = 0.0;
            for i in 0..n {
                let x = 0.5 * (2.0 * std::f64::consts::PI * freq * i as f64 / 48_000.0).sin();
                let y = n2.process(n1.process(x));
                if i > 4800 {
                    rms += y * y;
                }
            }
            (rms / (n - 4800) as f64).sqrt()
        };
        let voice = run(1_000.0);
        for hz in [100.0, 200.0] {
            let hum = run(hz);
            assert!(
                hum < voice * 0.2,
                "{hz} Hz should be notched: {hum:.4} vs {voice:.4}"
            );
        }
    }

    #[test]
    fn gate_keeps_quiet_speech() {
        // -50 dBFS is quiet but still speech; it must pass almost untouched
        let out_db = 20.0 * run(-60.0, 0.0032).log10();
        assert!(
            out_db > -53.0,
            "quiet speech was cut down to {out_db:.1} dBFS"
        );
    }

    #[test]
    fn gate_closes_on_room_noise() {
        // -80 dBFS is below the threshold and has to be pushed down
        let out_db = 20.0 * run(-60.0, 0.0001).log10();
        assert!(out_db < -72.0, "noise floor stayed at {out_db:.1} dBFS");
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn agc_lifts_quiet_speech_to_the_target() {
        let mut agc = Agc::default();
        agc.target_db = -24.0;
        agc.max_gain_db = 12.0;
        agc.target_present = true;
        // 3 s of a -45 dBFS tone: the model cut the voice, the AGC has to
        // bring it back towards the target
        for n in 0..144_000 {
            let x = (2.0 * std::f64::consts::PI * 220.0 * n as f64 / 48_000.0).sin() * 0.0056;
            let _ = agc.process(x, 48_000.0);
        }
        assert!(agc.gain_db > 6.0, "agc stayed at {:.1} dB", agc.gain_db);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn agc_gain_is_smoothed_per_sample() {
        // a block-constant gain steps once per detector window; that step is
        // audible as a quiet click in the voice
        let mut agc = Agc::default();
        let mut previous = agc.gain_lin;
        let mut biggest = 0.0f64;

        // loud, then quiet: the gain has to move, and it has to move smoothly
        for n in 0..96_000 {
            let x = if n < 48_000 { 0.5 } else { 0.05 };
            let _ = agc.process(x, 48_000.0);
            biggest = biggest.max((agc.gain_lin - previous).abs());
            previous = agc.gain_lin;
        }

        assert!(
            biggest < 0.005,
            "gain stepped by {biggest:.4} in one sample"
        );
    }

    /// Feeds `seconds` of a 220 Hz tone whose RMS is `rms` through the AGC.
    fn run_agc(agc: &mut Agc, rms: f64, seconds: f64) {
        let n = (48_000.0 * seconds) as usize;
        for i in 0..n {
            let x = (2.0 * std::f64::consts::PI * 220.0 * i as f64 / 48_000.0).sin()
                * rms
                * std::f64::consts::SQRT_2;
            let _ = agc.process(x, 48_000.0);
        }
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn agc_pulls_a_loud_block_down() {
        // the level control used to be lift-only, so a phrase already above the
        // target kept whatever gain the quiet phrase before it had learned
        let mut agc = Agc::default();
        agc.target_db = -24.0;
        agc.max_gain_db = 18.0;
        agc.target_present = true;

        run_agc(&mut agc, 0.316, 3.0); // about -10 dBFS rms, 14 dB above target
        assert!(agc.gain_db < -3.0, "agc stayed at {:.1} dB", agc.gain_db);
        assert!(agc.gain_db >= agc.min_gain_db);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn agc_does_not_rise_on_a_new_phrase() {
        // a soft first window used to set the whole phrase's gain in one go;
        // that is what made one sentence 7 dB louder than the next. The gain may
        // come down quickly now, but it must still not jump *up*.
        let mut agc = Agc::default();
        agc.target_db = -24.0;
        agc.max_gain_db = 18.0;
        agc.target_present = true;

        run_agc(&mut agc, 0.2, 2.0); // a loud phrase settles the gain near 0
        let before = agc.gain_db;
        run_agc(&mut agc, 0.016, 0.1); // a quiet window at the start of a phrase
        assert!(
            agc.gain_db - before < 1.5,
            "the gain jumped {:.1} dB up at a phrase start",
            agc.gain_db - before
        );
    }

    #[test]
    fn agc_pulls_a_loud_phrase_down_at_once() {
        // the other way round: after a quiet passage the held gain is high, and
        // the first loud word used to arrive 10+ dB too loud for a full second
        let mut agc = Agc {
            target_db: -24.0,
            max_gain_db: 18.0,
            target_present: true,
            ..Default::default()
        };

        run_agc(&mut agc, 0.016, 2.0);
        let held = agc.gain_db;
        assert!(
            held > 10.0,
            "the quiet passage should ask for gain, got {held:.1}"
        );
        run_agc(&mut agc, 0.2, 0.1); // one window of a loud phrase
        assert!(
            held - agc.gain_db >= 3.0,
            "the gain fell only {:.1} dB: the loud phrase stays loud",
            held - agc.gain_db
        );
    }

    fn rms_db(buffer: &[f32]) -> f64 {
        let sum: f64 = buffer.iter().map(|v| (*v as f64) * (*v as f64)).sum();
        20.0 * ((sum / buffer.len().max(1) as f64).sqrt() + 1e-12).log10()
    }

    /// A chain that only runs the click suppressor: no gate, AGC, limiter or gain.
    fn plain_chain() -> Chain {
        let mut chain = Chain::new();
        let settings = Settings {
            high_pass_enabled: false,
            gate_enabled: false,
            agc_enabled: false,
            limiter_enabled: false,
            comfort_noise_enabled: false,
            output_gain_db: 0.0,
            ..Settings::default()
        };
        chain.configure(&settings, 48_000);
        chain
    }

    #[test]
    fn a_keyboard_click_is_ducked_and_speech_is_not() {
        let mut chain = plain_chain();
        // a click: one spike of 0.5 in an otherwise quiet block
        let mut click = vec![0.0005f32; 3072];
        click[100] = 0.5;
        let before = rms_db(&click);
        chain.process(&mut click);
        let after = rms_db(&click);
        assert!(
            after < before - 8.0,
            "the click was not ducked: {before:.1} -> {after:.1} dBFS"
        );

        // speech: a tone with a natural crest, twenty blocks in a row
        let mut chain = plain_chain();
        for _ in 0..20 {
            let mut block: Vec<f32> = (0..3072).map(|i| (i as f32 * 0.05).sin() * 0.05).collect();
            let before = rms_db(&block);
            chain.process(&mut block);
            let after = rms_db(&block);
            assert!(
                (after - before).abs() < 1.0,
                "speech changed: {before:.1} -> {after:.1} dBFS"
            );
        }
    }

    /// A chain that only runs the tone shelves: no high-pass, gate, AGC,
    /// limiter or gain, so the measured change is the shelf and nothing else.
    fn tone_chain(low_db: f32, high_db: f32) -> Chain {
        let mut chain = Chain::new();
        let settings = Settings {
            high_pass_enabled: false,
            gate_enabled: false,
            agc_enabled: false,
            limiter_enabled: false,
            comfort_noise_enabled: false,
            output_gain_db: 0.0,
            tone_enabled: true,
            tone_low_db: low_db,
            tone_high_db: high_db,
            ..Settings::default()
        };
        chain.configure(&settings, 48_000);
        chain
    }

    /// Change in dB of a sine at `hz` after the chain has settled.
    fn tone_change_db(chain: &mut Chain, hz: f64) -> f64 {
        let sine = |from: usize, len: usize| -> Vec<f32> {
            (from..from + len)
                .map(|i| {
                    (2.0 * std::f64::consts::PI * hz * i as f64 / 48_000.0).sin() as f32 * 0.05
                })
                .collect()
        };
        let mut warmup = sine(0, 12_000);
        chain.process(&mut warmup);
        let reference = rms_db(&sine(12_000, 12_000));
        let mut measured = sine(12_000, 12_000);
        chain.process(&mut measured);
        rms_db(&measured) - reference
    }

    #[test]
    fn a_tone_shelf_at_zero_db_changes_nothing() {
        // every settings file written before the control existed lands here,
        // and the chain has to sound exactly as it did
        let mut chain = tone_chain(0.0, 0.0);
        for hz in [400.0, 3_000.0, 6_000.0] {
            let change = tone_change_db(&mut chain, hz);
            assert!(change.abs() < 0.05, "{hz} Hz moved by {change:.3} dB");
        }
    }

    #[test]
    fn presence_lifts_consonants_more_than_the_corner() {
        // the corner of a shelf sits at half its gain, so 3 kHz moves ~3 dB and
        // 8 kHz gets almost the full +6
        let mut chain = tone_chain(0.0, 6.0);
        let corner = tone_change_db(&mut chain, 3_000.0);
        let high = tone_change_db(&mut chain, 8_000.0);
        assert!(
            (2.0..=4.0).contains(&corner),
            "3 kHz moved by {corner:.1} dB"
        );
        assert!((4.5..=6.5).contains(&high), "8 kHz moved by {high:.1} dB");
    }

    #[test]
    fn the_body_shelf_is_mostly_a_cut() {
        // +3 dB is the ceiling in the settings: everything below 100 Hz is the
        // rumble the high-pass just removed
        let mut chain = tone_chain(-6.0, 0.0);
        let low = tone_change_db(&mut chain, 80.0);
        assert!((low + 6.0).abs() < 1.5, "80 Hz moved by {low:.1} dB");
    }

    #[test]
    fn the_input_filter_removes_rumble_but_keeps_the_fundamental() {
        let run = |hz: f64| {
            let mut filter = InputFilter::new(16_000.0);
            let mut block: Vec<f32> = (0..16_000)
                .map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / 16_000.0).sin() as f32 * 0.1)
                .collect();
            // let the filter settle, then measure the last half second
            filter.process(&mut block[..8_000]);
            let mut tail = block[8_000..].to_vec();
            filter.process(&mut tail);
            20.0 * (tail.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / tail.len() as f64)
                .sqrt()
                .log10()
        };
        let voice = run(96.0);
        let rumble = run(20.0);
        assert!(voice > -24.0, "96 Hz was cut: {voice:.1} dBFS");
        assert!(
            rumble < voice - 18.0,
            "20 Hz was not removed: {rumble:.1} vs {voice:.1} dBFS"
        );
    }

    #[test]
    fn a_positive_output_volume_cannot_clip() {
        // +18 dB used to be applied after the limiter and then hard-clamped:
        // every loud syllable lost its top. The safety limiter must keep the
        // block under full scale without reaching for `clamp`.
        let mut chain = Chain::new();
        let settings = Settings {
            high_pass_enabled: false,
            gate_enabled: false,
            agc_enabled: false,
            limiter_enabled: true,
            comfort_noise_enabled: false,
            output_gain_db: 18.0,
            ..Settings::default()
        };
        chain.configure(&settings, 48_000);

        let mut block: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.07).sin() * 0.3).collect();
        chain.process(&mut block);

        let peak = block.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= 1.0, "output reached {peak}");
        assert!(peak < 0.999, "the safety limiter did not engage: {peak}");
        assert!(block.iter().all(|v| v.is_finite()));
    }
}
