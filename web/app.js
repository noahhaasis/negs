// The Negs web app: opens a photograph in the engine worker (web/worker.js)
// once, then renders on every change. While a slider moves it renders a
// smaller interactive frame and drops frames a newer change superseded; when
// it stops, the settled frame at the canvas's device resolution.
const $ = (id) => document.getElementById(id);
const query = new URLSearchParams(location.search);

import { PREP_SECTIONS, SECTIONS, OUTPUT_SECTIONS, optionTitle, snap, formatValue } from "./controls.js";
import { createView } from "./view.js";
import { placeholder } from "./placeholder.js";
import * as Border from "./border.js";
import * as Crop from "./crop.js";
import * as Stocks from "./stocks.js";
import * as Help from "./help.js";
import { createCropEditor } from "./crop-editor.js";
import { fetchFailed } from "./engine.js";
import { DEFAULT_SAMPLE, createSamplePicker } from "./samples.js";
import { createPresets } from "./preset-ui.js";
import { RAW_WB_PATH, createSourceQueue, isRawColour, sameWb } from "./raw-wb.js";
import { createTutorial } from "./tutorial.js";

let tutorial = null, tutorialPresentation = false, tutorialRestoring = false;

// The edit, as desktop keeps it (macOS EditState): the film, the paper and a
// complete RuntimeParams, starting from the engine's defaults. Values a
// film's presets fill stay null until edited.
let catalog = { profiles: [], defaults: {}, nullable_paths: [] };
// `match`: Prep › Match camera brightness while it's on (see `matchInputs`):
// { ev, inputs, limited }, ev null until solved.
const state = { film: "", paper: "", params: {}, match: null };
// Every value a film's presets fill (macOS Stock.presetPaths): a film change resets them.
const GRAIN_FIELDS = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"];
const PRESET_PATHS = [
  ...GRAIN_FIELDS.map((f) => `film_render.grain.${f}`),
  "film_render.dir_couplers.gamma_samelayer_rgb", "film_render.dir_couplers.gamma_interlayer_r_to_gb",
  "film_render.dir_couplers.gamma_interlayer_g_to_rb", "film_render.dir_couplers.gamma_interlayer_b_to_rg",
  "film_render.halation.halation_strength", "film_render.halation.halation_first_sigma_um",
];
// { name, bytes, width, height, exportWidth, exportHeight, sampleId,
//   colour, raw, wb }: `colour` is what the loader read it as (InputColour),
// `raw` whether that is a camera RAW, `wb` the RAW white balance the
// engine's pixels of it were developed at (see "RAW white balance" below).
let photo = null;
// The presentation border (web/border.js), beside the edit as desktop keeps
// it: not in its params, so it never renders the film.
const border = { ...Border.DEFAULT_BORDER };
let crop = Crop.defaults(), cropSession = null, presentationOnly = false;
const rotation = () => Crop.turns(get("io.rotate_cw") ?? 0);
function photoGeometry() {
  if (early) return Crop.presentation(Crop.defaults(), 0, early.width, early.height);
  if (!photo) return null;
  return Crop.presentation(cropSession?.settings ?? crop, cropSession?.rotation ?? rotation(), photo.width, photo.height, !!cropSession);
}
const photoSize = () => photoGeometry() ?? photo;
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
  worker.onmessage = ({ data: { id, result, error, code, crashed } }) => {
    if (!pending.has(id)) return;
    if (crashed) {
      // The panic's message follows on the channel; wait for it briefly.
      return setTimeout(() => fail(Object.assign(new Error(`the engine crashed (${error})`), { crashed: true })), 200);
    }
    const [resolve, reject] = pending.get(id);
    pending.delete(id);
    error ? reject(Object.assign(new Error(error), { code })) : resolve(result);
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
    worker.postMessage({ id: next, method, args }, args.flatMap((a, i) => a instanceof OffscreenCanvas ? [a]
      : a?.pixels instanceof Float32Array ? [a.pixels.buffer] : method === "finishPhoto" && i === 0 ? [a.buffer] : []));
  });
  const ready = call("init", engineKind, threads, instance);
  const kill = () => {
    // The worker ends its thread pool and itself (see worker.js), once a
    // call it's running returns; terminating it alone leaves the pool's
    // memory taken.
    worker.postMessage({ method: "dispose" });
    setTimeout(() => worker.terminate(), 10000);
    panics.close();
    fail(Object.assign(new Error("engine restarted"), { restarted: true }));
  };
  return { call, ready, kill };
}

async function startEngine(engineKind) {
  // Safari versions without WebGPU cannot start this backend. Keep the
  // working CPU engine and its decoded RAW instead of allocating another
  // WASM pool just to discover that the API is absent.
  if (engineKind === "gpu" && !navigator.gpu) {
    engineKind = "cpu";
    $("engine").value = engineKind;
    if (host && kind === "cpu") return;
  }
  host?.kill();
  forgetOriginal();
  kind = engineKind;
  $("engine").value = kind;
  $("engine-info").textContent = "Starting…";
  host = startHost(kind);
  try {
    const info = await host.ready;
    minScale = kind === "gpu" ? 0.25 : 0.15;
    interactiveScale = Math.max(interactiveScale, minScale);
    $("engine-info").textContent = `${info.backend}${info.threads > 1 ? ` · ${info.threads} threads` : " · 1 thread"}`;
    const [side, pixels] = await host.call("limits");
    limits = side ? { side, pixels } : null;
    if (side && DIFFUSION_GRID_BUDGET) await host.call("setDiffusionGridBudget", DIFFUSION_GRID_BUDGET);
    if (keptPixels()) decoder ??= startDecoder();
    sync(); // Commands that need the engine.
    await attachCanvas();
  } catch (e) {
    // A newer selection replaced this engine; that start owns the page.
    if (e.restarted) return false;
    // rayon's threads not starting (web/pool.js) isn't WebGPU's doing, and
    // the CPU engine would start them the same way.
    if (kind === "gpu" && e.code !== "pool") {
      showMessage(`WebGPU isn't available here (${e.message}); using the CPU engine.`);
      return startEngine("cpu");
    }
    throw e;
  }
  if (photo) {
    $("busy").classList.add("on");
    // At the WB its last development used; one the edit wants since follows.
    const loaded = await reloadSource({ unlessOpening: true })
      .then(() => true, (e) => { if (e.restarted) return false; throw e; })
      .finally(() => running || developing || $("busy").classList.remove("on"));
    if (!loaded) return false;
    redevelop();
  }
  return true;
}

// The WebGPU engine presents on a canvas it owns, with nothing read back. A
// canvas transfers to one worker only, so each engine gets a new one.
let presenting = false;
// Where presenting crashed the engine, it draws the read-back frames
// instead, remembered per adapter. Seen with two GPUs: WebGPU rendered on the
// idle one while the other drove the display, and presenting lost the device
// (VK_ERROR_OUT_OF_DEVICE_MEMORY); reading back worked. ?present=0 and
// ?present=1 force either way.
const adapterKey = () => { const i = adapter?.info ?? {}; return [i.vendor, i.architecture, i.device, i.description].join("|"); };
const presentCrashed = () => remembered("presentCrashed", []).includes(adapterKey());
const mayPresent = () => query.get("present") === "1" || (query.get("present") !== "0" && !presentCrashed());
async function attachCanvas() {
  presenting = false;
  sentView = "";
  engineTexture = null;
  $("gpu-canvas")?.remove();
  // The page's canvas keeps the bytes' frame, or a placeholder.
  $("canvas").hidden = !frame && !early;
  if (kind !== "gpu" || !self.OffscreenCanvas || !HTMLCanvasElement.prototype.transferControlToOffscreen || !mayPresent()) return;
  const canvas = Object.assign(document.createElement("canvas"), { id: "gpu-canvas", className: "photo", hidden: true });
  $("canvas").after(canvas);
  try {
    await host.call("setCanvas", canvas.transferControlToOffscreen());
    presenting = true;
    showView();
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

// Rotation is finishing only, not part of the film's render/cache key.
const params = () => JSON.stringify({ ...state.params, io: { ...state.params.io, rotate_cw: 0 } });

// A new film brings its own grain, DIR and halation presets, as upstream
// applies them (macOS EditorModel.chooseFilm). `into`: the edit, or a copy
// (a preview tile's).
function chooseFilm(id, into = state) {
  into.film = id;
  set("film_render.development_time", null, into.params);
  for (const path of PRESET_PATHS) set(path, null, into.params);
  pairFilm(into);
}
function choosePaper(id, into = state) {
  into.paper = id;
  set("print_render.development_time", null, into.params);
}
// A slide scans directly; a film with a target paper prints on it. Others
// keep the paper, as on desktop.
function pairFilm(into = state) {
  const f = stock(into.film);
  if (!f) return;
  set("io.scan_film", f.positive, into.params);
  // The film set Scan film directly, not you: its row shows while on and
  // hides again with a negative film.
  if (into === state) revealed.delete("io.scan_film");
  if (papers().some((p) => p.id === f.target_print) && into.paper !== f.target_print) {
    into.paper = f.target_print;
    set("print_render.development_time", null, into.params);
  }
}

// Restores the defaults, keeping the film, paper and rotation, as desktop's
// Reset Adjustments does (macOS EditorModel.reset).
function resetAdjustments() {
  const kept = ["io.rotate_cw", "io.photo_long_side_px"].map((path) => [path, get(path)]);
  state.params = structuredClone(catalog.defaults);
  state.match = newMatch();
  for (const [path, value] of kept) set(path, value);
  set("io.scan_film", stock(state.film)?.positive ?? false);
}

// Grain lanes (`film_render.grain.<field>`) are null until edited: the
// engine then uses the film's preset, which the catalog resolves (macOS
// EditorModel.stockNumber and setStock).
const filmIsBW = () => stock(state.film)?.bw ?? false;
const stockDefaults = (path) => stock(state.film)?.grain?.[path.split(".").at(-1)] ?? [];
const stockValues = (path) => get(path) ?? stockDefaults(path);
const near = (a, b) => Math.abs(a - b) <= 1e-6 * Math.max(1, Math.abs(b));
function setStock(path, index, value) {
  const values = [...stockValues(path)];
  if (index >= values.length) return;
  values[index] = value;
  const preset = stockDefaults(path);
  const atPreset = values.length === preset.length && values.every((v, i) => near(v, preset[i]));
  set(path, atPreset ? null : values);
}

// Each control's view of the state, run after anything changes it.
const syncs = [];
const sync = () => syncs.forEach((f) => f());
// One control's reads and writes of the edit: sliders, stock lanes, toggles
// and choices alike.
function binding(c) {
  if (c.kind === "stock") {
    return {
      get: () => stockValues(c.path)[c.index] ?? 0,
      set: (v) => setStock(c.path, c.index, v),
      initial: () => stockDefaults(c.path)[c.index] ?? 0,
      modified: () => { const v = stockValues(c.path)[c.index]; return v !== undefined && !near(v, stockDefaults(c.path)[c.index] ?? 0); },
    };
  }
  if (c.kind === "offset") {
    // Prep's Exposure: the user's exposure above the match (macOS
    // EditorModel.exposureOffset), within the engine's ±3 EV in all.
    return {
      get: () => get(c.path) - matchedEv(),
      set: (v) => {
        const m = matchedEv();
        const offset = Math.min(Math.max(Math.round(v / c.step) * c.step, -3 - m), 3 - m);
        set(c.path, Math.min(3, Math.max(-3, Math.abs(offset) < 1e-9 ? m : m + offset)));
      },
      initial: () => 0,
      modified: () => Math.abs(get(c.path) - matchedEv()) > 1e-6,
    };
  }
  if (c.border) {
    const initial = Border.DEFAULT_BORDER[c.border];
    return {
      get: () => (c.percent ? border[c.border] * 100 : border[c.border]),
      set: (v) => { border[c.border] = c.percent ? v / 100 : v; },
      initial: () => (c.percent ? initial * 100 : initial),
      modified: () => (c.percent ? Math.abs(border[c.border] - initial) > 1e-9 : border[c.border] !== initial),
    };
  }
  return { get: () => get(c.path), set: (v) => set(c.path, v), initial: () => defaultValue(c.path), modified: () => isModified(c.path) };
}

const el = (tag, props = {}, ...children) => {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...children);
  return e;
};

// A reset button, shown while the control is off its default.
function resetButton(title, b, after) {
  const button = el("button", { type: "button", className: "reset", textContent: "↺", hidden: true });
  button.setAttribute("aria-label", `Reset ${title}`);
  button.addEventListener("click", () => { b.set(b.initial()); after(); });
  return button;
}

// A control's `when`: shown while that path is true, or `[path, value]`
// while it has that value.
const shownWhen = (when) => !when || (Array.isArray(when) ? get(when[0]) === when[1] : !!get(when));

// Commands the controls' `action` buttons run, with whether each may run now.
const actions = {
  balancePrint: { run: balancePrint, busy: () => balancing, busyLabel: "Balancing…", enabled: () => !balancing && !!host && !get("io.scan_film") },
};

// The (i) beside a control: its tooltip in a bubble, on click or tap, since
// phones have no hover. Clicking elsewhere, Escape or scrolling closes it.
const tip = el("div", { className: "tip", id: "tip", role: "tooltip" });
tip.setAttribute("popover", "auto");
document.body.append(tip);
let tipFor = null, tipWasOpen = false;
tip.addEventListener("toggle", (ev) => {
  if (ev.newState !== "closed") return;
  tipFor?.setAttribute("aria-expanded", "false");
  tipFor = null;
});
// Forgets the (i) at once: the toggle event comes a task later.
function closeTip() {
  if (!tipFor) return;
  tipFor.setAttribute("aria-expanded", "false");
  tipFor = null;
  tip.hidePopover();
}
// `text`: a string, or a function for one that depends on the edit.
function info(label, text) {
  const button = el("button", { type: "button", className: "info" });
  button.setAttribute("aria-label", `About ${label}`);
  button.setAttribute("aria-expanded", "false");
  // Light dismiss closes it on the press, before the click: a click on the
  // (i) that was open only closes it.
  button.addEventListener("pointerdown", () => { tipWasOpen = tipFor === button; });
  button.addEventListener("click", (ev) => {
    // Not the group's foldout or the label's control.
    ev.preventDefault();
    ev.stopPropagation();
    const open = tipWasOpen || tipFor === button;
    tipWasOpen = false;
    closeTip();
    if (open) return;
    tip.textContent = typeof text === "function" ? text() : text;
    tipFor = button;
    button.setAttribute("aria-expanded", "true");
    tip.showPopover();
    // Under the (i), or over it where the screen ends; inside the window.
    const r = button.getBoundingClientRect(), w = tip.offsetWidth, h = tip.offsetHeight;
    tip.style.left = `${Math.max(8, Math.min(r.left + r.width / 2 - w / 2, innerWidth - w - 8))}px`;
    const top = r.bottom + 4 + h > innerHeight - 8 ? r.top - 4 - h : r.bottom + 4;
    tip.style.top = `${Math.max(8, Math.min(top, innerHeight - h - 8))}px`;
  });
  return button;
}
// A static label's leading text gets its (i) (the stock area's rows).
function labelInfo(label, text) {
  const node = [...label.childNodes].find((n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim());
  const title = node.textContent.trim();
  node.replaceWith(el("span", { className: "label-line" }, el("span", { textContent: title }), info(title, text)));
  label.removeAttribute("title");
}

// A control's tooltip (help.js): its own, else its path's. Prep's Exposure
// reads differently while it's an offset on the match (macOS Inspector).
function helpFor(c) {
  if (c.kind === "offset") {
    return () => state.match
      ? "Brighter or darker than the match, in stops. It stays when the match changes with the film, paper or a preset."
      : "Brightens or darkens the scene before the film, in stops. Unlike Negative exposure, the print doesn’t make up for it.";
  }
  return c.help ?? Help.PATHS[c.path] ?? null;
}

function buildControl(c) {
  const row = buildRow(c);
  if (c.path ?? c.tutorial) row.dataset.tutorialPath = c.path ?? c.tutorial;
  const help = helpFor(c);
  if (help) {
    const button = info(c.label, help);
    if (c.kind === "action") row.append(button);
    else if (c.kind === "styles") row.prepend(el("div", { className: "head" }, el("span", { className: "label", textContent: c.label }), button));
    else row.querySelector(".head > .label, :scope > label > span")?.after(button);
  }
  // While matched the meter only sets where the highlight limit starts, so
  // Auto exposure shows when matching is off, or when it's changed.
  if (c.metering) {
    syncs.push(() => {
      row.hidden = !shownWhen(c.when) || (matchActive() && !isModified("camera.auto_exposure") && !isModified("camera.auto_exposure_method"));
    });
  }
  // A border setting: shown for its styles, off while the border is.
  if (c.border && c.border !== "enabled") {
    syncs.push(() => {
      row.hidden = !!c.styles && !c.styles.includes(border.style);
      row.classList.toggle("off", !border.enabled);
      for (const e of row.querySelectorAll("input, button, select")) e.disabled = !border.enabled || (e.dataset.fits === "false");
    });
  }
  return row;
}

function buildRow(c) {
  if (c.kind === "styles") return buildStyles(c);
  if (c.kind === "action") {
    const a = actions[c.action];
    const button = el("button", { type: "button", className: "btn", textContent: c.label });
    button.addEventListener("click", () => a.run());
    syncs.push(() => {
      button.disabled = !a.enabled();
      button.textContent = a.busy() ? a.busyLabel : c.label;
    });
    return el("div", { className: "ctl action" }, button);
  }
  if (c.kind === "match") {
    const sw = el("input", { type: "checkbox", className: "switch" });
    sw.addEventListener("change", () => setMatch(sw.checked));
    const row = el("div", { className: "ctl row" }, el("label", {}, el("span", { textContent: c.label }), sw));
    // Camera RAWs only, for now: a JPEG's own pixels as the reference isn't
    // in the browser yet.
    syncs.push(() => { sw.checked = !!state.match; row.hidden = !photo?.raw; });
    return row;
  }
  if (c.kind === "note") {
    const note = el("p", { className: "hint" });
    const row = el("div", { className: "ctl" }, note);
    syncs.push(() => { const text = matchNote(); row.hidden = !text; note.textContent = text ?? ""; });
    return row;
  }
  const b = binding(c);
  // A border change is a display pass; anything else renders.
  const changed = c.border ? () => borderChanged() : (mode) => { sync(); schedule(mode); };
  if (c.kind === "color") {
    const input = el("input", { type: "color" });
    input.setAttribute("aria-label", c.label);
    const reset = resetButton(c.label, b, () => changed("settled"));
    input.addEventListener("input", () => { b.set(input.value); changed("settled"); });
    const row = el("div", { className: "ctl row color" }, el("label", {}, el("span", { textContent: c.label }), reset, input));
    syncs.push(() => { input.value = b.get(); reset.hidden = !b.modified(); row.classList.toggle("changed", b.modified()); });
    return row;
  }
  if (c.kind === "toggle") {
    const sw = el("input", { type: "checkbox", className: "switch" });
    const reset = resetButton(c.label, b, () => changed("settled"));
    sw.addEventListener("change", () => { b.set(sw.checked); changed("settled"); });
    const row = el("div", { className: "ctl row" }, el("label", {}, el("span", { textContent: c.label }), reset, sw));
    syncs.push(() => { sw.checked = b.get(); reset.hidden = !b.modified(); row.classList.toggle("changed", b.modified()); });
    return row;
  }
  if (c.kind === "choice") {
    const select = el("select", {}, ...c.options.map((o) => new Option(optionTitle(o), o)));
    const reset = resetButton(c.label, b, () => changed("settled"));
    select.addEventListener("change", () => { b.set(select.value); changed("settled"); });
    const row = el("div", { className: "ctl choice" }, el("label", {}, el("span", { textContent: c.label }), reset, select));
    syncs.push(() => {
      select.value = b.get();
      reset.hidden = !b.modified();
      row.classList.toggle("changed", b.modified());
      row.hidden = !shownWhen(c.when);
    });
    return row;
  }
  // A slider under its title and value. Clicking the value types one; off
  // its default, a button resets it (macOS LabeledSlider).
  const title = el("span", { className: "label" });
  const value = el("button", { type: "button", className: "value", title: "Click to type a value" });
  const reset = resetButton(c.label, b, () => changed("settled"));
  const range = el("input", { type: "range", min: c.min, max: c.max, step: c.step });
  const track = el("div", { className: "track" }, range);
  touchSlider(track, range);
  const row = el("div", { className: "ctl slider" }, el("div", { className: "head" }, title, reset, value), track);
  range.setAttribute("aria-label", c.label);
  range.addEventListener("input", () => { b.set(snap(Number(range.value), c)); changed("interactive"); });
  range.addEventListener("change", () => { b.set(snap(Number(range.value), c)); changed("settled"); });
  const resetToInitial = () => { b.set(b.initial()); changed("settled"); };
  title.addEventListener("dblclick", resetToInitial);
  // Double-click or Option/Alt-click on the slider resets it, as in
  // Lightroom (macOS ResettableSlider). A double-click's first click still
  // moves the thumb. Alt-click is caught before the range or touchSlider
  // starts a drag.
  track.addEventListener("dblclick", () => { if (!range.disabled) resetToInitial(); });
  for (const type of ["pointerdown", "mousedown"]) {
    track.addEventListener(type, (ev) => {
      if (!ev.altKey || ev.button !== 0 || range.disabled) return;
      ev.preventDefault();
      ev.stopPropagation();
      if (type === "pointerdown") resetToInitial();
    }, { capture: true });
  }
  value.addEventListener("click", () => {
    const field = el("input", { className: "value-field", inputMode: "decimal", value: formatValue(b.get(), c.step) });
    field.setAttribute("aria-label", c.label);
    // Return, Tab and clicking elsewhere commit; Escape discards the typing.
    // Not through blur alone: removing a focused field doesn't fire it.
    let done = false;
    const finish = (commit) => {
      if (done) return;
      done = true;
      const typed = Number(field.value.replace(",", "."));
      field.replaceWith(value);
      if (commit && field.value.trim() && Number.isFinite(typed)) { b.set(snap(typed, c)); changed("settled"); }
    };
    field.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") finish(true);
      if (ev.key === "Escape") finish(false);
    });
    field.addEventListener("blur", () => finish(true));
    value.replaceWith(field);
    field.focus();
    field.select();
  });
  syncs.push(() => {
    const bw = c.kind === "stock" && filmIsBW();
    row.hidden = (bw && !c.bw) || !shownWhen(c.when);
    title.textContent = bw ? c.bw : c.label;
    // Sync never writes back: a value outside the slider's window stays.
    range.value = b.get();
    range.style.setProperty("--f", (Number(range.value) - c.min) / (c.max - c.min));
    value.textContent = formatValue(b.get(), c.step);
    reset.hidden = !b.modified();
    reset.title = `Reset to ${formatValue(b.initial(), c.step)}`;
    row.classList.toggle("changed", b.modified());
  });
  return row;
}

