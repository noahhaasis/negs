// Where the film and paper previews' memory goes on a phone
// (docs/handoff-web-previews-memory.md): the Fuji RAF, both grids filled as
// web/checks/stocks-phone.mjs fills them, then long idle plateaus with one
// thing let go before each, and an export with the grids still open. Pair
// with web/checks/footprint.py; each step is logged to
// web/results/device-iphone-stocks-memory.jsonl.
//   ?previews=off      the same steps with the previews off (the baseline)
//   ?previewPaint=0    tiles render but aren't painted
//   node web/safari.mjs run web/checks/stocks-memory-phone.mjs
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
      const t0 = performance.now();
      const row = async (event, extra = {}) => {
        const p = negs.previews();
        const r = { at: Date.now(), event, t: Math.round(performance.now() - t0), tiles: p.stats.length, current: p.current, onScreen: p.onScreen,
          wasmMiB: await negs.engineCall('memoryMiB').catch(() => null), ...extra };
        __s.rows.push(r);
        await fetch('/device-log?run=iphone-stocks-memory', { method: 'POST', body: JSON.stringify(r) }).catch(() => {});
      };
      // A plateau: the step's footprint once it has settled.
      const hold = async (event, ms, extra) => { await row(event + ' start', extra); await sleep(ms); await row(event + ' end'); };
      const settled = async () => { for (let i = 0; i < 1200 && (negs.match().pending || negs.match().solving || !negs.idle()); i++) await sleep(50); };
      const scroll = document.getElementById('scroll');
      const fill = async () => {
        for (let pass = 0; pass < 60; pass++) {
          for (let i = 0; i < 1200; i++) { const p = negs.previews(); if (!p.on || (p.onScreen && !p.previewing && p.current + p.failed === p.onScreen)) break; await sleep(25); }
          if (scroll.scrollTop + scroll.clientHeight >= scroll.scrollHeight - 2) break;
          scroll.scrollTop += scroll.clientHeight * 0.8;
          await sleep(150);
        }
        scroll.scrollTop = 0;
        await sleep(150);
      };
      try {
        negs.setPreviews(!location.search.includes('previews=off'));
        await negs.open('/results/fuji.raf');
        await negs.whenDeveloped();
        await settled();
        await hold('developed', 30000);
        [...document.querySelectorAll('#tabs button')].find((b) => b.dataset.tab === 'stock')?.click();
        negs.openStocks(true, true);
        await fill();
        negs.edit('enlarger.print_exposure', 0.3);
        await settled();
        await fill();
        negs.edit('enlarger.print_exposure', 0);
        await settled();
        await fill();
        await hold('filled', 60000, { outdated: negs.previews().outdated.length, wasmByTile: negs.previews().stats.map((s) => s.memoryMiB) });
        // ?reopen=1: open the photograph again with the grids open, instead
        // of the export and the releases.
        if (location.search.includes('reopen=1')) {
          await negs.open('/results/fuji.raf');
          await negs.whenDeveloped();
          await settled();
          await hold('reopened', 20000);
          await fill();
          await hold('refilled', 30000);
          __s.done = true;
          return;
        }
        // An export with both grids open: exportJpeg's render, finish and
        // encode, without the download.
        const e0 = performance.now();
        const r = await negs.exportBytes();
        const canvas = new OffscreenCanvas(r.width, r.height);
        canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height), 0, 0);
        const blob = await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.92 });
        canvas.width = canvas.height = 1;
        await hold('exported', 30000, { width: r.width, height: r.height, jpegBytes: blob.size, exportMs: Math.round(performance.now() - e0) });
        await settled();
        // Let go of one thing at a time.
        for (const c of document.querySelectorAll('.stock-thumb')) { c.width = 0; c.height = 0; }
        await hold('canvases blanked', 30000);
        negs.setPreviews(false);
        await sleep(500);
        await hold('previews off (thumbnails released)', 30000);
        await negs.engineCall('releaseRenderBuffers');
        await hold('render buffers released', 30000);
      } catch (e) { await row('error', { error: String(e?.stack ?? e) }); }
      __s.done = true;
    })();`);
  for (let i = 0; i < 200; i++) {
    await wait(5000);
    const s = await execute("return globalThis.__s && { done: __s.done, n: __s.rows.length, last: __s.rows.at(-1) };").catch((e) => ({ lost: String(e) }));
    log("stocks-memory-progress", s ?? {});
    if (s?.done || s?.lost) return s;
  }
  return { timeout: true };
}
