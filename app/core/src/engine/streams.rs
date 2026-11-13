//! The two audio callbacks and the hand-off to the processor thread:
//! capture -> input ring, output ring -> device.

use std::sync::atomic::Ordering;

use cpal::traits::DeviceTrait;
use cpal::{SampleFormat, StreamConfig};

use crate::resample::Resampler;

use super::ab::{ab_append_raw, AbBuffer};
use super::{Shared, MODEL_RATE};

/// Pushes a rendered block into the output ring, counting what actually landed.
///
/// The silence path used to count the whole block even when the ring was full,
/// so `pushed`/`pulled` disagreed exactly when the machine was struggling.
pub(super) fn push_block(out: &mut rtrb::Producer<f32>, samples: &[f32], shared: &Shared) {
    let mut written = 0usize;
    for sample in samples {
        if out.push(*sample).is_err() {
            break;
        }
        written += 1;
    }
    if written < samples.len() {
        // the ring was full: these samples never reached the device
        shared.overflows.fetch_add(1, Ordering::Relaxed);
    }
    shared.pushed.fetch_add(written as u64, Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn capture_f32(
    data: &[f32],
    channels: usize,
    mono: &mut Vec<f32>,
    down: &mut Resampler,
    down_buf: &mut Vec<f32>,
    ring: &mut rtrb::Producer<f32>,
    shared: &Shared,
    ab: &std::sync::Arc<std::sync::Mutex<Option<AbBuffer>>>,
) {
    let frames = data.len() / channels.max(1);
    if frames == 0 {
        return;
    }
    if mono.len() < frames {
        mono.resize(frames, 0.0);
    }

    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    // A stereo interface often has the voice on one channel and something else
    // on the other; averaging two channels with a phase difference comb-filters
    // the voice. -1 keeps the old mono mix, 0.. picks a channel.
    let channel = shared.input_channel.load(Ordering::Relaxed);
    for f in 0..frames {
        let value = if channel >= 0 && (channel as usize) < channels {
            data[f * channels + channel as usize]
        } else {
            let mut acc = 0.0f32;
            for c in 0..channels {
                acc += data[f * channels + c];
            }
            acc / channels as f32
        };
        mono[f] = value;
        sum_sq += (value as f64) * (value as f64);
        peak = peak.max(value.abs());
    }

    ab_append_raw(ab, &mono[..frames]);
    shared.in_frames.fetch_add(frames as u64, Ordering::Relaxed);

    let rms = (sum_sq / frames as f64).sqrt() as f32;
    shared.input_rms.store(rms.to_bits(), Ordering::Relaxed);
    // decaying peak: a 5 ms clip must survive long enough for the 150 ms UI poll
    let held = (f32::from_bits(shared.input_peak.load(Ordering::Relaxed)) * 0.995).max(peak);
    shared.input_peak.store(held.to_bits(), Ordering::Relaxed);

    let needed = down.max_output(frames);
    if down_buf.len() < needed {
        down_buf.resize(needed, 0.0);
    }
    let produced = down.process(&mono[..frames], down_buf);
    let mut dropped = 0usize;
    for (i, sample) in down_buf[..produced].iter().enumerate() {
        if ring.push(*sample).is_err() {
            // the processor is behind: the rest of this callback will not fit
            dropped = produced - i;
            break;
        }
    }
    if dropped > 0 {
        shared
            .capture_drops
            .fetch_add(dropped as u32, Ordering::Relaxed);
    }
}

pub(super) fn render_f32(
    data: &mut [f32],
    channels: usize,
    ring: &mut rtrb::Consumer<f32>,
    shared: &Shared,
    out_rate: u32,
    smoothed: &mut f32,
) {
    let mut sum_sq = 0.0f64;
    let frames = data.len() / channels.max(1);
    let mut last = 0.0f32;

    // smooth the fill over ~0.5 s: the producer pushes a whole block at once, so
    // the raw value is a sawtooth and a controller fed with it hunts
    let fill = ring.slots() as f32;
    shared.out_raw.store(fill as u32, Ordering::Relaxed);
    shared
        .callback_frames
        .store(frames as u32, Ordering::Relaxed);
    if *smoothed <= 0.0 {
        *smoothed = fill;
    }
    *smoothed += (fill - *smoothed) * 0.02;
    // stored as f32 bits: the controller reads it with `from_bits`, and storing
    // a plain number here made it read back as ~0, so the ring always looked
    // starved
    shared.out_fill.store(smoothed.to_bits(), Ordering::Relaxed);
    shared.pulled.fetch_add(frames as u64, Ordering::Relaxed);

    let muted = shared.muted.load(Ordering::Relaxed);
    let mut missed = 0u32;
    let mut peak = 0.0f32;

    for f in 0..frames {
        // a starved ring repeats the last sample instead of inserting silence: a
        // hole in the middle of a word is what "words disappear" sounds like.
        // The repeat decays, so a longer gap fades out instead of holding DC and
        // clicking when the audio comes back.
        let value = match ring.pop() {
            Ok(sample) => sample,
            Err(_) => {
                missed += 1;
                shared.underruns.fetch_add(1, Ordering::Relaxed);
                last *= 0.985;
                last
            }
        };
        last = value;
        sum_sq += (value as f64) * (value as f64);
        // muted: keep draining so the timing does not drift, but send silence
        let value = if muted { 0.0 } else { value };
        peak = peak.max(value.abs());
        for c in 0..channels {
            data[f * channels + c] = value;
        }
    }

    // the auto buffer reacts to callbacks that missed data, not to samples
    if missed > 0 {
        shared.underrun_events.fetch_add(1, Ordering::Relaxed);
    }

    let rms = if frames > 0 {
        (sum_sq / frames as f64).sqrt() as f32
    } else {
        0.0
    };
    shared.output_rms.store(rms.to_bits(), Ordering::Relaxed);
    let held = (f32::from_bits(shared.output_peak.load(Ordering::Relaxed)) * 0.995).max(peak);
    shared.output_peak.store(held.to_bits(), Ordering::Relaxed);

    // real buffer fill seen by the consumer, plus VoxLock's own 24 ms of latency
    let fill_ms = ring.slots() as f32 / out_rate.max(1) as f32 * 1000.0;
    shared
        .latency
        .store((fill_ms + 24.0).to_bits(), Ordering::Relaxed);
}

static PANIC_HOOK: std::sync::Once = std::sync::Once::new();

/// A panic in the processing thread used to leave silence with no trace; log it
/// so `data/app.log` explains what happened.
pub(super) fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::log::info(&format!("panic: {info}"));
            default_hook(info);
        }));
    });
}

