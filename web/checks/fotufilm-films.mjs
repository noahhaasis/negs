// node web/cdp.mjs http://localhost:8137/ web/checks/fotufilm-films.mjs 3000
// Settings › Fotufilm films (macOS FotufilmFilms): From datasheets hides the
// films Fotufilm models from samples, None every Fotufilm film, from the
// Film menu, the grid and its search; the film in use stays listed.
export default async ({ evaluate }) => {
  const check = (ok, message) => { if (!ok) throw new Error(message); };
  const r = await evaluate(`
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    const quiet = async () => { await wait(450); for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25); await wait(100); };
    const canvas = new OffscreenCanvas(96, 64), ctx = canvas.getContext('2d');
    ctx.fillStyle = '#8a7'; ctx.fillRect(0, 0, 96, 64);
    const url = URL.createObjectURL(await canvas.convertToBlob());
    await negs.open(url); URL.revokeObjectURL(url); await quiet();
    negs.start('kodak_portra_400', 'kodak_portra_endura'); await quiet();
    const fold = document.querySelector('.stock-fold'), field = fold.querySelector('.stock-search');
    fold.open = true;
    const setting = document.getElementById('fotufilm-films');
    const choose = async (value) => { setting.value = value; setting.dispatchEvent(new Event('change')); await quiet(); };
    const menu = () => [...document.querySelectorAll('#film optgroup:not(.favorites) option')].map((o) => o.value);
    const grid = () => [...fold.querySelectorAll('.stock-grid:not(.stock-favorites) .stock-cell:not([hidden]) .stock-tile')].map((b) => b.dataset.id);
    const type = (text) => { field.value = text; field.dispatchEvent(new Event('input')); };
    const look = () => ({ menu: menu(), grid: grid() });
    const all = look();
    await choose('datasheets');
    const datasheets = look();
    type('cinestill');
    const search = grid();
    type('');
    await choose('off');
    const off = look();
    negs.start('cinestill_400d', 'kodak_portra_endura'); await quiet();
    const inUse = look();
    negs.start('kodak_portra_400', 'kodak_portra_endura'); await quiet();
    const after = look();
    await choose('all');
    const back = look();
    return { all, datasheets, search, off, inUse, after, back, saved: localStorage.getItem('negs.fotufilmFilms') ?? Object.keys(localStorage).filter((k) => k.includes('otufilm')) };
  `);
  console.log(JSON.stringify({ ...r, all: r.all.menu.length, datasheets: r.datasheets.menu.length, off: r.off.menu.length,
    inUse: r.inUse.menu.length, after: r.after.menu.length, back: r.back.menu.length }));
  const has = (list, id) => list.includes(id);
  for (const view of ["menu", "grid"]) {
    check(has(r.all[view], "cinestill_400d") && has(r.all[view], "fujifilm_velvia_50"), `${view}: All lists every film`);
    check(!has(r.datasheets[view], "cinestill_400d") && !has(r.datasheets[view], "lomography_lomochrome_turquoise"), `${view}: From datasheets hides sampled films`);
    check(has(r.datasheets[view], "fujifilm_velvia_50") && has(r.datasheets[view], "kodak_portra_400"), `${view}: From datasheets keeps the rest`);
    check(r.all[view].length - r.datasheets[view].length === 8, `${view}: eight sampled films`);
    check(r.all[view].length - r.off[view].length === 29 && has(r.off[view], "kodak_portra_400"), `${view}: None hides every Fotufilm film`);
    check(has(r.inUse[view], "cinestill_400d") && r.inUse[view].length === r.off[view].length + 1, `${view}: the film in use stays listed`);
    check(!has(r.after[view], "cinestill_400d"), `${view}: and leaves once another is chosen`);
    check(r.back[view].length === r.all[view].length, `${view}: All brings them back`);
  }
  check(r.search.length === 0, "the search leaves hidden films out");
  return "ok";
};
