//! Plays a recorded wav through an output device (profile preview).

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};

use crate::devices;
use crate::lang::Lang;
use crate::resample::Resampler;
use crate::wav;

/// Set by `stop()`; the stream fades to silence and `play` returns.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Which side of the A/B pair is audible: 0 = first (raw), 1 = second (clean).
/// The pair player crossfades between them, so switching during playback is
/// instant and always at the same timestamp.
static AB_SIDE: AtomicU8 = AtomicU8::new(0);

/// Switches the live A/B pair. Safe to call when nothing plays.
pub fn set_pair_side(clean: bool) {
    AB_SIDE.store(u8::from(clean), Ordering::Relaxed);
}

/// Stops whatever preview is playing. Safe to call when nothing plays.
pub fn stop() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Blocking preview: returns when the file has been played.
pub fn play(path: &std::path::Path, device_name: Option<&str>, lang: Lang) -> Result<()> {
    play_inner(path, device_name, None, lang)
}

/// Same, but first brings the speech in the file to `target_db` (dBFS RMS).
/// Used by the A/B buttons: the processed take is 10-15 dB louder than the raw
/// one, and that difference hides what the comparison is about.
pub fn play_normalized(
    path: &std::path::Path,
    device_name: Option<&str>,
    lang: Lang,
) -> Result<()> {
    play_inner(path, device_name, Some(-24.0), lang)
}

/// Plays a finished A/B pair from the same position and switches between the
/// two sides with a short crossfade. Both files are normalised to the same
/// speech level first, otherwise the processed take is 10-15 dB louder and the
/// comparison is about volume, not clarity.
pub fn play_pair(
    raw: &std::path::Path,
    clean: &std::path::Path,
    device_name: Option<&str>,
    normalize_levels: bool,
    initial_clean: bool,
    lang: Lang,
) -> Result<()> {
    CANCEL.store(false, Ordering::Relaxed);
    AB_SIDE.store(u8::from(initial_clean), Ordering::Relaxed);

    let (mut first, first_rate) = wav::read_mono(raw)?;
    let (mut second, second_rate) = wav::read_mono(clean)?;
    if first.is_empty() || second.is_empty() {
        return Err(anyhow!(lang.text("файл пуст", "the file is empty")));
    }
    if normalize_levels {
        normalize(&mut first, first_rate, -24.0);
        normalize(&mut second, second_rate, -24.0);
    }

    let host = cpal::default_host();
    let device = devices::pick_output(&host, device_name, lang)?;
    let config = device.default_output_config()?;
    let out_rate = config.sample_rate();
    let channels = config.channels() as usize;

    let first = resample_to(first, first_rate, out_rate);
    let second = resample_to(second, second_rate, out_rate);
    let len = first.len().max(second.len());
    let mut a = first;
    let mut b = second;
    a.resize(len, 0.0);
    b.resize(len, 0.0);

    let data = Arc::new((a, b));
    let cursor = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    // a 15 ms ramp hides the switch without smearing the comparison
    let blend = 1.0 - (-1000.0 / 15.0 / out_rate as f64).exp();

    fn err(error: cpal::Error) {
        eprintln!("playback error: {error}");
    }

    let stream = {
        let data = data.clone();
        let cursor = cursor.clone();
        let finished = finished.clone();

        match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config.config(),
                move |buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut mix = f32::from(AB_SIDE.load(Ordering::Relaxed));
                    for frame in buffer.chunks_mut(channels) {
                        if CANCEL.load(Ordering::Relaxed) {
                            for slot in frame.iter_mut() {
                                *slot = 0.0;
                            }
                            finished.store(true, Ordering::Relaxed);
                            continue;
                        }
                        let index = cursor.fetch_add(1, Ordering::Relaxed);
                        let target = f32::from(AB_SIDE.load(Ordering::Relaxed));
                        mix += (target - mix) * blend as f32;
                        let value = data.0.get(index).copied().unwrap_or(0.0) * (1.0 - mix)
                            + data.1.get(index).copied().unwrap_or(0.0) * mix;
                        for slot in frame.iter_mut() {
                            *slot = value;
                        }
                        if index >= data.0.len() {
                            finished.store(true, Ordering::Relaxed);
                        }
                    }
                },
                err,
                None,
            )?,
            cpal::SampleFormat::I16 => {
                let data = data.clone();
                let cursor = cursor.clone();
                let finished = finished.clone();
                device.build_output_stream(
                    config.config(),
                    move |buffer: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        let mut mix = f32::from(AB_SIDE.load(Ordering::Relaxed));
                        for frame in buffer.chunks_mut(channels) {
                            if CANCEL.load(Ordering::Relaxed) {
                                for slot in frame.iter_mut() {
                                    *slot = 0;
                                }
                                finished.store(true, Ordering::Relaxed);
                                continue;
                            }
                            let index = cursor.fetch_add(1, Ordering::Relaxed);
                            let target = f32::from(AB_SIDE.load(Ordering::Relaxed));
                            mix += (target - mix) * blend as f32;
                            let value = data.0.get(index).copied().unwrap_or(0.0) * (1.0 - mix)
                                + data.1.get(index).copied().unwrap_or(0.0) * mix;
                            let sample = (value * 32767.0) as i16;
                            for slot in frame.iter_mut() {
                                *slot = sample;
                            }
                            if index >= data.0.len() {
                                finished.store(true, Ordering::Relaxed);
                            }
                        }
                    },
                    err,
                    None,
                )?
            }
            other => return Err(anyhow!("unsupported output format: {other:?}")),
        }
    };

    stream.play()?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !finished.load(Ordering::Relaxed)
        && !CANCEL.load(Ordering::Relaxed)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    let tail = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < tail {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Ok(())
}

