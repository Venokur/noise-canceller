// The two screens are not type-checked: there is no `vue-tsc` here (TypeScript 7
// is not supported by it yet), and `AppContext` is an open object, so a name can
// be used in a template and simply render nothing. That is exactly how the
// "Потоки модели" block lost `autoThreads`/`maxThreads`/`sysInfo`/`slowConfig`
// when App.vue was split into two screens.
//
// This script is the cheap guard for that class of mistake:
//   1. every name destructured from `inject(APP_CONTEXT)` must be provided;
//   2. every identifier used in the template of a screen must be declared in its
//      `<script setup>` (or be a provided key, a v-for alias, an arrow parameter
//      or a browser/JS global).
//
// It is intentionally a heuristic, not a parser. False negatives are possible;
// a false positive is fixed by declaring the name where it belongs, not by
// widening the whitelist.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..", "src");

const GLOBALS = new Set([
  "true", "false", "null", "undefined", "NaN", "Infinity",
  "Math", "Number", "String", "Boolean", "Object", "Array", "JSON", "Date", "Promise",
  "window", "document", "console", "event", "$event", "this", "new", "typeof", "in", "of",
  "if", "else", "return", "void", "delete", "instanceof", "keyof", "as", "is",
]);

function scriptOf(file) {
  const text = readFileSync(file, "utf8");
  const match = text.match(/<script setup[^>]*>([\s\S]*?)<\/script>/);
  return { text, script: match ? match[1] : "", template: text.slice(match ? match.index + match[0].length : 0) };
}

function declarations(script) {
  const names = new Set();
  for (const m of script.matchAll(/\b(?:const|let|var|function|class)\s+([A-Za-z_$][\w$]*)/g)) {
    names.add(m[1]);
  }
  // destructuring: const { a, b } = ..., const [a, b] = ...
  for (const m of script.matchAll(/\b(?:const|let|var)\s*\{([^}]*)\}\s*=/g)) {
    for (const part of m[1].split(",")) {
      const name = part.split(":").pop().trim().replace(/=.*$/, "").trim();
      if (/^[A-Za-z_$][\w$]*$/.test(name)) names.add(name);
    }
  }
  for (const m of script.matchAll(/import\s+(?:type\s+)?\{([^}]*)\}\s+from/g)) {
    for (const part of m[1].split(",")) {
      const name = part.split(/\s+as\s+/).pop().trim();
      if (/^[A-Za-z_$][\w$]*$/.test(name)) names.add(name);
    }
  }
  for (const m of script.matchAll(/import\s+([A-Za-z_$][\w$]*)\s+from/g)) {
    names.add(m[1]);
  }
  // props declared with defineProps<{...}>() are exposed to the template
  for (const m of script.matchAll(/defineProps<\{([\s\S]*?)\}>/g)) {
    for (const prop of m[1].matchAll(/^\s*([A-Za-z_$][\w$]*)\??\s*:/gm)) {
      names.add(prop[1]);
    }
  }
  return names;
}

function provideKeys(appVue) {
  const text = readFileSync(appVue, "utf8");
  const start = text.indexOf("provide(APP_CONTEXT, {");
  if (start < 0) throw new Error("App.vue has no provide(APP_CONTEXT, …)");
  const end = text.indexOf("});", start);
  const keys = new Set();
  for (const m of text.slice(start, end).matchAll(/^\s{2}([A-Za-z_$][\w$]*),?$/gm)) {
    keys.add(m[1]);
  }
  return keys;
}

