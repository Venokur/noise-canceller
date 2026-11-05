use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Version of the shipped defaults. Bumped when a default changes so that a
/// stored settings file that still has the old, untouched values is migrated.
pub const DEFAULTS_REV: u32 = 10;

fn default_threads() -> u16 {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(2, 4) as u16)
        .unwrap_or(2)
}

/// Logical processors the machine reports (SMT threads).
pub fn logical_cores() -> u16 {
    std::thread::available_parallelism()
        .map(|n| n.get().min(64) as u16)
        .unwrap_or(4)
}

/// Threads for the model, derived from the machine and the block size.
///
/// Measurements on this codebase (64 ms blocks): RTF 0.83 on one thread, 0.69 on
/// two, 0.48 on four, 0.38 on eight. Most of the gain is in the first four, so
/// the recommendation stays there and only small blocks — where every
/// millisecond of processing time hurts — get more.
pub fn recommended_threads(chunk_ms: f32) -> u16 {
    threads_for(logical_cores(), chunk_ms)
}

fn threads_for(logical: u16, chunk_ms: f32) -> u16 {
    let logical = logical.max(1);
    let mut threads = (logical / 4).clamp(2, 6);
    if chunk_ms < 60.0 {
        threads = (threads + 2).min(8);
    }
    threads.min(logical)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub theme: Theme,

    /// "profile" or "auto"
    pub mode: String,
    pub profile_path: Option<String>,
    pub isolation_strength: f32,
    /// hard floor under the voice: the processed output is never more than this
    /// many dB below the model input while the input is speech. 0 = no limit.
    pub max_suppression_db: f32,

    /// remembered device names, so a restart keeps the user's choice
    pub input_device: String,
    pub output_device: String,

    /// keeps the model input at a usable level on quiet microphones
    pub input_leveling: bool,

    /// second denoiser after VoxLock: "off" | "gtcrn" | "dpdfnet"
    pub residual_denoise: String,
    pub residual_attenuation_db: f32,
    /// "after" (VoxLock then denoise) or "before" (denoise then VoxLock)
    pub residual_order: String,
    /// Silero VAD drives the output gate instead of a level threshold
    pub vad_gate: bool,
    /// keep only the enrolled speaker; off = plain noise suppression
    pub voice_isolation: bool,

    pub high_pass_enabled: bool,
    pub high_pass_hz: f32,

    pub gate_enabled: bool,
    pub gate_threshold_db: f32,
    pub gate_ratio: f32,
    pub gate_attack_ms: f32,
    pub gate_release_ms: f32,
    pub gate_hold_ms: f32,

    pub agc_enabled: bool,
    pub agc_target_db: f32,
    pub agc_max_gain_db: f32,
    pub limiter_enabled: bool,

    pub comfort_noise_enabled: bool,
    pub comfort_noise_db: f32,
    pub output_gain_db: f32,

    pub close_to_tray: bool,
    /// start hidden in the tray instead of showing the window
    pub start_minimized: bool,
    /// the first-run wizard was shown and dismissed
    pub onboarded: bool,
    /// start processing automatically when the cable is available
    pub auto_start: bool,
    /// follow the Windows default input device instead of a fixed name
    pub follow_default_input: bool,
    /// pick the best-graded profile automatically
    pub auto_profile: bool,
    /// global mute shortcut, e.g. "Ctrl+Alt+M"
    pub mute_hotkey: String,
    /// "toggle" (press to switch) or "hold" (push-to-talk)
    pub hotkey_mode: String,
    /// hide the DSP sliders behind "Дополнительно"
    pub basic_ui: bool,
    /// how many A/B pairs to keep in data/recordings
    pub keep_recordings: u32,
    /// A/B take length in seconds
    pub ab_seconds: f32,
    pub buffer_ms: f32,
    pub chunk_ms: f32,
    /// pick the thread count from the machine instead of `denoiser_threads`
    pub auto_threads: bool,
    pub denoiser_threads: u16,

    /// field-level default is 0 even though the struct has `#[serde(default)]`,
    /// so an older settings file is recognised as not migrated yet
    #[serde(default)]
    pub defaults_rev: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            theme: Theme::System,
            mode: "profile".into(),
            profile_path: None,
            isolation_strength: 1.0,
            max_suppression_db: 24.0,

            input_device: String::new(),
            output_device: String::new(),
            input_leveling: true,

            residual_denoise: "gtcrn".into(),
            residual_attenuation_db: 18.0,
            residual_order: "after".into(),
            vad_gate: true,
            voice_isolation: true,

            high_pass_enabled: true,
            high_pass_hz: 100.0,
            gate_enabled: true,
            gate_threshold_db: -50.0,
            gate_ratio: 3.0,
            gate_attack_ms: 10.0,
            gate_release_ms: 300.0,
            gate_hold_ms: 150.0,

            agc_enabled: true,
            agc_target_db: -24.0,
            agc_max_gain_db: 18.0,
            limiter_enabled: true,

            comfort_noise_enabled: false,
            comfort_noise_db: -66.0,
            output_gain_db: -3.0,

            close_to_tray: true,
            start_minimized: false,
            onboarded: false,
            auto_start: true,
            follow_default_input: true,
            auto_profile: true,
            mute_hotkey: "Ctrl+Alt+M".into(),
            hotkey_mode: "toggle".into(),
            basic_ui: true,
            keep_recordings: 20,
            ab_seconds: 30.0,
            buffer_ms: 50.0,
            chunk_ms: 60.0,
            auto_threads: true,
            denoiser_threads: default_threads(),

            defaults_rev: DEFAULTS_REV,
        }
    }
}

