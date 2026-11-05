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
    class="flex h-9 shrink-0 select-none items-center gap-1 pr-1.5 pl-3"
    @mousedown="startDrag"
  >
    <div class="flex flex-1 items-center gap-2">
      <span class="h-2 w-2 rounded-full bg-[var(--accent)]"></span>
      <span class="text-[12px] tracking-wide text-[var(--muted)]">Noise Canceller</span>
    </div>

    <button class="winbtn" title="Свернуть" @mousedown.stop @click.stop="minimize">
      <AppIcon name="minus" :size="13" :stroke="1.5" />
    </button>
    <button class="winbtn" title="Развернуть" @mousedown.stop @click.stop="maximize">
      <AppIcon name="maximize" :size="12" :stroke="1.5" />
    </button>
    <button class="winbtn close" title="Закрыть" @mousedown.stop @click.stop="close">
      <AppIcon name="close" :size="13" :stroke="1.5" />
    </button>
  </div>
</template>
