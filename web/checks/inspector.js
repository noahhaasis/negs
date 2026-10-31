// The inspector's wiring: dragging, the reset buttons, typing values, grain
// lanes and B&W films, group switches, scanning and Reset Adjustments.
//   node web/cdp.mjs 'http://localhost:8137/?image=results/card.jpg&max=1200&runs=1&name=x' web/checks/inspector.js 6000
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (n) => { for (let i = 0; i < 400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); };
const g = (path) => path.split(".").reduce((o, k) => o?.[k], negs.state().params) ?? null;
const out = {};
await settle(0);
const rowOf = (label, group) => {
  const scope = group ? [...document.querySelectorAll("details")].find((d) => d.querySelector("summary span").textContent === group) : document;
  return [...scope.querySelectorAll(".ctl")].find((r) => (r.querySelector(".label, label > span")?.textContent) === label);
};
// Drag Exposure: input events render interactive frames, change settles.
let row = rowOf("Exposure (EV)");
let range = row.querySelector("input[type=range]");
let n = negs.renders.length;
for (const v of [0.3, 0.6, 0.9, 1.2]) { range.value = v; range.dispatchEvent(new Event("input")); await wait(40); }
range.dispatchEvent(new Event("change"));
await settle(n);
out.drag = { value: g("camera.exposure_compensation_ev"), frames: negs.renders.slice(n).map((r) => `${r.interactive ? "i" : "s"}${r.width}`), resetShown: !row.querySelector(".reset").hidden, shown: row.querySelector(".value").textContent };
// The reset button.
n = negs.renders.length;
row.querySelector(".reset").click();
await settle(n);
out.reset = { value: g("camera.exposure_compensation_ev"), resetShown: !row.querySelector(".reset").hidden };
// Type a value: out of range clamps and snaps; Escape cancels.
row = rowOf("Print exposure", "Enlarger");
row.querySelector(".value").click();
let field = row.querySelector(".value-field");
out.fieldFocused = document.activeElement === field;
field.value = "1,234"; field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
await settle(negs.renders.length - 1);
out.typed = g("enlarger.print_exposure");
row.querySelector(".value").click();
field = row.querySelector(".value-field");
field.value = "99"; field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
await wait(200);
out.typedClamped = g("enlarger.print_exposure");
row.querySelector(".value").click();
field = row.querySelector(".value-field");
field.value = "2"; field.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
await wait(100);
out.escaped = g("enlarger.print_exposure");
// A grain lane: editing it writes the array; back at the preset it's null again.
row = rowOf("Green granularity", "Grain");
range = row.querySelector("input[type=range]");
const preset = Number(range.value);
range.value = 30; range.dispatchEvent(new Event("change"));
out.lane = g("film_render.grain.rms_granularity");
row.querySelector(".reset").click();
out.laneReset = g("film_render.grain.rms_granularity");
// B&W: one granularity lane, under its B&W name.
const pick = (id, v) => { const s = document.getElementById(id); s.value = v; s.dispatchEvent(new Event("change")); };
pick("film", "kodak_trix400_d76_8min");
const lanes = (field) => [...document.querySelectorAll(".ctl")].filter((r) => !r.hidden && /granularity|uniformity|grain size/i.test(r.querySelector(".label")?.textContent ?? "")).map((r) => r.querySelector(".label").textContent);
out.bwLanes = lanes();
pick("film", "kodak_portra_400");
out.colourLanes = lanes().length;
// Group switch disables its controls; scanning disables the print section.
const lens = [...document.querySelectorAll("details")].find((d) => d.querySelector("summary span").textContent === "Lens Diffusion");
out.lensDisabled = lens.querySelector("fieldset").disabled;
lens.querySelector("summary .switch").click();
out.lensEnabled = !lens.querySelector("fieldset").disabled && g("camera.diffusion_filter.active");
document.getElementById("scan-film").click();
const print = [...document.querySelectorAll("fieldset.section")].find((f) => f.querySelector("h2").textContent === "Print");
out.scanning = { printDisabled: print.disabled, footer: !print.querySelector(".footer").hidden, paperHidden: document.getElementById("paper-row").hidden };
// Reset Adjustments keeps the film and paper, restores scan for the film.
pick("film", "kodak_vision3_250d");
rowOf("Exposure (EV)").querySelector(".value").click();
field = document.querySelector(".value-field"); field.value = "2"; field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
document.getElementById("reset").click();
await wait(300);
const s = negs.state();
out.resetAll = { film: s.film, paper: s.paper, ev: g("camera.exposure_compensation_ev"), lens: g("camera.diffusion_filter.active"), scan: g("io.scan_film"), print: g("enlarger.print_exposure"), modifiedRows: document.querySelectorAll(".ctl.changed").length };
await settle(negs.renders.length - 1);
out.last = negs.renders.at(-1);
return out;
