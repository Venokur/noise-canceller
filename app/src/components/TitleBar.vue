<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import AppIcon from "./AppIcon.vue";

function minimize() {
  invoke("window_minimize");
}

function maximize() {
  invoke("window_toggle_maximize");
}

function close() {
  invoke("window_close");
}

function startDrag(event: MouseEvent) {
  if (event.button !== 0) return;
  invoke("window_drag");
}
</script>

<template>
  <div
    class="grid h-10 shrink-0 select-none items-center gap-4 border-b border-[var(--stroke)] pr-1.5 pl-3"
    style="grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr)"
    @mousedown="startDrag"
  >
    <div class="flex min-w-0 items-center gap-2.5">
      <AppIcon name="audio" :size="18" :stroke="1.9" class="text-[var(--accent)]" />
      <slot name="status" />
    </div>

    <div class="flex h-full items-center gap-5">
      <slot name="nav" />
    </div>

    <div class="flex items-center justify-end gap-1">
      <slot name="actions" />
      <span class="mx-1 h-4 w-px bg-[var(--stroke)]"></span>
      <button class="winbtn" title="Свернуть" @mousedown.stop @click.stop="minimize">
        <AppIcon name="minus" :size="12" :stroke="1.6" />
      </button>
      <button class="winbtn" title="Развернуть" @mousedown.stop @click.stop="maximize">
        <AppIcon name="maximize" :size="10" :stroke="1.6" />
      </button>
      <button class="winbtn close" title="Закрыть" @mousedown.stop @click.stop="close">
        <AppIcon name="close" :size="12" :stroke="1.6" />
      </button>
    </div>
  </div>
</template>
