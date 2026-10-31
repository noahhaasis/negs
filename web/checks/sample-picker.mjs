// node web/cdp.mjs http://localhost:8137/ web/checks/sample-picker.mjs 3000
// Also run with VIEW=390x844@3m. No SW needed: offline storage policy has
// separate asset checks; here offline events must disable online-only inputs.
// With ?sw=1 on a packaged server, also verify actual one-RAW Cache Storage
// and switching back to Sample 1 without a network.
export default async ({ send, evaluate }) => {
  const check = (ok, message) => { if (!ok) throw new Error(message); };
  const initial = await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.sampleReady = async () => {
      for (let i = 0; i < 2400; i++) {
        if (negs.photo() && document.getElementById('samples').getAttribute('aria-busy') === 'false' && negs.idle()) { await wait(150); return; }
        await wait(25);
      }
      throw new Error('Sample did not finish opening');
    };
    globalThis.sampleRequests = () => performance.getEntriesByType('resource').filter((r) => new URL(r.name).pathname.startsWith('/sample/')).map((r) => new URL(r.name).pathname);
    return { requests: sampleRequests(), hidden: document.getElementById('samples').hidden };
  `);
  check(initial.hidden && initial.requests.length === 0, "startup loaded sample data or revealed picker");
  const first = await evaluate(`
    document.getElementById('open-sample').click(); await sampleReady();
    return { selected: document.getElementById('sample-select').value,
      labels: [...document.getElementById('sample-select').options].map((o) => o.textContent), requests: sampleRequests() };
  `);
  check(first.selected === "DSC_6377", "one-click default did not open");
  check(first.labels.join() === Array.from({ length: 7 }, (_, i) => `Sample ${i + 1}`).join(), "picker must use numbers, not photo names");
  check(first.requests.length === 2 && first.requests.includes("/sample/catalog.json") && first.requests.includes("/sample/DSC_6377.dng"), "first click loaded unnecessary sample data");
  const second = await evaluate(`
    negs.edit('film_render.grain.active', false);
    negs.edit('camera.exposure_compensation_ev', 0.3);
    negs.rotate(1);
    globalThis.before = negs.state();
    document.getElementById('sample-next').click(); await wait(50); await sampleReady();
    return { selected: document.getElementById('sample-select').value, photo: negs.photo(),
      film: negs.state().film, paper: negs.state().paper,
      exposure: negs.state().params.camera.exposure_compensation_ev,
      rotation: negs.state().params.io.rotate_cw, crop: negs.crop(), requests: sampleRequests() };
  `);
  check(second.selected === "DSC_6028" && second.photo.width === 2560, "Next did not open Sample 2");
  check(second.exposure === 0.3 && second.rotation === 0 && second.crop.width === 1, "switch did not preserve adjustments/reset finishing");
  check(second.requests.length === 3, "Next prefetched data or refetched catalog");
  check(await evaluate(`return negs.state().film === before.film && negs.state().paper === before.paper;`), "film/paper changed between samples");
  const direct = await evaluate(`
    const s = document.getElementById('sample-select'); s.value = 'IMG_4416'; s.dispatchEvent(new Event('change'));
    await sampleReady();
    return { selected: s.value, photo: negs.photo(), requests: sampleRequests() };
  `);
  check(direct.selected === "IMG_4416" && direct.photo.height === 2560 && direct.requests.length === 4, "direct numbered choice did not open only Sample 7");
  await send("Network.enable");
  await send("Network.emulateNetworkConditions", { offline: true, latency: 0, downloadThroughput: 0, uploadThroughput: 0 });
  const offline = await evaluate(`
    await wait(100);
    return { disabled: [...document.getElementById('sample-select').options].map((o) => o.disabled),
      next: document.getElementById('sample-next').disabled, note: !document.getElementById('sample-offline').hidden };
  `);
  check(offline.next && offline.note && offline.disabled.join() === [false, true, true, true, true, true, true].join(), "offline picker did not restrict choices to Sample 1");
  await send("Network.emulateNetworkConditions", { offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  const wrap = await evaluate(`
    await wait(100); document.getElementById('sample-next').click(); await wait(50); await sampleReady();
    return document.getElementById('sample-select').value;
  `);
  check(wrap === "DSC_6377", "Next did not wrap to Sample 1");
  const local = await evaluate(`
    const c = new OffscreenCanvas(16, 12), ctx = c.getContext('2d'); ctx.fillStyle = '#875'; ctx.fillRect(0, 0, 16, 12);
    const url = URL.createObjectURL(await c.convertToBlob()); await negs.open(url); URL.revokeObjectURL(url);
    for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25);
    return document.getElementById('samples').hidden;
  `);
  check(local, "opening a local photograph left sample controls visible");
  const layout = await evaluate(`
    await negs.openSample(); await sampleReady();
    const root = document.getElementById('samples').getBoundingClientRect(), select = document.getElementById('sample-select').getBoundingClientRect(), next = document.getElementById('sample-next').getBoundingClientRect();
    const fits = select.left >= root.left && select.right <= next.left && next.right <= root.right;
    negs.beginCrop(); const cropHidden = getComputedStyle(document.getElementById('samples')).display === 'none'; negs.endCrop(false);
    return { fits, cropHidden, width: root.width };
  `);
  check(layout.fits && layout.cropHidden, "sample controls overflow or interfere with crop mode");
  let saved = null;
  if (await evaluate(`return new URLSearchParams(location.search).get('sw') === '1';`)) {
    saved = await evaluate(`
      await navigator.serviceWorker.ready;
      for (let i = 0; i < 100 && !navigator.serviceWorker.controller; i++) await wait(50);
      if (!navigator.serviceWorker.controller) throw new Error('No active service worker');
      const s = document.getElementById('sample-select'); s.value = 'DSC_6028'; s.dispatchEvent(new Event('change')); await sampleReady();
      await wait(200);
      const raws = [];
      for (const name of await caches.keys()) for (const request of await (await caches.open(name)).keys()) {
        const path = new URL(request.url).pathname; if (path.endsWith('.dng')) raws.push(path);
      }
      return raws;
    `);
    check(saved.length === 1 && saved[0].endsWith('/sample/DSC_6377.dng'), 'service worker retained more than the default sample');
    await send('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: 0, uploadThroughput: 0 });
    const cached = await evaluate(`
      await wait(100); const s = document.getElementById('sample-select'); s.value = 'DSC_6377'; s.dispatchEvent(new Event('change')); await sampleReady();
      return s.value === 'DSC_6377' && negs.photo().width === 6048;
    `);
    check(cached, 'default sample did not open from offline storage');
    await send('Network.emulateNetworkConditions', { offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  }
  return { initial, first, second, direct, offline, wrap, local, layout, saved };
};
