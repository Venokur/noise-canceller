<script setup lang="ts">
import AppIcon from "./AppIcon.vue";

defineProps<{ text: string }>();
</script>

<template>
  <span class="group relative inline-flex align-middle">
    <button
      type="button"
      class="grid h-[15px] w-[15px] cursor-help place-items-center rounded-full text-[var(--muted)] opacity-60 transition-colors hover:text-[var(--accent)] hover:opacity-100 focus:text-[var(--accent)] focus:opacity-100 focus:outline-none"
      :aria-label="text"
      @click.stop
    >
      <AppIcon name="help" :size="13" :stroke="1.9" />
    </button>
    <span
      role="tooltip"
      class="pointer-events-none absolute top-full left-0 z-[70] mt-2 w-[268px] rounded-[10px] border border-[var(--stroke)] bg-[var(--tip-bg)] px-3 py-2 text-[12px] leading-snug font-normal text-[var(--tip-text)] opacity-0 shadow-[0_14px_34px_-16px_rgba(0,0,0,0.6)] transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100"
    >
      {{ text }}
    </span>
  </span>
</template>
