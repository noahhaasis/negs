// node --test web/checks/presets-link.test.mjs   (after bash web/build.sh)
// Preset naming (macOS PresetName's policy) and the shared-link codec
// (web/presets.js), with link sizes measured on presets the shared exporter
// writes (web/pkg). Prints the measured sizes.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateRawSync } from "node:zlib";
import {
  LINK_VERSION, MAX_ENVELOPE_BYTES, MAX_LINK_CHARS, LinkError, decodeLink, encodeLink, linkUrl, linkValue, nameFrom, nameProblem, stem,
} from "../presets.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const wasm = await import("../pkg/spektrafilm_wasm.js");
await wasm.default({ module_or_path: readFileSync(join(root, "web/pkg/spektrafilm_wasm_bg.wasm")) });
const catalog = JSON.parse(readFileSync(join(root, "web/catalog.json"), "utf8"));
for (const p of catalog.profiles) wasm.registerData(`profiles/${p.id}.json`, readFileSync(join(root, `data/profiles/${p.id}.json`)));
function exported(film, paper, params) {
  const r = wasm.exportPreset(film, paper, JSON.stringify(params));
  assert.equal(r.error, "");
  return r.text;
}
const b64url = (bytes) => Buffer.from(bytes).toString("base64url");

test("names follow desktop's policy", () => {
  assert.equal(nameProblem("Warm afternoon"), null);
  assert.equal(nameProblem("Kodak Portra 400 · Ünïcødé 🎞"), null);
  assert.match(nameProblem(""), /Enter a name/);
  assert.match(nameProblem("   "), /Enter a name/);
  assert.match(nameProblem("a/b"), /can’t contain/);
  assert.match(nameProblem("tab\there"), /can’t contain/);
  assert.match(nameProblem(".hidden"), /start with a period/);
  assert.match(nameProblem("trailing."), /end with a space or period/);
  assert.match(nameProblem("com1.txt"), /reserved on Windows/);
  assert.match(nameProblem("x".repeat(121)), /at most 120/);
  assert.equal(nameProblem("🎞".repeat(120)), null, "characters, not UTF-16 units");
  assert.equal(nameFrom("my: look?"), "my look");
  assert.equal(nameFrom("..dots.."), "dots");
  assert.equal(nameFrom("a\nb c"), "a b c");
  assert.equal(nameFrom("CON"), "Preset");
  assert.equal(nameFrom(""), "Preset");
  assert.equal(stem("Warm.v2.json"), "Warm.v2");
  assert.equal(stem("noext"), "noext");
});

test("links round-trip full-precision text, Unicode names and Infinity", async () => {
  const text = '{\n  "couplers": {"langmuir_donor_k_rgb": [Infinity, 0.123456789012345, Infinity]},\n  "x": "<script>alert(1)</script>"\n}\n';
  const name = "Été · 夕方 🎞 <b>bold</b>";
  const value = await encodeLink(name, text);
  assert.match(value, /^[A-Za-z0-9_-]+$/, "URL-safe");
  const back = await decodeLink(value);
  assert.equal(back.text, text, "byte-exact preset text");
  assert.equal(back.name, nameFrom(name));
  const url = linkUrl({ origin: "https://negs.example", pathname: "/" }, value);
  assert.equal(url, `https://negs.example/#preset=${value}`);
  assert.equal(linkValue(new URL(url).hash), value);
  assert.equal(linkValue("#other=1"), null);
  assert.equal(linkValue(""), null);
});

test("malformed, truncated, unknown-version and oversized links fail with a reason", async () => {
  const value = await encodeLink("Look", exported("kodak_portra_400", "kodak_portra_endura", catalog.defaults));
  const reasons = [
    ["", /empty/],
    ["!!!not base64", /damaged/],
    [value.slice(0, value.length / 2), /damaged/],
    [value.slice(0, -1) + (value.at(-1) === "A" ? "B" : "A"), /damaged/],
    [b64url([LINK_VERSION + 1, ...deflateRawSync("{}")]), /newer Negs/],
    [b64url([LINK_VERSION, ...deflateRawSync("not json")]), /damaged/],
    [b64url([LINK_VERSION, ...deflateRawSync(JSON.stringify({ name: 1, preset: "{}" }))]), /damaged/],
    [b64url([LINK_VERSION, ...deflateRawSync(Buffer.from([0xff, 0xfe]))]), /damaged/],
    ["A".repeat(MAX_LINK_CHARS), /too long/],
    // A small link that inflates past the envelope limit (a zip bomb).
    [b64url([LINK_VERSION, ...deflateRawSync(JSON.stringify({ name: "x", preset: " ".repeat(MAX_ENVELOPE_BYTES + 10) }))]), /too large/],
  ];
  for (const [bad, reason] of reasons) {
    await assert.rejects(decodeLink(bad), (e) => e instanceof LinkError && reason.test(e.message), `${bad.slice(0, 40)}…`);
  }
  // A preset that compresses past the link budget isn't made into a link.
  const noise = Array.from({ length: 40_000 }, (_, i) => ((i * 2654435761) >>> 0).toString(36)).join("");
  await assert.rejects(encodeLink("Big", noise), (e) => e instanceof LinkError && /too large for a link/.test(e.message));
});

test("measured link sizes for shared-exporter presets stay well inside the budget", async () => {
  const sizes = [];
  const edited = structuredClone(catalog.defaults);
  // Full-precision values throughout: the worst case for compression.
  let k = 0;
  const jitter = (o) => {
    for (const [key, v] of Object.entries(o)) {
      if (typeof v === "number" && !Number.isInteger(v) && key !== "temperature") o[key] = v + Math.fround(0.000123457 * ++k);
      else if (v && typeof v === "object") jitter(v);
    }
  };
  jitter(edited.film_render);
  jitter(edited.print_render);
  jitter(edited.enlarger);
  for (const [film, paper, params, label] of [
    ["kodak_portra_400", "kodak_portra_endura", catalog.defaults, "defaults"],
    ["kodak_doublex", "kodak_2302", catalog.defaults, "B&W defaults"],
    ["kodak_portra_400", "kodak_portra_endura", edited, "full-precision edit"],
  ]) {
    const text = exported(film, paper, params);
    const url = linkUrl({ origin: "https://negs.example", pathname: "/" }, await encodeLink("A preset name of reasonable length", text));
    sizes.push(`${label}: file ${text.length} B → link ${url.length} chars`);
    assert.ok(url.length < MAX_LINK_CHARS / 2, `${label}: ${url.length}`);
  }
  console.log(`preset link sizes (budget ${MAX_LINK_CHARS}):\n  ${sizes.join("\n  ")}`);
});
