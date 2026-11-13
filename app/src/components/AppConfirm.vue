<script setup lang="ts">
// In-app confirmation.
//
// `window.confirm` renders an OS dialog: it ignores the theme, the fonts and the
// radius of everything else, and on Windows it can be moved behind the main
// window. This is the same card the rest of the app uses, with the same rules:
// neutral colours, a single primary action, Escape cancels and focus starts on
// the safe button.

import { onMounted, onUnmounted, ref } from "vue";
import AppIcon from "./AppIcon.vue";
import { t } from "../i18n";

const props = withDefaults(
  defineProps<{
    title: string;
    text?: string;
    confirmLabel?: string;
    cancelLabel?: string;
    /// Destroying something should not be one Enter away: focus starts on cancel.
    focus?: "cancel" | "confirm";
  }>(),
  {
    text: "",
    confirmLabel: "",
    cancelLabel: "",
    focus: "cancel",
  },
);

const emit = defineEmits<{ confirm: []; cancel: [] }>();

const root = ref<HTMLElement | null>(null);
const cancelButton = ref<HTMLButtonElement | null>(null);
const confirmButton = ref<HTMLButtonElement | null>(null);

let previousFocus: HTMLElement | null = null;

onMounted(() => {
  previousFocus = document.activeElement as HTMLElement | null;
  if (props.focus === "confirm") confirmButton.value?.focus();
  else cancelButton.value?.focus();
});

onUnmounted(() => {
  // the dialog closed by a click or Escape: the keyboard user has to land back
  // on the control that opened it, not at the top of the page
  previousFocus?.focus?.();
});

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("cancel");
    return;
  }
  if (event.key !== "Tab" || !root.value) return;
  const items = Array.from(root.value.querySelectorAll<HTMLElement>("button:not([disabled])")).filter(
    (el) => el.offsetParent !== null,
  );
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
</script>

<template>
  <div class="fixed inset-0 z-[96] grid place-items-center bg-black/45 p-5">
    <div
      ref="root"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
      :aria-describedby="text ? 'confirm-text' : undefined"
      class="w-full max-w-[400px] rounded-[14px] border border-[var(--stroke)] bg-[var(--surface)] p-5 shadow-[0_30px_70px_-30px_rgba(0,0,0,0.7)]"
      @keydown="onKeydown"
    >
      <div class="flex items-start gap-3">
        <span
          class="mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-full bg-[var(--surface-alt)] text-[var(--muted)]"
        >
          <AppIcon name="help" :size="15" />
        </span>
        <div class="min-w-0">
          <h2 id="confirm-title" class="text-[15px] font-semibold tracking-[-0.01em]">{{ title }}</h2>
          <p v-if="text" id="confirm-text" class="mt-1 text-[12.5px] leading-relaxed text-[var(--muted)]">
            {{ text }}
          </p>
        </div>
      </div>

      <div class="mt-5 flex justify-end gap-2">
        <button ref="cancelButton" type="button" class="btn btn-ghost" @click="emit('cancel')">
          {{ cancelLabel || t("common.cancel") }}
        </button>
        <button ref="confirmButton" type="button" class="btn btn-primary" @click="emit('confirm')">
          {{ confirmLabel || t("common.continue") }}
        </button>
      </div>
    </div>
  </div>
</template>
