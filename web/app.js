// The Negs web app: opens a photograph in the engine worker (web/worker.js)
// once, then renders on every change. While a slider moves it renders a
// smaller interactive frame and drops frames a newer change superseded; when
// it stops, the settled frame at the canvas's device resolution.
const $ = (id) => document.getElementById(id);
const query = new URLSearchParams(location.search);

// The sliders: RuntimeParams paths. Their defaults come from the engine's
// catalog. `grain` scales the film's preset lanes from the catalog.
const SECTIONS = [
  { name: "Camera", open: true, knobs: [
    { key: "camera.exposure_compensation_ev", label: "Exposure", min: -5, max: 5, step: 0.1, unit: " EV", signed: true },
    { key: "camera.film_format_mm", label: "Film format", min: 4, max: 120, step: 1, unit: " mm" },
  ], toggles: [{ key: "camera.auto_exposure", label: "Auto exposure" }] },
  { name: "Enlarger", open: true, print: true, knobs: [
    { key: "enlarger.print_exposure", label: "Print exposure", min: 0.1, max: 5, step: 0.01 },
    { key: "enlarger.y_filter_shift", label: "Yellow filter", min: -50, max: 50, step: 0.5, signed: true },
    { key: "enlarger.m_filter_shift", label: "Magenta filter", min: -50, max: 50, step: 0.5, signed: true },
  ] },
  { name: "Grain", switch: "film_render.grain.active", knobs: [
    { key: "grain.amount", label: "Amount", min: 0, max: 3, step: 0.05, unit: "×", grain: "rms_granularity" },
    { key: "grain.size", label: "Size", min: 0.25, max: 3, step: 0.05, unit: "×", grain: "particle_scale_sublayers" },
  ] },
  { name: "Halation", switch: "film_render.halation.active", knobs: [
    { key: "film_render.halation.halation_amount", label: "Halation", min: 0, max: 3, step: 0.01 },
    { key: "film_render.halation.halation_spatial_scale", label: "Halation size", min: 0.1, max: 5, step: 0.01 },
    { key: "film_render.halation.scatter_amount", label: "Scatter", min: 0, max: 3, step: 0.01 },
  ] },
  { name: "Glare", print: true, switch: "print_render.glare.active", knobs: [
    { key: "print_render.glare.percent", label: "Amount", min: 0, max: 0.2, step: 0.001 },
  ] },
];

// The edit, as desktop keeps it (macOS EditState): the film, the paper and a
// complete RuntimeParams, starting from the engine's defaults. Values a
// film's presets fill stay null until edited. The grain multipliers are the
// page's own until the grain lanes get their sliders.
let catalog = { profiles: [], defaults: {}, nullable_paths: [] };
const state = { film: "", paper: "", params: {} };
const grainScale = new Map();
// Every value a film's presets fill (macOS Stock.presetPaths): a film change resets them.
const GRAIN_FIELDS = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"];
const PRESET_PATHS = [
  ...GRAIN_FIELDS.map((f) => `film_render.grain.${f}`),
  "film_render.dir_couplers.gamma_samelayer_rgb", "film_render.dir_couplers.gamma_interlayer_r_to_gb",
  "film_render.dir_couplers.gamma_interlayer_g_to_rb", "film_render.dir_couplers.gamma_interlayer_b_to_rg",
  "film_render.halation.halation_strength", "film_render.halation.halation_first_sigma_um",
];
let photo = null;             // { name, bytes, width, height }
let host = null;              // the engine worker
let kind = "gpu";

// ---- Engine worker ---------------------------------------------------------

const threads = self.crossOriginIsolated && query.get("threads") !== "0";

