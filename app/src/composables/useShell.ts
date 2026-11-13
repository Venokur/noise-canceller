// Theme switching and the global mute shortcut capture.
//
// Both used to live in App.vue together with the engine state; they only need
// the settings ref, so they are the safe part of the split to pull out first.

import { computed, onMounted, onUnmounted, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

import { t } from "../i18n";
import type { Settings } from "../types";

export function useTheme(settings: Ref<Settings | null>) {
  const theme = ref("dark");
  const isDark = computed(() => theme.value === "dark");
  const media = window.matchMedia("(prefers-color-scheme: dark)");

  function applyTheme(value: string) {
    const systemDark = media.matches;
    theme.value = value === "system" ? (systemDark ? "dark" : "light") : value;
    document.documentElement.dataset.theme = theme.value;
  }

  function onSystemTheme() {
    if (settings.value?.theme === "system") applyTheme("system");
  }

  function toggleTheme() {
    const next = isDark.value ? "light" : "dark";
    if (settings.value) settings.value.theme = next;
    applyTheme(next);
  }

  media.addEventListener("change", onSystemTheme);
  onUnmounted(() => media.removeEventListener("change", onSystemTheme));

  return { theme, isDark, applyTheme, toggleTheme };
}

/// Codes the global-shortcut parser understands as a key. `event.code` is used
/// instead of `event.key` so the chord does not depend on the layout, and the
/// list is deliberately narrow: a punctuation key used to produce strings like
/// `Ctrl+Semicolon` that `Shortcut::from_str` rejected only after the setting
/// had already been saved.
const KEY_CODE = /^(?:Key[A-Z]|Digit[0-9]|F(?:[1-9]|1[0-9]|2[0-4])|Space|Enter|Escape|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Arrow(?:Up|Down|Left|Right|Home|End))$/;

/// Captures the next chord for the global mute shortcut. Registered on the
/// capture phase so the keys never reach a focused input or a control.
export function useHotkeyCapture(settings: Ref<Settings | null>) {
  const capturingHotkey = ref(false);
  const hotkeyError = ref("");
  let validating = false;

  function startHotkeyCapture() {
    if (capturingHotkey.value) {
      cancelHotkeyCapture();
      return;
    }
    hotkeyError.value = "";
    capturingHotkey.value = true;
  }

  function cancelHotkeyCapture() {
    capturingHotkey.value = false;
    validating = false;
  }

  /// A click anywhere else abandons the capture instead of swallowing the next
  /// keystroke (the old version kept the listener armed after a click on the
  /// page, so typing in the search box silently became the new shortcut).
  function onPointerDown(event: MouseEvent) {
    if (!capturingHotkey.value) return;
    if ((event.target as HTMLElement | null)?.closest("[data-hotkey-capture]")) return;
    cancelHotkeyCapture();
  }

  function onBlur() {
    if (capturingHotkey.value) cancelHotkeyCapture();
  }

  async function onHotkeyKey(event: KeyboardEvent) {
    if (!capturingHotkey.value) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      cancelHotkeyCapture();
      return;
    }
    if (validating) return;
    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push("Ctrl");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");
    if (event.metaKey) modifiers.push("Super");
    const modifierCodes = [
      "ControlLeft",
      "ControlRight",
      "AltLeft",
      "AltRight",
      "ShiftLeft",
      "ShiftRight",
      "MetaLeft",
      "MetaRight",
    ];
    if (modifierCodes.includes(event.code) || modifiers.length === 0) return;
    if (!KEY_CODE.test(event.code)) {
      hotkeyError.value = t("settings.control.badKey");
      return;
    }
    const key = event.code.startsWith("Key")
      ? event.code.slice(3)
      : event.code.startsWith("Digit")
        ? event.code.slice(5)
        : event.code;
    const hotkey = [...modifiers, key].join("+");
    validating = true;
    try {
      // the Rust parser is the authority: if it cannot parse the chord, do not
      // write it into settings and pretend the shortcut works
      await invoke("validate_hotkey", { hotkey });
      if (settings.value) settings.value.muteHotkey = hotkey;
      hotkeyError.value = "";
      capturingHotkey.value = false;
    } catch {
      hotkeyError.value = t("settings.control.badKey");
    } finally {
      validating = false;
    }
  }

  onMounted(() => {
    window.addEventListener("keydown", onHotkeyKey, true);
    window.addEventListener("mousedown", onPointerDown, true);
    window.addEventListener("blur", onBlur);
  });
  onUnmounted(() => {
    window.removeEventListener("keydown", onHotkeyKey, true);
    window.removeEventListener("mousedown", onPointerDown, true);
    window.removeEventListener("blur", onBlur);
  });

  return {
    capturingHotkey,
    hotkeyError,
    startHotkeyCapture,
    cancelHotkeyCapture,
  };
}
