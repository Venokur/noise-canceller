<script setup lang="ts">
import AppIcon from "./AppIcon.vue";
import { t } from "../i18n";

withDefaults(
  defineProps<{
    kind?: "info" | "warn" | "ok";
    title: string;
    text?: string;
  }>(),
  { kind: "info", text: "" },
);

const emit = defineEmits<{ close: [] }>();

/// Colour lives in the leading dot only: the card itself stays neutral, so a
/// stack of notices does not turn into a traffic light.
const tint = (kind: string): string => {
  if (kind === "warn") return "bg-[var(--danger)]";
  if (kind === "ok") return "bg-[var(--accent)]";
  return "bg-[var(--muted)]";
};
</script>

<template>
  <div
    class="pointer-events-auto flex w-full items-start gap-2.5 rounded-xl border border-[var(--stroke)] bg-[var(--surface)] px-3 py-2.5 shadow-[0_16px_36px_-22px_rgba(0,0,0,0.7)]"
    role="status"
  >
    <span class="mt-[5px] h-[6px] w-[6px] shrink-0 rounded-full" :class="tint(kind)"></span>
    <div class="min-w-0 flex-1">
      <div class="text-[12.5px] leading-snug font-medium text-[var(--text)] break-words [overflow-wrap:anywhere]">
        {{ title }}
      </div>
      <div
        v-if="text"
        class="mt-0.5 text-[11.5px] leading-snug text-[var(--muted)] break-words [overflow-wrap:anywhere]"
      >
        {{ text }}
      </div>
    </div>
    <button
      class="-mt-0.5 -mr-1 grid h-6 w-6 shrink-0 cursor-pointer place-items-center rounded-lg text-[var(--muted)] transition-colors hover:bg-[var(--surface-alt)] hover:text-[var(--text)]"
      :title="t('common.close')"
      @click="emit('close')"
    >
      <AppIcon name="close" :size="12" />
    </button>
  </div>
</template>