let instances = 0;
function startHost(engineKind) {
  const instance = ++instances;
  const worker = new Worker("worker.js", { type: "module" });
  const pending = new Map();
  let next = 0, dead = null;
  const fail = (error) => {
    if (dead) return;
    dead = error;
    for (const [, [, reject]] of pending) reject(error);
    pending.clear();
  };
  worker.onmessage = ({ data: { id, result, error, crashed } }) => {
    if (!pending.has(id)) return;
    if (crashed) {
      // The panic's message follows on the channel; wait for it briefly.
      return setTimeout(() => fail(Object.assign(new Error(`the engine crashed (${error})`), { crashed: true })), 200);
    }
    const [resolve, reject] = pending.get(id);
    pending.delete(id);
    error ? reject(new Error(error)) : resolve(result);
  };
  worker.onerror = (ev) => fail(Object.assign(new Error(ev.message || "the engine stopped"), { crashed: true }));
  // A panic on one of rayon's threads leaves the engine waiting forever;
  // the engine reports every panic here.
  const panics = new BroadcastChannel("negs-engine-panic");
  panics.onmessage = ({ data }) => {
    if (data.instance !== instance) return;
    worker.terminate();
    panics.close();
    fail(Object.assign(new Error(`the engine crashed: ${data.message}`), { crashed: true }));
  };
  const call = (method, ...args) => dead ? Promise.reject(dead) : new Promise((resolve, reject) => {
    pending.set(++next, [resolve, reject]);
    worker.postMessage({ id: next, method, args }, args.filter((a) => a instanceof OffscreenCanvas));
  });
  const ready = call("init", engineKind, threads, instance);
  const kill = () => {
    worker.terminate();
    panics.close();
    fail(Object.assign(new Error("engine restarted"), { restarted: true }));
  };
  return { call, ready, kill };
}

async function startEngine(engineKind) {
  host?.kill();
  kind = engineKind;
  $("engine").value = kind;
  $("engine-info").textContent = "Starting…";
  host = startHost(kind);
  try {
    const info = await host.ready;
    minScale = kind === "gpu" ? 0.25 : 0.15;
    interactiveScale = Math.max(interactiveScale, minScale);
    $("engine-info").textContent = `${info.backend}${info.threads > 1 ? ` · ${info.threads} threads` : " · 1 thread"}`;
    await attachCanvas();
  } catch (e) {
    if (kind === "gpu") {
      showMessage(`WebGPU isn't available here (${e.message}); using the CPU engine.`);
      return startEngine("cpu");
    }
    throw e;
  }
  if (photo) {
    $("busy").classList.add("on");
    await host.call("load", photo.bytes, 0).finally(() => running || $("busy").classList.remove("on"));
  }
}

// The WebGPU engine presents on a canvas it owns, with nothing read back. A
// canvas transfers to one worker only, so each engine gets a new one.
let presenting = false;
async function attachCanvas() {
  presenting = false;
  $("gpu-canvas")?.remove();
  $("canvas").hidden = !photo;
  if (kind !== "gpu" || !self.OffscreenCanvas || !HTMLCanvasElement.prototype.transferControlToOffscreen || query.get("present") === "0") return;
  const canvas = Object.assign(document.createElement("canvas"), { id: "gpu-canvas", className: "photo", hidden: true });
  $("canvas").after(canvas);
  try {
    await host.call("setCanvas", canvas.transferControlToOffscreen());
    presenting = true;
    layout();
  } catch (e) {
    console.warn(`Presenting from the engine failed (${e.message}); drawing its bytes instead.`);
    canvas.remove();
  }
}

// ---- Parameters ------------------------------------------------------------

const stock = (id) => catalog.profiles.find((p) => p.id === id);
const films = () => catalog.profiles.filter((p) => !p.paper);
const papers = () => catalog.profiles.filter((p) => p.paper);

function get(path, from = state.params) {
  return path.split(".").reduce((o, k) => o?.[k], from) ?? null;
}
function set(path, value, into = state.params) {
  const keys = path.split(".");
  let o = into;
  for (const k of keys.slice(0, -1)) o = o[k] ??= {};
  o[keys.at(-1)] = value;
}
const defaultValue = (path) => get(path, catalog.defaults);
// Rust defaults arrive as widened f32 values; slider edits are decimal.
function isModified(path) {
  const [a, b] = [get(path), defaultValue(path)];
  if (typeof a === "number" && typeof b === "number") return Math.abs(a - b) > 1e-6 * Math.max(1, Math.abs(b));
  return JSON.stringify(a) !== JSON.stringify(b);
}

const params = () => JSON.stringify(state.params);

