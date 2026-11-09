// node --test web/checks/i18n.test.mjs
// The translations (web/locales/*.js): every string the app can pass through
// tr() — the literals in its calls, the data the modules hand it (controls,
// tooltips, tutorial steps, styles, aspects) and index.html's own text —
// has an entry, and every entry is one of those, so a changed English source
// is a failing check rather than a silently English string.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { PATHS, GROUPS, FILM, PAPER } from "../help.js";
import { PREP_SECTIONS, SECTIONS, OUTPUT_SECTIONS, TITLES } from "../controls.js";
import { STEPS } from "../tutorial.js";
import { STYLES } from "../border.js";
import { ASPECTS } from "../crop.js";

const read = (name) => readFileSync(new URL(`../${name}`, import.meta.url), "utf8");
const sources = ["app.js", "i18n.js", "stocks.js", "tutorial.js", "samples.js", "preset-ui.js", "presets.js", "crop-editor.js"].map(read).join("\n");

// The strings a page can show, in English: what tr() is called with.
function english() {
  const strings = new Set();
  const add = (...texts) => { for (const text of texts) strings.add(text); };
  // tr("…") and tr(`…`) literals ({name} holes and all).
  for (const m of sources.matchAll(/\btr\(\s*(["'`])((?:[^\\]|\\.)*?)\1/g)) {
    if (m[2].includes("${")) continue; // a template built at runtime
    const text = m[2].replaceAll("\\'", "'").replaceAll('\\"', '"').replaceAll("\\`", "`").replaceAll("\\\\", "\\").replaceAll("\\n", "\n");
    strings.add(text);
  }
  // app.js's data, shown the same way: badge texts, busy labels, the foldout
  // titles (whose plurals stock the search's empty note) and their groups,
  // save()'s `what`s, updateReady's texts, the crash notes, Exposure's two
  // tooltips and i18n.js's CSS label.
  for (const m of sources.matchAll(/setBadge\("([^"]+)"\)/g)) strings.add(m[1]);
  for (const m of sources.matchAll(/busyLabel: "([^"]+)"/g)) strings.add(m[1]);
  // tr(cond ? "…" : "…") picks its string at runtime (badge texts, threads).
  for (const m of sources.matchAll(/\btr\(\s*[\w.>\s()]*?\?\s*"([^"]+)"\s*:\s*"([^"]+)"/g)) add(m[1], m[2]);
  add("Original", "Loading Original"); // updateOriginal's badge
  for (const m of sources.matchAll(/fold\("[^"]+", "([^"]+)"/g)) { add(m[1], `${m[1].toLowerCase()}s`); }
  for (const m of sources.matchAll(/return stockGroups\(\w+\(\), \[([^\]]+)\]/g)) {
    for (const title of m[1].matchAll(/"([^"]+)"/g)) add(title[1]);
  }
  for (const m of sources.matchAll(/what [=:] "([^"]+)"/g)) strings.add(m[1]);
  for (const m of sources.matchAll(/updateReady\((?:text = )?"([^"]+)"/g)) strings.add(m[1]);
  for (const m of sources.matchAll(/note = "([^"]+)"/g)) strings.add(m[1]);
  for (const m of sources.matchAll(/"((?:Brighter|Brightens)[^"]*)"/g)) strings.add(m[1]);
  for (const m of sources.matchAll(/dict\["([^"]+)"\]/g)) strings.add(m[1]);
  strings.add("Search films");
  // The modules' data: controls (with their option titles), tooltips,
  // tutorial steps, border styles, crop aspects.
  for (const section of [...PREP_SECTIONS, ...SECTIONS, ...OUTPUT_SECTIONS]) {
    strings.add(section.title);
    if (section.tab) strings.add(section.tab);
    if (section.footer) strings.add(section.footer);
    for (const c of section.controls ?? []) for (const label of [c.label, c.bw, c.help]) if (label) strings.add(label);
    for (const group of section.groups ?? []) {
      strings.add(group.title);
      for (const c of group.controls) for (const label of [c.label, c.bw, c.help]) if (label) strings.add(label);
    }
  }
  strings.add("Stock");
  for (const title of Object.values(TITLES)) strings.add(title);
  for (const text of [FILM, PAPER, ...Object.values(PATHS), ...Object.values(GROUPS)]) strings.add(text);
  for (const step of STEPS) for (const text of [step.title, step.text, step.hint]) if (text) strings.add(text);
  for (const style of STYLES) for (const text of [style.title, style.help]) strings.add(text);
  for (const [, title] of ASPECTS) strings.add(title);
  return strings;
}

// index.html's own text: text nodes and the translated attributes, without
// its styles or scripts. Symbols and brand names stay as they are.
function html() {
  const strings = new Set();
  const page = read("index.html").replace(/<(style|script)[\s\S]*?<\/\1>/g, "");
  for (const m of page.matchAll(/<(title|h1|h2|p|span|button|option|label|a|summary)\b[^>]*>([^<]+)</g)) {
    const text = m[2].replace(/&amp;/g, "&").trim();
    if (/[a-zA-Z]/.test(text) && text !== "Negs") strings.add(text);
  }
  for (const m of page.matchAll(/(?:title|aria-label|placeholder)="([^"]+)"/g)) {
    const text = m[1].trim();
    if (/[a-zA-Z]/.test(text) && text !== "Negs") strings.add(text);
  }
  return strings;
}

// A locale's dictionary: its script, run with a window to fill.
function dictionary(name) {
  const window = { NEGS_I18N: {} };
  new Function("window", read(`locales/${name}`))(window);
  assert.ok(window.NEGS_I18N.dict, `${name} fills window.NEGS_I18N.dict`);
  return window.NEGS_I18N.dict;
}

const locales = () => readdirSync(new URL("../locales", import.meta.url)).filter((f) => f.endsWith(".js"));
const notTranslated = new Set([ // brand, model and ratio names, shown as they are
  "WebGPU", "CPU", "English", "简体中文", "negs", "35mm", "16:9", "3:2 · 35mm", "4:3 · 645",
  "5:4 · 4×5", "7:5 · 5×7", "OKLCh", "OKLrab", "CAM16-UCS", "ACES RGC", "xy Chromaticity",
  "Black Pro-Mist", "Glimmerglass", "Pro-Mist", "CineBloom", "Hanatos 2025",
  "Arctic 2026 (Beta 04)", "Mallett 2019",
]);

test("every English string the app can show is translated", () => {
  const shown = [...english(), ...html()].filter((s) => !notTranslated.has(s));
  for (const name of locales()) {
    const missing = shown.filter((s) => !(s in dictionary(name)));
    assert.deepEqual(missing, [], `${name}: ${missing.length} strings`);
  }
});

test("every entry is a string the app can show", () => {
  const shown = new Set([...english(), ...html(), ...notTranslated]);
  for (const name of locales()) {
    const extra = Object.keys(dictionary(name)).filter((s) => !shown.has(s));
    assert.deepEqual(extra, [], `${name}: ${extra.length} unused entries`);
  }
});

test("placeholders match their English source", () => {
  const holes = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
  for (const name of locales()) {
    const wrong = Object.entries(dictionary(name)).filter(([source, text]) => holes(source) !== holes(text)).map(([source]) => source);
    assert.deepEqual(wrong, [], `${name}: ${wrong.length} mismatched`);
  }
});