fn resample_to(samples: Vec<f32>, rate: u32, out_rate: u32) -> Vec<f32> {
    if rate == out_rate {
        return samples;
    }
    let mut resampler = Resampler::new(rate, out_rate);
    let mut out = vec![0.0f32; resampler.max_output(samples.len()) + 64];
    let count = resampler.process(&samples, &mut out);
    out.truncate(count);
    out
}

fn play_inner(
    path: &std::path::Path,
    device_name: Option<&str>,
    target_db: Option<f32>,
    lang: Lang,
) -> Result<()> {
    CANCEL.store(false, Ordering::Relaxed);
    let (mut samples, rate) = wav::read_mono(path)?;
    if samples.is_empty() {
        return Err(anyhow!(lang.text("файл пуст", "the file is empty")));
    }
    if let Some(target) = target_db {
        normalize(&mut samples, rate, target);
    }

    let host = cpal::default_host();
    let device = devices::pick_output(&host, device_name, lang)?;
    let config = device.default_output_config()?;
    let out_rate = config.sample_rate();
    let channels = config.channels() as usize;

    // resample to the device rate once, then stream from memory
    let data: Vec<f32> = if rate != out_rate {
        let mut resampler = Resampler::new(rate, out_rate);
        let mut out = vec![0.0f32; resampler.max_output(samples.len()) + 64];
        let count = resampler.process(&samples, &mut out);
        out.truncate(count);
        out
    } else {
        samples
    };

    let data = Arc::new(data);
    let cursor = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicBool::new(false));

    fn err(error: cpal::Error) {
        eprintln!("playback error: {error}");
    }

    let stream = {
        let data = data.clone();
        let cursor = cursor.clone();
        let finished = finished.clone();

        match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config.config(),
                move |buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for frame in buffer.chunks_mut(channels) {
                        if CANCEL.load(Ordering::Relaxed) {
                            for slot in frame.iter_mut() {
                                *slot = 0.0;
                            }
                            finished.store(true, Ordering::Relaxed);
                            continue;
                        }
                        let index = cursor.fetch_add(1, Ordering::Relaxed);
                        let value = data.get(index).copied().unwrap_or(0.0);
                        for slot in frame.iter_mut() {
                            *slot = value;
                        }
                        if index >= data.len() {
                            finished.store(true, Ordering::Relaxed);
                        }
                    }
                },
                err,
                None,
            )?,
            cpal::SampleFormat::I16 => {
                let data = data.clone();
                let cursor = cursor.clone();
                let finished = finished.clone();
                device.build_output_stream(
                    config.config(),
                    move |buffer: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        for frame in buffer.chunks_mut(channels) {
                            if CANCEL.load(Ordering::Relaxed) {
                                for slot in frame.iter_mut() {
                                    *slot = 0;
                                }
                                finished.store(true, Ordering::Relaxed);
                                continue;
                            }
                            let index = cursor.fetch_add(1, Ordering::Relaxed);
                            let value = (data.get(index).copied().unwrap_or(0.0) * 32767.0) as i16;
                            for slot in frame.iter_mut() {
                                *slot = value;
                            }
                            if index >= data.len() {
                                finished.store(true, Ordering::Relaxed);
                            }
                        }
                    },
                    err,
                    None,
                )?
            }
            other => return Err(anyhow!("unsupported output format: {other:?}")),
        }
    };

    stream.play()?;

    // wait for the file plus a short tail, but never hang forever
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !finished.load(Ordering::Relaxed)
        && !CANCEL.load(Ordering::Relaxed)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    let tail = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < tail {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Ok(())
}

