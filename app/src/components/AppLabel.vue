<script setup lang="ts">
import AppHint from "./AppHint.vue";

defineProps<{ text: string; hint?: string }>();
</script>

<template>
  <span class="flex min-w-0 items-center gap-1.5 text-[13px] text-[var(--text)]">
    <span class="truncate" v-tooltip="text">{{ text }}</span>
    <AppHint v-if="hint" :text="hint" />
  </span>
</template>