// A new film brings its own grain, DIR and halation presets, as upstream
// applies them (macOS EditorModel.chooseFilm).
function chooseFilm(id) {
  state.film = id;
  set("film_render.development_time", null);
  for (const path of PRESET_PATHS) set(path, null);
  grainScale.clear();
  pairFilm();
}
function choosePaper(id) {
  state.paper = id;
  set("print_render.development_time", null);
}
// A slide scans directly; a film with a target paper prints on it. Others
// keep the paper, as on desktop.
function pairFilm() {
  const f = stock(state.film);
  if (!f) return;
  set("io.scan_film", f.positive);
  if (papers().some((p) => p.id === f.target_print) && state.paper !== f.target_print) {
    state.paper = f.target_print;
    set("print_render.development_time", null);
  }
}

// The grain multipliers scale the film's preset lanes; at 1 the lane is
// unset again, so the engine uses its preset.
function setGrainScale(knob, v) {
  v === 1 ? grainScale.delete(knob.key) : grainScale.set(knob.key, v);
  const preset = stock(state.film)?.grain?.[knob.grain];
  set(`film_render.grain.${knob.grain}`, grainScale.has(knob.key) && preset ? preset.map((x) => x * v) : null);
}

function format(knob, v) {
  const digits = Math.max(0, -Math.floor(Math.log10(knob.step) + 1e-9));
  return `${knob.signed && v > 0 ? "+" : ""}${v.toFixed(digits)}${knob.unit ?? ""}`;
}

// Each control's view of the state, run after anything changes it.
const syncs = [];
const sync = () => syncs.forEach((f) => f());

function buildControls() {
  const root = $("sections");
  for (const section of SECTIONS) {
    const details = document.createElement("details");
    details.open = section.open ?? false;
    if (section.print) details.dataset.print = "";
    const summary = document.createElement("summary");
    summary.append(section.name);
    if (section.switch) {
      const sw = Object.assign(document.createElement("input"), { type: "checkbox", className: "switch" });
      sw.title = `${section.name} on or off`;
      sw.addEventListener("click", (ev) => ev.stopPropagation());
      sw.addEventListener("change", () => {
        set(section.switch, sw.checked);
        sync();
        schedule("settled");
      });
      syncs.push(() => {
        sw.checked = get(section.switch);
        sw.checked ? delete details.dataset.off : (details.dataset.off = "");
      });
      summary.append(sw);
    }
    const knobs = document.createElement("div");
    knobs.className = "knobs";
    for (const t of section.toggles ?? []) {
      const row = document.createElement("label");
      row.className = "row";
      const sw = Object.assign(document.createElement("input"), { type: "checkbox", className: "switch" });
      sw.addEventListener("change", () => {
        set(t.key, sw.checked);
        schedule("settled");
      });
      syncs.push(() => (sw.checked = get(t.key)));
      row.append(t.label, sw);
      knobs.append(row);
    }
    for (const knob of section.knobs) {
      const div = document.createElement("div");
      div.className = "knob";
      const id = `k-${knob.key}`;
      const label = Object.assign(document.createElement("label"), { htmlFor: id, textContent: knob.label });
      const out = document.createElement("output");
      const range = Object.assign(document.createElement("input"), { type: "range", id, min: knob.min, max: knob.max, step: knob.step });
      const value = () => knob.grain ? grainScale.get(knob.key) ?? 1 : get(knob.key);
      const show = () => {
        range.value = value();
        out.textContent = format(knob, value());
        div.classList.toggle("changed", knob.grain ? grainScale.has(knob.key) : isModified(knob.key));
      };
      const update = (v) => {
        knob.grain ? setGrainScale(knob, v) : set(knob.key, v);
        show();
      };
      syncs.push(show);
      range.addEventListener("input", () => { update(Number(range.value)); schedule("interactive"); });
      range.addEventListener("change", () => { update(Number(range.value)); schedule("settled"); });
      range.addEventListener("dblclick", () => { update(knob.grain ? 1 : defaultValue(knob.key)); schedule("settled"); });
      div.append(label, out, range);
      knobs.append(div);
    }
    details.append(summary, knobs);
    root.append(details);
  }
}

