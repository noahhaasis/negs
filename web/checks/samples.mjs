// node web/checks/samples.mjs [optional-static-package-directory]
// Asset/provenance and offline-budget checks; RAW development is checked
// separately with the native rawler golden_inputs/bounded_decode harnesses.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { asset, OFFLINE_SAMPLE, role, serviceWorker } from "../offline.mjs";
import { jpegs } from "../placeholder.js";

const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = process.argv[2] ? resolve(process.argv[2]) : web;
const catalog = JSON.parse(readFileSync(join(root, "sample/catalog.json")));
assert.equal(catalog.samples.length, 7);
assert.equal(new Set(catalog.samples.map((s) => s.id)).size, 7);
assert.equal(`sample/${catalog.offline}.dng`, OFFLINE_SAMPLE);
assert.equal(catalog.default, catalog.offline);

for (const s of catalog.samples) {
  const bytes = readFileSync(join(root, s.url));
  assert.equal(bytes.length, s.bytes, s.id);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), s.sha256, s.id);
  assert.ok(jpegs(bytes).some((p) => p.width >= 1024 && p.height >= 683), `${s.id}: embedded preview`);
  assert.ok(s.credit && s.originalFilename);
  assert.equal(s.title, undefined, "samples must not have descriptive names");
  if (s.id === catalog.default) {
    assert.equal(role(s.url), "use", "default downloads on demand, not at install");
  } else {
    assert.equal(role(s.url), null, "additional photos must never enter save-all or runtime SW cache");
    assert.equal(Math.max(s.width, s.height), 2560);
    assert.ok(s.bytes < 1024 * 1024, `${s.id}: under 1 MiB`);
  }
  if (s.source === "signature-edits") assert.ok(s.sourceUrl && s.licenseUrl);
}
assert.equal(role("sample/make-dng.py"), null);
assert.equal(role("samples.js"), "install", "the picker code must work offline");
const assets = readdirSync(join(root, "sample")).map((f) => asset(`sample/${f}`, join(root, "sample", f)));
const source = serviceWorker(readFileSync(join(web, "sw.js"), "utf8"), assets);
const build = JSON.parse(source.match(/\/\*BUILD\*\/ (.*?) \/\*END\*\//)[1]);
assert.deepEqual(Object.keys(build.files).filter((p) => p.endsWith(".dng")), [OFFLINE_SAMPLE]);
assert.equal(build.files[OFFLINE_SAMPLE][2], 0, "no sample in install precache");
if (process.argv[2]) {
  assert.ok(!readdirSync(join(root, "sample")).some((f) => f.endsWith(".py")));
  assert.match(readFileSync(join(root, "_headers"), "utf8"), /\/sample\/\*\.dng\s+Cache-Control: no-store/);
}
console.log("PASS: 7 samples, intact metadata/previews, 6 small online-only RAWs, exactly 1 offline sample, no sample precache");
