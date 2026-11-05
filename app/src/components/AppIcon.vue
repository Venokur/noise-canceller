<script setup lang="ts">
import {
  ChevronDown,
  CircleCheck,
  FileText,
  Info,
  SlidersHorizontal,
  Circle,
  CircleHelp,
  Headphones,
  Loader2,
  Mic,
  Minus,
  Moon,
  Play,
  Power,
  RefreshCw,
  Square,
  Sun,
  Trash2,
  TriangleAlert,
  X,
} from "@lucide/vue";

/// One place for every glyph in the app: Lucide 24-grid icons, drawn at the
/// same optical weight and a constant stroke whatever the rendered size.
const icons = {
  sun: Sun,
  moon: Moon,
  power: Power,
  refresh: RefreshCw,
  mic: Mic,
  record: Circle,
  play: Play,
  stop: Square,
  headphones: Headphones,
  trash: Trash2,
  chevron: ChevronDown,
  help: CircleHelp,
  file: FileText,
  sliders: SlidersHorizontal,
  minus: Minus,
  maximize: Square,
  close: X,
  spinner: Loader2,
  alert: TriangleAlert,
  info: Info,
  check: CircleCheck,
} as const;

withDefaults(
  defineProps<{
    name: keyof typeof icons;
    size?: number;
    stroke?: number;
  }>(),
  { size: 16, stroke: 1.8 },
);
</script>

<template>
  <component
    :is="icons[name]"
    :size="size"
    :stroke-width="stroke"
    :non-scaling-stroke="true"
    class="shrink-0"
    aria-hidden="true"
  />
</template>
