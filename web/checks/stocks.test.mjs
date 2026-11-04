// node --test web/checks/stocks.test.mjs   (after bash web/build.sh)
// The film and paper previews' engine side (crates/spektrafilm-wasm
// thumbnail, web/engine.js) on the CPU engine: a tile is the render of that
// edit at its size, it leaves the render alone, and a render that takes a
// tile's calibrated pipeline and metered auto exposure is a fresh engine's.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { client } from "../engine.js";
import { defaults } from "../crop.js";
import { frameSide } from "../stocks.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const wasm = await import("../pkg/spektrafilm_wasm.js");
await wasm.default({ module_or_path: readFileSync(join(root, "web/pkg/spektrafilm_wasm_bg.wasm")) });
const catalog = JSON.parse(readFileSync(join(root, "web/catalog.json"), "utf8"));
const dataDir = new URL("../data/", import.meta.url).href;
globalThis.fetch = async (url) => {
  try {
    const bytes = readFileSync(join(root, "data", String(url).slice(dataDir.length)));
    return { ok: true, status: 200, headers: new Headers(), arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
  } catch {
    return { ok: false, status: 404, headers: new Headers() };
  }
};

const SAMPLE = readFileSync(join(root, "web/sample/DSC_6028.dng"));
const CAP = 0.25e6;
const params = (edit = () => {}) => {
  const p = structuredClone(catalog.defaults);
  p.camera.auto_exposure = true;
  edit(p);
  return JSON.stringify(p);
};
const FULL = defaults();
async function started() {
  const engine = client(wasm, "cpu");
  await engine.start();
  engine.load(SAMPLE, 0, CAP);
  return engine;
}
const same = (a, b) => a.width === b.width && a.height === b.height && Buffer.compare(Buffer.from(a.rgba), Buffer.from(b.rgba)) === 0;

test("a tile is the interactive render of its edit at its size", async () => {
  const engine = await started();
  const p = params();
  const tile = await engine.thumbnail("kodak_gold_200", "kodak_portra_endura", p, 160, 40, FULL, 0);
  const frame = await engine.render("kodak_gold_200", "kodak_portra_endura", p, 160, true);
  assert.equal(Math.max(tile.width, tile.height), 160);
  assert.ok(same(tile, frame), "the tile's bytes are the render's");
});

test("tiles leave the render's pipeline and frame alone", async () => {
  const engine = await started();
  const p = params((p) => { p.enlarger.print_exposure = 1.1; });
  const before = await engine.render("kodak_portra_400", "kodak_portra_endura", p, 200, false);
  for (const [film, paper] of [["kodak_ektar_100", "kodak_portra_endura"], ["kodak_portra_400", "kodak_2383"], ["kodak_trix", "ilford_mgrc_deluxe_grade2"]]) {
    await engine.thumbnail(film, paper, p, 120, 40, FULL, 0);
  }
  const after = await engine.render("kodak_portra_400", "kodak_portra_endura", p, 200, false);
  assert.ok(same(before, after));
});

test("a render taking a tile's pipeline and metered exposure is a fresh engine's", async () => {
  const p = params((p) => { p.camera.exposure_compensation_ev = 0.4; });
  const warm = await started();
  await warm.render("kodak_portra_400", "kodak_portra_endura", p, 180, true);
  // The paper tile meters once per film, whatever the paper.
  await warm.thumbnail("kodak_portra_400", "kodak_supra_endura", p, 180, 40, FULL, 0);
  const chosen = await warm.render("kodak_portra_400", "kodak_supra_endura", p, 180, true);
  const fresh = await (await started()).render("kodak_portra_400", "kodak_supra_endura", p, 180, true);
  assert.ok(same(chosen, fresh));
});

test("a tile is cropped and turned as an export is, from a frame the crop fills", async () => {
  const engine = await started();
  const crop = { ...FULL, x: 0.25, y: 0.1, width: 0.5, height: 0.8 };
  const whole = await engine.thumbnail("kodak_portra_400", "kodak_portra_endura", params(), 200, 40, FULL, 0);
  const side = frameSide(whole.width, whole.height, crop, 100);
  const tile = await engine.thumbnail("kodak_portra_400", "kodak_portra_endura", params(), side, 40, crop, 1);
  // Turned a quarter: the crop's height across.
  assert.ok(Math.abs(tile.width - Math.round(0.8 * whole.height * side / 200)) <= 1, `${tile.width}×${tile.height}`);
  assert.ok(tile.width > tile.height);
  engine.releaseThumbnails();
  const again = await engine.thumbnail("kodak_portra_400", "kodak_portra_endura", params(), side, 40, crop, 1);
  assert.ok(same(tile, again), "the same after the caches are let go");
});

test("frameSide: the crop fills the long side, within 1280", () => {
  assert.equal(frameSide(6000, 4000, FULL, 320), 320);
  assert.equal(frameSide(6000, 4000, { ...FULL, width: 0.5, height: 0.5 }, 320), 640);
  assert.equal(frameSide(6000, 4000, { ...FULL, width: 0.1, height: 0.1 }, 320), 1280);
  assert.equal(frameSide(6000, 4000, { ...FULL, width: 1, height: 0.25 }, 320), 320);
});

test("a bad size is refused", async () => {
  const engine = await started();
  await assert.rejects(engine.thumbnail("kodak_portra_400", "kodak_portra_endura", params(), 8, 40, FULL, 0), /thumbnail size/);
});