/// Speech RMS to `target_db`: silent frames (below -50 dBFS) do not count, so
/// pauses do not drag the gain up.
fn normalize(samples: &mut [f32], rate: u32, target_db: f32) {
    let window = (rate as usize / 100).max(1); // 10 ms
    let mut sum = 0.0f64;
    let mut frames = 0usize;
    for chunk in samples.chunks(window) {
        let rms = (chunk.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>()
            / chunk.len().max(1) as f64)
            .sqrt();
        if 20.0 * (rms + 1e-9).log10() > -50.0 {
            sum += rms * rms;
            frames += 1;
        }
    }
    if frames == 0 {
        return;
    }
    let rms = (sum / frames as f64).sqrt();
    let rms_db = 20.0 * (rms as f32 + 1e-9).log10();
    let gain = 10f32.powf((target_db - rms_db) / 20.0).clamp(0.05, 20.0);
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// Speech RMS of a wav in dBFS: 10 ms windows below -50 dB are ignored, so
/// pauses do not drag the value down.
pub fn speech_rms_db(path: &std::path::Path) -> Result<f32> {
    let (samples, rate) = wav::read_mono(path)?;
    let window = (rate as usize / 100).max(1);
    let mut sum = 0.0f64;
    let mut frames = 0usize;
    for chunk in samples.chunks(window) {
        let rms = (chunk.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>()
            / chunk.len().max(1) as f64)
            .sqrt();
        if 20.0 * (rms + 1e-9).log10() > -50.0 {
            sum += rms * rms;
            frames += 1;
        }
    }
    if frames == 0 {
        return Ok(-120.0);
    }
    let rms = (sum / frames as f64).sqrt();
    Ok(20.0 * (rms as f32 + 1e-9).log10())
}

/// Limits the regression run treats as a broken take. They live next to the
/// metric so the number the A/B card shows and the verdict `nc metrics --check`
/// prints are the same code: this used to be a second implementation in
/// `scripts/ab_metrics.py`, and the two disagreed (26.7 vs 27.4 dB of pause
/// contrast on a real take, 123 vs 63 dB on a silent one).
pub const MAX_DROPOUT_S: f32 = 0.4;
pub const MIN_PAUSE_CONTRAST_DB: f32 = 12.0;
pub const MAX_LEVEL_DELTA_DB: f32 = 20.0;
pub const MIN_SPEECH_DB: f32 = -55.0;
pub const MAX_SPEECH_DB: f32 = -10.0;

/// Numbers for a finished A/B pair, so a take can be judged without listening.
/// All levels are medians over 20 ms frames; contrast does not depend on the
/// output volume, so raising the gain cannot make a take look cleaner.
#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct AbMetrics {
    /// speech level of the processed take, dBFS
    pub speech_level_db: f32,
    /// processed speech minus raw speech: how much louder the take got
    pub level_delta_db: f32,
    /// how far pauses sit below speech in the processed take
    pub pause_contrast_db: f32,
    pub longest_dropout_s: f32,
}

