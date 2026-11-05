//! Polyphase windowed-sinc resampler with a persistent phase table.
//!
//! Own implementation instead of `rubato` for two reasons: it is the exact
//! algorithm already validated against the reference implementation, and the
//! caller needs direct control over the input/output sample accounting that
//! the two clock domains (microphone and virtual cable) require.

pub struct Resampler {
    up: usize,
    down: usize,
    half: usize,
    phases: Vec<Vec<f32>>,
    buffer: Vec<f32>,
    base: i64,
    out_index: i64,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self::with_taps(in_rate, out_rate, 24)
    }

    pub fn with_taps(in_rate: u32, out_rate: u32, half: usize) -> Self {
        let g = gcd(in_rate, out_rate);
        let up = (out_rate / g) as usize;
        let down = (in_rate / g) as usize;

        let phases = build_phases(up, down, half);

        let mut buffer = Vec::with_capacity(2 * half + 8192);
        buffer.resize(half, 0.0);

        Self {
            up,
            down,
            half,
            phases,
            buffer,
            base: -(half as i64),
            out_index: 0,
        }
    }

    pub fn max_output(&self, input: usize) -> usize {
        input * self.up / self.down + 4
    }

    /// Processes `input` and writes into `output`; returns the number of samples written.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        self.buffer.extend_from_slice(input);

        let half = self.half as i64;
        let up = self.up as i64;
        let down = self.down as i64;
        let mut produced = 0usize;

        while produced < output.len() {
            let t = self.out_index * down / up;
            let phase = ((self.out_index * down) % up) as usize;
            let lo = t - half + 1;
            let hi = t + half;

            if hi >= self.base + self.buffer.len() as i64 {
                break;
            }
            if lo < self.base {
                self.out_index += 1;
                continue;
            }

            let coeffs = &self.phases[phase];
            let start = (lo - self.base) as usize;
            let mut acc = 0.0f32;
            for (k, c) in coeffs.iter().enumerate() {
                acc += self.buffer[start + k] * c;
            }

            output[produced] = acc;
            produced += 1;
            self.out_index += 1;
        }

        // drop history that can never be requested again
        let next_t = self.out_index * down / up;
        let keep_from = next_t - half + 1;
        let drop = (keep_from - self.base).max(0) as usize;
        if drop > 0 {
            if drop >= self.buffer.len() {
                self.base += self.buffer.len() as i64;
                self.buffer.clear();
            } else {
                self.buffer.drain(0..drop);
                self.base += drop as i64;
            }
        }

        produced
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.buffer.resize(self.half, 0.0);
        self.base = -(self.half as i64);
        self.out_index = 0;
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

fn build_phases(up: usize, down: usize, half: usize) -> Vec<Vec<f32>> {
    let fc = 0.5 * 0.94 * (up as f64 / down as f64).min(1.0);
    let mut phases = Vec::with_capacity(up);

    for p in 0..up {
        let frac = p as f64 / up as f64;
        let mut coeffs = vec![0.0f32; 2 * half];
        let mut sum = 0.0f64;

        for (k, slot) in coeffs.iter_mut().enumerate() {
            let kk = k as i64 - half as i64 + 1;
            let x = frac - kk as f64;

            let (w, sinc) = if x.abs() >= half as f64 {
                (0.0, 0.0)
            } else {
                let w = 0.42
                    + 0.5 * (std::f64::consts::PI * x / half as f64).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * x / half as f64).cos();
                let arg = 2.0 * fc * x;
                let sinc = if arg.abs() < 1e-12 {
                    1.0
                } else {
                    (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
                };
                (w, sinc)
            };

            let v = 2.0 * fc * sinc * w;
            *slot = v as f32;
            sum += v;
        }

        if sum.abs() > 1e-9 {
            for c in coeffs.iter_mut() {
                *c = (*c as f64 / sum) as f32;
            }
        }

        phases.push(coeffs);
    }

    phases
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(in_rate: u32, out_rate: u32, chunk: usize, chunks: usize) -> usize {
        let mut resampler = Resampler::new(in_rate, out_rate);
        let mut out = vec![0.0f32; resampler.max_output(chunk) + 16];
        let mut total = 0;
        for _ in 0..chunks {
            let input = vec![0.0f32; chunk];
            total += resampler.process(&input, &mut out);
        }
        total
    }

    #[test]
    fn up_16k_to_48k_keeps_the_sample_budget() {
        // the live engine feeds model blocks of 1024 samples at 16 kHz and
        // expects exactly three times that at 48 kHz; more than that fills the
        // output ring until the latency hits its ceiling
        let produced = count(16_000, 48_000, 1024, 100);
        assert!(
            (produced as i64 - 307_200).abs() <= 480,
            "expected about 307200 samples, got {produced}"
        );
    }

    #[test]
    fn down_48k_to_16k_keeps_the_sample_budget() {
        // one second of audio in 10 ms callbacks must yield about 16000 samples;
        // the filter deliberately keeps a short tail in its history
        let produced = count(48_000, 16_000, 480, 100);
        assert!(
            (produced as i64 - 16_000).abs() <= 160,
            "expected about 16000 samples, got {produced}"
        );
    }

    #[test]
    fn up_16k_to_44k1_keeps_the_sample_budget() {
        // 1.02 s of model output, 60 ms blocks, must fill about 1.02 s of audio
        let produced = count(16_000, 44_100, 960, 17);
        assert!(
            (produced as i64 - 44_982).abs() <= 450,
            "expected about 44982 samples, got {produced}"
        );
    }
}
