// What App.vue hands to the two screens.
//
// The shell owns the state, the timers and the engine calls; a view only renders
// that state and calls these functions. Keeping the contract in one injected
// object avoided threading eighty props through two levels and kept the split
// from touching the logic.
//
// The index signature is intentionally loose: there is no `vue-tsc` in this repo
// (TypeScript 7 is not supported by it yet), so precise signatures here would be
// documentation that nothing checks. `settings`/`stats` are typed because they
// are the two refs a view reads the most.
import type { InjectionKey, Ref } from "vue";

import type { Settings, Stats } from "./types";

export interface AppContext {
  settings: Ref<Settings | null>;
  stats: Ref<Stats>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [key: string]: any;
}

export const APP_CONTEXT: InjectionKey<AppContext> = Symbol("app-context");
