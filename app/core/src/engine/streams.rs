//! The two audio callbacks and the hand-off to the processor thread:
//! capture -> input ring, output ring -> device.

use std::sync::atomic::Ordering;

use crate::resample::Resampler;

use super::ab::{ab_append_raw, AbBuffer};
use super::Shared;

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
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..channels {
            acc += data[f * channels + c];
        }
        let value = acc / channels as f32;
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
    shared.callback_frames.store(frames as u32, Ordering::Relaxed);
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

    // real buffer fill seen by the consumer, plus VoxLock's own 24 ms of latency
    let fill_ms = ring.slots() as f32 / out_rate.max(1) as f32 * 1000.0;
    shared.latency.store((fill_ms + 24.0).to_bits(), Ordering::Relaxed);
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

