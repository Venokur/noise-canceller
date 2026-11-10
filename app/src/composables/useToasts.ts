// Toasts: at most three cards in the corner, duplicates refresh the timer
// instead of stacking a second copy (the engine watchdog fires repeatedly).
import { ref } from "vue";

import type { Toast, ToastKind } from "../types";

const TOAST_MS = 7000;

export function useToasts() {
  const toasts = ref<Toast[]>([]);
  const timers = new Map<number, number>();
  let seq = 0;

  function dismiss(id: number) {
    window.clearTimeout(timers.get(id));
    timers.delete(id);
    toasts.value = toasts.value.filter((t) => t.id !== id);
  }

  function notify(title: string, kind: ToastKind = "info", text = "") {
    const existing = toasts.value.find((t) => t.title === title && t.text === text);
    if (existing) {
      window.clearTimeout(timers.get(existing.id));
      timers.set(
        existing.id,
        window.setTimeout(() => dismiss(existing.id), TOAST_MS),
      );
      return;
    }
    const id = ++seq;
    const next = [...toasts.value, { id, title, text, kind }].slice(-3);
    for (const gone of toasts.value) {
      if (!next.some((t) => t.id === gone.id)) {
        window.clearTimeout(timers.get(gone.id));
        timers.delete(gone.id);
      }
    }
    toasts.value = next;
    timers.set(id, window.setTimeout(() => dismiss(id), TOAST_MS));
  }

  return { toasts, notify, dismissToast: dismiss };
}
