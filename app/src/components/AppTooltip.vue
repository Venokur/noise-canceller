<script setup lang="ts">
// The singleton bubble behind `v-tooltip`. It lives in the body, is positioned
// in viewport coordinates and flips above the trigger when there is no room
// below: the old per-component bubble was clipped by the scrolling column.
import { nextTick, onUnmounted, ref, watch } from "vue";

import { hideTooltip, tooltipState } from "../tooltip";

const tip = ref<HTMLElement | null>(null);
const style = ref<Record<string, string>>({});
let resizeBound = false;

const GAP = 8;
const MARGIN = 8;

function place(target: HTMLElement, width: number, height: number) {
  const rect = target.getBoundingClientRect();
  let top = rect.top - height - GAP;
  let placement = "top";
  if (top < MARGIN) {
    placement = "bottom";
    top = rect.bottom + GAP;
  }
  if (top + height > window.innerHeight - MARGIN) {
    top = Math.max(MARGIN, window.innerHeight - height - MARGIN);
  }
  let left = rect.left + rect.width / 2 - width / 2;
  left = Math.min(Math.max(MARGIN, left), Math.max(MARGIN, window.innerWidth - width - MARGIN));
  style.value = {
    top: `${Math.round(top)}px`,
    left: `${Math.round(left)}px`,
  };
  tip.value?.setAttribute("data-placement", placement);
}

function update() {
  const target = tooltipState.target;
  if (!target || !target.isConnected || !tip.value) {
    hideTooltip();
    return;
  }
  // the first frame uses the element's own width as an estimate, then the real
  // bubble size is measured; either way it never flashes at (0, 0)
  place(target, tip.value.offsetWidth || 220, tip.value.offsetHeight || 44);
}

function onResize() {
  update();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") hideTooltip();
}

function onScroll() {
  // native tooltips disappear when the page moves; a bubble pinned to a moving
  // row would point at the wrong place between the scroll and the next frame
  hideTooltip();
}

function onWindowBlur() {
  hideTooltip();
}

watch(
  () => [tooltipState.open, tooltipState.target, tooltipState.text] as const,
  async ([open]) => {
    if (!open) {
      if (resizeBound) {
        window.removeEventListener("resize", onResize);
        resizeBound = false;
      }
      return;
    }
    // a rough position before the first paint, the measured one right after
    const target = tooltipState.target;
    if (target) place(target, 240, 44);
    await nextTick();
    update();
    if (!resizeBound) {
      window.addEventListener("resize", onResize);
      resizeBound = true;
    }
  },
);

window.addEventListener("keydown", onKeydown, true);
window.addEventListener("scroll", onScroll, true);
window.addEventListener("blur", onWindowBlur);

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown, true);
  window.removeEventListener("scroll", onScroll, true);
  window.removeEventListener("blur", onWindowBlur);
  window.removeEventListener("resize", onResize);
});
</script>

<template>
  <Teleport to="body">
    <transition name="tip">
      <div
        v-if="tooltipState.open"
        ref="tip"
        role="tooltip"
        :style="style"
        class="pointer-events-none fixed z-[120] w-max max-w-[280px] rounded-[9px] border border-[var(--stroke)] bg-[var(--tip-bg)] px-3 py-2 text-[12px] leading-snug font-normal text-[var(--tip-text)] break-words shadow-[0_14px_34px_-16px_rgba(0,0,0,0.6)] [overflow-wrap:anywhere]"
      >
        {{ tooltipState.text }}
      </div>
    </transition>
  </Teleport>
</template>
