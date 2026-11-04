// TEMP(previews memory): does changing the film, then the paper, grow the
// wasm memory as the previews' calibrations do? Previews off, the Fuji RAF.
// Rows go to web/results/device-iphone-stock-change.jsonl.
//   node web/safari.mjs run web/checks/stock-change-memory.mjs
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
      const row = async (event) => {
        const r = { at: Date.now(), event, wasmMiB: await negs.engineCall('memoryMiB'), film: negs.state().film, paper: negs.state().paper };
        __s.rows.push(r);
        await fetch('/device-log?run=iphone-stock-change', { method: 'POST', body: JSON.stringify(r) }).catch(() => {});
      };
      const settled = async () => { for (let i = 0; i < 1200 && (negs.match().pending || negs.match().solving || !negs.idle()); i++) await sleep(50); await sleep(20000); };
      try {
        negs.setPreviews(false);
        await negs.open('/results/fuji.raf');
        await negs.whenDeveloped();
        await settled();
        await row('developed');
        for (const film of ['kodak_gold_200', 'fujifilm_pro_400h', 'kodak_ektar_100']) {
          negs.replaceSettings({ film });
          await settled();
          await row('film ' + film);
        }
        for (const paper of ['fujifilm_crystal_archive_typeii', 'kodak_supra_endura']) {
          negs.replaceSettings({ paper });
          await settled();
          await row('paper ' + paper);
        }
      } catch (e) { __s.rows.push({ event: 'error', error: String(e?.stack ?? e) }); }
      __s.done = true;
    })();`);
  for (let i = 0; i < 100; i++) {
    await wait(5000);
    const s = await execute("return globalThis.__s && { done: __s.done, rows: __s.rows };").catch((e) => ({ lost: String(e) }));
    if (s?.done || s?.lost) { log("stock-change", s); return s; }
  }
  return { timeout: true };
}
