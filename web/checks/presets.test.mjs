// node --test web/checks/presets.test.mjs   (after bash web/build.sh)
// The browser's preset bridge (crates/spektrafilm-wasm importPreset and
// exportPreset, web/engine.js) against the shared desktop mapping. With
// NEGS_NATIVE=target/debug/spektrafilm-native (cargo build -p
// spektrafilm-native) it also checks files cross between the browser and the
// native helper desktop runs.
import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { client } from "../engine.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const wasm = await import("../pkg/spektrafilm_wasm.js");
await wasm.default({ module_or_path: readFileSync(join(root, "web/pkg/spektrafilm_wasm_bg.wasm")) });
const catalog = JSON.parse(readFileSync(join(root, "web/catalog.json"), "utf8"));
const defaults = JSON.stringify(catalog.defaults);

// Profiles arrive as the page fetches them: count what the bridge asks for.
const fetched = [];
globalThis.location = { origin: "http://negs.test" };
globalThis.fetch = async (url) => {
  const path = new URL(url).pathname.replace(/^\/data\//, "");
  fetched.push(path);
  try {
    const bytes = readFileSync(join(root, "data", path));
    return { ok: true, status: 200, headers: new Headers(), arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
  } catch {
    return { ok: false, status: 404, headers: new Headers() };
  }
};
const engine = client(wasm, "cpu");
// The native helper widens f32 values (1.2345677614212036), the browser
// writes their shortest decimals (1.2345678): the same f32 either way.
const f32 = (v) => typeof v === "number" ? Math.fround(v) : Array.isArray(v) ? v.map(f32)
  : v && typeof v === "object" ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, f32(x)])) : v;

// A preset with values off every default, hidden fields included.
const EDITED = JSON.stringify({
  simulation: { film_stock: "kodak_doublex", print_paper: "kodak_2302", route: "input > film > print > scan", print_exposure: 1.2345678 },
  film_chemistry: { development_time: 6.5 },
  chemistry: { development_time: 5 },
  couplers: { langmuir_donor_k_rgb: [Infinity, 0.37, Infinity], gamma_interlayer_g_to_rb: [0.11, 0.22] },
  halation: { scatter_tail_weight: 0.123, boost_ev: 0.7, halation_renormalize: false },
  grain: { density_min: [0.05, 0.06, 0.07] },
  input_image: { rgb_to_raw_method: "mallett2019", spectral_gaussian_blur: 0.3 },
  output_gamut_compress: { algorithm: "oklch", knee: [0.6, 1.2, 1.5] },
  input_gamut_compress: { active: true, knee: [0.7, 1.1, 1.3] },
  load_raw: { white_balance: "custom", temperature: 3456, tint: 1.23 },
  camera: { color_filter: "none" },
});

test("import applies the preset over the edit, keeping KEPT values", async () => {
  const current = structuredClone(catalog.defaults);
  current.post.brightness = 25;
  current.io.rotate_cw = 3;
  current.io.photo_long_side_px = 1234;
  current.settings.use_cat16 = !current.settings.use_cat16;
  fetched.length = 0;
  const r = await engine.importPreset(EDITED, JSON.stringify(current));
  assert.deepEqual(fetched.sort(), ["profiles/kodak_2302.json", "profiles/kodak_doublex.json"], "only the stocks, fetched on demand");
  assert.equal(r.film, "kodak_doublex");
  assert.equal(r.paper, "kodak_2302");
  const p = JSON.parse(r.params);
  assert.equal(p.post.brightness, 25, "Post is kept");
  assert.equal(p.io.rotate_cw, 3);
  assert.equal(p.io.photo_long_side_px, 1234);
  assert.equal(p.settings.use_cat16, current.settings.use_cat16);
  assert.equal(p.film_render.development_time, 6.5);
  assert.equal(p.print_render.development_time, 5);
  assert.deepEqual(p.film_render.dir_couplers.langmuir_donor_k_rgb, [null, 0.37, null], "an infinite knee is null in params");
  assert.equal(p.settings.rgb_to_raw_method, "mallett2019");
  assert.equal(p.io.output_gamut_compress.algorithm, "oklch");
  assert.equal(p.io.input_gamut_compress.algorithm, "xy");
  assert.deepEqual(p.io.raw_white_balance, { mode: "custom", temperature: 3456, tint: 1.23 });
  assert.ok(Math.abs(p.enlarger.print_exposure - 1.2345678) < 1e-6);
});

