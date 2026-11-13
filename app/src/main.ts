import { createApp } from "vue";

import App from "./App.vue";
import Osd from "./Osd.vue";
import "./style.css";

// The mute overlay is the same bundle in a second, transparent window. The hash
// is set by the Rust side when the window is created.
const osd = location.hash === "#osd";
if (osd) {
  document.documentElement.dataset.window = "osd";
}

createApp(osd ? Osd : App).mount("#app");
