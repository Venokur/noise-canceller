// Theme switching and the global mute shortcut capture.
//
// Both used to live in App.vue together with the engine state; they only need
// the settings ref, so they are the safe part of the split to pull out first.

import { computed, onMounted, onUnmounted, ref, type Ref } from "vue";

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

/// Captures the next chord for the global mute shortcut. Registered on the
/// capture phase so the keys never reach a focused input or a control.
export function useHotkeyCapture(settings: Ref<Settings | null>) {
  const capturingHotkey = ref(false);

  function startHotkeyCapture() {
    capturingHotkey.value = true;
  }

  function onHotkeyKey(event: KeyboardEvent) {
    if (!capturingHotkey.value) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      capturingHotkey.value = false;
      return;
    }
    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push("Ctrl");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");
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
    const key = event.code.startsWith("Key")
      ? event.code.slice(3)
      : event.code.startsWith("Digit")
        ? event.code.slice(5)
        : event.code;
    if (settings.value) {
      settings.value.muteHotkey = [...modifiers, key].join("+");
    }
    capturingHotkey.value = false;
  }

  onMounted(() => window.addEventListener("keydown", onHotkeyKey, true));
  onUnmounted(() => window.removeEventListener("keydown", onHotkeyKey, true));

  return { capturingHotkey, startHotkeyCapture };
}
