//! User presets: a named snapshot of the DSP settings.
//!
//! Devices, the voice profile, theme and language are deliberately not applied
//! when a preset is loaded; only the sound-shaping fields travel. The full
//! `Settings` is stored so a preset survives new fields being added to the app.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::paths;
use crate::settings::Settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub name: String,
    pub settings: Settings,
}

fn path() -> PathBuf {
    paths::data_dir().join("presets.json")
}

pub fn list() -> Vec<Preset> {
    let text = std::fs::read_to_string(path()).ok().unwrap_or_default();
    let mut presets: Vec<Preset> =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_default();
    presets.sort_by_key(|preset| preset.name.to_lowercase());
    presets
}

fn write(presets: &[Preset]) -> Result<()> {
    let text = serde_json::to_string_pretty(presets)?;
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn save(name: &str, settings: Settings) -> Result<Preset> {
    let name: String = name.trim().chars().take(40).collect();
    if name.is_empty() {
        return Err(anyhow!("preset name is empty"));
    }
    let mut presets = list();
    presets.retain(|preset| preset.name != name);
    let preset = Preset { name, settings };
    presets.push(preset.clone());
    write(&presets)?;
    Ok(preset)
}

pub fn delete(name: &str) -> Result<()> {
    let mut presets = list();
    let before = presets.len();
    presets.retain(|preset| preset.name != name);
    if presets.len() == before {
        return Err(anyhow!("preset not found"));
    }
    write(&presets)
}
