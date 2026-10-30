// Numeric parity in the actual single-thread WASM module, not renderer pixels.
//   node web/checks/bounded-pixels.mjs ~/Downloads/3980774776.dng [bounded|force] [baseline-pkg-dir]
// An optional ORIGINAL pkg directory also checks current legacy against the
// original, instead of comparing only two routes in the rebuilt candidate.
// Separate worker lifetimes keep the WASM high-water allocations apart.
// Phone/threaded reliability is a separate test (raw-bounded.mjs).
import { readFileSync } from "node:fs";
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const currentPkg = fileURLToPath(new URL("../pkg/", import.meta.url));
if (isMainThread) {
  const file = process.argv[2], mode = process.argv[3] ?? "bounded";
  if (!file || !["bounded", "force"].includes(mode)) throw new Error("Usage: bounded-pixels.mjs <RAW> [bounded|force] [baseline-pkg-dir]");
  const run = (mode, pkg = currentPkg) => new Promise((resolve, reject) => {
    const worker = new Worker(new URL(import.meta.url), { workerData: { file, mode, pkg } });
    let result;
    worker.once("message", (value) => { result = value; });
    worker.once("error", reject);
    worker.once("exit", (code) => {
      if (code || !result) reject(new Error(`Pixel worker exited ${code}${result ? "" : " without pixels"}`));
      else resolve(result);
    });
  });
  const legacy = await run("legacy", process.argv[4] ? resolve(process.argv[4]) : currentPkg);
  const candidates = process.argv[4] ? ["legacy", mode] : [mode];
  for (const candidateMode of candidates) {
    const bounded = await run(candidateMode);
    if (legacy.width !== bounded.width || legacy.height !== bounded.height || legacy.fullWidth !== bounded.fullWidth || legacy.fullHeight !== bounded.fullHeight) {
      throw new Error("Decoded dimensions differ");
    }
    const a = new Float32Array(legacy.pixels), b = new Float32Array(bounded.pixels);
    if (a.length !== b.length || a.length !== legacy.width * legacy.height * 3) throw new Error("Decoded pixel lengths differ");
    const aBits = new Uint32Array(legacy.pixels), bBits = new Uint32Array(bounded.pixels);
    let max = 0, sum = 0, changed = 0;
    for (let i = 0; i < a.length; i++) {
      if (!Number.isFinite(a[i]) || !Number.isFinite(b[i])) throw new Error(`Nonfinite scalar ${i}`);
      const delta = Math.abs(a[i] - b[i]);
      max = Math.max(max, delta); sum += delta; changed += aBits[i] !== bBits[i];
    }
    console.log(JSON.stringify({ file, candidateMode, max, mean: sum / a.length, changed,
      legacy: { ...legacy, pixels: undefined }, bounded: { ...bounded, pixels: undefined } }));
    if (changed) process.exitCode = 1;
  }
} else {
  const wasm = await import(pathToFileURL(resolve(workerData.pkg, "spektrafilm_wasm.js")));
  await wasm.default({ module_or_path: readFileSync(resolve(workerData.pkg, "spektrafilm_wasm_bg.wasm")) });
  const bytes = readFileSync(workerData.file), start = performance.now();
  const decoded = workerData.mode === "legacy" ? wasm.decode(bytes, 0, 12e6)
    : wasm.decodeBounded(bytes, 0, 12e6, workerData.mode === "force");
  const pixels = decoded.takePixels();
  const result = { pixels: pixels.buffer, width: decoded.width, height: decoded.height,
    fullWidth: decoded.fullWidth, fullHeight: decoded.fullHeight, bounded: decoded.bounded ?? false,
    pkg: workerData.pkg, ms: performance.now() - start, MiB: wasm.memoryBytes() / 2 ** 20 };
  decoded.free();
  parentPort.postMessage(result, [pixels.buffer]);
  parentPort.close();
}
