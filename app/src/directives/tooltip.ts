import type { Directive } from "vue";

import { hideTooltip, refreshTooltip, showTooltip, tooltipState } from "../tooltip";

interface Handlers {
  enter: () => void;
  leave: () => void;
  focus: () => void;
  blur: () => void;
  down: () => void;
}

const handlers = new WeakMap<HTMLElement, Handlers>();
const texts = new WeakMap<HTMLElement, string>();

function textOf(el: HTMLElement): string {
  return texts.get(el) ?? "";
}

function attach(el: HTMLElement) {
  const enter = () => showTooltip(textOf(el), el);
  const leave = () => hideTooltip();
  // `focus-visible` keeps a mouse click from re-opening a tooltip after the
  // pointer pressed the button; keyboard focus still gets it immediately.
  const focus = () => {
    if (el.matches(":focus-visible")) showTooltip(textOf(el), el, 0);
  };
  const blur = () => hideTooltip();
  const down = () => hideTooltip();

  el.addEventListener("mouseenter", enter);
  el.addEventListener("mouseleave", leave);
  el.addEventListener("focus", focus);
  el.addEventListener("blur", blur);
  el.addEventListener("mousedown", down);
  handlers.set(el, { enter, leave, focus, blur, down });
}

function detach(el: HTMLElement) {
  const bound = handlers.get(el);
  if (!bound) return;
  el.removeEventListener("mouseenter", bound.enter);
  el.removeEventListener("mouseleave", bound.leave);
  el.removeEventListener("focus", bound.focus);
  el.removeEventListener("blur", bound.blur);
  el.removeEventListener("mousedown", bound.down);
  handlers.delete(el);
  texts.delete(el);
}

/// `v-tooltip="text"` — a replacement for the browser's native `title`.
/// Works on any element, shows on hover and keyboard focus, hides on leave,
/// blur, press, Escape, scroll and resize, and never inherits to children.
export const vTooltip: Directive<HTMLElement, string | null | undefined> = {
  mounted(el, binding) {
    texts.set(el, String(binding.value ?? ""));
    attach(el);
  },
  updated(el, binding) {
    const value = String(binding.value ?? "");
    texts.set(el, value);
    if (tooltipState.target === el) refreshTooltip(value, el);
  },
  unmounted(el) {
    if (tooltipState.target === el) hideTooltip();
    detach(el);
  },
};