// The border's style picker (macOS BorderStylePicker): a drawn sample of
// each in the current mat and line colours. 35mm only fits 3:2 and 3:4
// photographs; the note under the picker says why it's off.
function buildStyles(c) {
  const buttons = Border.STYLES.map((style) => {
    const swatch = el("span", { className: "swatch" });
    const button = el("button", { type: "button", className: "style", title: style.help }, swatch, el("span", { textContent: style.title }));
    button.dataset.style = style.id;
    button.setAttribute("aria-label", `${style.title} border`);
    button.addEventListener("click", () => { border.style = style.id; borderChanged(); });
    return { style, button, swatch };
  });
  const note = el("p", { className: "hint note", hidden: true });
  const row = el("div", { className: "ctl styles" }, el("div", { className: "picker", role: "group" }, ...buttons.map((b) => b.button)), note);
  row.querySelector(".picker").setAttribute("aria-label", c.label);
  let colours = "";
  syncs.push(() => {
    const size = photoSize() ?? early;
    for (const { style, button } of buttons) {
      button.setAttribute("aria-pressed", border.style === style.id);
      button.dataset.fits = !size || Border.fits(style.id, size.width, size.height);
    }
    const fits35 = !size || Border.fits("35mm", size.width, size.height);
    note.hidden = fits35;
    if (!fits35) note.textContent = `35mm needs a 3:2 or 3:4 photograph. This one is ${Border.ratioName(size.width, size.height)}.`;
    if (colours !== border.color + border.lineColor) {
      colours = border.color + border.lineColor;
      for (const { style, swatch } of buttons) swatch.innerHTML = swatchSvg(style.id, border.color, border.lineColor);
    }
  });
  return row;
}

