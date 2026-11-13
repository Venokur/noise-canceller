<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import AppIcon from "./components/AppIcon.vue";
import { isLang, setLocale, t } from "./i18n";

const muted = ref(false);
let unlisten: UnlistenFn | null = null;
let unlistenLocale: UnlistenFn | null = null;

onMounted(async () => {
  try {
    const settings = await invoke<{ language: string }>("get_settings");
    if (isLang(settings.language)) setLocale(settings.language);
  } catch {
    /* the OSD is cosmetic, keep the default language */
  }
  try {
    unlisten = await listen<boolean>("mute-changed", (event) => {
      muted.value = event.payload;
    });
    // the main window owns the language switch; this second webview has to be
    // told, or it keeps its startup language until the app restarts
    unlistenLocale = await listen<string>("locale-changed", (event) => {
      if (isLang(event.payload)) setLocale(event.payload);
    });
  } catch {
    /* no event bridge: the overlay simply does not update */
  }
});

onUnmounted(() => {
  unlisten?.();
  unlistenLocale?.();
});
</script>

<template>
  <div
    class="flex h-full w-full items-center gap-3 rounded-[12px] border px-4"
    :class="muted ? 'border-[var(--danger)]' : 'border-[var(--signal-line)]'"
    style="background: color-mix(in srgb, var(--surface) 94%, transparent)"
  >
    <span
      class="grid h-9 w-9 shrink-0 place-items-center rounded-full"
      :class="muted ? 'bg-[var(--danger-soft)] text-[var(--danger)]' : 'bg-[var(--accent-soft)] text-[var(--accent)]'"
    >
      <AppIcon :name="muted ? 'micOff' : 'mic'" :size="18" :stroke="1.8" />
    </span>
    <div class="min-w-0">
      <div class="truncate text-[13px] font-semibold">{{ muted ? t("osd.muted") : t("osd.live") }}</div>
      <div class="truncate text-[11px] text-[var(--muted)]">
        {{ muted ? t("osd.mutedHint") : t("osd.liveHint") }}
      </div>
    </div>
  </div>
</template>
