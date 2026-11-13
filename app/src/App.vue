<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, provide, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import TitleBar from "./components/TitleBar.vue";
import AppSelect from "./components/AppSelect.vue";
import AppIcon from "./components/AppIcon.vue";
import AppToast from "./components/AppToast.vue";
import AppConfirm from "./components/AppConfirm.vue";
import { useToasts } from "./composables/useToasts";
import SoundView from "./views/SoundView.vue";
import SettingsView from "./views/SettingsView.vue";
import { APP_CONTEXT } from "./appContext";
import { isCableInput, isCableOutput } from "./cable";
import { useSettingsSearch } from "./composables/useSettingsSearch";
import { useSystemInfo } from "./composables/useSystemInfo";
import { useTheme, useHotkeyCapture } from "./composables/useShell";
import { useCable } from "./composables/useCable";
import { useProfiles } from "./composables/useProfiles";
import { shortName } from "./format";
import { currentLocale, setLocale, t, u } from "./i18n";
import type {
  AbEntry,
  AbMetrics,
  AbStatus,
  LastAb,
  Profile,
  Quality,
  Settings,
  SettingsResult,
  Stats,
} from "./types";

const { toasts, notify, dismissToast } = useToasts();
const settings = ref<Settings | null>(null);
const stats = ref<Stats>({
  inputRms: 0,
  outputRms: 0,
  rtf: 0,
  latencyMs: 0,
  bufferMs: 0,
  underruns: 0,
  inRate: 0,
  inChannels: 0,
  outRate: 0,
  overflows: 0,
  captureDrops: 0,
  backend: "",
  running: false,
  spectrum: Array(32).fill(0),
  inputPeakDb: -120,
  muted: false,
  guardResets: 0,
  levelerGainDb: 0,
  slewPpm: 0,
  threads: 0,
  healthy: false,
  suppressionDb: 0,
  speech: false,
});
const inputs = ref<string[]>([]);
const outputs = ref<string[]>([]);
/// capture channels of the selected microphone, for the channel picker
const inputChannels = ref(0);
const selectedInput = ref("");
const selectedOutput = ref("");
const busy = ref(false);
const starting = ref(false);
const error = ref("");
const {
  profiles,
  selectedProfile,
  profileQuality,
  profileScores,
  profileOptions,
  activeProfileScore,
  profileWeak,
  profileMeta,
  profileInfo,
  profileMismatch,
  refreshProfiles,
  pickProfile,
  deleteProfile,
} = useProfiles({
  settings,
  error,
  inputDevice: selectedInput,
  restart: async () => {
    if (!running.value) return;
    const test = testMode.value;
    await invoke("stop");
    await startEngine(test);
  },
});
const tab = ref<"sound" | "settings">("sound");

const recording = ref(false);
const probing = ref(false);
const probeLeft = ref(0);
const probeResult = ref<Quality | null>(null);
const recordLeft = ref(0);
const refreshing = ref(false);
const reconnecting = ref(false);
/// which preview is playing: "profile", "raw", "clean" — one at a time, and a
/// second click on the same button stops it
const playingKey = ref<string | null>(null);

const abRunning = ref(false);
const abProgress = ref(0);
const abFolder = ref<string | null>(null);
const abRaw = ref<string | null>(null);
const abClean = ref<string | null>(null);
const history = ref<AbEntry[]>([]);
const abNormalize = ref(true);
const abSide = ref<"raw" | "clean">("raw");
const showWizard = ref(false);
const resetDialog = ref(false);
const takeToApply = ref<AbEntry | null>(null);
const wizardEl = ref<HTMLElement | null>(null);
const abDelta = ref<number | null>(null);
const abMetrics = ref<AbMetrics | null>(null);
let slowStreak = 0;
let slowWarnAt = 0;
let quietMicWarned = false;

const cableOk = ref(true);

const modelStatusText = ref<string | null>(null);
/// The default text is a localized placeholder; a check replaces it with the
/// answer from Rust, which is already in the current language.
const modelStatus = computed(() => modelStatusText.value ?? t("model.status"));
const checkingModel = ref(false);
const autostart = ref(false);
const testMode = ref(false);

let timer: number | undefined;
let deviceTimer: number | undefined;
let healthRetryAt = 0;
let probeTimer: number | undefined;
let spectrumTimer: number | undefined;
let abMisses = 0;
const deviceLost = ref(false);
/// Whether processing *should* be running. The three-second device poll uses
/// this instead of `!running`, otherwise auto-start fought the off button and
/// started the engine again a moment after the user stopped it.
const engineWanted = ref(true);

/// With no audio flowing the spectrum is a still, dim baseline: it marks the
/// block as "waiting" instead of faking a signal.
const idleSpectrum = Array.from({ length: 32 }, () => 0.1);
/// bands captured from the microphone during a check or a profile take
const probeSpectrum = ref<number[]>(Array.from({ length: 32 }, () => 0));
const microphoneLive = computed(() => probing.value || recording.value);
const hasAudio = computed(() => stats.value.running);
const spectrumLive = computed(() => microphoneLive.value || hasAudio.value);
const shownSpectrum = computed(() => {
  if (microphoneLive.value) return probeSpectrum.value;
  return hasAudio.value ? stats.value.spectrum : idleSpectrum;
});
const deviceTest = ref(false);
let saveTimer: number | undefined;
let recordTimer: number | undefined;
let restartTimer: number | undefined;