// The pickers' sections, in desktop's order (macOS StockGroup).
const isMotionPicturePrint = (p) => p.paper && (p.name.includes("Vision") || p.name.includes("Print Film"));
function stockGroups(stocks, order, kind) {
  return order.map((title) => ({ title, stocks: stocks.filter((s) => kind(s) === title) })).filter((g) => g.stocks.length);
}
function filmGroups() {
  const cinemaPrints = new Set(papers().filter(isMotionPicturePrint).map((p) => p.id));
  return stockGroups(films(), ["Color Negative", "Color Slide", "Motion Picture", "Black & White"],
    (s) => s.bw ? "Black & White" : s.positive ? "Color Slide" : cinemaPrints.has(s.target_print) ? "Motion Picture" : "Color Negative");
}
function paperGroups() {
  return stockGroups(papers(), ["Color Paper", "Motion Picture Print", "Black & White"],
    (s) => s.bw ? "Black & White" : isMotionPicturePrint(s) ? "Motion Picture Print" : "Color Paper");
}

function fillStocks() {
  const fill = (select, groups) => {
    for (const g of groups) {
      const og = Object.assign(document.createElement("optgroup"), { label: g.title });
      for (const s of g.stocks) og.append(new Option(s.name, s.id));
      select.append(og);
    }
  };
  fill($("film"), filmGroups());
  fill($("paper"), paperGroups());
  state.params = structuredClone(catalog.defaults);
  state.paper = "kodak_portra_endura";
  chooseFilm(stock(query.get("film"))?.paper === false ? query.get("film") : "kodak_portra_400");
  if (stock(query.get("paper"))?.paper) choosePaper(query.get("paper"));
  syncs.push(() => {
    $("film").value = state.film;
    $("paper").value = state.paper;
    const scanning = get("io.scan_film");
    $("scan-film").checked = scanning;
    // Scanned film isn't printed: no paper, enlarger or print glare.
    $("paper-row").hidden = scanning;
    for (const d of document.querySelectorAll("details[data-print]")) d.hidden = scanning;
  });
  sync();
}

// ---- Rendering -------------------------------------------------------------

// Interactive frames start at half the settled size and adapt to stay near
// the frame budget: on a fast GPU up to the settled size, on a slow one or
// the CPU engine down to a few hundred pixels. A WebGPU frame costs about
// 8 ms whatever its size, so it goes no smaller than a quarter.
const BUDGET_MS = 90;
let interactiveScale = 0.5, minScale = 0.15;
let wanted = null, running = false, settleTimer = 0, exporting = false;

function schedule(mode) {
  if (!photo) return;
  clearTimeout(settleTimer);
  // A slider that stops without a change event (keyboard, wheel) settles anyway.
  if (mode === "interactive") settleTimer = setTimeout(() => schedule("settled"), 250);
  wanted = mode;
  pump();
}

async function pump() {
  if (running || exporting) return;
  running = true;
  $("busy").classList.add("on");
  while (wanted && photo) {
    const interactive = wanted === "interactive";
    wanted = null;
    const settled = settledSize();
    const maxDim = interactive ? Math.max(256, Math.round(settled * interactiveScale)) : settled;
    try {
      const r = await host.call("render", state.film, state.paper, params(), maxDim, interactive, presenting);
      draw(r);
      if (interactive) {
        interactiveScale = Math.min(1, Math.max(minScale, interactiveScale * Math.sqrt(BUDGET_MS / Math.max(r.elapsedMs, 1))));
      }
      hud(r, interactive);
      hideMessage();
    } catch (e) {
      await failed(e, "Render failed");
    }
  }
  running = false;
  $("busy").classList.remove("on");
}

// The longest side the viewport shows the photograph at, in device pixels.
function settledSize() {
  const { width, height } = fit();
  return Math.min(Math.max(photo.width, photo.height), Math.ceil(Math.max(width, height) * devicePixelRatio));
}

