// The Enlarger's controls, moved a few steps each as a slider drag would
// settle: every frame's time and stages (a "pipeline" stage is a rebuild of
// the spectral pipeline), and whether it presented.
//   node web/cdp.mjs 'http://localhost:8137/?image=results/card.jpg&max=1200&runs=1&name=x' web/checks/enlarger.js 6000
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (n) => { for (let i = 0; i < 400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); return negs.renders.at(-1); };
const edit = async (path, v) => { const n = negs.renders.length; negs.edit(path, v); return settle(n); };
const row = (label, r) => ({ control: label, size: `${r.width}×${r.height}`, ms: Math.round(r.elapsedMs), stages: r.stages, presented: r.presented });
const out = [];
await settle(0);
const moves = [
  ["Magenta shift", "enlarger.m_filter_shift", [5, 10, 15, -10, 0]],
  ["Yellow shift", "enlarger.y_filter_shift", [5, 10, 15, -10, 0]],
  ["Print exposure", "enlarger.print_exposure", [1.2, 1.4, 1]],
  ["Preflash exposure", "enlarger.preflash_exposure", [0.05, 0.1, 0.2]],
  ["Preflash magenta", "enlarger.preflash_m_filter_shift", [5, 10, -10]],
  ["Preflash yellow", "enlarger.preflash_y_filter_shift", [5, 10, -10]],
];
for (const [label, path, values] of moves) for (const v of values) out.push(row(`${label} ${v}`, await edit(path, v)));
out.push(row("Preflash exposure 0", await edit("enlarger.preflash_exposure", 0)));
out.push(row("Exposure +1 EV", await edit("camera.exposure_compensation_ev", 1)));
const comp = negs.state().params.enlarger.print_exposure_compensation;
out.push(row(`Exposure compensation ${!comp}`, await edit("enlarger.print_exposure_compensation", !comp)));
out.push(row(`Exposure compensation ${comp}`, await edit("enlarger.print_exposure_compensation", comp)));
return out;