// Actions are monochrome on purpose: colour in this window means "audio is
// flowing", so only the record dot and the signal states are allowed to use it.
const btn = "btn";
const btnPrimary = "btn btn-primary";
const btnGhost = "btn btn-ghost";
const settingRow = "srow";

const running = computed(() => stats.value.running);
const { theme, isDark, applyTheme, toggleTheme } = useTheme(settings);
const { capturingHotkey, startHotkeyCapture } = useHotkeyCapture(settings);
const { installingCable, cableMessage, installCable } = useCable({
  refreshDevices: () => refreshDevices(),
  cableOk,
  error,
});
const statusText = computed(() =>
  error.value
    ? t("status.error")
    : starting.value
      ? t("status.starting")
      : running.value
        ? t("status.working")
        : t("common.off"),
);
const dotClass = computed(() =>
  error.value
    ? "bg-[var(--danger)]"
    : running.value
      ? "bg-[var(--ok)]"
      : "bg-[var(--muted)]"
);
const inputLevel = computed(() => toLevel(stats.value.inputRms));
const outputLevel = computed(() => toLevel(stats.value.outputRms));
const inputDb = computed(() => (stats.value.inputRms > 1e-5 ? `${(20 * Math.log10(stats.value.inputRms)).toFixed(0)} ${u.db}` : "—"));
const outputDb = computed(() => (stats.value.outputRms > 1e-5 ? `${(20 * Math.log10(stats.value.outputRms)).toFixed(0)} ${u.db}` : "—"));


const presets = computed(() => [
  { id: "home", label: t("preset.home"), patch: { gateThresholdDb: -60, gateRatio: 2, gateReleaseMs: 300, agcTargetDb: -24, agcMaxGainDb: 12, highPassHz: 85, comfortNoiseEnabled: false } },
  { id: "office", label: t("preset.office"), patch: { gateThresholdDb: -52, gateRatio: 3, gateReleaseMs: 250, agcTargetDb: -22, highPassHz: 100, comfortNoiseEnabled: true, comfortNoiseDb: -64 } },
  { id: "street", label: t("preset.street"), patch: { gateThresholdDb: -46, gateRatio: 4, gateReleaseMs: 200, agcTargetDb: -20, highPassHz: 120, comfortNoiseEnabled: true, comfortNoiseDb: -60 } },
  { id: "stream", label: t("preset.stream"), patch: { gateThresholdDb: -58, gateRatio: 2.5, gateReleaseMs: 300, agcTargetDb: -24, highPassHz: 80, comfortNoiseEnabled: false, outputGainDb: -6 } },
]);

function applyPreset(patch: Record<string, unknown>) {
  if (!settings.value) return;
  Object.assign(settings.value, patch);
}

const inputOptions = computed(() => inputs.value.map((n) => ({ value: n, label: n })));
const outputOptions = computed(() => outputs.value.map((n) => ({ value: n, label: n })));
const previewOutput = computed(
  () => outputs.value.find((n) => !isCableOutput(n)) ?? outputs.value[0] ?? ""
);

// the device the user picked for listening; the cable is never a preview target
const testOutput = computed(() =>
  selectedOutput.value && !isCableOutput(selectedOutput.value) ? selectedOutput.value : previewOutput.value
);

function toLevel(rms: number): number {
  const db = 20 * Math.log10(rms + 1e-7);
  return Math.max(0, Math.min(1, (db + 60) / 60));
}

/// Channel count of the current microphone: the picker is hidden for a mono
/// device and offers real numbers only for a stereo interface.
async function refreshInputChannels() {
  try {
    inputChannels.value = await invoke<number>("input_channels", {
      name: selectedInput.value || null,
    });
  } catch {
    inputChannels.value = 0;
  }
}

async function refreshDevices() {
  try {
    const [ins, outs] = await invoke<[string[], string[]]>("list_devices");
    inputs.value = ins;
    outputs.value = outs;
    if (!selectedInput.value && ins.length) {
      // only real microphones; the cable's capture side is a feedback loop
      selectedInput.value = ins.find((n) => !isCableInput(n)) ?? "";
    }
    if (!selectedOutput.value && outs.length) {
      selectedOutput.value = outs.find((n) => /CABLE Input|VB-Audio/i.test(n)) ?? outs[0];
    }
    // a remembered device wins over the default pick, unless it is the cable
    if (
      settings.value?.inputDevice &&
      ins.includes(settings.value.inputDevice) &&
      !isCableInput(settings.value.inputDevice)
    ) {
      selectedInput.value = settings.value.inputDevice;
    }
    if (settings.value?.outputDevice && outs.includes(settings.value.outputDevice)) {
      selectedOutput.value = settings.value.outputDevice;
    }
    cableOk.value = outs.some((n) => isCableOutput(n));
    await refreshInputChannels();
  } catch (e) {
    error.value = String(e);
  }
}

async function refreshDevicesManually() {
  if (refreshing.value) return;
  refreshing.value = true;
  try {
    await refreshDevices();
    // keep the spinner readable even when the list comes back instantly
    await new Promise((resolve) => window.setTimeout(resolve, 400));
  } finally {
    refreshing.value = false;
  }
}

const inputIsCable = computed(() => !!selectedInput.value && isCableInput(selectedInput.value));

/// Recording a profile needs a real microphone: the cable's capture side is our
/// own output, so it cannot teach the model a voice.
const micReady = computed(() => !!selectedInput.value && !inputIsCable.value);

