<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import TitleBar from "./components/TitleBar.vue";
import AppSwitch from "./components/AppSwitch.vue";
import AppSlider from "./components/AppSlider.vue";
import AppSelect from "./components/AppSelect.vue";
import AppLabel from "./components/AppLabel.vue";
import AppIcon from "./components/AppIcon.vue";
import AppToast from "./components/AppToast.vue";

interface Settings {
  enabled: boolean;
  theme: string;
  mode: string;
  profilePath: string | null;
  isolationStrength: number;
  maxSuppressionDb: number;
  inputDevice: string;
  outputDevice: string;
  inputLeveling: boolean;
  residualDenoise: string;
  residualAttenuationDb: number;
  vadGate: boolean;
  voiceIsolation: boolean;
  highPassEnabled: boolean;
  highPassHz: number;
  gateEnabled: boolean;
  gateThresholdDb: number;
  gateRatio: number;
  gateAttackMs: number;
  gateReleaseMs: number;
  gateHoldMs: number;
  agcEnabled: boolean;
  agcTargetDb: number;
  agcMaxGainDb: number;
  limiterEnabled: boolean;
  comfortNoiseEnabled: boolean;
  comfortNoiseDb: number;
  outputGainDb: number;
  closeToTray: boolean;
  bufferMs: number;
  bufferAuto: boolean;
  chunkMs: number;
  autoThreads: boolean;
  denoiserThreads: number;
  startMinimized: boolean;
  abSeconds: number;
  onboarded: boolean;
  autoStart: boolean;
  followDefaultInput: boolean;
  autoProfile: boolean;
  muteHotkey: string;
  hotkeyMode: string;
  basicUi: boolean;
  keepRecordings: number;
}

interface Stats {
  inputRms: number;
  outputRms: number;
  rtf: number;
  latencyMs: number;
  bufferMs: number;
  underruns: number;
  outRate: number;
  overflows: number;
  captureDrops: number;
  backend: string;
  running: boolean;
  spectrum: number[];
  suppressionDb: number;
  speech: boolean;
  inputPeakDb: number;
  muted: boolean;
  guardResets: number;
  levelerGainDb: number;
  slewPpm: number;
  threads: number;
  healthy: boolean;
}

interface Quality {
  duration: number;
  speechPercent: number;
  snrDb: number;
  peakDb: number;
  clipPercent: number;
  score: number;
}

interface Profile {
  path: string;
  quality: Quality;
}

interface AbStatus {
  running: boolean;
  progress: number;
  folder: string | null;
  raw: string | null;
  clean: string | null;
}

interface LastAb {
  folder: string;
  raw: string;
  clean: string;
}

interface AbEntry {
  stamp: number;
  raw: string;
  clean: string;
}

interface ProfileScore {
  path: string;
  score: number;
  snrDb: number;
}

interface AbMetrics {
  speechLevelDb: number;
  levelDeltaDb: number;
  pauseContrastDb: number;
  longestDropoutS: number;
}

interface Toast {
  id: number;
  title: string;
  text: string;
  kind: ToastKind;
}

type ToastKind = "info" | "warn" | "ok";

