//! Tray icon, global mute shortcut, window rectangle and quitting.
//!
//! The tray menu is shown by hand (`Menu::popup` at the cursor) instead of being
//! attached to the icon: the built-in menu is bottom-aligned and goes off-screen
//! when the taskbar sits at the top.

use std::sync::atomic::{AtomicBool, Ordering};

use noise_canceller_core::lang::Lang;
use noise_canceller_core::paths;
use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, ContextMenu, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::AppState;

/// Tray menu kept in state: showing it ourselves places it at the cursor,
/// going down. Every label is stored, because the language can change while the
/// app runs and the items have to be renamed in place.
struct TrayState {
    menu: Menu<tauri::Wry>,
    status: MenuItem<tauri::Wry>,
    profile: MenuItem<tauri::Wry>,
    mute: CheckMenuItem<tauri::Wry>,
    open: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

pub(crate) fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Push-to-talk and the tray checkbox share this: mute the cable, keep the
/// meters running.
pub(crate) fn apply_mute(app: &tauri::AppHandle, muted: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.with_engine(|engine| engine.set_muted(muted));
    }
    refresh_tray(app);
}

pub(crate) fn toggle_mute(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Some(next) = state.with_engine(|engine| {
            let next = !engine.is_muted();
            engine.set_muted(next);
            next
        }) {
            noise_canceller_core::log::info(if next { "mute on" } else { "mute off" });
        }
    }
    refresh_tray(app);
}

pub(crate) fn register_hotkey(app: &tauri::AppHandle, hotkey: &str, lang: Lang) -> Option<String> {
    let _ = app.global_shortcut().unregister_all();
    match hotkey.parse::<Shortcut>() {
        Ok(shortcut) => {
            if let Err(err) = app.global_shortcut().register(shortcut) {
                noise_canceller_core::log::info(&format!(
                    "hotkey '{hotkey}' registration failed: {err}"
                ));
                return Some(lang.fmt(
                    "Не удалось занять сочетание {hotkey}: {err}. Его уже использует другая программа.",
                    "Could not register the shortcut {hotkey}: {err}. Another program already uses it.",
                    &[("hotkey", hotkey), ("err", &err.to_string())],
                ));
            }
            None
        }
        Err(err) => {
            noise_canceller_core::log::info(&format!("hotkey '{hotkey}' is not valid: {err}"));
            Some(lang.fmt(
                "Сочетание {hotkey} не распознано: {err}",
                "The shortcut {hotkey} was not recognised: {err}",
                &[("hotkey", hotkey), ("err", &err.to_string())],
            ))
        }
    }
}

pub(crate) fn handle_hotkey(
    app: &tauri::AppHandle,
    shortcut: &Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
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
}

fn refresh_tray(app: &tauri::AppHandle) {
    let (Some(tray), Some(state)) = (app.try_state::<TrayState>(), app.try_state::<AppState>())
    else {
        return;
    };
    let muted = state.with_engine(|engine| engine.is_muted());
    let running = muted.is_some();
    let muted = muted.unwrap_or(false);
    // one lock for both: the language decides the words and the profile name
    let (lang, profile_path) = state
        .settings
        .lock()
        .map(|s| (Lang::from_settings(&s), s.profile_path.clone()))
        .unwrap_or((Lang::Ru, None));
    let profile = profile_path
        .map(|p| p.rsplit(['\\', '/']).next().unwrap_or(&p).to_string())
        .unwrap_or_else(|| lang.text("авто", "auto").to_string());
    let _ = tray.status.set_text(if running {
        lang.text("Обработка включена", "Processing is on")
    } else {
        lang.text("Выключено", "Off")
    });
    let _ = tray.profile.set_text(lang.fmt(
        "Профиль: {name}",
        "Profile: {name}",
        &[("name", &profile)],
    ));
    let _ = tray.mute.set_checked(muted);
    if let Some(icon) = app.tray_by_id("main") {
        let text = if muted {
            lang.text(
                "Noise Canceller · микрофон выключен",
                "Noise Canceller · microphone muted",
            )
        } else if running {
            lang.text(
                "Noise Canceller · обработка включена",
                "Noise Canceller · processing is on",
            )
        } else {
            lang.text("Noise Canceller · выключено", "Noise Canceller · off")
        };
        let _ = icon.set_tooltip(Some(text));
    }
}

/// Renames the items that do not change while the app runs. The status and the
/// profile name are refreshed by `refresh_tray` on every menu opening, but
/// "Open" and "Quit" would keep the language they were created with.
pub(crate) fn relabel(app: &tauri::AppHandle, lang: Lang) {
    let Some(tray) = app.try_state::<TrayState>() else {
        return;
    };
    let _ = tray
        .mute
        .set_text(lang.text("Микрофон выключен", "Microphone muted"));
    let _ = tray.open.set_text(lang.text("Открыть", "Open"));
    let _ = tray.quit.set_text(lang.text("Выход", "Quit"));
    refresh_tray(app);
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
static EXITING: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_exiting() -> bool {
    EXITING.load(Ordering::SeqCst)
}

/// Quit from the tray: release the audio streams, save the window, and make sure
/// the process is gone. `AppHandle::exit` sends a request to the event loop, but
/// the tray menu runs a modal loop on Windows, so the request can sit unprocessed
/// and leave a windowless process behind (the tray icon stayed after "Выход").
pub(crate) fn quit_app(app: &tauri::AppHandle) {
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

pub(crate) fn save_window(app: &tauri::AppHandle) {
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

/// Creates the tray menu and the icon, arms the global shortcut and puts the
/// window where the user expects it. Called once from `setup`.
pub(crate) fn install(
    app: &mut tauri::App,
    start_hotkey: &str,
    lang: Lang,
    start_minimized: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem};

    let status = MenuItemBuilder::with_id("status", lang.text("Выключено", "Off"))
        .enabled(false)
        .build(app)?;
    let profile = MenuItemBuilder::with_id("profile", lang.text("Профиль: —", "Profile: —"))
        .enabled(false)
        .build(app)?;
    let mute =
        CheckMenuItemBuilder::with_id("mute", lang.text("Микрофон выключен", "Microphone muted"))
            .checked(false)
            .build(app)?;
    let open = MenuItemBuilder::with_id("open", lang.text("Открыть", "Open")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", lang.text("Выход", "Quit")).build(app)?;
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
        open: open.clone(),
        quit: quit.clone(),
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
                            let _ = state.menu.popup(tauri::Webview::window(webview.as_ref()));
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

    register_hotkey(app.handle(), start_hotkey, lang);
    // the window is created hidden: restore its size and center it while the
    // page is loading; `on_page_load` shows it after the first paint
    restore_window(app.handle());
    if !start_minimized {
        // safety net: a broken page must still be visible and closable
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if let Some(window) = handle.get_webview_window("main") {
                if !window.is_visible().unwrap_or(false) {
                    let _ = window.show();
                }
            }
        });
    }
    Ok(())
}
