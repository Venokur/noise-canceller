//! Every `#[tauri::command]` the window can call, plus the PowerShell bridge.
//!
//! Long work (`Engine::start`, device enumeration, playback, file scans) runs
//! with `#[tauri::command(async)]` or on a blocking thread: a synchronous
//! command executes on the main thread and freezes the window.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;

use noise_canceller_core::engine::Engine;
use noise_canceller_core::lang::Lang;
use noise_canceller_core::paths;
use noise_canceller_core::settings::Settings;
use noise_canceller_core::{devices, recordings};
use tauri::{Manager, State};

use crate::dto::{
    AbEntry, AbMetricsDto, AbWaveformDto, LastAb, ProfileScoreDto, SettingsResult,
    SpeakerSimilarityDto, StartResult, StatsDto, SystemInfo,
};
use crate::state::AppState;
use crate::tray::register_hotkey;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[tauri::command(async)]
pub(crate) fn list_devices() -> (Vec<String>, Vec<String>) {
    (devices::input_devices(), devices::output_devices())
}

#[tauri::command]
pub(crate) fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command(async)]
pub(crate) async fn check_app_update(state: State<'_, AppState>) -> Result<String, String> {
    const SCRIPT: &str =
        "$r = Invoke-RestMethod 'https://api.github.com/repos/Venokur/noise-canceller/releases/latest'; $r.tag_name";
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    let output = tauri::async_runtime::spawn_blocking(move || powershell(SCRIPT))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(lang
            .text(
                "не удалось проверить обновления на github.com",
                "could not check for updates on github.com",
            )
            .to_string());
    }
    let latest = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let latest = latest.trim_start_matches('v');
    if latest.is_empty() {
        return Err(lang
            .text(
                "github.com вернул пустой ответ",
                "github.com returned an empty answer",
            )
            .to_string());
    }
    let current = env!("CARGO_PKG_VERSION");
    if latest == current {
        Ok(lang.fmt(
            "Установлена последняя версия {version}.",
            "The latest version {version} is installed.",
            &[("version", current)],
        ))
    } else {
        Ok(lang.fmt(
            "Доступна версия {latest}; установлена {current}.",
            "Version {latest} is available; {current} is installed.",
            &[("latest", latest), ("current", current)],
        ))
    }
}

