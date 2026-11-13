//! The Tauri builder: plugins, first paint, tray, window events and the command
//! list. Kept apart from `main` so the entry point stays a one-liner.

use noise_canceller_core::lang::Lang;
use noise_canceller_core::paths;
use noise_canceller_core::recordings;
use noise_canceller_core::settings::Settings;
use tauri::Manager;

use crate::state::AppState;

pub(crate) fn run() {
    let settings_path = paths::data_dir().join("settings.json");
    let settings = Settings::load(&settings_path);
    // persist a migrated file once, so the next start reads the tuned values
    settings.save(&settings_path);
    let start_minimized = settings.start_minimized;
    let start_hotkey = settings.mute_hotkey.clone();
    // the tray is created before the window: its labels need the language now,
    // not after the frontend has loaded
    let lang = Lang::from_settings(&settings);
    noise_canceller_core::log::info("app start");
    let pruned = recordings::prune(settings.keep_recordings as usize);
    if pruned > 0 {
        noise_canceller_core::log::info(&format!("removed {pruned} old A/B files"));
    }

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    crate::tray::handle_hotkey(app, shortcut, event);
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
        // WebView2 can show a white frame if the window becomes visible before
        // the page has painted. Keep it hidden until the first page has loaded,
        // then show it and nudge the size once so the compositor commits.
        .on_page_load({
            let show = !start_minimized;
            move |webview, payload| {
                if !show
                    || webview.label() != "main"
                    || !matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                {
                    return;
                }
                let window = webview.window();
                let _ = window.show();
                let _ = window.set_focus();
                if let Ok(size) = window.inner_size() {
                    let _ = window.set_size(tauri::PhysicalSize::new(size.width + 1, size.height));
                    let _ = window.set_size(tauri::PhysicalSize::new(size.width, size.height));
                }
            }
        })
        .setup(move |app| {
            crate::tray::install(app, &start_hotkey, lang, start_minimized)?;
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => crate::tray::show_main(app),
            "quit" => crate::tray::quit_app(app),
            "mute" => crate::tray::toggle_mute(app),
            _ => {}
        })
        .manage(AppState {
            engine: std::sync::Mutex::new(None),
            settings: std::sync::Mutex::new(settings),
            settings_path,
            maximized: std::sync::atomic::AtomicBool::new(false),
            restore_rect: std::sync::Mutex::new(None),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if crate::tray::is_exiting() {
                    return;
                }
                crate::tray::save_window(window.app_handle());
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
            crate::commands::list_devices,
            crate::commands::start,
            crate::commands::stop,
            crate::commands::stats,
            crate::commands::get_settings,
            crate::commands::set_settings,
            crate::commands::record_profile,
            crate::commands::probe_mic,
            crate::commands::probe_spectrum,
            crate::commands::list_profiles,
            crate::commands::delete_profile,
            crate::commands::ab_start,
            crate::commands::ab_status,
            crate::commands::last_ab,
            crate::commands::default_devices,
            crate::commands::system_info,
            crate::commands::profile_scores,
            crate::commands::ab_level_delta,
            crate::commands::ab_metrics,
            crate::commands::prune_recordings,
            crate::commands::set_muted,
            crate::commands::open_log,
            crate::commands::open_licenses,
            crate::commands::reset_settings,
            crate::commands::export_settings,
            crate::commands::import_settings,
            crate::commands::list_recordings,
            crate::commands::delete_recording,
            crate::commands::cable_present,
            crate::commands::install_cable,
            crate::commands::check_model_update,
            crate::commands::play_profile,
            crate::commands::stop_playback,
            crate::commands::set_autostart,
            crate::commands::autostart_enabled,
            crate::commands::window_minimize,
            crate::commands::window_toggle_maximize,
            crate::commands::window_close,
            crate::commands::window_drag
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
