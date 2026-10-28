// The Negs web app: opens a photograph in the engine worker (web/worker.js)
// once, then renders on every change. While a slider moves it renders a
// smaller interactive frame and drops frames a newer change superseded; when
// it stops, the settled frame at the canvas's device resolution.
const $ = (id) => document.getElementById(id);
const query = new URLSearchParams(location.search);

// The sliders: RuntimeParams paths, with the engine's defaults. Only values
// moved off their default are sent, so the engine keeps its per-film presets.
// `grain` scales the film's preset lanes from the catalog.
const SECTIONS = [
  { name: "Camera", open: true, knobs: [
    { key: "camera.exposure_compensation_ev", label: "Exposure", min: -5, max: 5, step: 0.1, value: 0, unit: " EV", signed: true },
    { key: "camera.film_format_mm", label: "Film format", min: 4, max: 120, step: 1, value: 35, unit: " mm" },
  ], toggles: [{ key: "camera.auto_exposure", label: "Auto exposure", value: true }] },
  { name: "Enlarger", open: true, print: true, knobs: [
    { key: "enlarger.print_exposure", label: "Print exposure", min: 0.1, max: 5, step: 0.01, value: 1 },
    { key: "enlarger.y_filter_shift", label: "Yellow filter", min: -50, max: 50, step: 0.5, value: 0, signed: true },
    { key: "enlarger.m_filter_shift", label: "Magenta filter", min: -50, max: 50, step: 0.5, value: 0, signed: true },
  ] },
  { name: "Grain", switch: "film_render.grain.active", knobs: [
    { key: "grain.amount", label: "Amount", min: 0, max: 3, step: 0.05, value: 1, unit: "×", grain: "rms_granularity" },
    { key: "grain.size", label: "Size", min: 0.25, max: 3, step: 0.05, value: 1, unit: "×", grain: "particle_scale_sublayers" },
  ] },
  { name: "Halation", switch: "film_render.halation.active", knobs: [
    { key: "film_render.halation.halation_amount", label: "Halation", min: 0, max: 3, step: 0.01, value: 1 },
    { key: "film_render.halation.halation_spatial_scale", label: "Halation size", min: 0.1, max: 5, step: 0.01, value: 1 },
    { key: "film_render.halation.scatter_amount", label: "Scatter", min: 0, max: 3, step: 0.01, value: 1 },
  ] },
  { name: "Glare", print: true, switch: "print_render.glare.active", knobs: [
    { key: "print_render.glare.percent", label: "Amount", min: 0, max: 0.2, step: 0.001, value: 0.02 },
  ] },
];
const SWITCH_DEFAULT = true;

const values = new Map();     // key → value, for knobs and switches moved off their default
let catalog = [];
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

const film = () => catalog.find((p) => p.id === $("film").value);

function params() {
  const out = {};
  const set = (path, value) => {
    const keys = path.split(".");
    let o = out;
    for (const k of keys.slice(0, -1)) o = o[k] ??= {};
    o[keys.at(-1)] = value;
  };
  for (const [key, value] of values) {
    if (!key.startsWith("grain.")) set(key, value);
  }
  const preset = film()?.grain;
  for (const knob of SECTIONS.flatMap((s) => s.knobs).filter((k) => k.grain)) {
    if (values.has(knob.key) && preset) {
      set(`film_render.grain.${knob.grain}`, preset[knob.grain].map((v) => v * values.get(knob.key)));
    }
  }
  if (film()?.positive) set("io.scan_film", true);
  return JSON.stringify(out);
}

function format(knob, v) {
  const digits = Math.max(0, -Math.floor(Math.log10(knob.step) + 1e-9));
  return `${knob.signed && v > 0 ? "+" : ""}${v.toFixed(digits)}${knob.unit ?? ""}`;
}

