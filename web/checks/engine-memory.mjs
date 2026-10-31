// Node single-thread WASM diagnostic. Runs a synchronous burst so result
// finalizers cannot be relied on between frames. Not OS/process/GPU memory.
// node web/checks/engine-memory.mjs [CLIENT_MODULE] [COUNT=80]
// A baseline module lets the old JS and the fixed JS use the SAME WASM.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import assert from "node:assert/strict";
import * as wasm from "../pkg/spektrafilm_wasm.js";
const { client } = await import(pathToFileURL(resolve(process.argv[2] ?? "web/engine.js")));
await wasm.default({ module_or_path: readFileSync(new URL("../pkg/spektrafilm_wasm_bg.wasm", import.meta.url)) });
const host = client(wasm, "cpu");
await host.start();
// Deterministic synthetic source and the IO layer's raw InputColour shape.
// Do not decode a real RAW here: its high-water memory would hide retention.
const colour = JSON.stringify({ source: "raw", encoding: "Linear sRGB (scene-referred)",
  summary: "Synthetic RAW", automatic: "Synthetic RAW", warning: null, scene_referred: true, linear_srgb: true });
const width = 1024, height = 768, pixels = new Float32Array(width * height * 3);
for (let i = 0; i < pixels.length; i++) pixels[i] = (i % 257) / 256;
host.loadDecoded({ pixels, width, height, fullWidth: width, fullHeight: height, colour });
const count = Number(process.argv[3] ?? 80);
let expected;
// Warm display conversion before measuring growth.
host.original("", width, false);
const before = wasm.memoryBytes(), began = performance.now();
for (let n = 0; n < count; n++) {
  const r = host.original("", width, false);
  assert.equal(r.rgba.length, width * height * 4);
  let checksum = 0;
  for (let i = 0; i < r.rgba.length; i += 101) checksum = (checksum * 31 + r.rgba[i]) >>> 0;
  expected ??= checksum;
  assert.equal(checksum, expected, "freeing the Rust result must not invalidate its JS pixels");
}
console.log(JSON.stringify({ client: process.argv[2] ?? "web/engine.js", count, width, height, checksum: expected,
  wasmBeforeMiB: before / 2 ** 20, wasmAfterMiB: wasm.memoryBytes() / 2 ** 20,
  growthMiB: (wasm.memoryBytes() - before) / 2 ** 20, elapsedMs: performance.now() - began }));