// The photograph's box in the viewport, in CSS pixels.
function fit() {
  const view = $("view");
  const pad = view.clientWidth < 500 ? 12 : 32;
  const scale = Math.min((view.clientWidth - 2 * pad) / photo.width, (view.clientHeight - 2 * pad) / photo.height, 1);
  return { width: Math.max(1, Math.round(photo.width * scale)), height: Math.max(1, Math.round(photo.height * scale)) };
}

function layout() {
  if (!photo) return;
  const { width, height } = fit();
  for (const canvas of document.querySelectorAll(".photo")) Object.assign(canvas.style, { width: `${width}px`, height: `${height}px` });
}

// A presented frame is already on the engine's canvas; otherwise draw the bytes.
function draw(r) {
  const canvas = $("canvas"), gpu = $("gpu-canvas");
  if (!r.presented) {
    if (canvas.width !== r.width || canvas.height !== r.height) Object.assign(canvas, { width: r.width, height: r.height });
    canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height), 0, 0);
  }
  canvas.hidden = r.presented;
  if (gpu) gpu.hidden = !r.presented;
  $("empty").hidden = true;
}

function hud(r, interactive) {
  const el = $("hud");
  el.textContent = `${photo.width}×${photo.height} · ${interactive ? "preview " : ""}${r.width}×${r.height} in ${r.elapsedMs.toFixed(0)} ms · ${r.memoryMiB} MiB`;
  el.title = r.stages;
}

// ---- Opening and exporting -------------------------------------------------

async function open(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const previous = photo;
  showMessage(`Opening ${file.name}…`);
  $("busy").classList.add("on");
  try {
    const t = performance.now();
    let size;
    // Switching engines restarts the worker under a load: load again.
    for (;;) {
      try {
        await host.ready;
        size = await host.call("load", bytes, 0);
        break;
      } catch (e) {
        if (!e.restarted) throw e;
      }
    }
    photo = { name: file.name, bytes, width: size.width, height: size.height };
    console.log(`decoded ${file.name} ${size.width}×${size.height} in ${(performance.now() - t).toFixed(0)} ms`);
    $("export").disabled = false;
    hideMessage();
    layout();
    schedule("settled");
  } catch (e) {
    photo = previous;
    await failed(e, `Couldn't open ${file.name}`);
  } finally {
    if (!running) $("busy").classList.remove("on");
  }
}

// A crash kills the engine's module: start a new worker, with the last
// photograph that opened.
async function failed(e, what) {
  if (e.restarted) return;
  console.error(e);
  showMessage(`${what}: ${e.message}`, true);
  if (e.crashed) {
    const message = $("message").textContent;
    await startEngine(kind).catch(() => {});
    showMessage(message, true);
  }
}

async function exportJpeg() {
  if (!photo || exporting) return;
  exporting = true;
  $("export").disabled = true;
  showMessage(`Rendering ${photo.width}×${photo.height} for export…`);
  $("busy").classList.add("on");
  try {
    while (running) await new Promise((r) => setTimeout(r, 20));
    const r = await host.call("render", state.film, state.paper, params(), Math.max(photo.width, photo.height), false);
    const canvas = new OffscreenCanvas(r.width, r.height);
    canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height), 0, 0);
    const blob = await canvas.convertToBlob({ type: "image/jpeg", quality: 0.92 });
    const a = Object.assign(document.createElement("a"), {
      href: URL.createObjectURL(blob),
      download: `${photo.name.replace(/\.[^.]+$/, "")}-${state.film}.jpg`,
    });
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
    hideMessage();
  } catch (e) {
    await failed(e, "Export failed");
  } finally {
    exporting = false;
    $("export").disabled = !photo;
    $("busy").classList.remove("on");
    if (wanted) pump();
  }
}

function showMessage(text, error = false) {
  const m = $("message");
  m.textContent = text;
  m.classList.toggle("error", error);
  m.hidden = false;
}
function hideMessage() {
  $("message").hidden = true;
}

// ---- Wiring ----------------------------------------------------------------

buildControls();
catalog = await (await fetch("catalog.json")).json();
fillStocks();
// For automation over the DevTools protocol: a copy of the edit.
globalThis.negsState = () => structuredClone(state);

