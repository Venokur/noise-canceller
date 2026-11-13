// Guards the two languages.
//
// There is no `vue-tsc` here and Vite strips types without checking them, so a
// typo in a key or a forgotten translation would render as an empty label and
// nobody would notice until a user did. This script fails the build instead:
//
//   1. `ru.json` and `en.json` carry exactly the same keys, no empty values;
//   2. both messages of a key use the same `{placeholders}`;
//   3. every `t("…")` in `src/` names a key that exists;
//   4. no Cyrillic is left outside `src/i18n/` — a hard-coded string cannot be
//      translated later, and finding it here is cheaper than finding it in a
//      screenshot of an English window. A line that genuinely needs Cyrillic
//      can opt out with an `i18n-ignore` comment.
//
// It is a heuristic, like `check-ui.mjs`: a false positive is fixed in the
// source, not by widening the rules.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(app, "src");
const i18n = join(src, "i18n");

const problems = [];
const fail = (message) => problems.push(message);

const ru = JSON.parse(readFileSync(join(i18n, "ru.json"), "utf8"));
const en = JSON.parse(readFileSync(join(i18n, "en.json"), "utf8"));

// 1. parity ----------------------------------------------------------------
const ruKeys = Object.keys(ru).sort();
const enKeys = Object.keys(en).sort();
for (const key of ruKeys) {
  if (!(key in en)) fail(`en.json is missing "${key}"`);
}
for (const key of enKeys) {
  if (!(key in ru)) fail(`ru.json has no counterpart for "${key}"`);
}
for (const [file, messages] of [["ru", ru], ["en", en]]) {
  for (const [key, value] of Object.entries(messages)) {
    if (typeof value !== "string" || !value.trim()) fail(`${file}.json: "${key}" is empty`);
  }
}

// 2. placeholders ----------------------------------------------------------
const placeholders = (message) =>
  [...message.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
for (const key of ruKeys) {
  if (!(key in en)) continue;
  const a = placeholders(ru[key]);
  const b = placeholders(en[key]);
  if (a !== b) fail(`"${key}": ru uses {${a}}, en uses {${b}}`);
}

// 3. keys used in the code -------------------------------------------------
function walk(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (path !== i18n) files.push(...walk(path));
    } else if (/\.(vue|ts)$/.test(entry.name)) {
      files.push(path);
    }
  }
  return files;
}

const files = walk(src);
const used = new Set();
for (const file of files) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(/\bt\(\s*["']([\w.]+)["']/g)) used.add(match[1]);
  for (const match of text.matchAll(/\{\{\s*["']([\w.]+)["']/g)) used.add(match[1]);
}
for (const key of used) {
  if (!(key in ru)) fail(`t("${key}") has no message`);
}

// 4. no untranslated text --------------------------------------------------
const CYRILLIC = /[\u0400-\u04FF]/;
for (const file of files) {
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  lines.forEach((line, index) => {
    if (line.includes("i18n-ignore")) return;
    if (CYRILLIC.test(line)) {
      fail(`${relative(app, file)}:${index + 1} has hard-coded Cyrillic: ${line.trim().slice(0, 80)}`);
    }
  });
}

// unused keys are not a failure, they are just dead weight
const unused = ruKeys.filter((key) => !used.has(key));

if (unused.length) {
  console.log(`i18n: ${ruKeys.length} messages, ${used.size} used`);
  console.log(`unused (fine, but check they are not leftovers): ${unused.join(", ")}`);
}

if (problems.length) {
  console.error(`i18n: ${problems.length} problem(s)`);
  for (const problem of problems) console.error("  " + problem);
  process.exit(1);
}

console.log(`i18n: ok, ${ruKeys.length} messages in ru/en`);