// macOS BorderSwatch, 50 × 36.
function swatchSvg(style, mat, line) {
  const photo = (x, y, w, h, r = 0) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${r}" fill="url(#sky-${style})"/>`;
  const perfs = (y) => Array.from({ length: 8 }, (_, i) => `<rect x="${1.5 + i * 6}" y="${y}" width="3.5" height="3.5" rx="0.8" fill="#f2f2f2"/>`).join("");
  const body = {
    solid: `<rect width="50" height="36" fill="${mat}"/>${photo(5, 5, 40, 26)}`,
    keyline: `<rect width="50" height="36" fill="${mat}"/><rect x="4" y="4" width="42" height="28" fill="${line}"/>${photo(5, 5, 40, 26)}`,
    film: `<rect width="50" height="36" fill="#000"/>${photo(2, 2, 46, 32, 2)}`,
    "35mm": `<rect width="50" height="36" fill="#000"/>${perfs(1.5)}${photo(1, 8, 48, 20)}${perfs(31)}`,
  }[style];
  return `<svg viewBox="0 0 50 36" width="50" height="36" aria-hidden="true"><defs><linearGradient id="sky-${style}" x1="0" y1="0" x2="0" y2="1">`
    + `<stop offset="0" stop-color="rgb(140 173 199)"/><stop offset="1" stop-color="rgb(219 189 143)"/></linearGradient></defs>${body}</svg>`;
}

// Per-viewer conveniences (which groups are open, the phone's tab and sheet
// height), kept where the browser allows it.
function remembered(key, fallback) {
  try { return JSON.parse(localStorage.getItem(`negs.${key}`)) ?? fallback; } catch { return fallback; }
}
function remember(key, value) {
  if ((tutorialPresentation || tutorialRestoring) && ["groups", "pane", "tab", "sheet", "stockFolds", "matchNewRaws"].includes(key)) return;
  try { localStorage.setItem(`negs.${key}`, JSON.stringify(value)); } catch {}
}

const sectionId = (section) => (section.tab ?? section.title).toLowerCase();

// On touch screens the slider itself takes no input (index.html), as a
// browser's slider keeps vertical swipes that start on it from scrolling the
// panel. Its track takes it instead: a sideways drag anywhere on it moves
// the value from where it was, as on iOS; an upward or downward one
// scrolls. A mouse or pen there (an iPad's trackpad) sets the value under it.
function touchSlider(track, range) {
  let drag = null;
  const fraction = () => (Number(range.value) - range.min) / (range.max - range.min);
  const to = (f) => {
    const before = range.value;
    range.value = Number(range.min) + Math.min(1, Math.max(0, f)) * (range.max - range.min);
    if (range.value !== before) range.dispatchEvent(new Event("input"));
  };
  // The thumb's centre travels the track less one thumb's width.
  const travel = () => track.clientWidth - parseFloat(getComputedStyle(range).getPropertyValue("--thumb"));
  track.addEventListener("pointerdown", (ev) => {
    if (ev.target !== track || range.matches(":disabled") || !ev.isPrimary) return;
    drag = { x: ev.clientX, y: ev.clientY, from: fraction(), id: ev.pointerId, moving: ev.pointerType !== "touch" };
    if (drag.moving) {
      track.setPointerCapture(ev.pointerId);
      const box = track.getBoundingClientRect();
      drag.x = box.x + (track.clientWidth - travel()) / 2;
      drag.from = 0;
      to((ev.clientX - drag.x) / travel());
    }
  });
  track.addEventListener("pointermove", (ev) => {
    if (!drag || ev.pointerId !== drag.id) return;
    const dx = ev.clientX - drag.x;
    if (!drag.moving) {
      if (Math.abs(dx) < 6 || Math.abs(dx) < Math.abs(ev.clientY - drag.y)) return;
      drag.moving = true;
      track.setPointerCapture(ev.pointerId);
    }
    to(drag.from + dx / travel());
  });
  const end = (ev) => {
    if (!drag || ev.pointerId !== drag.id) return;
    const moved = drag.moving;
    drag = null;
    if (moved) range.dispatchEvent(new Event("change"));
  };
  track.addEventListener("pointerup", end);
  track.addEventListener("pointercancel", end);
}

// Advanced controls (Settings; macOS calls it the same): off, the Lab shows
// the controls most photographs need (controls.js `basic`, `advanced`) and
// any that's off its default, so a preset never changes what you can't see.
// A row shown for its change stays until you switch pane or tab, reset or
// open a photograph, so setting it back doesn't pull it from under you.
let advanced = remembered("advanced", false);
const revealed = new Set();
function changedShows(key, changed) {
  if (changed) revealed.add(key);
  return revealed.has(key);
}
// Tucks `node` away while Advanced controls is off and `shows()` is false.
// Returns whether it showed at the last sync.
function tuck(node, shows) {
  let on = true;
  syncs.push(() => {
    on = advanced || shows();
    node.classList.toggle("tucked", !on);
  });
  return () => on;
}
function forgetRevealed() {
  revealed.clear();
  sync();
}

// The Lab's note that Advanced controls hides some: at the end of every
// Lab tab on the sheet, once at the Lab's end in the sidebar.
function advancedHint(tab) {
  const open = el("button", { type: "button", className: "link", textContent: "Settings › Advanced controls" });
  open.addEventListener("click", () => openSettings());
  const hint = el("p", { className: `hint advanced-hint ${tab ? "per-tab" : "lab-end"}` }, "Some settings are hidden. ", open, " shows them all.");
  if (tab) hint.dataset.tab = tab;
  hint.dataset.pane = "lab";
  syncs.push(() => { hint.hidden = advanced; });
  return hint;
}

function buildControls() {
  $("scroll").addEventListener("scroll", closeTip, { passive: true });
  addEventListener("resize", closeTip);
  const opened = remembered("groups", {});
  const sections = [...PREP_SECTIONS.map((s) => [s, "prep"]), ...SECTIONS.map((s) => [s, "lab"]), ...OUTPUT_SECTIONS.map((s) => [s, "output"])];
  for (const [section, pane] of sections) {
    // A fieldset, so a section or group that is off disables its controls.
    const body = el("fieldset", { className: "section" }, el("h2", { textContent: section.title }));
    body.dataset.tab = sectionId(section);
    body.dataset.pane = pane;
    // A phone tab with several sections keeps their titles.
    body.classList.toggle("titled", sections.filter(([s]) => sectionId(s) === body.dataset.tab).length > 1);
    const lab = pane === "lab";
    // Whether each of the section's loose controls and groups shows.
    const parts = [];
    for (const c of section.controls ?? []) {
      const row = buildControl(c);
      body.append(row);
      if (!lab) continue;
      if (!c.advanced) parts.push(() => true);
      else { const b = binding(c); parts.push(tuck(row, () => changedShows(c.path, b.modified()))); }
    }
    if (section.raw) syncs.push(() => { body.hidden = !photo?.raw; });
    for (const group of section.groups ?? []) {
      const key = `${section.title}/${group.title}`;
      const details = el("details", { open: opened[key] ?? group.open ?? false });
      details.dataset.tutorialGroup = key;
      details.addEventListener("toggle", () => remember("groups", { ...remembered("groups", {}), [key]: details.open }));
      const summary = el("summary", {}, el("span", { textContent: group.title }));
      if (Help.GROUPS[group.title]) summary.append(info(group.title, Help.GROUPS[group.title]));
      const controls = el("fieldset", { className: "controls" });
      // Past the basics, each control shows with Advanced controls or once changed.
      const changed = [];
      group.controls.forEach((c, i) => {
        const row = buildControl(c);
        controls.append(row);
        if (!lab || i < (group.basic ?? 0) || c.kind === "action") return;
        const b = binding(c), id = c.index == null ? c.path : `${c.path}.${c.index}`;
        changed.push(tuck(row, () => changedShows(id, b.modified())));
      });
      if (group.enable) {
        const sw = el("input", { type: "checkbox", className: "switch", title: `${group.title} on or off` });
        sw.setAttribute("aria-label", group.title);
        sw.addEventListener("click", (ev) => ev.stopPropagation());
        sw.addEventListener("change", () => { set(group.enable, sw.checked); sync(); schedule("settled"); });
        syncs.push(() => {
          sw.checked = get(group.enable);
          controls.disabled = !sw.checked;
        });
        summary.append(sw);
      }
      details.append(summary, controls);
      // RAW-only settings (RAW White Balance) hide for developed rasters, as
      // on desktop; the edit keeps them for the next RAW.
      if (group.raw) syncs.push(() => { details.hidden = !photo?.raw; });
      body.append(details);
      // A group with no basics shows with Advanced controls, or once
      // something in it changed. After its controls: their syncs run first.
      if (lab) {
        parts.push(group.basic ? () => true
          : tuck(details, () => changedShows(key, !!group.enable && isModified(group.enable)) || changed.some((shown) => shown())));
      }
    }
    if (section.print) {
      const footer = el("p", { className: "hint footer", textContent: section.footer });
      body.append(footer);
      syncs.push(() => {
        body.disabled = get("io.scan_film");
        footer.hidden = !body.disabled;
      });
    } else if (section.footer) {
      body.append(el("p", { className: "hint footer", textContent: section.footer }));
    }
    $(pane === "lab" ? "sections" : `${pane}-sections`).append(body);
    if (lab) {
      tuck(body, () => parts.some((shown) => shown()));
      $("sections").append(advancedHint(sectionId(section)));
    }
  }
  $("sections").append(advancedHint(null));
  // Scan film directly, beside the stocks: few photographs want it.
  tuck($("scan-film").closest("label"), () => changedShows("io.scan_film", get("io.scan_film")));
}

// Desktop's Prep | Lab | Output switch (macOS AdjustmentInspector): which
// pane the sidebar shows. On the phone's sheet each section has its tab
// instead. Remembered "post" was Output's.
const PANES = ["prep", "lab", "output"];
function showPane(pane) {
  if (pane === "post") pane = "output";
  if (!PANES.includes(pane)) pane = "prep";
  $("side").dataset.pane = pane;
  for (const button of $("panes").children) button.setAttribute("aria-selected", button.dataset.pane === pane);
  remember("pane", pane);
}
function buildPanes() {
  for (const button of $("panes").children) {
    button.addEventListener("click", () => {
      showPane(button.dataset.pane);
      forgetRevealed();
      $("scroll").scrollTop = 0;
    });
  }
  showPane(remembered("pane", "prep"));
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

// Starred stocks, in the order starred (macOS Previews.favoriteFilms): first
// in the grids and the menus, each staying in its group too. The menus'
// copies have their own values, so the closed menu shows the group's entry.
const FAVORITE = "favorite:";
const favoritesKey = (which) => (which === "film" ? "favoriteFilms" : "favoritePapers");
// Until a viewer changes them, the films start with these starred; what
// they unstar stays unstarred (an empty list is remembered too).
const DEFAULT_FAVORITES = { film: ["kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200"], paper: [] };
const favorites = (which) => remembered(favoritesKey(which), DEFAULT_FAVORITES[which]);
const stockValue = (value) => (value.startsWith(FAVORITE) ? value.slice(FAVORITE.length) : value);
function fillFavorites(which) {
  const select = $(which);
  select.querySelector("optgroup.favorites")?.remove();
  const ids = favorites(which).filter((id) => stock(id)?.paper === (which === "paper"));
  if (!ids.length) return;
  const og = Object.assign(document.createElement("optgroup"), { label: "Favorites", className: "favorites" });
  for (const id of ids) og.append(new Option(`★ ${stock(id).name}`, FAVORITE + id));
  select.prepend(og);
  select.value = which === "film" ? state.film : state.paper;
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
  fillFavorites("film");
  fillFavorites("paper");
  labelInfo($("film").closest("label"), Help.FILM);
  labelInfo($("paper-row"), Help.PAPER);
  labelInfo($("film-time-row"), Help.PATHS["film_render.development_time"]);
  labelInfo($("paper-time-row"), Help.PATHS["print_render.development_time"]);
  labelInfo($("scan-film").closest("label"), Help.PATHS["io.scan_film"]);
  buildPreviews();
  developmentTime("film", "film_render.development_time", () => state.film);
  developmentTime("paper", "print_render.development_time", () => state.paper);
  state.params = structuredClone(catalog.defaults);
  state.paper = "kodak_portra_endura";
  chooseFilm(stock(query.get("film"))?.paper === false ? query.get("film") : "kodak_portra_400");
  if (stock(query.get("paper"))?.paper) choosePaper(query.get("paper"));
  syncs.push(() => {
    $("film").value = state.film;
    $("paper").value = state.paper;
    $("scan-film").checked = get("io.scan_film");
  });
  sync();
}

// A stock's development time picker (macOS Inspector developmentTime): the
// times its family lists, shown when there's a choice. Profile default is
// null; a stock change unsets it (chooseFilm, choosePaper).
const minutes = (t) => `${Number(t.toFixed(4))} min`;
function developmentTime(which, path, id) {
  const row = $(`${which}-time-row`), select = $(`${which}-time`);
  let listed = null;
  select.addEventListener("change", () => {
    set(path, select.value === "" ? null : Number(select.value));
    sync();
    schedule("settled");
  });
  syncs.push(() => {
    const times = stock(id())?.development_times ?? [];
    row.hidden = times.length <= 1 || (which === "paper" && get("io.scan_film"));
    const key = times.join();
    if (key !== listed) {
      listed = key;
      select.replaceChildren(new Option("Profile default", ""), ...times.map((t) => new Option(minutes(t), String(t))));
    }
    const time = get(path);
    // An imported time is one the family lists; match it as a number.
    const i = time === null ? -1 : times.findIndex((t) => Math.abs(t - time) <= 1e-6 * Math.max(1, Math.abs(t)));
    select.value = i < 0 ? "" : String(times[i]);
  });
}

// ---- Phone and iPad layout -------------------------------------------------

// Where a sidebar doesn't fit beside the photograph, the inspector is a
// bottom sheet with a tab per section (index.html has the same query).
const SHEET = matchMedia("(max-width: 899px) and (orientation: portrait), (max-width: 639px)");
// Prep comes before Stock, as it does before the film, but Stock opens first.
const TABS = [...PREP_SECTIONS.map((s) => ({ id: sectionId(s), title: s.tab ?? s.title })), { id: "stock", title: "Stock" },
  ...[...SECTIONS, ...OUTPUT_SECTIONS].map((s) => ({ id: sectionId(s), title: s.tab ?? s.title }))]
  .filter((t, i, all) => all.findIndex((u) => u.id === t.id) === i);

function showTab(id) {
  if (!TABS.some((t) => t.id === id)) id = "stock";
  $("side").dataset.tab = id;
  // The footer too, wherever it is: buildSheet moves it into the scroll on
  // the sheet after the first tab is shown.
  for (const panel of [...$("scroll").querySelectorAll("[data-tab]"), $("bottom")]) panel.classList.toggle("shown", panel.dataset.tab === id);
  for (const button of $("tabs").children) button.setAttribute("aria-selected", button.dataset.tab === id);
  remember("tab", id);
}

// The sheet rests at one of three heights: just its tabs, the room the
// photograph leaves at the screen's full width (within 40 to 62% of it), or
// most of the screen. Drag the grabber between them, or tap it to step.
const SNAPS = ["low", "mid", "high"];
let sheet = remembered("sheet", "mid");
function snapHeight(snap) {
  const side = $("side"), height = $("app").clientHeight, width = $("app").clientWidth;
  if (tutorialPresentation) return Math.max(0, Math.min(height - 96, height * (snap === "high" ? 0.74 : 0.59)));
  if (snap === "low") return (cropSession ? $("grabber").offsetHeight + side.querySelector(".top").offsetHeight
    : $("scroll").offsetTop - side.offsetTop) + parseFloat(getComputedStyle(side).paddingBottom);
  if (snap === "high") return 0.8 * height;
  const shape = photoSize() ?? early;
  if (!shape) return 0.45 * height;
  const pad = width < 500 ? 12 : 32, hud = 28;
  const view = ((width - 2 * pad) * shape.height) / shape.width + 2 * pad + hud;
  return Math.min(0.62 * height, Math.max(0.4 * height, height - view));
}
function setSheet(snap) {
  sheet = SNAPS.includes(snap) ? snap : "mid";
  if (tutorialPresentation && sheet === "low") sheet = "mid";
  $("app").style.setProperty("--sheet", `${Math.round(snapHeight(sheet))}px`);
  remember("sheet", sheet);
}

function buildSheet() {
  for (const t of TABS) {
    const button = el("button", { type: "button", textContent: t.title, role: "tab" });
    button.dataset.tab = t.id;
    button.addEventListener("click", () => {
      showTab(t.id);
      forgetRevealed();
      if (sheet === "low") setSheet("mid");
      $("scroll").scrollTop = 0;
    });
    $("tabs").append(button);
    // A tab whose sections Advanced controls hides goes too (Scan).
    const sections = $("scroll").querySelectorAll(`.section[data-tab="${t.id}"]`);
    if (sections.length) {
      syncs.push(() => {
        button.hidden = [...sections].every((s) => s.classList.contains("tucked"));
        if (button.hidden && $("side").dataset.tab === t.id) showTab("stock");
      });
    }
  }
  showTab(remembered("tab", "stock"));
  setSheet(sheet);
  let drag = null;
  // On the sheet, and on screens too short to spare the room, the engine's
  // footer scrolls with the Stock tab; otherwise it stays at the sidebar's foot.
  const short = matchMedia("(max-height: 599px)");
  const stocks = $("side").querySelector(".stock");
  const place = () => {
    // The Prep | Lab | Output panes are the sidebar's; the sheet shows a tab at a time.
    $("side").classList.toggle("sheet", SHEET.matches);
    // Film and paper: pinned above the panes in the sidebar, as on the Mac,
    // since they shape everything the panes adjust; the sheet's Stock tab.
    SHEET.matches ? $("prep-sections").before(stocks) : $("panes").before(stocks);
    if (folds) { folds.film.center(); folds.paper.center(); }
    SHEET.matches || short.matches ? $("scroll").append($("bottom")) : $("scroll").after($("bottom"));
    setSheet(sheet);
  };
  SHEET.addEventListener("change", place);
  short.addEventListener("change", place);
  addEventListener("resize", () => drag || setSheet(sheet));
  place();

  const grabber = $("grabber");
  grabber.addEventListener("pointerdown", (ev) => {
    grabber.setPointerCapture(ev.pointerId);
    drag = { y: ev.clientY, height: $("side").offsetHeight, moved: false };
    $("app").classList.add("dragging-sheet");
  });
  grabber.addEventListener("pointermove", (ev) => {
    if (!drag) return;
    const dy = drag.y - ev.clientY;
    drag.moved ||= Math.abs(dy) > 4;
    const max = tutorialPresentation ? Math.max(0, $("app").clientHeight - 96) : 0.9 * $("app").clientHeight;
    $("app").style.setProperty("--sheet", `${Math.min(max, Math.max(snapHeight("low"), drag.height + dy))}px`);
  });
  const end = () => {
    if (!drag) return;
    $("app").classList.remove("dragging-sheet");
    const { moved } = drag;
    drag = null;
    if (!moved) return setSheet(SNAPS[(SNAPS.indexOf(sheet) + 1) % SNAPS.length]);
    const height = $("side").offsetHeight;
    setSheet(SNAPS.reduce((a, b) => Math.abs(snapHeight(b) - height) < Math.abs(snapHeight(a) - height) ? b : a));
  };
  grabber.addEventListener("pointerup", end);
  grabber.addEventListener("pointercancel", end);
}

// ---- Rendering -------------------------------------------------------------

// Interactive frames start at half the settled size and adapt to stay near
// the frame budget: on a fast GPU up to the settled size, on a slow one or
// the CPU engine down to a few hundred pixels. A WebGPU frame costs about
// 8 ms whatever its size, so it goes no smaller than a quarter.
const BUDGET_MS = 90;
let interactiveScale = 0.5, minScale = 0.15;
let wanted = null, running = false, settleTimer = 0, exporting = false, lastSlide = -Infinity;
const renders = [];
let renderFailures = 0;

function schedule(mode) {
  // Every change comes here, so a WB the photograph wasn't developed at
  // develops it again, whatever changed it (a control, Reset, `negs`).
  requestDevelop(mode);
  if (!photo) return;
  clearTimeout(settleTimer);
  // A slider that stops without a change event (keyboard, wheel) settles anyway.
  if (mode === "interactive") {
    settleTimer = setTimeout(() => schedule("settled"), 250);
    lastSlide = performance.now();
  }
  wanted = mode;
  pump();
}

async function pump() {
  // Not while the engine's GPU solves the brightness match either: its
  // trials share the chain's buffers.
  if (running || exporting || matching?.engine || previewing) return;
  running = true;
  $("busy").classList.add("on");
  // Not while the engine's photograph is being replaced: the next frame is
  // of the new one.
  while (wanted && photo && !loading && !source.busy) {
    const interactive = wanted === "interactive";
    wanted = null;
    const side = renderSide();
    const maxDim = interactive ? Math.max(256, Math.round(side * interactiveScale)) : side;
    if (!interactive) settledSide = side;
    try {
      // The engine presents with the view it was last sent.
      if (presenting && viewKey() !== sentView) sendView().catch(() => {});
      const r = await host.call("render", state.film, state.paper, params(), maxDim, interactive, presenting);
      await draw(r);
      if (interactive) {
        interactiveScale = Math.min(1, Math.max(minScale, interactiveScale * Math.sqrt(BUDGET_MS / Math.max(r.elapsedMs, 1))));
      }
      hud(r, interactive);
      renders.push({ interactive, width: r.width, height: r.height, elapsedMs: r.elapsedMs, stages: r.stages, presented: r.presented, backend: r.backend, memoryMiB: r.memoryMiB });
      hideMessage();
    } catch (e) {
      renderFailures++;
      await failed(e, "Render failed");
    }
  }
  running = false;
  $("busy").classList.remove("on");
  reconcileMatch();
  pumpPreviews();
}

// The longest side to render, in photograph pixels: as many as the device
// pixels the view covers, so all of them from 100% on, within what the GPU
// takes and the device's cap (MAX_PIXELS). The CPU engine renders the
// fitted size and more only up to a few megapixels, as each takes seconds.
// Touch screens stop at 4.4 MP (2560×1707): on the iPhone 13 Pro, 3072 px
// frames and a 12 MP export after edits got the tab killed from 1.4 GB
// (docs/web-prep-port-plan.md).
const MAX_PIXELS = matchMedia("(pointer: coarse)").matches ? 4.4e6 : 40e6;
const CPU_PIXELS = 4e6;
let limits = null;       // the WebGPU engine's: { side, pixels }

// The pixels the engine keeps of a photograph, 0 for all. A phone or tablet,
// whose tab iOS ends from about 1.5 GB, keeps no more than it ever renders,
// and exports that copy: a full-size export there doesn't fit, so the desktop
// exports every pixel. The same budget as renderSide's, so its largest frame
// is this copy at 1:1.
const keptPixels = () => query.has("kept") ? Number(query.get("kept")) // measurement only: the working copy's size
  : (matchMedia("(pointer: coarse)").matches ? Math.min(MAX_PIXELS, limits?.pixels ?? MAX_PIXELS) : 0);
// The engine's photograph from `bytes`, a RAW developed at `wb` (the edit's
// io.raw_white_balance). Only through `source` (below). A decode made in
// web/decoder.js is dropped instead of kept when `stale()` says by then it
// isn't wanted; the engine's own load keeps what it decodes. A failure of
// the engine's call (`forgot`) has let the last photograph go.
const superseded = () => Object.assign(new Error("superseded"), { stale: true });
const forgot = (e) => { throw Object.assign(e, { forgot: true }); };
async function loadPhoto(bytes, wb, { stale = () => false } = {}) {
  if (stale()) throw superseded();
  const whiteBalance = JSON.stringify(wb ?? {});
  const pixels = keptPixels();
  if (!pixels) return host.call("load", bytes, 0, 0, whiteBalance).catch(forgot);
  // Decoded in a worker that then ends, so the engine's memory holds only
  // the copy it keeps, never the decode's peak.
  // Leave the old source/display intact for failed-decode recovery, but do
  // not overlap its cached GPU working buffers with the new RAW decoder.
  await host.call("releaseRenderBuffers");
  if (stale()) throw superseded();
  const t = performance.now();
  const decoded = await decodeKept(bytes, pixels, whiteBalance);
  if (stale()) throw superseded();
  const size = await host.call("loadDecoded", decoded).catch(forgot);
  console.log(`decoded in a worker in ${decoded.decodeMs.toFixed(0)} ms (ready after ${decoded.readyMs.toFixed(0)} ms), ${decoded.memoryMiB} MiB; kept after ${(performance.now() - t).toFixed(0)} ms; RAW develop: ${decoded.bounded ? "bounded" : "legacy"}`);
  return { ...size, decodeMs: decoded.decodeMs };
}

// web/decoder.js on `bytes`, kept within `maxPixels`. Each decodes once;
// the next starts after, so it has compiled by the next photograph.
let decoder = null;
function startDecoder() {
  const worker = new Worker("decoder.js", { type: "module" });
  worker.postMessage({ threads });
  return worker;
}
function decodeKept(bytes, maxPixels, whiteBalance) {
  const worker = decoder ?? startDecoder();
  decoder = null;
  // A panic on one of its rayon threads says so here, as the engine's do;
  // its instance is 0, which no engine is.
  const panics = new BroadcastChannel("negs-engine-panic");
  return new Promise((resolve, reject) => {
    worker.onmessage = ({ data: { result, error } }) => error ? reject(new Error(error)) : resolve(result);
    worker.onerror = (ev) => reject(new Error(ev.message || "the decoder stopped"));
    panics.onmessage = ({ data }) => data.instance === 0 && reject(new Error(`the decoder crashed: ${data.message}`));
    worker.postMessage({ bytes, maxDim: 0, maxPixels, rawDevelop: query.get("rawDevelop"), whiteBalance });
  }).finally(() => {
    panics.close();
    worker.terminate();
    decoder ??= startDecoder();
  });
}
// A diffusion filter's FFT grid, several times the frame, may take this much
// there, 0 for any: past it, the engine makes the glow from a copy of the
// frame shrunk 2× or more (a 12 MP export's default diffusion halves, which
// differs from the exact glow by at most 0.5 ΔE00). Previews fit as they are.
const DIFFUSION_GRID_BUDGET = matchMedia("(pointer: coarse)").matches ? 160 * 2 ** 20 : 0;
// The photograph as opened: its size as decoded, and as the engine keeps and
// exports it.
const opened = (name, bytes, size) => ({ name, bytes, width: size.width, height: size.height, exportWidth: size.kept, exportHeight: size.keptHeight });
function renderSide() {
  const long = Math.max(photo.width, photo.height), short = Math.min(photo.width, photo.height);
  const sideAt = (zoom) => Math.min(long, Math.round(long * zoom));
  // Fitted, as many as the photograph would fill without a border, so a
  // border's change never renders: the frame shrinks into it.
  let side = sideAt(view.fitting() ? view.fitZoom(true) : view.zoom());
  const pixels = limits ? Math.min(limits.pixels, MAX_PIXELS) : Math.max(CPU_PIXELS, (sideAt(view.fitZoom(true)) ** 2 * short) / long);
  if ((side * side * short) / long > pixels) side = Math.floor(Math.sqrt((pixels * long) / short));
  if (limits) side = Math.min(side, limits.side);
  return Math.max(1, side);
}

// ---- The view --------------------------------------------------------------

// Around the photograph, as the page's background (index.html --bg).
const BACKGROUND = [0x16 / 255, 0x16 / 255, 0x16 / 255];
const view = createView($("stage"), $("view"), { changed: showView, settled: viewSettled, enabled: () => !cropSession,
  around: (w, h) => !cropSession && borderLayout(w, h) || { width: w, height: h, x: 0, y: 0 } });
const cropEditor = createCropEditor({ session: () => cropSession, view, changed: showView,
  exportSize: () => ({ width: photo.exportWidth, height: photo.exportHeight }) });
let frame = null;        // the last frame drawn from bytes: { bitmap, width, height }
let settledSide = 0;     // the longest side of the last settled frame asked for
let lastHud = null;
const views = [];        // display passes' times (the engine's, or the page's paint), for checks

// Draw the photograph where the view now puts it: the engine draws its
// canvas again from the frame it holds (one pass at a time, the newest view
// next), or the page draws the bytes.
let showing = false, showAgain = false, sentView = "";
function showView() {
  if (!photo && !early) return;
  zoomLevel();
  cropEditor.refresh();
  if (!presenting || early) {
    const t = performance.now();
    paint();
    if (!presenting && !early) views.push(performance.now() - t);
  }
  if (!presenting || !photo) return;
  if (showing) return void (showAgain = true);
  showing = true;
  sendView()
    .then((ms) => ms >= 0 && views.push(ms), () => {})
    .finally(() => {
      showing = false;
      if (showAgain) {
        showAgain = false;
        showView();
      }
    });
}
const viewKey = () => JSON.stringify([view.size(), view.rect(), showingOriginal, borderView(), photoGeometry()?.matrix]);
function sendView() {
  const { width, height } = view.size(), r = view.rect();
  sentView = viewKey();
  sendBorderTexture();
  return host.call("show", width, height, [r.x, r.y, r.width, r.height], BACKGROUND, showingOriginal, borderView(), photoGeometry()?.matrix ?? []);
}

// Once a zoom, pan or resize stops: render again when the view needs more
// pixels than the frame has, or, fitted, other ones.
function viewSettled() {
  if (cropSession) return;
  if (presentationOnly) { presentationOnly = false; return; }
  if (!photo) return;
  const side = renderSide();
  if (side > settledSide + 1 || (view.fitting() && side !== settledSide)) schedule("settled");
}

// The bytes' frame (or the placeholder) on the page's canvas, placed as the
// engine places its own (`View::place` in wgpu_backend.rs): 1:1 within a
// pixel, else smoothed.
function paint() {
  const canvas = $("canvas"), { width, height } = view.size();
  if (canvas.width !== width || canvas.height !== height) Object.assign(canvas, { width, height });
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = `rgb(${BACKGROUND.map((c) => Math.round(c * 255)).join(" ")})`;
  ctx.fillRect(0, 0, width, height);
  const shown = early ?? (photo && (showingOriginal && original?.bitmap ? original : frame));
  if (!shown) return;
  const r = view.rect(), size = photoSize();
  const l = !early && !cropSession && borderLayout(size.width, size.height);
  if (l) Border.paintUnder(ctx, l, r, size.width);
  Crop.paint(ctx, shown.bitmap, r, photoGeometry());
  if (l) Border.paintOver(ctx, l, r, size.width, borderTexture()?.bitmap);
}

// ---- Crop, leveling and quarter turns ----------------------------------------

function presentationChanged() {
  if (!photo) return;
  presentationOnly = true;
  const size = photoSize();
  view.setPhoto(size.width, size.height);
  if (lastHud) hud(...lastHud);
  sync();
  setSheet(sheet);
}
function cropMode() {
  const on = !!cropSession;
  $("view").classList.toggle("cropping", on);
  $("side").classList.toggle("cropping", on);
  $("crop").setAttribute("aria-pressed", on);
  for (const id of ["crop-cancel", "crop-done", "crop-inspector"]) $(id).hidden = !on;
  $("export").hidden = on;
  presets.sync();
  updateOriginal();
  cropEditor.refresh();
}
function beginCrop() {
  if (cropSession || !photo || loading || early || !developed || exporting) return;
  holdOriginal(false);
  cropSession = new Crop.Session(crop, photo.width, photo.height, rotation());
  cropMode();
  if (sheet === "low") setSheet("mid");
  presentationChanged();
}
function endCrop(apply) {
  if (!cropSession) return;
  if (apply) { crop = cropSession.committed(); set("io.rotate_cw", cropSession.rotation); }
  cropSession = null;
  cropMode();
  presentationChanged();
  $("crop").focus({ preventScroll: true });
}
function rotate(quarterTurns) {
  if (!photo || loading || early || !developed || exporting) return;
  if (cropSession) cropSession.rotate(quarterTurns);
  else set("io.rotate_cw", Crop.turns(rotation() + quarterTurns));
  presentationChanged();
}

// ---- The border --------------------------------------------------------------

// Desktop's presentation border (macOS BorderLayers), drawn around the
// photograph in the display pass, so a change never renders the film. The
// 120 frame and the 35mm strip need textures (data/borders), downloaded
// when first chosen and kept for the session; until one arrives the border
// shows without it, and the 35mm canvas is the photograph alone.
const textures = new Map();   // what a texture is for (textureFor) → { path, bytes, width, height, bitmap } or its download
let engineTexture = null;     // the texture the engine has

// The border a `w` × `h` photograph shows, and the texture it needs: the
// 120 frame's, or the film's 35mm strip.
function textureFor(w, h) {
  const b = Border.shown(border, w, h);
  if (b?.style === "film") return Border.FILM_TEXTURE.path;
  if (b?.style === "35mm") return Border.stripPath(Border.filmFormat(w, h), state.film);
  return null;
}
// That texture, once it's here. Until another film's strip arrives, the
// last strip of the same format stands in (as on macOS).
let lastStrip = null;
function borderTexture(w = photoSize()?.width, h = photoSize()?.height) {
  const key = w && textureFor(w, h);
  const t = key && textures.get(key);
  if (t && !(t instanceof Promise)) return t;
  return key && border.style === "35mm" && lastStrip?.format === Border.filmFormat(w, h) ? lastStrip : null;
}
// Where everything goes around a `w` × `h` photograph (web/border.js), or
// null without a border.
function borderLayout(w, h) {
  const b = Border.shown(border, w, h);
  return b && Border.layout(b, w, h, borderTexture(w, h));
}
// The layout as the engine's view takes it.
function borderView() {
  const size = photoSize(), l = photo && !cropSession && borderLayout(size.width, size.height);
  return l ? Border.viewOf(l, size.width, size.height) : [];
}

// Fetch the texture the border needs, with progress, once: a film without
// an edge print takes the generic strip.
function fetchBorderTexture() {
  const size = photoSize();
  const key = photo && !cropSession && textureFor(size.width, size.height);
  if (!key || textures.has(key)) return;
  const style = Border.STYLES.find((s) => s.id === border.style).title;
  const progress = (f) => showMessage(`Downloading the ${style} border… ${Math.floor(f * 100)}%`);
  const get = (path) => download(`data/${path}`, undefined, progress).then((bytes) => ({ path, bytes }));
  const load = get(key)
    .catch((e) => {
      // Offline, the generic strip may be saved where the film's isn't.
      if ((e.status !== 404 && !e.offline) || border.style !== "35mm") throw e;
      return get(Border.genericStrip(Border.filmFormat(size.width, size.height)));
    })
    .then(async ({ path, bytes }) => {
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const format = key === Border.FILM_TEXTURE.path ? null : Border.filmFormat(size.width, size.height);
      const t = { path, format, bytes, width: bitmap.width, height: bitmap.height, bitmap };
      textures.set(key, t);
      if (format) lastStrip = t;
      hideMessage();
      view.refresh();
    }, (e) => {
      textures.delete(key);
      showMessage(`The border couldn't be downloaded: ${e.message}`, true, 4000);
    });
  textures.set(key, load);
}

// The engine draws the texture the view needs; it gets each once.
function sendBorderTexture() {
  const t = borderTexture();
  if (!t || t.path === engineTexture) return;
  engineTexture = t.path;
  host.call("borderTexture", t.path, t.bytes).catch((e) => console.warn(`The border texture didn't load: ${e.message}`));
}

// A border setting changed: lay it out again, a display pass.
function borderChanged() {
  sync();
  view.refresh();
}

// A presented frame is already on the engine's canvas; otherwise draw the
// bytes. The photograph's first frame takes the placeholder's place; while
// another photograph opens, its placeholder stays.
async function draw(r) {
  const canvas = $("canvas"), gpu = $("gpu-canvas");
  if (!r.presented) {
    const image = new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height);
    const bitmap = await createImageBitmap(image, { colorSpaceConversion: "none", premultiplyAlpha: "none" });
    frame?.bitmap.close();
    frame = { bitmap, width: r.width, height: r.height };
  }
  if (loading) return;
  if (activity) mark("frame", photo.name);
  dropEarly();
  setBadge(null);
  developed = true;
  if (!r.presented) paint();
  canvas.hidden = r.presented;
  if (gpu) gpu.hidden = !r.presented;
  $("empty").hidden = true;
  $("zoom").hidden = false;
  $("edit-tools").hidden = false;
  updateOriginal();
}

// ---- The placeholder -------------------------------------------------------

// What shows while a photograph opens, until its first frame (web/placeholder.js):
// { bitmap, width, height } in the photograph's (estimated) size, and what it covers.
let early = null;
// When each opening's placeholder, decode and first frame happened, for checks.
const timeline = [];
const mark = (event, name) => timeline.push({ event, name, t: performance.now() });
function showEarly(p, opening) {
  const canvases = [...document.querySelectorAll(".photo")];
  early = { ...p, opening, covered: { canvases: canvases.map((c) => [c, c.hidden]), hud: $("hud").textContent } };
  for (const c of canvases) c.hidden = c.id !== "canvas";
  $("empty").hidden = true;
  $("hud").textContent = "";
  view.setPhoto(p.width, p.height);
  setSheet(sheet);
}
// Gone, once the photograph shows; or, when it didn't open, showing again
// what it covered.
function dropEarly(restore = false) {
  if (!early) return;
  const { bitmap, covered } = early;
  early = null;
  bitmap.close();
  if (!restore) return;
  $("empty").hidden = !!photo;
  for (const [c, hidden] of covered.canvases) c.hidden = hidden;
  $("hud").textContent = covered.hud;
  if (photo) { const size = photoSize(); view.setPhoto(size.width, size.height); }
  else paint();
}

// macOS ActivityBadge: "Developing" until a photograph's first frame shows,
// and "Original" over it while the original shows.
let activity = null, originalBadge = null;
function setBadge(text) {
  activity = text;
  showBadge();
}
function showBadge() {
  const text = originalBadge ?? activity;
  $("badge").hidden = !text;
  if (text) $("badge-text").textContent = text;
}

// ---- Show Original -----------------------------------------------------------

// The photograph before any film, as desktop's Show Original (macOS
// EditorModel "Original"; the helper's `original`, as Linux and Windows show
// it): the engine's decoded source, rawler's develop for a RAW, made at the
// settled frame's size and drawn in the developed frame's place with the
// same view, so switching is one display pass. It shows while the button or
// `\` is held; a tap toggles it on touch screens, where holding would hide
// the photograph under the finger. The WebGPU engine keeps it on its canvas;
// otherwise the page keeps its bitmap.
let original = null;          // { side, width, height, presented, bitmap, elapsedMs, stages }
let originalLoad = null;      // the original being made
let originals = 0;            // which photograph and engine it's for
let holding = false;          // asked to show it
let showingOriginal = false;  // and it shows
let developed = false;        // the photograph's first frame has shown
const canShowOriginal = () => !!photo && !loading && !early && developed && !!host && !cropSession;

// Starts making the original at the settled frame's size, if it isn't made
// or being made: when the pointer reaches the button, so a press usually
// shows it at once.
function prepareOriginal() {
  if (!canShowOriginal() || originalLoad || original?.side === settledSide) return originalLoad;
  const made = originals, side = settledSide;
  originalLoad = host.call("original", params(), side, presenting).then(async (r) => {
    if (made !== originals) return;
    const bitmap = r.presented ? null : await createImageBitmap(
      new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height),
      { colorSpaceConversion: "none", premultiplyAlpha: "none" });
    if (made !== originals) return bitmap?.close();
    original?.bitmap?.close();
    original = { side, width: r.width, height: r.height, presented: r.presented, bitmap, elapsedMs: r.elapsedMs, stages: r.stages };
    console.log(`original ${r.width}×${r.height} in ${r.elapsedMs.toFixed(0)} ms (${r.stages})`);
    // A larger one replaces the one showing.
    if (showingOriginal) showView();
  }, (e) => {
    if (made !== originals) return;
    holding = false;
    showMessage(`The original couldn't be shown: ${e.message}`, true, 4000);
  }).finally(() => {
    if (made !== originals) return;
    originalLoad = null;
    updateOriginal();
  });
  return originalLoad;
}

