<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, useId, watch } from "vue";
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
const active = ref(-1);
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const listId = `select-${useId()}`;
const optionId = (index: number) => `${listId}-${index}`;

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
  closeList(true);
}

function openList() {
  if (props.disabled) return;
  const current = props.options.findIndex((option) => option.value === props.modelValue);
  active.value = current >= 0 ? current : 0;
  open.value = true;
}

function closeList(focusTrigger = false) {
  open.value = false;
  active.value = -1;
  if (focusTrigger) trigger.value?.focus();
}

function move(delta: number) {
  if (!props.options.length) return;
  if (!open.value) {
    openList();
    return;
  }
  active.value = (active.value + delta + props.options.length) % props.options.length;
}

/// A listbox without a keyboard is a mouse-only control: arrows move, Home/End
/// jump, Enter takes the highlighted item, Escape closes and returns focus.
function onKeydown(event: KeyboardEvent) {
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      move(1);
      break;
    case "ArrowUp":
      event.preventDefault();
      move(-1);
      break;
    case "Home":
      if (!open.value) return;
      event.preventDefault();
      active.value = 0;
      break;
    case "End":
      if (!open.value) return;
      event.preventDefault();
      active.value = props.options.length - 1;
      break;
    case "Enter":
      if (!open.value) return;
      event.preventDefault();
      if (active.value >= 0 && active.value < props.options.length) {
        choose(props.options[active.value].value);
      }
      break;
    case "Escape":
      if (!open.value) return;
      event.preventDefault();
      closeList(true);
      break;
    case "Tab":
      if (open.value) closeList();
      break;
  }
}

function onDocumentClick(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) closeList();
}

onMounted(() => document.addEventListener("mousedown", onDocumentClick));
onUnmounted(() => document.removeEventListener("mousedown", onDocumentClick));
</script>

<template>
  <div ref="root" class="relative min-w-0">
    <button
      ref="trigger"
      type="button"
      :disabled="disabled"
      class="flex h-[34px] w-full cursor-pointer items-center justify-between gap-2.5 rounded-[9px] border border-[var(--stroke)] bg-[var(--surface)] px-2.5 text-left text-[13px] text-[var(--text)] transition-colors hover:border-[var(--muted)] disabled:cursor-default disabled:opacity-50 disabled:hover:border-[var(--stroke)]"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="open ? listId : undefined"
      :aria-activedescendant="open && active >= 0 ? optionId(active) : undefined"
      @click="open ? closeList() : openList()"
      @keydown="onKeydown"
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
        :id="listId"
        role="listbox"
        :aria-label="placeholder"
        class="absolute z-50 mt-1.5 max-h-64 w-full overflow-y-auto rounded-[10px] border border-[var(--stroke)] bg-[var(--surface)] p-1 shadow-[0_18px_40px_-20px_rgba(0,0,0,0.65)]"
      >
        <button
          v-for="(option, index) in options"
          :id="optionId(index)"
          :key="option.value"
          type="button"
          role="option"
          tabindex="-1"
          :aria-selected="option.value === modelValue"
          class="block h-[30px] w-full cursor-pointer truncate rounded-[7px] px-2.5 text-left text-[13px] transition-colors hover:bg-[var(--surface-alt)]"
          :class="[
            option.value === modelValue ? 'font-medium text-[var(--accent)]' : 'text-[var(--text)]',
            index === active ? 'bg-[var(--surface-alt)]' : '',
          ]"
          @click="choose(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
    </transition>
  </div>
</template>
