// node --test web/checks/raw-wb.test.mjs   (after bash web/build.sh)
// RAW input white balance through the browser engine's decode routes (web/pkg,
// the same spektrafilm-io loaders the native apps use; their own bytes-vs-path
// parity is cargo test -p spektrafilm-io byte_loaders_apply_the_same_wb), and
// the page's pure WB helpers (web/raw-wb.js).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { deflateSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createSourceQueue, isRawColour, sameWb, wbKey } from "../raw-wb.js";
import { RAW_WB_MODES, SECTIONS, optionTitle } from "../controls.js";

const web = join(dirname(fileURLToPath(import.meta.url)), "..");
const wasm = await import("../pkg/spektrafilm_wasm.js");
await wasm.default({ module_or_path: readFileSync(join(web, "pkg/spektrafilm_wasm_bg.wasm")) });

const SAMPLE = readFileSync(join(web, "sample/DSC_6028.dng"));
const CAP = 0.25e6; // A small working copy; the develop itself is full size.
const wbJson = (mode, temperature = 5500, tint = 1) => JSON.stringify({ mode, temperature, tint });
// Kept pixels of a throwaway-worker decode (web/decoder.js's call).
function decode(bytes, wb, cap = CAP) {
  const d = wasm.decode(bytes, 0, cap, wb);
  const out = { pixels: d.takePixels(), width: d.width, height: d.height, colour: JSON.parse(d.colour) };
  d.free();
  return out;
}
const equal = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
const differ = (a, b) => a.length === b.length && a.some((v, i) => v !== b[i]);
function mean(pixels) {
  const sum = [0, 0, 0];
  for (let i = 0; i < pixels.length; i++) sum[i % 3] += pixels[i];
  return sum.map((s) => (3 * s) / pixels.length);
}

test("helpers: effective WB keys, RAW classification, serial source queue", async () => {
  assert.equal(wbKey(undefined), "as_shot");
  assert.equal(wbKey({ mode: "daylight", temperature: 3000, tint: 2 }), "daylight");
  assert.ok(sameWb({ mode: "as_shot", temperature: 5500, tint: 1 }, { mode: "as_shot", temperature: 9000, tint: 3 }), "inactive custom fields don't develop again");
  assert.ok(!sameWb({ mode: "custom", temperature: 5500, tint: 1 }, { mode: "custom", temperature: 5501, tint: 1 }));
  assert.ok(isRawColour('{"source":"raw"}') && isRawColour({ source: "raw" }));
  assert.ok(!isRawColour('{"source":"embedded_profile"}') && !isRawColour("not json") && !isRawColour(null));

  const queue = createSourceQueue(), order = [];
  let release;
  const first = queue.run(() => new Promise((r) => { release = r; }).then(() => order.push("first")));
  const second = queue.run(async () => { order.push("second"); throw new Error("fails"); });
  const third = queue.run(async () => order.push("third"));
  assert.ok(queue.busy);
  await new Promise((r) => setTimeout(r, 10));
  assert.deepEqual(order, [], "nothing overtakes the running operation");
  release();
  await first;
  await assert.rejects(second, /fails/);
  await third;
  assert.deepEqual(order, ["first", "second", "third"]);
  assert.ok(!queue.busy, "a failure doesn't wedge the queue");
});

test("controls: RAW White Balance in Camera before Lens Diffusion, desktop ranges", () => {
  const camera = SECTIONS.find((s) => s.title === "Camera");
  const titles = camera.groups.map((g) => g.title);
  assert.equal(titles.indexOf("RAW White Balance") + 1, titles.indexOf("Lens Diffusion"));
  const group = camera.groups.find((g) => g.title === "RAW White Balance");
  assert.equal(group.raw, true);
  const [mode, temperature, tint] = group.controls;
  assert.deepEqual(mode.options, ["as_shot", "daylight", "tungsten", "custom"]);
  assert.deepEqual(RAW_WB_MODES.map(optionTitle), ["As Shot", "Daylight", "Tungsten (2850 K)", "Custom"]);
  assert.deepEqual([temperature.path, temperature.min, temperature.max, temperature.step], ["io.raw_white_balance.temperature", 1667, 25000, 1]);
  assert.deepEqual([tint.path, tint.min, tint.max, tint.step], ["io.raw_white_balance.tint", 0.01, 4, 0.01]);
  for (const c of [temperature, tint]) assert.deepEqual(c.when, ["io.raw_white_balance.mode", "custom"]);
});

