// The film and paper previews on a phone (docs/handoff-web-previews-memory.md):
// the Fuji RAF, both grids filled, an edit, a slider moving while tiles fill and
// a tile chosen, each step logged to web/results/device-iphone-stocks.jsonl.
// URL ?previews=off runs the same steps with the previews off (the baseline).
//   node web/safari.mjs run web/checks/stocks-phone.mjs
export default async function (test) {
  const { execute, wait, log } = test;
  for (let i = 0; i < 120; i++) {
    if (await execute("return !!globalThis.negs && /thread|CPU|GPU/i.test(document.getElementById('engine-info')?.textContent ?? '');").catch(() => false)) break;
    await wait(500);
  }
  await wait(1500);
  await execute(`globalThis.__s = { done: false, rows: [] };
    (async () => {
      const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
      const row = (event, extra = {}) => { const p = negs.previews(); const r = { at: Date.now(), event, t: Math.round(performance.now() - t0), tiles: p.stats.length, onScreen: p.onScreen, current: p.current, failed: p.failed, memoryMiB: negs.renders.at(-1)?.memoryMiB, tileMiB: p.stats.at(-1)?.memoryMiB, ...extra }; __s.rows.push(r); fetch('/device-log?run=iphone-stocks', { method: 'POST', body: JSON.stringify(r) }).catch(() => {}); };
      const settled = async () => { for (let i = 0; i < 1200 && (negs.match().pending || negs.match().solving || !negs.idle()); i++) await sleep(50); };
      const scroll = document.getElementById('scroll');
      const fill = async () => {
        for (let pass = 0; pass < 60; pass++) {
          for (let i = 0; i < 1200; i++) { const p = negs.previews(); if (p.onScreen && !p.previewing && p.current + p.failed === p.onScreen) break; await sleep(25); }
          if (scroll.scrollTop + scroll.clientHeight >= scroll.scrollHeight - 2) break;
          scroll.scrollTop += scroll.clientHeight * 0.8;
          await sleep(150);
        }
        scroll.scrollTop = 0;
        await sleep(150);
      };
      const ms = (a) => a.map((s) => Math.round(s.elapsedMs));
      const med = (a) => [...a].sort((x, y) => x - y)[a.length >> 1];
      const t0 = performance.now();
      try {
        negs.setPreviews(!location.search.includes('previews=off'));
        await negs.open('/results/fuji.raf');
        await negs.whenDeveloped();
        await settled();
        row('developed');
        [...document.querySelectorAll('#tabs button')].find((b) => b.dataset.tab === 'stock')?.click();
        negs.openStocks(true, true);
        const f0 = performance.now();
        await fill();
        let s = negs.previews().stats;
        row('first fill', { outdated: negs.previews().outdated, fillMs: Math.round(performance.now() - f0), median: med(ms(s)), max: Math.max(...ms(s)), stages: s.slice(0, 2).map((x) => x.stages) });
        let n = s.length, e0 = performance.now();
        negs.edit('enlarger.print_exposure', 0.3);
        await settled();
        await fill();
        s = negs.previews().stats.slice(n);
        row('edit fill', { outdated: negs.previews().outdated, fillMs: Math.round(performance.now() - e0), median: med(ms(s)), max: Math.max(...ms(s)), stages: s.slice(0, 2).map((x) => x.stages) });
        // A slider moving while tiles fill: the first frame's wait.
        n = negs.previews().stats.length;
        negs.edit('enlarger.print_exposure', 0);
        await settled();
        await sleep(600);
        const r0 = negs.renders.length, d0 = performance.now();
        negs.edit('prep.shadows', 20);
        for (let i = 0; i < 400 && negs.renders.length === r0; i++) await sleep(5);
        row('frame during fill', { waitMs: Math.round(performance.now() - d0), render: negs.renders.at(-1)?.elapsedMs });
        await settled();
        await fill();
        row('after drag fill');
        // Choose a tile's film: the render takes its pipeline.
        const f1 = performance.now(), r1 = negs.renders.length;
        document.querySelector('.stock-tile[data-id="kodak_gold_200"]')?.click();
        for (let i = 0; i < 400 && negs.renders.length === r1; i++) await sleep(5);
        row('chose tile', { waitMs: Math.round(performance.now() - f1), render: negs.renders.at(-1)?.elapsedMs, stages: negs.renders.at(-1)?.stages });
        await settled();
        await fill();
        row('after choice fill');
      } catch (e) { row('error', { error: String(e?.stack ?? e) }); }
      __s.done = true;
    })();`);
  for (let i = 0; i < 200; i++) {
    await wait(3000);
    const s = await execute("return globalThis.__s && { done: __s.done, n: __s.rows.length, last: __s.rows.at(-1) };").catch((e) => ({ lost: String(e) }));
    log("stocks-progress", s ?? {});
    if (s?.done || s?.lost) return s;
  }
  return { timeout: true };
}
