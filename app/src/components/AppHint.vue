<script setup lang="ts">
// The help icon. The bubble itself is the shared `v-tooltip` layer, so it has
// the same theme, delay, flip and clamping as every other hint in the window.
import AppIcon from "./AppIcon.vue";

defineProps<{ text: string }>();
</script>

<template>
  <span class="inline-flex align-middle">
    <button
      type="button"
      class="grid h-[15px] w-[15px] cursor-help place-items-center rounded-full text-[var(--muted)] opacity-60 transition-colors hover:text-[var(--accent)] hover:opacity-100 focus:text-[var(--accent)] focus:opacity-100"
      :aria-label="text"
      v-tooltip="text"
      @click.stop
    >
      <AppIcon name="help" :size="13" :stroke="1.9" />
    </button>
  </span>
</template>
