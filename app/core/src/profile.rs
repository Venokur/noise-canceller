//! 15-second voice enrolment: record, resample to 16 kHz, score the take.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::devices;
use crate::engine::BANDS;
use crate::lang::Lang;
use crate::paths;
use crate::resample::Resampler;
use crate::spectrum::Spectrum;

/// Spectrum of the take currently being recorded: the check-microphone view
/// polls it, so the bars move while the user is speaking instead of staying
/// idle until the take ends.
static TAKE_BANDS: Mutex<[f32; BANDS]> = Mutex::new([0.0; BANDS]);

pub fn take_bands() -> [f32; BANDS] {
    TAKE_BANDS.lock().map(|b| *b).unwrap_or([0.0; BANDS])
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quality {
    pub duration: f32,
    pub speech_percent: f32,
    pub snr_db: f32,
    pub peak_db: f32,
    pub clip_percent: f32,
    pub score: i32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub path: String,
    pub quality: Quality,
}

/// Sidecar written next to every profile: which microphone it was recorded on
/// and when. Without it a profile recorded on a headset was silently applied to
/// a desk microphone, where VoxLock stops recognising the same voice.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMeta {
    /// user-facing name; empty means "use the file name/date"
    #[serde(default)]
    pub name: String,
    pub device: String,
    pub created: u64,
    pub score: i32,
    pub snr_db: f32,
    /// how consistent the speech is across the enrolment (0..1); negative =
    /// not measured yet
    #[serde(default = "unknown_consistency")]
    pub consistency: f32,
    pub app_version: String,
}

fn unknown_consistency() -> f32 {
    -1.0
}

/// Persists a measured consistency without touching the wav. Used by the
/// speaker-verification pass after recording.
pub fn set_consistency(path: &std::path::Path, value: f32) -> Result<ProfileMeta> {
    let mut meta = read_meta(path).ok_or_else(|| anyhow!("profile sidecar is missing"))?;
    meta.consistency = value.clamp(0.0, 1.0);
    write_meta(path, &meta);
    Ok(meta)
}

/// Renames a profile by rewriting its sidecar. An old profile without a
/// sidecar gets one, so the name survives even though the wav did not change.
pub fn rename(path: &std::path::Path, name: &str) -> Result<ProfileMeta> {
    let mut meta = read_meta(path).unwrap_or_else(|| ProfileMeta {
        name: String::new(),
        device: String::new(),
        created: 0,
        score: 0,
        snr_db: 0.0,
        consistency: -1.0,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    });
    meta.name = name.trim().chars().take(48).collect();
    write_meta(path, &meta);
    Ok(meta)
}

fn meta_path(path: &std::path::Path) -> std::path::PathBuf {
    path.with_extension("json")
}

pub fn read_meta(path: &std::path::Path) -> Option<ProfileMeta> {
    let text = std::fs::read_to_string(meta_path(path)).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn write_meta(path: &std::path::Path, meta: &ProfileMeta) {
    if let Ok(text) = serde_json::to_string_pretty(meta) {
        let _ = std::fs::write(meta_path(path), text);
    }
}

/// Records a short take and grades it without saving a profile. Used by the
/// "check microphone" button: the user speaks once and sees whether the level
/// and the signal-to-noise ratio are sane.
pub fn probe(input_name: Option<&str>, seconds: f32, lang: Lang) -> Result<Quality> {
    // no normalisation: the check must report the microphone's real level, not
    // a take pulled up to -20 dBFS
    let profile = record_inner(input_name, seconds, lang, false)?;
    let path = std::path::Path::new(&profile.path);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(meta_path(path));
    Ok(profile.quality)
}

/// Grades an existing profile wav (16 kHz mono i16). Used by the UI to show
/// the quality of every remembered profile and to pick the best one.
pub fn score_file(path: &std::path::Path) -> Result<Quality> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let mut samples: Vec<f32> = Vec::new();
    for sample in reader.samples::<i16>() {
        samples.push(sample? as f32 / 32768.0);
    }
    if samples.is_empty() {
        // an internal invariant, not a UI message: callers only score files the
        // recorder wrote, and the UI treats a failure as "no score"
        return Err(anyhow!("profile is empty"));
    }
    Ok(evaluate(&samples, spec.sample_rate))
}

pub fn record(input_name: Option<&str>, seconds: f32, lang: Lang) -> Result<Profile> {
    record_inner(input_name, seconds, lang, true)
}

