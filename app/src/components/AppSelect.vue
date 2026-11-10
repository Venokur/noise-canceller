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
  <div ref="root" class="relative min-w-0">
    <button
      type="button"
      :disabled="disabled"
      class="flex h-[34px] w-full cursor-pointer items-center justify-between gap-2.5 rounded-[9px] border border-[var(--stroke)] bg-[var(--surface)] px-2.5 text-left text-[13px] text-[var(--text)] transition-colors hover:border-[var(--muted)] disabled:cursor-default disabled:opacity-50 disabled:hover:border-[var(--stroke)]"
      @click="open = !open"
    >
      <span class="truncate">{{ label }}</span>
      <AppIcon
        name="chevron"
        :size="14"
        class="text-[var(--muted)] transition-transform duration-200"
        :class="{ 'rotate-180': open }"
      />
    </button>

    <transition name="pop">
      <div
        v-if="open"
        class="absolute z-50 mt-1.5 max-h-64 w-full overflow-y-auto rounded-[10px] border border-[var(--stroke)] bg-[var(--surface)] p-1 shadow-[0_18px_40px_-20px_rgba(0,0,0,0.65)]"
      >
        <button
          v-for="option in options"
          :key="option.value"
          type="button"
          class="block h-[30px] w-full cursor-pointer truncate rounded-[7px] px-2.5 text-left text-[13px] transition-colors hover:bg-[var(--surface-alt)]"
          :class="option.value === modelValue ? 'font-medium text-[var(--accent)]' : 'text-[var(--text)]'"
          @click="choose(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
    </transition>
  </div>
</template>
