// Balance Print: the wasm solver for a few films, papers and edits (print the
// same cases with the native helper's balance_print to compare), then the
// button: it sets both shifts and the frame after it doesn't rebuild.
//   node web/cdp.mjs 'http://localhost:8137/?image=results/card.jpg&max=1200&runs=1&name=x' web/checks/balance.js 6000
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (n) => { for (let i = 0; i < 400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); return negs.renders.at(-1); };
await settle(0);
const cases = await (await fetch("/checks/balance-cases.json")).json();
const catalog = await (await fetch("/catalog.json")).json();
const merge = (a, b) => { for (const [k, v] of Object.entries(b)) a[k] = v && typeof v === "object" && !Array.isArray(v) ? merge(a[k] ?? {}, v) : v; return a; };
const solved = [];
for (const c of cases) {
  const params = JSON.stringify(merge(structuredClone(catalog.defaults), c.params));
  const r = await negs.balance(c.film, c.paper, params);
  solved.push({ m: r.m, y: r.y, ms: Math.round(r.elapsedMs) });
}
const button = [...document.querySelectorAll(".ctl.action button")].find((b) => b.textContent === "Balance Print");
const n = negs.renders.length;
negs.edit("film_render.base.active", true);
await settle(n);
const before = negs.renders.length;
button.click();
const label = button.textContent, disabled = button.disabled;
for (let i = 0; i < 400 && !negs.state().params.enlarger.m_filter_shift; i++) await wait(25);
const frame = await settle(before);
return {
  solved,
  whileBalancing: { label, disabled },
  after: { label: button.textContent, disabled: button.disabled, message: document.getElementById("message").textContent,
    m: negs.state().params.enlarger.m_filter_shift, y: negs.state().params.enlarger.y_filter_shift, stages: frame.stages, ms: Math.round(frame.elapsedMs), presented: frame.presented },
};
