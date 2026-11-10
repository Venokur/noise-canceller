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
///
/// The file is written next to the target and renamed when complete: quitting
/// in the middle of a take used to leave a truncated wav that the history then
/// offered to play.
pub fn write_mono(path: &Path, samples: &[f32], rate: u32) -> Result<()> {
    let tmp = path.with_extension("wav.tmp");
    let written = (|| -> Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&tmp, spec)?;
        for sample in samples {
            writer.write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)?;
        }
        writer.finalize()?;
        Ok(())
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_pair_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("nc-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("ab-1-raw.wav");

        let samples = [0.0, 0.5, -0.5, 0.25];
        write_mono(&path, &samples, 16_000).expect("write");

        let (read, rate) = read_mono(&path).expect("read");
        assert_eq!(rate, 16_000);
        assert_eq!(read.len(), samples.len());
        assert!((read[1] - 0.5).abs() < 1e-3);

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("list")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "a .tmp file survived the rename");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
