// Installing VB-CABLE and waiting for the device to appear.
//
// The installer returns as soon as it starts, so the only honest signal is the
// device list. The wait is bounded: if the user closes the installer without
// installing, the block must not stay on "Установка…" forever.

import { onUnmounted, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface CableDeps {
  refreshDevices: () => Promise<void>;
  cableOk: Ref<boolean>;
  error: Ref<string>;
}

export function useCable({ refreshDevices, cableOk, error }: CableDeps) {
  const installingCable = ref(false);
  const cableMessage = ref("");
  let timer: number | undefined;

  function stopPolling() {
    if (timer) {
      window.clearInterval(timer);
      timer = undefined;
    }
  }

  onUnmounted(stopPolling);

  async function installCable() {
    if (installingCable.value) return;
    installingCable.value = true;
    cableMessage.value = "Скачиваю драйвер…";
    error.value = "";
    try {
      await invoke<string>("install_cable");
      cableMessage.value = "Установщик запущен. Нажмите «Install Driver» и разрешите установку.";
      const started = Date.now();
      stopPolling();
      timer = window.setInterval(async () => {
        await refreshDevices();
        if (cableOk.value) {
          stopPolling();
          cableMessage.value = "Готово: виртуальный микрофон найден.";
          installingCable.value = false;
          return;
        }
        // the installer can be closed without installing: stop waiting instead
        // of leaving the block in "installing" forever
        if (Date.now() - started > 150_000) {
          stopPolling();
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

  return { installingCable, cableMessage, installCable };
}
