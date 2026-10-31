// Run: node --test web/checks/pool.test.mjs
// rayon's pool workers (web/pool.js). On a physical iPhone (Safari 26.6.1),
// only the first of several same-URL workers a worker starts went through
// the service worker; offline the others never loaded and gave no error, so
// the threaded engine stayed "Starting…" (docs/iphone-offline-fix-handoff.md).
import test from "node:test";
import assert from "node:assert/strict";
import { threadPool } from "../pool.js";
import { role } from "../offline.mjs";

function scope() {
  const made = [];
  class Worker {
    constructor(url, options) {
      this.url = url;
      this.options = options;
      this.terminated = false;
      made.push(this);
    }
    terminate() { this.terminated = true; }
  }
  return { made, Worker, location: { href: "https://negs.test/worker.js" } };
}

test("each pool worker loads its helper at its own URL, the same file", () => {
  const s = scope();
  threadPool(s);
  // As wasm-bindgen-rayon's startWorkers makes them: several at once, at one URL.
  const helper = new URL("https://negs.test/pkg-threads/snippets/wasm-bindgen-rayon-x/src/workerHelpers.js");
  const workers = Array.from({ length: 4 }, () => new s.Worker(helper, { type: "module" }));
  const urls = workers.map((w) => String(w.url));
  assert.equal(new Set(urls).size, 4, "no two share a URL");
  for (const w of workers) {
    assert.equal(w.url.pathname, helper.pathname, "the service worker and servers match by path");
    assert.equal(w.url.origin, helper.origin);
    assert.deepEqual(w.options, { type: "module" });
  }
  // Relative URLs resolve against the creating worker, as `new Worker` does.
  assert.equal(String(new s.Worker("pkg-threads/x.js").url), "https://negs.test/pkg-threads/x.js?pool=4");
});

test("a pool whose workers never load fails instead of starting forever", async () => {
  const pool = threadPool(scope(), 20);
  const never = { initThreadPool: () => new Promise(() => {}) };
  await assert.rejects(pool.start(never, 4), (e) => e.code === "pool" && /4 threads didn't start/.test(e.message));
});

test("a pool that starts in time starts, and leaves no timer behind", async () => {
  const pool = threadPool(scope(), 60_000);
  let asked;
  await pool.start({ initThreadPool: async (n) => { asked = n; } }, 3);
  assert.equal(asked, 3);
  // node --test would wait out a leaked 60 s timer; this test's duration shows it.
});

test("a pool's own failure passes through unchanged", async () => {
  const pool = threadPool(scope(), 60_000);
  await assert.rejects(pool.start({ initThreadPool: async () => { throw new Error("Out of memory"); } }, 4), (e) => e.message === "Out of memory" && !e.code);
});

test("ending a pool terminates every worker, then wakes them", () => {
  const s = scope();
  const pool = threadPool(s);
  for (let i = 0; i < 4; i++) new s.Worker("pkg-threads/h.js");
  const order = [];
  pool.end({ wakeThreadPool: () => order.push(s.made.every((w) => w.terminated)) });
  assert.deepEqual(order, [true], "woken once, after all were terminated");
});

test("ending a pool tolerates no module and a failing wake", () => {
  const s = scope();
  const pool = threadPool(s);
  new s.Worker("pkg-threads/h.js");
  pool.end(null);
  pool.end({ wakeThreadPool: () => { throw new Error("no pool"); } });
  assert.ok(s.made[0].terminated);
});

test("the pool module is saved for offline use when the worker installs", () => {
  assert.equal(role("pool.js"), "install");
});
