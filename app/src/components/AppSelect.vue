<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import AppIcon from "./AppIcon.vue";

interface Option {
  value: string;
  label: string;
}

const props = defineProps<{
  modelValue: string;
  options: Option[];
  placeholder?: string;
  disabled?: boolean;
}>();
const emit = defineEmits<{ (event: "update:modelValue", value: string): void }>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);

watch(
  () => props.disabled,
  (value) => {
    if (value) open.value = false;
  },
);

const label = computed(
  () => props.options.find((option) => option.value === props.modelValue)?.label ?? props.placeholder ?? "—"
);

function choose(value: string) {
  emit("update:modelValue", value);
  open.value = false;
}

function onDocumentClick(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) open.value = false;
}

onMounted(() => document.addEventListener("mousedown", onDocumentClick));
onUnmounted(() => document.removeEventListener("mousedown", onDocumentClick));
</script>

<template>
  <div ref="root" class="relative">
    <button
      type="button"
      :disabled="disabled"
      class="flex w-full cursor-pointer items-center justify-between gap-2 rounded-xl border border-[var(--stroke)] bg-[var(--surface-alt)] px-3 py-[9px] text-left text-[13px] text-[var(--text)] transition hover:border-[var(--accent)] disabled:cursor-default disabled:opacity-55 disabled:hover:border-[var(--stroke)]"
      @click="open = !open"
    >
      <span class="truncate">{{ label }}</span>
      <AppIcon
        name="chevron"
        :size="15"
        class="opacity-60 transition-transform duration-200"
        :class="{ 'rotate-180': open }"
      />
    </button>

    <transition name="pop">
      <div
        v-if="open"
        class="absolute z-50 mt-1.5 max-h-64 w-full overflow-y-auto rounded-xl border border-[var(--stroke)] bg-[var(--surface)] p-1 shadow-2xl"
      >
        <button
          v-for="option in options"
          :key="option.value"
          type="button"
          class="block w-full cursor-pointer truncate rounded-lg px-2.5 py-1.5 text-left text-[13px] transition hover:bg-[var(--surface-alt)]"
          :class="option.value === modelValue ? 'font-medium text-[var(--accent)]' : 'text-[var(--text)]'"
          @click="choose(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
    </transition>
  </div>
</template>