#[tauri::command(async)]
pub(crate) fn save_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    use std::fmt::Write as _;

    let settings = state.settings_snapshot();
    let stats = state.with_engine(|engine| engine.stats());
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Noise Canceller {} · {}",
        env!("CARGO_PKG_VERSION"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let _ = writeln!(text, "\n## settings\n");
    let _ = writeln!(
        text,
        "{}",
        serde_json::to_string_pretty(&settings).unwrap_or_default()
    );
    let _ = writeln!(text, "\n## stats\n");
    match stats {
        Some(stats) => {
            let _ = writeln!(
                text,
                "{}",
                serde_json::to_string_pretty(&StatsDto::from(stats)).unwrap_or_default()
            );
        }
        None => {
            let _ = writeln!(text, "engine is not running");
        }
    }
    let _ = writeln!(text, "\n## devices\n");
    let _ = writeln!(text, "inputs: {:?}", devices::input_devices());
    let _ = writeln!(text, "outputs: {:?}", devices::output_devices());
    let _ = writeln!(text, "\n## app.log (last 200 lines)\n");
    let log_path = paths::data_dir().join("app.log");
    if let Ok(log) = std::fs::read_to_string(&log_path) {
        let lines: Vec<&str> = log.lines().rev().take(200).collect();
        for line in lines.iter().rev() {
            let _ = writeln!(text, "{line}");
        }
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = paths::data_dir().join(format!("diagnostics-{stamp}.txt"));
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    noise_canceller_core::log::info(&format!("diagnostics saved to {}", path.display()));
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub(crate) fn open_sound_settings() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg("ms-settings:sound")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) fn input_channels(name: Option<String>) -> u16 {
    devices::input_channels(name.as_deref())
}

#[tauri::command]
pub(crate) async fn start(
    app: tauri::AppHandle,
    input: Option<String>,
    output: Option<String>,
    voice: Option<String>,
    test_mode: bool,
) -> Result<StartResult, String> {
    // a dedicated blocking thread: loading the model takes 1.7-2.1 s and must
    // not occupy the main thread (the window would freeze) or a runtime worker
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        // never hold the engine lock while the model loads: `stats` polls it
        // from the UI every 150 ms and would block for the whole start-up
        {
            let guard = state.engine.lock().map_err(|e| e.to_string())?;
            if guard.is_some() {
                return Ok(StartResult { started: false });
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
            Ok(StartResult { started: true })
        } else {
            // another start won the race: this engine is dropped unused
            Ok(StartResult { started: false })
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub(crate) fn stop(state: State<'_, AppState>) {
    // take the engine out first: joining the processing thread takes up to
    // ~100 ms and `stats` would otherwise wait on the lock from the UI thread
    let engine = state.engine.lock().ok().and_then(|mut guard| guard.take());
    if let Some(mut engine) = engine {
        engine.stop();
    }
}

#[tauri::command]
pub(crate) fn stats(state: State<'_, AppState>) -> StatsDto {
    state
        .with_engine(|engine| StatsDto::from(engine.stats()))
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings_snapshot()
}

#[tauri::command]
pub(crate) fn set_settings(
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
pub(crate) fn apply_settings(
    app: &tauri::AppHandle,
    state: &AppState,
    settings: Settings,
    rearm_hotkey: bool,
) -> Option<String> {
    let previous_language = state.settings.lock().ok().map(|s| s.language.clone());
    settings.save(&state.settings_path);
    if let Ok(mut guard) = state.settings.lock() {
        *guard = settings.clone();
    }
    let lang = Lang::from_settings(&settings);
    // the tray was built with the old language: rename it before the window
    // redraws, so the two never disagree for longer than a save
    if previous_language.as_deref() != Some(settings.language.as_str()) {
        crate::tray::relabel(app, lang);
    }
    state.with_engine(|engine| engine.update_settings(&settings));
    if rearm_hotkey {
        register_hotkey(app, &settings.mute_hotkey, lang)
    } else {
        None
    }
}

#[tauri::command]
pub(crate) async fn probe_mic(
    state: State<'_, AppState>,
    input: Option<String>,
    seconds: f32,
) -> Result<noise_canceller_core::profile::Quality, String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        noise_canceller_core::profile::probe(input.as_deref(), seconds, lang)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn probe_spectrum() -> Vec<f32> {
    noise_canceller_core::profile::take_bands().to_vec()
}

#[tauri::command]
pub(crate) async fn record_profile(
    state: State<'_, AppState>,
    input: Option<String>,
    seconds: f32,
) -> Result<noise_canceller_core::profile::Profile, String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        noise_canceller_core::profile::record(input.as_deref(), seconds, lang)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) async fn import_profile(
    input: Option<String>,
    path: String,
) -> Result<noise_canceller_core::profile::Profile, String> {
    tauri::async_runtime::spawn_blocking(move || {
        noise_canceller_core::profile::import(std::path::Path::new(&path), input.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) fn list_profiles() -> Vec<String> {
    noise_canceller_core::profile::list()
}

#[tauri::command(async)]
pub(crate) fn delete_profile(path: String) -> Result<(), String> {
    noise_canceller_core::profile::delete(&path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) async fn speaker_recommendation(
    state: State<'_, AppState>,
) -> Result<noise_canceller_core::speaker::Recommendation, String> {
    let settings = state.settings_snapshot();
    let lang = Lang::from_settings(&settings);
    let profile = settings.profile_path.clone().ok_or_else(|| {
        lang.text(
            "сначала запишите профиль голоса",
            "record a voice profile first",
        )
        .to_string()
    })?;
    let model = noise_canceller_core::paths::speaker_model();
    if !model.exists() {
        return Err(lang
            .text(
                "модель проверки голоса не найдена",
                "the speaker-check model is missing",
            )
            .to_string());
    }
    let takes: Vec<std::path::PathBuf> = noise_canceller_core::recordings::pairs()
        .into_iter()
        .take(20)
        .map(noise_canceller_core::recordings::raw)
        .collect();
    if takes.is_empty() {
        return Err(lang
            .text(
                "нет записей A/B для оценки",
                "there are no A/B takes to evaluate",
            )
            .to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut embedder = noise_canceller_core::model::SpeakerEmbedder::new(&model, 2)
            .map_err(|e| e.to_string())?;
        noise_canceller_core::speaker::recommend_threshold(
            &mut embedder,
            std::path::Path::new(&profile),
            &takes,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub(crate) async fn profile_consistency(
    path: String,
) -> Result<noise_canceller_core::profile::ProfileMeta, String> {
    let model = noise_canceller_core::paths::speaker_model();
    if !model.exists() {
        return Err("speaker-check model is missing".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut embedder = noise_canceller_core::model::SpeakerEmbedder::new(&model, 2)
            .map_err(|e| e.to_string())?;
        let value = noise_canceller_core::speaker::profile_consistency(
            &mut embedder,
            std::path::Path::new(&path),
        )
        .map_err(|e| e.to_string())?;
        noise_canceller_core::profile::set_consistency(std::path::Path::new(&path), value)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub(crate) fn rename_profile(
    path: String,
    name: String,
) -> Result<noise_canceller_core::profile::ProfileMeta, String> {
    noise_canceller_core::profile::rename(std::path::Path::new(&path), &name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn ab_start(state: State<'_, AppState>, seconds: f32) -> Result<(), String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    state
        .with_engine(|engine| engine.start_ab(seconds).map_err(|e| e.to_string()))
        .unwrap_or_else(|| {
            Err(lang
                .text("движок не запущен", "the engine is not running")
                .to_string())
        })
}

#[tauri::command]
pub(crate) fn ab_status(
    state: State<'_, AppState>,
) -> Option<noise_canceller_core::engine::AbStatus> {
    state.with_engine(|engine| engine.ab_status())
}

/// Newest finished A/B pair on disk. The in-memory paths die with the app, so
/// without this the play buttons disappear after a restart even though the
/// files are still there.
#[tauri::command(async)]
pub(crate) fn last_ab() -> Option<LastAb> {
    let (_, raw, clean) = recordings::newest_pair()?;
    Some(LastAb {
        folder: recordings::dir().to_string_lossy().to_string(),
        raw: raw.to_string_lossy().to_string(),
        clean: clean.to_string_lossy().to_string(),
    })
}

#[tauri::command(async)]
pub(crate) fn cable_present() -> bool {
    devices::cable_present()
}

#[tauri::command]
pub(crate) fn stop_playback() {
    noise_canceller_core::playback::stop();
}

#[tauri::command]
pub(crate) async fn play_profile(
    state: State<'_, AppState>,
    path: String,
    output: Option<String>,
    normalize: Option<bool>,
) -> Result<(), String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let target = output.as_deref().filter(|s| !s.is_empty());
        if normalize.unwrap_or(false) {
            noise_canceller_core::playback::play_normalized(
                std::path::Path::new(&path),
                target,
                lang,
            )
        } else {
            noise_canceller_core::playback::play(std::path::Path::new(&path), target, lang)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn play_ab_pair(
    state: State<'_, AppState>,
    raw: String,
    clean: String,
    output: Option<String>,
    normalize: Option<bool>,
    initial_clean: Option<bool>,
) -> Result<(), String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let target = output.as_deref().filter(|s| !s.is_empty());
        noise_canceller_core::playback::play_pair(
            std::path::Path::new(&raw),
            std::path::Path::new(&clean),
            target,
            normalize.unwrap_or(true),
            initial_clean.unwrap_or(false),
            lang,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_ab_side(clean: bool) {
    noise_canceller_core::playback::set_pair_side(clean);
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
pub(crate) async fn install_cable() -> Result<String, String> {
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
pub(crate) async fn check_model_update(state: State<'_, AppState>) -> Result<String, String> {
    const SCRIPT: &str =
        "$m = Invoke-RestMethod 'https://huggingface.co/api/models/arbazsiddiqui/voxlock'; $m.sha";

    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();

    let output = tauri::async_runtime::spawn_blocking(move || powershell(SCRIPT))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(lang
            .text(
                "не удалось связаться с huggingface.co",
                "could not reach huggingface.co",
            )
            .to_string());
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        return Err(lang
            .text(
                "пустой ответ от huggingface.co",
                "huggingface.co returned an empty answer",
            )
            .to_string());
    }

    let state_path = paths::data_dir().join("model-state.json");
    let known = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("sha")
                .and_then(|s| s.as_str())
                .map(str::to_string)
        });

    let short: String = sha.chars().take(8).collect();
    let write = |value: &str| {
        let _ = std::fs::write(&state_path, format!("{{\"sha\":\"{value}\"}}"));
    };

    match known {
        None => {
            write(&sha);
            Ok(lang.fmt(
                "Модель VoxLock · ревизия {short} · отслеживание включено",
                "VoxLock model · revision {short} · tracking enabled",
                &[("short", &short)],
            ))
        }
        Some(previous) if previous == sha => Ok(lang.fmt(
            "Модель актуальна · ревизия {short}",
            "The model is up to date · revision {short}",
            &[("short", &short)],
        )),
        Some(_) => {
            write(&sha);
            Ok(lang.fmt(
                "На huggingface.co есть новая ревизия ({short}) — она появится в следующем обновлении приложения",
                "A new revision ({short}) is available on huggingface.co — it will arrive with the next app update",
                &[("short", &short)],
            ))
        }
    }
}

#[tauri::command]
pub(crate) fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
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
pub(crate) fn autostart_enabled(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub(crate) fn window_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub(crate) fn window_toggle_maximize(window: tauri::Window, state: State<'_, AppState>) {
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
pub(crate) fn window_close(window: tauri::Window) {
    let _ = window.close();
}

#[tauri::command]
pub(crate) fn window_drag(window: tauri::Window) {
    let _ = window.start_dragging();
}

#[tauri::command]
pub(crate) fn set_muted(state: State<'_, AppState>, muted: bool) {
    state.with_engine(|engine| engine.set_muted(muted));
}

#[tauri::command]
pub(crate) fn open_log() -> Result<(), String> {
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
pub(crate) fn open_licenses(state: State<'_, AppState>) -> Result<(), String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    let root = paths::root_dir();
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

    Err(lang
        .text(
            "файлы лицензий не найдены рядом с программой",
            "the license files were not found next to the program",
        )
        .to_string())
}

#[tauri::command]
pub(crate) fn reset_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsResult, String> {
    let mut fresh = Settings::default();
    // resolve "auto" now: the window renders this snapshot, and an unresolved
    // language would show as an empty picker
    fresh.sanitize();
    let warning = apply_settings(&app, &state, fresh.clone(), true);
    noise_canceller_core::log::info("settings reset to defaults");
    Ok(SettingsResult {
        settings: fresh,
        warning,
    })
}

#[tauri::command]
pub(crate) fn export_settings(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let mut path = std::path::PathBuf::from(path);
    if path.extension().is_none() {
        path.set_extension("json");
    }
    settings.save(&path);
    noise_canceller_core::log::info(&format!("settings exported to {}", path.display()));
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub(crate) fn import_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<SettingsResult, String> {
    let path = std::path::PathBuf::from(path);
    if !path.exists() {
        let lang = state
            .settings
            .lock()
            .map(|s| Lang::from_settings(&s))
            .unwrap_or_default();
        return Err(lang
            .text("файл настроек не найден", "the settings file was not found")
            .to_string());
    }
    let settings = Settings::load(&path);
    let warning = apply_settings(&app, &state, settings.clone(), true);
    noise_canceller_core::log::info(&format!("settings imported from {}", path.display()));
    Ok(SettingsResult { settings, warning })
}

#[tauri::command(async)]
pub(crate) fn list_recordings(state: State<'_, AppState>) -> Vec<AbEntry> {
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
pub(crate) fn delete_recording(stamp: u64) -> Result<(), String> {
    recordings::delete(stamp).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) fn default_devices() -> (Option<String>, Option<String>) {
    devices::default_names()
}

/// What the machine offers, so the thread picker can explain itself instead of
/// showing a bare 1–8 slider.
#[tauri::command(async)]
pub(crate) fn system_info(chunk_ms: f32) -> SystemInfo {
    let logical = noise_canceller_core::settings::logical_cores();
    SystemInfo {
        logical_cores: logical,
        physical_cores: (logical / 2).max(1),
        recommended_threads: noise_canceller_core::settings::recommended_threads(chunk_ms),
    }
}

#[tauri::command(async)]
pub(crate) fn profile_meta(path: String) -> Option<noise_canceller_core::profile::ProfileMeta> {
    noise_canceller_core::profile::read_meta(std::path::Path::new(&path))
}

#[tauri::command(async)]
pub(crate) fn profile_scores() -> Vec<ProfileScoreDto> {
    noise_canceller_core::profile::list()
        .into_iter()
        .filter_map(|path| {
            let quality =
                noise_canceller_core::profile::score_file(std::path::Path::new(&path)).ok()?;
            // the sidecar holds the grade of the raw take; re-scoring the saved
            // wav used to grade the normalised copy (and count its clamps as
            // clipping), so a quiet microphone could look worse than it is
            let meta = noise_canceller_core::profile::read_meta(std::path::Path::new(&path));
            Some(ProfileScoreDto {
                path,
                name: meta.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
                device: meta.as_ref().map(|m| m.device.clone()).unwrap_or_default(),
                score: meta.as_ref().map(|m| m.score).unwrap_or(quality.score),
                snr_db: meta.as_ref().map(|m| m.snr_db).unwrap_or(quality.snr_db),
            })
        })
        .collect()
}

/// How much louder the processed take is than the raw one (dB, speech RMS).
#[tauri::command(async)]
pub(crate) fn ab_level_delta(stamp: u64) -> Result<f32, String> {
    let raw_db = noise_canceller_core::playback::speech_rms_db(&recordings::raw(stamp))
        .map_err(|e| e.to_string())?;
    let clean_db = noise_canceller_core::playback::speech_rms_db(&recordings::clean(stamp))
        .map_err(|e| e.to_string())?;
    Ok(clean_db - raw_db)
}

/// Numbers for one take: speech level, how far pauses dropped and the longest
/// stretch where the model muted a word. The verdict comes from
/// `AbMetrics::problems()`, the same code `nc metrics --check` runs, so the card
/// and the regression cannot disagree.
#[tauri::command(async)]
pub(crate) fn ab_settings(state: State<'_, AppState>, stamp: u64) -> Result<Settings, String> {
    let path = noise_canceller_core::recordings::snapshot(stamp);
    if !path.exists() {
        let lang = state
            .settings
            .lock()
            .map(|s| Lang::from_settings(&s))
            .unwrap_or_default();
        return Err(lang
            .text(
                "снимок настроек этого дубля не найден",
                "the settings snapshot for this take was not found",
            )
            .to_string());
    }
    Ok(Settings::load(&path))
}

#[tauri::command(async)]
pub(crate) fn ab_waveform(stamp: u64) -> Result<AbWaveformDto, String> {
    use noise_canceller_core::wav;

    let (raw, raw_rate) = wav::read_mono(&recordings::raw(stamp)).map_err(|e| e.to_string())?;
    let (clean, clean_rate) =
        wav::read_mono(&recordings::clean(stamp)).map_err(|e| e.to_string())?;
    let duration = (raw.len() as f32 / raw_rate.max(1) as f32)
        .max(clean.len() as f32 / clean_rate.max(1) as f32);

    fn envelope(samples: &[f32], buckets: usize) -> Vec<f32> {
        if samples.is_empty() {
            return vec![0.0; buckets];
        }
        let step = (samples.len() as f32 / buckets as f32).max(1.0);
        (0..buckets)
            .map(|bucket| {
                let start = (bucket as f32 * step) as usize;
                let end = (((bucket + 1) as f32 * step) as usize).min(samples.len());
                samples[start..end.max(start + 1)]
                    .iter()
                    .fold(0.0f32, |max, value| max.max(value.abs()))
            })
            .collect()
    }

    Ok(AbWaveformDto {
        duration,
        raw: envelope(&raw, 240),
        clean: envelope(&clean, 240),
    })
}

#[tauri::command(async)]
pub(crate) async fn ab_speaker(
    state: State<'_, AppState>,
    stamp: u64,
) -> Result<SpeakerSimilarityDto, String> {
    let settings = state.settings_snapshot();
    let lang = Lang::from_settings(&settings);
    let profile = settings.profile_path.clone().ok_or_else(|| {
        lang.text(
            "для проверки голоса нужен записанный профиль",
            "a recorded profile is needed to check the voice",
        )
        .to_string()
    })?;
    let model = noise_canceller_core::paths::speaker_model();
    if !model.exists() {
        return Err(lang
            .text(
                "модель проверки голоса не найдена",
                "the speaker-check model is missing",
            )
            .to_string());
    }
    let raw = recordings::raw(stamp);
    tauri::async_runtime::spawn_blocking(move || {
        let mut embedder = noise_canceller_core::model::SpeakerEmbedder::new(&model, 2)
            .map_err(|e| e.to_string())?;
        let (samples, rate) =
            noise_canceller_core::wav::read_mono(&raw).map_err(|e| e.to_string())?;
        noise_canceller_core::speaker::similarity_to_profile(
            &mut embedder,
            std::path::Path::new(&profile),
            &samples,
            rate,
        )
        .map(SpeakerSimilarityDto::from)
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub(crate) fn ab_metrics(state: State<'_, AppState>, stamp: u64) -> Result<AbMetricsDto, String> {
    let lang = state
        .settings
        .lock()
        .map(|s| Lang::from_settings(&s))
        .unwrap_or_default();
    let m = noise_canceller_core::playback::ab_metrics(
        &recordings::raw(stamp),
        &recordings::clean(stamp),
        lang,
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
pub(crate) fn prune_recordings(state: State<'_, AppState>) -> usize {
    let keep = state
        .settings
        .lock()
        .map(|s| s.keep_recordings as usize)
        .unwrap_or(20);
    recordings::prune(keep)
}
