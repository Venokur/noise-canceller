//! Post-isolation clean-up: high-pass, expander, AGC, limiter, optional comfort noise.
//! Deterministic, no allocations in `process`.

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

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
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

        let time = if target > self.gain { self.attack_ms } else { self.release_ms };
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
    block_sum: f64,
    block_peak: f64,
    block_len: usize,
    block_count: usize,
    rms: f64,
    gain_db: f64,
    gain_lin: f64,
    target_lin: f64,
    smooth_coef: f64,
    smooth_rate: f64,
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
            block_sum: 0.0,
            block_peak: 0.0,
            block_len: 0,
            block_count: 0,
            rms: 0.0,
            gain_db: 0.0,
            gain_lin: 1.0,
            target_lin: 1.0,
            smooth_coef: 0.0,
            smooth_rate: 0.0,
            target_db: -24.0,
            max_gain_db: 12.0,
            min_gain_db: 0.0,
        }
    }
}

impl Agc {
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

            // a knock is a short loud crack; it must not pull the level down,
            // or the words after it come out quiet for a second or two
            let crest = 20.0 * ((block_peak + 1e-9) / (block_rms + 1e-9)).log10();

            if block_rms > 1.8e-3 && crest < 14.0 && self.target_present {
                self.rms = if self.rms <= 0.0 { block_rms } else { self.rms * 0.9 + block_rms * 0.1 };
                let rms_db = 20.0 * self.rms.log10();
                let wanted = (self.target_db - rms_db).clamp(self.min_gain_db, self.max_gain_db);
                // drop fast, lift slowly: a loud syllable must be caught at once,
                // but raising the level over a couple of seconds is what keeps
                // the loudness even instead of jumping between words
                let step = wanted - self.gain_db;
                let coef = if step < 0.0 { 0.5 } else { 0.25 };
                self.gain_db += step.clamp(-5.0, 5.0) * coef;
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
        self.gain_db = 0.0;
        self.gain_lin = 1.0;
    }
}

pub struct Limiter {
    gain: f64,
    ceiling: f64,
    pub ceiling_db: f64,
}

impl Default for Limiter {
    fn default() -> Self {
        Self { gain: 1.0, ceiling: 10f64.powf(-1.0 / 20.0), ceiling_db: -1.0 }
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
    silence_env: f64,
    agc_mix: f64,
    agc_mix_coef: f64,

    high_pass_enabled: bool,
    high_pass_hz: f32,
    gate_enabled: bool,
    agc_enabled: bool,
    limiter_enabled: bool,
    comfort_enabled: bool,
    comfort_level: f32,
    output_gain_db: f32,
    noise_state: u64,
}

impl Chain {
    pub fn new() -> Self {
        let mut chain = Self {
            sample_rate: 48000.0,
            high_pass: Biquad::default(),
            high_pass2: Biquad::default(),
            gate: Gate::default(),
            agc: Agc::default(),
            limiter: Limiter::default(),
            silence_env: 0.0,
            // the crossfade starts fully dry: the AGC has no gain yet, so there
            // is nothing to fade in
            agc_mix: 0.0,
            agc_mix_coef: 0.0,
            high_pass_enabled: true,
            high_pass_hz: 85.0,
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

        // 4th order Butterworth: one biquad alone lets 40-60 Hz rumble through
        self.high_pass.high_pass(sample_rate as f32, self.high_pass_hz, 0.5412);
        self.high_pass2.high_pass(sample_rate as f32, self.high_pass_hz, 1.3066);
        // 30 ms crossfade between "no level change" and the AGC output
        self.agc_mix_coef = 1.0 - (-1000.0 / 30.0 / sample_rate as f64).exp();
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
        self.gate.reset();
        self.agc.reset();
        self.limiter.reset();
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

        for sample in buffer.iter_mut() {
            let dry = *sample as f64;
            let abs = dry.abs();
            self.silence_env += (abs - self.silence_env) * if abs > self.silence_env { 0.01 } else { 0.0002 };

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
                let target = if self.agc.target_present { 1.0 } else { 0.0 };
                self.agc_mix += (target - self.agc_mix) * self.agc_mix_coef;
                x = dry + (wet - dry) * self.agc_mix;
            }
            if self.limiter_enabled {
                x = self.limiter.process(x, sr);
            }

            x *= gain;

            if self.comfort_enabled && self.silence_env < comfort_threshold {
                let ramp = 1.0 - self.silence_env / comfort_threshold.max(1e-9);
                x += self.next_noise() * ramp;
            }

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

    fn run(gate_threshold_db: f64, rms: f32) -> f64 {        let mut gate = Gate {
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
    fn gate_keeps_quiet_speech() {
        // -50 dBFS is quiet but still speech; it must pass almost untouched
        let out_db = 20.0 * run(-60.0, 0.0032).log10();
        assert!(out_db > -53.0, "quiet speech was cut down to {out_db:.1} dBFS");
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

        assert!(biggest < 0.005, "gain stepped by {biggest:.4} in one sample");
    }
}
