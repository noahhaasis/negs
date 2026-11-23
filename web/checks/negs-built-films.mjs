// node web/cdp.mjs http://localhost:8137/ web/checks/negs-built-films.mjs 3000
// Settings › Show Negs-built films (macOS NegsBuiltFilms): off by default, the
// films and print films Negs prepared itself from datasheets are left out of
// the Film and Paper menus, the grids and the film search; on, they are
// listed; the stock in use stays listed.
export default async ({ evaluate }) => {
  const check = (ok, message) => { if (!ok) throw new Error(message); };
  const r = await evaluate(`
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    const quiet = async () => { await wait(450); for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25); await wait(100); };
    localStorage.removeItem('negs.showNegsBuiltFilms');
    const canvas = new OffscreenCanvas(96, 64), ctx = canvas.getContext('2d');
    ctx.fillStyle = '#8a7'; ctx.fillRect(0, 0, 96, 64);
    const url = URL.createObjectURL(await canvas.convertToBlob());
    await negs.open(url); URL.revokeObjectURL(url); await quiet();
    negs.start('kodak_portra_400', 'kodak_portra_endura'); await quiet();
    const fold = document.querySelector('.stock-fold'), field = fold.querySelector('.stock-search');
    fold.open = true;
    const setting = document.getElementById('negs-built-films');
    const tick = async (on) => { setting.checked = on; setting.dispatchEvent(new Event('change')); await quiet(); };
    const menu = () => [...document.querySelectorAll('#film optgroup:not(.favorites) option')].map((o) => o.value);
    const paperFold = document.querySelectorAll('.stock-fold')[1];
    const papers = () => [...document.querySelectorAll('#paper optgroup:not(.favorites) option')].map((o) => o.value);
    const paperGrid = () => [...paperFold.querySelectorAll('.stock-grid:not(.stock-favorites) .stock-cell:not([hidden]) .stock-tile')].map((b) => b.dataset.id);
    const grid = () => [...fold.querySelectorAll('.stock-grid:not(.stock-favorites) .stock-cell:not([hidden]) .stock-tile')].map((b) => b.dataset.id);
    const type = (text) => { field.value = text; field.dispatchEvent(new Event('input')); };
    const look = () => ({ menu: menu(), grid: grid(), papers: papers(), paperGrid: paperGrid() });
    const off = look();
    type('eterna 250d');
    const search = grid();
    type('');
    await tick(true);
    const on = look();
    await tick(false);
    negs.start('fujifilm_eterna_250d', 'fujifilm_eterna_cp_3513di'); await quiet();
    const inUse = look();
    negs.start('kodak_portra_400', 'kodak_portra_endura'); await quiet();
    const after = look();
    return { off, search, on, inUse, after, saved: localStorage.getItem('negs.showNegsBuiltFilms') };
  `);
  console.log(JSON.stringify({ off: r.off.menu.length, on: r.on.menu.length, inUse: r.inUse.menu.length, after: r.after.menu.length, search: r.search, saved: r.saved }));
  const has = (list, id) => list.includes(id);
  for (const view of ["menu", "grid"]) {
    check(!has(r.off[view], "fujifilm_eterna_250d") && has(r.off[view], "fujifilm_eterna_500"), `${view}: off by default hides the Negs-built films only`);
    check(has(r.on[view], "fujifilm_eterna_250d") && r.on[view].length > r.off[view].length, `${view}: on lists them`);
    check(has(r.inUse[view], "fujifilm_eterna_250d") && r.inUse[view].length === r.off[view].length + 1, `${view}: the film in use stays listed`);
    check(!has(r.after[view], "fujifilm_eterna_250d"), `${view}: and leaves once another is chosen`);
  }
  for (const view of ["papers", "paperGrid"]) {
    check(!has(r.off[view], "fujifilm_eterna_cp_3513di") && has(r.off[view], "kodak_2383"), `${view}: off by default hides the Negs-built prints only`);
    check(has(r.on[view], "fujifilm_eterna_cp_3513di") && r.on[view].length === r.off[view].length + 5, `${view}: on lists the five`);
    check(has(r.inUse[view], "fujifilm_eterna_cp_3513di") && r.inUse[view].length === r.off[view].length + 1, `${view}: the print in use stays listed`);
    check(!has(r.after[view], "fujifilm_eterna_cp_3513di"), `${view}: and leaves once another is chosen`);
  }
  check(!has(r.search, "fujifilm_eterna_250d"), "the search leaves hidden films out");
  check(r.saved === "false", "the choice is remembered");
  return "ok";
};
