// Every Lab control (web/controls.js): set it off its default, render, and
// report the frame's stages (a "pipeline" stage is a rebuild), its time,
// whether it presented, and whether the canvas changed.
//   node web/cdp.mjs 'http://localhost:8137/?image=results/card.jpg&max=1200&runs=1&name=x' web/checks/controls.js 6000
const { SECTIONS } = await import("/controls.js");
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (n) => { for (let i = 0; i < 400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); return negs.renders.at(-1); };
async function hash() {
  const shown = await createImageBitmap(document.getElementById("gpu-canvas"));
  const ctx = new OffscreenCanvas(shown.width, shown.height).getContext("2d");
  ctx.drawImage(shown, 0, 0);
  const d = ctx.getImageData(0, 0, shown.width, shown.height).data;
  let h = 0; for (let i = 0; i < d.length; i += 4) h = (h * 31 + d[i] + d[i + 1] * 7 + d[i + 2] * 13) | 0;
  return h;
}
const g = (path) => path.split(".").reduce((o, k) => o?.[k], negs.state().params) ?? null;
const edit = async (path, v) => { const n = negs.renders.length; negs.edit(path, v); return settle(n); };
// Controls that only act with another on.
const needs = { "scanner.white_level": "scanner.white_correction", "scanner.black_level": "scanner.black_correction", "camera.auto_exposure_method": "camera.auto_exposure" };
const out = [];
await settle(0);
for (const section of SECTIONS) {
  const groups = [{ title: section.title, controls: section.controls ?? [] }, ...section.groups];
  for (const group of groups) {
    const was = group.enable && g(group.enable);
    if (group.enable) {
      const r = await edit(group.enable, !was);
      out.push({ group: group.title, control: "(switch)", to: !was, stages: r.stages, ms: Math.round(r.elapsedMs), presented: r.presented });
      if (was) await edit(group.enable, true);
    }
    for (const c of group.controls) {
      if (c.kind === "action") continue;
      if (needs[c.path] && !g(needs[c.path])) await edit(needs[c.path], true);
      const base = await hash();
      let path = c.path, to, from = g(c.path);
      if (c.kind === "toggle") to = !from;
      else if (c.kind === "choice") to = c.options.find((o) => o !== from);
      else if (c.kind === "stock") {
        const film = negs.state().film;
        const preset = (await (await fetch("/catalog.json")).json()).profiles.find((p) => p.id === film).grain[c.path.split(".").at(-1)];
        const lanes = [...(from ?? preset)];
        const span = c.max - c.min;
        lanes[c.index] = lanes[c.index] + 0.3 * span <= c.max ? lanes[c.index] + 0.3 * span : lanes[c.index] - 0.3 * span;
        to = lanes;
      } else {
        const span = c.max - c.min;
        to = from + 0.3 * span <= c.max ? from + 0.3 * span : from - 0.3 * span;
        to = Math.round(to / c.step) * c.step;
      }
      const r1 = await edit(path, to);
      const changed = (await hash()) !== base;
      // A second move: the warm cost of the same control.
      let r2 = r1;
      if (c.kind === "slider" || c.kind === "stock") {
        const nudge = (v) => (v + c.step <= c.max ? v + c.step : v - c.step);
        const again = Array.isArray(to) ? to.map((v, i) => (i === c.index ? nudge(v) : v)) : nudge(to);
        r2 = await edit(path, again);
      }
      out.push({ group: group.title, control: c.label, path, changed, first: Math.round(r1.elapsedMs), warm: Math.round(r2.elapsedMs), stages: r2.stages, firstStages: r1.stages, presented: r2.presented, backend: r2.backend });
      await edit(path, from);
      if (needs[c.path]) await edit(needs[c.path], false);
    }
    if (group.enable && !was) await edit(group.enable, false);
  }
}
return out;
