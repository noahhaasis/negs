// The Negs web app: opens a photograph in the engine worker (web/worker.js)
// once, then renders on every change. While a slider moves it renders a
// smaller interactive frame and drops frames a newer change superseded; when
// it stops, the settled frame at the canvas's device resolution.
const $ = (id) => document.getElementById(id);
const query = new URLSearchParams(location.search);

import { SECTIONS, POST_SECTIONS, optionTitle, snap, formatValue } from "./controls.js";
import { createView } from "./view.js";
import { placeholder } from "./placeholder.js";
import * as Border from "./border.js";
import { fetchFailed } from "./engine.js";

// The edit, as desktop keeps it (macOS EditState): the film, the paper and a
// complete RuntimeParams, starting from the engine's defaults. Values a
// film's presets fill stay null until edited.
let catalog = { profiles: [], defaults: {}, nullable_paths: [] };
const state = { film: "", paper: "", params: {} };
// Every value a film's presets fill (macOS Stock.presetPaths): a film change resets them.
const GRAIN_FIELDS = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"];
const PRESET_PATHS = [
  ...GRAIN_FIELDS.map((f) => `film_render.grain.${f}`),
  "film_render.dir_couplers.gamma_samelayer_rgb", "film_render.dir_couplers.gamma_interlayer_r_to_gb",
  "film_render.dir_couplers.gamma_interlayer_g_to_rb", "film_render.dir_couplers.gamma_interlayer_b_to_rg",
  "film_render.halation.halation_strength", "film_render.halation.halation_first_sigma_um",
];
let photo = null;             // { name, bytes, width, height }
// The presentation border (web/border.js), beside the edit as desktop keeps
// it: not in its params, so it never renders the film.
const border = { ...Border.DEFAULT_BORDER };
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
    sync(); // Commands that need the engine.
    await attachCanvas();
  } catch (e) {
    // A newer selection replaced this engine; that start owns the page.
    if (e.restarted) return false;
    if (kind === "gpu") {
      showMessage(`WebGPU isn't available here (${e.message}); using the CPU engine.`);
      return startEngine("cpu");
    }
    throw e;
  }
  if (photo) {
    $("busy").classList.add("on");
    const loaded = await loadPhoto(photo.bytes)
      .then(() => true, (e) => { if (e.restarted) return false; throw e; })
      .finally(() => running || $("busy").classList.remove("on"));
    if (!loaded) return false;
  }
  return true;
}

