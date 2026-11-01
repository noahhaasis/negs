// node web/cdp.mjs http://localhost:8137/ web/checks/tutorial.mjs 3000
// Repeat with VIEW=320x568@2m and VIEW=844x390@2m.
export default async ({ send, evaluate }) => {
  const check = (ok, message) => { if (!ok) throw new Error(message); };
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.until = async (test) => {
      for (let i = 0; i < 4000; i++) { if (test()) return; await wait(30); }
      throw new Error('Tutorial timed out: ' + JSON.stringify(negs.tutorial()));
    };
    await until(() => !!globalThis.negs && !!negs.tutorial());
    globalThis.tourReady = async (index) => {
      await until(() => negs.tutorial().phase === 'active' && negs.tutorial().index === index || negs.tutorial().phase.endsWith('error'));
      if (negs.tutorial().phase !== 'active') throw new Error(negs.tutorial().error);
      await wait(250);
    };
  `);
  const fresh = await evaluate(`return {
    invitation: !document.getElementById('tutorial-invitation').hidden,
    requests: performance.getEntriesByType('resource').filter((r) => r.name.endsWith('.dng')).length,
    overflow: document.documentElement.scrollWidth > innerWidth,
    tutorial: document.getElementById('tutorial').getBoundingClientRect().width,
  };`);
  check(fresh.invitation && fresh.requests === 0 && !fresh.overflow && fresh.tutorial > 0, 'first-open invitation/button/loading policy failed');
  const started = await evaluate(`
    document.getElementById('start-tutorial').click(); await tourReady(0);
    return { film: negs.state().film, paper: negs.state().paper, scan: negs.state().params.io.scan_film,
      choice: JSON.parse(localStorage.getItem('negs.tutorial')), shown: !!negs.photo() && !document.getElementById('empty').hidden };
  `);
  check(started.film === 'kodak_portra_400' && started.paper === 'kodak_portra_endura' && !started.scan && !started.shown && started.choice.status === 'started', 'tutorial did not start from rendered default negative');
  const layouts = [];
  for (let i = 0; i < 9; i++) {
    if (i) await evaluate(`document.getElementById('tour-next').click(); await tourReady(${i});`);
    const layout = await evaluate(`
      const nav = document.getElementById('tutorial-nav').getBoundingClientRect();
      const preview = document.getElementById('view').getBoundingClientRect();
      const controls = [...document.querySelectorAll('.tour-control')].filter((n) => n.getBoundingClientRect().height > 0);
      const scroll = document.getElementById('scroll');
      const targets = controls.map((n) => {
        n.scrollIntoView({ block: 'nearest' });
        const r = n.getBoundingClientRect(), s = scroll.getBoundingClientRect();
        return r.left >= -1 && r.right <= innerWidth + 1 && r.bottom <= s.bottom + 1 && r.top >= s.top - 1;
      });
      scroll.scrollTop = 0;
      return { step: negs.tutorial().index, targets, controls: controls.length,
        navFits: nav.left >= 0 && nav.right <= innerWidth + 1 && nav.bottom <= innerHeight + 1 && nav.height >= 44,
        previewHeight: preview.height, overflow: document.documentElement.scrollWidth > innerWidth,
        defaults: negs.state().params.camera.exposure_compensation_ev === 0 && negs.state().params.enlarger.print_exposure === 1 };
    `);
    check(layout.navFits && layout.previewHeight >= 90 && !layout.overflow && layout.defaults && layout.targets.every(Boolean), `layout/defaults failed: ${JSON.stringify(layout)}`);
    check(i === 8 || layout.controls > 0, `no actual control for step ${i}`);
    layouts.push(layout);
    if (i === 0) {
      await evaluate(`
        const film = document.getElementById('film'); film.value = 'fuji_velvia_100';
        const slide = [...film.options].find((o) => /Velvia/.test(o.textContent));
        if (slide) { film.value = slide.value; film.dispatchEvent(new Event('change')); await until(() => negs.idle()); }
      `);
    }
    if (i === 3) {
      await evaluate(`negs.edit('camera.exposure_compensation_ev', 2); await until(() => negs.idle()); document.getElementById('tour-reset').click(); await tourReady(3);`);
      check(await evaluate(`return negs.state().params.camera.exposure_compensation_ev === 0;`), 'Reset step did not reset exposure');
    }
  }
  await evaluate(`
    document.getElementById('more').click(); document.getElementById('export-preset').click();
    await until(() => document.getElementById('export-dialog').open);
  `);
  check(await evaluate(`return negs.tutorial().phase === 'active';`), 'preset dialog ended tutorial');
  await evaluate(`document.getElementById('export-dialog').close(); document.getElementById('tour-next').click(); await until(() => negs.tutorial().phase === 'idle');`);
  check(await evaluate(`return JSON.parse(localStorage.getItem('negs.tutorial')).status === 'completed' && !negs.photo() && !document.getElementById('empty').hidden && !document.getElementById('empty-normal').hidden && document.getElementById('open-empty').getBoundingClientRect().height > 0 && !document.getElementById('tutorial').hidden;`), 'finish did not show photo selection/completion/button');

  // RAW white balance and a nontrivial crop survive the temporary sample.
  await evaluate(`
    await negs.openSample(); await until(() => negs.photo() && negs.idle());
    negs.edit('io.raw_white_balance.mode', 'daylight');
    await negs.whenDeveloped(); await until(() => negs.idle());
    negs.beginCrop();
    const aspect = document.getElementById('crop-aspect'); aspect.value = 'square'; aspect.dispatchEvent(new Event('change'));
    negs.endCrop(true); await until(() => negs.idle());
    globalThis.rawEdit = JSON.stringify(negs.state()); globalThis.rawCrop = JSON.stringify(negs.crop());
    globalThis.rawWb = JSON.stringify(negs.source().wb);
    document.getElementById('tutorial').click(); document.getElementById('confirm-tutorial').click(); await tourReady(0);
    document.getElementById('tour-end').click(); await until(() => negs.tutorial().phase === 'idle');
  `);
  check(await evaluate(`return JSON.stringify(negs.state()) === rawEdit && JSON.stringify(negs.crop()) === rawCrop && JSON.stringify(negs.source().wb) === rawWb && negs.source().current;`), 'RAW white balance/crop restoration failed');

  // A local edit survives a complete tutorial round trip, including finishing.
  await evaluate(`
    const c = new OffscreenCanvas(120, 80), ctx = c.getContext('2d'); ctx.fillStyle = '#a87'; ctx.fillRect(0, 0, 120, 80);
    const url = URL.createObjectURL(await c.convertToBlob()); await negs.open(url); URL.revokeObjectURL(url);
    await until(() => negs.idle());
    negs.edit('camera.exposure_compensation_ev', 0.7); negs.border({ enabled: true, width: 0.035 }); negs.rotate(1);
    await until(() => negs.idle()); negs.view.actualSize(5); await wait(500); await until(() => negs.idle());
    globalThis.originalEdit = JSON.stringify(negs.state()); globalThis.originalBorder = JSON.stringify(negs.border());
    globalThis.originalView = negs.view.snapshot();
    globalThis.originalLayout = { app: document.getElementById('app').clientHeight, sheet: document.getElementById('app').style.getPropertyValue('--sheet'), side: document.getElementById('side').getBoundingClientRect().height, geometry: negs.photoGeometry() };
    globalThis.originalPrefs = ['pane', 'tab', 'sheet', 'groups'].map((k) => localStorage.getItem('negs.' + k));
    document.getElementById('tutorial').click();
    if (!document.getElementById('tutorial-confirm').open) throw new Error('No confirmation before replacing edit');
    document.getElementById('confirm-tutorial').click(); await tourReady(0);
    document.getElementById('tour-end').click(); await until(() => negs.tutorial().phase === 'idle');
  `);
  const restored = await evaluate(`return {
    edit: JSON.stringify(negs.state()) === originalEdit, border: JSON.stringify(negs.border()) === originalBorder,
    photo: negs.photo(), zoom: Math.abs(negs.view.zoom() - originalView.zoom) < 0.001,
    pan: Math.abs(negs.view.snapshot().x - originalView.x) < 1 && Math.abs(negs.view.snapshot().y - originalView.y) < 1,
    beforeView: originalView, afterView: negs.view.snapshot(), originalLayout,
    afterLayout: { app: document.getElementById('app').clientHeight, sheet: document.getElementById('app').style.getPropertyValue('--sheet'), side: document.getElementById('side').getBoundingClientRect().height, geometry: negs.photoGeometry() },
    prefs: JSON.stringify(originalPrefs) === JSON.stringify(['pane', 'tab', 'sheet', 'groups'].map((k) => localStorage.getItem('negs.' + k))),
  };`);
  check(restored.edit && restored.border && restored.zoom && restored.pan && restored.prefs && restored.photo.width === 120, `edit restoration failed: ${JSON.stringify(restored)}`);

  // Immediate cancellation must drain the sample open and keep the old edit.
  await evaluate(`
    document.getElementById('tutorial').click(); document.getElementById('confirm-tutorial').click();
    document.getElementById('tour-end').click(); await until(() => negs.tutorial().phase === 'idle');
    await wait(500);
  `);
  check(await evaluate(`return JSON.stringify(negs.state()) === originalEdit && negs.photo().width === 120;`), 'cancel replaced original edit');

  // An offline/unavailable sample has a retry/exit path, no empty active lesson.
  await send('Network.enable');
  await send('Network.setBlockedURLs', { urls: ['*sample/DSC_6377.dng*'] });
  await evaluate(`
    document.getElementById('tutorial').click(); document.getElementById('confirm-tutorial').click();
    await until(() => negs.tutorial().phase === 'error');
  `);
  check(await evaluate(`return !document.getElementById('tour-retry').hidden && !document.getElementById('tour-end').disabled;`), 'sample failure cannot recover');
  await send('Network.setBlockedURLs', { urls: [] });
  await evaluate(`document.getElementById('tour-retry').click(); await tourReady(0); document.getElementById('tour-end').click(); await until(() => negs.tutorial().phase === 'idle');`);
  check(await evaluate(`return JSON.stringify(negs.state()) === originalEdit;`), 'retry lost original snapshot');
  return { fresh, started, layouts, restored, cancellation: 'passed', retry: 'passed' };
};