/// The three numbers on the console. Kept together so the strip reads as one
/// instrument row and an empty value always shows a dash, never "0".
const readouts = computed(() => {
  const latency = stats.value.latencyMs > 0 ? stats.value.latencyMs.toFixed(0) : "—";
  const load = stats.value.rtf > 0 ? (stats.value.rtf * 100).toFixed(0) : "—";
  const suppressed = stats.value.suppressionDb > 0.5;
  const cap = settings.value?.maxSuppressionDb ?? 0;
  return [
    { label: t("readout.latency"), value: latency, unit: latency === "—" ? "" : u.ms },
    { label: t("readout.load"), value: load, unit: load === "—" ? "" : "%" },
    {
      label: t("readout.suppression"),
      value: suppressed ? `−${stats.value.suppressionDb.toFixed(0)}` : "—",
      unit: suppressed ? (cap > 0 ? `${u.db} / ${cap.toFixed(0)}` : u.db) : "",
    },
  ];
});

/// Where the sound actually goes, spelled out next to the status dot.
const routeText = computed(() => {
  const input = shortName(selectedInput.value) || t("route.noMic");
  const output = testMode.value
    ? shortName(testOutput.value) || t("common.outputDevice")
    : "CABLE Input";
  return `${input} → ${output}`;
});

/// Score and SNR of the profile in use, shown next to the section title.
/// The score already sits in the picker label, so only the SNR is repeated here
/// unless the profile was recorded seconds ago.
const settingsSections = computed(() => [
  { id: "voice", label: t("section.voice") },
  { id: "clean", label: t("section.clean") },
  { id: "latency", label: t("section.latency") },
  { id: "control", label: t("section.control") },
  { id: "diag", label: t("section.diag") },
  { id: "app", label: t("section.app") },
]);
const settingsSection = ref("voice");
const scrollEl = ref<HTMLElement | null>(null);
const { settingsQuery, searching, filterSettings } = useSettingsSearch();

function switchSection(id: string) {
  settingsSection.value = id;
  filterSettings("");
  scrollEl.value?.scrollTo({ top: 0 });
}

const { sysInfo, maxThreads, autoThreads, slowConfig, loadSystemInfo } = useSystemInfo(settings);

async function matchLevel() {
  if (!abRaw.value) return;
  const match = /ab-(\d+)-raw/.exec(abRaw.value);
  if (!match || !settings.value) return;
  error.value = "";
  try {
    const delta = await invoke<number>("ab_level_delta", { stamp: Number(match[1]) });
    abDelta.value = delta;
    const next = Math.max(-18, Math.min(18, settings.value.outputGainDb - delta));
    settings.value.outputGainDb = Math.round(next * 2) / 2;
    notify(
      t("toast.levelMatched"),
      "ok",
      t("toast.levelMatchedText", {
        sign: delta >= 0 ? "−" : "+",
        db: Math.abs(delta).toFixed(1),
      }),
    );
  } catch (e) {
    error.value = String(e);
  }
}

async function startEngine(test: boolean) {
  if (starting.value) return;
  if (!selectedInput.value || isCableInput(selectedInput.value)) {
    testMode.value = false;
    error.value = selectedInput.value
      ? t("error.cableAsMic")
      : t("error.noMic");
    return;
  }
  starting.value = true;
  engineWanted.value = true;
  error.value = "";
  try {
    const voice = settings.value?.mode === "profile" ? settings.value?.profilePath ?? null : null;
    const wanted = test ? testOutput.value : selectedOutput.value;
    const output = wanted && outputs.value.includes(wanted) ? wanted : test ? previewOutput.value : "";
    const payload = {
      input: selectedInput.value || null,
      output: output || null,
      voice,
      testMode: test,
    };
    let result = await invoke<{ started: boolean }>("start", payload);
    if (!result.started) {
      // the engine was already up with the old devices or the other test mode:
      // the command cannot change them in place, so restart it once
      await invoke("stop");
      result = await invoke<{ started: boolean }>("start", payload);
      if (!result.started) throw new Error(t("error.engineRestart"));
    }
    testMode.value = test;
    stats.value = await invoke<Stats>("stats");
  } catch (e) {
    testMode.value = false;
    error.value = String(e);
  } finally {
    starting.value = false;
  }
}

async function stopEngine() {
  engineWanted.value = false;
  try {
    await invoke("stop");
  } catch (e) {
    error.value = String(e);
  }
  testMode.value = false;
  stats.value = await invoke<Stats>("stats");
}

async function togglePower() {
  if (busy.value || starting.value) return;
  busy.value = true;
  try {
    if (running.value) await stopEngine();
    else await startEngine(false);
  } finally {
    busy.value = false;
  }
}

async function toggleTest() {
  if (busy.value || starting.value) return;
  busy.value = true;
  try {
    if (testMode.value) await stopEngine();
    else {
      if (running.value) await stopEngine();
      await startEngine(true);
    }
  } finally {
    busy.value = false;
  }
}

function restartSoon(reconnect = false) {
  if (!running.value) {
    reconnecting.value = false;
    return;
  }
  if (restartTimer) window.clearTimeout(restartTimer);
  if (reconnect) reconnecting.value = true;
  restartTimer = window.setTimeout(async () => {
    const test = testMode.value;
    try {
      await invoke("stop");
      await startEngine(test);
    } catch (e) {
      error.value = String(e);
    } finally {
      reconnecting.value = false;
    }
  }, 800);
}