// Hold (or let go of) the original, as macOS holdOriginal.
function holdOriginal(on) {
  holding = on && canShowOriginal();
  updateOriginal();
}

// Shows the original while it's held and can show, "Loading Original" until
// it's made; one made for a smaller frame shows smoothed until the new size's
// is.
function updateOriginal() {
  const can = canShowOriginal(), show = holding && can, on = show && !!original;
  originalBadge = show ? (on ? "Original" : "Loading Original") : null;
  showBadge();
  const button = $("original");
  button.disabled = !can;
  button.setAttribute("aria-pressed", on);
  if (on !== showingOriginal) {
    showingOriginal = on;
    showView();
  }
  // After the switch's display pass, which the engine then draws first.
  if (show && original?.side !== settledSide) prepareOriginal();
}

// A new photograph or engine has none.
function forgetOriginal() {
  holding = false;
  invalidateOriginal();
}
// Nor does a photograph developed again (at another RAW white balance):
// the one made goes, and one still being made is dropped when it comes. One
// held is made again from the new development.
function invalidateOriginal() {
  originals++;
  originalLoad = null;
  original?.bitmap?.close();
  original = null;
  updateOriginal();
}

// macOS EditorModel.percent.
const percent = (f) => `${(f * 100).toFixed(f < 0.1 ? 1 : 0)}%`;

function zoomLevel() {
  $("zoom-level").textContent = percent(view.zoom());
  $("fit").disabled = view.atFit();
}

// As desktop's status line: the photograph's size, and the share of its
// pixels the frame shows when it has fewer (exports develop all of them, or
// on a phone or tablet the copy it keeps).
function hud(r, interactive) {
  lastHud = [r, interactive];
  const size = photoSize(), el = $("hud");
  const ms = `${r.elapsedMs.toFixed(0)} ms`;
  const shown = Math.max(r.width, r.height) / Math.max(photo.width, photo.height);
  const parts = interactive
    ? ["Preview", `${r.width} × ${r.height}`, ms]
    : [`${size.width} × ${size.height}`, ...(shown < 1 ? [`shown at ${percent(shown)}`] : []), ms, `${r.memoryMiB} MiB`];
  // Each part on one line; the line breaks between them.
  el.textContent = parts.map((p) => p.replaceAll(" ", "\u00a0")).join("\u00a0· ");
  el.title = shown < 1 && !interactive
    ? `The preview shows ${percent(shown)} of these pixels. ${exportNote()}; inspect grain in an exported file.\n${r.stages}`
    : r.stages;
}

