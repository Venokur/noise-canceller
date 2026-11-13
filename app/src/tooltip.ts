// One tooltip layer for the whole window.
//
// The browser's native `title` is the system tooltip: it ignores the theme, the
// fonts and the radius, appears after a browser-defined delay, and on a
// container it is inherited by every child (`title` on the A/B card made the
// "normalize" switch and the waveform show the recordings path). This module
// replaces it with a single custom layer, driven by the `v-tooltip` directive.
import { shallowReactive } from "vue";

export const tooltipState = shallowReactive({
  open: false,
  text: "",
  target: null as HTMLElement | null,
});

const SHOW_DELAY_MS = 180;
let showTimer: number | undefined;

function clearShowTimer() {
  if (showTimer !== undefined) {
    window.clearTimeout(showTimer);
    showTimer = undefined;
  }
}

export function hideTooltip() {
  clearShowTimer();
  tooltipState.open = false;
  tooltipState.target = null;
  tooltipState.text = "";
}

export function showTooltip(text: string, target: HTMLElement, delay = SHOW_DELAY_MS) {
  const value = text.trim();
  if (!value) {
    hideTooltip();
    return;
  }
  // same target and text: keep the open tooltip, only restart a pending show
  if (tooltipState.open && tooltipState.target === target && tooltipState.text === value) {
    return;
  }
  clearShowTimer();
  showTimer = window.setTimeout(() => {
    showTimer = undefined;
    if (!target.isConnected) return;
    tooltipState.text = value;
    tooltipState.target = target;
    tooltipState.open = true;
  }, delay);
}

/// Called from the directive's `updated` hook: the text can be reactive (a
/// confirm state, a changing title, the current shortcut).
export function refreshTooltip(text: string, target: HTMLElement) {
  const value = text.trim();
  if (tooltipState.target !== target) return;
  if (!tooltipState.open) return;
  if (!value) {
    hideTooltip();
    return;
  }
  tooltipState.text = value;
}
