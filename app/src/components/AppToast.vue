<script setup lang="ts">
import AppIcon from "./AppIcon.vue";

withDefaults(
  defineProps<{
    kind?: "info" | "warn" | "ok";
    title: string;
    text?: string;
  }>(),
  { kind: "info", text: "" },
);

const emit = defineEmits<{ close: [] }>();

/// The tint of the round icon: warning, success or neutral.
const tint = (kind: string): string => {
  if (kind === "warn") return "bg-[var(--danger-soft)] text-[var(--danger)]";
  if (kind === "ok") return "bg-[var(--ok-soft)] text-[var(--ok)]";
  return "bg-[var(--accent-soft)] text-[var(--accent)]";
};
</script>

<template>
  <div
    class="pointer-events-auto flex w-full items-start gap-2.5 rounded-2xl border border-[var(--stroke)] bg-[var(--surface)] p-3 shadow-[0_14px_34px_-18px_rgba(0,0,0,0.5)]"
    role="status"
  >
    <span class="grid h-6 w-6 shrink-0 place-items-center rounded-full" :class="tint(kind)">
      <AppIcon
        :name="kind === 'warn' ? 'alert' : kind === 'ok' ? 'check' : 'info'"
        :size="13"
        :stroke="2"
      />
    </span>
    <div class="min-w-0 flex-1 pt-px">
      <div class="text-[12.5px] leading-snug font-medium text-[var(--text)]">{{ title }}</div>
      <div v-if="text" class="mt-0.5 text-[12px] leading-snug text-[var(--muted)]">{{ text }}</div>
    </div>
    <button
      class="-mt-0.5 -mr-1 grid h-6 w-6 shrink-0 cursor-pointer place-items-center rounded-lg text-[var(--muted)] transition hover:bg-[var(--surface-alt)] hover:text-[var(--text)]"
      title="Закрыть"
      @click="emit('close')"
    >
      <AppIcon name="close" :size="12" />
    </button>
  </div>
</template>
