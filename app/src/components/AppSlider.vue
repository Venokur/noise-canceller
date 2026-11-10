<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    modelValue: number;
    min: number;
    max: number;
    step?: number;
    format?: (value: number) => string;
  }>(),
  { step: 1 }
);

const emit = defineEmits<{ (event: "update:modelValue", value: number): void }>();

const percent = computed(() => {
  const span = props.max - props.min || 1;
  return Math.max(0, Math.min(100, ((props.modelValue - props.min) / span) * 100));
});

const text = computed(() =>
  props.format ? props.format(props.modelValue) : props.modelValue.toString()
);

function onInput(event: Event) {
  emit("update:modelValue", Number((event.target as HTMLInputElement).value));
}
</script>

<template>
  <div class="flex w-full items-center gap-2.5">
    <input
      class="slider"
      type="range"
      :min="min"
      :max="max"
      :step="step"
      :value="modelValue"
      :style="{
        background: `linear-gradient(to right, var(--accent) 0%, var(--accent) ${percent}%, var(--track) ${percent}%, var(--track) 100%)`,
      }"
      @input="onInput"
    />
    <span class="w-[58px] shrink-0 text-right font-mono text-[11px] tabular-nums text-[var(--muted)]">{{ text }}</span>
  </div>
</template>
