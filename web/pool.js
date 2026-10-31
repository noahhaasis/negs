// rayon's pool workers, as web/worker.js and web/decoder.js start them
// (wasm-bindgen-rayon's `initThreadPool`, which makes them with `new Worker`).
//
// Each gets its own URL (`?pool=<n>`). Safari 26 hands only the first of
// several workers a worker starts at the same URL to the service worker;
// the others load from the network, which works online and offline
// never loads at all, without an error: rayon waited for them forever.
// The service worker and the servers match files by path, so the query
// changes nothing else.
//
// Ending: Safari keeps a terminated pool worker waiting in
// `memory.atomic.wait`, and so the memory it shares (about 1 GB), until
// something wakes it. `end` terminates them and then wakes them.
export const POOL_TIMEOUT_MS = 30_000;

export function threadPool(scope = globalThis, timeoutMs = POOL_TIMEOUT_MS) {
  const workers = [];
  scope.Worker = class extends scope.Worker {
    constructor(url, options) {
      url = new URL(url, scope.location.href);
      url.searchParams.set("pool", workers.length);
      super(url, options);
      workers.push(this);
    }
  };
  return {
    // A worker that never loads says nothing; this says so instead of
    // starting forever.
    async start(wasm, threads) {
      let timer;
      const late = new Promise((_, reject) => {
        timer = setTimeout(() => reject(Object.assign(new Error(`its ${threads} threads didn't start`), { code: "pool" })), timeoutMs);
      });
      try {
        await Promise.race([wasm.initThreadPool(threads), late]);
      } finally {
        clearTimeout(timer);
      }
    },
    end(wasm) {
      for (const worker of workers) worker.terminate();
      try {
        wasm?.wakeThreadPool?.();
      } catch {}
    },
  };
}
