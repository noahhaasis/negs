// node --test web/checks/help.test.mjs
// The inspector's tooltips (web/help.js): the Mac's texts word for word
// (macos/Sources/NegsApp/ControlHelp.swift), and one behind every control's (i).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { PATHS, GROUPS } from "../help.js";
import { PREP_SECTIONS, SECTIONS, OUTPUT_SECTIONS } from "../controls.js";

const swift = readFileSync(new URL("../../macos/Sources/NegsApp/ControlHelp.swift", import.meta.url), "utf8");
function swiftTable(name) {
  const body = swift.match(new RegExp(`private static let ${name}: \\[String: String\\] = \\[([\\s\\S]*?)\\n    \\]`))[1];
  return Object.fromEntries([...body.matchAll(/^\s*"([^"]+)": "(.*)",?$/gm)].map((m) => [m[1], m[2]]));
}

test("the Mac's tooltips, word for word", () => {
  for (const [table, web] of [["paths", PATHS], ["groups", GROUPS]]) {
    const mac = swiftTable(table);
    assert.ok(Object.keys(mac).length > 10, `parsed ControlHelp.${table}`);
    for (const [key, text] of Object.entries(mac)) assert.equal(web[key], text, `${table}: ${key}`);
  }
});

test("every control and group has a tooltip", () => {
  const missing = [];
  for (const section of [...PREP_SECTIONS, ...SECTIONS, ...OUTPUT_SECTIONS]) {
    for (const c of section.controls ?? []) {
      // The match's note is text, not a control; Exposure's comes from app.js.
      if (c.kind === "note" || c.kind === "offset") continue;
      if (!c.help && !PATHS[c.path]) missing.push(`${section.title}: ${c.label}`);
    }
    for (const group of section.groups ?? []) {
      if (!GROUPS[group.title]) missing.push(`group ${group.title}`);
      for (const c of group.controls) if (!c.help && !PATHS[c.path]) missing.push(`${group.title}: ${c.label}`);
    }
  }
  assert.deepEqual(missing, []);
});
