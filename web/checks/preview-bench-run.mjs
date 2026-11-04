// Runs web/checks/preview-bench.js on a device through web/safari.mjs:
//   BENCH_TAG=<tag> BENCH_IMAGE=/results/fuji.raf [BENCH_SIZES=1200,2048] node web/safari.mjs run web/checks/preview-bench-run.mjs
// The page posts every record to /device-log?run=<tag>; this only starts it and waits.
export default async function (test) {
  const { execute, wait, log } = test;
  const env = test.env ?? globalThis.process?.env ?? {};
  const tag = env.BENCH_TAG ?? `bench-${Date.now()}`;
  const options = { image: env.BENCH_IMAGE ?? "/results/fuji.raf", tag };
  if (env.BENCH_SIZES) options.sizes = env.BENCH_SIZES.split(",").map(Number);
  if (env.BENCH_EXPORT === "0") options.exportFrame = false;
  for (let i = 0; i < 120; i++) {
    const ready = await execute("return !!globalThis.negs && /thread|CPU|GPU/i.test(document.getElementById('engine-info')?.textContent ?? '');").catch(() => false);
    if (ready) break;
    await wait(500);
  }
  await wait(1500);
  await execute("globalThis.__bench = null; import('/checks/preview-bench.js').then((m) => m.default(arguments[0]));", options);
  log("bench-started", options);
  let last = -1;
  for (let i = 0; i < 400; i++) {
    await wait(3000);
    const s = await execute("const b = globalThis.__bench; return b && { done: b.done, n: b.records.length, error: b.error, last: b.records.at(-1)?.event };").catch((e) => ({ lost: String(e) }));
    if (s?.n !== last || s?.done || s?.lost) log("bench-progress", s ?? {});
    last = s?.n;
    if (s?.done || s?.lost) return s;
  }
  return { timeout: true };
}
