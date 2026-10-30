// node --test web/checks/crop.test.mjs
// Cases shared with macOS CropTests and spektrafilm-io::crop's exporter.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as C from "../crop.js";
const near = (a, b, eps = 1e-6) => { for (const k of Object.keys(b)) assert.ok(Math.abs(a[k] - b[k]) < eps, `${k}: ${a[k]} != ${b[k]}`); };
const photo = { width: 600, height: 400 }, bounds = { x: 0, y: 0, ...photo };
test("native edge rounding, slivers and straightened side rounding", () => {
  assert.deepEqual(C.pixels({ ...C.defaults(), x: 0.1004, width: 0.5 }, 1000, 800), { x: 100, y: 0, width: 500, height: 800 });
  assert.deepEqual(C.pixels({ ...C.defaults(), x: 0.999, width: 0.0001 }, 10, 10), { x: 9, y: 0, width: 1, height: 10 });
  assert.deepEqual(C.pixels({ ...C.defaults(), x: 0.2, y: 0.1, width: 0.5, height: 0.25, angle: -3 }, 1000, 800), { x: 200, y: 80, width: 500, height: 200 });
});
test("rectangles turn clockwise and round-trip", () => {
  const r = { x: 0, y: 0, width: 0.5, height: 0.25 };
  assert.deepEqual(C.shown(r, 1), { x: 0.75, y: 0, width: 0.25, height: 0.5 });
  const any = { x: 0.1, y: 0.2, width: 0.3, height: 0.4 };
  for (let n = -4; n <= 4; n++) near(C.unrotated(C.shown(any, n), n), any);
});
test("native corner/edge/freeform resizing and bounded movement", () => {
  const r = { x: 100, y: 100, width: 300, height: 200 };
  near(C.resize(r, [1, 1], { x: 900, y: 900 }, 1.5, bounds, 10), { x: 100, y: 100, width: 450, height: 300 });
  near(C.resize(r, [1, 0], { x: 460, y: 0 }, 1.5, bounds, 10), { x: 100, y: 80, width: 360, height: 240 });
  near(C.resize(r, [-1, -1], { x: 395, y: -50 }, null, bounds, 20), { x: 380, y: 0, width: 20, height: 300 });
  near(C.move(r, 1000, -1000, bounds), { x: 300, y: 0, width: 300, height: 200 });
  assert.deepEqual(C.handleAt({ x: 104, y: 96 }, r), [-1, -1]);
  assert.deepEqual(C.handleAt({ x: 398, y: 200 }, r), [1, 0]);
  assert.equal(C.handleAt({ x: 20, y: 20 }, r), null);
});
test("aspect locks and orientation follow quarter turns", () => {
  const s = new C.Session(C.defaults(), 600, 400, 0);
  s.choose("square"); near(s.shown, { x: 100, y: 0, width: 400, height: 400 });
  s.choose("3:2"); s.orient(true); near(s.shown, { x: 500 / 3, y: 0, width: 800 / 3, height: 400 });
  const original = structuredClone(s.settings); s.rotate(1);
  near(s.settings, { x: original.x, y: original.y, width: original.width, height: original.height });
  assert.equal(s.portrait, false); assert.equal(s.ratio, 1.5);
  s.choose("custom"); s.settings.custom = [4, 5]; s.refit(); assert.equal(s.ratio, 0.8);
  s.orient(false); assert.equal(s.ratio, 1.25);
  s.reset(); assert.deepEqual(s.settings.custom, [5, 4]);
  s.settings.custom = [7, 4]; s.reset(); assert.deepEqual(s.settings.custom, [7, 4]);
});
test("leveling shrinks onto the photograph, then restores during the same drag", () => {
  const s = new C.Session(C.defaults(), 600, 400, 0);
  s.straighten(5); const r = s.shown;
  assert.ok(C.fits(r, 5, photo)); assert.ok(r.width < 600);
  assert.ok(Math.abs(r.width / r.height - 1.5) < 1e-6);
  near({ x: r.x + r.width / 2, y: r.y + r.height / 2 }, { x: 300, y: 200 });
  assert.ok(!C.fits({ x: r.x - r.width * 0.005, y: r.y - r.height * 0.005, width: r.width * 1.01, height: r.height * 1.01 }, 5, photo));
  s.straighten(0); near(s.shown, bounds);
  s.straighten(-3.04); assert.equal(s.settings.angle, -3);
  s.straighten(-3.05); assert.equal(s.settings.angle, -3.1);
  s.straighten(99); assert.equal(s.settings.angle, 45);
  const start = C.inscribe({ x: 250, y: 150, width: 100, height: 100 }, 10, photo);
  const stopped = C.constrain(start, { ...start, x: start.x + 1000 }, 10, photo);
  assert.ok(C.fits(stopped, 10, photo)); assert.ok(!C.fits({ ...stopped, x: stopped.x + 1 }, 10, photo));
});
test("random straightened crops stay on the source, also after turning", () => {
  let seed = 7; const random = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 2 ** 32);
  for (let i = 0; i < 300; i++) {
    const s = new C.Session(C.defaults(), i % 2 ? 400 : 600, i % 2 ? 600 : 400, i % 4);
    s.choose(C.ASPECTS[2 + i % 6][0]); s.straighten(random() * 90 - 45);
    assert.ok(C.fits(s.shown, s.settings.angle, s.photo));
    const g = C.presentation(s.settings, s.rotation, s.source.width, s.source.height), m = g.matrix;
    // Within the source apart from at most one pixel of edge rounding.
    for (const [u, v] of [[0, 0], [1, 0], [1, 1], [0, 1]]) {
      const x = m[0] * u + m[1] * v + m[2], y = m[4] * u + m[5] * v + m[6];
      assert.ok(x >= -0.003 && x <= 1.003 && y >= -0.003 && y <= 1.003);
    }
  }
});
const rgba = Uint8Array.from([1, 2, 3, 4, 5, 6].flatMap((v) => [v, v, v, 255]));
const values = (r) => [...r.rgba].filter((_, i) => i % 4 === 0);
test("lossless byte turns match ImageBuf::rotated_cw, crop before turn", () => {
  const rotated = C.applyRgba(rgba, 3, 2, C.defaults(), 1);
  assert.equal(rotated.width, 2); assert.equal(rotated.height, 3); assert.deepEqual(values(rotated), [4, 1, 5, 2, 6, 3]);
  assert.deepEqual(values(C.applyRgba(rgba, 3, 2, C.defaults(), -1)), [3, 6, 2, 5, 1, 4]);
  const crop = { ...C.defaults(), x: 1 / 3, width: 2 / 3 };
  assert.deepEqual(values(C.applyRgba(rgba, 3, 2, crop, 1)), [5, 2, 6, 3]);
  const back = C.applyRgba(rotated.rgba, 2, 3, C.defaults(), -1); assert.deepEqual(back.rgba, rgba);
});
test("bilinear leveling matches the native inverse pixel-centre convention", () => {
  const w = 8, h = 6, data = Uint8Array.from(Array.from({ length: w * h }, (_, i) => [i % w * 30, Math.floor(i / w) * 40, 0, 255]).flat());
  const crop = { ...C.defaults(), x: 0.2, y: 0.1, width: 0.5, height: 0.5, angle: 12 };
  const out = C.applyRgba(data, w, h, crop, 0), a = crop.angle * Math.PI / 180;
  for (let y = 0; y < out.height; y++) for (let x = 0; x < out.width; x++) {
    const dx = crop.x * w + x + 0.5 - w / 2, dy = crop.y * h + y + 0.5 - h / 2;
    const sx = C.clamp(w / 2 + dx * Math.cos(a) + dy * Math.sin(a) - 0.5, 0, w - 1);
    const sy = C.clamp(h / 2 - dx * Math.sin(a) + dy * Math.cos(a) - 0.5, 0, h - 1);
    const i = (y * out.width + x) * 4;
    assert.equal(out.rgba[i], Math.round(sx * 30)); assert.equal(out.rgba[i + 1], Math.round(sy * 40));
  }
});
