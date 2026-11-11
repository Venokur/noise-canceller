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
pub const DEFAULTS_REV: u32 = 12;

/// Range of the output ring buffer, ms. The floor is also the auto buffer's
/// floor: below 40 ms a single 200 ms cable callback drains the ring.
pub const BUFFER_MS_MIN: f32 = 40.0;
pub const BUFFER_MS_MAX: f32 = 250.0;

/// Tone shelves. Body may go down to −6 dB and no further up than +3: below
/// 100 Hz the chain has just removed the rumble a boost would bring back.
pub const TONE_LOW_MIN_DB: f32 = -6.0;
pub const TONE_LOW_MAX_DB: f32 = 3.0;
/// Presence is symmetric, but never more than ±6 dB: a larger lift makes
/// sibilance and residual hiss stand out and moves the "pauses are quieter
/// than speech" figure in the A/B check.
pub const TONE_HIGH_MAX_DB: f32 = 6.0;

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

    /// second denoiser after VoxLock: "off" | "gtcrn"
    pub residual_denoise: String,
    /// "after" (VoxLock then denoise) or "before" (denoise then VoxLock)
    pub residual_order: String,
    /// Silero VAD drives the output gate instead of a level threshold
    pub vad_gate: bool,
    /// keep only the enrolled speaker; off = plain noise suppression
    pub voice_isolation: bool,

    pub high_pass_enabled: bool,
    pub high_pass_hz: f32,

    /// Two fixed shelves after the AGC: body at 200 Hz, presence at 3 kHz.
    /// 0 dB is off. There is deliberately no band above 8 kHz — the model runs
    /// at 16 kHz, so that part of the output is empty.
    pub tone_enabled: bool,
    pub tone_low_db: f32,
    pub tone_high_db: f32,

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
    /// pick the buffer automatically from the observed buffer misses
    pub buffer_auto: bool,
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
            theme: Theme::Dark,
            mode: "profile".into(),
            profile_path: None,
            isolation_strength: 1.0,
            max_suppression_db: 15.0,

            input_device: String::new(),
            output_device: String::new(),
            input_leveling: true,

            residual_denoise: "dpdfnet".into(),
            residual_order: "after".into(),
            vad_gate: true,
            voice_isolation: true,

            high_pass_enabled: true,
            high_pass_hz: 100.0,
            tone_enabled: true,
            tone_low_db: 0.0,
            tone_high_db: 0.0,
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
            buffer_auto: true,
            chunk_ms: 60.0,
            auto_threads: true,
            denoiser_threads: default_threads(),

            defaults_rev: DEFAULTS_REV,
        }
    }
}

impl Settings {
    /// Threads actually handed to the model for a given (already rounded) block.
    /// The engine rounds the block to VoxLock's 128-sample hop first, so the
    /// automatic pick has to see the rounded value, not the slider.
    pub fn effective_threads(&self, chunk_ms: f32) -> u16 {
        if self.auto_threads {
            recommended_threads(chunk_ms)
        } else {
            self.denoiser_threads.clamp(1, 16)
        }
    }

    /// Keeps a hand-edited or imported settings file from driving the engine
    /// into invalid ranges: 0 ms blocks, 0 recordings kept, unknown modes.
    pub fn sanitize(&mut self) {
        self.isolation_strength = self.isolation_strength.clamp(0.0, 1.0);
        self.max_suppression_db = self.max_suppression_db.clamp(0.0, 48.0);
        self.buffer_ms = self.buffer_ms.clamp(BUFFER_MS_MIN, BUFFER_MS_MAX);
        self.chunk_ms = self.chunk_ms.clamp(20.0, 200.0);
        self.denoiser_threads = self.denoiser_threads.clamp(1, 16);
        self.agc_target_db = self.agc_target_db.clamp(-40.0, -6.0);
        self.agc_max_gain_db = self.agc_max_gain_db.clamp(0.0, 30.0);
        self.gate_threshold_db = self.gate_threshold_db.clamp(-80.0, -20.0);
        self.gate_ratio = self.gate_ratio.clamp(1.0, 10.0);
        self.gate_attack_ms = self.gate_attack_ms.clamp(1.0, 100.0);
        self.gate_release_ms = self.gate_release_ms.clamp(20.0, 1000.0);
        self.gate_hold_ms = self.gate_hold_ms.clamp(0.0, 1000.0);
        self.output_gain_db = self.output_gain_db.clamp(-24.0, 6.0);
        self.high_pass_hz = self.high_pass_hz.clamp(20.0, 300.0);
        self.tone_low_db = self.tone_low_db.clamp(TONE_LOW_MIN_DB, TONE_LOW_MAX_DB);
        self.tone_high_db = self.tone_high_db.clamp(-TONE_HIGH_MAX_DB, TONE_HIGH_MAX_DB);
        self.comfort_noise_db = self.comfort_noise_db.clamp(-90.0, -30.0);
        self.ab_seconds = self.ab_seconds.clamp(5.0, 60.0);
        self.keep_recordings = self.keep_recordings.clamp(1, 100);
        if !matches!(self.mode.as_str(), "profile" | "auto") {
            self.mode = "profile".into();
        }
        if !matches!(
            self.residual_denoise.as_str(),
            "off" | "gtcrn" | "dpdfnet" | "ulunas"
        ) {
            self.residual_denoise = "gtcrn".into();
        }
        if !matches!(self.residual_order.as_str(), "after" | "before") {
            self.residual_order = "after".into();
        }
        if !matches!(self.hotkey_mode.as_str(), "toggle" | "hold") {
            self.hotkey_mode = "toggle".into();
        }
        if self.mute_hotkey.trim().is_empty() {
            self.mute_hotkey = "Ctrl+Alt+M".into();
        }
    }