impl AbMetrics {
    /// Why this take looks broken, in the words `nc metrics --check` prints.
    /// Empty means it passed.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.longest_dropout_s > MAX_DROPOUT_S {
            problems.push(format!(
                "dropout {:.2}s > {MAX_DROPOUT_S}s",
                self.longest_dropout_s
            ));
        }
        if self.pause_contrast_db < MIN_PAUSE_CONTRAST_DB {
            problems.push(format!(
                "pauses only {:.1} dB below speech < {MIN_PAUSE_CONTRAST_DB} dB",
                self.pause_contrast_db
            ));
        }
        if self.level_delta_db.abs() > MAX_LEVEL_DELTA_DB {
            problems.push(format!(
                "level moved {:.1} dB from the raw take",
                self.level_delta_db
            ));
        }
        if !(MIN_SPEECH_DB..=MAX_SPEECH_DB).contains(&self.speech_level_db) {
            problems.push(format!(
                "speech level {:.1} dBFS out of range",
                self.speech_level_db
            ));
        }
        problems
    }
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return -120.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

fn envelope(samples: &[f32], window: usize) -> Vec<f32> {
    samples
        .chunks(window)
        .map(|chunk| {
            let rms = (chunk.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>()
                / chunk.len().max(1) as f64)
                .sqrt();
            20.0 * (rms as f32 + 1e-9).log10()
        })
        .collect()
}

fn percentile(values: &[f32], percent: f32) -> f32 {
    if values.is_empty() {
        return -120.0;
    }
    let mut copy = values.to_vec();
    copy.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((copy.len() - 1) as f32 * percent / 100.0).round() as usize;
    copy[index.min(copy.len() - 1)]
}