const settings = ref<Settings | null>(null);
const stats = ref<Stats>({
  inputRms: 0,
  outputRms: 0,
  rtf: 0,
  latencyMs: 0,
  bufferMs: 0,
  underruns: 0,
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
const selectedInput = ref("");
const selectedOutput = ref("");
const busy = ref(false);
const starting = ref(false);
const error = ref("");
const tab = ref<"sound" | "settings">("sound");

const profiles = ref<string[]>([]);
const selectedProfile = ref("");
const profileQuality = ref<Quality | null>(null);
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
const showWizard = ref(false);
const profileScores = ref<ProfileScore[]>([]);
const abDelta = ref<number | null>(null);
const abMetrics = ref<AbMetrics | null>(null);
const toasts = ref<Toast[]>([]);
const capturingHotkey = ref(false);
const settingsQuery = ref("");
const toastTimers = new Map<number, number>();
const TOAST_MS = 7000;
let toastSeq = 0;
let slowStreak = 0;
let slowWarnAt = 0;
let quietMicWarned = false;

const cableOk = ref(true);
const installingCable = ref(false);
const cableMessage = ref("");

const modelStatus = ref("Модель VoxLock · int8 · встроена");
const checkingModel = ref(false);
const autostart = ref(false);
const testMode = ref(false);
const theme = ref("dark");

let timer: number | undefined;
let deviceTimer: number | undefined;
let healthRetryAt = 0;
let probeTimer: number | undefined;
let cableTimer: number | undefined;
let spectrumTimer: number | undefined;
let abMisses = 0;
const deviceLost = ref(false);

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
let media: MediaQueryList | undefined;

const card =
  "rounded-2xl border border-[var(--stroke)] bg-[var(--surface)] p-5 shadow-[0_1px_2px_rgba(0,0,0,0.06)]";
const btn =
  "inline-flex cursor-pointer items-center gap-1.5 rounded-xl border border-[var(--stroke)] bg-[var(--surface-alt)] px-3 py-2 text-[13px] text-[var(--text)] transition duration-150 hover:border-[var(--accent)] active:scale-[0.98] disabled:cursor-default disabled:opacity-45";
const btnPrimary =
  "inline-flex cursor-pointer items-center gap-1.5 rounded-xl bg-[var(--accent)] px-3 py-2 text-[13px] font-medium text-[var(--on-accent)] transition duration-150 hover:brightness-110 active:scale-[0.98] disabled:cursor-default disabled:opacity-45";
const muted = "text-[12px] leading-relaxed text-[var(--muted)]";
const labelText = "text-[13px] text-[var(--text)]";
const settingRow = "srow grid grid-cols-[210px_1fr] items-center gap-3 py-[7px]";

const running = computed(() => stats.value.running);
const isDark = computed(() => theme.value === "dark");
const statusText = computed(() =>
  error.value ? "Ошибка" : starting.value ? "Запуск модели…" : running.value ? "Работает" : "Выключено"
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
const inputDb = computed(() => (stats.value.inputRms > 1e-5 ? `${(20 * Math.log10(stats.value.inputRms)).toFixed(0)} дБ` : "—"));
const outputDb = computed(() => (stats.value.outputRms > 1e-5 ? `${(20 * Math.log10(stats.value.outputRms)).toFixed(0)} дБ` : "—"));
const abInfo = computed(() => {
  const path = abClean.value;
  if (!path) return "";
  const match = /ab-(\d+)-clean/.exec(path);
  if (!match) return "Последняя запись";
  const at = new Date(Number(match[1]) * 1000);
  return `Последняя запись · ${at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
});
const cpuText = computed(() => (stats.value.rtf > 0 ? `${(stats.value.rtf * 100).toFixed(0)}%` : "—"));
const latencyText = computed(() => (stats.value.latencyMs > 0 ? `${stats.value.latencyMs.toFixed(0)}` : "—"));

const presets = [
  { id: "home", label: "Дом", patch: { gateThresholdDb: -60, gateRatio: 2, gateReleaseMs: 300, agcTargetDb: -24, agcMaxGainDb: 12, highPassHz: 85, comfortNoiseEnabled: false } },
  { id: "office", label: "Офис", patch: { gateThresholdDb: -52, gateRatio: 3, gateReleaseMs: 250, agcTargetDb: -22, highPassHz: 100, comfortNoiseEnabled: true, comfortNoiseDb: -64 } },
  { id: "street", label: "Улица", patch: { gateThresholdDb: -46, gateRatio: 4, gateReleaseMs: 200, agcTargetDb: -20, highPassHz: 120, comfortNoiseEnabled: true, comfortNoiseDb: -60 } },
  { id: "stream", label: "Стрим", patch: { gateThresholdDb: -58, gateRatio: 2.5, gateReleaseMs: 300, agcTargetDb: -24, highPassHz: 80, comfortNoiseEnabled: false, outputGainDb: -6 } },
];

function applyPreset(patch: Record<string, unknown>) {
  if (!settings.value) return;
  Object.assign(settings.value, patch);
}

const inputOptions = computed(() => inputs.value.map((n) => ({ value: n, label: n })));
const outputOptions = computed(() => outputs.value.map((n) => ({ value: n, label: n })));
const previewOutput = computed(
  () => outputs.value.find((n) => !isCable(n)) ?? outputs.value[0] ?? ""
);

// the device the user picked for listening; the cable is never a preview target
const testOutput = computed(() =>
  selectedOutput.value && !isCable(selectedOutput.value) ? selectedOutput.value : previewOutput.value
);

function isCable(name: string): boolean {
  return /CABLE Input|VB-Audio|VB-CABLE/i.test(name);
}

function toLevel(rms: number): number {
  const db = 20 * Math.log10(rms + 1e-7);
  return Math.max(0, Math.min(1, (db + 60) / 60));
}

function shortName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/// The cable's *capture* side. Using it as the microphone means the app
/// processes its own output (and every app playing into the cable), which is a
/// feedback loop, not a microphone: it must never be picked automatically.
function isCableInput(name: string): boolean {
  return /CABLE Output|VB-Audio|VB-CABLE/i.test(name);
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
    cableOk.value = outs.some((n) => isCable(n));
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

async function refreshProfiles() {
  profiles.value = await invoke<string[]>("list_profiles");
  try {
    profileScores.value = (await invoke<ProfileScore[] | null>("profile_scores")) ?? [];
  } catch {
    profileScores.value = [];
  }
  const best = [...profileScores.value].sort((a, b) => b.score - a.score)[0];
  if (settings.value?.autoProfile && best) {
    selectedProfile.value = best.path;
    settings.value.profilePath = best.path;
    return;
  }
  if (!selectedProfile.value && settings.value?.profilePath) {
    selectedProfile.value = settings.value.profilePath;
  }
  if (!selectedProfile.value && profiles.value.length) {
    selectedProfile.value = profiles.value[0];
    if (settings.value) settings.value.profilePath = profiles.value[0];
  }
}


const profileOptions = computed(() =>
  profiles.value.map((path) => {
    const score = profileScores.value.find((s) => s.path === path);
    const name = shortName(path);
    return { value: path, label: score ? `${name} · ${score.score}/100` : name };
  })
);

const activeProfileScore = computed(() => {
  const path = selectedProfile.value || settings.value?.profilePath || "";
  return profileScores.value.find((s) => s.path === path) ?? null;
});

const profileWeak = computed(() => !!activeProfileScore.value && activeProfileScore.value.score < 55);

const inputIsCable = computed(() => !!selectedInput.value && isCableInput(selectedInput.value));

interface SystemInfo {
  logicalCores: number;
  physicalCores: number;
  recommendedThreads: number;
}

const sysInfo = ref<SystemInfo | null>(null);
const maxThreads = computed(() => Math.max(2, sysInfo.value?.logicalCores ?? 8));
const autoThreads = computed(() => sysInfo.value?.recommendedThreads ?? 4);

async function loadSystemInfo() {
  try {
    sysInfo.value = await invoke<SystemInfo>("system_info", {
      chunkMs: settings.value?.chunkMs ?? 60,
    });
  } catch {
    sysInfo.value = null;
  }
}

/// Configurations measured slower than real time in scripts/voxlock.ps1: the
/// model then cannot keep up and the ring drops blocks.
const slowConfig = computed(() => {
  const s = settings.value;
  if (!s) return false;
  const threads = s.autoThreads ? autoThreads.value : s.denoiserThreads;
  return s.chunkMs <= 40 && threads <= 2;
});

function notify(title: string, kind: ToastKind = "info", text = "") {
  // the same message twice (the engine watchdog fires every few seconds) must
  // refresh the timer, not stack a second card
  const existing = toasts.value.find((t) => t.title === title && t.text === text);
  if (existing) {
    window.clearTimeout(toastTimers.get(existing.id));
    toastTimers.set(
      existing.id,
      window.setTimeout(() => dismissToast(existing.id), TOAST_MS),
    );
    return;
  }
  const id = ++toastSeq;
  const next = [...toasts.value, { id, title, text, kind }].slice(-3);
  for (const gone of toasts.value) {
    if (!next.some((t) => t.id === gone.id)) {
      window.clearTimeout(toastTimers.get(gone.id));
      toastTimers.delete(gone.id);
    }
  }
  toasts.value = next;
  toastTimers.set(
    id,
    window.setTimeout(() => dismissToast(id), TOAST_MS),
  );
}

function dismissToast(id: number) {
  window.clearTimeout(toastTimers.get(id));
  toastTimers.delete(id);
  toasts.value = toasts.value.filter((t) => t.id !== id);
}

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
      "Громкость выровнена",
      "ok",
      `Выход сдвинут на ${delta >= 0 ? "−" : "+"}${Math.abs(delta).toFixed(1)} дБ относительно оригинала.`,
    );
  } catch (e) {
    error.value = String(e);
  }
}

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

function filterSettings(value: string) {
  settingsQuery.value = value;
  const query = value.trim().toLowerCase();
  document.querySelectorAll<HTMLElement>(".srow").forEach((row) => {
    row.style.display =
      !query || (row.textContent ?? "").toLowerCase().includes(query) ? "" : "none";
  });
}


async function startEngine(test: boolean) {
  if (starting.value) return;
  if (!selectedInput.value || isCableInput(selectedInput.value)) {
    testMode.value = false;
    error.value = selectedInput.value
      ? "Выбран виртуальный кабель вместо микрофона — так приложение слышит собственный выход. Выберите настоящий микрофон."
      : "Микрофон не найден. Подключите микрофон и нажмите «Обновить».";
    return;
  }
  starting.value = true;
  error.value = "";
  try {
    const voice = settings.value?.mode === "profile" ? settings.value?.profilePath ?? null : null;
    const wanted = test ? testOutput.value : selectedOutput.value;
    const output = wanted && outputs.value.includes(wanted) ? wanted : test ? previewOutput.value : "";
    await invoke("start", {
      input: selectedInput.value || null,
      output: output || null,
      voice,
      testMode: test,
    });
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
  if (q.clipPercent > 1) return "есть перегруз, убавьте ручку микрофона";
  if (q.peakDb < -30) return "микрофон тихий, поднимите ручку";
  if (q.speechPercent < 0.2) return "речи почти не слышно";
  return "уровень в норме";
});

async function recordProfile() {
  if (recording.value || !settings.value) return;
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

function playProfile() {
  if (!selectedProfile.value) return;
  return togglePlay(selectedProfile.value, "profile");
}

async function pickProfile() {
  if (!settings.value || !selectedProfile.value) return;
  settings.value.profilePath = selectedProfile.value;
  profileQuality.value = null;
  if (running.value) {
    const test = testMode.value;
    await invoke("stop");
    await startEngine(test);
  }
}

async function deleteProfile() {
  if (!selectedProfile.value) return;
  try {
    await invoke("delete_profile", { path: selectedProfile.value });
    selectedProfile.value = "";
    if (settings.value) settings.value.profilePath = null;
    await refreshProfiles();
  } catch (e) {
    error.value = String(e);
  }
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
      error.value = "Запись прервана: обработка остановилась.";
      return;
    }
  } catch {
    if (++abMisses >= 5) {
      abRunning.value = false;
      error.value = "Запись прервана: обработка остановилась.";
      return;
    }
  }
  window.setTimeout(pollAb, 200);
}

function formatStamp(stamp: number): string {
  const date = new Date(stamp * 1000);
  return date.toLocaleString([], {
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

async function resetSettings() {
  if (!window.confirm("Сбросить все настройки к заводским?")) return;
  error.value = "";
  try {
    settings.value = await invoke<Settings>("reset_settings");
    applyTheme(settings.value.theme);
    await refreshDevices();
    await refreshProfiles();
  } catch (e) {
    error.value = String(e);
  }
}

async function exportSettings() {
  error.value = "";
  try {
    const path = await invoke<string>("export_settings");
    cableMessage.value = `Настройки сохранены: ${path}`;
  } catch (e) {
    error.value = String(e);
  }
}

async function importSettings() {
  error.value = "";
  try {
    settings.value = await invoke<Settings>("import_settings");
    applyTheme(settings.value.theme);
    await refreshDevices();
    await refreshProfiles();
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

async function setMuted(muted: boolean) {
  try {
    await invoke("set_muted", { muted });
    stats.value.muted = muted;
  } catch (e) {
    error.value = String(e);
  }
}

function closeWizard() {
  showWizard.value = false;
  if (settings.value) settings.value.onboarded = true;
}

async function checkModel() {
  checkingModel.value = true;
  try {
    modelStatus.value = await invoke<string>("check_model_update");
  } catch (e) {
    modelStatus.value = String(e);
  } finally {
    checkingModel.value = false;
  }
}

async function installCable() {
  if (installingCable.value) return;
  installingCable.value = true;
  cableMessage.value = "Скачиваю драйвер…";
  error.value = "";
  try {
    await invoke<string>("install_cable");
    cableMessage.value = "Установщик запущен. Нажмите «Install Driver» и разрешите установку.";
    const started = Date.now();
    if (cableTimer) window.clearInterval(cableTimer);
    cableTimer = window.setInterval(async () => {
      await refreshDevices();
      if (cableOk.value) {
        if (cableTimer) window.clearInterval(cableTimer);
        cableTimer = undefined;
        cableMessage.value = "Готово: виртуальный микрофон найден.";
        installingCable.value = false;
        return;
      }
      // the installer can be closed without installing: stop waiting instead
      // of leaving the block in "installing" forever
      if (Date.now() - started > 150_000) {
        if (cableTimer) window.clearInterval(cableTimer);
        cableTimer = undefined;
        installingCable.value = false;
        cableMessage.value =
          "Установка не завершена. Закройте установщик и запустите ещё раз; если драйвер уже встал, перезагрузите компьютер.";
      }
    }, 2000);
  } catch (e) {
    cableMessage.value = "";
    error.value = String(e);
    installingCable.value = false;
  }
}

async function applyAutostart() {
  try {
    await invoke("set_autostart", { enabled: autostart.value });
  } catch (e) {
    error.value = String(e);
  }
}

function applyTheme(value: string) {
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
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

onMounted(async () => {
  try {
    settings.value = await invoke<Settings>("get_settings");
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

  window.addEventListener("keydown", onHotkeyKey, true);
  if (profileWeak.value) {
    notify(
      "Профиль голоса слабый",
      "warn",
      "Запишите новый — 15 секунд в тишине, обычным голосом.",
    );
  }
  if (settings.value?.autoStart && cableOk.value) {
    await startEngine(false);
  }

  media = window.matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener("change", onSystemTheme);

  timer = window.setInterval(async () => {
    try {
      stats.value = await invoke<Stats>("stats");
      // the processor thread can die on a model panic: the streams stay alive
      // but nothing comes out, so restart the engine and say why
      if (stats.value.running && !stats.value.healthy && Date.now() > healthRetryAt) {
        healthRetryAt = Date.now() + 4000;
        error.value = "Обработка остановилась — перезапускаю движок…";
        await stopEngine();
        await startEngine(testMode.value);
        // `startEngine` reports a failure by leaving `running` false and writing
        // `error`; clearing the error unconditionally hid a failed restart
        if (running.value) {
          error.value = "";
          notify("Движок перезапущен", "ok", "Обработка снова работает.");
        } else {
          notify(
            "Движок не поднялся",
            "warn",
            error.value.replace("Обработка остановилась — перезапускаю движок…", "").trim() ||
              "Проверьте устройства и лог.",
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
            "Процессор не успевает",
            "warn",
            `Обработка занимает ${(stats.value.rtf * 100).toFixed(0)} % реального времени. Поставьте блок 60 мс или больше потоков.`,
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
          "Микрофон очень тихий",
          "warn",
          "Выравнивание работает на пределе (+18 дБ) и поднимает шум комнаты. Поднимите громкость микрофона в Windows.",
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
        error.value = "Устройство отключилось. Ждём подключения…";
        notify("Микрофон отключился", "warn", "Ждём, пока устройство вернётся.");
        return;
      }

      if (deviceLost.value && !inputGone && !outputGone) {
        deviceLost.value = false;
        error.value = "";
        notify("Микрофон снова на связи", "ok");
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
  if (cableTimer) window.clearInterval(cableTimer);
  if (spectrumTimer) window.clearInterval(spectrumTimer);
  if (saveTimer) window.clearTimeout(saveTimer);
  if (recordTimer) window.clearInterval(recordTimer);
  if (restartTimer) window.clearTimeout(restartTimer);
  media?.removeEventListener("change", onSystemTheme);
  window.removeEventListener("keydown", onHotkeyKey, true);
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
    <TitleBar />

    <div class="app-scroll mx-auto w-full max-w-[1040px] flex-1 overflow-y-auto px-6 pb-8">
      <header class="flex items-end gap-3 pb-4">
        <div>
          <h1 class="text-[26px] font-semibold tracking-tight">Noise Canceller</h1>
          <div class="mt-1 flex items-center gap-2 text-[12px] text-[var(--muted)]">
            <span class="h-1.5 w-1.5 rounded-full" :class="dotClass"></span>
            {{ statusText }}
            <span v-if="stats.backend" class="opacity-70">· {{ stats.backend }}</span>
          </div>
        </div>
        <div class="flex-1"></div>
        <button :class="btn" @click="toggleTheme">
          <AppIcon :name="isDark ? 'sun' : 'moon'" />
          {{ isDark ? "Светлая" : "Тёмная" }}
        </button>
      </header>

      <!-- hero -->
      <!-- hero -->
      <section
        class="relative mb-4 overflow-hidden rounded-3xl border border-[var(--stroke)] p-6"
        style="
          background:
            radial-gradient(120% 160% at 0% 0%, var(--accent-soft) 0%, transparent 58%),
            var(--surface);
        "
      >
        <div class="flex flex-wrap items-center gap-6">
          <button
            class="relative grid h-[92px] w-[92px] shrink-0 cursor-pointer place-items-center rounded-full transition-all duration-200 active:scale-95 disabled:cursor-default disabled:opacity-60"
            :class="
              running
                ? 'bg-[var(--accent)] text-[var(--on-accent)] shadow-[0_0_46px_-10px_var(--accent)] pulse-ring'
                : starting
                  ? 'bg-[var(--accent-soft)] text-[var(--accent)]'
                  : 'border border-[var(--stroke)] bg-[var(--surface-alt)] text-[var(--muted)]'
            "
            :disabled="busy || starting"
            @click="togglePower"
          >
            <AppIcon v-if="starting" name="spinner" :size="32" :stroke="2.2" class="spin" />
            <AppIcon v-else name="power" :size="34" :stroke="2.2" />
          </button>

          <div class="min-w-[220px] flex-1">
            <div class="text-[17px] font-semibold">
              {{ testMode ? "Проверка в колонках" : "Шумоподавление" }}
            </div>
            <p :class="muted" class="mt-1 min-h-[39px]">
              {{
                testMode
                  ? "Говорите — звук идёт в «" + (testOutput || "устройство вывода") + "». Это режим проверки."
                  : running
                    ? "Активно · VoxLock 16 кГц · в виртуальный микрофон VB-CABLE"
                    : "Выключено. Без виртуального кабеля звук в колонки не идёт — используйте проверку."
              }}
            </p>
            <!-- fixed-height slot: the mute chip appears and disappears here, but the
                 space is always reserved, so the block never re-centers and the text
                 does not jump when the hotkey is pressed -->
            <div class="mt-1.5 flex h-[24px] items-center">
              <button
                v-if="stats.muted"
                class="inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-lg bg-[var(--danger-soft)] px-2 text-[11.5px] leading-none text-[var(--danger)] transition hover:brightness-105"
                title="Включить микрофон"
                @click="setMuted(false)"
              >
                <AppIcon name="mic" :size="13" />
                Микрофон выключен · Ctrl+Alt+M
              </button>
            </div>
          </div>

          <div class="flex gap-7">
            <div>
              <div class="text-[11px] uppercase tracking-wide text-[var(--muted)]">Задержка</div>
              <div class="mt-0.5 text-[22px] font-semibold tabular-nums">
                {{ latencyText }}<span v-if="stats.latencyMs > 0" class="ml-1 text-[12px] font-normal text-[var(--muted)]">мс</span>
              </div>
            </div>
            <div>
              <div class="text-[11px] uppercase tracking-wide text-[var(--muted)]">Нагрузка</div>
              <div class="mt-0.5 text-[22px] font-semibold tabular-nums">{{ cpuText }}</div>
            </div>
            <div>
              <div class="text-[11px] uppercase tracking-wide text-[var(--muted)]">Подавление</div>
              <div class="mt-0.5 text-[22px] font-semibold tabular-nums">
                {{ stats.suppressionDb > 0.5 ? "−" + stats.suppressionDb.toFixed(0) : "—"
                }}<span
                  v-if="stats.suppressionDb > 0.5"
                  class="ml-1 text-[12px] font-normal text-[var(--muted)]"
                  >дБ</span
                >
                <span
                  v-if="stats.suppressionDb > 0.5 && settings && settings.maxSuppressionDb > 0"
                  class="ml-1 text-[11px] font-normal text-[var(--muted)]"
                  >/ {{ settings.maxSuppressionDb.toFixed(0) }}</span
                >
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- cable banner -->
      <section
        v-if="!cableOk"
        :class="card"
        class="mb-4 !border-[var(--danger)]"
      >
        <div class="text-[15px] font-semibold text-[var(--danger)]">Нужен виртуальный микрофон</div>
        <p :class="muted" class="mt-1">
          Zoom / Meet / Discord не увидят обработанный звук без VB-CABLE. Установка одной кнопкой,
          права администратора один раз.
        </p>
        <div class="mt-3 flex flex-wrap items-center gap-2">
          <button :class="btnPrimary" :disabled="installingCable" @click="installCable">
            <AppIcon v-if="installingCable" name="spinner" class="spin" />
            {{ installingCable ? "Установка…" : "Установить виртуальный микрофон" }}
          </button>
          <span :class="muted">{{ cableMessage }}</span>
        </div>
      </section>

      <div class="mb-4 inline-flex rounded-xl border border-[var(--stroke)] bg-[var(--surface-alt)] p-1">
        <button
          class="cursor-pointer rounded-lg px-3.5 py-1.5 text-[12.5px] transition"
          :class="tab === 'sound' ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm' : 'text-[var(--muted)] hover:text-[var(--text)]'"
          @click="tab = 'sound'"
        >
          Звук
        </button>
        <button
          class="cursor-pointer rounded-lg px-3.5 py-1.5 text-[12.5px] transition"
          :class="tab === 'settings' ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm' : 'text-[var(--muted)] hover:text-[var(--text)]'"
          @click="tab = 'settings'"
        >
          Настройки
        </button>
      </div>

      <transition name="fade" mode="out-in">
        <!-- sound tab -->
        <div v-if="tab === 'sound' && settings" key="sound" class="flex flex-col gap-3.5">
          <div class="grid grid-cols-1 gap-3.5 lg:grid-cols-2">
          <section :class="card">
            <div class="mb-3 text-[13px] font-medium text-[var(--muted)]">Устройства</div>
            <div class="grid grid-cols-[86px_1fr] items-center gap-3">
              <span :class="labelText">Микрофон</span>
              <AppSelect
                :model-value="selectedInput"
                :options="inputOptions"
                :disabled="reconnecting || starting"
                 placeholder="Нет устройств"
                 @update:model-value="pickInput"
               />
             </div>
             <p v-if="inputIsCable" class="mt-1.5 text-[12px] text-[var(--danger)]">
               Выбран виртуальный кабель как микрофон: приложение будет слышать собственный выход и
               звук других программ. Выберите настоящий микрофон.
             </p>
            <div class="mt-2.5 grid grid-cols-[86px_1fr] items-center gap-3">
              <span :class="labelText">Наушники</span>
              <AppSelect
                :model-value="selectedOutput"
                :options="outputOptions"
                :disabled="reconnecting || starting"
                placeholder="Нет устройств"
                @update:model-value="pickOutput"
              />
            </div>
            <p v-if="reconnecting" :class="muted" class="mt-2 flex items-center gap-1.5">
              <AppIcon name="spinner" :size="13" class="spin" />
              Переподключаю устройства…
            </p>
            <p v-else :class="muted" class="mt-2">
              Куда слушать себя в режиме проверки. Обычная работа всегда идёт в виртуальный микрофон.
            </p>
            <div class="mt-3 flex flex-wrap gap-2">
              <button :class="btn" :disabled="refreshing" @click="refreshDevicesManually">
                <AppIcon name="refresh" :class="{ spin: refreshing }" />
                {{ refreshing ? "Обновляю…" : "Обновить" }}
              </button>
              <button :class="btn" :disabled="probing" @click="probeMic">
                <AppIcon name="mic" />
                {{
                  probing
                    ? probeLeft > 0
                      ? `Слушаю… ${probeLeft} с`
                      : "Слушаю…"
                    : "Проверить микрофон"
                }}
              </button>
            </div>
            <p v-if="probeResult" :class="muted" class="mt-2">
              Уровень {{ probeResult.peakDb.toFixed(0) }} дБ · речь
              {{ (probeResult.speechPercent * 100).toFixed(0) }}% · SNR {{ probeResult.snrDb.toFixed(0) }} дБ —
              {{ probeHint }}
            </p>
          </section>

          <section :class="card">
            <div class="mb-3 text-[13px] font-medium text-[var(--muted)]">Уровень и спектр</div>
            <div class="flex items-center justify-between text-[11px] uppercase tracking-wide text-[var(--muted)]">
              <span class="flex items-center gap-2">
                Вход
                <span
                  v-if="stats.inputPeakDb > -1"
                  class="rounded bg-[var(--danger-soft)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--danger)]"
                >
                  ПЕРЕГРУЗ
                </span>
              </span>
              <span class="tabular-nums normal-case tracking-normal">{{ inputDb }}</span>
            </div>
            <div class="mt-1.5 h-1.5 overflow-hidden rounded-full bg-[var(--stroke)]">
              <div
                class="h-full rounded-full transition-[width] duration-75"
                style="background: linear-gradient(90deg, var(--accent), var(--accent-2))"
                :style="{ width: inputLevel * 100 + '%' }"
              ></div>
            </div>
            <div class="mt-4 flex items-center justify-between text-[11px] uppercase tracking-wide text-[var(--muted)]">
              <span>Выход</span>
              <span class="tabular-nums normal-case tracking-normal">{{ outputDb }}</span>
            </div>
            <div class="mt-1.5 h-1.5 overflow-hidden rounded-full bg-[var(--stroke)]">
              <div
                class="h-full rounded-full transition-[width] duration-75"
                style="background: linear-gradient(90deg, var(--accent), var(--accent-2))"
                :style="{ width: outputLevel * 100 + '%' }"
              ></div>
            </div>
            <div v-if="settings" class="mt-5">
              <div class="flex items-center justify-between text-[11px] uppercase tracking-wide text-[var(--muted)]">
                <span>Громкость выхода</span>
                <span class="normal-case tracking-normal">в виртуальный микрофон</span>
              </div>
              <div class="mt-1">
                <AppSlider
                  v-model="settings.outputGainDb"
                  :min="-18"
                  :max="18"
                  :step="0.5"
                  :format="(v) => `${v > 0 ? '+' : ''}${v.toFixed(1)} дБ`"
                />
              </div>
              <p v-if="abMetrics" :class="muted" class="mt-1">
                В последней записи обработанный звук
                {{ abMetrics.levelDeltaDb >= 0 ? "громче" : "тише" }} сырого на
                {{ Math.abs(abMetrics.levelDeltaDb).toFixed(1) }} дБ. Кнопка «Сравнять с
                оригиналом» подгонит выходную громкость.
              </p>
              <p v-else :class="muted" class="mt-1">
                Громкость выхода меняется после всей обработки: шумодав и изоляцию голоса она
                не трогает.
              </p>
            </div>
            <div class="mt-4 flex h-[76px] items-end gap-[3px] rounded-lg bg-[var(--surface-alt)] px-2.5 pt-2 pb-2">
              <div
                v-for="(value, index) in shownSpectrum"
                :key="index"
                class="flex-1 rounded-full transition-[height,background-color] duration-150"
                :style="{
                  height: Math.max(spectrumLive ? 3 : 10, value * 100) + '%',
                  background: spectrumLive
                    ? 'linear-gradient(180deg, var(--accent-2), var(--accent))'
                    : 'var(--muted)',
                  opacity: spectrumLive ? 0.4 + value * 0.6 : 0.3,
                }"
              ></div>
            </div>
            <p v-if="microphoneLive" :class="muted" class="mt-2 flex items-center gap-1.5">
              <AppIcon name="mic" :size="13" />
              Слушаю микрофон…
            </p>
            <p v-else-if="!hasAudio" :class="muted" class="mt-2">
              Обработка не запущена: включите виртуальный микрофон, и здесь появится живой спектр.
            </p>
          </section>

          <section :class="card">
            <div class="mb-3 text-[13px] font-medium text-[var(--muted)]">Профиль голоса</div>
            <AppSelect v-model="selectedProfile" :options="profileOptions" @update:model-value="pickProfile" />
            <div class="mt-3 flex flex-wrap gap-2">
              <button :class="btnPrimary" :disabled="recording" @click="recordProfile">
                <span v-if="recording" class="h-2.5 w-2.5 shrink-0 rounded-full bg-current" />
                <AppIcon v-else name="mic" />
                {{ recording ? `Запись… ${recordLeft} с` : "Записать 15 с" }}
              </button>
              <button
                :class="playingKey === 'profile' ? btnPrimary : btn"
                :disabled="!selectedProfile"
                @click="playProfile"
                :title="playingKey === 'profile' ? 'Остановить' : 'Прослушать профиль'"
              >
                <AppIcon :name="playingKey === 'profile' ? 'stop' : 'play'" />
                {{ playingKey === "profile" ? "Стоп" : "Прослушать" }}
              </button>
              <button
                :class="btn"
                class="!px-3 hover:!text-[var(--danger)]"
                :disabled="!selectedProfile"
                @click="deleteProfile"
                title="Удалить профиль"
              >
                <AppIcon name="trash" />
              </button>
            </div>
            <p :class="muted" class="mt-3">
              Говорите обычным голосом в тишине: модель запомнит тембр и будет пропускать только его.
            </p>
            <p v-if="profileQuality" :class="muted">
              Оценка <span class="font-medium text-[var(--text)]">{{ profileQuality.score }}/100</span> · речь
              {{ (profileQuality.speechPercent * 100).toFixed(0) }}% · SNR
              {{ profileQuality.snrDb.toFixed(0) }} дБ
            </p>
            <p v-else-if="activeProfileScore" :class="muted">
              Оценка профиля
              <span class="font-medium text-[var(--text)]">{{ activeProfileScore.score }}/100</span>
              · SNR {{ activeProfileScore.snrDb.toFixed(0) }} дБ
            </p>
            <p v-if="profileWeak" class="mt-1 text-[12px] text-[var(--danger)]">
              Профиль слабый: модель будет хуже узнавать голос. Запишите новый — 15 секунд в тишине.
            </p>
            <div class="mt-2 flex items-center gap-2">
              <AppSwitch v-model="settings.autoProfile" @update:model-value="refreshProfiles" />
              <span :class="muted">Выбирать лучший профиль автоматически</span>
            </div>
          </section>

          <section :class="card">
            <div class="mb-3 flex items-center justify-between gap-2">
              <div class="text-[13px] font-medium text-[var(--muted)]">Проверка качества</div>
              <div v-if="settings" class="inline-flex items-center gap-0.5 rounded-lg bg-[var(--surface-alt)] p-0.5">
                <button
                  v-for="sec in [10, 30, 60]"
                  :key="sec"
                  class="cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] transition"
                  :class="
                    settings.abSeconds === sec
                      ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm'
                      : 'text-[var(--muted)] hover:text-[var(--text)]'
                  "
                  @click="settings.abSeconds = sec"
                >
                  {{ sec }} с
                </button>
              </div>
            </div>
            <div class="flex flex-wrap gap-2">
              <button
                :class="abRunning ? btnPrimary : btn"
                :disabled="!running && !abRunning"
                @click="toggleAb"
              >
                <span v-if="abRunning" class="h-2.5 w-2.5 shrink-0 rounded-full bg-current" />
                <AppIcon v-else name="record" />
                {{
                  abRunning
                    ? `Запись… ${Math.round(abProgress * (settings?.abSeconds ?? 30))} с`
                    : `Записать A/B (${settings?.abSeconds ?? 30} с)`
                }}
              </button>
              <button :class="testMode ? btnPrimary : btn" :disabled="busy || starting" @click="toggleTest">
                <AppIcon
                  :name="busy || starting ? 'spinner' : 'headphones'"
                  :class="busy || starting ? 'spin' : ''"
                />
                {{
                  busy || starting
                    ? "Переключаю…"
                    : testMode
                      ? "Остановить проверку"
                      : "Проверить в наушниках"
                }}
              </button>
            </div>
            <div v-if="abRunning" class="mt-3 h-1.5 overflow-hidden rounded-full bg-[var(--stroke)]">
              <div class="h-full rounded-full bg-[var(--accent)]" :style="{ width: abProgress * 100 + '%' }"></div>
            </div>
            <div v-if="abRaw && abClean" class="mt-3 flex flex-wrap gap-2">
              <button
                :class="playingKey === 'raw' ? btnPrimary : btn"
                @click="playFile(abRaw, 'raw')"
                :title="playingKey === 'raw' ? 'Остановить' : 'Слушать сырой микрофон'"
              >
                <AppIcon :name="playingKey === 'raw' ? 'stop' : 'play'" />
                {{ playingKey === "raw" ? "Стоп" : "Сырой" }}
              </button>
              <button
                :class="playingKey === 'clean' ? btnPrimary : btn"
                @click="playFile(abClean, 'clean')"
                :title="playingKey === 'clean' ? 'Остановить' : 'Слушать обработанный звук'"
              >
                <AppIcon :name="playingKey === 'clean' ? 'stop' : 'play'" />
                {{ playingKey === "clean" ? "Стоп" : "Обработанный" }}
              </button>
            </div>
            <p v-if="abRaw && abClean" :class="muted" class="mt-2" :title="abFolder || ''">
              {{ abInfo }}
            </p>
            <label
              v-if="abRaw && abClean"
              class="mt-2 flex cursor-pointer items-center gap-2 text-[12px] text-[var(--muted)]"
            >
              <AppSwitch v-model="abNormalize" />
              Выравнивать громкость при прослушивании
            </label>
            <div v-if="abRaw && abClean" class="mt-2 flex flex-wrap items-center gap-2">
              <button :class="btn" @click="matchLevel" title="Подогнать выходную громкость под сырой микрофон">
                <AppIcon name="sliders" />
                Сравнять с оригиналом
              </button>
              <span v-if="abDelta !== null" :class="muted">
                обработанный {{ abDelta >= 0 ? "громче" : "тише" }} на
                {{ Math.abs(abDelta).toFixed(1) }} дБ
              </span>
            </div>
            <div
              v-if="abMetrics"
              class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[12px] text-[var(--muted)]"
            >
              <span>
                речь
                <span class="tabular-nums text-[var(--text)]"
                  >{{ abMetrics.speechLevelDb.toFixed(0) }} дБ</span
                >
              </span>
              <span>
                паузы тише речи на
                <span class="tabular-nums text-[var(--text)]"
                  >{{ Math.max(0, abMetrics.pauseContrastDb).toFixed(0) }} дБ</span
                >
              </span>
              <span>
                {{ abMetrics.levelDeltaDb >= 0 ? "громче" : "тише" }} оригинала на
                <span class="tabular-nums text-[var(--text)]"
                  >{{ Math.abs(abMetrics.levelDeltaDb).toFixed(1) }} дБ</span
                >
              </span>
              <span :class="abMetrics.longestDropoutS > 0.4 ? 'text-[var(--danger)]' : ''">
                провал
                <span class="tabular-nums">{{ abMetrics.longestDropoutS.toFixed(2) }} с</span>
              </span>
            </div>
            <p :class="muted" class="mt-2">Два файла: сырой микрофон и обработанный. Слушайте в наушниках.</p>
            <p v-if="!running" :class="muted" class="mt-1">
              Запись идёт через обработку: подключите виртуальный кабель или включите проверку в
              наушниках.
            </p>
            <p :class="muted" class="mt-1">
              Говорите текст, включите телевизор и постучите по столу — так слышно, что остаётся от
              фона и чужих голосов.
            </p>

            <div v-if="history.length" class="mt-3">
              <div class="mb-1 text-[11px] uppercase tracking-wide text-[var(--muted)]">
                История записей
              </div>
              <div class="flex flex-col gap-0.5">
                <div
                  v-for="entry in history"
                  :key="entry.stamp"
                  class="flex items-center gap-2 rounded-lg px-1.5 py-1 transition hover:bg-[var(--surface-alt)]"
                >
                  <button
                    class="flex-1 cursor-pointer text-left text-[12px]"
                    :class="abRaw === entry.raw ? 'text-[var(--accent)]' : 'text-[var(--text)]'"
                    :title="entry.raw"
                    @click="selectRecording(entry)"
                  >
                    {{ formatStamp(entry.stamp) }}
                  </button>
                  <button
                    class="cursor-pointer rounded-md p-1 text-[var(--muted)] transition hover:text-[var(--danger)]"
                    title="Удалить запись"
                    @click="removeRecording(entry)"
                  >
                    <AppIcon name="trash" :size="14" />
                  </button>
                </div>
              </div>
            </div>
          </section>
          </div>
        </div>

        <!-- settings tab -->
        <div v-else-if="settings" key="settings" class="flex flex-col gap-3.5">
          <div class="flex flex-wrap items-center gap-3">
            <input
              :value="settingsQuery"
              class="min-w-[200px] flex-1 rounded-xl border border-[var(--stroke)] bg-[var(--surface-alt)] px-3 py-[9px] text-[13px] text-[var(--text)] outline-none placeholder:text-[var(--muted)] focus:border-[var(--accent)]"
              placeholder="Поиск по настройкам…"
              @input="filterSettings(($event.target as HTMLInputElement).value)"
            />
            <label class="flex cursor-pointer items-center gap-2 text-[12px] whitespace-nowrap text-[var(--muted)]">
              <AppSwitch
                :model-value="!settings.basicUi"
                @update:model-value="(v: boolean) => (settings!.basicUi = !v)"
              />
              Дополнительно
            </label>
          </div>

          <section :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Пресеты</div>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="preset in presets"
                :key="preset.id"
                :class="btn"
                @click="applyPreset(preset.patch)"
              >
                {{ preset.label }}
              </button>
            </div>
            <p :class="muted" class="mt-2">
              Меняют порог гейта, целевой уровень и срез низов. Дальше можно подправить ползунками.
            </p>
          </section>

          <section :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Автоподавление голоса</div>
            <div :class="settingRow">
              <AppLabel text="Оставлять только мой голос" hint="VoxLock пропускает только ваш голос. Выключено — остаётся обычный шумодав: легче процессору и профиль не нужен." />
              <AppSwitch v-model="settings.voiceIsolation" @update:model-value="restartSoon" />
            </div>
            <p :class="muted" class="mb-1">
              VoxLock оставляет только ваш голос. Выключите, если нужен просто шумодав — так
              заметно легче процессору и не нужен профиль.
            </p>
            <div v-if="settings.voiceIsolation" :class="settingRow">
              <AppLabel text="Сила изоляции" hint="Сколько очищенного моделью сигнала идёт в выход. 1.00 — максимальная изоляция, ниже — естественнее, но слышнее фон." />
              <AppSlider
                v-model="settings.isolationStrength"
                :min="0"
                :max="1"
                :step="0.05"
                :format="(v) => v.toFixed(2)"
              />
            </div>
            <div v-if="settings.voiceIsolation" :class="settingRow">
              <AppLabel text="Предел подавления" hint="Выход модели никогда не будет тише входа больше чем на эту величину, пока вы говорите. 0 — без предела: максимальная изоляция, но модель может полностью заглушить голос." />
              <AppSlider
                v-model="settings.maxSuppressionDb"
                :min="0"
                :max="40"
                :step="1"
                :format="(v) => (v <= 0 ? 'без предела' : `${v} дБ`)"
              />
            </div>
            <div v-if="settings.voiceIsolation" :class="settingRow">
              <AppLabel text="Режим" hint="«По профилю» ищет записанный тембр — точнее всего. «Авто» работает без профиля, но чаще пропускает чужие голоса." />
              <AppSelect
                v-model="settings.mode"
                :options="[
                  { value: 'profile', label: 'По профилю голоса' },
                  { value: 'auto', label: 'Авто-определение' },
                ]"
                @update:model-value="restartSoon"
              />
            </div>
          </section>

          <section :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Чистка сигнала</div>

            <div :class="settingRow">
              <AppLabel text="Усиление тихого микрофона" hint="Поднимает тихий микрофон до −26 dBFS перед моделью. Влияет на разборчивость и ровность громкости." />
              <AppSwitch v-model="settings.inputLeveling" />
            </div>
            <p :class="muted" class="mb-1">
              Поднимает тихий микрофон до нормального уровня до обработки. Выключите, если пишете громко.
            </p>

            <div class="my-2 h-px bg-[var(--stroke)]"></div>

            <div :class="settingRow">
              <AppLabel text="Остаточный шум" hint="GTCRN добивает шум после VoxLock: паузы тише примерно на 20 дБ. Выключение немного снижает нагрузку." />
              <AppSelect
                v-model="settings.residualDenoise"
                :options="[
                  { value: 'off', label: 'Выключено' },
                  { value: 'gtcrn', label: 'GTCRN · лёгкий' },
                ]"
                @update:model-value="restartSoon"
              />
            </div>
            <p :class="muted" class="mb-1">
              Вторая модель после VoxLock: добивает шум, который остался после выделения вашего голоса.
              GTCRN почти не нагружает процессор.
            </p>

            <div :class="settingRow">
              <AppLabel text="VAD-гейт" hint="Закрывает выход в паузах по детектору речи, а не по уровню. Фон между словами исчезает, тихие слова не режутся." />
              <AppSwitch v-model="settings.vadGate" />
            </div>
            <p :class="muted" class="mb-1">
              Silero VAD закрывает выход в паузах по детектору речи, а не по уровню — тихие слова
              не режутся, а фон в паузах уходит.
            </p>

            <div class="my-2 h-px bg-[var(--stroke)]"></div>

            <div :class="settingRow">
              <AppLabel text="Срез низов" hint="Фильтр 4-го порядка убирает гул, ветер и низкочастотный стук по столу." />
              <AppSwitch v-model="settings.highPassEnabled" />
            </div>
            <div v-if="!settings.basicUi" :class="settingRow">
              <AppLabel text="Частота среза" hint="Ниже этой частоты сигнал ослабляется. 80–120 Гц убирают гул, выше — начинает худеть сам голос." />
              <AppSlider v-model="settings.highPassHz" :min="20" :max="300" :step="5" :format="(v) => `${v} Гц`" />
            </div>

            <div class="my-2 h-px bg-[var(--stroke)]"></div>

            <div :class="settingRow">
              <AppLabel text="Гейт" hint="Пороговый шумодав для остатков сигнала: закрывает всё, что тише порога." />
              <AppSwitch v-model="settings.gateEnabled" />
            </div>
            <template v-if="!settings.basicUi">
            <div :class="settingRow">
              <AppLabel text="Порог" hint="Уровень закрытия гейта. Выше — чище паузы, но можно срезать тихие слова; ниже — мягче." />
              <AppSlider v-model="settings.gateThresholdDb" :min="-90" :max="-20" :step="1" :format="(v) => `${v} дБ`" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Степень" hint="Насколько сильно гейт глушит ниже порога: 1 — мягко, 10 — почти в ноль." />
              <AppSlider v-model="settings.gateRatio" :min="1" :max="10" :step="0.5" :format="(v) => v.toFixed(1)" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Атака" hint="Как быстро открывается гейт. Больше — плавнее, но начало слова может приглушаться." />
              <AppSlider v-model="settings.gateAttackMs" :min="1" :max="100" :step="1" :format="(v) => `${v} мс`" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Восстановление" hint="Как быстро гейт закрывается. Больше — не режет концы слов, но дольше тянет фон." />
              <AppSlider v-model="settings.gateReleaseMs" :min="20" :max="600" :step="10" :format="(v) => `${v} мс`" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Удержание" hint="Сколько гейт остаётся открытым после спада уровня, чтобы не обрубать концы слов." />
              <AppSlider v-model="settings.gateHoldMs" :min="0" :max="500" :step="10" :format="(v) => `${v} мс`" />
            </div>
            </template>

            <div class="my-2 h-px bg-[var(--stroke)]"></div>

            <div :class="settingRow">
              <AppLabel text="Автогромкость" hint="AGC возвращает уровень, который срезала модель, и выравнивает громкость фраз. Работает только на речи." />
              <AppSwitch v-model="settings.agcEnabled" />
            </div>
            <template v-if="!settings.basicUi">
            <div :class="settingRow">
              <AppLabel text="Целевой уровень" hint="К какому уровню AGC тянет речь. Ниже — тише и безопаснее, выше — громче, но ближе к перегрузу." />
              <AppSlider v-model="settings.agcTargetDb" :min="-36" :max="-8" :step="1" :format="(v) => `${v} дБ`" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Макс. усиление" hint="Предел подъёма AGC. Большой поднимает и остатки шума; для тихого микрофона хватает 12–18 дБ." />
              <AppSlider v-model="settings.agcMaxGainDb" :min="0" :max="30" :step="1" :format="(v) => `${v} дБ`" />
            </div>
            </template>
            <div :class="settingRow">
              <AppLabel text="Лимитер" hint="Не даёт сигналу выйти за 0 дБ и срезает пики. Лучше держать включённым." />
              <AppSwitch v-model="settings.limiterEnabled" />
            </div>

            <div class="my-2 h-px bg-[var(--stroke)]"></div>

            <div :class="settingRow">
              <AppLabel text="Комфортный шум" hint="Тихий ровный фон в паузах вместо «звонкой» тишины. По умолчанию выключен." />
              <AppSwitch v-model="settings.comfortNoiseEnabled" />
            </div>
            <div v-if="!settings.basicUi && settings.comfortNoiseEnabled" :class="settingRow">
              <AppLabel text="Уровень шума" hint="Громкость комфортного шума: −60…−70 дБ почти не слышно." />
              <AppSlider v-model="settings.comfortNoiseDb" :min="-90" :max="-30" :step="1" :format="(v) => `${v} дБ`" />
            </div>
            <div v-if="!settings.basicUi" :class="settingRow">
              <AppLabel text="Выходная громкость" hint="Общее усиление после всей обработки. Меняет только громкость, шумодав не трогает." />
              <AppSlider v-model="settings.outputGainDb" :min="-18" :max="18" :step="0.5" :format="(v) => `${v.toFixed(1)} дБ`" />
            </div>
          </section>

          <section v-if="!settings.basicUi" :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Задержка и нагрузка</div>
            <div :class="settingRow">
              <AppLabel text="Буфер автоматически" hint="Подбирает минимальный буфер без пропусков: при пропуске +20 мс, после 90 с без пропусков −10 мс. Значение запоминается между запусками." />
              <AppSwitch v-model="settings.bufferAuto" />
            </div>
            <div :class="settingRow">
              <AppLabel
                text="Буфер"
                :hint="
                  settings.bufferAuto
                    ? 'Текущее значение подобрано автоматически. Выключите авто, чтобы задать вручную.'
                    : 'Запас в кольце вывода. Больше — стабильнее и без пропусков, но выше задержка.'
                "
              />
              <AppSlider
                v-if="!settings.bufferAuto"
                v-model="settings.bufferMs"
                :min="20"
                :max="250"
                :step="10"
                :format="(v) => `${v} мс`"
              />
              <span v-else class="tabular-nums text-[13px]">
                {{ stats.bufferMs ? stats.bufferMs.toFixed(0) + " мс" : "—" }}
                <span class="text-[var(--muted)]">· авто</span>
              </span>
            </div>
            <div :class="settingRow">
              <AppLabel text="Блок модели" hint="Размер блока VoxLock: меньше — ниже задержка и выше нагрузка. 60 мс — баланс." />
              <AppSlider v-model="settings.chunkMs" :min="20" :max="120" :step="20" :format="(v) => `${v} мс`" />
            </div>
            <div :class="settingRow">
              <AppLabel
                text="Потоки модели"
                hint="Сколько ядер отдаётся VoxLock. До 4 потоков время обработки падает почти вдвое, дальше почти не помогает. «Автоматически» подбирает число по конфигурации машины и размеру блока."
              />
              <div class="flex items-center gap-2.5">
                <AppSwitch v-model="settings.autoThreads" />
                <span :class="muted">
                  Автоматически<span v-if="settings.autoThreads"> · {{ autoThreads }}</span>
                </span>
                <button
                  v-if="!settings.autoThreads"
                  :class="btn"
                  class="!px-2.5 !py-1.5"
                  title="Подставить рекомендованное число потоков"
                  @click="settings.denoiserThreads = autoThreads"
                >
                  Рекомендуется {{ autoThreads }}
                </button>
              </div>
            </div>
            <div v-if="!settings.autoThreads" :class="settingRow">
              <span :class="labelText" class="pl-[26px]">Сколько потоков</span>
              <AppSlider
                v-model="settings.denoiserThreads"
                :min="1"
                :max="maxThreads"
                :step="1"
                :format="(v) => `${v} из ${maxThreads}`"
              />
            </div>
            <p :class="muted" class="mt-1">
              Система:
              <span class="text-[var(--text)]">
                {{ sysInfo?.logicalCores ?? "—" }} логических ядер
                <span v-if="sysInfo">({{ sysInfo.physicalCores }} физических)</span>
              </span>
              · рекомендуется
              <span class="text-[var(--text)]">{{ autoThreads }}</span
              >. До четырёх потоков время обработки падает почти вдвое, выше — почти не
              помогает, но забирает ядра у игры.
            </p>
            <p v-if="slowConfig" class="mt-1 text-[12px] text-[var(--danger)]">
              На блоке {{ settings.chunkMs }} мс с
              {{ settings.autoThreads ? autoThreads : settings.denoiserThreads }} поток(ами) модель
              не успевает за реальным временем — звук будет рваться. Поставьте 60 мс или больше
              потоков.
            </p>
          </section>

          <section :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Управление</div>
            <div class="flex flex-wrap items-center gap-3">
              <button :class="capturingHotkey ? btnPrimary : btn" @click="startHotkeyCapture">
                <AppIcon name="mic" :size="14" />
                {{ capturingHotkey ? "Нажмите сочетание…" : settings.muteHotkey }}
              </button>
              <AppSelect
                v-model="settings.hotkeyMode"
                class="w-[170px]"
                :options="[
                  { value: 'toggle', label: 'Переключение' },
                  { value: 'hold', label: 'Удержание (PTT)' },
                ]"
              />
            </div>
            <p :class="muted" class="mt-2">
              Глобальное отключение микрофона. «Удержание» — пока клавиша нажата, микрофон включён
              (push-to-talk). Escape отменяет запись сочетания.
            </p>
          </section>

          <section :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Диагностика</div>
            <div class="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1.5 text-[12.5px]">
              <span class="text-[var(--muted)]">Усиление выравнивания</span>
              <span class="tabular-nums" :class="stats.levelerGainDb > 16 ? 'text-[var(--danger)]' : ''">
                {{ stats.levelerGainDb.toFixed(1) }} дБ
              </span>
              <span class="text-[var(--muted)]">Потоков модели</span>
              <span class="tabular-nums">
                {{ stats.threads || "—" }}<span v-if="settings?.autoThreads && stats.threads" class="text-[var(--muted)]"> · авто</span>
              </span>
              <span class="text-[var(--muted)]">Сбросов модели</span>
              <span class="tabular-nums">{{ stats.guardResets }}</span>
              <span class="text-[var(--muted)]" title="Сэмплы, которые не успели доехать до вывода; в скобках — примерная длительность. Растёт — обработка не успевает.">
                Пропущено сэмплов
              </span>
              <span class="tabular-nums">
                {{ stats.underruns
                }}<span v-if="stats.underruns && stats.outRate" class="text-[var(--muted)]">
                  · ≈ {{ ((stats.underruns / stats.outRate) * 1000).toFixed(0) }} мс</span
                >
              </span>
              <span class="text-[var(--muted)]" title="Блоки, которые не влезли в кольцо вывода и были потеряны. Ноль — норма.">
                Переполнений буфера
              </span>
              <span class="tabular-nums">{{ stats.overflows }}</span>
              <span class="text-[var(--muted)]" title="Сэмплы микрофона, потерянные из-за переполнения входного кольца. Ноль — норма.">
                Потеряно на входе
              </span>
              <span class="tabular-nums">{{ stats.captureDrops }}</span>
              <span class="text-[var(--muted)]">Подавление</span>
              <span class="tabular-nums">
                {{ stats.suppressionDb > 0.5 ? "−" + stats.suppressionDb.toFixed(0) + " дБ" : "—" }}
              </span>
              <span class="text-[var(--muted)]">Пик входа</span>
              <span class="tabular-nums">
                {{ stats.inputPeakDb > -100 ? stats.inputPeakDb.toFixed(0) + " дБ" : "—" }}
              </span>
              <span class="text-[var(--muted)]" title="Подстройка под расхождение часов микрофона и вывода. 0 ppm — конвертер работает ровно, темп и тембр не меняются.">
                Расхождение часов
              </span>
              <span class="tabular-nums">
                {{ Math.abs(stats.slewPpm) < 1 ? "0 ppm" : (stats.slewPpm > 0 ? "+" : "−") + Math.abs(stats.slewPpm).toFixed(0) + " ppm" }}
              </span>
            </div>
            <div class="mt-3 flex flex-wrap gap-2">
              <button :class="btn" @click="openLog">
                <AppIcon name="file" />
                Открыть лог
              </button>
              <button :class="btn" @click="showWizard = true">Мастер настройки</button>
            </div>
            <p :class="muted" class="mt-2">
              Лог пишется в data/app.log: старт/стоп движка, ошибки устройств, сбросы модели.
            </p>
          </section>

          <section :class="card">
            <div class="mb-2 text-[13px] font-medium text-[var(--muted)]">Модель и приложение</div>
            <p :class="muted">{{ modelStatus }}</p>
            <div class="mt-3 flex flex-wrap items-center gap-2">
              <button :class="btn" :disabled="checkingModel" @click="checkModel">
                {{ checkingModel ? "Проверяю…" : "Проверить обновления модели" }}
              </button>
            </div>
            <div class="mt-4" :class="settingRow">
              <AppLabel text="Запускать с Windows" hint="Автозапуск при входе в систему: приложение стартует свёрнутым в трей." />
              <AppSwitch v-model="autostart" @update:model-value="applyAutostart" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Свернуть в трей при закрытии" hint="Крестик прячет окно в трей, обработка при этом продолжает работать." />
              <AppSwitch v-model="settings.closeToTray" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Запускать свёрнутым" hint="При старте окно не показывается: приложение сразу живёт в трее и обрабатывает звук." />
              <AppSwitch v-model="settings.startMinimized" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Включать обработку сразу" hint="Как только виртуальный кабель доступен — обработка запускается без нажатия кнопки." />
              <AppSwitch v-model="settings.autoStart" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Следовать системному микрофону" hint="Если в Windows выбран другой микрофон по умолчанию, приложение переключится на него." />
              <AppSwitch v-model="settings.followDefaultInput" />
            </div>
            <div :class="settingRow">
              <AppLabel text="Хранить записей A/B" hint="Сколько последних пар хранить в data/recordings; старые удаляются." />
              <AppSlider v-model="settings.keepRecordings" :min="5" :max="100" :step="5" :format="(v) => `${v}`" />
            </div>

            <div class="my-3 h-px bg-[var(--stroke)]"></div>
            <div class="flex flex-wrap gap-2">
              <button :class="btn" @click="exportSettings">Экспорт настроек</button>
              <button :class="btn" @click="importSettings">Импорт настроек</button>
              <button :class="btn" class="hover:!text-[var(--danger)]" @click="resetSettings">
                Сбросить настройки
              </button>
            </div>
            <p :class="muted" class="mt-2">
              Экспорт кладёт файл в data/settings-export.json; для импорта положите его туда же.
            </p>
          </section>
        </div>
      </transition>
    </div>

    <div
      class="pointer-events-none fixed right-4 bottom-4 z-[95] flex w-[330px] max-w-[calc(100vw-32px)] flex-col gap-2"
    >
      <!-- an error stays until dismissed: it is a card, not a toast, but it lives
           in the same fixed column so it never moves the page -->
      <AppToast v-if="error" kind="warn" title="Ошибка" :text="error" @close="error = ''" />
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

    <!-- first-run wizard -->
    <div
      v-if="showWizard && settings"
      class="fixed inset-0 z-[90] grid place-items-center bg-black/45 p-5"
    >
      <div
        class="w-full max-w-[540px] rounded-2xl border border-[var(--stroke)] bg-[var(--surface)] p-6 shadow-2xl"
      >
        <div class="flex items-start justify-between gap-3">
          <div>
            <div class="text-[18px] font-semibold">Настройка за три шага</div>
            <p :class="muted" class="mt-1">
              Можно пропустить и вернуться позже: «Настройки» → «Мастер настройки».
            </p>
          </div>
          <button :class="btn" class="!px-2.5" title="Закрыть" @click="closeWizard">
            <AppIcon name="close" :size="14" />
          </button>
        </div>

        <ol class="mt-4 flex flex-col gap-4">
          <li class="flex gap-3">
            <span
              class="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-[var(--accent-soft)] text-[12px] font-semibold text-[var(--accent)]"
            >
              1
            </span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">Виртуальный микрофон</div>
              <p :class="muted" class="mt-0.5">
                {{
                  cableOk
                    ? "VB-CABLE найден — обработанный звук увидят Zoom, Meet и Discord."
                    : "Без него другие программы не увидят очищенный звук."
                }}
              </p>
              <button
                v-if="!cableOk"
                :class="btnPrimary"
                class="mt-2"
                :disabled="installingCable"
                @click="installCable"
              >
                <AppIcon v-if="installingCable" name="spinner" class="spin" />
                {{ installingCable ? "Установка…" : "Установить VB-CABLE" }}
              </button>
              <p v-if="cableMessage" :class="muted" class="mt-1">{{ cableMessage }}</p>
            </div>
          </li>

          <li class="flex gap-3">
            <span
              class="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-[var(--accent-soft)] text-[12px] font-semibold text-[var(--accent)]"
            >
              2
            </span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">Микрофон</div>
              <div class="mt-1.5">
                <AppSelect
                  :model-value="selectedInput"
                  :options="inputOptions"
                  placeholder="Нет устройств"
                  @update:model-value="pickInput"
                />
              </div>
              <button :class="btn" class="mt-2" :disabled="probing" @click="probeMic">
                <AppIcon name="mic" />
                {{
                  probing
                    ? probeLeft > 0
                      ? `Слушаю… ${probeLeft} с`
                      : "Слушаю…"
                    : "Проверить микрофон"
                }}
              </button>
              <p v-if="probeResult" :class="muted" class="mt-1">
                Уровень {{ probeResult.peakDb.toFixed(0) }} дБ — {{ probeHint }}
              </p>
            </div>
          </li>

          <li class="flex gap-3">
            <span
              class="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-[var(--accent-soft)] text-[12px] font-semibold text-[var(--accent)]"
            >
              3
            </span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">Профиль голоса</div>
              <p :class="muted" class="mt-0.5">
                Прочитайте любой текст 15 секунд в тишине — модель запомнит тембр.
              </p>
              <button :class="btnPrimary" class="mt-2" :disabled="recording" @click="recordProfile">
                <span v-if="recording" class="h-2.5 w-2.5 shrink-0 rounded-full bg-current" />
                <AppIcon v-else name="mic" />
                {{ recording ? `Запись… ${recordLeft} с` : "Записать 15 с" }}
              </button>
              <p v-if="profileQuality" :class="muted" class="mt-1">
                Оценка {{ profileQuality.score }}/100 · SNR {{ profileQuality.snrDb.toFixed(0) }} дБ
              </p>
            </div>
          </li>
        </ol>

        <div class="mt-5 flex items-center justify-end gap-2">
          <button :class="btn" @click="closeWizard">Пропустить</button>
          <button :class="btnPrimary" @click="closeWizard">Готово</button>
        </div>
      </div>
    </div>
  </div>
</template>
