#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::Mutex;

use noise_canceller_core::engine::Engine;
use noise_canceller_core::paths;
use noise_canceller_core::settings::Settings;
use noise_canceller_core::{devices, recordings};
use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, ContextMenu, Menu, MenuItem};
use tauri::Manager;
use tauri::State;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct AppState {
    engine: Mutex<Option<Engine>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    maximized: std::sync::atomic::AtomicBool,
    restore_rect: Mutex<Option<(i32, i32, u32, u32)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsDto {
    input_rms: f32,
    output_rms: f32,
    rtf: f32,
    latency_ms: f32,
    buffer_ms: f32,
    underruns: u32,
    out_rate: u32,
    overflows: u32,
    capture_drops: u32,
    backend: String,
    running: bool,
    spectrum: Vec<f32>,
    suppression_db: f32,
    speech: bool,
    input_peak_db: f32,
    muted: bool,
    guard_resets: u32,
    leveler_gain_db: f32,
    slew_ppm: f32,
    threads: u16,
    healthy: bool,
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
            muted: false,
            guard_resets: 0,
            leveler_gain_db: 0.0,
            slew_ppm: 0.0,
            threads: 0,
            healthy: false,
        }
    }
}

impl From<noise_canceller_core::engine::Stats> for StatsDto {
    fn from(s: noise_canceller_core::engine::Stats) -> Self {
        Self {
            input_rms: s.input_rms,
            output_rms: s.output_rms,
            rtf: s.rtf,
            latency_ms: s.latency_ms,
            buffer_ms: s.buffer_ms,
            underruns: s.underruns,
            out_rate: s.out_rate,
            overflows: s.overflows,
            capture_drops: s.capture_drops,
            backend: s.backend,
            running: true,
            spectrum: s.spectrum.to_vec(),
            suppression_db: s.suppression_db,
            speech: s.speech,
            input_peak_db: s.input_peak_db,
            muted: s.muted,
            guard_resets: s.guard_resets,
            leveler_gain_db: s.leveler_gain_db,
            slew_ppm: s.slew_ppm,
            threads: s.threads,
            healthy: s.healthy,
        }
    }
}

#[tauri::command(async)]
fn list_devices() -> (Vec<String>, Vec<String>) {
    (devices::input_devices(), devices::output_devices())
}

