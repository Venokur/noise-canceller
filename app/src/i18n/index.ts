// The interface language: Russian and English.
//
// The project deliberately has no i18n library: two languages with ~150 short
// messages do not pay for a message compiler and a runtime. What matters here is
// the other way round — a missing key or an untranslated string must fail the
// build, not render as a blank label. `scripts/check-i18n.mjs` is that guard:
// key parity between the two files, equal placeholders, every `t("…")` used in
// `src/` exists, and no Cyrillic outside this folder.
//
// Keys are flat (`settings.voice.bodyHint`) so the type stays exact and the two
// files diff line by line.
import { readonly, ref } from "vue";

import en from "./en.json";
import ru from "./ru.json";

export type Lang = "ru" | "en";
export type MsgKey = keyof typeof ru;
export type Vars = Record<string, string | number>;

type Messages = Record<MsgKey, string>;

const messages: Record<Lang, Messages> = { ru, en };

/// Autonyms: the language picker is the one control that is never translated,
/// otherwise a user who cannot read the current language cannot find their way
/// back.
export const LANGUAGE_OPTIONS: { value: Lang; label: string }[] = [
  { value: "ru", label: "Русский" },
  { value: "en", label: "English" },
];

const locale = ref<Lang>("ru");

/// The active language, for formatting that needs a locale tag (timestamps).
export const currentLocale = readonly(locale);

export function isLang(value: string | null | undefined): value is Lang {
  return value === "ru" || value === "en";
}

/// Rust resolves the stored language before the window ever asks for settings,
/// so anything unexpected falls back to Russian rather than to a guess.
export function setLocale(value: string | null | undefined) {
  locale.value = value === "en" ? "en" : "ru";
  document.documentElement.lang = locale.value;
}

/// Reads `locale` during render, so a language switch re-renders every label
/// that went through `t()`.
export function t(key: MsgKey, vars?: Vars): string {
  const message = messages[locale.value][key] ?? messages.ru[key] ?? key;
  if (!vars) return message;
  return message.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}

/// Units live outside the messages: they are glued to numbers in formulas, and
/// a bare "дБ" left in an English window is exactly the kind of leak the check
/// script cannot see. Getters keep them reactive without unwrapping a ref in
/// every template.
export const u = {
  get db(): string {
    return locale.value === "en" ? "dB" : "дБ";
  },
  get ms(): string {
    return locale.value === "en" ? "ms" : "мс";
  },
  get khz(): string {
    return locale.value === "en" ? "kHz" : "кГц";
  },
  get hz(): string {
    return locale.value === "en" ? "Hz" : "Гц";
  },
  get s(): string {
    return locale.value === "en" ? "s" : "с";
  },
};
