// Physical iPhone verification for mobile full-resolution tiled export:
// - Settings UI toggle & persistence
// - Full-resolution 26 MP Fuji X-Trans export + capped 4.4 MP export toggle
// - Full-resolution 60 MP Leica Q3 export (default + Prep + Crop/Rotate + Border)
// - Post-export editing stability & editing engine memory staying flat
// - Crash-recovery fallback to 4.4 MP with notification
// Run via web/safari.mjs:
//   SAFARI_TEST_DIR=target/iphone-export-probe node web/safari.mjs run web/checks/mobile-full-export.mjs
export default async function (test) {
  const { execute, wait, waitFor, probe, log, step } = test;
  const results = {};

  // Capture exported JPEG blobs (size + decoded bitmap dimensions) without
  // retaining them in memory across tests.
  await execute(`
    if (!globalThis.__exportProbeInstalled) {
      globalThis.__exportProbeInstalled = true;
      globalThis.__lastBlob = null;
      const origCreate = URL.createObjectURL.bind(URL);
      URL.createObjectURL = (blob) => {
        if (blob?.type === 'image/jpeg') globalThis.__lastBlob = blob;
        return origCreate(blob);
      };
    }
    return true;
  `);

  async function measureLastJpeg(decodeBitmap = true, seamCheck = null) {
    return execute(`
      return (async () => {
        const blob = globalThis.__lastBlob;
        globalThis.__lastBlob = null;
        if (!blob) throw new Error('No JPEG blob captured');
        const buf = new Uint8Array(await blob.arrayBuffer());
        if (buf[0] !== 0xff || buf[1] !== 0xd8) throw new Error('Not a valid JPEG header');
        let width = 0, height = 0;
        for (let i = 2; i + 8 < buf.length; ) {
          if (buf[i] !== 0xff) { i++; continue; }
          const marker = buf[i + 1];
          if (marker === 0xd9 || marker === 0xda) break;
          const len = (buf[i + 2] << 8) | buf[i + 3];
          if (marker === 0xc0 || marker === 0xc1 || marker === 0xc2) {
            height = (buf[i + 5] << 8) | buf[i + 6];
            width = (buf[i + 7] << 8) | buf[i + 8];
            break;
          }
          i += 2 + len;
        }
        let decoded = false, seam = null;
        if (arguments[0]) {
          const bmp = await createImageBitmap(blob);
          if (bmp.width !== width || bmp.height !== height) throw new Error('Bitmap dims mismatch SOF');
          if (arguments[1]) {
            const { stepX, stepY } = arguments[1];
            const size = 128;
            const c = new OffscreenCanvas(size, size);
            const ctx = c.getContext('2d');
            ctx.drawImage(bmp, stepX - size / 2, stepY - size / 2, size, size, 0, 0, size, size);
            const d = ctx.getImageData(0, 0, size, size).data;
            const colDiff = (x) => {
              let sum = 0;
              for (let y = 0; y < size; y++) {
                const a = (y * size + x - 1) * 4, b = (y * size + x) * 4;
                sum += Math.abs(d[a] - d[b]) + Math.abs(d[a + 1] - d[b + 1]) + Math.abs(d[a + 2] - d[b + 2]);
              }
              return sum / (size * 3);
            };
            const rowDiff = (y) => {
              let sum = 0;
              for (let x = 0; x < size; x++) {
                const a = ((y - 1) * size + x) * 4, b = (y * size + x) * 4;
                sum += Math.abs(d[a] - d[b]) + Math.abs(d[a + 1] - d[b + 1]) + Math.abs(d[a + 2] - d[b + 2]);
              }
              return sum / (size * 3);
            };
            const mid = size / 2;
            seam = {
              vBoundary: Number(colDiff(mid).toFixed(2)),
              vNeighbor: Number(((colDiff(mid - 2) + colDiff(mid + 2)) / 2).toFixed(2)),
              hBoundary: Number(rowDiff(mid).toFixed(2)),
              hNeighbor: Number(((rowDiff(mid - 2) + rowDiff(mid + 2)) / 2).toFixed(2)),
            };
            c.width = c.height = 1;
          }
          bmp.close();
          decoded = true;
        }
        return { width, height, bytes: blob.size, decoded, seam };
      })();
    `, decodeBitmap, seamCheck);
  }

  async function triggerExport(label, { timeout = 300000, decodeBitmap = true, checkSeam = false } = {}) {
    const before = await probe();
    log("export-begin", { label, photo: before.photo, fullExport: await execute("return negs.fullExport();") });
    const started = Date.now();
    await execute("globalThis.__lastBlob = null; document.getElementById('export').click();");
    await waitFor("!document.getElementById('export').disabled && negs.idle()", { timeout });
    const after = await probe();
    test.assertHealthy(after);
    const meta = await execute("return negs.fullExport();");
    const seamCheck = checkSeam && meta?.last && meta.last.totalTiles > 1
      ? { stepX: meta.last.tileWidth - 2 * meta.last.halo, stepY: meta.last.tileHeight - 2 * meta.last.halo }
      : null;
    const jpeg = await measureLastJpeg(decodeBitmap, seamCheck);
    const elapsedMs = Date.now() - started;
    const summary = { label, elapsedMs, jpeg, fullExport: meta, engineInstances: after.engineInstances, hud: after.hud };
    log("export-done", summary);
    return summary;
  }

  // 1. Settings UI check on mobile (clear any stale test flags first)
  await execute("localStorage.removeItem('negs.presentCrashed'); localStorage.removeItem('negs.fullExport'); negs.setFullExport(true);");
  await execute("document.getElementById('settings').click();");
  await wait(300);
  results.settings = await execute(`
    const row = document.getElementById('full-export-setting');
    const sw = document.getElementById('full-export');
    const info = { hidden: row.hidden, checked: sw.checked, stored: localStorage.getItem('negs.fullExport') };
    sw.click();
    const afterOff = { checked: sw.checked, stored: localStorage.getItem('negs.fullExport'), api: negs.fullExport().on };
    sw.click();
    const afterOn = { checked: sw.checked, stored: localStorage.getItem('negs.fullExport'), api: negs.fullExport().on };
    document.getElementById('settings-dialog').close();
    return { info, afterOff, afterOn };
  `);
  log("settings-check", results.settings);
  if (results.settings.info.hidden || !results.settings.info.checked || results.settings.afterOff.api !== false || results.settings.afterOn.api !== true) {
    throw new Error("Settings full-export toggle check failed: " + JSON.stringify(results.settings));
  }

  // 2. 26 MP Fuji X-Trans RAF (6240 × 4160): full-res export AND capped 4.4 MP export
  await step("open 26 MP Fuji RAF", () => execute("negs.open('results/fuji.raf').catch(e => console.error(e));"));
  await waitFor("negs.photo()?.width === 6240 && negs.idle()");
  results.fujiOpened = await execute("return { photo: negs.photo(), exportDims: { w: negs.photo().exportWidth, h: negs.photo().exportHeight }, kept: negs.keptPixels() };");
  log("fuji-opened", results.fujiOpened);

  results.fujiFull = await triggerExport("fuji-26mp-full");
  if (results.fujiFull.jpeg.width !== 6240 || results.fujiFull.jpeg.height !== 4160) {
    throw new Error(`Expected 6240x4160 Fuji export, got ${results.fujiFull.jpeg.width}x${results.fujiFull.jpeg.height}`);
  }
  await step("edit Fuji after 26 MP export", () => execute("negs.edit('camera.exposure_compensation_ev', 0.4);"));

  // Toggle OFF -> capped 4.4 MP export
  await execute("negs.setFullExport(false);");
  results.fujiCapped = await triggerExport("fuji-4mp-capped");
  if (results.fujiCapped.jpeg.width >= 6240 || results.fujiCapped.jpeg.width < 2000) {
    throw new Error(`Expected ~2569x1713 capped Fuji export, got ${results.fujiCapped.jpeg.width}x${results.fujiCapped.jpeg.height}`);
  }
  await execute("negs.setFullExport(true);");

  // 3. 60 MP Leica Q3 DNG (9520 × 6336): full-res default + Prep + Crop/Rotate + Border
  await step("open 60 MP Leica Q3 DNG", () => execute("negs.open('results/leica60mp.dng').catch(e => console.error(e));"), { timeout: 240000 });
  await waitFor("negs.photo()?.width === 9520 && negs.idle()", { timeout: 240000 });
  results.leicaOpened = await execute("return { photo: negs.photo(), hud: document.getElementById('hud').textContent };");
  log("leica-opened", results.leicaOpened);

  results.leicaFullDefault = await triggerExport("leica-60mp-default", { timeout: 420000, decodeBitmap: true, checkSeam: true });
  if (results.leicaFullDefault.jpeg.width !== 9520 || results.leicaFullDefault.jpeg.height !== 6336) {
    throw new Error(`Expected 9520x6336 Leica export, got ${results.leicaFullDefault.jpeg.width}x${results.leicaFullDefault.jpeg.height}`);
  }

  // Apply Prep + Halation + Solid Border (0.03 = 3%) and export 60 MP again!
  await step("apply Prep + Halation + Solid Border on 60 MP Leica", () => execute(`
    negs.start('kodak_portra_400', 'kodak_endura_premier', {
      'prep.highlights': -45,
      'prep.shadows': 35,
      'prep.midtone_detail': 35,
      'film_render.halation.active': true,
    });
    negs.border({ enabled: true, style: 'solid', width: 0.03, export: true, instagram: false });
  `));
  results.leicaFullPrepBorder = await triggerExport("leica-60mp-prep-border", { timeout: 420000, decodeBitmap: false });
  if (results.leicaFullPrepBorder.jpeg.width <= 9520 || results.leicaFullPrepBorder.jpeg.height <= 6336) {
    throw new Error(`Expected bordered >9520x6336 Leica export, got ${results.leicaFullPrepBorder.jpeg.width}x${results.leicaFullPrepBorder.jpeg.height}`);
  }
  await step("edit Leica after 60 MP exports", () => execute("negs.border({ enabled: false }); negs.edit('camera.exposure_compensation_ev', 0.2);"));

  // 4. Crash fallback simulation: block pagehide/visibilitychange (as iOS jetsam does)
  // so negs.alive stays fresh across reload, then verify crashCheck falls back to 4.4 MP.
  await execute(`
    addEventListener('pagehide', (e) => e.stopImmediatePropagation(), true);
    addEventListener('visibilitychange', (e) => e.stopImmediatePropagation(), true);
    localStorage.setItem('negs.fullExport', 'true');
    localStorage.setItem('negs.stockPreviews', 'true');
    localStorage.setItem('negs.alive', JSON.stringify({ previews: true, fullExport: true, exportingFull: true, at: Date.now() }));
    location.reload();
  `);
  await wait(3500);
  await waitFor("!!globalThis.negs && !document.getElementById('engine-info').textContent.includes('Starting')", { healthy: false });
  results.crashFallback = await execute(`
    const msg = document.getElementById('message');
    const info = {
      fullExport: negs.fullExport().on,
      previews: negs.previews().on,
      messageVisible: !msg.hidden,
      messageText: msg.textContent,
    };
    const btn = msg.querySelector('button.link');
    if (btn) btn.click();
    info.settingsOpened = document.getElementById('settings-dialog').open;
    info.switchUnchecked = !document.getElementById('full-export').checked;
    document.getElementById('settings-dialog').close();
    return info;
  `);
  log("crash-fallback", results.crashFallback);
  // Restore clean default state
  await execute("localStorage.removeItem('negs.fullExport'); localStorage.removeItem('negs.stockPreviews');");
  if (results.crashFallback.fullExport !== false || results.crashFallback.previews !== true || !results.crashFallback.messageVisible || !results.crashFallback.messageText.includes("4.4 MP") || !results.crashFallback.settingsOpened || !results.crashFallback.switchUnchecked) {
    throw new Error("Crash fallback check failed: " + JSON.stringify(results.crashFallback));
  }

  return results;
}
