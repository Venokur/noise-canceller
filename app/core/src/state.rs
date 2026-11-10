//! Learned state that survives between sessions, kept in `data/state.json`.
//!
//! These numbers are not settings: the user never edits them, the app measures
//! them on this machine — the leveler gain that suits the microphone and the
//! output ring this CPU and cable driver actually sustain. They live in their
//! own file for two reasons:
//!
//! - exporting, importing or resetting `settings.json` must not carry another
//!   machine's numbers, and a hand-edited config must not freeze a stale gain;
//! - `settings.json` is written by the UI from a full snapshot of `Settings`
//!   (debounced 300 ms), while these values change inside the running engine.
//!   Sharing one file would need a merge layer, and a frontend save would
//!   overwrite the gain the engine just learned.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::settings::{BUFFER_MS_MAX, BUFFER_MS_MIN};

pub const STATE_FILE: &str = "state.json";

/// The live engine clamps the remembered gain to this, so the file can carry the
/// same ceiling without importing the engine.
pub const LEVELER_GAIN_MAX_DB: f32 = 18.0;

/// Files written by versions before `state.json` existed.
const LEGACY_FILES: [&str; 2] = ["leveler.txt", "auto-buffer.txt"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LearnedState {
    /// leveler gain in dB; 0 = nothing measured yet
    pub leveler_gain_db: f32,
    /// output ring learned by `buffer_auto`, ms; 0 = nothing learned yet
    pub auto_buffer_ms: f32,
}

impl LearnedState {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(STATE_FILE)
    }

    /// Reads the learned state. A missing or broken file falls back to the old
    /// `.txt` files, then to "nothing learned": a start must never fail because
    /// of this file.
    pub fn load(dir: &Path) -> Self {
        match std::fs::read_to_string(Self::path(dir)) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(state) => state.sanitized(),
                Err(err) => {
                    crate::log::info(&format!(
                        "state.json is unreadable ({err}), looking for the old state files"
                    ));
                    Self::migrate(dir).sanitized()
                }
            },
            Err(_) => Self::migrate(dir).sanitized(),
        }
    }

    /// Moves `leveler.txt` / `auto-buffer.txt` into `state.json`. The old files
    /// are removed only after the new one is really on disk, so a failed write
    /// cannot lose the learned values.
    fn migrate(dir: &Path) -> Self {
        let gain = read_number(&dir.join(LEGACY_FILES[0]));
        let buffer = read_number(&dir.join(LEGACY_FILES[1]));
        let state = Self {
            leveler_gain_db: gain.unwrap_or(0.0),
            auto_buffer_ms: buffer.unwrap_or(0.0),
        }
        .sanitized();

        if gain.is_none() && buffer.is_none() {
            return state;
        }
        if state.save(dir) {
            for name in LEGACY_FILES {
                let _ = std::fs::remove_file(dir.join(name));
            }
            crate::log::info(&format!(
                "learned state migrated to {}: leveler {:.1} dB, auto buffer {:.0} ms",
                STATE_FILE, state.leveler_gain_db, state.auto_buffer_ms
            ));
        }
        state
    }

    /// Keeps a hand-edited file (or an old one) inside the ranges the engine can
    /// use. Zero keeps its meaning: "not learned", the engine then falls back to
    /// the manual setting.
    pub fn sanitized(mut self) -> Self {
        if self.leveler_gain_db.is_finite() {
            self.leveler_gain_db = self.leveler_gain_db.clamp(0.0, LEVELER_GAIN_MAX_DB);
        } else {
            self.leveler_gain_db = 0.0;
        }
        if self.auto_buffer_ms.is_finite() && self.auto_buffer_ms > 0.0 {
            self.auto_buffer_ms = self.auto_buffer_ms.clamp(BUFFER_MS_MIN, BUFFER_MS_MAX);
        } else {
            self.auto_buffer_ms = 0.0;
        }
        self
    }

    /// Write-then-rename, like `Settings::save`: a crash in the middle must not
    /// leave a truncated file that reads as "nothing learned".
    pub fn save(&self, dir: &Path) -> bool {
        let path = Self::path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(text) = serde_json::to_string_pretty(self) else {
            return false;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_err() {
            return false;
        }
        if let Err(err) = std::fs::rename(&tmp, &path) {
            crate::log::info(&format!("cannot write {}: {err}", path.display()));
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        true
    }
}

fn read_number(path: &Path) -> Option<f32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nc-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_keeps_both_values() {
        let dir = temp_dir("roundtrip");
        let state = LearnedState {
            leveler_gain_db: 7.5,
            auto_buffer_ms: 170.0,
        };
        assert!(state.save(&dir));
        assert_eq!(LearnedState::load(&dir), state);
        let text = std::fs::read_to_string(LearnedState::path(&dir)).unwrap();
        assert!(text.contains("levelerGainDb"));
        assert!(text.contains("autoBufferMs"));
    }

    #[test]
    fn legacy_files_are_migrated_and_removed() {
        let dir = temp_dir("migrate");
        std::fs::write(dir.join("leveler.txt"), "4.5").unwrap();
        std::fs::write(dir.join("auto-buffer.txt"), "170").unwrap();

        let state = LearnedState::load(&dir);
        assert_eq!(
            state,
            LearnedState {
                leveler_gain_db: 4.5,
                auto_buffer_ms: 170.0
            }
        );
        assert!(LearnedState::path(&dir).is_file());
        assert!(!dir.join("leveler.txt").exists());
        assert!(!dir.join("auto-buffer.txt").exists());
    }

    #[test]
    fn broken_file_falls_back_to_defaults() {
        let dir = temp_dir("broken");
        std::fs::write(LearnedState::path(&dir), "{not json").unwrap();
        assert_eq!(LearnedState::load(&dir), LearnedState::default());
    }

    #[test]
    fn missing_files_mean_nothing_learned() {
        let dir = temp_dir("empty");
        let state = LearnedState::load(&dir);
        assert_eq!(state.auto_buffer_ms, 0.0);
        assert_eq!(state.leveler_gain_db, 0.0);
    }

    #[test]
    fn values_out_of_range_are_clamped() {
        let dir = temp_dir("clamp");
        std::fs::write(
            LearnedState::path(&dir),
            r#"{"levelerGainDb":-5,"autoBufferMs":1000}"#,
        )
        .unwrap();
        let state = LearnedState::load(&dir);
        assert_eq!(state.leveler_gain_db, 0.0);
        assert_eq!(state.auto_buffer_ms, BUFFER_MS_MAX);

        std::fs::write(
            LearnedState::path(&dir),
            r#"{"levelerGainDb":99,"autoBufferMs":5}"#,
        )
        .unwrap();
        let state = LearnedState::load(&dir);
        assert_eq!(state.leveler_gain_db, LEVELER_GAIN_MAX_DB);
        assert_eq!(state.auto_buffer_ms, BUFFER_MS_MIN);
    }
}
