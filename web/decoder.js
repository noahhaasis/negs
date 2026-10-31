// Decodes one photograph and ends: wasm's memory never shrinks, so a decode
// in the engine's module (web/worker.js) leaves its peak there for good. A
// phone keeps a smaller copy than it decodes; this module makes it, the
// engine takes it (`loadDecoded`) and this worker ends, which hands its
// memory back. The page starts it before it's needed, so a photograph
// doesn't wait for the module to compile.

import { threadPool } from "./pool.js";

// rayon's pool workers, ended with this one as web/worker.js ends them:
// Safari keeps a terminated worker waiting in `memory.atomic.wait`, and so
// the memory it shares.
const pool = threadPool();

// The page's first message says whether to start rayon; then a photograph,
// or `dispose` when the page goes away before one.
let wasm, readyMs;
async function start(threads) {
  const begun = performance.now();
  const wasm = threads ? await import("./pkg-threads/index.js") : await import("./pkg/spektrafilm_wasm.js");
  await wasm.default();
  if (threads) await pool.start(wasm, navigator.hardwareConcurrency);
  readyMs = performance.now() - begun;
  return wasm;
}

// `whiteBalance`: io.raw_white_balance as JSON, which a RAW develops at.
onmessage = async ({ data: { dispose, threads, bytes, maxDim, maxPixels, rawDevelop, whiteBalance } }) => {
  if (dispose) {
    pool.end(await wasm?.catch(() => null));
    return close();
  }
  if (!wasm) return void (wasm = start(threads));
  let message;
  try {
    const module = await wasm;
    const t = performance.now();
    // Explicit benchmark switch only; the shipping/default route is unchanged.
    const decoded = rawDevelop === "bounded" || rawDevelop === "force"
      ? module.decodeBounded(bytes, maxDim, maxPixels, rawDevelop === "force", whiteBalance)
      : module.decode(bytes, maxDim, maxPixels, whiteBalance);
    const pixels = decoded.takePixels();
    message = [{ result: { pixels, width: decoded.width, height: decoded.height, fullWidth: decoded.fullWidth, fullHeight: decoded.fullHeight,
      colour: decoded.colour, bounded: decoded.bounded, decodeMs: performance.now() - t, readyMs, memoryMiB: Math.round(module.memoryBytes() / 2 ** 20) } }, [pixels.buffer]];
    decoded.free();
  } catch (e) {
    message = [{ error: String(e?.message ?? e) }];
  }
  pool.end(await wasm.catch(() => null));
  postMessage(...message);
  close();
};