test("throwaway decode: every mode develops the RAW, Custom 6504 K / 1 is Daylight, As Shot is the default", () => {
  const legacy = decode(SAMPLE, undefined);
  assert.equal(legacy.colour.source, "raw");
  assert.equal(legacy.colour.scene_referred, true);
  const asShot = decode(SAMPLE, wbJson("as_shot"));
  assert.ok(equal(asShot.pixels, legacy.pixels), "As Shot (and an empty object) is the old default");
  assert.ok(equal(decode(SAMPLE, "{}").pixels, legacy.pixels));
  const daylight = decode(SAMPLE, wbJson("daylight"));
  const tungsten = decode(SAMPLE, wbJson("tungsten"));
  const custom = decode(SAMPLE, wbJson("custom", 3200, 0.85));
  for (const [name, d] of [["daylight", daylight], ["tungsten", tungsten], ["custom", custom]]) {
    assert.ok(differ(d.pixels, asShot.pixels), `${name} changes the pixels`);
    assert.deepEqual([d.width, d.height], [asShot.width, asShot.height]);
    assert.ok(d.pixels.every(Number.isFinite));
  }
  assert.ok(differ(tungsten.pixels, daylight.pixels));
  assert.ok(equal(decode(SAMPLE, wbJson("custom", 6504, 1)).pixels, daylight.pixels), "Custom 6504 K / tint 1 equals Daylight");
  assert.ok(equal(decode(SAMPLE, wbJson("daylight", 99999, -1)).pixels, daylight.pixels), "inactive Custom values are ignored");
  // Tungsten adapts a 2850 K white to daylight: the scene turns bluer.
  const [dr, , db] = mean(daylight.pixels), [tr, , tb] = mean(tungsten.pixels);
  assert.ok(tb / tr > db / dr, "tungsten is bluer than daylight");
  // A lower Custom tint is more magenta (less green).
  const [, g1] = mean(decode(SAMPLE, wbJson("custom", 5500, 1)).pixels), [, g2] = mean(decode(SAMPLE, wbJson("custom", 5500, 0.8)).pixels);
  assert.ok(g2 < g1);
  // Back to As Shot is the baseline again, not a correction of the last one.
  assert.ok(equal(decode(SAMPLE, wbJson("as_shot")).pixels, legacy.pixels));
});

test("invalid active Custom values fail clearly; malformed JSON fails", () => {
  assert.throws(() => decode(SAMPLE, wbJson("custom", 1000, 1)), { message: "RAW WB temperature must be between 1667 and 25000 K" });
  assert.throws(() => decode(SAMPLE, wbJson("custom", 5500, 0)), /tint must be greater than 0 and at most 4/);
  assert.throws(() => decode(SAMPLE, wbJson("custom", 5500, 4.5)), /tint must be greater than 0 and at most 4/);
  assert.throws(() => decode(SAMPLE, '{"mode":"sunset"}'), /RAW white balance/);
  assert.throws(() => decode(SAMPLE, "{"), /RAW white balance/);
});

