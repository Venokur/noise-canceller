// Voice profiles: the list, the automatic pick, scoring and deletion.
//
// Split out of App.vue without changing behaviour. The only engine coupling is
// switching the profile of a running engine, so that comes in as `restart`.

import { computed, ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

import { shortName } from "../format";
import { t } from "../i18n";
import type { Quality, Settings, ProfileMeta, ProfileScore } from "../types";

interface ProfileDeps {
  settings: Ref<Settings | null>;
  error: Ref<string>;
  /// the microphone currently selected, to warn when the profile was recorded
  /// on a different one
  inputDevice: Ref<string>;
  restart: () => Promise<void>;
}

export function useProfiles({ settings, error, inputDevice, restart }: ProfileDeps) {
  const profiles = ref<string[]>([]);
  const selectedProfile = ref("");
  const profileQuality = ref<Quality | null>(null);
  const profileScores = ref<ProfileScore[]>([]);
  const profileInfo = ref<ProfileMeta | null>(null);

  async function loadProfileMeta() {
    const path = selectedProfile.value;
    if (!path) {
      profileInfo.value = null;
      return;
    }
    try {
      profileInfo.value = await invoke<ProfileMeta | null>("profile_meta", { path });
    } catch {
      profileInfo.value = null;
    }
  }

  watch(selectedProfile, () => void loadProfileMeta());

  async function refreshProfiles() {
    try {
      profiles.value = await invoke<string[]>("list_profiles");
    } catch (e) {
      // also runs as a template event handler (`autoProfile` switch), where a
      // rejected promise would otherwise be an unhandled rejection
      error.value = String(e);
      return;
    }
    try {
      profileScores.value = (await invoke<ProfileScore[] | null>("profile_scores")) ?? [];
    } catch {
      profileScores.value = [];
    }
    const previous = selectedProfile.value;
    // a file deleted outside the app must not stay selected: the engine would
    // try to load it on the next start
    if (selectedProfile.value && !profiles.value.includes(selectedProfile.value)) {
      selectedProfile.value = "";
      if (settings.value) settings.value.profilePath = null;
    }
    const best = [...profileScores.value].sort((a, b) => b.score - a.score)[0];
    // a profile bound to this microphone wins over "pick the best": a headset
    // enrolment does not describe a desk microphone
    const bound = settings.value?.profileByDevice?.[inputDevice.value];
    if (bound && profiles.value.includes(bound)) {
      selectedProfile.value = bound;
      if (settings.value) settings.value.profilePath = bound;
    } else if (settings.value?.autoProfile && best) {
      selectedProfile.value = best.path;
      settings.value.profilePath = best.path;
    } else if (!selectedProfile.value && settings.value?.profilePath) {
      // the file may have been deleted outside the app: a stale path made the
      // engine fail at start with "file not found"
      if (profiles.value.includes(settings.value.profilePath)) {
        selectedProfile.value = settings.value.profilePath;
      } else if (settings.value) {
        settings.value.profilePath = null;
      }
    } else if (!selectedProfile.value && profiles.value.length) {
      selectedProfile.value = profiles.value[0];
      if (settings.value) settings.value.profilePath = profiles.value[0];
    }
    // a different profile was selected by the best-score / device rule: the
    // quality figure belongs to the take that was just recorded, not to this one
    if (selectedProfile.value !== previous) profileQuality.value = null;
    await loadProfileMeta();
  }

  const profileOptions = computed(() =>
    profiles.value.map((path) => {
      const score = profileScores.value.find((s) => s.path === path);
      const name = score?.name?.trim() || shortName(path);
      return { value: path, label: score ? `${name} · ${score.score}/100` : name };
    }),
  );

  const activeProfileScore = computed(() => {
    const path = selectedProfile.value || settings.value?.profilePath || "";
    return profileScores.value.find((s) => s.path === path) ?? null;
  });

  const profileWeak = computed(
    () => !!activeProfileScore.value && activeProfileScore.value.score < 55,
  );

  /// A profile recorded on a headset does not match a desk microphone: the
  /// model stops recognising the same voice, and the user blames the app.
  const profileMismatch = computed(() => {
    const info = profileInfo.value;
    const device = inputDevice.value;
    if (!info?.device || !device) return false;
    return info.device !== device;
  });

  /// Score and SNR of the profile in use, shown next to the section title.
  /// The score already sits in the picker label, so only the SNR is repeated
  /// here unless the profile was recorded seconds ago.
  const profileMeta = computed(() => {
    if (profileQuality.value) {
      const q = profileQuality.value;
      return t("profile.meta", {
        score: q.score,
        speech: (q.speechPercent * 100).toFixed(0),
        snr: q.snrDb.toFixed(0),
      });
    }
    const score = activeProfileScore.value;
    if (score) return t("profile.snr", { snr: score.snrDb.toFixed(0) });
    return "";
  });

  async function pickProfile() {
    if (!settings.value || !selectedProfile.value) return;
    settings.value.profilePath = selectedProfile.value;
    profileQuality.value = null;
    try {
      await restart();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function renameProfile(name: string) {
    if (!selectedProfile.value || !name.trim()) return;
    try {
      const meta = await invoke<ProfileMeta>("rename_profile", {
        path: selectedProfile.value,
        name,
      });
      profileInfo.value = meta;
      const score = profileScores.value.find((s) => s.path === selectedProfile.value);
      if (score) score.name = meta.name;
    } catch (e) {
      error.value = String(e);
    }
  }

  async function deleteProfile() {
    if (!selectedProfile.value) return;
    try {
      const removed = selectedProfile.value;
      await invoke("delete_profile", { path: removed });
      if (settings.value?.profileByDevice) {
        for (const [device, path] of Object.entries(settings.value.profileByDevice)) {
          if (path === removed) delete settings.value.profileByDevice[device];
        }
      }
      selectedProfile.value = "";
      profileQuality.value = null;
      profileInfo.value = null;
      if (settings.value) settings.value.profilePath = null;
      await refreshProfiles();
      // the running engine still holds the embeddings of the deleted file
      await restart();
    } catch (e) {
      error.value = String(e);
    }
  }

  return {
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
    loadProfileMeta,
    refreshProfiles,
    pickProfile,
    renameProfile,
    deleteProfile,
  };
}