/// Compares a raw take with the processed one: how loud speech ended up, how
/// far pauses dropped, and the longest stretch where the model muted a word.
///
/// Speech is found **relative to the take's own noise floor**, not with an
/// absolute -50 dB threshold: in a room with a television the background sits at
/// -45 dB, an absolute threshold calls it speech, and a take that correctly
/// silenced the noise looked like one long dropout.
pub fn ab_metrics(raw: &std::path::Path, clean: &std::path::Path, lang: Lang) -> Result<AbMetrics> {
    let (raw_samples, rate) = wav::read_mono(raw)?;
    let (clean_samples, _) = wav::read_mono(clean)?;
    let n = raw_samples.len().min(clean_samples.len());
    if n == 0 {
        return Err(anyhow!(lang.text("запись пуста", "the take is empty")));
    }
    let window = (rate as usize / 50).max(1); // 20 ms
    let raw_env = envelope(&raw_samples[..n], window);
    let clean_env = envelope(&clean_samples[..n], window);
    let frames = raw_env.len().min(clean_env.len());
    let frame_s = window as f32 / rate as f32;

    let floor = percentile(&raw_env[..frames], 10.0);
    let speech_level = (floor + 18.0).max(-55.0);
    let pause_level = floor + 5.0;

    let mut speech_raw = Vec::new();
    let mut speech_clean = Vec::new();
    for i in 0..frames {
        if raw_env[i] > speech_level {
            speech_raw.push(raw_env[i]);
            speech_clean.push(clean_env[i]);
        }
    }
    let clean_speech_level = median(&mut speech_clean);
    // a word is "muted" when the take drops 30 dB below its own speech level
    let mute_line = clean_speech_level - 30.0;

    let mut pause_clean = Vec::new();
    let mut longest = 0.0f32;
    let mut run = 0.0f32;
    for i in 0..frames {
        let raw_db = raw_env[i];
        let clean_db = clean_env[i];
        if raw_db > speech_level {
            if clean_db < mute_line {
                run += frame_s;
            } else {
                longest = longest.max(run);
                run = 0.0;
            }
        } else {
            longest = longest.max(run);
            run = 0.0;
            if raw_db < pause_level {
                pause_clean.push(clean_db);
            }
        }
    }
    longest = longest.max(run);

    let raw_speech = median(&mut speech_raw);
    let clean_pause = median(&mut pause_clean);

    Ok(AbMetrics {
        speech_level_db: clean_speech_level,
        level_delta_db: clean_speech_level - raw_speech,
        pause_contrast_db: clean_speech_level - clean_pause,
        longest_dropout_s: longest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// speech 0.1 in the raw take, silence between words, and one word muted
    /// in the processed take: the numbers must show both suppression and the gap
    #[test]
    fn ab_metrics_reports_suppression_and_dropouts() {
        let rate = 16_000u32;
        let mut raw = Vec::new();
        let mut clean = Vec::new();
        for block in 0..10 {
            // 0.2 s of speech, 0.2 s of room noise
            for i in 0..rate as usize / 5 {
                let t = i as f32 / rate as f32;
                let voice = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 0.1;
                raw.push(voice);
                // the fourth second is muted by the model
                let muted = block == 5;
                clean.push(if muted { 0.0 } else { voice * 0.5 });
            }
            for _ in 0..rate as usize / 5 {
                raw.push(0.002);
                clean.push(0.00002);
            }
        }

        let dir = std::env::temp_dir().join(format!("nc-ab-metrics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let raw_path = dir.join("raw.wav");
        let clean_path = dir.join("clean.wav");
        wav::write_mono(&raw_path, &raw, rate).expect("write raw");
        wav::write_mono(&clean_path, &clean, rate).expect("write clean");

        let m = ab_metrics(&raw_path, &clean_path, crate::lang::Lang::Ru).expect("metrics");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            m.pause_contrast_db > 20.0,
            "pauses only {:.1} dB below speech",
            m.pause_contrast_db
        );
        assert!(
            (-10.0..0.0).contains(&m.level_delta_db),
            "level delta {:.1} dB",
            m.level_delta_db
        );
        assert!(
            (m.longest_dropout_s - 0.2).abs() < 0.06,
            "dropout measured as {:.2} s",
            m.longest_dropout_s
        );
        assert!(
            (-30.0..-15.0).contains(&m.speech_level_db),
            "speech level {:.1} dB",
            m.speech_level_db
        );
    }

    #[test]
    fn a_healthy_take_has_no_problems() {
        let m = AbMetrics {
            speech_level_db: -30.0,
            level_delta_db: 2.0,
            pause_contrast_db: 30.0,
            longest_dropout_s: 0.1,
        };
        assert!(m.problems().is_empty(), "{:?}", m.problems());
    }

    #[test]
    fn every_limit_reports_its_own_problem() {
        let base = AbMetrics {
            speech_level_db: -30.0,
            level_delta_db: 2.0,
            pause_contrast_db: 30.0,
            longest_dropout_s: 0.1,
        };

        let dropout = AbMetrics {
            longest_dropout_s: 0.5,
            ..base
        };
        assert_eq!(dropout.problems().len(), 1);
        assert!(dropout.problems()[0].starts_with("dropout"));

        let quiet_pauses = AbMetrics {
            pause_contrast_db: 8.0,
            ..base
        };
        assert!(quiet_pauses.problems()[0].contains("pauses only"));

        let loud = AbMetrics {
            level_delta_db: -21.0,
            ..base
        };
        assert!(loud.problems()[0].contains("level moved"));

        let silent = AbMetrics {
            speech_level_db: -60.0,
            ..base
        };
        assert!(silent.problems()[0].contains("out of range"));
    }
}
