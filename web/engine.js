// The engine inside the worker (web/worker.js): fetch whatever data files a
// render asks for, then retry.
export function client(wasm, kind) {
  let engine;
  return {
    async start() {
      engine = kind === "gpu" ? await wasm.Engine.newGpu() : wasm.Engine.newCpu();
    },
    backend: () => engine.backend(),
    setDiffusionGridBudget: (bytes) => engine.setDiffusionGridBudget(bytes),
    // An OffscreenCanvas the WebGPU engine presents renders on.
    setCanvas: (canvas) => engine.setCanvas(canvas),
    // Where that canvas shows the photograph, drawn again at once without
    // the film chain; and where it drew the last frame.
    show: (width, height, rect, background, original = false, border = []) => engine.show(width, height, rect, background, original, border),
    // A border texture's PNG at its data path, kept for exports, and on the
    // canvas for views whose border has one.
    borderTexture(path, bytes) {
      wasm.registerData(path, bytes);
      return [...engine.borderTexture(path)];
    },
    // A border around an export's bytes, as the CLI adds one.
    async addBorder(rgba, width, height, style, options, instagram, film) {
      for (;;) {
        const r = wasm.addBorder(rgba, width, height, style, options, instagram, film);
        if (!r.missing.length) {
          return { width: r.width, height: r.height, rgba: r.rgba, elapsedMs: r.elapsedMs, memoryMiB: Math.round(wasm.memoryBytes() / 2 ** 20) };
        }
        await fetchData(wasm, r.missing);
      }
    },
    shownRect: () => [...engine.shownRect()],
    // The largest frame the WebGPU engine renders and shows.
    limits: () => [...engine.limits()],
    // `width` × `height` as decoded; the engine keeps `kept` × `keptHeight`,
    // within `maxDim` and `maxPixels` (0: no limit).
    load(bytes, maxDim, maxPixels = 0) {
      const t = performance.now();
      engine.forget();
      const [kept, keptHeight, width, height] = engine.load(bytes, maxDim, maxPixels);
      return { width, height, kept, keptHeight, decodeMs: performance.now() - t };
    },
    // A photograph web/decoder.js decoded, as `load` keeps one. The last
    // goes before its pixels are copied in, which then take its room.
    loadDecoded({ pixels, width, height, fullWidth, fullHeight, colour }) {
      engine.forget();
      engine.loadDecoded(pixels, width, height, colour);
      return { width: fullWidth, height: fullHeight, kept: width, keptHeight: height };
    },
    async render(film, paper, params, maxDim, interactive, present = false) {
      for (;;) {
        const r = await engine.render(film, paper, params, maxDim, interactive, present);
        if (!r.missing.length) {
          return { width: r.width, height: r.height, rgba: r.rgba, presented: r.presented, backend: r.backend, elapsedMs: r.elapsedMs, stages: r.stages,
            memoryMiB: Math.round(wasm.memoryBytes() / 2 ** 20) };
        }
        await fetchData(wasm, r.missing);
      }
    },
    // Unquantised scan for golden tests; normal renders keep their byte-only
    // readback/presentation path. Free the wasm wrapper after copying its fields.
    async renderRaw(film, paper, params, maxDim) {
      for (;;) {
        const r = await engine.renderRaw(film, paper, params, maxDim);
        const missing = r.missing;
        if (!missing.length) {
          const out = { width: r.width, height: r.height, scan: r.scan, rgba: r.rgba, backend: r.backend, stages: r.stages };
          r.free();
          return out;
        }
        r.free();
        await fetchData(wasm, missing);
      }
    },
    // The photograph before any film, kept on the canvas to show in the
    // developed frame's place when `present`; otherwise its bytes.
    original(params, maxDim, present = false) {
      const r = engine.original(params, maxDim, present);
      return { width: r.width, height: r.height, rgba: r.rgba, presented: r.presented, elapsedMs: r.elapsedMs, stages: r.stages };
    },
    // The M/Y shifts that print a mid-grey neutral (Balance Print).
    async balancePrint(film, paper, params) {
      for (;;) {
        const r = wasm.balancePrint(film, paper, params);
        if (!r.missing.length) return { m: r.mFilterShift, y: r.yFilterShift, elapsedMs: r.elapsedMs };
        await fetchData(wasm, r.missing);
      }
    },
  };
}

async function fetchData(wasm, paths) {
  await Promise.all(paths.map(async (path) => {
    const res = await fetch(new URL(`data/${path}`, location.origin));
    if (!res.ok) throw fetchFailed(path, res);
    wasm.registerData(path, new Uint8Array(await res.arrayBuffer()));
  }));
}

// Why a file didn't arrive: the service worker (web/sw.js) says when it's
// offline and the file isn't saved, or when a newer Negs replaced it.
export function fetchFailed(path, res) {
  const why = res.headers.get("X-Negs-Offline");
  const message = why === "updated" ? `${path} has changed in a newer Negs; reload to use it`
    : why ? `${path} isn't saved for offline use; connect to the internet to download it`
    : `${path}: ${res.status}`;
  return Object.assign(new Error(message), { status: res.status, offline: !!why });
}
