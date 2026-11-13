import { createApp } from "vue";

import App from "./App.vue";
import Osd from "./Osd.vue";
import { vTooltip } from "./directives/tooltip";
import "./style.css";

// The mute overlay is the same bundle in a second, transparent window. The hash
// is set by the Rust side when the window is created.
const osd = location.hash === "#osd";
if (osd) {
  document.documentElement.dataset.window = "osd";
}

const app = createApp(osd ? Osd : App);
// one custom tooltip layer for every `v-tooltip`, instead of the native `title`
app.directive("tooltip", vTooltip);
app.mount("#app");