impl Settings {
    /// Threads actually handed to the model: the automatic pick when enabled,
    /// otherwise the slider value.
    pub fn effective_threads(&self) -> u16 {
        if self.auto_threads {
            recommended_threads(self.chunk_ms)
        } else {
            self.denoiser_threads.clamp(1, 16)
        }
    }

    pub fn load(path: &std::path::Path) -> Self {
        let mut settings: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();

        // a settings file written before the tuning pass still holds the old
        // defaults; bump them, but leave values the user changed alone
        if settings.defaults_rev < DEFAULTS_REV {
            if settings.chunk_ms <= 40.5 && settings.denoiser_threads <= 2 {
                settings.chunk_ms = 60.0;
                settings.denoiser_threads = default_threads();
            }

            // the old "Дом" preset cut quiet syllables; the new one opens the
            // gate later and closes it slowly
            if settings.gate_threshold_db == -55.0
                && settings.gate_ratio == 2.0
                && settings.gate_attack_ms == 8.0
                && settings.gate_release_ms == 120.0
            {
                settings.gate_threshold_db = -60.0;
                settings.gate_attack_ms = 10.0;
                settings.gate_release_ms = 300.0;
            }

            // rev 5: the chain boosted too much, which made the output loud and
            // the gain steps audible as quiet clicks
            // rev 7: 0 dB left the voice quiet whenever the model attenuated it
            if settings.defaults_rev < 7 && settings.agc_max_gain_db == 0.0 {
                settings.agc_max_gain_db = 12.0;
            }

            // rev 8: the gate moved behind the AGC, so its threshold is now
            // relative to the corrected speech level
            if settings.defaults_rev < 8 {
                if settings.gate_threshold_db == -60.0 {
                    settings.gate_threshold_db = -45.0;
                }
                if settings.agc_max_gain_db == 12.0 {
                    settings.agc_max_gain_db = 18.0;
                }
            }

            if settings.defaults_rev < 5 {
                if settings.agc_target_db == -20.0 {
                    settings.agc_target_db = -24.0;
                }
                if settings.agc_max_gain_db == 18.0 {
                    settings.agc_max_gain_db = 12.0;
                }
                if settings.output_gain_db == 0.0 {
                    settings.output_gain_db = -6.0;
                }
            }

            // rev 6: the standard preset must not amplify at all — the leveler
            // already brings a quiet microphone up
            if settings.defaults_rev < 6 {
                if settings.agc_max_gain_db == 12.0 {
                    settings.agc_max_gain_db = 0.0;
                }
                if settings.output_gain_db == -6.0 {
                    settings.output_gain_db = -3.0;
                }
            }

            // rev 9: the gate moved back behind the filter and is now tuned
            // against the corrected speech level
            if settings.defaults_rev < 9 {
                if settings.gate_threshold_db == -45.0 {
                    settings.gate_threshold_db = -50.0;
                }
                if settings.high_pass_hz == 85.0 {
                    settings.high_pass_hz = 100.0;
                }
            }

            // rev 10: the expander needs a bit more slope to actually close
            if settings.defaults_rev < 10 && settings.gate_ratio == 2.0 {
                settings.gate_ratio = 3.0;
            }

            settings.defaults_rev = DEFAULTS_REV;
        }

        // only known stages are allowed; anything else (including the removed
        // DPDFNet option) falls back to GTCRN
        match settings.residual_denoise.as_str() {
            "off" | "gtcrn" => {}
            _ => settings.residual_denoise = "gtcrn".into(),
        }

        // the cable's capture side is not a microphone: a saved selection of it
        // turns the app into a feedback loop, so drop it once and pick a real one
        if !settings.input_device.is_empty() && crate::engine::is_cable_name(&settings.input_device) {
            settings.input_device.clear();
        }

        settings
    }

    pub fn save(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            // write-then-rename: a crash in the middle must not leave a
            // truncated settings file that loads as defaults
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_pick_follows_the_machine() {
        // the measured sweet spot: about a quarter of the logical processors,
        // never below two, never above six
        assert_eq!(threads_for(16, 60.0), 4);
        assert_eq!(threads_for(12, 60.0), 3);
        assert_eq!(threads_for(8, 60.0), 2);
        assert_eq!(threads_for(4, 60.0), 2);
        assert_eq!(threads_for(32, 60.0), 6);
        // a single-core machine gets one thread, not two
        assert_eq!(threads_for(1, 60.0), 1);
    }

    #[test]
    fn small_blocks_get_more_threads() {
        // 40 ms costs almost twice as much per second of audio
        assert_eq!(threads_for(16, 40.0), 6);
        assert_eq!(threads_for(8, 40.0), 4);
        assert_eq!(threads_for(4, 40.0), 4);
        // never more threads than the machine has
        assert_eq!(threads_for(2, 40.0), 2);
        assert_eq!(threads_for(1, 40.0), 1);
    }

    #[test]
    fn manual_mode_wins_over_the_pick() {
        let mut settings = Settings {
            auto_threads: false,
            denoiser_threads: 7,
            chunk_ms: 60.0,
            ..Settings::default()
        };
        assert_eq!(settings.effective_threads(), 7);

        settings.auto_threads = true;
        assert_eq!(settings.effective_threads(), recommended_threads(60.0));
    }
}