$("film").addEventListener("change", () => { chooseFilm($("film").value); sync(); schedule("settled"); });
$("paper").addEventListener("change", () => { choosePaper($("paper").value); sync(); schedule("settled"); });
$("scan-film").addEventListener("change", () => { set("io.scan_film", $("scan-film").checked); sync(); schedule("settled"); });
$("engine").addEventListener("change", async () => {
  await startEngine($("engine").value).catch((e) => showMessage(e.message, true));
  schedule("settled");
});
for (const b of [$("open"), $("open-empty")]) b.addEventListener("click", () => $("file").click());
$("file").addEventListener("change", () => $("file").files[0] && open($("file").files[0]));
$("export").addEventListener("click", exportJpeg);

const view = $("view");
view.addEventListener("dragover", (ev) => { ev.preventDefault(); view.classList.add("dragging"); });
view.addEventListener("dragleave", () => view.classList.remove("dragging"));
view.addEventListener("drop", (ev) => {
  ev.preventDefault();
  view.classList.remove("dragging");
  const file = ev.dataTransfer.files[0];
  if (file) open(file);
});

let resizeTimer = 0;
new ResizeObserver(() => {
  layout();
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => schedule("settled"), 150);
}).observe(view);

const gpu = query.get("backend") ?? (navigator.gpu && (await navigator.gpu.requestAdapter().catch(() => null)) ? "gpu" : "cpu");
await startEngine(gpu).catch((e) => showMessage(`The engine didn't start: ${e.message}`, true));

// Automation: ?image=<url>&name=<result>&runs=<n>&max=<px>[&params=<json>]
// opens the image, renders it n times at max pixels, then posts the last image and the timings
// to the dev server (web/serve.mjs).
if (query.get("image")) {
  const name = query.get("name") ?? "result";
  // ?params= merges into the edit, as a preset over the defaults would.
  const merge = (into, from) => {
    for (const [k, v] of Object.entries(from)) {
      v && typeof v === "object" && !Array.isArray(v) && into[k] && typeof into[k] === "object" ? merge(into[k], v) : (into[k] = v);
    }
  };
  if (query.get("params")) {
    merge(state.params, JSON.parse(query.get("params")));
    sync();
  }
  const post = (q, body) => fetch(`result?name=${encodeURIComponent(q)}`, { method: "POST", body });
  try {
    const bytes = new Uint8Array(await (await fetch(query.get("image"))).arrayBuffer());
    const t = performance.now();
    const size = await host.call("load", bytes, 0);
    const loadMs = performance.now() - t;
    photo = { name: "automation", bytes, ...size };
    layout();
    const max = Number(query.get("max") ?? 2048);
    const interactive = query.get("interactive") === "1";
    const times = [], stages = [], frames = [];
    let r;
    for (let i = 0; i < Number(query.get("runs") ?? 1); i++) {
      // The whole frame, as the page sees it: the call, and drawing it.
      const t = performance.now();
      r = await host.call("render", state.film, state.paper, params(), max, interactive, presenting);
      draw(r);
      frames.push(performance.now() - t);
      times.push(r.elapsedMs);
      stages.push(r.stages);
    }
    // The image itself, read back, and what the engine's canvas shows.
    if (r.presented) {
      const shown = await createImageBitmap($("gpu-canvas"));
      const copy = new OffscreenCanvas(shown.width, shown.height).getContext("2d");
      copy.drawImage(shown, 0, 0);
      const body = copy.getImageData(0, 0, shown.width, shown.height).data;
      await fetch(`result?name=${encodeURIComponent(`${name}-canvas`)}&w=${shown.width}&h=${shown.height}`, { method: "POST", body });
      r = await host.call("render", state.film, state.paper, params(), max, interactive, false);
    }
    await fetch(`result?name=${encodeURIComponent(name)}&w=${r.width}&h=${r.height}`, { method: "POST", body: r.rgba });
    await post(name, JSON.stringify({ threads, presenting, backend: r.backend, width: r.width, height: r.height, loadMs, times, frames, stages, memoryMiB: r.memoryMiB }));
  } catch (e) {
    await post(name, JSON.stringify({ error: String(e), stack: e?.stack }));
  }
}