// ---- RAW white balance ---------------------------------------------------------

// A RAW's input white balance (io.raw_white_balance) is in the engine's
// decoded pixels, developed by spektrafilm-io from the original bytes; the
// film render can't change it afterwards. So `photo.wb` is the WB the
// engine's pixels were developed at, the edit's is the one wanted, and a RAW
// whose two differ develops again from `photo.bytes`, never from the last
// development. Crop, rotation, view, border, sample and every other setting
// stay as they are: this isn't an opening.
//
// Everything that replaces the engine's photograph goes through `source`, one
// at a time: an opening, a development, a reload after an engine restart.
// Each development checks it's still for this photograph, at the WB still
// wanted, on the same engine before its pixels are installed (or, where the
// engine decodes them itself, records what it installed), so a stale one never
// installs or is taken for the WB shown. A failed one is reported, the edit's
// WB returns to the photograph's (nothing else of it), and the
// development stays what it was; trying again may succeed.
//
// Programmatic changes (Reset, `negs`, a preset) take the same path: change
// state.params, then `settingsChanged()`; `sourceReady()` resolves once the
// engine's photograph matches the edit's WB, or rejects if that failed.
const source = createSourceQueue();
const sourceOp = (operation) => source.run(operation).finally(() => { if (wanted && !source.busy) pump(); });
const wantedWb = () => structuredClone(get(RAW_WB_PATH) ?? defaultValue(RAW_WB_PATH) ?? { mode: "as_shot" });
// What a load says the photograph was read as, and the WB it developed at.
function developedAs(size, wb) {
  let colour = null;
  try { colour = size.colour ? JSON.parse(size.colour) : null; } catch {}
  return { colour, raw: isRawColour(colour), wb: structuredClone(wb) };
}
const needsDevelop = () => !!photo?.raw && !sameWb(wantedWb(), photo.wb);
// The engine has the photograph the edit develops: nothing replacing it or
// about to.
const sourceCurrent = () => !!photo && !loading && !source.busy && !needsDevelop();
let developTimer = 0, developing = false, developFailures = 0;

// A change asks for a development after a short pause, so a burst of them (a
// typed value, Reset's) is one; a slider's interactive frames wait for it to
// settle (or let go), as a RAW develops in a second or more.
function requestDevelop(mode) {
  clearTimeout(developTimer);
  if (mode === "interactive" || !needsDevelop()) return;
  developTimer = setTimeout(redevelop, 150);
}

// Develops the photograph at the edit's WB until the two agree: one at a
// time, the newest WB next.
async function redevelop() {
  clearTimeout(developTimer);
  if (developing) return;
  developing = true;
  try {
    while (photo && !loading && needsDevelop()) {
      const target = photo, wb = wantedWb(), opening = opens, engine = host;
      const current = () => photo === target && opening === opens;
      setBadge("Developing");
      $("busy").classList.add("on");
      try {
        const outcome = await sourceOp(async () => {
          if (!current() || !sameWb(wb, wantedWb())) return "stale";
          try {
            const size = await loadPhoto(target.bytes, wb, { stale: () => !current() || host !== engine || !sameWb(wb, wantedWb()) });
            // The engine has it, even if a newer WB or photograph is wanted
            // by now: that one replaces it next.
            if (!current()) return "stale";
            Object.assign(target, developedAs(size, wb));
            return "developed";
          } catch (e) {
            if (e.stale) return "stale";
            if (e.restarted) return "restarted";
            // The engine's own load let the last development go: back to it.
            if (e.forgot && current()) await loadPhoto(target.bytes, target.wb).catch((again) => console.error(again));
            throw e;
          }
        });
        if (outcome === "restarted") break; // The new engine reloads it, then develops.
        if (outcome === "developed") {
          console.log(`developed ${target.name} again at RAW white balance ${JSON.stringify(wb)}`);
          invalidateOriginal();
          schedule("settled");
        }
      } catch (e) {
        developFailures++;
        // The failed WB, only, returns to the one developed; a newer one is
        // tried next.
        if (photo === target && sameWb(wantedWb(), wb)) set(RAW_WB_PATH, structuredClone(target.wb));
        sync();
        setBadge(null);
        const what = "The RAW white balance couldn't be applied";
        // Shown for a while: the frames that follow (of the development
        // restored) leave it up. A crash restarts the engine, as for a render.
        if (e.crashed) await failed(e, what);
        else {
          console.error(e);
          showMessage(`${what}: ${e.message}`, true, 10_000);
        }
      }
    }
  } finally {
    developing = false;
    if (activity === "Developing" && !loading && !running && !wanted) setBadge(null);
    if (!running && !loading) $("busy").classList.remove("on");
    if (wanted) pump();
  }
}

// Resolves once the engine's photograph is the edit's (its RAW at the edit's
// WB); rejects if a development fails meanwhile.
async function sourceReady() {
  const failures = developFailures;
  for (;;) {
    if (developFailures !== failures) throw new Error("the RAW white balance couldn't be applied");
    if (sourceCurrent()) return;
    if (!photo) throw new Error("no photograph is open");
    if (needsDevelop() && !developing && !loading) redevelop();
    await new Promise((r) => setTimeout(r, 25));
  }
}

// After anything changes the edit outside its controls: the inspector shows
// it, and it renders (and develops again, if its RAW WB changed).
function settingsChanged(mode = "settled") {
  sync();
  schedule(mode);
}

// ---- Prep › Match camera brightness ----------------------------------------

// The macOS model (EditorModel §Camera brightness), for camera RAWs: the
// reference is the RAW's embedded camera preview (spektrafilm-io
// brightness.rs, in a short-lived module), the solve the engine's
// `placeExposure`. New RAWs start with the switch as it was last set.
const matchNewRaws = () => remembered("matchNewRaws", true);
const newMatch = () => (!photo || photo.raw) && matchNewRaws() ? { ev: null, inputs: null, limited: false } : null;
// A failed match, for `photo`: for the photograph (`inputs` null, the
// reference is the problem) or until these inputs change.
let matchFailure = null;
let matching = null;   // the solve running: { inputs }
let lastSolve = null;  // how long the last took: { solveMs, totalMs, ev }
// What the match depends on: everything that changes how bright the
// developed photograph is, except the brightness edits that are the user's
// own (Prep, and the enlarger's print exposure) and settings that can't
// change it. A change re-solves the match; Prep edits never do.
const MATCH_IGNORES = ["prep", "enlarger.print_exposure", "post", "film_render.grain", "io.rotate_cw", "io.crop",
  "io.crop_center", "io.crop_size", "io.output_color_space", "io.output_cctf_encoding", "settings.serial_grain"];
function matchInputs() {
  const p = structuredClone(state.params);
  for (const path of MATCH_IGNORES) set(path, null, p);
  return JSON.stringify({ film: state.film, paper: state.paper, params: p });
}
const matchUnavailable = () => matchFailure?.photo === photo && matchFailure.inputs === null ? matchFailure.message : null;
const matchFailedHere = () => matchFailure?.photo === photo && matchFailure.inputs !== null && matchFailure.inputs === matchInputs();
// On, possible, and not failed for the current settings.
const matchActive = () => !!state.match && !!photo?.raw && !matchUnavailable() && !matchFailedHere();
// On and not yet solved for the current settings. Export waits.
const matchPending = () => matchActive() && (state.match.ev === null || state.match.inputs !== matchInputs());
const matchedEv = () => (matchActive() ? state.match.ev ?? 0 : 0);
// The note under the Exposure slider.
function matchNote() {
  if (!state.match || !photo?.raw) return null;
  if (matchUnavailable()) return matchUnavailable();
  if (matchFailedHere()) return matchFailure.message;
  if (matchPending() || matching) return "Matching…";
  const ev = state.match.ev;
  if (ev === null) return null;
  const value = `${ev >= 0 ? "+" : "−"}${Math.abs(ev).toFixed(2)}`;
  return state.match.limited ? `Starts at ${value} EV to match the camera, held back to protect highlights.` : `Starts at ${value} EV to match the camera.`;
}

// Turns matching on (from the match, Exposure back to 0) or off (the
// brightness stays; Exposure then shows all of it).
function setMatch(on) {
  remember("matchNewRaws", on);
  if (on === !!state.match) return;
  if (on) {
    state.match = { ev: null, inputs: null, limited: false };
    set("prep.exposure_ev", 0);
  } else {
    state.match = null;
  }
  settingsChanged();
}

// The reference for `target`, made once per photograph by a module that
// then ends, so the RAW's bytes and preview decode leave its memory.
function cameraReference(target) {
  target.reference ??= new Promise((resolve, reject) => {
    const worker = startDecoder();
    worker.onmessage = ({ data: { result, error } }) => error ? reject(new Error(error)) : resolve(result);
    worker.onerror = (ev) => reject(new Error(ev.message || "the camera preview couldn't be read"));
    // It ends after its one answer (web/decoder.js).
    worker.postMessage({ bytes: target.bytes, reference: { width: target.width, height: target.height } });
  });
  return target.reference;
}

// Solves the match when it's on and stale, once the frame of the current
// settings has shown (`force`: for an export, whatever shows). One solve at
// a time; its end looks again, so edits made meanwhile coalesce. A result
// applies only if the inputs it was solved for are still current.
async function reconcileMatch({ force = false } = {}) {
  if (matching || previewing || !matchPending() || !sourceCurrent() || !host || cropSession) return;
  if (!force && (running || wanted || exporting || !developed || renders.at(-1)?.interactive !== false)) return;
  const inputs = matchInputs(), target = photo, opening = opens, engine = host;
  const stale = () => photo !== target || opening !== opens || host !== engine;
  const p = structuredClone(state.params);
  set("prep", structuredClone(defaultValue("prep")), p);
  set("enlarger.print_exposure", defaultValue("enlarger.print_exposure"), p);
  const request = JSON.stringify({ ...p, io: { ...p.io, rotate_cw: 0 } });
  matching = { inputs };
  sync();
  const t = performance.now();
  try {
    let reference;
    try {
      reference = await cameraReference(target);
    } catch (e) {
      const unusable = /^UNUSABLE: /.test(e.message);
      throw Object.assign(new Error(unusable ? e.message.slice(10) : "The camera preview couldn’t be read."), { photograph: true, cause: e });
    }
    if (stale() || running) return;
    matching.engine = true;
    const result = JSON.parse(await engine.call("placeExposure", state.film, state.paper, request, reference.pixels, reference.width, reference.height));
    lastSolve = { solveMs: Math.round(result.elapsed_ms), totalMs: Math.round(performance.now() - t), ev: result.exposure_ev };
    console.log(`matched camera brightness: ${result.exposure_ev.toFixed(3)} EV${result.highlight_limited ? " (highlight-limited)" : ""} in ${result.elapsed_ms.toFixed(0)} ms (${(performance.now() - t).toFixed(0)} ms with the reference)`);
    if (stale() || !state.match || matchInputs() !== inputs) return;
    if (!Number.isFinite(result.exposure_ev) || Math.abs(result.exposure_ev) > 3) throw new Error("No reliable brightness match with these settings.");
    applyMatch(result.exposure_ev, !!result.highlight_limited, inputs);
  } catch (e) {
    if (stale() || e.restarted || matchInputs() !== inputs) return;
    console.warn(`no camera brightness match: ${e.cause?.message ?? e.message}`);
    let message = e.message, photograph = !!e.photograph;
    if (!photograph) [message, photograph] = await engine.call("brightnessFailure", e.message).then(([m, scope]) => [m, scope === "photograph"]).catch(() => ["No reliable brightness match with these settings.", false]);
    matchFailure = { photo: target, inputs: photograph ? null : inputs, message, reason: e.cause?.message ?? e.message };
    sync();
    schedule("settled");
  } finally {
    matching = null;
    sync();
    if (wanted) pump();
    queueMicrotask(() => { reconcileMatch(); pumpPreviews(); });
  }
}

// Moves the total with the match, keeping the user's exposure above it. A
// change under 0.05 EV keeps the values and only marks them current.
function applyMatch(ev, limited, inputs) {
  const match = state.match;
  if (!match || matchInputs() !== inputs) return;
  const offset = get("prep.exposure_ev") - (match.ev ?? 0);
  if (match.ev === null || Math.abs(ev - match.ev) >= 0.05) {
    match.ev = ev;
    set("prep.exposure_ev", Math.min(3, Math.max(-3, Math.abs(offset) < 1e-9 ? ev : ev + offset)));
  }
  match.inputs = inputs;
  match.limited = limited;
  settingsChanged();
}

// Until a pending match is solved, for an export: it renders what Match
// camera brightness would show.
async function matchSettled() {
  for (let i = 0; i < 1200 && (matchPending() || matching); i++) {
    reconcileMatch({ force: true });
    await new Promise((r) => setTimeout(r, 25));
  }
}

// ---- Film and paper previews ----------------------------------------------

// The foldouts of web/stocks.js beside the menus, one tile request at a
// time and only while the photograph has nothing else to render: no frame
// running or wanted, no slider moving, no brightness match or export
// (docs/stock-previews.md). Off: the menus, and the engine lets go of what
// the previews kept.
const PREVIEW_SIDE = 320;
// The calibrated stocks the engine keeps for them: every film with its
// paper, and every paper with the current film.
const PREVIEW_PIPELINES = 40;
// On by default: on the iPhone 13 Pro filled grids hold ~100 MiB of the tab,
// and neither an export nor opening another photograph with them came near
// the tab's limit (docs/web-prep-port-plan.md). A memory crash turns them
// off (`crashCheck`).
let previewsOn = remembered("stockPreviews", true);
let previewing = false, previewTimer = 0, previewRefresh = 0;
const previewStats = [];
let folds = null;

function buildPreviews() {
  const opened = remembered("stockFolds", {});
  const fold = (which, title, groups, choose, search) => {
    const f = Stocks.foldout(title, groups, (id) => { choose(id); sync(); schedule("settled"); }, refreshPreviews, !!opened[which],
      favorites(which), (ids) => { remember(favoritesKey(which), ids); fillFavorites(which); }, search);
    f.root.addEventListener("toggle", () => remember("stockFolds", { ...remembered("stockFolds", {}), [which]: f.root.open }));
    return f;
  };
  // One grid open at a time: opening one folds the other (and a remembered
  // pair opens on the film's).
  if (opened.film && opened.paper) opened.paper = false;
  folds = { film: fold("film", "Film", filmGroups(), chooseFilm, "Search films"), paper: fold("paper", "Paper", paperGroups(), choosePaper) };
  for (const [one, other] of [[folds.film, folds.paper], [folds.paper, folds.film]]) {
    one.root.addEventListener("toggle", () => { if (one.root.open) other.root.open = false; });
  }
  folds.film.root.querySelector(".stock-title").after(info("Film", Help.FILM));
  folds.paper.root.querySelector(".stock-title").after(info("Paper", Help.PAPER));
  $("film").closest("label").after(folds.film.root);
  $("paper-row").after(folds.paper.root);
  $("previews").addEventListener("change", () => setPreviews($("previews").checked));
  syncs.push(() => {
    // Scanned film isn't printed: no paper. Until a photograph is open the
    // tiles would be blank, so the menus show.
    const tiles = previewsOn && !!photo;
    $("film").closest("label").hidden = tiles;
    $("paper-row").hidden = tiles || get("io.scan_film");
    folds.film.root.hidden = !tiles;
    folds.paper.root.hidden = !tiles || get("io.scan_film");
    folds.film.select(state.film);
    folds.paper.select(state.paper);
    $("previews").checked = previewsOn;
    refreshPreviews();
  });
}

function setPreviews(on) {
  previewsOn = on;
  remember("stockPreviews", on);
  markAlive();
  if (!on) {
    for (const t of [...folds.film.tiles.values(), ...folds.paper.tiles.values()]) Stocks.markStale(t, null, null);
    host?.call("releaseThumbnails").catch(() => {});
  }
  sync();
}

// ---- Memory crashes ------------------------------------------------------------

// A tab iOS kills for memory gets no pagehide, and Safari loads it again
// at once (as a "navigate", not a "reload": iPhone, iOS 26). So while the
// page is shown it keeps a marker with a heartbeat, which leaving or hiding
// it (another app, another tab, a reload) clears; a marker still there and
// fresh at load means the last page was killed moments ago. A tab
// discarded in the background was hidden first, and a browser quit is
// rarely opened again within the minute.
const ALIVE = "alive";
const lastLife = remembered(ALIVE, null);
function markAlive() {
  if (document.visibilityState === "visible") remember(ALIVE, { previews: previewsOn, at: Date.now() });
}
setInterval(markAlive, 5000);
const markGone = () => { try { localStorage.removeItem(`negs.${ALIVE}`); } catch {} };
addEventListener("visibilitychange", () => (document.visibilityState === "visible" ? markAlive() : markGone()));
addEventListener("pagehide", markGone);
addEventListener("pageshow", markAlive);
markAlive();

