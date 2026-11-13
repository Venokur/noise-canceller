// Shared DTO shapes for the Tauri commands.
//
// The Rust side is the source of truth: `Settings` lives in
// `core/src/settings.rs`, the rest are `Serialize` structs in `src-tauri`.
// Keep these in sync when a command changes — a missing field used to be
// invisible because nothing type-checks `.vue` files.

export interface Settings {
  enabled: boolean;
  theme: string;
  /** "ru" or "en": resolved from Windows on the first launch, then the user's */
  language: string;
  mode: string;
  profilePath: string | null;
  profileByDevice: Record<string, string>;
  isolationStrength: number;
  maxSuppressionDb: number;
  inputDevice: string;
  outputDevice: string;
  /** -1 mixes all capture channels, 0.. picks one */
  inputChannel: number;
  inputLeveling: boolean;
  residualDenoise: string;
  residualOrder: string;
  residualMix: number;
  vadGate: boolean;
  voiceIsolation: boolean;
  speakerCheck: boolean;
  speakerGuard: boolean;
  speakerThreshold: number;
  highPassEnabled: boolean;
  highPassHz: number;
  toneEnabled: boolean;
  toneLowDb: number;
  toneHighDb: number;
  deessEnabled: boolean;
  deessDb: number;
  airAmount: number;
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
  defaultsRev: number;
}

export interface Stats {
  inputRms: number;
  outputRms: number;
  rtf: number;
  latencyMs: number;
  bufferMs: number;
  underruns: number;
  inRate: number;
  inChannels: number;
  outRate: number;
  overflows: number;
  captureDrops: number;
  backend: string;
  running: boolean;
  spectrum: number[];
  suppressionDb: number;
  speech: boolean;
  inputPeakDb: number;
  outputPeakDb: number;
  muted: boolean;
  guardResets: number;
  levelerGainDb: number;
  slewPpm: number;
  threads: number;
  speakerConfidence: number;
  lufs: number;
  healthy: boolean;
}

export interface SettingsResult {
  settings: Settings;
  /** non-fatal: saved, but the mute shortcut could not be registered */
  warning: string | null;
}

export interface Quality {
  duration: number;
  speechPercent: number;
  snrDb: number;
  peakDb: number;
  clipPercent: number;
  score: number;
}

export interface Profile {
  path: string;
  quality: Quality;
}

/** Sidecar of a profile: which microphone and when it was recorded. */
export interface ProfileMeta {
  name: string;
  device: string;
  created: number;
  score: number;
  snrDb: number;
  appVersion: string;
}

export interface AbStatus {
  running: boolean;
  progress: number;
  folder: string | null;
  raw: string | null;
  clean: string | null;
}

export interface LastAb {
  folder: string;
  raw: string;
  clean: string;
}

export interface AbEntry {
  stamp: number;
  raw: string;
  clean: string;
}

export interface ProfileScore {
  path: string;
  name: string;
  device: string;
  score: number;
  snrDb: number;
}

export interface AbMetrics {
  speechLevelDb: number;
  levelDeltaDb: number;
  pauseContrastDb: number;
  longestDropoutS: number;
}

/** Similarity of the raw take's speech windows to the enrolled profile. */
export interface SpeakerSimilarity {
  windows: number;
  min: number;
  median: number;
  max: number;
}

export interface SystemInfo {
  logicalCores: number;
  physicalCores: number;
  recommendedThreads: number;
}

export type ToastKind = "info" | "warn" | "ok";

export interface Toast {
  id: number;
  title: string;
  text: string;
  kind: ToastKind;
}
