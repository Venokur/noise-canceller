//! The two interface languages.
//!
//! Most of the window is translated on the frontend (`app/src/i18n`), but Rust
//! still speaks to the user where the text is created here: the tray menu, the
//! engine's device errors, the model update check. The language lives in
//! `Settings` and is resolved once from Windows, so every message is chosen by a
//! `Lang` value instead of a global — `nc` and the tests stay deterministic.
//!
//! New messages go through `text`/`fmt` with both variants next to each other;
//! that keeps a missing translation visible in review instead of shipping as a
//! Russian line in an English window.

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Ru,
    En,
}

impl Lang {
    pub fn from_settings(settings: &Settings) -> Self {
        // "auto" survives only in `Settings::default()` — the CLI and the tests
        // build those without going through `sanitize` — so resolve it here too
        // instead of silently falling back to Russian
        match settings.language.as_str() {
            "auto" => Self::from_code(system_language()),
            code => Self::from_code(code),
        }
    }

    /// Anything but "en" is Russian: `sanitize` has already resolved "auto", and
    /// a hand-edited file must not leave the UI without a language.
    pub fn from_code(code: &str) -> Self {
        if code == "en" {
            Self::En
        } else {
            Self::Ru
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Ru => "ru",
            Self::En => "en",
        }
    }

    pub fn text<'a>(self, ru: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Ru => ru,
            Self::En => en,
        }
    }

    /// Picks the message and fills its `{name}` placeholders, so a caller does
    /// not have to keep two `format!` strings in sync.
    pub fn fmt(self, ru: &str, en: &str, vars: &[(&str, &str)]) -> String {
        let mut message = self.text(ru, en).to_string();
        for (name, value) in vars {
            message = message.replace(&format!("{{{name}}}"), value);
        }
        message
    }
}

/// The interface language Windows is set to: Russian for a Russian UI, English
/// for everything else. Used once, when a settings file has no language yet, so
/// a fresh install already speaks the language of the machine.
pub fn system_language() -> &'static str {
    const LANG_RUSSIAN: u16 = 0x19;
    // the primary language id is the low 10 bits of the LANGID
    if windows_ui_language() & 0x03ff == LANG_RUSSIAN {
        "ru"
    } else {
        "en"
    }
}

#[cfg(windows)]
fn windows_ui_language() -> u16 {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // SAFETY: no arguments, no preconditions, always available on Windows.
    unsafe { GetUserDefaultUILanguage() }
}

#[cfg(not(windows))]
fn windows_ui_language() -> u16 {
    0x19
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_codes_fall_back_to_russian() {
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("ru"), Lang::Ru);
        assert_eq!(Lang::from_code("auto"), Lang::Ru);
        assert_eq!(Lang::from_code(""), Lang::Ru);
    }

    #[test]
    fn fmt_fills_every_placeholder() {
        let message = Lang::En.fmt("{a} и {b}", "{a} and {b}", &[("a", "1"), ("b", "2")]);
        assert_eq!(message, "1 and 2");
        assert_eq!(Lang::Ru.fmt("{a} и {b}", "{a} and {b}", &[]), "{a} и {b}");
    }
}
