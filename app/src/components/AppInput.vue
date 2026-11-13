<script setup lang="ts">
// Text fields had three copies of the same native <input> classes. This is a
// thin project-owned wrapper: attributes (placeholder, maxlength, class,
// keydown listeners) pass through to the input, so focus, IME and selection
// keep their native behaviour.
import { useAttrs } from "vue";

defineOptions({ inheritAttrs: false });

defineProps<{
  modelValue: string;
  type?: "text" | "search";
}>();

const emit = defineEmits<{ (event: "update:modelValue", value: string): void }>();

const attrs = useAttrs();

function onInput(event: Event) {
  emit("update:modelValue", (event.target as HTMLInputElement).value);
}
</script>

<template>
  <input
    v-bind="attrs"
    :type="type ?? 'text'"
    :value="modelValue"
    spellcheck="false"
    autocomplete="off"
    class="min-w-0"
    @input="onInput"
  />
</template>