test("export → import reproduces the params, hidden values and Infinity included", async () => {
  const first = await engine.importPreset(EDITED, defaults);
  const text = await engine.exportPreset(first.film, first.paper, first.params);
  assert.match(text, /Infinity/, "Python's Infinity, not null");
  assert.doesNotThrow(() => JSON.parse(text.replaceAll("Infinity", "1e999")));
  const second = await engine.importPreset(text, defaults);
  assert.equal(second.film, first.film);
  assert.equal(second.paper, first.paper);
  assert.deepEqual(JSON.parse(second.params), JSON.parse(first.params));
});

test("a default edit exports and reads back", async () => {
  const text = await engine.exportPreset("kodak_portra_400", "kodak_portra_endura", defaults);
  const r = await engine.importPreset(text, defaults);
  assert.equal(r.film, "kodak_portra_400");
  assert.equal(r.paper, "kodak_portra_endura");
  const p = JSON.parse(r.params);
  assert.equal(p.io.raw_white_balance.mode, "as_shot");
  assert.equal(p.io.scan_film, false);
});

test("refusals keep the shared reasons; unknown stocks fetch once then fail", async () => {
  for (const [text, reason] of [
    ["{", /valid JSON/],
    ["[1, 2]", /no settings/],
    ['{"simulation": {"film_stock": "kodak_nonexistent"}}', /doesn't have the film “kodak_nonexistent”/],
    ['{"simulation": {"film_stock": "../profiles/kodak_gold_200"}}', /doesn't have the film/],
    ['{"simulation": {"film_stock": "kodak_supra_endura"}}', /isn't a film/],
    ['{"simulation": {"print_paper": "kodak_portra_400"}}', /isn't a print paper/],
    ['{"output_gamut_compress": {"algorithm": "jzazbz"}}', /jzazbz/],
    ['{"simulation": {"output_color_space": "Display P3"}}', /Display P3/],
    ['{"special": {"film_channel_swap": [0, 2, 1]}}', /swaps colour channels/],
    ['{"couplers": {"amount": NaN}}', /couplers.amount/],
    ['{"load_raw": {"white_balance": "custom", "temperature": 100}}', /temperature/],
  ]) {
    await assert.rejects(engine.importPreset(text, defaults), reason, text);
  }
  await assert.rejects(engine.exportPreset("kodak_nonexistent", "kodak_portra_endura", defaults), /doesn't have the film/);
  await assert.rejects(engine.exportPreset("kodak_portra_400", "kodak_portra_400", defaults), /isn't a print paper/);
});

test("browser and native helper read each other's files", { skip: !process.env.NEGS_NATIVE && "set NEGS_NATIVE" }, async () => {
  const native = spawn(process.env.NEGS_NATIVE, ["--data-dir", join(root, "data")], { stdio: ["pipe", "pipe", "inherit"] });
  const lines = createInterface({ input: native.stdout })[Symbol.asyncIterator]();
  let id = 0;
  const call = async (command) => {
    native.stdin.write(JSON.stringify({ id: ++id, ...command }) + "\n");
    const r = JSON.parse((await lines.next()).value);
    if (r.error) throw new Error(r.error);
    return r.result;
  };
  try {
    const mine = await engine.importPreset(EDITED, defaults);
    const text = await engine.exportPreset(mine.film, mine.paper, mine.params);
    // Browser file → native importer.
    const read = await call({ command: "preset_import", text, params: catalog.defaults });
    assert.equal(read.film, mine.film);
    assert.deepEqual(f32(read.params), f32(JSON.parse(mine.params)));
    // Native file → browser importer; and the same file both write.
    const written = await call({ command: "preset_export", film: mine.film, paper: mine.paper, params: JSON.parse(mine.params) });
    assert.equal(written.text, text);
    assert.deepEqual(JSON.parse((await engine.importPreset(written.text, defaults)).params), JSON.parse(mine.params));
    // The edit's own sparse file the same through both.
    const viaNative = await call({ command: "preset_import", text: EDITED, params: catalog.defaults });
    assert.deepEqual(f32(viaNative.params), f32(JSON.parse(mine.params)));
  } finally {
    native.stdin.end();
  }
});

test("every shipped stock exports and reads back as itself", async () => {
  const films = catalog.profiles.filter((p) => !p.paper), papers = catalog.profiles.filter((p) => p.paper);
  for (const film of films) {
    const text = await engine.exportPreset(film.id, papers[0].id, defaults);
    const r = await engine.importPreset(text, defaults);
    assert.equal(r.film, film.id);
  }
  for (const paper of papers) {
    const r = await engine.importPreset(await engine.exportPreset(films[0].id, paper.id, defaults), defaults);
    assert.equal(r.paper, paper.id);
  }
  assert.equal(readdirSync(join(root, "data/profiles")).filter((f) => f.endsWith(".json")).length, catalog.profiles.length);
});
