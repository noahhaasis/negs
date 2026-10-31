// Runs the engine off the main thread: rayon can't block the main thread, and
// a panic (RuntimeError: unreachable) kills the module, which the page then
// replaces with a new worker instead of reloading. The engine broadcasts the
// panic's message to the page itself (see crates/spektrafilm-wasm).
import { client } from "./engine.js";
import { threadPool } from "./pool.js";

// rayon's pool workers, which share the module's memory. Safari keeps them,
// and so that memory (about 1 GB), when only this worker is terminated:
// every engine replaced in a tab, or page left, added its memory until a
// new one ran out. `dispose` ends them (web/pool.js).
const pool = threadPool();

let started, engine, wasm;
async function start(kind, threads, instance) {
  wasm = threads ? await import("./pkg-threads/index.js") : await import("./pkg/spektrafilm_wasm.js");
  await wasm.default();
  wasm.setInstance(instance);
  if (threads) await pool.start(wasm, navigator.hardwareConcurrency);
  engine = client(wasm, kind);
  await engine.start();
  return { backend: engine.backend(), threads: threads ? navigator.hardwareConcurrency : 1 };
}

onmessage = async ({ data: { id, method, args } }) => {
  if (method === "dispose") {
    pool.end(wasm);
    return close();
  }
  try {
    if (method === "init") return postMessage({ id, result: await (started = start(...args)) });
    // Calls sent while the engine starts wait for it.
    await started;
    const result = await engine[method](...args);
    postMessage({ id, result }, [result?.rgba?.buffer, result?.scan?.buffer].filter(Boolean));
  } catch (e) {
    postMessage({ id, error: String(e?.message ?? e), code: e?.code, crashed: e instanceof WebAssembly.RuntimeError });
  }
};
