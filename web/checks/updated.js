// A pipeline updated for the enlarger's shifts, preflash and exposure
// compensation, against one built for them. Edits each case of
// checks/enlarger-cases.json in this page (the pipeline is kept) and saves
// the 1200 px render as results/updated-<i>.ppm, with its stages. The edits
// accumulate, so render cases 0..i merged fresh, with
// ?image=results/card.jpg&max=1200&params=<merged>&name=fresh-<i>, and
// compare with web/compare.mjs.
//   node web/cdp.mjs 'http://localhost:8137/?image=results/card.jpg&max=1200&runs=1&name=x' web/checks/updated.js 6000
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (n) => { for (let i = 0; i < 400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); return negs.renders.at(-1); };
await settle(0);
const cases = await (await fetch("/checks/enlarger-cases.json")).json();
const flat = (o, prefix = "") => Object.entries(o).flatMap(([k, v]) => v && typeof v === "object" && !Array.isArray(v) ? flat(v, `${prefix}${k}.`) : [[`${prefix}${k}`, v]]);
const out = [];
for (const [i, c] of cases.entries()) {
  for (const [path, v] of flat(c)) {
    const n = negs.renders.length;
    negs.edit(path, v);
    await settle(n);
  }
  const r = await negs.read(1200);
  await fetch(`/result?name=updated-${i}&w=${r.width}&h=${r.height}`, { method: "POST", body: r.rgba });
  out.push({ case: i, stages: r.stages, frames: negs.renders.slice(-flat(c).length).map((f) => f.stages.includes("pipeline")) });
}
return out;
