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
        await Promise.all(r.missing.map(async (path) => {
          const res = await fetch(new URL(`data/${path}`, location.origin));
          if (!res.ok) throw new Error(`${path}: ${res.status}`);
          wasm.registerData(path, new Uint8Array(await res.arrayBuffer()));
        }));
      }
    },
  };
}
