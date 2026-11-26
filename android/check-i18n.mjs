// node android/check-i18n.mjs
// The Android app's translations (I18n.kt), as web/checks/i18n.test.mjs
// checks the web's: every string the app passes through tr() — the literals
// in its calls and the data it hands it (controls, option titles, glasses,
// border styles, crop aspects, stock groups, tabs) — has an entry in the
// web's dictionary (web/locales/<lang>.js) or the app's own
// (app/src/main/assets/locales/<lang>.android.jsonc); the app's own entries
// are each still used and not already the web's; and {name} holes match.
import { readFileSync, readdirSync } from "node:fs";

const here = new URL(".", import.meta.url);
const read = (path) => readFileSync(new URL(path, here), "utf8");
const sourceDir = "app/src/main/java/dev/negs/app/";
const files = [...readdirSync(new URL(sourceDir, here)).filter((f) => f.endsWith(".kt")),
  ...readdirSync(new URL(`${sourceDir}ui/`, here)).filter((f) => f.endsWith(".kt")).map((f) => `ui/${f}`)];
const source = Object.fromEntries(files.map((f) => [f, read(sourceDir + f)]));
const all = Object.values(source).join("\n");

// I18n.kt's parse(): the JSON strings of the file, outside // comments, in pairs.
function parse(text) {
  const strings = [];
  for (let i = 0; i < text.length;) {
    if (text.startsWith("//", i)) { const end = text.indexOf("\n", i); i = end < 0 ? text.length : end; continue; }
    if (text[i] === '"') {
      let j = i + 1;
      while (text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
      strings.push(JSON.parse(text.slice(i, j + 1)));
      i = j + 1;
      continue;
    }
    i++;
  }
  if (strings.length % 2) throw new Error("an odd number of strings");
  const pairs = [];
  for (let k = 0; k < strings.length; k += 2) pairs.push([strings[k], strings[k + 1]]);
  return pairs;
}
// A Kotlin string literal's text ("$x" templates are left for the caller to refuse).
const unquote = (literal) => JSON.parse(literal.replace(/\\\$/g, "$"));
const LITERAL = /"(?:[^"\\\n]|\\.)*"/g;

function english() {
  const strings = new Set();
  const add = (...texts) => texts.forEach((t) => strings.add(t));
  const templated = [];
  // tr("…"), and tr(if (…) "…" else "…").
  for (const m of all.matchAll(/\btr\(\s*("(?:[^"\\\n]|\\.)*")/g)) add(unquote(m[1]));
  for (const m of all.matchAll(/\btr\(if \([^)]*\) ("(?:[^"\\\n]|\\.)*") else ("(?:[^"\\\n]|\\.)*")/g)) add(unquote(m[1]), unquote(m[2]));
  for (const text of strings) if (/\$[{\w]/.test(text)) templated.push(text);
  // Controls.kt: labels, B&W lanes, section and group titles, tabs, footers,
  // option titles and the glasses' effects.
  const controls = source["Controls.kt"];
  for (const m of controls.matchAll(/\b(?:Slider|Toggle|Choice|MatchSwitch|ExposureOffset|MatchNote|Action|StockLane|ColorFilters|BorderControl|Group|Section)\(\s*"([^"]+)"/g)) add(m[1]);
  for (const m of controls.matchAll(/\bSection\("[^"]+", "([^"]+)"/g)) add(m[1]);
  for (const m of controls.matchAll(/\bStockLane\(.*, "([^"]+)"\),?$/gm)) add(m[1]);
  for (const m of controls.matchAll(/footer = "([^"]+)"/g)) add(m[1]);
  const block = (text, name) => text.slice(text.indexOf(name), text.indexOf(")\n", text.indexOf(name)));
  for (const m of block(controls, "val TITLES").matchAll(/"[^"]+" to ("[^"]+")/g)) add(unquote(m[1]));
  for (const m of block(controls, "val COLOR_FILTER_EFFECTS").matchAll(/"[^"]+" to ("[^"]+")/g)) add(unquote(m[1]));
  // The glasses' names, from the engine's catalog (spektrafilm-core color_filters.rs `colour`).
  add("Green", "Yellow-green", "Yellow", "Orange", "Red");
  // Border styles, crop aspects, stock groups, the tabs Inspector adds.
  for (const m of source["Border.kt"].matchAll(/^\s*\w+\("[^"]+", "([^"]+)",/gm)) add(m[1]);
  for (const m of block(source["Crop.kt"], "ASPECTS").matchAll(/"[^"]+" to ("[^"]+")/g)) add(unquote(m[1]));
  for (const m of source["Catalog.kt"].matchAll(/return listOf\(([^)]*)\)/g)) for (const t of m[1].matchAll(LITERAL)) add(unquote(t[0]));
  add("Stock", "Presets");
  return { strings, templated };
}

// Names and symbols every language shows as they are.
const KEPT = new Set(["Black Pro-Mist", "Glimmerglass", "Pro-Mist", "CineBloom", "OKLCh", "OKLrab", "CAM16-UCS", "ACES RGC",
  "Hanatos 2025", "Arctic 2026 (Beta 04)", "Mallett 2019", "35mm", "16:9", "3:2 · 35mm", "4:3 · 645", "5:4 · 4×5", "7:5 · 5×7"]);
const holes = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
let failed = false;
const fail = (message) => { failed = true; console.error(message); };
const { strings, templated } = english();
for (const t of templated) fail(`tr() of a template, use {holes}: ${JSON.stringify(t)}`);

for (const name of readdirSync(new URL("app/src/main/assets/locales/", here))) {
  const lang = name.replace(/\.android\.jsonc$/, "");
  globalThis.window = { NEGS_I18N: {} };
  // eslint-disable-next-line no-eval
  (0, eval)(read(`../web/locales/${lang}.js`));
  const web = window.NEGS_I18N.dict;
  const own = new Map();
  for (const [en, translated] of parse(read(`app/src/main/assets/locales/${name}`))) {
    if (own.has(en)) fail(`${name}: twice: ${JSON.stringify(en)}`);
    own.set(en, translated);
  }
  for (const [en, translated] of own) {
    if (!strings.has(en)) fail(`${name}: not in the sources: ${JSON.stringify(en)}`);
    if (en in web) fail(`${name}: the web's already: ${JSON.stringify(en)}`);
    if (holes(en) !== holes(translated)) fail(`${name}: holes differ: ${JSON.stringify(en)}`);
  }
  const missing = [...strings].filter((s) => !own.has(s) && !(s in web) && !KEPT.has(s));
  for (const s of missing) fail(`${lang}: no translation: ${JSON.stringify(s)}`);
  console.log(`${lang}: ${strings.size} strings, ${strings.size - missing.length} translated (${own.size} the app's own)`);
}
process.exit(failed ? 1 : 0);