function templateIdentifiers(template) {
  const withoutComments = template.replace(/<!--[\s\S]*?-->/g, " ");
  const expressions = [];
  for (const m of withoutComments.matchAll(/\{\{([\s\S]*?)\}\}/g)) expressions.push(m[1]);
  for (const m of withoutComments.matchAll(/(?:^|\s)(?::|v-|@)[\w-]*="([^"]*)"/g)) expressions.push(m[1]);
  const joined = expressions.join("\n");
  // drop string literals, template literals and `as Type`
  const cleaned = joined
    .replace(/`[^`]*`/g, " ")
    .replace(/'[^']*'/g, " ")
    .replace(/"[^"]*"/g, " ")
    .replace(/\bas\s+[A-Za-z_$][\w$]*/g, " ");

  const aliases = new Set();
  for (const m of withoutComments.matchAll(/v-for="\(?([^)"]*?)\)?\s+in\s/g)) {
    for (const part of m[1].split(",")) {
      const name = part.trim();
      if (/^[A-Za-z_$][\w$]*$/.test(name)) aliases.add(name);
    }
  }
  const params = new Set();
  for (const m of cleaned.matchAll(/(?:\(([^()]*)\)|([A-Za-z_$][\w$]*))\s*=>/g)) {
    const list = (m[1] ?? m[2] ?? "").split(",");
    for (const part of list) {
      const name = part.trim();
      if (/^[A-Za-z_$][\w$]*$/.test(name)) params.add(name);
    }
  }

  const used = new Set();
  for (const m of cleaned.matchAll(/(?<![\w$.])([A-Za-z_$][\w$]*)\s*(?![\w$]*\s*:)/g)) {
    used.add(m[1]);
  }
  for (const name of aliases) used.delete(name);
  for (const name of params) used.delete(name);
  for (const name of GLOBALS) used.delete(name);
  return used;
}

/// An attribute line belongs to the tag that opened above it. A bare
/// `label="…"` left outside the tag is rendered as text: that is exactly what
/// happened when the slider labels were inserted for the accessible name.
function attributesOutsideTags(file, text) {
  const problems = [];
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    if (!/^\s*(?:label|:label|aria-label|aria-valuetext)="/.test(line)) return;
    // Walk up until the nearest angle bracket: if a tag is still open, the
    // attribute belongs to it; if the previous tag already closed, it is text.
    let inside = false;
    for (let j = index - 1; j >= 0 && j >= index - 40; j--) {
      const open = lines[j].lastIndexOf("<");
      const close = lines[j].lastIndexOf(">");
      if (open < 0 && close < 0) continue;
      inside = open > close;
      break;
    }
    if (!inside) {
      problems.push(`${file}:${index + 1}: ${line.trim()} is outside an open tag`);
    }
  });
  return problems;
}

const appVue = join(app, "App.vue");
const provided = provideKeys(appVue);

const views = readdirSync(join(app, "views"))
  .filter((name) => name.endsWith(".vue"))
  .map((name) => join(app, "views", name));

const problems = [];

for (const file of [appVue, ...views]) {
  const { script, template } = scriptOf(file);
  const declared = declarations(script);
  const destructured = new Set();

  for (const m of script.matchAll(/const\s*\{([^}]*)\}\s*=\s*inject\(APP_CONTEXT\)/g)) {
    for (const part of m[1].split(",")) {
      const name = part.split(":").pop().trim();
      if (!name) continue;
      destructured.add(name);
      if (!provided.has(name)) {
        problems.push(`${file}: ${name} is destructured from APP_CONTEXT but App.vue does not provide it`);
      }
    }
  }

  if (!/inject\(APP_CONTEXT\)/.test(script) && file !== appVue) continue;

  // A view must destructure what it uses: being provided is not enough, and
  // using it without destructuring is exactly the bug this script exists for.
  const missing = [...templateIdentifiers(template)].filter(
    (name) => !declared.has(name) && !destructured.has(name),
  );
  if (missing.length) {
    problems.push(`${file}: template uses undeclared ${missing.join(", ")}`);
  }
  problems.push(...attributesOutsideTags(file, template));
}

if (problems.length) {
  for (const problem of problems) console.error(`check-ui: ${problem}`);
  process.exit(1);
}
console.log(`check-ui: ${views.length + 1} screens OK, ${provided.size} provided names`);
