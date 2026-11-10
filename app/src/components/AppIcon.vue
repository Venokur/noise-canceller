<script setup lang="ts">
import {
  Activity,
  AudioLines,
  Cable,
  ChevronDown,
  ChevronRight,
  Circle,
  CircleCheck,
  CircleHelp,
  FileText,
  Gauge,
  Headphones,
  Info,
  Loader2,
  Mic,
  Minus,
  Moon,
  Play,
  Power,
  RefreshCw,
  Search,
  Settings2,
  SlidersHorizontal,
  Square,
  Sun,
  Trash2,
  TriangleAlert,
  Volume2,
  X,
} from "@lucide/vue";

/// One place for every glyph in the app: Lucide 24-grid icons, drawn at the
/// same optical weight and a constant stroke whatever the rendered size.
const icons = {
  audio: AudioLines,
  activity: Activity,
  cable: Cable,
  gauge: Gauge,
  volume: Volume2,
  settings: Settings2,
  search: Search,
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
  chevronRight: ChevronRight,
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
