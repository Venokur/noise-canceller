//! Mono wav read/write.
//!
//! One implementation: the A/B recorder writes the pair, the preview player and
//! the metrics read it back, and a second writer meant the files could differ
//! in bit depth or clipping.

use std::path::Path;

use anyhow::Result;

/// Reads any integer or float wav and folds it to one channel at its own rate.
pub fn read_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let mut out = Vec::new();

    match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            for sample in reader.samples::<i32>() {
                out.push(sample? as f32 * scale);
            }
        }
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                out.push(sample?);
            }
        }
    }

    if spec.channels > 1 {
        let channels = spec.channels as usize;
        out = out
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
    }

    Ok((out, spec.sample_rate))
}

/// Writes 16-bit mono: everything the app records is meant for a person to
/// listen to and for `read_mono` to measure, not for further processing.
pub fn write_mono(path: &Path, samples: &[f32], rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}
