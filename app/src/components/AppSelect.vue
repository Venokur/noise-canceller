<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from "vue";
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
const panel = ref<HTMLElement | null>(null);
const panelStyle = ref<Record<string, string>>({});
const listId = `select-${useId()}`;
const optionId = (index: number) => `${listId}-${index}`;

watch(
  () => props.disabled,
  (value) => {
    // `closeList` also removes the scroll/resize listeners; setting `open`
    // directly left them attached for the life of the component
    if (value) closeList();
  },
);

const label = computed(
  () => props.options.find((option) => option.value === props.modelValue)?.label ?? props.placeholder ?? "—"
);

function choose(value: string) {
  emit("update:modelValue", value);
  closeList(true);
}

/// The list is teleported to the body and positioned in viewport coordinates.
/// The old absolute panel lived inside `.app-scroll` (`overflow-y: auto`), so
/// a select near the bottom of a section had its options clipped: the last
/// entries were simply unreachable.
function updatePosition() {
  const anchor = trigger.value;
  if (!anchor) return;
  const rect = anchor.getBoundingClientRect();
  const margin = 6;
  const roomBelow = window.innerHeight - rect.bottom - margin;
  const roomAbove = rect.top - margin;
  const desired = Math.min(256, Math.max(42, props.options.length * 30 + 8));
  const openUp = roomBelow < Math.min(desired, 120) && roomAbove > roomBelow;
  const maxHeight = Math.max(48, Math.min(desired, openUp ? roomAbove : roomBelow));
  const top = openUp
    ? Math.max(margin, rect.top - maxHeight - 6)
    : Math.min(window.innerHeight - margin, rect.bottom + 6);
  panelStyle.value = {
    top: `${top}px`,
    left: `${Math.max(margin, Math.min(rect.left, window.innerWidth - rect.width - margin))}px`,
    width: `${rect.width}px`,
    maxHeight: `${maxHeight}px`,
  };
}

function addWindowListeners() {
  window.addEventListener("scroll", updatePosition, true);
  window.addEventListener("resize", updatePosition);
}

function removeWindowListeners() {
  window.removeEventListener("scroll", updatePosition, true);
  window.removeEventListener("resize", updatePosition);
}

function openList() {
  if (props.disabled || !props.options.length) return;
  const current = props.options.findIndex((option) => option.value === props.modelValue);
  active.value = current >= 0 ? current : 0;
  // position before the first paint: the panel must not flash at (0, 0)
  updatePosition();
  open.value = true;
  addWindowListeners();
  void nextTick(updatePosition);
}

function closeList(focusTrigger = false) {
  open.value = false;
  active.value = -1;
  removeWindowListeners();
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
  const target = event.target as Node;
  if (root.value?.contains(target) || panel.value?.contains(target)) return;
  closeList();
}

watch(
  () => props.options,
  () => {
    if (open.value) void nextTick(updatePosition);
  },
  { deep: true },
);

onMounted(() => document.addEventListener("mousedown", onDocumentClick));
onUnmounted(() => {
  document.removeEventListener("mousedown", onDocumentClick);
  removeWindowListeners();
});
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

    <Teleport to="body">
      <transition name="pop">
        <div
          v-if="open"
          :id="listId"
          ref="panel"
          role="listbox"
          :aria-label="placeholder"
          :style="panelStyle"
          class="fixed z-[100] overflow-y-auto rounded-[10px] border border-[var(--stroke)] bg-[var(--surface)] p-1 shadow-[0_18px_40px_-20px_rgba(0,0,0,0.65)]"
          @mousedown.prevent
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
    </Teleport>
  </div>
</template>
