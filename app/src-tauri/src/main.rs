#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Noise Canceller: the Tauri shell around the audio core.
//!
//! This file only wires the modules and starts the app. The window commands
//! live in `commands`, the tray/window/hotkey side in `tray`, the builder in
//! `setup`, the shared state in `state` and the wire shapes in `dto`.

mod commands;
mod dto;
mod setup;
mod state;
mod tray;

fn main() {
    setup::run();
}

/// The DTO shapes live in two places: these structs and `src/types.ts`.
/// Nothing type-checks the Vue side (TypeScript 7 is not supported by
/// `vue-tsc` yet), so a field renamed here went missing there without a word —
/// `defaultsRev` had already fallen out, and the settings screen broke exactly
/// that way. This test is the cheap guard: it compares the serialized key sets
/// against the interfaces in `types.ts`, so CI fails on drift instead of the
/// user seeing an empty control.
#[cfg(test)]
mod contract_tests {
    use serde::Serialize;

    use crate::dto::*;
    use noise_canceller_core::settings::Settings;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    fn ts_interface(name: &str) -> BTreeSet<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/types.ts");
        let text = fs::read_to_string(&path).expect("src/types.ts should be readable");
        let header = format!("export interface {name} {{");
        let start = text
            .find(&header)
            .unwrap_or_else(|| panic!("types.ts has no interface {name}"));
        let body = &text[start + header.len()..];
        let end = body.find("\n}").expect("unterminated interface");

        body[..end]
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty()
                    || line.starts_with('/')
                    || line.starts_with('*')
                    || line.starts_with("//")
                {
                    return None;
                }
                let name = line.split(':').next()?.trim().trim_end_matches('?');
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn json_keys<T: Serialize>(value: &T) -> BTreeSet<String> {
        match serde_json::to_value(value).expect("DTO should serialize to JSON") {
            serde_json::Value::Object(map) => map.keys().cloned().collect(),
            other => panic!("DTO serialized to {}, expected an object", other),
        }
    }

    fn assert_same(name: &str, rust: BTreeSet<String>, ts: BTreeSet<String>) {
        let only_rust: Vec<_> = rust.difference(&ts).cloned().collect();
        let only_ts: Vec<_> = ts.difference(&rust).cloned().collect();
        assert!(
            only_rust.is_empty() && only_ts.is_empty(),
            "{name} drifted: only in Rust {only_rust:?}, only in types.ts {only_ts:?}"
        );
    }

    #[test]
    fn settings_matches_types_ts() {
        assert_same(
            "Settings",
            json_keys(&Settings::default()),
            ts_interface("Settings"),
        );
    }

    #[test]
    fn stats_matches_types_ts() {
        assert_same(
            "Stats",
            json_keys(&StatsDto::default()),
            ts_interface("Stats"),
        );
    }

    #[test]
    fn ab_status_matches_types_ts() {
        let status = noise_canceller_core::engine::AbStatus {
            running: false,
            progress: 0.0,
            folder: None,
            raw: None,
            clean: None,
        };
        assert_same("AbStatus", json_keys(&status), ts_interface("AbStatus"));
    }

    #[test]
    fn speaker_similarity_matches_types_ts() {
        assert_same(
            "SpeakerSimilarity",
            json_keys(&SpeakerSimilarityDto {
                windows: 0,
                min: 0.0,
                median: 0.0,
                max: 0.0,
            }),
            ts_interface("SpeakerSimilarity"),
        );
    }

    #[test]
    fn recording_and_profile_dtos_match_types_ts() {
        let quality = noise_canceller_core::profile::Quality {
            duration: 0.0,
            speech_percent: 0.0,
            snr_db: 0.0,
            peak_db: 0.0,
            clip_percent: 0.0,
            score: 0,
        };
        assert_same("Quality", json_keys(&quality), ts_interface("Quality"));
        assert_same(
            "ProfileScore",
            json_keys(&ProfileScoreDto {
                path: String::new(),
                score: 0,
                snr_db: 0.0,
            }),
            ts_interface("ProfileScore"),
        );
        assert_same(
            "LastAb",
            json_keys(&LastAb {
                folder: String::new(),
                raw: String::new(),
                clean: String::new(),
            }),
            ts_interface("LastAb"),
        );
        assert_same(
            "AbEntry",
            json_keys(&AbEntry {
                stamp: 0,
                raw: String::new(),
                clean: String::new(),
            }),
            ts_interface("AbEntry"),
        );
        assert_same(
            "AbMetrics",
            json_keys(&AbMetricsDto {
                speech_level_db: 0.0,
                level_delta_db: 0.0,
                pause_contrast_db: 0.0,
                longest_dropout_s: 0.0,
            }),
            ts_interface("AbMetrics"),
        );
        assert_same(
            "SystemInfo",
            json_keys(&SystemInfo {
                logical_cores: 1,
                physical_cores: 1,
                recommended_threads: 1,
            }),
            ts_interface("SystemInfo"),
        );
        assert_same(
            "SettingsResult",
            json_keys(&SettingsResult {
                settings: Settings::default(),
                warning: None,
            }),
            ts_interface("SettingsResult"),
        );
    }
}