// The WebGPU engine presents on a canvas it owns, with nothing read back. A
// canvas transfers to one worker only, so each engine gets a new one.
let presenting = false;
async function attachCanvas() {
  presenting = false;
  sentView = "";
  engineTexture = null;
  $("gpu-canvas")?.remove();
  // The page's canvas keeps the bytes' frame, or a placeholder.
  $("canvas").hidden = !frame && !early;
  if (kind !== "gpu" || !self.OffscreenCanvas || !HTMLCanvasElement.prototype.transferControlToOffscreen || query.get("present") === "0") return;
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

const params = () => JSON.stringify(state.params);

// A new film brings its own grain, DIR and halation presets, as upstream
// applies them (macOS EditorModel.chooseFilm).
function chooseFilm(id) {
  state.film = id;
  set("film_render.development_time", null);
  for (const path of PRESET_PATHS) set(path, null);
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

// Restores the defaults, keeping the film, paper and rotation, as desktop's
// Reset Adjustments does (macOS EditorModel.reset).
function resetAdjustments() {
  const kept = ["io.rotate_cw", "io.photo_long_side_px"].map((path) => [path, get(path)]);
  state.params = structuredClone(catalog.defaults);
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

// Commands the controls' `action` buttons run, with whether each may run now.
const actions = {
  balancePrint: { run: balancePrint, busy: () => balancing, busyLabel: "Balancing…", enabled: () => !balancing && !!host && !get("io.scan_film") },
};

function buildControl(c) {
  const row = buildRow(c);
  if (c.help) row.title = c.help;
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
    const button = el("button", { type: "button", className: "btn", textContent: c.label, title: c.help });
    button.addEventListener("click", () => a.run());
    syncs.push(() => {
      button.disabled = !a.enabled();
      button.textContent = a.busy() ? a.busyLabel : c.label;
    });
    return el("div", { className: "ctl action" }, button);
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
      if (c.when) row.hidden = !get(c.when);
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
  title.addEventListener("dblclick", () => { b.set(b.initial()); changed("settled"); });
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
    row.hidden = bw && !c.bw;
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
    const size = photo ?? early;
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

function buildControls() {
  const opened = remembered("groups", {});
  const sections = [...SECTIONS.map((s) => [s, "lab"]), ...POST_SECTIONS.map((s) => [s, "post"])];
  for (const [section, pane] of sections) {
    // A fieldset, so a section or group that is off disables its controls.
    const body = el("fieldset", { className: "section" }, el("h2", { textContent: section.title }));
    body.dataset.tab = sectionId(section);
    body.dataset.pane = pane;
    // A phone tab with several sections keeps their titles.
    body.classList.toggle("titled", sections.filter(([s]) => sectionId(s) === body.dataset.tab).length > 1);
    for (const c of section.controls ?? []) body.append(buildControl(c));
    for (const group of section.groups ?? []) {
      const key = `${section.title}/${group.title}`;
      const details = el("details", { open: opened[key] ?? group.open ?? false });
      details.addEventListener("toggle", () => remember("groups", { ...remembered("groups", {}), [key]: details.open }));
      const summary = el("summary", {}, el("span", { textContent: group.title }));
      const controls = el("fieldset", { className: "controls" }, ...group.controls.map(buildControl));
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
      body.append(details);
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
    $(pane === "post" ? "post-sections" : "sections").append(body);
  }
}

// Desktop's Lab | Post switch (macOS AdjustmentInspector): which pane the
// sidebar shows. On the phone's sheet each section has its tab instead.
function showPane(pane) {
  if (pane !== "post") pane = "lab";
  $("side").dataset.pane = pane;
  for (const button of $("panes").children) button.setAttribute("aria-selected", button.dataset.pane === pane);
  remember("pane", pane);
}
function buildPanes() {
  for (const button of $("panes").children) {
    button.addEventListener("click", () => {
      showPane(button.dataset.pane);
      $("scroll").scrollTop = 0;
    });
  }
  showPane(remembered("pane", "lab"));
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
    // Scanned film isn't printed: no paper.
    $("scan-film").checked = get("io.scan_film");
    $("paper-row").hidden = get("io.scan_film");
  });
  sync();
}

// ---- Phone and iPad layout -------------------------------------------------

// Where a sidebar doesn't fit beside the photograph, the inspector is a
// bottom sheet with a tab per section (index.html has the same query).
const SHEET = matchMedia("(max-width: 899px) and (orientation: portrait), (max-width: 639px)");
const TABS = [{ id: "stock", title: "Stock" }, ...[...SECTIONS, ...POST_SECTIONS].map((s) => ({ id: sectionId(s), title: s.tab ?? s.title }))]
  .filter((t, i, all) => all.findIndex((u) => u.id === t.id) === i);

function showTab(id) {
  if (!TABS.some((t) => t.id === id)) id = "stock";
  $("side").dataset.tab = id;
  for (const panel of $("scroll").querySelectorAll("[data-tab]")) panel.classList.toggle("shown", panel.dataset.tab === id);
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
  if (snap === "low") return $("scroll").offsetTop - side.offsetTop + parseFloat(getComputedStyle(side).paddingBottom);
  if (snap === "high") return 0.8 * height;
  const shape = photo ?? early;
  if (!shape) return 0.45 * height;
  const pad = width < 500 ? 12 : 32, hud = 28;
  const view = ((width - 2 * pad) * shape.height) / shape.width + 2 * pad + hud;
  return Math.min(0.62 * height, Math.max(0.4 * height, height - view));
}
function setSheet(snap) {
  sheet = SNAPS.includes(snap) ? snap : "mid";
  $("app").style.setProperty("--sheet", `${Math.round(snapHeight(sheet))}px`);
  remember("sheet", sheet);
}

function buildSheet() {
  for (const t of TABS) {
    const button = el("button", { type: "button", textContent: t.title, role: "tab" });
    button.dataset.tab = t.id;
    button.addEventListener("click", () => {
      showTab(t.id);
      if (sheet === "low") setSheet("mid");
      $("scroll").scrollTop = 0;
    });
    $("tabs").append(button);
  }
  showTab(remembered("tab", "stock"));
  setSheet(sheet);
  let drag = null;
  // On the sheet, and on screens too short to spare the room, the engine's
  // footer scrolls with the Stock tab; otherwise it stays at the sidebar's foot.
  const short = matchMedia("(max-height: 599px)");
  const place = () => {
    // The Lab | Post panes are the sidebar's; the sheet shows a tab at a time.
    $("side").classList.toggle("sheet", SHEET.matches);
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
    const max = 0.9 * $("app").clientHeight;
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
let wanted = null, running = false, settleTimer = 0, exporting = false;
const renders = [];

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
  while (wanted && photo && !loading) {
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
      await failed(e, "Render failed");
    }
  }
  running = false;
  $("busy").classList.remove("on");
}

// The longest side to render, in photograph pixels: as many as the device
// pixels the view covers, so all of them from 100% on, within what the GPU
// takes and the device's cap (MAX_PIXELS). The CPU engine renders the
// fitted size and more only up to a few megapixels, as each takes seconds.
const MAX_PIXELS = matchMedia("(pointer: coarse)").matches ? 12e6 : 40e6;
const CPU_PIXELS = 4e6;
let limits = null;       // the WebGPU engine's: { side, pixels }

// The pixels the engine keeps of a photograph, 0 for all. A phone or tablet,
// whose tab iOS ends from about 1.5 GB, keeps no more than it ever renders,
// and exports that copy: a full-size export there doesn't fit, so the desktop
// exports every pixel. The same budget as renderSide's, so its largest frame
// is this copy at 1:1.
const keptPixels = () => (matchMedia("(pointer: coarse)").matches ? Math.min(MAX_PIXELS, limits?.pixels ?? MAX_PIXELS) : 0);
const loadPhoto = (bytes) => host.call("load", bytes, 0, keptPixels());
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
const view = createView($("stage"), $("view"), { changed: showView, settled: viewSettled, around: (w, h) => borderLayout(w, h) ?? { width: w, height: h, x: 0, y: 0 } });
let frame = null;        // the last frame drawn from bytes: { bitmap, width, height }
let settledSide = 0;     // the longest side of the last settled frame asked for
const views = [];        // display passes' times (the engine's, or the page's paint), for checks

// Draw the photograph where the view now puts it: the engine draws its
// canvas again from the frame it holds (one pass at a time, the newest view
// next), or the page draws the bytes.
let showing = false, showAgain = false, sentView = "";
function showView() {
  if (!photo && !early) return;
  zoomLevel();
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
const viewKey = () => JSON.stringify([view.size(), view.rect(), showingOriginal, borderView()]);
function sendView() {
  const { width, height } = view.size(), r = view.rect();
  sentView = viewKey();
  sendBorderTexture();
  return host.call("show", width, height, [r.x, r.y, r.width, r.height], BACKGROUND, showingOriginal, borderView());
}

// Once a zoom, pan or resize stops: render again when the view needs more
// pixels than the frame has, or, fitted, other ones.
function viewSettled() {
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
  const r = view.rect(), { width: w, height: h } = shown.bitmap;
  // The border around the photograph's frame, not the placeholder's.
  const l = !early && borderLayout(photo.width, photo.height);
  if (l) Border.paintUnder(ctx, l, r, photo.width);
  if (Math.abs(r.width - w) <= 1 && Math.abs(r.height - h) <= 1) {
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(shown.bitmap, Math.round(r.x + (r.width - w) / 2), Math.round(r.y + (r.height - h) / 2));
  } else {
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(shown.bitmap, r.x, r.y, r.width, r.height);
  }
  if (l) Border.paintOver(ctx, l, r, photo.width, borderTexture()?.bitmap);
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
function borderTexture(w = photo?.width, h = photo?.height) {
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
  const l = photo && borderLayout(photo.width, photo.height);
  return l ? Border.viewOf(l, photo.width, photo.height) : [];
}

// Fetch the texture the border needs, with progress, once: a film without
// an edge print takes the generic strip.
function fetchBorderTexture() {
  const key = photo && textureFor(photo.width, photo.height);
  if (!key || textures.has(key)) return;
  const style = Border.STYLES.find((s) => s.id === border.style).title;
  const progress = (f) => showMessage(`Downloading the ${style} border… ${Math.floor(f * 100)}%`);
  const get = (path) => download(`data/${path}`, undefined, progress).then((bytes) => ({ path, bytes }));
  const load = get(key)
    .catch((e) => {
      // Offline, the generic strip may be saved where the film's isn't.
      if ((e.status !== 404 && !e.offline) || border.style !== "35mm") throw e;
      return get(Border.genericStrip(Border.filmFormat(photo.width, photo.height)));
    })
    .then(async ({ path, bytes }) => {
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const format = key === Border.FILM_TEXTURE.path ? null : Border.filmFormat(photo.width, photo.height);
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
  if (photo) view.setPhoto(photo.width, photo.height);
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
const canShowOriginal = () => !!photo && !loading && !early && developed && !!host;

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
  originals++;
  originalLoad = null;
  original?.bitmap?.close();
  original = null;
  holding = false;
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
  const el = $("hud");
  const ms = `${r.elapsedMs.toFixed(0)} ms`;
  const shown = Math.max(r.width, r.height) / Math.max(photo.width, photo.height);
  const parts = interactive
    ? ["Preview", `${r.width} × ${r.height}`, ms]
    : [`${photo.width} × ${photo.height}`, ...(shown < 1 ? [`shown at ${percent(shown)}`] : []), ms, `${r.memoryMiB} MiB`];
  // Each part on one line; the line breaks between them.
  el.textContent = parts.map((p) => p.replaceAll(" ", "\u00a0")).join("\u00a0· ");
  el.title = shown < 1 && !interactive
    ? `The preview shows ${percent(shown)} of these pixels. ${exportNote()}; inspect grain in an exported file.\n${r.stages}`
    : r.stages;
}

// ---- Opening and exporting -------------------------------------------------

// What an export of the photograph develops.
const exportNote = () => photo.exportWidth < photo.width
  ? `Exports here develop ${photo.exportWidth} × ${photo.exportHeight}; a desktop browser exports all of them`
  : "Exports develop all of them";

// Opens `name`, whose bytes `data` resolves to. Until its first frame, a
// placeholder from the bytes shows.
let opens = 0, loading = false, starting = null;
async function open(name, data) {
  const opening = ++opens, previous = photo;
  let loaded = false;
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
    let size;
    await starting;
    // Switching engines restarts the worker under a load: load again.
    for (;;) {
      try {
        await host.ready;
        size = await loadPhoto(bytes);
        break;
      } catch (e) {
        if (!e.restarted) throw e;
      }
    }
    loaded = true;
    mark("decoded", name);
    // A newer photograph is opening; the engine loads it next.
    if (opening !== opens) return;
    // Its size as decoded, whatever the engine keeps.
    photo = opened(name, bytes, size);
    console.log(`decoded ${name} ${size.width}×${size.height} in ${(performance.now() - t).toFixed(0)} ms`);
    $("export").disabled = false;
    $("export").title = photo.exportWidth < photo.width ? `${exportNote()}.` : "";
    // Nothing of the last photograph shows; the engine forgot its frame.
    frame?.bitmap.close();
    frame = null;
    developed = false;
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
  } catch (e) {
    loaded = true;
    if (opening !== opens || e.name === "AbortError") return;
    setBadge(null);
    photo = previous;
    dropEarly(true);
    // The engine let the last photograph go before this one's decode.
    if (photo) await reloadPhoto().catch(() => {});
    $("empty").hidden = !!photo;
    await failed(e, `Couldn't open ${name}`);
  } finally {
    if (opening === opens) loading = false;
    if (!running) $("busy").classList.remove("on");
    if (!loading && wanted) pump();
    updateOriginal();
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
function openFile(file) {
  sample?.abort();
  return open(file.name, file.arrayBuffer().then((b) => new Uint8Array(b)));
}

// ---- The sample photograph ---------------------------------------------------

// A photograph to try Negs on without one of your own, offered on the empty
// screen and downloaded only when asked for: a Nikon Z f NEF made into a
// 3.4 MB lossy DNG (web/sample/make-dng.py), whose camera preview shows
// while it develops.
const SAMPLE = { name: "DSC_6377.dng", url: "sample/DSC_6377.dng" };
let sample = null;
function openSample() {
  sample = new AbortController();
  const { signal } = sample;
  setBadge("Downloading");
  $("empty").hidden = true;
  // open() counts this opening as the next.
  const opening = opens + 1;
  const data = download(SAMPLE.url, signal, (f) => opens === opening && setBadge(`Downloading ${Math.floor(f * 100)}%`));
  return open(SAMPLE.name, data).finally(() => {
    if (sample?.signal === signal) sample = null;
  });
}

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
    const message = $("message").textContent;
    await startEngine(kind).catch(() => {});
    showMessage(message, true);
  }
}

async function exportJpeg() {
  if (!photo || exporting) return;
  exporting = true;
  $("export").disabled = true;
  showMessage(`Rendering ${photo.exportWidth}×${photo.exportHeight} for export…`);
  $("busy").classList.add("on");
  try {
    while (running) await new Promise((r) => setTimeout(r, 20));
    const r = await exportRender();
    const canvas = new OffscreenCanvas(r.width, r.height);
    canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(r.rgba.buffer, r.rgba.byteOffset, r.rgba.length), r.width, r.height), 0, 0);
    const blob = await canvas.convertToBlob({ type: "image/jpeg", quality: 0.92 });
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
  const r = await host.call("render", state.film, state.paper, params(), Math.max(photo.exportWidth, photo.exportHeight), false);
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
  await loadPhoto(photo.bytes);
  forgetOriginal();
  settledSide = 0;
  schedule("settled");
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
  if (!actions.balancePrint.enabled()) return;
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

// What's saved: "Works offline · 24 MB saved", and the rest on offer.
async function offlineStatus() {
  const s = await ask("status");
  if (!s) return;
  if (s.shellMissing) {
    $("offline-text").textContent = "Saving Negs for offline use…";
    $("offline-all").hidden = $("offline-clear").hidden = true;
  } else {
    $("offline-text").textContent = `Works offline · ${mb(s.saved)} saved.`;
    const rest = s.total - s.saved;
    $("offline-all").textContent = `Save everything (${mb(rest)} more)`;
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
if (matchMedia("(pointer: coarse)").matches) $("value-hint").textContent = "Tap a value to type one.";
catalog = await (await fetch("catalog.json")).json();
fillStocks();
// For automation over the DevTools protocol (web/cdp.mjs): a copy of the
// edit, edits through the path the controls take, and the renders so far.
globalThis.negs = {
  engineInstances: () => instances,
  state: () => structuredClone(state),
  edit(path, value) { set(path, value); sync(); schedule("settled"); },
  // Desktop's Lab | Post switch.
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
  borderLayout: () => photo && borderLayout(photo.width, photo.height),
  borderTexture: () => { const t = borderTexture(); return t && { path: t.path, width: t.width, height: t.height }; },
  borderTextures: () => [...textures.keys()],
  canvasRect: () => view.canvasRect(),
  // The export's bytes, bordered as it would be, and the bytes before; and
  // the engine's border on any bytes.
  exportBytes: () => exportRender(),
  addBorder: (...args) => host.call("addBorder", ...args),
};

syncs.push(fetchBorderTexture);
$("film").addEventListener("change", () => { chooseFilm($("film").value); sync(); schedule("settled"); });
$("paper").addEventListener("change", () => { choosePaper($("paper").value); sync(); schedule("settled"); });
$("reset").addEventListener("click", () => { resetAdjustments(); sync(); schedule("settled"); });
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
    if (ev.key !== "\\" || ev.ctrlKey || ev.metaKey || typing(ev.target)) return;
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

const gpu = query.get("backend") ?? (navigator.gpu && (await navigator.gpu.requestAdapter().catch(() => null)) ? "gpu" : "cpu");
starting = startEngine(gpu).catch((e) => showMessage(`The engine didn't start: ${e.message}`, true));
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
    const size = await loadPhoto(bytes);
    const loadMs = performance.now() - t;
    photo = opened("automation", bytes, size);
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