function buildControls() {
  const root = $("sections");
  for (const section of SECTIONS) {
    const details = document.createElement("details");
    details.open = section.open ?? false;
    if (section.print) details.dataset.print = "";
    const summary = document.createElement("summary");
    summary.append(section.name);
    if (section.switch) {
      const sw = Object.assign(document.createElement("input"), { type: "checkbox", className: "switch", checked: SWITCH_DEFAULT });
      sw.title = `${section.name} on or off`;
      sw.addEventListener("click", (ev) => ev.stopPropagation());
      sw.addEventListener("change", () => {
        sw.checked === SWITCH_DEFAULT ? values.delete(section.switch) : values.set(section.switch, sw.checked);
        sw.checked ? delete details.dataset.off : (details.dataset.off = "");
        schedule("settled");
      });
      summary.append(sw);
    }
    const knobs = document.createElement("div");
    knobs.className = "knobs";
    for (const t of section.toggles ?? []) {
      const row = document.createElement("label");
      row.className = "row";
      const sw = Object.assign(document.createElement("input"), { type: "checkbox", className: "switch", checked: t.value });
      sw.addEventListener("change", () => {
        sw.checked === t.value ? values.delete(t.key) : values.set(t.key, sw.checked);
        schedule("settled");
      });
      row.append(t.label, sw);
      knobs.append(row);
    }
    for (const knob of section.knobs) {
      const div = document.createElement("div");
      div.className = "knob";
      const id = `k-${knob.key}`;
      const label = Object.assign(document.createElement("label"), { htmlFor: id, textContent: knob.label });
      const out = Object.assign(document.createElement("output"), { textContent: format(knob, knob.value) });
      const range = Object.assign(document.createElement("input"), { type: "range", id, min: knob.min, max: knob.max, step: knob.step, value: knob.value });
      const update = () => {
        const v = Number(range.value);
        out.textContent = format(knob, v);
        Math.abs(v - knob.value) < knob.step / 2 ? values.delete(knob.key) : values.set(knob.key, v);
        div.classList.toggle("changed", values.has(knob.key));
      };
      range.addEventListener("input", () => { update(); schedule("interactive"); });
      range.addEventListener("change", () => { update(); schedule("settled"); });
      range.addEventListener("dblclick", () => { range.value = knob.value; update(); schedule("settled"); });
      div.append(label, out, range);
      knobs.append(div);
    }
    details.append(summary, knobs);
    root.append(details);
  }
}

function fillStocks() {
  const group = (select, profiles) => {
    let og;
    for (const p of profiles) {
      if (og?.label !== p.group) select.append(og = Object.assign(document.createElement("optgroup"), { label: p.group }));
      og.append(new Option(p.name, p.id));
    }
  };
  group($("film"), catalog.filter((p) => !p.paper));
  group($("paper"), catalog.filter((p) => p.paper));
  $("film").value = query.get("film") ?? "kodak_portra_400";
  $("paper").value = query.get("paper") ?? "kodak_portra_endura";
  filmChanged(!query.get("paper"));
}

function filmChanged(pickPaper = true) {
  const f = film();
  if (pickPaper && f.target) $("paper").value = f.target;
  else if (pickPaper && f.bw && !f.positive) $("paper").value = "kodak_2302";
  // A slide is scanned directly: no paper.
  $("paper").disabled = f.positive;
  $("paper").querySelector("option[value=none]")?.remove();
  if (f.positive) {
    $("paper").prepend(new Option("None: slide film is scanned", "none", true, true));
  } else if ($("paper").value === "none" || !$("paper").value) {
    $("paper").value = f.target || "kodak_portra_endura";
  }
  // A slide is scanned, not printed: no enlarger or print glare.
  for (const d of document.querySelectorAll("details[data-print]")) d.hidden = f.positive;
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
      const r = await host.call("render", $("film").value, $("paper").value, params(), maxDim, interactive, presenting);
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
    const r = await host.call("render", $("film").value, $("paper").value, params(), Math.max(photo.width, photo.height), false);
    const canvas = new OffscreenCanvas(r.width, r.height);
    canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height), 0, 0);
    const blob = await canvas.convertToBlob({ type: "image/jpeg", quality: 0.92 });
    const a = Object.assign(document.createElement("a"), {
      href: URL.createObjectURL(blob),
      download: `${photo.name.replace(/\.[^.]+$/, "")}-${$("film").value}.jpg`,
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
catalog = (await (await fetch("catalog.json")).json()).profiles;
fillStocks();

$("film").addEventListener("change", () => { filmChanged(); schedule("settled"); });
$("paper").addEventListener("change", () => schedule("settled"));
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
      r = await host.call("render", $("film").value, $("paper").value, query.get("params") ?? params(), max, interactive, presenting);
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
      r = await host.call("render", $("film").value, $("paper").value, query.get("params") ?? params(), max, interactive, false);
    }
    await fetch(`result?name=${encodeURIComponent(name)}&w=${r.width}&h=${r.height}`, { method: "POST", body: r.rgba });
    await post(name, JSON.stringify({ threads, presenting, backend: r.backend, width: r.width, height: r.height, loadMs, times, frames, stages, memoryMiB: r.memoryMiB }));
  } catch (e) {
    await post(name, JSON.stringify({ error: String(e), stack: e?.stack }));
  }
}
