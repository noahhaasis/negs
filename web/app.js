// The Negs web app: opens a photograph in the engine worker (web/worker.js)
// once, then renders on every change. While a slider moves it renders a
// smaller interactive frame and drops frames a newer change superseded; when
// it stops, the settled frame at the canvas's device resolution.
const $ = (id) => document.getElementById(id);
const query = new URLSearchParams(location.search);

import { SECTIONS, optionTitle, snap, formatValue } from "./controls.js";

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
    sync(); // Commands that need the engine.
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
  const changed = (mode) => { sync(); schedule(mode); };
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
  const root = $("sections");
  const opened = remembered("groups", {});
  for (const section of SECTIONS) {
    // A fieldset, so a section or group that is off disables its controls.
    const body = el("fieldset", { className: "section" }, el("h2", { textContent: section.title }));
    body.dataset.tab = sectionId(section);
    for (const c of section.controls ?? []) body.append(buildControl(c));
    for (const group of section.groups) {
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
    }
    root.append(body);
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
const TABS = [{ id: "stock", title: "Stock" }, ...SECTIONS.map((s) => ({ id: sectionId(s), title: s.tab ?? s.title }))];

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
  if (!photo) return 0.45 * height;
  const pad = width < 500 ? 12 : 32, hud = 28;
  const view = ((width - 2 * pad) * photo.height) / photo.width + 2 * pad + hud;
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
      renders.push({ interactive, width: r.width, height: r.height, elapsedMs: r.elapsedMs, stages: r.stages, presented: r.presented, backend: r.backend });
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
    setSheet(sheet);
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

// A message stays until hidden; one shown for `ms` hides itself then, and
// the renders that follow it leave it up.
let messageTimer;
function showMessage(text, error = false, ms = 0) {
  const m = $("message");
  m.textContent = text;
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

// ---- Wiring ----------------------------------------------------------------

buildControls();
buildSheet();
if (matchMedia("(pointer: coarse)").matches) $("value-hint").textContent = "Tap a value to type one.";
catalog = await (await fetch("catalog.json")).json();
fillStocks();
// For automation over the DevTools protocol (web/cdp.mjs): a copy of the
// edit, edits through the path the controls take, and the renders so far.
globalThis.negs = {
  state: () => structuredClone(state),
  edit(path, value) { set(path, value); sync(); schedule("settled"); },
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
  open: async (url) => open(new File([await (await fetch(url)).arrayBuffer()], url.split("/").at(-1))),
};

$("film").addEventListener("change", () => { chooseFilm($("film").value); sync(); schedule("settled"); });
$("paper").addEventListener("change", () => { choosePaper($("paper").value); sync(); schedule("settled"); });
$("reset").addEventListener("click", () => { resetAdjustments(); sync(); schedule("settled"); });
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