test("main engine load and throwaway decode develop the same source at the same WB", () => {
  const engine = wasm.Engine.newCpu();
  const originalAt = () => {
    const r = engine.original("", 512, false);
    const rgba = r.rgba;
    r.free();
    return rgba;
  };
  const sources = {};
  for (const mode of ["as_shot", "daylight"]) {
    engine.load(SAMPLE, 0, CAP, wbJson(mode));
    assert.equal(JSON.parse(engine.inputColour()).source, "raw");
    const main = originalAt();
    const d = decode(SAMPLE, wbJson(mode));
    engine.loadDecoded(d.pixels, d.width, d.height, JSON.stringify(d.colour));
    assert.ok(equal(originalAt(), main), `${mode}: both routes keep the same pixels`);
    sources[mode] = main;
  }
  assert.ok(differ(sources.daylight, sources.as_shot), "Show Original's source follows the WB");
  // A failed load (invalid WB) leaves no photograph, which the page reloads.
  assert.throws(() => engine.load(SAMPLE, 0, CAP, wbJson("custom", 99999, 1)), /temperature/);
  assert.equal(engine.inputColour(), undefined);
  assert.throws(() => engine.original("", 512, false), /no image loaded/);
  engine.load(SAMPLE, 0, CAP);
  assert.ok(equal(originalAt(), sources.as_shot), "load without a WB is As Shot (node-test.mjs, older callers)");
  engine.free();
});

// The bundled samples are linear DNGs, which the banded developer refuses;
// BAYER_RAW=<path> also forces a Bayer file through it (the banded WB itself
// is covered by cargo test -p spektrafilm-io bounded).
test("opt-in bounded decode takes the selected WB", () => {
  const cases = [[SAMPLE, false], ...(process.env.BAYER_RAW ? [[readFileSync(process.env.BAYER_RAW), true]] : [])];
  for (const [bytes, force] of cases) {
    const at = (wb) => {
      const d = wasm.decodeBounded(bytes, 0, CAP, force, wb);
      const out = { pixels: d.takePixels(), bounded: d.bounded, colour: JSON.parse(d.colour) };
      d.free();
      return out;
    };
    const asShot = at(undefined), daylight = at(wbJson("daylight"));
    assert.equal(asShot.bounded, force, "a small RAW is banded only when forced");
    assert.equal(daylight.colour.source, "raw");
    assert.ok(differ(daylight.pixels, asShot.pixels), `force=${force}: Daylight changes the pixels`);
    assert.ok(equal(at(wbJson("custom", 6504, 1)).pixels, daylight.pixels));
    assert.ok(equal(at(wbJson("as_shot")).pixels, asShot.pixels));
    assert.throws(() => at(wbJson("custom", 5500, -1)), /tint/);
  }
  assert.throws(() => wasm.decodeBounded(SAMPLE, 0, CAP, true, wbJson("daylight")), /Bayer/, "forcing a linear DNG refuses it, never ignores the WB");
});

// A 2 × 2 RGB PNG, by hand.
function png() {
  const crc = (buf) => {
    let c = ~0;
    for (const b of buf) { c ^= b; for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1)); }
    return ~c >>> 0;
  };
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc(body), body.length + 4);
    return out;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(2, 0);
  header.writeUInt32BE(2, 4);
  header.set([8, 2, 0, 0, 0], 8);
  const rows = Buffer.from([0, 200, 120, 40, 30, 60, 90, 0, 10, 250, 10, 128, 128, 128]);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", deflateSync(rows)), chunk("IEND", Buffer.alloc(0))]);
}

test("developed rasters ignore RAW WB, including inapplicable Custom values", () => {
  const bytes = png();
  const base = decode(bytes, undefined, 0);
  assert.notEqual(base.colour.source, "raw");
  assert.equal(base.colour.scene_referred, false);
  for (const wb of [wbJson("daylight"), wbJson("tungsten"), wbJson("custom", 99999, -2)]) {
    assert.ok(equal(decode(bytes, wb, 0).pixels, base.pixels), wb);
  }
});

test("every bundled sample is classified as RAW by the decoder", () => {
  const catalog = JSON.parse(readFileSync(join(web, "sample/catalog.json")));
  for (const s of catalog.samples) {
    const d = decode(readFileSync(join(web, s.url)), undefined, 0.05e6);
    assert.equal(d.colour.source, "raw", s.id);
  }
});
