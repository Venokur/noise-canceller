// What the machine offers, so the thread picker can explain itself instead of
// showing a bare 1–8 slider.
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Ref } from "vue";

import type { Settings, SystemInfo } from "../types";

export function useSystemInfo(settings: Ref<Settings | null>) {
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

  return { sysInfo, maxThreads, autoThreads, slowConfig, loadSystemInfo };
}