    pub fn load(path: &std::path::Path) -> Self {
        let mut settings: Self = std::fs::read_to_string(path)
            .ok()
            // Notepad and PowerShell write a BOM; serde rejects it and the whole
            // file silently loads as defaults, which looks like "my settings
            // were reset" and also drops the learned-gain migration
            .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
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

            // rev 11: the cap is now applied only to blocks that look like the
            // speaker, so it no longer brings the background back; 15 dB keeps
            // a word audible while the room stays clearly below the voice, and
            // the old 24 dB let a phone playing music sit only 24 dB down
            if settings.defaults_rev < 11 && (settings.max_suppression_db - 24.0).abs() < 0.1 {
                settings.max_suppression_db = 15.0;
            }

            // rev 12: the shipped denoiser is DPDFNet now. Only the old default
            // moves: "off" and a hand-picked UL-UNAS are left alone.
            if settings.defaults_rev < 12 && settings.residual_denoise == "gtcrn" {
                settings.residual_denoise = "dpdfnet".into();
            }

            settings.defaults_rev = DEFAULTS_REV;
        }

        // the cable's capture side is not a microphone: a saved selection of it
        // turns the app into a feedback loop, so drop it once and pick a real one
        if !settings.input_device.is_empty()
            && crate::devices::is_cable_name(&settings.input_device)
        {
            settings.input_device.clear();
        }

        settings.sanitize();
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
        assert_eq!(settings.effective_threads(60.0), 7);

        settings.auto_threads = true;
        assert_eq!(settings.effective_threads(60.0), recommended_threads(60.0));
    }

    #[test]
    fn a_settings_file_with_a_bom_is_not_silently_dropped() {
        // Notepad and Windows PowerShell write a BOM; serde used to reject the
        // file and every value fell back to defaults
        let dir = std::env::temp_dir().join(format!("nc-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "\u{feff}{\"theme\":\"light\",\"chunkMs\":80.0}").unwrap();

        let settings = Settings::load(&path);
        assert_eq!(settings.theme, Theme::Light);
        assert!(
            (settings.chunk_ms - 80.0).abs() < 0.1,
            "{}",
            settings.chunk_ms
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_old_default_denoiser_becomes_dpdfnet() {
        assert_eq!(Settings::default().residual_denoise, "dpdfnet");

        let dir = std::env::temp_dir().join(format!("nc-denoise-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        // the old shipped default moves over...
        std::fs::write(&path, r#"{"defaultsRev":11,"residualDenoise":"gtcrn"}"#).unwrap();
        assert_eq!(Settings::load(&path).residual_denoise, "dpdfnet");

        // ...a deliberate choice does not
        std::fs::write(&path, r#"{"defaultsRev":11,"residualDenoise":"ulunas"}"#).unwrap();
        assert_eq!(Settings::load(&path).residual_denoise, "ulunas");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_clamps_a_hand_edited_file() {
        let mut settings = Settings {
            buffer_ms: 0.0,
            chunk_ms: 0.0,
            denoiser_threads: 0,
            agc_max_gain_db: 99.0,
            keep_recordings: 0,
            ab_seconds: 1.0,
            max_suppression_db: 99.0,
            mode: "nonsense".into(),
            residual_denoise: "nonsense".into(),
            residual_order: "nonsense".into(),
            hotkey_mode: "nonsense".into(),
            mute_hotkey: "  ".into(),
            tone_low_db: 99.0,
            tone_high_db: -99.0,
            ..Settings::default()
        };
        settings.sanitize();
        assert_eq!(settings.buffer_ms, 40.0);
        assert_eq!(settings.chunk_ms, 20.0);
        assert_eq!(settings.denoiser_threads, 1);
        assert_eq!(settings.agc_max_gain_db, 30.0);
        assert_eq!(settings.keep_recordings, 1);
        assert_eq!(settings.ab_seconds, 5.0);
        assert_eq!(settings.max_suppression_db, 48.0);
        assert_eq!(settings.mode, "profile");
        assert_eq!(settings.residual_denoise, "gtcrn");
        assert_eq!(settings.residual_order, "after");
        assert_eq!(settings.hotkey_mode, "toggle");
        assert_eq!(settings.mute_hotkey, "Ctrl+Alt+M");
        assert_eq!(settings.tone_low_db, TONE_LOW_MAX_DB);
        assert_eq!(settings.tone_high_db, -TONE_HIGH_MAX_DB);
    }

    #[test]
    fn an_old_settings_file_gets_a_neutral_tone_control() {
        // a file written before the shelves existed must keep the sound it had:
        // 0 dB makes the chain skip both filters entirely
        let dir = std::env::temp_dir().join(format!("nc-tone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"defaultsRev":12,"residualDenoise":"gtcrn"}"#).unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.tone_low_db, 0.0);
        assert_eq!(settings.tone_high_db, 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
