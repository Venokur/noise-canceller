<script setup lang="ts">
defineProps<{ modelValue: boolean; disabled?: boolean }>();
const emit = defineEmits<{ (event: "update:modelValue", value: boolean): void }>();
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="modelValue"
    :disabled="disabled"
    class="flex h-5 w-[34px] shrink-0 cursor-pointer items-center rounded-full border px-[2px] transition-colors duration-200 disabled:cursor-default disabled:opacity-40"
    :class="
      modelValue
        ? 'border-[var(--accent)] bg-[var(--accent)]'
        : 'border-[var(--stroke)] bg-[var(--switch-off)]'
    "
    @click="emit('update:modelValue', !modelValue)"
  >
    <span
      class="h-3.5 w-3.5 shrink-0 rounded-full transition-transform duration-200"
      :class="modelValue ? 'translate-x-[14px] bg-[var(--knob)]' : 'translate-x-0 bg-[var(--muted)]'"
    ></span>
  </button>
</template>
