// User presets: the list, save/delete and JSON export/import.
//
// Presets only carry the sound-shaping fields: devices, profile, theme and
// language are the machine's, not the preset's. The same key list is used by
// "apply this take's settings", so it lives here and is exported.

import { ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

import { t } from "../i18n";
import type { Preset, Settings, ToastKind } from "../types";

export const PRESET_KEYS: (keyof Settings)[] = [
  "isolationStrength",
  "maxSuppressionDb",
  "residualDenoise",
  "residualOrder",
  "residualMix",
  "vadGate",
  "highPassEnabled",
  "highPassHz",
  "toneEnabled",
  "toneLowDb",
  "toneHighDb",
  "deessEnabled",
  "deessDb",
  "airAmount",
  "gateEnabled",
  "gateThresholdDb",
  "gateRatio",
  "gateAttackMs",
  "gateReleaseMs",
  "gateHoldMs",
  "agcEnabled",
  "agcTargetDb",
  "agcMaxGainDb",
  "limiterEnabled",
  "comfortNoiseEnabled",
  "comfortNoiseDb",
  "outputGainDb",
  "loudnessTargetDb",
  "inputGainDb",
];

interface PresetDeps {
  settings: Ref<Settings | null>;
  error: Ref<string>;
  notify: (title: string, kind?: ToastKind, text?: string) => void;
  restartSoon: () => void;
  isBusy: () => boolean;
}

export function usePresets({ settings, error, notify, restartSoon, isBusy }: PresetDeps) {
  const customPresets = ref<Preset[]>([]);
  const presetName = ref("");
  const savingPreset = ref(false);

  async function loadPresets() {
    try {
      customPresets.value = await invoke<Preset[]>("list_presets");
    } catch {
      customPresets.value = [];
    }
  }

  function applyCustomPreset(preset: Preset) {
    if (!settings.value) return;
    if (isBusy()) {
      notify(t("toast.busyTitle"), "warn", t("toast.busyText"));
      return;
    }
    const previousResidual = settings.value.residualDenoise;
    for (const key of PRESET_KEYS) {
      (settings.value as unknown as Record<string, unknown>)[key] = (
        preset.settings as unknown as Record<string, unknown>
      )[key];
    }
    notify(t("toast.presetApplied"), "ok", preset.name);
    if (settings.value.residualDenoise !== previousResidual) restartSoon();
  }

  async function saveCustomPreset() {
    if (!settings.value || !presetName.value.trim()) return;
    savingPreset.value = true;
    try {
      await invoke<Preset>("save_preset", {
        name: presetName.value,
        settings: settings.value,
      });
      presetName.value = "";
      await loadPresets();
    } catch (e) {
      error.value = String(e);
    } finally {
      savingPreset.value = false;
    }
  }

  async function deleteCustomPreset(name: string) {
    try {
      await invoke("delete_preset", { name });
      await loadPresets();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function exportPresets() {
    error.value = "";
    try {
      const path = await save({
        defaultPath: "presets.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      await invoke("export_presets", { path });
      notify(t("toast.presetsExported"), "ok", path);
    } catch (e) {
      error.value = String(e);
    }
  }

  async function importPresets() {
    error.value = "";
    try {
      const picked = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!picked || Array.isArray(picked)) return;
      const count = await invoke<number>("import_presets", { path: picked });
      await loadPresets();
      notify(t("toast.presetsImported"), "ok", t("toast.presetsImportedText", { n: count }));
    } catch (e) {
      error.value = String(e);
    }
  }

  return {
    customPresets,
    presetName,
    savingPreset,
    loadPresets,
    applyCustomPreset,
    saveCustomPreset,
    deleteCustomPreset,
    exportPresets,
    importPresets,
  };
}
