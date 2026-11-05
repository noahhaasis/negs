// node web/cdp.mjs http://localhost:8137/ web/checks/stock-search.mjs 3000
// The Film foldout's search (macOS StockFoldout): filters by name, id and
// group, hides the favourites, Return takes the first match, Escape clears.
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
    fold.querySelector('.stock-star').click();
    const type = (text) => { field.value = text; field.dispatchEvent(new Event('input')); };
    const shown = () => [...fold.querySelectorAll('.stock-grid:not(.stock-favorites) .stock-cell:not([hidden]) .stock-tile')].map((b) => b.dataset.id);
    const all = shown().length;
    type('portra');
    const portra = shown(), favHidden = fold.querySelector('.stock-favorites').hidden, headings = [...fold.querySelectorAll('h3:not([hidden])')].length;
    type('fuji  400');
    const fuji400 = shown();
    type('zzzz');
    const none = shown().length, empty = fold.querySelector('.stock-empty');
    const emptyText = !empty.hidden && empty.textContent;
    type('ektar');
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await quiet();
    const chosen = negs.state().film;
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    const cleared = field.value === '' && shown().length === all && !fold.querySelector('.stock-favorites').hidden;
    fold.querySelector('.stock-favorites .stock-star').click();
    return { all, portra, favHidden, headings, fuji400, none, emptyText, chosen, cleared, papers: document.querySelectorAll('.stock-search').length };
  `);
  console.log(JSON.stringify(r));
  check(r.portra.length > 0 && r.portra.length < r.all && r.portra.every((id) => id.includes("portra")), "portra filters");
  check(r.favHidden, "favourites hidden while searching");
  check(r.fuji400.length > 0 && r.fuji400.every((id) => id.includes("fuji") && id.includes("400")), "words all match");
  check(r.none === 0 && r.emptyText === "No films match “zzzz”", "no matches message");
  check(r.chosen.includes("ektar"), "Return chooses the first match");
  check(r.cleared, "Escape clears");
  check(r.papers === 1, "only Film has a search");
  return "ok";
};