// After a crash with the previews on, they go off (the tiles' memory is the
// part of the tab that can go), and a note says so.
function crashCheck() {
  const fresh = Date.now() - (lastLife?.at ?? 0) < 60_000;
  if (!lastLife?.previews || !fresh || !previewsOn) return;
  setPreviews(false);
  showMessage("Negs ran out of memory and reloaded. Film and paper previews are now off to save memory.", false, 20_000,
    { label: "Settings", run: () => openSettings() });
}

// ---- Settings ------------------------------------------------------------------

// The gear's sheet: the previews (above), new RAWs' brightness match (the
// Prep switch also sets it) and the engine (its change handler is below).
function openSettings() {
  $("previews").checked = previewsOn;
  $("match-new-raws").checked = matchNewRaws();
  $("advanced").checked = advanced;
  $("settings-dialog").showModal();
}
$("advanced").addEventListener("change", () => {
  advanced = $("advanced").checked;
  remember("advanced", advanced);
  forgetRevealed();
});
$("settings").addEventListener("click", openSettings);
$("match-new-raws").addEventListener("change", () => remember("matchNewRaws", $("match-new-raws").checked));

// What tile `id` of `which` renders: the edit with that stock chosen.
function tileRequest(which, id) {
  const into = { film: state.film, paper: state.paper, params: structuredClone(state.params) };
  (which === "film" ? chooseFilm : choosePaper)(id, into);
  const request = { film: into.film, paper: into.paper, params: JSON.stringify({ ...into.params, io: { ...into.params.io, rotate_cw: 0 } }) };
  return { ...request, key: JSON.stringify([request, crop, rotation()]) };
}

// The tiles on screen, each with its request and whether its picture is
// current; stale ones dimmed. Once per frame, however many changes.
function previewTiles() {
  if (!previewsOn || !folds || !photo) return [];
  const out = [];
  for (const which of ["film", "paper"]) {
    for (const tile of folds[which].onScreen()) {
      const request = tileRequest(which, tile.id);
      Stocks.markStale(tile, photo, request.key);
      out.push({ tile, request });
    }
  }
  return out;
}
function refreshPreviews() {
  if (previewRefresh) return;
  previewRefresh = requestAnimationFrame(() => {
    previewRefresh = 0;
    pumpPreviews();
  });
}

// Renders the next tile on screen whose picture isn't current, then looks
// again.
async function pumpPreviews() {
  clearTimeout(previewTimer);
  const tiles = previewTiles();
  if (previewing || !photo || !host || !developed || loading || running || wanted || exporting || matching || cropSession || !sourceCurrent()) return;
  const quiet = performance.now() - lastSlide;
  if (quiet < 400) {
    previewTimer = setTimeout(pumpPreviews, 400 - quiet);
    return;
  }
  const next = tiles.find(({ tile, request }) => tile.key !== request.key && tile.failed !== request.key);
  if (!next) return;
  const { tile, request } = next, target = photo, engine = host;
  previewing = true;
  try {
    const side = Stocks.frameSide(photo.width, photo.height, crop, PREVIEW_SIDE);
    const r = await engine.call("thumbnail", request.film, request.paper, request.params, side, PREVIEW_PIPELINES, structuredClone(crop), rotation());
    previewStats.push({ id: tile.id, elapsedMs: r.elapsedMs, stages: r.stages, memoryMiB: r.memoryMiB, at: performance.now() });
    if (photo === target && previewsOn) Stocks.paint(tile, target, request.key, r);
  } catch (e) {
    if (!e.restarted) console.warn(`no preview of ${tile.id}: ${e.message}`);
    tile.failed = request.key;
  } finally {
    previewing = false;
  }
  if (wanted) pump();
  reconcileMatch();
  refreshPreviews();
}

// ---- Tutorial adapter ------------------------------------------------------

// Polling is only used at session boundaries, not on slider interaction.
// Wait for presentation, not just RAW decode or scheduling a render.
async function tourWait(test, signal, failures = renderFailures) {
  const deadline = performance.now() + 120000;
  for (;;) {
    signal?.throwIfAborted();
    if (renderFailures !== failures) throw new Error("The sample couldn't be rendered. Try again or return to the editor.");
    if (test()) return;
    if (performance.now() > deadline) throw new Error("The photo took too long to develop. Try again or return to the editor.");
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
}
const tourIdle = () => !loading && !running && !wanted && !source.busy && !developing && !matching;
const layoutSettled = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

function tutorialAdapter() {
  let sampleShown = false;
  function baseline() {
    state.params = structuredClone(catalog.defaults);
    // The steps show what each control does, so nothing re-solves under them.
    state.match = null;
    state.paper = "kodak_portra_endura";
    chooseFilm("kodak_portra_400");
    crop = Crop.defaults();
    Object.assign(border, Border.DEFAULT_BORDER);
    holdOriginal(false);
  }
  function reveal(step) {
    for (const fold of Object.values(folds)) fold.root.open = false;
    for (const node of document.querySelectorAll(".tour-control, .tour-group, .tour-section, .tour-stock")) {
      node.classList.remove("tour-control", "tour-group", "tour-section", "tour-stock");
    }
    $("app").dataset.tourStep = step.id;
    showPane(PANES.includes(step.tab) ? step.tab : "lab");
    showTab(step.tab);
    $("preset-menu").hidden = true;
    $("more").setAttribute("aria-expanded", "false");
    // The film and paper steps: the menu, or with previews its grid, opened.
    if (step.target === "film" || step.target === "paper") {
      const fold = folds[step.target].root;
      if (fold.hidden) $(step.target).closest("label").classList.add("tour-control");
      else { fold.classList.add("tour-control"); fold.open = true; }
      $(step.target).closest(".stock").classList.add("tour-stock");
    }
    for (const path of step.paths ?? []) {
      const row = [...document.querySelectorAll("[data-tutorial-path]")].find((n) => n.dataset.tutorialPath === path);
      if (!row) throw new Error(`Missing tutorial control: ${path}`);
      row.classList.add("tour-control");
      row.closest(".section").classList.add("tour-section");
    }
    if (step.group) {
      const group = [...document.querySelectorAll("[data-tutorial-group]")].find((n) => n.dataset.tutorialGroup === step.group);
      if (!group) throw new Error(`Missing tutorial group: ${step.group}`);
      group.classList.add("tour-group");
      group.open = true;
    }
    $("scroll").scrollTop = 0;
    setSheet(sheet);
  }
  return {
    read: () => remembered("tutorial", null),
    remember: (value) => remember("tutorial", value),
    hasPhoto: () => !!photo,
    launchIntent: () => new URLSearchParams(location.hash.slice(1)).has("preset") || query.has("image") || query.has("film") || query.has("paper"),
    unavailable: () => cropSession ? "Finish or cancel your crop before starting the tutorial."
      : loading || exporting || presets.busy() || dialogOpen() ? "Finish the current operation before starting the tutorial." : null,
    message: showMessage,
    resize: () => setSheet(sheet),
    capture() {
      return { edit: structuredClone(state), photo, crop: structuredClone(crop), border: { ...border }, view: view.snapshot(),
        pane: $("side").dataset.pane, tab: $("side").dataset.tab, sheet, scroll: $("scroll").scrollTop,
        groups: [...document.querySelectorAll("[data-tutorial-group]")].map((node) => [node, node.open]),
        folds: Object.values(folds).map((fold) => [fold.root, fold.root.open]) };
    },
    async enter(signal) {
      tutorialPresentation = true;
      sampleShown = false;
      setSheet("mid");
      await starting;
      signal.throwIfAborted();
      await host.ready;
      await tourWait(tourIdle, signal);
      baseline();
      sync();
      samplePicker.cancel();
      setBadge("Downloading");
      const first = renders.length;
      const data = download(DEFAULT_SAMPLE.url, signal, (fraction) => !signal.aborted && setBadge(`Downloading ${Math.floor(fraction * 100)}%`));
      const opened = await open("Sample 1.dng", data, DEFAULT_SAMPLE.id);
      signal.throwIfAborted();
      if (!opened) throw new Error("Couldn't open the sample. Check your connection, or save Sample 1 for offline use.");
      await tourWait(() => developed && renders.length > first && tourIdle(), signal);
      sampleShown = true;
    },
    async step(step, signal) {
      await tourWait(tourIdle, signal);
      baseline();
      // A step can start with an effect on, to show it.
      for (const [path, value] of Object.entries(step.set ?? {})) set(path, value);
      sync(); // reveal() reads what the baseline shows (the stock grids).
      reveal(step);
      sync();
      view.fit();
      await layoutSettled();
      signal.throwIfAborted();
      const first = renders.length;
      settingsChanged();
      await tourWait(() => developed && renders.length > first && tourIdle(), signal);
    },
    cancel() {
      samplePicker.cancel();
      ++opens; // Any sample open/decode still in progress is now stale.
      loading = false;
    },
    async restore(saved, { completed = false } = {}) {
      if (!saved) return;
      await tourWait(tourIdle);
      tutorialRestoring = true;
      tutorialPresentation = false;
      $("app").classList.add("tutorial-restoring");
      $("app").classList.remove("tutorial-mode", "tutorial-busy");
      delete $("app").dataset.tourStep;
      $("tutorial-guide").hidden = true;
      $("tutorial-heading").hidden = true;
      $("tutorial-nav").hidden = true;
      $("preset-menu").hidden = true;
      $("more").setAttribute("aria-expanded", "false");
      for (const [node, opened] of [...saved.groups, ...saved.folds]) node.open = opened;
      showPane(saved.pane);
      showTab(saved.tab);
      setSheet(saved.sheet);
      if (saved.photo) {
        Object.assign(state, structuredClone(saved.edit));
        const first = renders.length;
        if (!await open(saved.photo.name, Promise.resolve(saved.photo.bytes), saved.photo.sampleId)) throw new Error("Couldn't reopen your photograph. Please retry.");
        await tourWait(() => developed && renders.length > first && tourIdle());
        // open() resets finishing geometry; put the user's geometry back.
        Object.assign(state, structuredClone(saved.edit));
        crop = structuredClone(saved.crop);
        Object.assign(border, saved.border);
        presentationChanged();
        await layoutSettled();
        view.restore(saved.view);
        sync();
        const restored = renders.length;
        settingsChanged();
        await tourWait(() => renders.length > restored && tourIdle());
      } else if (!sampleShown || completed) {
        Object.assign(state, structuredClone(saved.edit));
        crop = structuredClone(saved.crop);
        Object.assign(border, saved.border);
        dropEarly();
        photo = null;
        frame?.bitmap.close();
        frame = null;
        wanted = null;
        developed = false;
        forgetOriginal();
        for (const canvas of document.querySelectorAll(".photo")) canvas.hidden = true;
        $("empty").hidden = false;
        $("zoom").hidden = true;
        $("edit-tools").hidden = true;
        $("export").disabled = true;
        $("hud").textContent = "";
        setBadge(null);
        sync();
      } else {
        // First-run users can carry on experimenting with the sample.
        view.fit();
        sync();
      }
      await layoutSettled(); // Disclosure toggle events must stay transient.
      $("scroll").scrollTop = saved.scroll;
      tutorialRestoring = false;
      $("app").classList.remove("tutorial-restoring");
      samplePicker.sync();
      presets.sync();
    },
    completed: () => showMessage("You’re ready. Film and paper shape the look; Prep’s Exposure sets the brightness. Reopen Tutorial whenever you like.", false, 8000),
  };
}

// ---- Opening and exporting -------------------------------------------------

// What an export of the photograph develops.
const exportNote = () => photo.exportWidth < photo.width
  ? `Exports here develop ${photo.exportWidth} × ${photo.exportHeight}; a desktop browser exports all of them`
  : "Exports develop all of them";

// Opens `name`, whose bytes `data` resolves to. Until its first frame, a
// placeholder from the bytes shows.
let opens = 0, loading = false, starting = null;
async function open(name, data, sampleId = null) {
  endCrop(false);
  const opening = ++opens, previous = photo;
  let loaded = false;
  revealed.clear();
  $("edit-tools").hidden = true;
  loading = true;
  forgetOriginal();
  // Another photograph's placeholder isn't this one's.
  dropEarly(true);
  $("busy").classList.add("on");
  const t = performance.now();
  mark("open", name);
  data.then((bytes) => placeholder(bytes, longSide())).then((p) => {
    if (!p) return;
    if (loaded || opening !== opens) return p.bitmap.close();
    showEarly(p, opening);
    mark("placeholder", name);
    console.log(`placeholder for ${name} ${p.bitmap.width}×${p.bitmap.height} after ${(performance.now() - t).toFixed(0)} ms`);
  }, (e) => console.warn(`No placeholder for ${name}: ${e.message}`));
  try {
    const bytes = await data;
    if (opening !== opens) return;
    setBadge("Developing");
    const t = performance.now();
    await starting;
    // After any other change of the engine's photograph, at the edit's RAW
    // white balance then. Switching engines restarts the worker under a
    // load: load again.
    const { size, wb } = await sourceOp(async () => {
      for (;;) {
        try {
          await host.ready;
          const wb = wantedWb();
          return { size: await loadPhoto(bytes, wb, { stale: () => opening !== opens }), wb };
        } catch (e) {
          if (!e.restarted) throw e;
        }
      }
    });
    loaded = true;
    mark("decoded", name);
    // A newer photograph is opening; the engine loads it next.
    if (opening !== opens) return;
    // Its size as decoded, whatever the engine keeps.
    // The exposure set above the last photograph's match carries over; the
    // match is solved anew (macOS EditorModel.open).
    const offset = (get("prep.exposure_ev") ?? 0) - (state.match?.ev ?? 0);
    photo = { ...opened(name, bytes, size), sampleId, ...developedAs(size, wb) };
    state.match = newMatch();
    set("prep.exposure_ev", Math.min(3, Math.max(-3, offset)));
    crop = Crop.defaults();
    set("io.rotate_cw", 0);
    console.log(`decoded ${name} ${size.width}×${size.height} in ${(performance.now() - t).toFixed(0)} ms`);
    $("export").disabled = false;
    $("export").title = photo.exportWidth < photo.width ? `${exportNote()}.` : "";
    // Nothing of the last photograph shows; the engine forgot its frame.
    frame?.bitmap.close();
    frame = null;
    developed = false;
    lastHud = null;
    settledSide = 0;
    if (early) {
      // The placeholder stays until the first frame, where it was.
      view.resize(photo.width, photo.height);
    } else {
      for (const canvas of document.querySelectorAll(".photo")) canvas.hidden = true;
      view.setPhoto(photo.width, photo.height);
    }
    setSheet(sheet);
    loading = false;
    sync();
    schedule("settled");
    return true;
  } catch (e) {
    loaded = true;
    if (opening !== opens || e.name === "AbortError" || e.stale) return;
    setBadge(null);
    photo = previous;
    dropEarly(true);
    // The engine let the last photograph go before this one's decode.
    if (photo) await reloadPhoto().catch(() => {});
    $("empty").hidden = !!photo;
    await failed(e, `Couldn't open ${name}`);
    return false;
  } finally {
    if (opening === opens) loading = false;
    if (!running) $("busy").classList.remove("on");
    if (!loading && wanted) pump();
    updateOriginal();
    $("edit-tools").hidden = !photo || !developed;
    samplePicker.sync();
    presets.sync();
    // The WB changed while it opened.
    if (!loading && needsDevelop()) redevelop();
  }
}

// The longest side a placeholder needs: the stage's in device pixels, or
// before it's measured, the window's, which it never exceeds.
function longSide() {
  const { width, height } = view.size();
  return width > 1 ? Math.max(width, height) : Math.round(Math.max(innerWidth, innerHeight) * devicePixelRatio);
}

// A photograph the viewer chose; it replaces the sample if that's still
// downloading.
async function openFile(file) {
  tutorial?.suppressInvitation();
  if (tutorial && tutorial.state().phase !== "idle") await tutorial.end();
  samplePicker.cancel();
  return open(file.name, file.arrayBuffer().then((b) => new Uint8Array(b)));
}

// ---- Sample photographs ------------------------------------------------------

// A single click starts the default immediately. The picker loads its small
// catalog only after that click; no other RAWs or thumbnails are prefetched.
const samplePicker = createSamplePicker({
  container: $("samples"), select: $("sample-select"), next: $("sample-next"), note: $("sample-offline"),
  current: () => photo?.sampleId,
  onError: (e) => showMessage(`Couldn't load sample choices: ${e.message}`, true),
  load: (entry, number, signal) => {
    setBadge("Downloading");
    $("empty").hidden = true;
    const opening = opens + 1;
    const data = download(entry.url, signal, (f) => opens === opening && setBadge(`Downloading ${Math.floor(f * 100)}%`));
    return open(`Sample ${number}.dng`, data, entry.id);
  },
});
const openSample = async () => {
  tutorial?.suppressInvitation();
  if (tutorial && tutorial.state().phase !== "idle") await tutorial.end();
  return samplePicker.open();
};

// ---- Presets ---------------------------------------------------------------------

// The ⋯ menu's Import and Export preset, and incoming preset links
// (web/preset-ui.js). Applying replaces the film, paper and params at once,
// as desktop applies a preset (no film pairing), through the same change path
// as Reset, so a RAW develops again at the preset's white balance.
const presets = createPresets({
  ready: () => !!catalog.profiles.length && !!host && !loading && !exporting && !cropSession,
  engineReady: () => starting,
  async beforeApply() {
    tutorial?.suppressInvitation();
    if (tutorial && tutorial.state().phase !== "idle") await tutorial.end();
  },
  call: (method, ...args) => host.call(method, ...args),
  state: () => structuredClone(state),
  hasPhoto: () => !!photo,
  stockName: (id) => stock(id)?.name,
  install({ film, paper, params }) {
    Object.assign(state, { film, paper, params });
    settingsChanged();
  },
  // Sample 1, whichever photograph or sample is open; nothing if it is.
  openSample: () => samplePicker.open(DEFAULT_SAMPLE.id),
  developed: () => (photo?.raw ? sourceReady() : Promise.resolve()),
  cropping: () => !!cropSession,
  shareFiles: () => IOS && standalone(),
  message: (text, error, ms) => showMessage(text, error, ms),
});
for (const event of ["online", "offline"]) addEventListener(event, () => samplePicker.sync());

// `url`'s bytes, telling `progress` the fraction so far when the server
// says the length.
async function download(url, signal, progress) {
  const res = await fetch(url, { signal });
  if (!res.ok) throw fetchFailed(url, res);
  const total = Number(res.headers.get("Content-Length")) || 0;
  if (!total || !res.body) return new Uint8Array(await res.arrayBuffer());
  const chunks = [];
  let received = 0;
  for (const reader = res.body.getReader(); ;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    progress(Math.min(1, received / total));
  }
  const bytes = new Uint8Array(received);
  let at = 0;
  for (const c of chunks) {
    bytes.set(c, at);
    at += c.length;
  }
  return bytes;
}

// A crash kills the engine's module: start a new worker, with the last
// photograph that opened.
async function failed(e, what) {
  if (e.restarted) return;
  if (e.message?.startsWith("BROWSER_RENDER_LIMIT:")) {
    setBadge(null);
    showMessage(e.message.slice("BROWSER_RENDER_LIMIT:".length).trim());
    return;
  }
  console.error(e);
  setBadge(null);
  if (!loading && early) {
    dropEarly();
    paint();
  }
  showMessage(`${what}: ${e.message}`, true);
  if (e.crashed) {
    // A WebGPU engine that crashed usually lost its device (out of GPU
    // memory, the GPU process restarting), and a new one the same way tends
    // to crash the same way on the next render. One that was presenting
    // starts again drawing read-back frames instead (see mayPresent); one
    // that wasn't gives way to the CPU engine, which the Engine menu can
    // switch back from. Either way the frame renders again.
    let next = kind, note = "";
    if (kind === "gpu" && presenting) {
      remember("presentCrashed", [...new Set([...remembered("presentCrashed", []), adapterKey()])]);
      console.warn(`Presenting from the WebGPU engine crashed on this GPU (${adapterKey()}); drawing its frames instead from now on.`);
      note = " Trying again without presenting from the GPU.";
    } else if (kind === "gpu") {
      next = "cpu";
      note = " Switched to the CPU engine.";
    }
    const message = note ? `${what}: ${e.message}.${note}` : $("message").textContent;
    await startEngine(next).catch(() => {});
    showMessage(message, true);
    if (note) schedule("settled");
  }
}

async function exportJpeg() {
  if (!photo || exporting || cropSession) return;
  exporting = true;
  const edit = editKey();
  $("export").disabled = true;
  showMessage(`Rendering ${photo.exportWidth}×${photo.exportHeight} for export…`);
  $("busy").classList.add("on");
  try {
    while (running || previewing) await new Promise((r) => setTimeout(r, 20));
    await matchSettled();
    const r = await exportRender();
    const canvas = new OffscreenCanvas(r.width, r.height);
    let blob;
    try {
      canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height), 0, 0);
      blob = await canvas.convertToBlob({ type: "image/jpeg", quality: 0.92 });
    } finally {
      // The JPEG owns its encoded bytes now. Release the full export-sized
      // backing store even if encoding failed, rather than wait for page GC.
      canvas.width = canvas.height = 1;
    }
    const name = `${photo.name.replace(/\.[^.]+$/, "")}-${state.film}.jpg`;
    const file = new File([blob], name, { type: "image/jpeg" });
    // A home-screen app on iOS has no downloads; the share sheet saves to
    // Photos or Files. Sharing needs a fresh tap, which the render outlasted.
    if (IOS && standalone() && navigator.canShare?.({ files: [file] })) {
      showMessage("Exported.", false, 60_000, { label: "Save or share…", run: () => navigator.share({ files: [file] }).catch(() => {}) });
    } else {
      const a = Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: name });
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
      hideMessage();
    }
    exportedEdit = edit;
  } catch (e) {
    await failed(e, "Export failed");
  } finally {
    exporting = false;
    $("export").disabled = !photo;
    $("busy").classList.remove("on");
    if (wanted) pump();
  }
}

