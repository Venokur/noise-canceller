//! Small radix-2 FFT spectrum analyser for the level view (32 log bands).

const BANDS: usize = 32;
const PI: f32 = std::f32::consts::PI;

pub struct Spectrum {
    size: usize,
    hop: usize,
    window: Vec<f32>,
    buffer: Vec<f32>,
    /// FFT scratch, reused: allocating two 1024-sample vectors 190 times a
    /// second was ~1.5 MB/s of churn on the processing thread
    re: Vec<f32>,
    im: Vec<f32>,
    filled: usize,
    since_fft: usize,
    band_start: [usize; BANDS],
    band_end: [usize; BANDS],
    bands: [f32; BANDS],
}

impl Spectrum {
    pub fn new(size: usize, sample_rate: f32) -> Self {
        let size = size.max(128).next_power_of_two();
        let mut band_start = [0usize; BANDS];
        let mut band_end = [0usize; BANDS];

        let low = 70.0f32.ln();
        let high = (sample_rate * 0.45).min(7800.0).ln();
        let bin_hz = sample_rate / size as f32;

        for b in 0..BANDS {
            let f0 = (low + (high - low) * b as f32 / BANDS as f32).exp();
            let f1 = (low + (high - low) * (b + 1) as f32 / BANDS as f32).exp();
            band_start[b] = ((f0 / bin_hz) as usize).clamp(1, size / 2 - 1);
            band_end[b] = ((f1 / bin_hz) as usize).clamp(band_start[b] + 1, size / 2);
        }

        Self {
            size,
            hop: size / 4,
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / size as f32).cos())
                .collect(),
            buffer: vec![0.0; size],
            re: vec![0.0; size],
            im: vec![0.0; size],
            filled: 0,
            since_fft: 0,
            band_start,
            band_end,
            bands: [0.0; BANDS],
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        for sample in samples {
            if self.filled < self.size {
                self.buffer[self.filled] = *sample;
                self.filled += 1;
            } else {
                self.buffer.copy_within(1.., 0);
                self.buffer[self.size - 1] = *sample;
            }

            self.since_fft += 1;
            if self.filled >= self.size && self.since_fft >= self.hop {
                self.since_fft = 0;
                self.analyze();
            }
        }
    }

    pub fn bands(&self) -> [f32; BANDS] {
        self.bands
    }

    fn analyze(&mut self) {
        let n = self.size;
        self.im.fill(0.0);

        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            self.re[i] = self.buffer[i] * self.window[i];
        }

        fft(&mut self.re, &mut self.im);

        for b in 0..BANDS {
            let mut peak = 0.0f32;
            for bin in self.band_start[b]..self.band_end[b] {
                let mag = (self.re[bin] * self.re[bin] + self.im[bin] * self.im[bin]).sqrt();
                if mag > peak {
                    peak = mag;
                }
            }

            let db = 20.0 * (peak + 1e-9).log10();
            let normalized = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
            let previous = self.bands[b];
            self.bands[b] = if normalized > previous {
                normalized
            } else {
                previous * 0.82 + normalized * 0.18
            };
        }
    }
}

fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;

    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());

        let mut start = 0;
        while start < n {
            let mut cur_r = 1.0f32;
            let mut cur_i = 0.0f32;

            for k in 0..len / 2 {
                let u_r = re[start + k];
                let u_i = im[start + k];
                let v_r = re[start + k + len / 2] * cur_r - im[start + k + len / 2] * cur_i;
                let v_i = re[start + k + len / 2] * cur_i + im[start + k + len / 2] * cur_r;

                re[start + k] = u_r + v_r;
                im[start + k] = u_i + v_i;
                re[start + k + len / 2] = u_r - v_r;
                im[start + k + len / 2] = u_i - v_i;

                let next_r = cur_r * wr - cur_i * wi;
                cur_i = cur_r * wi + cur_i * wr;
                cur_r = next_r;
            }

            start += len;
        }

        len <<= 1;
    }
}