async function probeMic() {
  if (probing.value) return;
  probing.value = true;
  probeLeft.value = 3;
  probeResult.value = null;
  error.value = "";
  startSpectrumPolling();
  if (probeTimer) window.clearInterval(probeTimer);
  probeTimer = window.setInterval(() => {
    probeLeft.value = Math.max(0, probeLeft.value - 1);
  }, 1000);
  try {
    probeResult.value = await invoke<Quality>("probe_mic", {
      input: selectedInput.value || null,
      seconds: 3,
    });
  } catch (e) {
    error.value = String(e);
  } finally {
    if (probeTimer) window.clearInterval(probeTimer);
    probeTimer = undefined;
    probeLeft.value = 0;
    stopSpectrumPolling();
    probing.value = false;
  }
}

/// The take runs in Rust; these bands are what it hears right now, so the
/// spectrum moves while the user speaks instead of waiting for the result.
function startSpectrumPolling() {
  stopSpectrumPolling();
  spectrumTimer = window.setInterval(async () => {
    try {
      probeSpectrum.value = await invoke<number[]>("probe_spectrum");
    } catch {
      /* take finished */
    }
  }, 100);
}

function stopSpectrumPolling() {
  if (spectrumTimer) window.clearInterval(spectrumTimer);
  spectrumTimer = undefined;
  probeSpectrum.value = Array.from({ length: 32 }, () => 0);
}

const probeHint = computed(() => {
  const q = probeResult.value;
  if (!q) return "";
  if (q.clipPercent > 1) return t("probe.hint.clip");
  if (q.peakDb < -30) return t("probe.hint.quiet");
  if (q.speechPercent < 0.2) return t("probe.hint.noSpeech");
  return t("probe.hint.ok");
});

