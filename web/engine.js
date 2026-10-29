// The engine inside the worker (web/worker.js): fetch whatever data files a
// render asks for, then retry.
export function client(wasm, kind) {
  let engine;
  return {
    async start() {
      engine = kind === "gpu" ? await wasm.Engine.newGpu() : wasm.Engine.newCpu();
    },
    backend: () => engine.backend(),
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
    load(bytes, maxDim) {
      const t = performance.now();
      const [width, height] = engine.load(bytes, maxDim);
      return { width, height, decodeMs: performance.now() - t };
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
    if (!res.ok) throw new Error(`${path}: ${res.status}`);
    wasm.registerData(path, new Uint8Array(await res.arrayBuffer()));
  }));
}
