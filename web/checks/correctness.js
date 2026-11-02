// The browser's renders for web/checks/correctness.sh. For each case of
// $CASES (default checks/correctness-cases.json) it opens the image, starts
// a fresh edit (negs.start: film, paper, edits), waits for the presented
// frame, then reads the edit back as bytes at full size and saves
// results/<prefix><name>.ppm with the edit it rendered
// (<prefix><name>-edit.json), for the CLI to render the same params.
// Cases run in one page, so the pipeline cache carries across them as it
// would for a viewer.
//   node web/cdp.mjs 'http://localhost:8137/?cases=checks/correctness-cases.json' web/checks/correctness.js 3000
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (n) => { for (let i = 0; i < 2400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); return negs.renders.at(-1); };
const query = new URLSearchParams(location.search);
const cases = await (await fetch(query.get("cases") ?? "checks/correctness-cases.json")).json();
const prefix = query.get("prefix") ?? "c-";
const only = query.get("only")?.split(",");
const out = [];
let image = null;
for (const c of cases) {
  if (only && !only.includes(c.name)) continue;
  if (c.image !== image) {
    const n = negs.renders.length;
    await negs.open(c.image);
    await settle(n);
    image = c.image;
  }
  const n = negs.renders.length;
  negs.start(c.film, c.paper, c.edits);
  const shown = await settle(n);
  const r = await negs.read(1 << 20);
  if (!r.rgba?.length) { out.push({ name: c.name, error: "no bytes", stages: r.stages }); continue; }
  const s = negs.state();
  await fetch(`/result?name=${prefix}${c.name}&w=${r.width}&h=${r.height}`, { method: "POST", body: r.rgba });
  await fetch(`/result?name=${prefix}${c.name}-edit`, { method: "POST", body: JSON.stringify({ image: c.image, film: s.film, paper: s.paper, params: s.params }) });
  out.push({ name: c.name, size: `${r.width}x${r.height}`, presented: shown?.presented, shownStages: shown?.stages, backend: r.backend });
}
return out;