/// Opens the capture stream: downmix, resample to the model rate and push into
/// the input ring. Format conversion buffers are allocated here, never in the
/// callback.
pub(super) fn open_input(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    shared: Shared,
    ring: rtrb::Producer<f32>,
) -> anyhow::Result<cpal::Stream> {
    let in_rate = supported.sample_rate();
    let mut down = Resampler::new(in_rate, MODEL_RATE);
    let mut mono = vec![0.0f32; 8192];
    let mut down_buf = vec![0.0f32; 8192];
    let mut in_prod = ring;

    let in_config: StreamConfig = supported.config();
    let channels = in_config.channels as usize;
    let in_format = supported.sample_format();
    let in_ab = shared.ab.clone();

    fn err_fn(err: cpal::Error) {
        eprintln!("input stream error: {err}");
        crate::log::info(&format!("input stream error: {err}"));
    }

    let stream = match in_format {
        SampleFormat::F32 => device.build_input_stream(
            in_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                capture_f32(
                    data,
                    channels,
                    &mut mono,
                    &mut down,
                    &mut down_buf,
                    &mut in_prod,
                    &shared,
                    &in_ab,
                );
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            in_config,
            {
                // one buffer for the whole stream: converting in the audio
                // callback must not allocate
                let mut converted: Vec<f32> = Vec::new();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    converted.clear();
                    converted.extend(data.iter().map(|v| *v as f32 / 32768.0));
                    capture_f32(
                        &converted,
                        channels,
                        &mut mono,
                        &mut down,
                        &mut down_buf,
                        &mut in_prod,
                        &shared,
                        &in_ab,
                    );
                }
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("unsupported input sample format: {other:?}"),
    };
    Ok(stream)
}

/// Opens the render stream and writes the output ring into the device.
pub(super) fn open_output(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    shared: Shared,
    ring: rtrb::Consumer<f32>,
) -> anyhow::Result<cpal::Stream> {
    let out_rate = supported.sample_rate();
    let out_config: StreamConfig = supported.config();
    let out_channels = out_config.channels as usize;
    let out_format = supported.sample_format();
    let mut out_cons = ring;
    let mut smoothed_fill = 0.0f32;

    fn err_fn(err: cpal::Error) {
        eprintln!("output stream error: {err}");
        crate::log::info(&format!("output stream error: {err}"));
    }

    let stream = match out_format {
        SampleFormat::F32 => device.build_output_stream(
            out_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                render_f32(
                    data,
                    out_channels,
                    &mut out_cons,
                    &shared,
                    out_rate,
                    &mut smoothed_fill,
                );
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_output_stream(
            out_config,
            {
                // one conversion buffer for the stream: no allocation in the
                // audio callback
                let mut tmp: Vec<f32> = Vec::new();
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    tmp.resize(data.len(), 0.0);
                    render_f32(
                        &mut tmp,
                        out_channels,
                        &mut out_cons,
                        &shared,
                        out_rate,
                        &mut smoothed_fill,
                    );
                    for (dst, src) in data.iter_mut().zip(tmp.iter()) {
                        *dst = (*src * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    }
                }
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("unsupported output sample format: {other:?}"),
    };
    Ok(stream)
}
