// node --test web/checks/lut-export.test.mjs   (after bash web/build.sh)
// The browser's Export LUT (crates/spektrafilm-wasm exportLut, web/engine.js)
// is desktop's: spektrafilm_core::lut_export, fetching the data it reads.
// With NEGS_CLI=target/release/spektrafilm (cargo build --release -p
// spektrafilm-cli) it also checks the browser's file against the CLI's.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { client } from "../engine.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const wasm = await import("../pkg/spektrafilm_wasm.js");
await wasm.default({ module_or_path: readFileSync(join(root, "web/pkg/spektrafilm_wasm_bg.wasm")) });
const catalog = JSON.parse(readFileSync(join(root, "web/catalog.json"), "utf8"));

const fetched = [];
const dataDir = new URL("../data/", import.meta.url).href;
globalThis.fetch = async (url) => {
  const path = String(url).slice(dataDir.length);
  fetched.push(path);
  try {
    const bytes = readFileSync(join(root, "data", path));
    return { ok: true, status: 200, headers: new Headers(), arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
  } catch {
    return { ok: false, status: 404, headers: new Headers() };
  }
};
const engine = client(wasm, "cpu");

// An edit off the defaults: kept tone controls, and per-image trims the LUT
// must ignore (as desktop's).
function edit({ scan = false } = {}) {
  const params = structuredClone(catalog.defaults);
  params.camera.exposure_compensation_ev = 0.7;
  params.enlarger.print_exposure = 1.2;
  params.enlarger.y_filter_shift = 8;
  params.enlarger.m_filter_shift = -5;
  params.enlarger.preflash_exposure = 0.1;
  params.film_render.grain.active = true;
  params.io.scan_film = scan;
  return params;
}

const rows = (cube) => cube.split("\n").filter((l) => /^[0-9-]/.test(l)).map((l) => l.split(" ").map(Number));

const CASES = [
  ["kodak_portra_400", "kodak_portra_endura", false],
  ["kodak_trix", "kodak_portra_endura", true],
];

test("exports a 33³ CUBE of the film/paper look, fetching what it reads", async () => {
  for (const [film, paper, scan] of CASES) {
    fetched.length = 0;
    const started = performance.now();
    const cube = await engine.exportLut(film, paper, JSON.stringify(edit({ scan })), 33);
    const ms = performance.now() - started;
    console.log(`${film} ${scan ? "scan" : paper}: ${ms.toFixed(0)} ms, fetched ${fetched.length} files`);
    assert.match(cube, /^LUT_3D_SIZE 33$/m);
    assert.match(cube, /^TITLE "Negs .*"$/m);
    const values = rows(cube);
    assert.equal(values.length, 33 ** 3);
    for (const v of values) {
      assert.equal(v.length, 3);
      assert.ok(v.every((x) => Number.isFinite(x) && x >= 0 && x <= 1), `${v} in [0, 1]`);
    }
    // Black stays darker than white.
    assert.ok(values[0].reduce((a, b) => a + b) < values.at(-1).reduce((a, b) => a + b));
  }
});

test("per-image trims don't change the LUT", async () => {
  const [film, paper] = CASES[0];
  const trimmed = await engine.exportLut(film, paper, JSON.stringify(edit()), 9);
  const plain = edit();
  Object.assign(plain.enlarger, { y_filter_shift: 0, m_filter_shift: 0, preflash_exposure: 0 });
  plain.film_render.grain.active = false;
  plain.camera.auto_exposure = !plain.camera.auto_exposure;
  assert.equal(trimmed, await engine.exportLut(film, paper, JSON.stringify(plain), 9));
});

test("an unknown stock is the error", async () => {
  await assert.rejects(engine.exportLut("no_such_film", "kodak_portra_endura", JSON.stringify(edit()), 9));
});

test("matches the native CLI's export-lut", { skip: !process.env.NEGS_CLI && "set NEGS_CLI" }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "negs-lut-"));
  try {
    for (const [film, paper, scan] of CASES) {
      const params = edit({ scan });
      writeFileSync(join(dir, "params.json"), JSON.stringify(params));
      execFileSync(process.env.NEGS_CLI, ["export-lut", "--film", film, "--paper", paper, "--size", "33",
        "--params", join(dir, "params.json"), "--data-dir", join(root, "data"), "-o", join(dir, "native.cube")], { stdio: "ignore" });
      const native = rows(readFileSync(join(dir, "native.cube"), "utf8"));
      const browser = rows(await engine.exportLut(film, paper, JSON.stringify(params), 33));
      assert.equal(browser.length, native.length);
      let worst = 0;
      browser.forEach((v, i) => v.forEach((x, c) => { worst = Math.max(worst, Math.abs(x - native[i][c])); }));
      console.log(`${film} ${scan ? "scan" : paper}: max |browser − native| = ${worst.toExponential(2)}`);
      assert.ok(worst < 1e-4, `browser LUT within 1e-4 of native (${worst})`);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