#[tauri::command]
async fn start(
    app: tauri::AppHandle,
    input: Option<String>,
    output: Option<String>,
    voice: Option<String>,
    test_mode: bool,
) -> Result<(), String> {
    // a dedicated blocking thread: loading the model takes 1.7-2.1 s and must
    // not occupy the main thread (the window would freeze) or a runtime worker
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        // never hold the engine lock while the model loads: `stats` polls it
        // from the UI every 150 ms and would block for the whole start-up
        {
            let guard = state.engine.lock().map_err(|e| e.to_string())?;
            if guard.is_some() {
                return Ok(());
            }
        }

        let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
        let started = std::time::Instant::now();
        let engine = Engine::start(
            settings,
            input.filter(|s| !s.is_empty()),
            output.filter(|s| !s.is_empty()),
            voice.filter(|s| !s.is_empty()).map(PathBuf::from),
            test_mode,
        )
        .map_err(|e| e.to_string())?;
        noise_canceller_core::log::info(&format!(
            "engine start took {} ms",
            started.elapsed().as_millis()
        ));

        let mut guard = state.engine.lock().map_err(|e| e.to_string())?;
        if guard.is_none() {
            *guard = Some(engine);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
fn stop(state: State<'_, AppState>) {
    // take the engine out first: joining the processing thread takes up to
    // ~100 ms and `stats` would otherwise wait on the lock from the UI thread
    let engine = state.engine.lock().ok().and_then(|mut guard| guard.take());
    if let Some(mut engine) = engine {
        engine.stop();
    }
}

#[tauri::command]
fn stats(state: State<'_, AppState>) -> StatsDto {
    let Ok(guard) = state.engine.lock() else {
        return StatsDto::default();
    };
    guard
        .as_ref()
        .map(|engine| StatsDto::from(engine.stats()))
        .unwrap_or_default()
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().map(|s| s.clone()).unwrap_or_default()
}

#[tauri::command]
fn set_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> Result<(), String> {
    settings.sanitize();
    let old_hotkey = state
        .settings
        .lock()
        .map(|s| s.mute_hotkey.clone())
        .unwrap_or_default();
    let rearm = settings.mute_hotkey != old_hotkey;

    // the setting is saved either way: the old shortcut is gone, so a failed
    // registration has to reach the user instead of being swallowed
    match apply_settings(&app, &state, settings, rearm) {
        Some(message) if rearm => Err(message),
        _ => Ok(()),
    }
}

/// Saves the settings, hands them to the running engine and optionally re-arms
/// the mute shortcut. Returns the registration error, if any: the caller decides
/// whether it is an error (a shortcut the user just picked) or a note (a reset
/// or an import that already went through).
fn apply_settings(
    app: &tauri::AppHandle,
    state: &AppState,
    settings: Settings,
    rearm_hotkey: bool,
) -> Option<String> {
    settings.save(&state.settings_path);
    if let Ok(mut guard) = state.settings.lock() {
        *guard = settings.clone();
    }
    if let Ok(guard) = state.engine.lock() {
        if let Some(engine) = guard.as_ref() {
            engine.update_settings(&settings);
        }
    }
    if rearm_hotkey {
        register_hotkey(app, &settings.mute_hotkey)
    } else {
        None
    }
}

#[tauri::command]
async fn probe_mic(
    input: Option<String>,
    seconds: f32,
) -> Result<noise_canceller_core::profile::Quality, String> {
    tauri::async_runtime::spawn_blocking(move || {
        noise_canceller_core::profile::probe(input.as_deref(), seconds)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn probe_spectrum() -> Vec<f32> {
    noise_canceller_core::profile::take_bands().to_vec()
}

#[tauri::command]
async fn record_profile(
    input: Option<String>,
    seconds: f32,
) -> Result<noise_canceller_core::profile::Profile, String> {
    tauri::async_runtime::spawn_blocking(move || {
        noise_canceller_core::profile::record(input.as_deref(), seconds)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_profiles() -> Vec<String> {
    noise_canceller_core::profile::list()
}

#[tauri::command(async)]
fn delete_profile(path: String) -> Result<(), String> {
    noise_canceller_core::profile::delete(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn ab_start(state: State<'_, AppState>, seconds: f32) -> Result<(), String> {
    let guard = state.engine.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(engine) => engine.start_ab(seconds).map_err(|e| e.to_string()),
        None => Err("engine is not running".to_string()),
    }
}

#[tauri::command]
fn ab_status(state: State<'_, AppState>) -> Option<noise_canceller_core::engine::AbStatus> {
    state
        .engine
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|engine| engine.ab_status()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LastAb {
    folder: String,
    raw: String,
    clean: String,
}

/// Newest finished A/B pair on disk. The in-memory paths die with the app, so
/// without this the play buttons disappear after a restart even though the
/// files are still there.
#[tauri::command(async)]
fn last_ab() -> Option<LastAb> {
    let (_, raw, clean) = recordings::newest_pair()?;
    Some(LastAb {
        folder: recordings::dir().to_string_lossy().to_string(),
        raw: raw.to_string_lossy().to_string(),
        clean: clean.to_string_lossy().to_string(),
    })
}

#[tauri::command(async)]
fn cable_present() -> bool {
    devices::cable_present()
}

#[tauri::command]
fn stop_playback() {
    noise_canceller_core::playback::stop();
}

#[tauri::command]
async fn play_profile(
    path: String,
    output: Option<String>,
    normalize: Option<bool>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = output.as_deref().filter(|s| !s.is_empty());
        if normalize.unwrap_or(false) {
            noise_canceller_core::playback::play_normalized(std::path::Path::new(&path), target)
        } else {
            noise_canceller_core::playback::play(std::path::Path::new(&path), target)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Runs a PowerShell script with no console window. Both the cable installer and
/// the model check used to carry their own copy of this invocation, and the
/// `CREATE_NO_WINDOW` flag was easy to forget in a new one.
fn powershell(script: &str) -> std::io::Result<std::process::Output> {
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

#[tauri::command]
async fn install_cable() -> Result<String, String> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:TEMP 'NoiseCancellerCable'
New-Item -ItemType Directory -Force $dir | Out-Null
$zip = Join-Path $dir 'vb-cable.zip'
Invoke-WebRequest 'https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack46.zip' -OutFile $zip
$out = Join-Path $dir 'setup'
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
Expand-Archive $zip $out -Force
$exe = Get-ChildItem $out -Recurse -Filter 'VBCABLE_Setup_x64.exe' | Select-Object -First 1
if (-not $exe) { throw 'installer not found in the archive' }
$sig = Get-AuthenticodeSignature $exe.FullName
if ($sig.Status -ne 'Valid') { throw 'installer signature is not valid' }
Start-Process -FilePath $exe.FullName -Verb RunAs -WorkingDirectory $exe.DirectoryName
'started'
"#;

    let output = tauri::async_runtime::spawn_blocking(move || powershell(SCRIPT))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
async fn check_model_update() -> Result<String, String> {
    const SCRIPT: &str =
        "$m = Invoke-RestMethod 'https://huggingface.co/api/models/arbazsiddiqui/voxlock'; $m.sha";

    let output = tauri::async_runtime::spawn_blocking(move || powershell(SCRIPT))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("не удалось связаться с huggingface.co".to_string());
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        return Err("пустой ответ от huggingface.co".to_string());
    }

    let state_path = noise_canceller_core::paths::data_dir().join("model-state.json");
    let known = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("sha")
                .and_then(|s| s.as_str())
                .map(str::to_string)
        });

    let short = &sha[..sha.len().min(8)];
    let write = |value: &str| {
        let _ = std::fs::write(&state_path, format!("{{\"sha\":\"{value}\"}}"));
    };

    match known {
        None => {
            write(&sha);
            Ok(format!(
                "Модель VoxLock · ревизия {short} · отслеживание включено"
            ))
        }
        Some(previous) if previous == sha => Ok(format!("Модель актуальна · ревизия {short}")),
        Some(_) => {
            write(&sha);
            Ok(format!(
                "На huggingface.co есть новая ревизия ({short}) — она появится в следующем обновлении приложения"
            ))
        }
    }
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn autostart_enabled(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn window_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
fn window_toggle_maximize(window: tauri::Window, state: State<'_, AppState>) {
    use std::sync::atomic::Ordering;
    use tauri::{PhysicalPosition, PhysicalSize};

    let maximized = state.maximized.load(Ordering::Relaxed);

    if maximized {
        if let Ok(mut slot) = state.restore_rect.lock() {
            if let Some((x, y, w, h)) = slot.take() {
                let _ = window.set_position(PhysicalPosition::new(x, y));
                // inner size: Tauri's set_size resizes the client area, so the
                // saved value must be the inner size too, otherwise the window
                // creeps bigger on every toggle
                let _ = window.set_size(PhysicalSize::new(w, h));
            }
        }
        state.maximized.store(false, Ordering::Relaxed);
        return;
    }

    let position = window.outer_position().ok();
    let inner = window.inner_size().ok();
    if let (Some(p), Some(s)) = (position, inner) {
        if let Ok(mut slot) = state.restore_rect.lock() {
            *slot = Some((p.x, p.y, s.width, s.height));
        }
    }

    // a borderless window still carries the invisible resize frame: the client
    // area is inset by a few pixels, so the maximized window must be moved out
    // by that inset for its content to line up with the work area
    let offset = match (position, window.inner_position().ok()) {
        (Some(o), Some(i)) => (i.x - o.x, i.y - o.y),
        _ => (0, 0),
    };

    if let Ok(Some(monitor)) = window.current_monitor() {
        // work_area, not the full monitor: the taskbar can sit on any edge and
        // would otherwise cover our custom title bar
        let area = monitor.work_area();
        let _ = window.set_position(PhysicalPosition::new(
            area.position.x - offset.0,
            area.position.y - offset.1,
        ));
        let _ = window.set_size(PhysicalSize::new(area.size.width, area.size.height));
        state.maximized.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
fn window_close(window: tauri::Window) {
    let _ = window.close();
}

#[tauri::command]
fn window_drag(window: tauri::Window) {
    let _ = window.start_dragging();
}

#[tauri::command]
fn set_muted(state: State<'_, AppState>, muted: bool) {
    if let Ok(guard) = state.engine.lock() {
        if let Some(engine) = guard.as_ref() {
            engine.set_muted(muted);
        }
    }
}

#[tauri::command]
fn open_log() -> Result<(), String> {
    let path = noise_canceller_core::log::path();
    if !path.exists() {
        noise_canceller_core::log::info("log opened from the settings screen");
    }
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Licenses travel next to the exe: `THIRD_PARTY_NOTICES.txt` and `licenses/`.
/// In a dev build they sit one level up, in the repository root.
#[tauri::command]
fn open_licenses() -> Result<(), String> {
    let root = noise_canceller_core::paths::root_dir();
    let candidates = [root.clone(), root.parent().unwrap_or(&root).to_path_buf()];

    for dir in candidates.iter() {
        let notices = dir.join("THIRD_PARTY_NOTICES.txt");
        let folder = dir.join("licenses");
        if notices.is_file() || folder.is_dir() {
            let mut command = std::process::Command::new("explorer");
            if notices.is_file() {
                command.arg(format!("/select,{}", notices.display()));
            } else {
                command.arg(&folder);
            }
            command
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    Err("файлы лицензий не найдены рядом с программой".to_string())
}

#[tauri::command]
fn reset_settings(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Settings, String> {
    let fresh = Settings::default();
    apply_settings(&app, &state, fresh.clone(), true);
    noise_canceller_core::log::info("settings reset to defaults");
    Ok(fresh)
}

#[tauri::command]
fn export_settings(state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let path = paths::data_dir().join("settings-export.json");
    settings.save(&path);
    noise_canceller_core::log::info(&format!("settings exported to {}", path.display()));
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn import_settings(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Settings, String> {
    let path = paths::data_dir().join("settings-export.json");
    if !path.exists() {
        return Err("нет файла settings-export.json в папке data".to_string());
    }
    let settings = Settings::load(&path);
    apply_settings(&app, &state, settings.clone(), true);
    noise_canceller_core::log::info("settings imported");
    Ok(settings)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AbEntry {
    stamp: u64,
    raw: String,
    clean: String,
}

#[tauri::command(async)]
fn list_recordings(state: State<'_, AppState>) -> Vec<AbEntry> {
    // history depth follows `keep_recordings`: the list used to stop at 20
    // while pruning happily kept 100
    let keep = state
        .settings
        .lock()
        .map(|s| s.keep_recordings as usize)
        .unwrap_or(20)
        .max(1);
    recordings::pairs()
        .into_iter()
        .take(keep)
        .map(|stamp| {
            let (raw, clean) = recordings::pair(stamp);
            AbEntry {
                stamp,
                raw: raw.to_string_lossy().to_string(),
                clean: clean.to_string_lossy().to_string(),
            }
        })
        .collect()
}

#[tauri::command(async)]
fn delete_recording(stamp: u64) -> Result<(), String> {
    recordings::delete(stamp).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn default_devices() -> (Option<String>, Option<String>) {
    devices::default_names()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemInfo {
    logical_cores: u16,
    physical_cores: u16,
    recommended_threads: u16,
}

/// What the machine offers, so the thread picker can explain itself instead of
/// showing a bare 1–8 slider.
#[tauri::command(async)]
fn system_info(chunk_ms: f32) -> SystemInfo {
    let logical = noise_canceller_core::settings::logical_cores();
    SystemInfo {
        logical_cores: logical,
        physical_cores: (logical / 2).max(1),
        recommended_threads: noise_canceller_core::settings::recommended_threads(chunk_ms),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileScoreDto {
    path: String,
    score: i32,
    snr_db: f32,
}

#[tauri::command(async)]
fn profile_scores() -> Vec<ProfileScoreDto> {
    noise_canceller_core::profile::list()
        .into_iter()
        .filter_map(|path| {
            let quality =
                noise_canceller_core::profile::score_file(std::path::Path::new(&path)).ok()?;
            Some(ProfileScoreDto {
                path,
                score: quality.score,
                snr_db: quality.snr_db,
            })
        })
        .collect()
}

/// How much louder the processed take is than the raw one (dB, speech RMS).
#[tauri::command(async)]
fn ab_level_delta(stamp: u64) -> Result<f32, String> {
    let raw_db = noise_canceller_core::playback::speech_rms_db(&recordings::raw(stamp))
        .map_err(|e| e.to_string())?;
    let clean_db = noise_canceller_core::playback::speech_rms_db(&recordings::clean(stamp))
        .map_err(|e| e.to_string())?;
    Ok(clean_db - raw_db)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AbMetricsDto {
    speech_level_db: f32,
    level_delta_db: f32,
    pause_contrast_db: f32,
    longest_dropout_s: f32,
}

/// Numbers for one take: speech level, how far pauses dropped and the longest
/// stretch where the model muted a word. The verdict comes from
/// `AbMetrics::problems()`, the same code `nc metrics --check` runs, so the card
/// and the regression cannot disagree.
#[tauri::command(async)]
fn ab_metrics(stamp: u64) -> Result<AbMetricsDto, String> {
    let m = noise_canceller_core::playback::ab_metrics(
        &recordings::raw(stamp),
        &recordings::clean(stamp),
    )
    .map_err(|e| e.to_string())?;
    Ok(AbMetricsDto {
        speech_level_db: (m.speech_level_db * 10.0).round() / 10.0,
        level_delta_db: (m.level_delta_db * 10.0).round() / 10.0,
        pause_contrast_db: (m.pause_contrast_db * 10.0).round() / 10.0,
        longest_dropout_s: (m.longest_dropout_s * 100.0).round() / 100.0,
    })
}

#[tauri::command(async)]
fn prune_recordings(state: State<'_, AppState>) -> usize {
    let keep = state
        .settings
        .lock()
        .map(|s| s.keep_recordings as usize)
        .unwrap_or(20);
    recordings::prune(keep)
}

/// Tray menu kept in state: showing it ourselves places it at the cursor,
/// going down. The icon's built-in menu is bottom-aligned, which puts it
/// off-screen when the taskbar sits at the top of the screen.
struct TrayState {
    menu: Menu<tauri::Wry>,
    status: MenuItem<tauri::Wry>,
    profile: MenuItem<tauri::Wry>,
    mute: CheckMenuItem<tauri::Wry>,
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn apply_mute(app: &tauri::AppHandle, muted: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(guard) = state.engine.lock() {
            if let Some(engine) = guard.as_ref() {
                engine.set_muted(muted);
            }
        }
    }
}

/// Global hotkey: mute what goes to the cable without touching the window.
fn toggle_mute(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(guard) = state.engine.lock() {
            if let Some(engine) = guard.as_ref() {
                let next = !engine.is_muted();
                engine.set_muted(next);
                noise_canceller_core::log::info(if next { "mute on" } else { "mute off" });
            }
        }
    }
}

fn register_hotkey(app: &tauri::AppHandle, hotkey: &str) -> Option<String> {
    let _ = app.global_shortcut().unregister_all();
    match hotkey.parse::<Shortcut>() {
        Ok(shortcut) => {
            if let Err(err) = app.global_shortcut().register(shortcut) {
                noise_canceller_core::log::info(&format!(
                    "hotkey '{hotkey}' registration failed: {err}"
                ));
                return Some(format!(
                    "Не удалось занять сочетание {hotkey}: {err}. Его уже использует другая программа."
                ));
            }
            None
        }
        Err(err) => {
            noise_canceller_core::log::info(&format!("hotkey '{hotkey}' is not valid: {err}"));
            Some(format!("Сочетание {hotkey} не распознано: {err}"))
        }
    }
}

fn refresh_tray(app: &tauri::AppHandle) {
    let (Some(tray), Some(state)) = (app.try_state::<TrayState>(), app.try_state::<AppState>())
    else {
        return;
    };
    let (running, muted) = {
        let guard = state.engine.lock().ok();
        match guard.as_ref().and_then(|g| g.as_ref()) {
            Some(engine) => (true, engine.is_muted()),
            None => (false, false),
        }
    };
    let profile = state
        .settings
        .lock()
        .ok()
        .and_then(|s| s.profile_path.clone())
        .map(|p| p.rsplit(['\\', '/']).next().unwrap_or(&p).to_string())
        .unwrap_or_else(|| "авто".to_string());
    let _ = tray.status.set_text(if running {
        "Обработка включена"
    } else {
        "Выключено"
    });
    let _ = tray.profile.set_text(format!("Профиль: {profile}"));
    let _ = tray.mute.set_checked(muted);
}

/// Only the size is remembered: the window always opens in the middle of the
/// screen, so a saved position would just make it jump after creation.
#[derive(Serialize, Deserialize)]
struct WindowState {
    #[serde(default)]
    w: u32,
    #[serde(default)]
    h: u32,
}

fn window_state_path() -> std::path::PathBuf {
    paths::data_dir().join("window.json")
}

/// Set while "Выход" is running: from that moment the window must be allowed to
/// close instead of hiding to the tray, and the process has to end for real.
static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Quit from the tray: release the audio streams, save the window, and make sure
/// the process is gone. `AppHandle::exit` sends a request to the event loop, but
/// the tray menu runs a modal loop on Windows, so the request can sit unprocessed
/// and leave a windowless process behind (the tray icon stayed after "Выход").
fn quit_app(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;

    EXITING.store(true, Ordering::SeqCst);
    save_window(app);
    noise_canceller_core::log::info("app quit from tray");

    // watchdog first: `engine.stop()` joins the processing thread and must not be
    // able to keep the process alive
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        std::process::exit(0);
    });

    if let Some(state) = app.try_state::<AppState>() {
        let engine = state.engine.lock().ok().and_then(|mut guard| guard.take());
        if let Some(mut engine) = engine {
            engine.stop();
        }
    }

    app.exit(0);
}

fn save_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    // a minimized window reports (-32000, -32000) and a bogus size; saving that
    // made the next start look broken
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let state = WindowState {
        w: size.width,
        h: size.height,
    };
    if let Ok(text) = serde_json::to_string(&state) {
        let _ = std::fs::write(window_state_path(), text);
    }
}

/// Restores the remembered size only. The position is never restored: the window
/// is created hidden (`visible: false` in `tauri.conf.json`) and centered here, so
/// it appears once, already in the middle of the screen, instead of first showing
/// up at the OS default spot and then jumping to the saved rectangle.
fn restore_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Ok(text) = std::fs::read_to_string(window_state_path()) {
        if let Ok(state) = serde_json::from_str::<WindowState>(&text) {
            let w = state.w.max(640);
            let h = state.h.max(480);
            let _ = window.set_size(tauri::PhysicalSize::new(w, h));
        }
    }
    let _ = window.center();
}

fn main() {
    let settings_path = paths::data_dir().join("settings.json");
    let settings = Settings::load(&settings_path);
    // persist a migrated file once, so the next start reads the tuned values
    settings.save(&settings_path);
    let start_minimized = settings.start_minimized;
    let start_hotkey = settings.mute_hotkey.clone();
    noise_canceller_core::log::info("app start");
    let pruned = recordings::prune(settings.keep_recordings as usize);
    if pruned > 0 {
        noise_canceller_core::log::info(&format!("removed {pruned} old A/B files"));
    }

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let (hotkey, mode) = app
                        .try_state::<AppState>()
                        .and_then(|state| {
                            state
                                .settings
                                .lock()
                                .ok()
                                .map(|s| (s.mute_hotkey.clone(), s.hotkey_mode.clone()))
                        })
                        .unwrap_or_default();
                    let Ok(current) = hotkey.parse::<Shortcut>() else {
                        return;
                    };
                    if *shortcut != current {
                        return;
                    }
                    if mode == "hold" {
                        // push-to-talk: sound while the key is held
                        apply_mute(app, event.state() != ShortcutState::Pressed);
                    } else if event.state() == ShortcutState::Pressed {
                        toggle_mute(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                // a second launch while the window is minimized used to show
                // and focus it but leave it minimized
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(move |app| {
            use tauri::menu::{
                CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
            };
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            let status = MenuItemBuilder::with_id("status", "Выключено")
                .enabled(false)
                .build(app)?;
            let profile = MenuItemBuilder::with_id("profile", "Профиль: —")
                .enabled(false)
                .build(app)?;
            let mute = CheckMenuItemBuilder::with_id("mute", "Микрофон выключен")
                .checked(false)
                .build(app)?;
            let open = MenuItemBuilder::with_id("open", "Открыть").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Выход").build(app)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&status, &profile, &sep1, &mute, &sep2, &open, &quit])
                .build()?;
            app.manage(TrayState {
                menu: menu.clone(),
                status: status.clone(),
                profile: profile.clone(),
                mute: mute.clone(),
            });

            let mut builder = TrayIconBuilder::with_id("main")
                .show_menu_on_left_click(false)
                .tooltip("Noise Canceller")
                .on_tray_icon_event(|tray, event| {
                    let TrayIconEvent::Click {
                        button,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    else {
                        return;
                    };
                    let app = tray.app_handle();
                    match button {
                        MouseButton::Left => show_main(app),
                        MouseButton::Right => {
                            refresh_tray(app);
                            if let Some(state) = app.try_state::<TrayState>() {
                                if let Some(webview) = app.get_webview_window("main") {
                                    let _ =
                                        state.menu.popup(tauri::Webview::window(webview.as_ref()));
                                }
                            }
                        }
                        _ => {}
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                builder = builder.icon(icon.clone());
            }
            builder.build(app)?;

            register_hotkey(app.handle(), &start_hotkey);
            // the window is created hidden: restore its size, center it, and only
            // then show it, so the first frame the user sees is the final one
            restore_window(app.handle());
            if !start_minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => quit_app(app),
            "mute" => toggle_mute(app),
            _ => {}
        })
        .manage(AppState {
            engine: Mutex::new(None),
            settings: Mutex::new(settings),
            settings_path,
            maximized: std::sync::atomic::AtomicBool::new(false),
            restore_rect: Mutex::new(None),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if EXITING.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                save_window(window.app_handle());
                let close_to_tray = window
                    .app_handle()
                    .state::<AppState>()
                    .settings
                    .lock()
                    .map(|s| s.close_to_tray)
                    .unwrap_or(true);

                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            start,
            stop,
            stats,
            get_settings,
            set_settings,
            record_profile,
            probe_mic,
            probe_spectrum,
            list_profiles,
            delete_profile,
            ab_start,
            ab_status,
            last_ab,
            default_devices,
            system_info,
            profile_scores,
            ab_level_delta,
            ab_metrics,
            prune_recordings,
            set_muted,
            open_log,
            open_licenses,
            reset_settings,
            export_settings,
            import_settings,
            list_recordings,
            delete_recording,
            cable_present,
            install_cable,
            check_model_update,
            play_profile,
            stop_playback,
            set_autostart,
            autostart_enabled,
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_drag
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
