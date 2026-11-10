//! Device discovery and selection.
//!
//! One copy: the engine, the profile recorder and the preview player all have to
//! agree on what "the chosen device" means and on which names are the virtual
//! cable. Three near-identical pickers used to disagree on the error text and on
//! whether a missing device falls back to the default.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// Shown in the UI, so the name has to survive a round trip through settings.
pub fn name_of(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|desc| desc.name().to_string())
        .filter(|name| !name.is_empty())
}

fn names(host: &cpal::Host, input: bool) -> Vec<String> {
    let list = if input {
        host.input_devices()
    } else {
        host.output_devices()
    };
    list.map(|it| it.filter_map(|d| name_of(&d)).collect())
        .unwrap_or_default()
}

pub fn input_devices() -> Vec<String> {
    names(&cpal::default_host(), true)
}

pub fn output_devices() -> Vec<String> {
    names(&cpal::default_host(), false)
}

/// Names of the Windows default input/output, for "follow the system device".
pub fn default_names() -> (Option<String>, Option<String>) {
    let host = cpal::default_host();
    let input = host
        .default_input_device()
        .and_then(|d| name_of(&d))
        // the cable's capture side is not a microphone: following it means
        // processing our own output (feedback) and every app playing into it
        .filter(|name| !is_cable_name(name));
    let output = host.default_output_device().and_then(|d| name_of(&d));
    (input, output)
}

/// True for the virtual cable in either direction.
pub fn is_cable_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("cable") || lower.contains("vb-audio") || lower.contains("vb-cable")
}

/// The cable's playback side, i.e. what we render into.
pub fn find_cable(host: &cpal::Host) -> Option<cpal::Device> {
    let list = host.output_devices().ok()?;
    list.into_iter().find(|device| {
        name_of(device)
            .map(|name| {
                name.contains("CABLE Input")
                    || name.contains("VB-Audio")
                    || name.contains("VB-CABLE")
            })
            .unwrap_or(false)
    })
}

pub fn cable_present() -> bool {
    find_cable(&cpal::default_host()).is_some()
}

/// The device the user chose, or the system default when nothing was chosen.
/// A chosen name that is gone is an error: silently falling back to the default
/// made the app process the wrong microphone after a device was unplugged.
pub fn pick(host: &cpal::Host, name: Option<&str>, input: bool) -> Result<cpal::Device> {
    if let Some(target) = name.filter(|n| !n.is_empty()) {
        let list = if input {
            host.input_devices()
        } else {
            host.output_devices()
        }?;
        for device in list {
            if name_of(&device).as_deref() == Some(target) {
                return Ok(device);
            }
        }
        return Err(anyhow!(
            "устройство «{target}» не найдено — обновите список устройств"
        ));
    }

    if input {
        host.default_input_device()
            .ok_or_else(|| anyhow!("микрофон не найден"))
    } else {
        host.default_output_device()
            .ok_or_else(|| anyhow!("устройство вывода не найдено"))
    }
}

pub fn pick_input(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    pick(host, name, true)
}

pub fn pick_output(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    pick(host, name, false)
}