// The export's bytes: the edit at full size, then its border, if it has one
// and includes it, as the CLI adds it (`addBorder`: spektrafilm-io's
// add_border on the bytes, in the engine's worker).
async function exportRender() {
  if (cropSession) throw new Error("Apply or cancel the crop before exporting");
  // Never the last development's pixels with a newly chosen RAW white
  // balance: wait for the matching one. The render is asked for in the same
  // turn as the check, so the engine renders it before any later change.
  do await sourceReady(); while (!sourceCurrent());
  const settings = structuredClone(crop), turn = rotation();
  const rendered = await host.call("render", state.film, state.paper, params(), Math.max(photo.exportWidth, photo.exportHeight), false);
  const r = { ...rendered, ...await host.call("finishPhoto", rendered.rgba, rendered.width, rendered.height, settings, turn) };
  const b = Border.exported(border, r.width, r.height);
  if (!b) return r;
  sendBorderTexture();
  const bordered = await host.call("addBorder", r.rgba, r.width, r.height, b.style, JSON.stringify(Border.options(b)), b.instagram, state.film);
  console.log(`border ${bordered.width}×${bordered.height} in ${bordered.elapsedMs.toFixed(0)} ms, ${bordered.memoryMiB} MiB`);
  return { ...bordered, unbordered: r };
}

// Loads the photograph again, after the engine let it go: nothing of it shows
// there until it renders again.
async function reloadPhoto() {
  await reloadSource();
  forgetOriginal();
  settledSide = 0;
  schedule("settled");
}
// The photograph again, at the WB its last development used, unless an
// opening photograph will replace it (`unlessOpening`).
function reloadSource({ unlessOpening = false } = {}) {
  return sourceOp(async () => {
    if (!photo || (unlessOpening && loading)) return;
    const p = photo;
    await loadPhoto(p.bytes, p.wb);
  });
}

// A message stays until hidden; one shown for `ms` hides itself then, and
// the renders that follow it leave it up. An `action` ({ label, run }) is a
// button after the text, which hides the message.
let messageTimer;
function showMessage(text, error = false, ms = 0, action = null) {
  const m = $("message");
  m.textContent = text;
  if (action) {
    m.append(" ", el("button", { className: "link", textContent: action.label, onclick: () => { m.hidden = true; action.run(); } }));
  }
  m.classList.toggle("error", error);
  m.hidden = false;
  clearTimeout(messageTimer);
  messageTimer = ms ? setTimeout(() => { messageTimer = null; m.hidden = true; }, ms) : null;
}
function hideMessage() {
  if (!messageTimer) $("message").hidden = true;
}

// The M/Y shifts that print a mid-grey neutral with the current film,
// bases, paper and enlarger, set as one change (macOS EditorModel.balancePrint).
let balancing = false;
async function balancePrint() {
  if (!actions.balancePrint.enabled() || cropSession) return;
  const film = state.film, paper = state.paper, edit = params();
  balancing = true;
  sync();
  try {
    const r = await host.call("balancePrint", film, paper, edit);
    console.log(`balance print: magenta ${r.m}, yellow ${r.y} in ${r.elapsedMs.toFixed(0)} ms`);
    set("enlarger.m_filter_shift", r.m);
    set("enlarger.y_filter_shift", r.y);
    showMessage(`Print balanced: magenta ${r.m.toFixed(2)}, yellow ${r.y.toFixed(2)}`, false, 4000);
    schedule("settled");
  } catch (e) {
    await failed(e, "Balance Print failed");
  } finally {
    balancing = false;
    sync();
  }
}

// ---- Installing, and working offline -----------------------------------------

// The service worker (web/sw.js) keeps Negs for offline use, and the
// footer says how much it keeps, offers to install the app, and tells of
// a new version. On a computer's own dev server (localhost) it registers
// only when asked, with ?sw=1 (remembered; ?sw=0 removes it), so checks
// and benchmarks never meet it; elsewhere ?sw=0 turns it off.
const ua = navigator.userAgent;
const IOS = /iP(hone|ad|od)/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1);
const SAFARI = /Safari\//.test(ua) && !/Chrome|Chromium|CriOS|FxiOS|EdgiOS|OPiOS|Android/.test(ua);
const standalone = () => navigator.standalone === true || matchMedia("(display-mode: standalone)").matches;
const mb = (bytes) => `${Math.max(1, Math.round(bytes / 2 ** 20))} MB`;

let registration = null;
async function startOffline() {
  if (!("serviceWorker" in navigator)) return;
  const local = ["localhost", "127.0.0.1", "[::1]"].includes(location.hostname);
  if (query.has("sw")) remember("sw", query.get("sw") === "1");
  if (!remembered("sw", !local)) {
    for (const r of await navigator.serviceWorker.getRegistrations()) await r.unregister();
    return;
  }
  // An installed app keeps what it saved when the browser needs room.
  if (standalone()) navigator.storage?.persist?.();
  let controlled = !!navigator.serviceWorker.controller, reloading = false;
  navigator.serviceWorker.addEventListener("controllerchange", () => {
    if (reloading) return location.reload();
    // Another window took the new version; this one keeps its own.
    if (controlled) updateReady("Negs was updated in another window.");
    controlled = true;
    offlineStatus();
  });
  new BroadcastChannel("negs-offline").onmessage = ({ data }) => {
    if (data.type === "failed") {
      $("offline-text").textContent = controlled ? "The new version couldn't be downloaded" : "Couldn't save Negs for offline use";
      $("offline-all").hidden = $("offline-clear").hidden = true;
      $("offline").hidden = false;
      return showMessage(`${$("offline-text").textContent} (${data.error}). It tries again the next time Negs opens.`, true, 8000);
    }
    if (data.type !== "progress") return offlineStatus();
    const what = data.task === "save-all" ? "Saving everything for offline use" : controlled ? "Downloading a new version" : "Saving Negs for offline use";
    $("offline-text").textContent = `${what}… ${data.total ? Math.floor((data.done / data.total) * 100) : 100}%`;
    $("offline-all").hidden = $("offline-clear").hidden = true;
    $("offline").hidden = false;
  };
  $("update-reload").addEventListener("click", () => {
    reloading = true;
    registration.waiting ? registration.waiting.postMessage({ type: "skip-waiting" }) : location.reload();
  });
  $("offline-all").addEventListener("click", async () => {
    navigator.storage?.persist?.();
    $("offline-all").disabled = true;
    const r = await ask("save-all");
    $("offline-all").disabled = false;
    if (r?.error) showMessage(`Not everything could be saved for offline use: ${r.error}`, true, 6000);
    offlineStatus();
  });
  $("offline-clear").addEventListener("click", clearOffline);
  try {
    registration = await navigator.serviceWorker.register("sw.js", { updateViaCache: "none" });
  } catch (e) {
    return console.warn(`Negs can't work offline here: ${e.message}`);
  }
  const watch = (w) => w?.addEventListener("statechange", () => w.state === "installed" && navigator.serviceWorker.controller && updateReady());
  if (registration.waiting && controlled) updateReady();
  watch(registration.installing);
  registration.addEventListener("updatefound", () => watch(registration.installing));
  // An app left open looks for a new version when it comes back, hourly.
  let checked = Date.now();
  document.addEventListener("visibilitychange", () => {
    if (document.hidden || Date.now() - checked < 3600e3) return;
    checked = Date.now();
    registration.update().catch(() => {});
  });
  offlineStatus();
}

// A question for the service worker, answered on a channel of its own.
function ask(type) {
  const worker = navigator.serviceWorker.controller ?? registration?.active;
  if (!worker) return Promise.resolve(null);
  const { port1, port2 } = new MessageChannel();
  const reply = new Promise((resolve) => (port1.onmessage = ({ data }) => resolve(data)));
  worker.postMessage({ type }, [port2]);
  return reply;
}

// What's saved: "Offline · 24 MB saved · Save all (28 MB) · Clear".
async function offlineStatus() {
  const s = await ask("status");
  if (!s) return;
  if (s.shellMissing) {
    $("offline-text").textContent = "Saving Negs for offline use…";
    $("offline-all").hidden = $("offline-clear").hidden = true;
  } else {
    $("offline-text").textContent = `Offline · ${mb(s.saved)} saved`;
    const rest = s.total - s.saved;
    $("offline-all").textContent = `Save all (${mb(rest)})`;
    $("offline-all").title = "Every border and spectral table, and the sample photo, for use offline";
    $("offline-all").hidden = rest <= 0;
    $("offline-clear").hidden = false;
  }
  $("offline").hidden = false;
}

async function clearOffline() {
  for (const r of await navigator.serviceWorker.getRegistrations()) await r.unregister();
  for (const name of await caches.keys()) if (name.startsWith("negs-")) await caches.delete(name);
  $("offline").hidden = $("update").hidden = true;
  showMessage("Removed what Negs saved for offline use. It's saved again the next time Negs opens.", false, 6000);
}

let announced = false;
function updateReady(text = "A new version of Negs is ready.") {
  $("update-text").textContent = text;
  $("update").hidden = false;
  if (!announced) showMessage(`${text} Reload to use it.`, false, 6000, { label: "Reload", run: () => $("update-reload").click() });
  announced = true;
}

// Installing: Chrome and Edge offer it (beforeinstallprompt), Safari has
// it in its menus. Shown in the footer until installed or dismissed.
let installPrompt = null;
addEventListener("beforeinstallprompt", (ev) => {
  ev.preventDefault();
  installPrompt = ev;
  showInstall();
});
addEventListener("appinstalled", () => {
  installPrompt = null;
  $("install").hidden = true;
  navigator.storage?.persist?.();
});
function showInstall() {
  if (standalone() || remembered("install-dismissed", false)) return;
  const macSafari = SAFARI && !IOS && /Macintosh/.test(ua) && Number(ua.match(/Version\/(\d+)/)?.[1]) >= 17;
  const text = installPrompt ? "Use Negs as an app, offline too."
    : IOS && SAFARI ? "Use Negs as an app: in Safari's Share menu, choose Add to Home Screen."
    : macSafari ? "Use Negs as an app: File → Add to Dock."
    : null;
  if (!text) return;
  $("install-text").textContent = text;
  $("install-app").hidden = !installPrompt;
  $("install").hidden = false;
}
$("install-app").addEventListener("click", async () => {
  const prompt = installPrompt;
  installPrompt = null;
  $("install").hidden = true;
  await prompt?.prompt();
});
$("install-dismiss").addEventListener("click", () => {
  remember("install-dismissed", true);
  $("install").hidden = true;
});
showInstall();

// ---- Wiring ----------------------------------------------------------------

