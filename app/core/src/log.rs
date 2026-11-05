//! Tiny append-only log for support: `data/app.log`, rotated at 1 MB.
//!
//! The app has no console, so without this every engine error, device loss or
//! dropout-guard reset disappears. Messages are UTC, one line each.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> PathBuf {
    crate::paths::data_dir().join("app.log")
}

/// Appends one line. Safe to call from any thread; never panics.
pub fn info(message: &str) {
    let _guard = WRITE_LOCK.lock();
    let path = path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 1_000_000 {
            let old = path.with_extension("log.old");
            // Windows refuses to rename onto an existing file, so the second
            // rotation used to fail silently and the log grew forever
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&path, &old);
        }
    }

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{} {message}", stamp());
    }
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { y + 1 } else { y }, month, day)
}
