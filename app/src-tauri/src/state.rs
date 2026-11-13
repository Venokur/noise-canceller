//! Everything the window and the tray share, behind two mutexes.
//!
//! The engine is taken out of the slot before it is joined (`stop`) and put back
//! only after a start finished, so a 2-second model load never blocks the 150 ms
//! `stats` poll.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use noise_canceller_core::engine::Engine;
use noise_canceller_core::settings::Settings;

pub(crate) struct AppState {
    pub(crate) engine: Mutex<Option<Engine>>,
    pub(crate) settings: Mutex<Settings>,
    pub(crate) settings_path: PathBuf,
    /// Serializes the whole apply step (save, memory, engine, hotkey). The
    /// command runs on a worker thread now, and two overlapping slider saves
    /// could otherwise reach the file and the engine out of order.
    pub(crate) apply_settings: Mutex<()>,
    pub(crate) maximized: AtomicBool,
    /// Whether processing should be running. Shared by the window's auto-start
    /// poll and the tray: stopping from the tray must not be undone three
    /// seconds later by the window.
    pub(crate) wanted: AtomicBool,
    /// where to put the window back when it leaves the maximized state
    pub(crate) restore_rect: Mutex<Option<(i32, i32, u32, u32)>>,
}

impl AppState {
    /// Runs `f` on the running engine without leaking the lock into the caller.
    ///
    /// There used to be eleven copies of `lock().ok().and_then(|g| g.as_ref()…)`
    /// spread across the commands and the tray; the nested `if let`s were where
    /// a wrong error path could silently do nothing.
    pub(crate) fn with_engine<R>(&self, f: impl FnOnce(&Engine) -> R) -> Option<R> {
        self.engine
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(f))
    }

    /// A copy of the settings, with the shipped defaults if the lock is poisoned.
    pub(crate) fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
}
