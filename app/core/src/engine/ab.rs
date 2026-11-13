//! The A/B take: the in-memory buffer and the two wav files it becomes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::{recordings, wav};

pub struct AbBuffer {
    pub(super) raw: Vec<f32>,
    pub(super) clean: Vec<f32>,
    pub(super) raw_rate: u32,
    pub(super) out_rate: u32,
    pub(super) target_out: usize,
    pub(super) stamp: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbStatus {
    pub running: bool,
    pub progress: f32,
    pub folder: Option<String>,
    pub raw: Option<String>,
    pub clean: Option<String>,
    /// Why the last take produced no files (disk full, read-only folder). The
    /// window used to see `running: false` and silently drop the take.
    pub error: Option<String>,
}

pub(super) fn ab_append_raw(ab: &Arc<Mutex<Option<AbBuffer>>>, samples: &[f32]) {
    // `try_lock`: this runs inside the capture callback, and blocking there is
    // exactly how a take gets a hole. The processor holds the lock only for the
    // length of one `extend_from_slice`, so a skipped callback is rare.
    if let Ok(mut slot) = ab.try_lock() {
        if let Some(buf) = slot.as_mut() {
            let limit = buf.raw_rate as usize * 90;
            if buf.raw.len() < limit {
                buf.raw.extend_from_slice(samples);
            }
        }
    }
}

pub(super) fn ab_append_clean(
    ab: &Arc<Mutex<Option<AbBuffer>>>,
    done: &Arc<AtomicBool>,
    active: &Arc<AtomicBool>,
    folder: &Arc<Mutex<Option<String>>>,
    paths: &Arc<Mutex<(Option<String>, Option<String>)>>,
    error: &Arc<Mutex<Option<String>>>,
    samples: &[f32],
) {
    let mut guard = match ab.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let Some(buf) = guard.as_mut() else {
        return;
    };

    buf.clean.extend_from_slice(samples);
    if buf.clean.len() < buf.target_out {
        return;
    }

    let finished = match guard.take() {
        Some(buf) => buf,
        None => return,
    };
    drop(guard);

    // writing two wav files takes tens of milliseconds; doing it here would
    // stall the processing thread at the exact moment the take ends
    let done = done.clone();
    let active = active.clone();
    let folder = folder.clone();
    let paths = paths.clone();
    let error = error.clone();
    std::thread::spawn(move || {
        match write_ab(&finished) {
            Ok((dir, raw, clean)) => {
                if let Ok(mut slot) = folder.lock() {
                    *slot = Some(dir);
                }
                if let Ok(mut slot) = paths.lock() {
                    *slot = (Some(raw), Some(clean));
                }
                if let Ok(mut slot) = error.lock() {
                    *slot = None;
                }
            }
            Err(err) => {
                crate::log::info(&format!("a/b write failed: {err}"));
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(err.to_string());
                }
            }
        }

        done.store(true, Ordering::Relaxed);
        active.store(false, Ordering::Relaxed);
    });
}

fn write_ab(buf: &AbBuffer) -> anyhow::Result<(String, String, String)> {
    let dir = recordings::dir();
    let (raw, clean) = recordings::pair(buf.stamp);
    wav::write_mono(&raw, &buf.raw, buf.raw_rate)?;
    wav::write_mono(&clean, &buf.clean, buf.out_rate)?;
    Ok((
        dir.to_string_lossy().to_string(),
        raw.to_string_lossy().to_string(),
        clean.to_string_lossy().to_string(),
    ))
}