buildControls();
buildPanes();
buildSheet();
touchSlider($("crop-angle-track"), $("crop-angle"));
catalog = await (await fetch("catalog.json")).json();
fillStocks();
crashCheck();
// Leaving loses the edit: only an exported JPEG keeps it. While a photograph
// is open with an edit other than the last exported (at first, the
// defaults), the Negs link asks first, and the browser asks before a reload,
// a closed tab or any other way out (except Safari on iOS, which never asks).
const editKey = () => JSON.stringify([state.film, state.paper, state.params, crop, border]);
let exportedEdit = editKey(), leaving = false;
const unexported = () => !!photo && editKey() !== exportedEdit;
addEventListener("beforeunload", (e) => {
  if (leaving || !unexported()) return;
  e.preventDefault();
  e.returnValue = "";
});
// Leaving ends the engine's and the decoder's thread pools, as replacing an
// engine does: Safari otherwise keeps their memory past the page, and a
// reload, online or offline, then ran out of it. A page Safari keeps to come
// back to starts a new engine when it's shown again.
addEventListener("pagehide", () => {
  host?.kill();
  decoder?.postMessage({ dispose: true });
  decoder = null;
});
addEventListener("pageshow", (e) => {
  if (e.persisted) starting = startEngine(kind).catch((e) => showMessage(`The engine didn't start: ${e.message}`, true));
});
$("brand").addEventListener("click", (e) => {
  if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0 || !unexported()) return;
  e.preventDefault();
  $("leave-confirm").showModal();
});
$("confirm-leave").addEventListener("click", () => { leaving = true; location.href = $("brand").href; });
// For automation over the DevTools protocol (web/cdp.mjs): a copy of the
// edit, edits through the path the controls take, and the renders so far.
globalThis.negs = {
  engineInstances: () => instances,
  tutorial: () => tutorial?.state(),
  state: () => structuredClone(state),
  edit(path, value) { set(path, value); settingsChanged(); },
  // Replaces the film, paper and params at once, as a preset's apply
  // would, through the same change path as the controls (settingsChanged).
  replaceSettings({ film = state.film, paper = state.paper, params = state.params }) {
    Object.assign(state, { film, paper, params: structuredClone(params) });
    settingsChanged();
  },
  reset: () => $("reset").click(),
  // The engine's photograph: what it was read as, the RAW WB it developed
  // at, and whether it is the edit's; `whenDeveloped` waits until it is.
  source: () => photo && { raw: photo.raw, colour: photo.colour, wb: structuredClone(photo.wb),
    current: sourceCurrent(), busy: source.busy, developing, needsDevelop: needsDevelop(), failures: developFailures },
  whenDeveloped: () => sourceReady(),
  // Applies preset text as Import preset does; resolves to whether it did.
  applyPreset: (text, name = "Preset", options) => presets.apply(text, name, options),
  presetBusy: () => presets.busy(),
  // Desktop's Prep | Lab | Output switch.
  pane: (pane) => showPane(pane),
  // A fresh edit: Reset Adjustments, the film (and paper, if given) chosen
  // as the pickers choose them, then `edits` ({ path: value }).
  start(film, paper, edits = {}) {
    resetAdjustments();
    chooseFilm(film);
    if (paper) choosePaper(paper);
    for (const [path, value] of Object.entries(edits)) set(path, value);
    sync();
    schedule("settled");
  },
  renders,
  idle: () => !running && !wanted,
  // The engine's Balance Print for any film, paper and params JSON, without
  // changing the edit.
  balance: (film, paper, params) => host.call("balancePrint", film, paper, params),
  // The edit rendered at `maxDim`, read back as bytes; the page shows nothing.
  read: (maxDim) => host.call("render", state.film, state.paper, params(), maxDim, false, false),
  // Measurement only: one frame at `maxDim`, presented as the editor's are,
  // with `patch` merged over the edit's params; the whole frame's time too.
  async frame(maxDim, interactive = false, patch = {}, film = state.film, paper = state.paper) {
    const merged = structuredClone(state.params);
    const merge = (into, from) => { for (const [k, v] of Object.entries(from)) v && typeof v === "object" && !Array.isArray(v) && into[k] && typeof into[k] === "object" ? merge(into[k], v) : (into[k] = v); };
    merge(merged, patch);
    const t = performance.now();
    const r = await host.call("render", film, paper, JSON.stringify({ ...merged, io: { ...merged.io, rotate_cw: 0 } }), maxDim, interactive, presenting);
    await draw(r);
    return { wallMs: performance.now() - t, elapsedMs: r.elapsedMs, width: r.width, height: r.height, stages: r.stages, presented: r.presented, backend: r.backend, memoryMiB: r.memoryMiB };
  },
  keptPixels: () => keptPixels(),
  // Prep › Match camera brightness: its state and note, and the switch.
  // The stock previews: whether they're on, the tiles on screen and how
  // many are current, and each tile render's time.
  previews: () => {
    const tiles = previewTiles();
    const every = folds ? ["film", "paper"].flatMap((which) => [...folds[which].tiles.values()].map((tile) => ({ which, tile, key: tileRequest(which, tile.id).key }))) : [];
    return { on: previewsOn, onScreen: tiles.length, current: tiles.filter(({ tile, request }) => tile.key === request.key).length,
      // Every tile, on screen or not, and the ones whose picture is out of date.
      all: every.length, outdated: every.filter(({ tile, key }) => tile.key !== key).map(({ which, tile }) => `${which}:${tile.id}${tile.key ? " (stale)" : ""}`),
      failed: tiles.filter(({ tile, request }) => tile.failed === request.key).length, previewing, stats: previewStats.slice() };
  },
  setPreviews: (on) => setPreviews(!!on),
  // One preview of the whole frame within `side`, for measurements.
  thumbnail: (side, film = state.film, paper = state.paper) => host.call("thumbnail", film, paper, params(), side, PREVIEW_PIPELINES, Crop.defaults(), 0),
  // Opens or folds the Film and Paper foldouts.
  openStocks: (film = true, paper = true) => { folds.film.root.open = film; folds.paper.root.open = paper; },
  match: () => ({ match: structuredClone(state.match), note: matchNote(), pending: matchPending(), active: matchActive(), solving: !!matching,
    exposure: get("prep.exposure_ev"), failure: matchFailure?.photo === photo ? matchFailure.message : null,
    reason: matchFailure?.photo === photo ? matchFailure.reason : null, lastSolve }),
  setMatch: (on) => setMatch(on),
  // Opens a photograph from a URL as the Open button would.
  // (An array buffer: Chrome fails blob() of a large response in a test profile.)
  open: async (url) => openFile(new File([await (await fetch(url)).arrayBuffer()], url.split("/").at(-1))),
  // The view (web/view.js), the times of its display passes, the side a
  // settled frame renders at, and where the engine draws its frame.
  view,
  views,
  renderSide: () => renderSide(),
  photo: () => photo && { width: photo.width, height: photo.height },
  openSample,
  // The placeholder showing, if any, and when each opening's steps happened.
  placeholder: () => early && { width: early.width, height: early.height, bitmap: [early.bitmap.width, early.bitmap.height], visible: !$("canvas").hidden },
  timeline,
  badge: () => !$("badge").hidden && $("badge-text").textContent,
  shown: () => host.call("shownRect"),
  // Show Original: hold it (or let go), whether it shows, the one made
  // (size, time, whether the engine keeps it), making it now, and its bytes
  // at `maxDim` without showing them.
  original: (on) => holdOriginal(on),
  showingOriginal: () => showingOriginal,
  originalMade: () => original && { side: original.side, width: original.width, height: original.height, presented: original.presented, elapsedMs: original.elapsedMs, stages: original.stages },
  prepareOriginal: () => prepareOriginal(),
  readOriginal: (maxDim) => host.call("original", params(), maxDim, false),
  // The border: change its settings as the controls do (a display pass),
  // read them, and where it lies (web/border.js layout) around the photograph.
  border(settings) {
    if (settings) {
      Object.assign(border, settings);
      borderChanged();
    }
    return { ...border };
  },
  borderLayout: () => { const size = photoSize(); return photo && borderLayout(size.width, size.height); },
  crop: () => structuredClone(cropSession?.settings ?? crop),
  cropSession: () => cropSession && { settings: structuredClone(cropSession.settings), rotation: cropSession.rotation, shown: cropSession.shown },
  beginCrop, endCrop, rotate,
  photoGeometry,
  borderTexture: () => { const t = borderTexture(); return t && { path: t.path, width: t.width, height: t.height }; },
  borderTextures: () => [...textures.keys()],
  canvasRect: () => view.canvasRect(),
  // The export's bytes, bordered as it would be, and the bytes before; and
  // the engine's border on any bytes.
  exportBytes: () => exportRender(),
  addBorder: (...args) => host.call("addBorder", ...args),
};

syncs.push(fetchBorderTexture);
$("film").addEventListener("change", () => { chooseFilm(stockValue($("film").value)); sync(); schedule("settled"); });
$("paper").addEventListener("change", () => { choosePaper(stockValue($("paper").value)); sync(); schedule("settled"); });
$("reset").addEventListener("click", () => { resetAdjustments(); forgetRevealed(); schedule("settled"); });
$("scan-film").addEventListener("change", () => { set("io.scan_film", $("scan-film").checked); sync(); schedule("settled"); });
$("engine").addEventListener("change", async () => {
  if ($("engine").value === kind) return;
  // A superseded start leaves rendering to the selection that replaced it.
  if (await startEngine($("engine").value).catch((e) => showMessage(e.message, true)) === false) return;
  schedule("settled");
});
for (const b of [$("open"), $("open-empty")]) b.addEventListener("click", () => $("file").click());
$("open-sample").addEventListener("click", openSample);
$("file").addEventListener("change", () => $("file").files[0] && openFile($("file").files[0]));
$("export").addEventListener("click", exportJpeg);
// The presets' dialogs take the keyboard: no crop, rotate or original keys.
const dialogOpen = () => !!document.querySelector("dialog[open]");

// Show Original: a mouse or pen holds the button, a tap (or Return, Space,
// or a screen reader, which can't hold) toggles it; `\` is held, not while
// typing in a field.
{
  const button = $("original");
  if (matchMedia("(pointer: coarse)").matches) button.title = "Tap to show the original; tap again to go back";
  let held = null, pointer = "";
  button.addEventListener("pointerenter", () => prepareOriginal());
  button.addEventListener("pointerdown", (ev) => {
    pointer = ev.pointerType;
    if (ev.pointerType === "touch" || ev.button !== 0) return;
    held = ev.pointerId;
    button.setPointerCapture(ev.pointerId);
    holdOriginal(true);
  });
  const release = (ev) => {
    if (ev.pointerId !== held) return;
    held = null;
    holdOriginal(false);
  };
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) button.addEventListener(type, release);
  // The click after a mouse or pen's press is the press's.
  button.addEventListener("click", (ev) => (ev.detail === 0 || pointer === "touch") && holdOriginal(!holding));
  const typing = (t) => t instanceof Element && t.matches("textarea, [contenteditable], input:not([type=range], [type=checkbox], [type=radio], [type=file])");
  let key = false;
  addEventListener("keydown", (ev) => {
    if (ev.key !== "\\" || ev.ctrlKey || ev.metaKey || typing(ev.target) || dialogOpen()) return;
    ev.preventDefault();
    if (ev.repeat || key) return;
    key = true;
    holdOriginal(true);
  });
  const letGo = () => {
    if (!key) return;
    key = false;
    holdOriginal(false);
  };
  addEventListener("keyup", (ev) => ev.key === "\\" && letGo());
  addEventListener("blur", letGo);
}

$("crop").addEventListener("click", () => cropSession ? endCrop(true) : beginCrop());
$("crop-done").addEventListener("click", () => endCrop(true));
$("crop-cancel").addEventListener("click", () => endCrop(false));
$("rotate-left").addEventListener("click", () => rotate(-1));
$("rotate-right").addEventListener("click", () => rotate(1));
addEventListener("keydown", (ev) => {
  if (dialogOpen() || tutorialPresentation || tutorialRestoring) return;
  if (ev.target instanceof Element && ev.target.matches("select, textarea, [contenteditable], input:not([type=range], [type=checkbox], [type=radio])")) return;
  const key = ev.key.toLowerCase();
  if (cropSession && (key === "escape" || key === "enter")) { ev.preventDefault(); endCrop(key === "enter"); return; }
  if (ev.repeat || ev.altKey) return;
  if ((key === "k" && (ev.metaKey || ev.ctrlKey)) || (key === "c" && !ev.metaKey && !ev.ctrlKey)) {
    ev.preventDefault(); cropSession ? endCrop(true) : beginCrop();
  } else if (key === "r" && !ev.metaKey && !ev.ctrlKey) { ev.preventDefault(); rotate(ev.shiftKey ? -1 : 1); }
});

$("fit").addEventListener("click", () => view.fit());
$("actual").addEventListener("click", () => view.actualSize());

const viewport = $("view");
viewport.addEventListener("dragover", (ev) => { ev.preventDefault(); viewport.classList.add("dragging"); });
viewport.addEventListener("dragleave", () => viewport.classList.remove("dragging"));
viewport.addEventListener("drop", (ev) => {
  ev.preventDefault();
  viewport.classList.remove("dragging");
  const file = ev.dataTransfer.files[0];
  if (file) openFile(file);
});

// The adapter the engine will get (it asks for high performance), named in
// the console so a crash report says which GPU it was.
const adapter = navigator.gpu && (await navigator.gpu.requestAdapter({ powerPreference: "high-performance" }).catch(() => null));
if (adapter) {
  const { vendor, architecture, device, description } = adapter.info ?? {};
  console.info(`WebGPU adapter: ${[vendor, architecture, device, description].filter(Boolean).join(" · ") || "unknown"}${adapter.info?.isFallbackAdapter ? " (fallback)" : ""}`);
}
const gpu = query.get("backend") ?? (adapter ? "gpu" : "cpu");
starting = startEngine(gpu).catch((e) => showMessage(`The engine didn't start: ${e.message}`, true));
starting.then(() => presets.sync());
tutorial = createTutorial(tutorialAdapter());
presets.sync();
// A shared preset link (#preset=…) shows its dialog over the editor.
void presets.arrive();
// Files opened with the installed app (the manifest's file_handlers).
if (self.launchQueue) {
  launchQueue.setConsumer(async ({ files }) => files?.length && openFile(await files[0].getFile()));
}
await starting;
// After the engine, so saving for offline use doesn't slow the first start.
startOffline();

// Automation: ?image=<url>&name=<result>&runs=<n>&max=<px>[&params=<json>]
// opens the image, renders it n times at max pixels, shown 1:1, then posts the last image and
// the timings to the dev server (web/serve.mjs). A presented frame's canvas is compared with
// the image read back (`canvas` in the timings), and the part of it the canvas shows is posted
// as <name>-canvas.
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
    const wb = wantedWb();
    const size = await sourceOp(() => loadPhoto(bytes, wb));
    const loadMs = performance.now() - t;
    photo = { ...opened("automation", bytes, size), ...developedAs(size, wb) };
    const max = Number(query.get("max") ?? 2048);
    // The frame 1:1, so the canvas shows its pixels (or those that fit).
    const long = Math.max(photo.width, photo.height);
    view.setPhoto(photo.width, photo.height);
    view.actualSize(Math.min(max, long) / long);
    // The frames below are the settled ones; the view asks for no other.
    settledSide = renderSide();
    if (presenting) await sendView();
    const interactive = query.get("interactive") === "1";
    const times = [], stages = [], frames = [];
    let r;
    for (let i = 0; i < Number(query.get("runs") ?? 1); i++) {
      // The whole frame, as the page sees it: the call, and drawing it.
      const t = performance.now();
      r = await host.call("render", state.film, state.paper, params(), max, interactive, presenting);
      await draw(r);
      frames.push(performance.now() - t);
      times.push(r.elapsedMs);
      stages.push(r.stages);
    }
    // The image itself, read back, and what the engine's canvas shows of
    // it, compared pixel for pixel.
    let canvas;
    if (r.presented) {
      const [width, height, left, top, drawnWidth, drawnHeight] = await host.call("shownRect");
      const shown = await createImageBitmap($("gpu-canvas"));
      const copy = new OffscreenCanvas(shown.width, shown.height).getContext("2d");
      copy.drawImage(shown, 0, 0);
      const [x0, y0] = [Math.max(0, left), Math.max(0, top)];
      const [w, h] = [Math.min(shown.width, left + width) - x0, Math.min(shown.height, top + height) - y0];
      const seen = copy.getImageData(x0, y0, w, h).data;
      await fetch(`result?name=${encodeURIComponent(`${name}-canvas`)}&w=${w}&h=${h}`, { method: "POST", body: seen });
      r = await host.call("render", state.film, state.paper, params(), max, interactive, false);
      let same = 0, most = 0;
      for (let y = 0; y < h; y++) {
        for (let x = 0; x < w; x++) {
          const a = (y * w + x) * 4, b = ((y + y0 - top) * width + (x + x0 - left)) * 4;
          const d = Math.max(...[0, 1, 2].map((c) => Math.abs(seen[a + c] - r.rgba[b + c])));
          same += d === 0;
          most = Math.max(most, d);
        }
      }
      canvas = { oneToOne: drawnWidth === width && drawnHeight === height, pixels: w * h, exact: same / (w * h), max: most };
    }
    await fetch(`result?name=${encodeURIComponent(name)}&w=${r.width}&h=${r.height}`, { method: "POST", body: r.rgba });
    await post(name, JSON.stringify({ threads, presenting, backend: r.backend, width: r.width, height: r.height, loadMs, times, frames, stages, canvas, memoryMiB: r.memoryMiB }));
  } catch (e) {
    await post(name, JSON.stringify({ error: String(e), stack: e?.stack }));
  }
}
