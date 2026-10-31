// node --test web/checks/raw-thumbnail.test.mjs (after bash web/build.sh)
// No private fixture needed: give a bundled DNG a decodable RGB IFD0 and
// retain its original RAW IFD as a SubIFD, as real camera DNGs commonly do.
// RAW_THUMBNAIL_DNG=<path> also checks the originally reported camera file.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const web = join(dirname(fileURLToPath(import.meta.url)), "..");
const wasm = await import("../pkg/spektrafilm_wasm.js");
await wasm.default({ module_or_path: readFileSync(join(web, "pkg/spektrafilm_wasm_bg.wasm")) });
const source = readFileSync(join(web, "sample/DSC_6028.dng"));
const CAP = 50000;

function thumbnailDng({ raster = false, damaged = false } = {}) {
  const bytes = Buffer.from(source);
  const little = bytes.toString("ascii", 0, 2) === "II";
  const read16 = at => little ? bytes.readUInt16LE(at) : bytes.readUInt16BE(at);
  const read32 = at => little ? bytes.readUInt32LE(at) : bytes.readUInt32BE(at);
  const put16 = (b, value, at) => little ? b.writeUInt16LE(value, at) : b.writeUInt16BE(value, at);
  const put32 = (b, value, at) => little ? b.writeUInt32LE(value, at) : b.writeUInt32BE(value, at);
  assert.equal(read16(2), 42);
  const original = read32(4), entries = new Map();
  for (let i = 0; i < read16(original); i++) {
    const at = original + 2 + i * 12;
    entries.set(read16(at), Buffer.from(bytes.subarray(at, at + 12)));
  }
  assert.ok(entries.has(50706), "fixture has DNGVersion in IFD0");
  const subIfd = entries.get(330);
  assert.ok(subIfd, "fixture has a RAW SubIFD");
  const rawOffset = little ? subIfd.readUInt32LE(8) : subIfd.readUInt32BE(8);
  const entry = (tag, type, count, value) => {
    const b = Buffer.alloc(12);
    put16(b, tag, 0); put16(b, type, 2); put32(b, count, 4);
    if (type === 3 && count === 1) put16(b, value, 8); else put32(b, value, 8);
    entries.set(tag, b);
  };
  // Remove tiled/predicted/extra-channel storage; IFD0 is now plain RGB8.
  for (const tag of [317, 322, 323, 324, 325, 338]) entries.delete(tag);
  entry(254, 4, 1, 1); // reduced-resolution image: never the RAW
  entry(256, 4, 1, 2); entry(257, 4, 1, 2);
  entry(258, 3, 3, 0); // BitsPerSample offset filled below
  entry(259, 3, 1, 1); entry(262, 3, 1, 2);
  entry(273, 4, 1, 0); // StripOffsets filled below
  entry(277, 3, 1, 3); entry(278, 4, 1, 2); entry(279, 4, 1, 12);
  entry(284, 3, 1, 1); entry(339, 3, 1, 1);
  entry(330, 4, 1, rawOffset);
  if (damaged) entries.delete(330);
  if (raster) { entries.delete(50706); entries.delete(330); }
  const start = (bytes.length + 1) & ~1;
  const size = 2 + entries.size * 12 + 4;
  const bitsAt = start + size, pixelsAt = bitsAt + 6;
  put32(entries.get(258), bitsAt, 8); put32(entries.get(273), pixelsAt, 8);
  const ifd = Buffer.alloc(size + 6 + 12);
  put16(ifd, entries.size, 0);
  [...entries].sort(([a], [b]) => a - b).forEach(([, b], i) => b.copy(ifd, 2 + i * 12));
  for (let i = 0; i < 3; i++) put16(ifd, 8, size + i * 2);
  ifd.set([220, 10, 10, 10, 220, 10, 10, 10, 220, 128, 128, 128], size + 6);
  put32(bytes, start, 4);
  return Buffer.concat([bytes, Buffer.alloc(start - bytes.length), ifd]);
}

function decode(bytes, wb, bounded = false) {
  const d = bounded ? wasm.decodeBounded(bytes, 0, CAP, false, wb) : wasm.decode(bytes, 0, CAP, wb);
  try {
    return { width: d.width, height: d.height, full: [d.fullWidth, d.fullHeight],
      colour: JSON.parse(d.colour), bounded: d.bounded, pixels: d.takePixels() };
  } finally { d.free(); }
}

const previewDng = thumbnailDng();
test("DNG with a decodable RGB thumbnail develops the RAW in decode, Engine.load and decodeBounded", () => {
  const engine = wasm.Engine.newCpu();
  try {
    for (const mode of ["as_shot", "daylight"]) {
      const wb = JSON.stringify({ mode });
      const expected = decode(source, wb);
      const actual = decode(previewDng, wb);
      assert.equal(actual.colour.source, "raw", "IFD0's RGB thumbnail is not the source");
      assert.deepEqual(actual.full, expected.full);
      assert.deepEqual(actual.pixels, expected.pixels);
      const bounded = decode(previewDng, wb, true);
      assert.equal(bounded.colour.source, "raw");
      assert.deepEqual(bounded.pixels, expected.pixels);
      const size = [...engine.load(previewDng, 0, CAP, wb)];
      assert.equal(JSON.parse(engine.inputColour()).source, "raw");
      assert.deepEqual(size, [actual.width, actual.height, ...actual.full]);
    }
    assert.notDeepEqual(decode(previewDng, '{"mode":"daylight"}').pixels,
      decode(previewDng, '{"mode":"as_shot"}').pixels, "RAW WB must affect the sensor image");
    assert.throws(() => decode(previewDng, '{"mode":"custom","temperature":100}'), /temperature/);
  } finally { engine.free(); }
});

test("ordinary RGB TIFF remains a raster and ignores inapplicable RAW WB", () => {
  const bytes = thumbnailDng({ raster: true });
  const normal = decode(bytes);
  assert.notEqual(normal.colour.source, "raw");
  assert.deepEqual(normal.full, [2, 2]);
  assert.deepEqual(decode(bytes, '{"mode":"custom","temperature":100}').pixels, normal.pixels);
});

test("identified DNG with broken RAW data fails instead of falling back to its thumbnail", () => {
  const bytes = thumbnailDng({ damaged: true });
  assert.throws(() => decode(bytes));
  assert.throws(() => decode(bytes, undefined, true));
  const engine = wasm.Engine.newCpu();
  try { assert.throws(() => engine.load(bytes, 0, CAP)); }
  finally { engine.free(); }
});

test("reported camera DNG reaches full-resolution RAW development", {
  skip: !process.env.RAW_THUMBNAIL_DNG && "set RAW_THUMBNAIL_DNG for the private camera fixture",
}, () => {
  const d = decode(readFileSync(process.env.RAW_THUMBNAIL_DNG), '{"mode":"daylight"}');
  assert.equal(d.colour.source, "raw");
  assert.ok(d.full[0] * d.full[1] > 30e6, `not the 171×256 thumbnail: ${d.full}`);
  assert.ok(d.pixels.every(Number.isFinite));
  console.log("Camera DNG RAW size:", d.full.join("×"));
});
