// Voice profiles: the list, the automatic pick, scoring and deletion.
//
// Split out of App.vue without changing behaviour. The only engine coupling is
// switching the profile of a running engine, so that comes in as `restart`.

import { computed, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

import { shortName } from "../format";
import type { Quality, Settings, ProfileScore } from "../types";

interface ProfileDeps {
  settings: Ref<Settings | null>;
  error: Ref<string>;
  restart: () => Promise<void>;
}

export function useProfiles({ settings, error, restart }: ProfileDeps) {
  const profiles = ref<string[]>([]);
  const selectedProfile = ref("");
  const profileQuality = ref<Quality | null>(null);
  const profileScores = ref<ProfileScore[]>([]);

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
    }),
  );

  const activeProfileScore = computed(() => {
    const path = selectedProfile.value || settings.value?.profilePath || "";
    return profileScores.value.find((s) => s.path === path) ?? null;
  });

  const profileWeak = computed(
    () => !!activeProfileScore.value && activeProfileScore.value.score < 55,
  );

  /// Score and SNR of the profile in use, shown next to the section title.
  /// The score already sits in the picker label, so only the SNR is repeated
  /// here unless the profile was recorded seconds ago.
  const profileMeta = computed(() => {
    if (profileQuality.value) {
      const q = profileQuality.value;
      return `${q.score} / 100 · речь ${(q.speechPercent * 100).toFixed(0)}% · SNR ${q.snrDb.toFixed(0)} дБ`;
    }
    const score = activeProfileScore.value;
    if (score) return `SNR ${score.snrDb.toFixed(0)} дБ`;
    return "";
  });

  async function pickProfile() {
    if (!settings.value || !selectedProfile.value) return;
    settings.value.profilePath = selectedProfile.value;
    profileQuality.value = null;
    await restart();
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

  return {
    profiles,
    selectedProfile,
    profileQuality,
    profileScores,
    profileOptions,
    activeProfileScore,
    profileWeak,
    profileMeta,
    refreshProfiles,
    pickProfile,
    deleteProfile,
  };
}
