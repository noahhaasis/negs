// Runs the engine off the main thread: rayon can't block the main thread, and
// a panic (RuntimeError: unreachable) kills the module, which the page then
// replaces with a new worker instead of reloading. The engine broadcasts the
// panic's message to the page itself (see crates/spektrafilm-wasm).
import { client } from "./engine.js";

let started, engine;
async function start(kind, threads, instance) {
  const wasm = threads ? await import("./pkg-threads/index.js") : await import("./pkg/spektrafilm_wasm.js");
  await wasm.default();
  wasm.setInstance(instance);
  if (threads) await wasm.initThreadPool(navigator.hardwareConcurrency);
  engine = client(wasm, kind);
  await engine.start();
  return { backend: engine.backend(), threads: threads ? navigator.hardwareConcurrency : 1 };
}

onmessage = async ({ data: { id, method, args } }) => {
  try {
    if (method === "init") return postMessage({ id, result: await (started = start(...args)) });
    // Calls sent while the engine starts wait for it.
    await started;
    const result = await engine[method](...args);
    postMessage({ id, result }, result?.rgba ? [result.rgba.buffer] : []);
  } catch (e) {
    postMessage({ id, error: String(e?.message ?? e), crashed: e instanceof WebAssembly.RuntimeError });
  }
};