fn record_inner(
    input_name: Option<&str>,
    seconds: f32,
    lang: Lang,
    normalize_level: bool,
) -> Result<Profile> {
    let host = cpal::default_host();
    let device = devices::pick_input(&host, input_name, lang)?;
    let config = device.default_input_config()?;
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    let format = config.sample_format();

    let target_in = (rate as f32 * seconds) as usize;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();

    fn err(err: cpal::Error) {
        eprintln!("profile stream error: {err}");
    }

    let stream = match format {
        cpal::SampleFormat::F32 => {
            let tx = tx.clone();
            device.build_input_stream(
                config.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.to_vec());
                },
                err,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let tx = tx.clone();
            device.build_input_stream(
                config.config(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.iter().map(|v| *v as f32 / 32768.0).collect());
                },
                err,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported input sample format: {other:?}")),
    };

    stream.play()?;

    let mut mono: Vec<f32> = Vec::with_capacity(target_in + 8192);
    let mut spectrum = Spectrum::new(1024, rate as f32);
    if let Ok(mut slot) = TAKE_BANDS.lock() {
        *slot = [0.0; BANDS];
    }
    while mono.len() < target_in {
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(chunk) => {
                let frames = chunk.len() / channels.max(1);
                let start = mono.len();
                for f in 0..frames {
                    let mut acc = 0.0f32;
                    for c in 0..channels {
                        acc += chunk[f * channels + c];
                    }
                    mono.push(acc / channels as f32);
                }
                spectrum.push(&mono[start..]);
                if let Ok(mut slot) = TAKE_BANDS.lock() {
                    *slot = spectrum.bands();
                }
            }
            Err(_) => break,
        }
    }

    drop(stream);

    if mono.len() < (rate as f32 * 3.0) as usize {
        return Err(anyhow!("recording too short"));
    }

    mono.truncate(target_in);

    // resample to the model rate and keep 16-bit wav, as VoxLock expects
    let mut resampler = Resampler::new(rate, 16000);
    let mut at16 = vec![0.0f32; resampler.max_output(mono.len()) + 64];
    let count = resampler.process(&mono, &mut at16);
    let mut at16 = at16[..count].to_vec();

    // grade the take as it was captured: a quiet microphone must be reported as
    // quiet, even though the stored enrolment is later pulled up to -20 dBFS
    let quality = evaluate(&at16, 16000);

    // Normalise the enrolment to the same level the model sees live. Without
    // this a quiet take (the fifine records around -60 dBFS) does not match the
    // live voice, and VoxLock starts dropping the speaker it was enrolled for.
    if normalize_level {
        let frame = 320usize; // 20 ms at 16 kHz
        let mut levels: Vec<f32> = at16
            .chunks_exact(frame)
            .map(|c| {
                let s: f64 = c.iter().map(|v| (*v as f64) * (*v as f64)).sum();
                20.0 * ((s / c.len() as f64).sqrt() as f32 + 1e-9).log10()
            })
            .collect();
        levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let loud = levels.get(levels.len() * 9 / 10).copied().unwrap_or(-60.0);
        let gain = 10f32.powf((-20.0 - loud) / 20.0).clamp(0.25, 16.0);
        for sample in at16.iter_mut() {
            *sample = (*sample * gain).clamp(-0.99, 0.99);
        }
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = paths::profiles_dir().join(format!("voice-{stamp}.wav"));

    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec)?;
    for sample in at16 {
        writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    writer.finalize()?;

    write_meta(
        &path,
        &ProfileMeta {
            name: String::new(),
            device: input_name.unwrap_or("").to_string(),
            created: stamp,
            score: quality.score,
            snr_db: quality.snr_db,
            consistency: -1.0,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        },
    );

    Ok(Profile {
        path: path.to_string_lossy().to_string(),
        quality,
    })
}

/// Frame based analysis: speech share, SNR estimate, peak and clipping.
pub fn evaluate(samples: &[f32], rate: u32) -> Quality {
    let frame = (rate as usize / 50).max(64); // 20 ms
    let mut levels: Vec<f32> = Vec::new();

    for chunk in samples.chunks_exact(frame) {
        let sum: f64 = chunk.iter().map(|v| (*v as f64) * (*v as f64)).sum();
        let rms = (sum / chunk.len() as f64).sqrt() as f32;
        levels.push(20.0 * (rms + 1e-9).log10());
    }

    if levels.is_empty() {
        return Quality {
            duration: 0.0,
            speech_percent: 0.0,
            snr_db: 0.0,
            peak_db: -120.0,
            clip_percent: 0.0,
            score: 0,
        };
    }

    let mut sorted = levels.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p10 = sorted[(sorted.len() as f32 * 0.10) as usize];
    let p90 = sorted[((sorted.len() as f32 * 0.90) as usize).min(sorted.len() - 1)];

    let speech_frames = levels.iter().filter(|l| **l > p10 + 8.0).count();
    let speech_percent = speech_frames as f32 / levels.len() as f32;

    let peak = samples.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    let peak_db = 20.0 * (peak + 1e-9).log10();
    let clipped = samples.iter().filter(|v| v.abs() > 0.985).count();
    let clip_percent = clipped as f32 / samples.len() as f32 * 100.0;
    let snr = (p90 - p10).max(0.0);

    let mut score = 100.0f32;
    if speech_percent < 0.35 {
        score -= 30.0;
    } else if speech_percent < 0.5 {
        score -= 12.0;
    }
    if snr < 15.0 {
        score -= 25.0;
    } else if snr < 22.0 {
        score -= 10.0;
    }
    if clip_percent > 0.5 {
        score -= 20.0;
    }
    if peak_db > -2.0 {
        score -= 10.0;
    }
    if samples.len() < 16000 * 8 {
        score -= 15.0;
    }

    Quality {
        duration: samples.len() as f32 / rate as f32,
        speech_percent,
        snr_db: snr,
        peak_db,
        clip_percent,
        score: score.clamp(0.0, 100.0) as i32,
    }
}

pub fn list() -> Vec<String> {
    let dir = paths::profiles_dir();
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "wav").unwrap_or(false))
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.reverse();
    out
}

pub fn delete(path: &str) -> Result<()> {
    let p = PathBuf::from(path);
    if !p.starts_with(paths::profiles_dir()) {
        return Err(anyhow!("refusing to delete outside the profiles folder"));
    }
    std::fs::remove_file(&p)?;
    // the sidecar is not a profile on its own, but leaving it behind would
    // orphan a device/date record after every deletion
    let _ = std::fs::remove_file(meta_path(&p));
    Ok(())
}
