//! The shapes the window receives from `invoke()`.
//!
//! Every struct here is mirrored in `src/types.ts`; `contract_tests` in
//! `main.rs` compares the two key sets so a rename cannot go unnoticed.

use noise_canceller_core::engine::Stats;
use noise_canceller_core::settings::Settings;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatsDto {
    pub(crate) input_rms: f32,
    pub(crate) output_rms: f32,
    pub(crate) rtf: f32,
    pub(crate) latency_ms: f32,
    pub(crate) buffer_ms: f32,
    pub(crate) underruns: u32,
    pub(crate) in_rate: u32,
    pub(crate) in_channels: u16,
    pub(crate) out_rate: u32,
    pub(crate) overflows: u32,
    pub(crate) capture_drops: u32,
    pub(crate) backend: String,
    pub(crate) running: bool,
    pub(crate) spectrum: Vec<f32>,
    pub(crate) suppression_db: f32,
    pub(crate) speech: bool,
    pub(crate) input_peak_db: f32,
    pub(crate) output_peak_db: f32,
    pub(crate) muted: bool,
    pub(crate) guard_resets: u32,
    pub(crate) leveler_gain_db: f32,
    pub(crate) slew_ppm: f32,
    pub(crate) threads: u16,
    pub(crate) healthy: bool,
}

impl Default for StatsDto {
    fn default() -> Self {
        Self {
            input_rms: 0.0,
            output_rms: 0.0,
            rtf: 0.0,
            latency_ms: 0.0,
            buffer_ms: 0.0,
            underruns: 0,
            in_rate: 0,
            in_channels: 0,
            out_rate: 0,
            overflows: 0,
            capture_drops: 0,
            backend: String::new(),
            running: false,
            spectrum: Vec::new(),
            suppression_db: 0.0,
            speech: false,
            // silence, not full scale: the clip indicator reads this value
            input_peak_db: -120.0,
            output_peak_db: -120.0,
            muted: false,
            guard_resets: 0,
            leveler_gain_db: 0.0,
            slew_ppm: 0.0,
            threads: 0,
            healthy: false,
        }
    }
}

impl From<Stats> for StatsDto {
    fn from(s: Stats) -> Self {
        Self {
            input_rms: s.input_rms,
            output_rms: s.output_rms,
            rtf: s.rtf,
            latency_ms: s.latency_ms,
            buffer_ms: s.buffer_ms,
            underruns: s.underruns,
            in_rate: s.in_rate,
            in_channels: s.in_channels,
            out_rate: s.out_rate,
            overflows: s.overflows,
            capture_drops: s.capture_drops,
            backend: s.backend,
            running: true,
            spectrum: s.spectrum.to_vec(),
            suppression_db: s.suppression_db,
            speech: s.speech,
            input_peak_db: s.input_peak_db,
            output_peak_db: s.output_peak_db,
            muted: s.muted,
            guard_resets: s.guard_resets,
            leveler_gain_db: s.leveler_gain_db,
            slew_ppm: s.slew_ppm,
            threads: s.threads,
            healthy: s.healthy,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartResult {
    /// false when the engine was already running: the caller passed new devices
    /// or a new test mode, so it has to stop it and ask again
    pub(crate) started: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsResult {
    pub(crate) settings: Settings,
    /// non-fatal: the file is saved, but the global mute shortcut could not be
    /// registered (another app owns it), so the UI has to say so
    pub(crate) warning: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LastAb {
    pub(crate) folder: String,
    pub(crate) raw: String,
    pub(crate) clean: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AbEntry {
    pub(crate) stamp: u64,
    pub(crate) raw: String,
    pub(crate) clean: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SystemInfo {
    pub(crate) logical_cores: u16,
    pub(crate) physical_cores: u16,
    pub(crate) recommended_threads: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileScoreDto {
    pub(crate) path: String,
    pub(crate) score: i32,
    pub(crate) snr_db: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AbMetricsDto {
    pub(crate) speech_level_db: f32,
    pub(crate) level_delta_db: f32,
    pub(crate) pause_contrast_db: f32,
    pub(crate) longest_dropout_s: f32,
}
