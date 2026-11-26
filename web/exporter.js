// Full-resolution tiled export in a short-lived worker: wasm's memory never
// shrinks, so running a 24–60 MP export here lets Safari reclaim all of its
// WASM linear memory and WebGPU buffers as soon as the JPEG returns, while
// the editing engine keeps its compact working copy.

import { fetchFailed } from "./engine.js";
import { threadPool } from "./pool.js";

const pool = threadPool();

async function fetchData(wasm, paths) {
  await Promise.all(paths.map(async (path) => {
    const res = await fetch(new URL(`data/${path}`, import.meta.url));
    if (!res.ok) throw fetchFailed(path, res);
    wasm.registerData(path, new Uint8Array(await res.arrayBuffer()));
  }));
}

onmessage = async ({ data: { threads, useGpu, bytes, film, paper, params, whiteBalance, autoexposureEv, crop, rotation, border, quality = 92 } }) => {
  let wasm, exporter;
  try {
    const started = performance.now();
    wasm = threads ? await import("./pkg-threads/index.js") : await import("./pkg/spektrafilm_wasm.js");
    await wasm.default();
    wasm.setInstance(0);
    if (threads) await pool.start(wasm, navigator.hardwareConcurrency);

    exporter = await wasm.TiledExporter.create(useGpu);
    const backend = exporter.backend();
    const borderJson = border ? JSON.stringify(border) : undefined;
    const cropJson = crop ? JSON.stringify(crop) : undefined;

    for (;;) {
      const prep = exporter.prepare(film, paper, params, borderJson);
      let missing;
      try { missing = prep.missing; } finally { prep.free(); }
      if (!missing.length) break;
      await fetchData(wasm, missing);
    }

    postMessage({ progress: { stage: "decode" } });
    const plan = exporter.start(bytes, whiteBalance, autoexposureEv);
    let width, height, totalTiles, tileWidth, tileHeight, halo, sourceKind, memoryMiB;
    try {
      ({ width, height, totalTiles, tileWidth, tileHeight, halo, sourceKind, memoryMiB } = plan);
    } finally { plan.free(); }
    postMessage({ progress: { stage: "render", done: 0, total: totalTiles, width, height, sourceKind, tileWidth, tileHeight, halo, memoryMiB } });

    for (let i = 0; i < totalTiles; i++) {
      const step = await exporter.renderNextTile();
      let done, total;
      try {
        ({ done, total, memoryMiB } = step);
      } finally { step.free(); }
      postMessage({ progress: { stage: "render", done, total, width, height, memoryMiB } });
    }

    postMessage({ progress: { stage: "encode", width, height } });
    const out = exporter.finishJpeg(cropJson, rotation ?? 0, borderJson, quality);
    let jpeg, outWidth, outHeight, finalMemoryMiB;
    try {
      outWidth = out.width;
      outHeight = out.height;
      finalMemoryMiB = out.memoryMiB;
      jpeg = out.takeJpeg();
    } finally { out.free(); }

    exporter.free();
    exporter = null;
    pool.end(wasm);
    postMessage({
      result: {
        jpeg,
        width: outWidth,
        height: outHeight,
        backend,
        sourceKind,
        totalTiles,
        tileWidth,
        tileHeight,
        halo,
        memoryMiB: finalMemoryMiB,
        elapsedMs: performance.now() - started,
      },
    }, [jpeg.buffer]);
  } catch (e) {
    try { exporter?.free(); } catch {}
    pool.end(wasm);
    postMessage({ error: String(e?.message ?? e) });
  }
  close();
};