async function recordProfile() {
  if (recording.value || !settings.value || !micReady.value) return;
  await stopPreview();
  recording.value = true;
  error.value = "";
  recordLeft.value = 15;
  startSpectrumPolling();
  recordTimer = window.setInterval(() => {
    recordLeft.value = Math.max(0, recordLeft.value - 1);
  }, 1000);

  try {
    const profile = await invoke<Profile>("record_profile", {
      input: selectedInput.value || null,
      seconds: 15,
    });
    profileQuality.value = profile.quality;
    settings.value.profilePath = profile.path;
    settings.value.mode = "profile";
    await refreshProfiles();
    selectedProfile.value = profile.path;
    if (running.value) {
      const test = testMode.value;
      await invoke("stop");
      await startEngine(test);
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    recording.value = false;
    if (recordTimer) window.clearInterval(recordTimer);
    stopSpectrumPolling();
  }
}

async function stopPreview() {
  if (!playingKey.value) return;
  playingKey.value = null;
  try {
    await invoke("stop_playback");
  } catch {
    /* nothing playing */
  }
  await new Promise((resolve) => window.setTimeout(resolve, 120));
}

async function togglePlay(path: string | null, key: string) {
  if (!path) return;
  // a second click on the same button stops the preview
  if (playingKey.value === key) {
    await stopPreview();
    return;
  }
  // switch previews: stop the old one and give the device a moment
  await stopPreview();
  error.value = "";
  playingKey.value = key;
  try {
    await invoke("play_profile", {
      path,
      output: testOutput.value || null,
      normalize: abNormalize.value,
    });
  } catch (e) {
    error.value = String(e);
  } finally {
    if (playingKey.value === key) playingKey.value = null;
  }
}

function playFile(path: string | null, key: string) {
  return togglePlay(path, key);
}

/// The A/B pair plays as one stream: clicking either side switches it at the
/// same timestamp instead of restarting a separate file. The first click starts
/// the pair; the second click on the active side stops it.
async function playAb(side: "raw" | "clean") {
  if (!abRaw.value || !abClean.value) return;
  if (playingKey.value === "pair") {
    if (abSide.value === side) {
      await stopPreview();
      return;
    }
    abSide.value = side;
    try {
      await invoke("set_ab_side", { clean: side === "clean" });
    } catch (e) {
      error.value = String(e);
    }
    return;
  }
  await stopPreview();
  playingKey.value = "pair";
  abSide.value = side;
  error.value = "";
  try {
    void invoke("play_ab_pair", {
      raw: abRaw.value,
      clean: abClean.value,
      output: testOutput.value || null,
      normalize: abNormalize.value,
      initialClean: side === "clean",
    })
      .catch((e) => {
        error.value = String(e);
      })
      .finally(() => {
        if (playingKey.value === "pair") playingKey.value = null;
      });
  } catch (e) {
    playingKey.value = null;
    error.value = String(e);
  }
}

function playProfile() {
  if (!selectedProfile.value) return;
  return togglePlay(selectedProfile.value, "profile");
}

async function toggleAb() {
  if (abRunning.value) return;
  await stopPreview();
  error.value = "";
  try {
    await invoke("ab_start", { seconds: settings.value?.abSeconds ?? 30 });
    abRunning.value = true;
    abProgress.value = 0;
    abFolder.value = null;
    abRaw.value = null;
    abClean.value = null;
    pollAb();
  } catch (e) {
    error.value = String(e);
  }
}

async function pollAb() {
  if (!abRunning.value) return;
  try {
    const status = await invoke<AbStatus | null>("ab_status");
    if (status) {
      abMisses = 0;
      abProgress.value = status.progress;
      abFolder.value = status.folder;
      abRaw.value = status.raw;
      abClean.value = status.clean;
      if (!status.running) {
        abRunning.value = false;
        try {
          await invoke("prune_recordings");
        } catch {
          /* nothing to prune */
        }
        await loadHistory();
        return;
      }
    } else if (++abMisses >= 5) {
      // the engine went away mid-take: do not poll forever
      abRunning.value = false;
      error.value = t("error.abInterrupted");
      return;
    }
  } catch {
    if (++abMisses >= 5) {
      abRunning.value = false;
      error.value = t("error.abInterrupted");
      return;
    }
  }
  window.setTimeout(pollAb, 200);
}

function formatStamp(stamp: number): string {
  const date = new Date(stamp * 1000);
  return date.toLocaleString(currentLocale.value, {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

async function loadHistory() {
  try {
    history.value = await invoke<AbEntry[]>("list_recordings");
  } catch {
    history.value = [];
  }
}

async function loadAbMetrics() {
  abMetrics.value = null;
  abDelta.value = null;
  if (!abRaw.value) return;
  const match = /ab-(\d+)-raw/.exec(abRaw.value);
  if (!match) return;
  try {
    abMetrics.value = await invoke<AbMetrics>("ab_metrics", { stamp: Number(match[1]) });
  } catch {
    abMetrics.value = null;
  }
}

watch(abRaw, () => {
  void loadAbMetrics();
});

function selectRecording(entry: AbEntry) {
  abRaw.value = entry.raw;
  abClean.value = entry.clean;
  abFolder.value = entry.raw.replace(/[\\/][^\\/]+$/, "");
}

async function removeRecording(entry: AbEntry) {
  error.value = "";
  try {
    await invoke("delete_recording", { stamp: entry.stamp });
    if (abRaw.value === entry.raw) {
      abRaw.value = null;
      abClean.value = null;
    }
    await loadHistory();
  } catch (e) {
    error.value = String(e);
  }
}

function askApplyTake(entry: AbEntry) {
  takeToApply.value = entry;
}

async function applyTakeSettings() {
  const entry = takeToApply.value;
  takeToApply.value = null;
  if (!entry) return;
  error.value = "";
  try {
    const next = await invoke<Settings>("ab_settings", { stamp: entry.stamp });
    settings.value = next;
    applyTheme(next.theme);
    await refreshDevices();
    await refreshProfiles();
    restartSoon(true);
    notify(t("toast.takeApplied"), "ok", t("toast.takeAppliedText"));
  } catch (e) {
    error.value = String(e);
  }
}

function resetSettings() {
  resetDialog.value = true;
}

async function applyResetSettings() {
  resetDialog.value = false;
  error.value = "";
  try {
    const result = await invoke<SettingsResult>("reset_settings");
    settings.value = result.settings;
    applyTheme(settings.value.theme);
    await refreshDevices();
    await refreshProfiles();
    if (result.warning) notify(t("toast.hotkeyTitle"), "warn", result.warning);
    // some fields (device, profile, residual stage, block) only take effect on
    // a fresh engine; importing them without a restart left the old chain on
    restartSoon(true);
  } catch (e) {
    error.value = String(e);
  }
}

async function exportSettings() {
  error.value = "";
  try {
    const path = await save({
      defaultPath: "settings-export.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    const saved = await invoke<string>("export_settings", { path });
    notify(t("toast.settingsSaved"), "ok", saved);
  } catch (e) {
    error.value = String(e);
  }
}

async function importSettings() {
  error.value = "";
  try {
    const picked = await open({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!picked || Array.isArray(picked)) return;
    const result = await invoke<SettingsResult>("import_settings", { path: picked });
    settings.value = result.settings;
    applyTheme(result.settings.theme);
    await refreshDevices();
    await refreshProfiles();
    if (result.warning) notify(t("toast.hotkeyTitle"), "warn", result.warning);
    restartSoon(true);
  } catch (e) {
    error.value = String(e);
  }
}

async function openLog() {
  error.value = "";
  try {
    await invoke("open_log");
  } catch (e) {
    error.value = String(e);
  }
}

async function openLicenses() {
  error.value = "";
  try {
    await invoke("open_licenses");
  } catch (e) {
    error.value = String(e);
  }
}

async function setMuted(muted: boolean) {
  try {
    await invoke("set_muted", { muted });
    stats.value.muted = muted;
  } catch (e) {
    error.value = String(e);
  }
}

watch(showWizard, async (open) => {
  if (!open) return;
  await nextTick();
  wizardEl.value?.focus();
});

/// The wizard covers the window: Escape closes it and Tab cycles inside it,
/// otherwise focus walks onto the controls hidden behind the overlay.
function onWizardKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    closeWizard();
    return;
  }
  if (event.key !== "Tab" || !wizardEl.value) return;
  const items = Array.from(
    wizardEl.value.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((el) => el.offsetParent !== null);
  if (!items.length) return;
  const first = items[0];
  const last = items[items.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function closeWizard() {
  showWizard.value = false;
  if (settings.value) settings.value.onboarded = true;
}

async function checkModel() {
  checkingModel.value = true;
  try {
    modelStatusText.value = await invoke<string>("check_model_update");
  } catch (e) {
    modelStatusText.value = String(e);
  } finally {
    checkingModel.value = false;
  }
}

async function applyAutostart() {
  try {
    await invoke("set_autostart", { enabled: autostart.value });
  } catch (e) {
    error.value = String(e);
  }
}


// the two screens get the state and the actions through one object:
// the shell stays the only place that owns timers and engine calls
provide(APP_CONTEXT, {
  abClean,
  abDelta,
  abFolder,
  abMetrics,
  abNormalize,
  abProgress,
  abRaw,
  abRunning,
  abSide,
  applyAutostart,
  applyPreset,
  askApplyTake,
  autostart,
  autoThreads,
  btn,
  btnGhost,
  btnPrimary,
  busy,
  cableMessage,
  cableOk,
  capturingHotkey,
  checkingModel,
  checkModel,
  deleteProfile,
  error,
  exportSettings,
  filterSettings,
  formatStamp,
  history,
  importSettings,
  inputDb,
  inputChannels,
  inputIsCable,
  inputLevel,
  inputOptions,
  installCable,
  installingCable,
  matchLevel,
  maxThreads,
  micReady,
  modelStatus,
  openLicenses,
  openLog,
  outputDb,
  outputLevel,
  outputOptions,
  pickInput,
  pickOutput,
  pickProfile,
  playFile,
  playAb,
  playingKey,
  playProfile,
  presets,
  probeHint,
  probeLeft,
  probeMic,
  probeResult,
  probing,
  profileMeta,
  profileOptions,
  profileInfo,
  profileMismatch,
  profileWeak,
  readouts,
  reconnecting,
  recording,
  recordLeft,
  recordProfile,
  refreshDevicesManually,
  refreshing,
  refreshProfiles,
  removeRecording,
  resetSettings,
  restartSoon,
  running,
  selectedInput,
  selectedOutput,
  selectedProfile,
  selectRecording,
  searching,
  settingRow,
  settings,
  settingsQuery,
  settingsSection,
  settingsSections,
  shortName,
  shownSpectrum,
  showWizard,
  slowConfig,
  spectrumLive,
  startHotkeyCapture,
  starting,
  stats,
  switchSection,
  sysInfo,
  t,
  tab,
  testMode,
  testOutput,
  toggleAb,
  togglePower,
  toggleTest,
  u,
});

onMounted(async () => {
  try {
    settings.value = await invoke<Settings>("get_settings");
    setLocale(settings.value.language);
    applyTheme(settings.value.theme);
    if (!settings.value.onboarded) showWizard.value = true;
    await loadSystemInfo();
    await refreshDevices();
    await refreshProfiles();
    stats.value = await invoke<Stats>("stats");
    autostart.value = await invoke<boolean>("autostart_enabled");
    // bring back the play buttons for the last take: the paths live in the
    // engine, which is new after every restart
    const last = await invoke<LastAb | null>("last_ab");
    if (last) {
      abFolder.value = last.folder;
      abRaw.value = last.raw;
      abClean.value = last.clean;
    }
    await loadHistory();
  } catch (e) {
    error.value = String(e);
  }

  if (profileWeak.value) {
    notify(t("toast.profileWeakTitle"), "warn", t("toast.profileWeakText"));
  }
  if (settings.value?.autoStart && cableOk.value) {
    await startEngine(false);
  }


  timer = window.setInterval(async () => {
    try {
      stats.value = await invoke<Stats>("stats");
      // the processor thread can die on a model panic: the streams stay alive
      // but nothing comes out, so restart the engine and say why
      if (stats.value.running && !stats.value.healthy && Date.now() > healthRetryAt) {
        healthRetryAt = Date.now() + 4000;
        error.value = t("error.processingStopped");
        await stopEngine();
        await startEngine(testMode.value);
        // `startEngine` reports a failure by leaving `running` false and writing
        // `error`; clearing the error unconditionally hid a failed restart
        if (running.value) {
          error.value = "";
          notify(t("toast.engineRestarted"), "ok", t("toast.engineRestartedText"));
        } else {
          notify(
            t("toast.engineFailed"),
            "warn",
            error.value.replace(t("error.processingStopped"), "").trim() ||
              t("toast.engineFailedText"),
          );
        }
      }
      // the model is slower than the room: it cannot keep up in real time and
      // the ring starts dropping blocks. Say it once, not every second.
      if (stats.value.running && stats.value.rtf > 0.95) {
        slowStreak += 1;
        if (slowStreak >= 20 && Date.now() > slowWarnAt) {
          slowWarnAt = Date.now() + 60_000;
          notify(
            t("toast.slowTitle"),
            "warn",
            t("toast.slowText", { pct: (stats.value.rtf * 100).toFixed(0) }),
          );
        }
      } else {
        slowStreak = 0;
      }
      // the leveler is at its ceiling: the microphone itself is ~18 dB too
      // quiet, and the boost lifts the room noise along with the voice
      if (stats.value.running && stats.value.levelerGainDb > 16 && !quietMicWarned) {
        quietMicWarned = true;
        notify(
          t("toast.quietTitle"),
          "warn",
          t("toast.quietText"),
        );
      }
    } catch {
      /* not running */
    }
  }, 150);

  // a USB microphone can drop off the bus for a second; stop cleanly and come
  // back as soon as it is listed again
  deviceTimer = window.setInterval(async () => {
    // the window is hidden in the tray: do not enumerate devices every 3 s
    if (document.hidden) return;
    try {
      await refreshDevices();
      const inputGone = !!selectedInput.value && !inputs.value.includes(selectedInput.value);
      const outputGone =
        testMode.value && !!selectedOutput.value && !outputs.value.includes(selectedOutput.value);

      if (running.value && (inputGone || outputGone)) {
        deviceTest.value = testMode.value;
        await stopEngine();
        deviceLost.value = true;
        error.value = t("error.deviceLost");
        notify(t("toast.micGoneTitle"), "warn", t("toast.micGoneText"));
        return;
      }

      if (deviceLost.value && !inputGone && !outputGone) {
        deviceLost.value = false;
        error.value = "";
        notify(t("toast.micBack"), "ok");
        await startEngine(deviceTest.value);
      }

      if (settings.value?.followDefaultInput && !reconnecting.value && !starting.value) {
        try {
          const [defaultInput] = await invoke<[string | null, string | null]>("default_devices");
          if (
            defaultInput &&
            !isCableInput(defaultInput) &&
            defaultInput !== selectedInput.value &&
            inputs.value.includes(defaultInput)
          ) {
            pickInput(defaultInput);
          }
        } catch {
          /* enumeration hiccup */
        }
      }

      if (
        settings.value?.autoStart &&
        cableOk.value &&
        engineWanted.value &&
        !running.value &&
        !starting.value &&
        !testMode.value &&
        !deviceLost.value
      ) {
        await startEngine(false);
      }
    } catch {
      /* ignore a transient enumeration error */
    }
  }, 3000);
});

onUnmounted(() => {
  if (timer) window.clearInterval(timer);
  if (deviceTimer) window.clearInterval(deviceTimer);
  if (probeTimer) window.clearInterval(probeTimer);
  if (spectrumTimer) window.clearInterval(spectrumTimer);
  if (saveTimer) window.clearTimeout(saveTimer);
  if (recordTimer) window.clearInterval(recordTimer);
  if (restartTimer) window.clearTimeout(restartTimer);
});

watch(
  settings,
  (value) => {
    if (!value) return;
    if (saveTimer) window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(async () => {
      try {
        await invoke("set_settings", { settings: value });
      } catch (e) {
        error.value = String(e);
      }
    }, 300);
  },
  { deep: true }
);

watch(
  () => settings.value?.language,
  (language) => {
    setLocale(language);
    // the settings search walks the rendered DOM: after a language switch the
    // old query would keep rows hidden until the next keystroke
    if (searching.value) void filterSettings(settingsQuery.value);
  },
);

watch(
  () => settings.value?.theme,
  (value) => {
    if (value) applyTheme(value);
  },
);

watch(
  () => [settings.value?.chunkMs, settings.value?.denoiserThreads, settings.value?.autoThreads],
  () => {
    void loadSystemInfo();
    restartSoon();
  }
);

watch([selectedInput, selectedOutput], ([input], [prevInput]) => {
  if (!settings.value) return;
  // only restart, never persist: the device list fills up during startup and
  // must not overwrite the choice saved in settings
  if (testMode.value || input !== prevInput) restartSoon();
});

function pickInput(value: string) {
  if (value === selectedInput.value) return;
  selectedInput.value = value;
  saveDevices(true);
  void refreshInputChannels();
}

function pickOutput(value: string) {
  if (value === selectedOutput.value) return;
  selectedOutput.value = value;
  saveDevices(true);
}

function saveDevices(reconnect = false) {  if (!settings.value) return;
  if (selectedInput.value && inputs.value.includes(selectedInput.value)) {
    settings.value.inputDevice = selectedInput.value;
  }
  if (selectedOutput.value && outputs.value.includes(selectedOutput.value)) {
    settings.value.outputDevice = selectedOutput.value;
  }
  restartSoon(reconnect);
}
</script>

<template>
  <div class="flex h-full flex-col">
    <TitleBar>
      <template #status>
        <span class="h-1.5 w-1.5 shrink-0 rounded-full" :class="dotClass"></span>
        <span class="shrink-0 text-[12.5px] font-semibold">{{ statusText }}</span>
        <span class="min-w-0 truncate text-[12px] text-[var(--muted)]">· {{ routeText }}</span>
        <button
          v-if="stats.muted"
          class="ml-1 flex h-5 shrink-0 cursor-pointer items-center gap-1.5 rounded-md bg-[var(--danger-soft)] px-1.5 text-[10.5px] text-[var(--danger)]"
          :title="t('nav.unmute')"
          @mousedown.stop
          @click.stop="setMuted(false)"
        >
          <AppIcon name="mic" :size="11" />
          {{ t("nav.muted") }}
        </button>
      </template>

      <template #nav>
        <button class="tab" :data-active="tab === 'sound'" @mousedown.stop @click="tab = 'sound'">
          {{ t("nav.sound") }}
        </button>
        <button class="tab" :data-active="tab === 'settings'" @mousedown.stop @click="tab = 'settings'">
          {{ t("nav.settings") }}
        </button>
      </template>

      <template #actions>
        <button
          class="winbtn"
          :title="isDark ? t('nav.lightTheme') : t('nav.darkTheme')"
          @mousedown.stop
          @click.stop="toggleTheme"
        >
          <AppIcon :name="isDark ? 'sun' : 'moon'" :size="15" :stroke="1.7" />
        </button>
      </template>
    </TitleBar>

    <div ref="scrollEl" class="app-scroll mx-auto w-full max-w-[940px] flex-1 overflow-y-auto px-7 pb-7">
      <!-- ---------------- sound ---------------- -->
      <SoundView v-if="tab === 'sound' && settings" />

      <!-- ---------------- settings ---------------- -->
      <SettingsView v-else-if="settings" />
    </div>

    <div class="pointer-events-none fixed right-4 bottom-4 z-[95] flex w-[330px] max-w-[calc(100vw-32px)] flex-col gap-2">
      <!-- an error stays until dismissed: it is a card, not a toast, but it lives
           in the same fixed column so it never moves the page -->
      <AppToast v-if="error" kind="warn" :title="t('status.error')" :text="error" @close="error = ''" />
      <transition-group name="toast" tag="div" class="relative flex flex-col gap-2">
        <div v-for="toast in toasts" :key="toast.id" class="relative">
          <AppToast
            :kind="toast.kind"
            :title="toast.title"
            :text="toast.text"
            @close="dismissToast(toast.id)"
          />
        </div>
      </transition-group>
    </div>

    <AppConfirm
      v-if="resetDialog"
      :title="t('confirm.resetTitle')"
      :text="t('confirm.resetText')"
      :confirm-label="t('confirm.reset')"
      @confirm="applyResetSettings"
      @cancel="resetDialog = false"
    />

    <AppConfirm
      v-if="takeToApply"
      :title="t('confirm.takeTitle')"
      :text="t('confirm.takeText', { stamp: formatStamp(takeToApply.stamp) })"
      :confirm-label="t('confirm.takeApply')"
      @confirm="applyTakeSettings"
      @cancel="takeToApply = null"
    />

    <!-- first-run wizard -->
    <div v-if="showWizard && settings" class="fixed inset-0 z-[90] grid place-items-center bg-black/45 p-5">
      <div
        ref="wizardEl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="wizard-title"
        tabindex="-1"
        class="w-full max-w-[540px] rounded-[16px] border border-[var(--stroke)] bg-[var(--surface)] p-6 shadow-[0_30px_70px_-30px_rgba(0,0,0,0.7)]"
        @keydown="onWizardKeydown"
      >
        <div class="flex items-start justify-between gap-3">
          <div>
            <h2 id="wizard-title" class="text-[17px] font-semibold tracking-[-0.01em]">{{ t("wizard.title") }}</h2>
            <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted)]">
              {{ t("wizard.subtitle") }}
            </p>
          </div>
          <button class="iconbtn" :title="t('common.close')" @click="closeWizard">
            <AppIcon name="close" :size="14" />
          </button>
        </div>

        <div class="mt-5 flex flex-col">
          <div class="flex gap-4 border-t border-[var(--stroke)] py-4">
            <span class="mt-0.5 font-mono text-[11px] text-[var(--muted)]">1</span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">{{ t("wizard.step1") }}</div>
              <p class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">
                {{
                  cableOk
                    ? t("wizard.cableOk")
                    : t("wizard.cableMissing")
                }}
              </p>
              <button
                v-if="!cableOk"
                :class="btnPrimary"
                class="mt-2"
                :disabled="installingCable"
                @click="installCable"
              >
                <AppIcon v-if="installingCable" name="spinner" :size="14" class="spin" />
                {{ installingCable ? t("wizard.installing") : t("wizard.install") }}
              </button>
              <p v-if="cableMessage" class="mt-1 text-[11.5px] text-[var(--muted)]">{{ cableMessage }}</p>
            </div>
          </div>

          <div class="flex gap-4 border-t border-[var(--stroke)] py-4">
            <span class="mt-0.5 font-mono text-[11px] text-[var(--muted)]">2</span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">{{ t("wizard.step2") }}</div>
              <div class="mt-2">
                <AppSelect
                  :model-value="selectedInput"
                  :options="inputOptions"
                  :placeholder="t('sound.noDevices')"
                  @update:model-value="pickInput"
                />
              </div>
              <button :class="btn" class="mt-2" :disabled="probing" @click="probeMic">
                <AppIcon name="mic" :size="14" />
                {{
                  probing
                    ? probeLeft > 0
                      ? t("wizard.listening", { s: probeLeft })
                      : t("wizard.listen")
                    : t("wizard.probe")
                }}
              </button>
              <p v-if="probeResult" class="mt-1 text-[11.5px] text-[var(--muted)]">
                {{ t("wizard.level", { db: probeResult.peakDb.toFixed(0), hint: probeHint }) }}
              </p>
            </div>
          </div>

          <div class="flex gap-4 border-t border-[var(--stroke)] py-4">
            <span class="mt-0.5 font-mono text-[11px] text-[var(--muted)]">3</span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">{{ t("wizard.step3") }}</div>
              <p class="mt-1 text-[11.5px] leading-relaxed text-[var(--muted)]">
                {{ t("wizard.profileHint") }}
              </p>
              <button
                :class="btnPrimary"
                class="mt-2"
                :disabled="recording || !micReady"
                :title="micReady ? t('wizard.recordTitle') : t('wizard.recordNeedMic')"
                @click="recordProfile"
              >
                <span v-if="recording" class="h-2 w-2 shrink-0 rounded-full bg-[var(--danger)]"></span>
                <AppIcon v-else name="mic" :size="14" />
                {{ recording ? t("wizard.recording", { s: recordLeft }) : t("wizard.record") }}
              </button>
              <p v-if="profileQuality" class="mt-1 text-[11.5px] text-[var(--muted)]">
                {{ t("wizard.quality", { score: profileQuality.score, snr: profileQuality.snrDb.toFixed(0) }) }}
              </p>
            </div>
          </div>
        </div>

        <div class="mt-5 flex items-center justify-end gap-2">
          <button :class="btnGhost" @click="closeWizard">{{ t("wizard.skip") }}</button>
          <button :class="btnPrimary" @click="closeWizard">{{ t("wizard.done") }}</button>
        </div>
      </div>
    </div>
  </div>
</template>
