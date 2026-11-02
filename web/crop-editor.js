// Crop interaction and inspector. Geometry and export share crop.js; the
// overlay is independent of the CPU/GPU image underneath it.
import * as Crop from "./crop.js";
const $ = (id) => document.getElementById(id);
export function createCropEditor({ session, view, changed, exportSize }) {
  const overlay = $("crop-overlay"), aspect = $("crop-aspect"), angle = $("crop-angle"), dial = $("crop-dial");
  for (const [id, title] of Crop.ASPECTS) aspect.add(new Option(title, id));
  const update = () => changed();
  aspect.addEventListener("change", () => { session().choose(aspect.value); update(); });
  for (const [id, portrait] of [["crop-landscape", false], ["crop-portrait", true]]) $(id).addEventListener("click", () => { session().orient(portrait); update(); });
  for (const [i, id] of ["crop-width", "crop-height"].entries()) $(id).addEventListener("change", () => {
    const v = Number($(id).value);
    if (Number.isFinite(v) && v > 0) { session().settings.custom[i] = Crop.clamp(v, 0.1, 1000); session().refit(); update(); }
    else refresh();
  });
  const straighten = (v) => { if (!session()) return; session().straighten(v); update(); };
  angle.addEventListener("input", () => straighten(Number(angle.value)));
  const endAngle = () => { if (session()) session().intended = null; refresh(); };
  for (const type of ["change", "pointercancel", "blur"]) angle.addEventListener(type, endAngle);
  $("crop-angle-value").addEventListener("change", () => {
    const value = Number($("crop-angle-value").value);
    if (Number.isFinite(value)) straighten(value);
    endAngle();
  });
  $("crop-angle-reset").addEventListener("click", () => { straighten(0); endAngle(); });
  $("crop-reset").addEventListener("click", () => { session().reset(); update(); });

  let drag = null, dialDrag = null;
  function layout() {
    const s = session(), r = view.rect(), dpr = view.size().width / $("stage").clientWidth, scale = r.width / s.photo.width;
    const c = s.shown;
    return { r, dpr, scale, crop: { x: r.x + c.x * scale, y: r.y + c.y * scale, width: c.width * scale, height: c.height * scale } };
  }
  function at(ev) { const b = overlay.getBoundingClientRect(), dpr = view.size().width / b.width; return { x: (ev.clientX - b.x) * dpr, y: (ev.clientY - b.y) * dpr }; }
  const cursor = (h) => !h ? "" : !h[0] && !h[1] ? "grab" : !h[0] ? "ns-resize" : !h[1] ? "ew-resize" : h[0] === h[1] ? "nwse-resize" : "nesw-resize";
  function handle(ev) {
    const { crop, dpr } = layout();
    return Crop.handleAt({ x: at(ev).x / dpr, y: at(ev).y / dpr }, Object.fromEntries(Object.entries(crop).map(([k, v]) => [k, v / dpr])), ev.pointerType === "touch");
  }
  overlay.addEventListener("pointerdown", (ev) => {
    if (!session() || drag || ev.button !== 0) return;
    const h = handle(ev);
    if (!h) return;
    ev.preventDefault();
    overlay.setPointerCapture(ev.pointerId);
    drag = { handle: h, start: session().shown, at: at(ev), id: ev.pointerId };
    overlay.style.cursor = h.some(Boolean) ? cursor(h) : "grabbing";
    refresh();
  });
  overlay.addEventListener("pointermove", (ev) => {
    const s = session();
    if (!s) return;
    if (!drag) { overlay.style.cursor = cursor(handle(ev)); return; }
    if (ev.pointerId !== drag.id) return;
    const { scale, dpr } = layout(), p = at(ev), dx = (p.x - drag.at.x) / scale, dy = (p.y - drag.at.y) / scale;
    const { start, handle: h } = drag, angle = s.settings.angle, bounds = Crop.bounds(s.photo, angle);
    if (!h.some(Boolean)) {
      const across = Crop.constrain(start, Crop.move(start, dx, 0, bounds), angle, s.photo);
      s.shown = Crop.constrain(across, Crop.move(across, 0, dy, bounds), angle, s.photo);
    } else {
      const edge = { x: (h[0] < 0 ? start.x : start.x + start.width) + dx, y: (h[1] < 0 ? start.y : start.y + start.height) + dy };
      s.shown = Crop.constrain(start, Crop.resize(start, h, edge, s.ratio, bounds, 40 * dpr / scale), angle, s.photo);
    }
    update();
  });
  const endDrag = () => { drag = null; overlay.style.cursor = ""; refresh(); };
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) overlay.addEventListener(type, endDrag);

  dial.addEventListener("pointerdown", (ev) => {
    if (!session() || dialDrag || ev.button !== 0) return;
    dial.setPointerCapture(ev.pointerId);
    dialDrag = { x: ev.clientX, angle: session().settings.angle, id: ev.pointerId };
  });
  dial.addEventListener("pointermove", (ev) => {
    if (!dialDrag || ev.pointerId !== dialDrag.id) return;
    let next = dialDrag.angle - (ev.clientX - dialDrag.x) / 7;
    if (Math.abs(next) < 0.4) next = 0;
    straighten(next);
  });
  const endDial = () => { dialDrag = null; endAngle(); };
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) dial.addEventListener(type, endDial);
  dial.addEventListener("dblclick", () => { straighten(0); endAngle(); });
  dial.addEventListener("keydown", (ev) => {
    if (!["ArrowLeft", "ArrowRight", "Home"].includes(ev.key) || !session()) return;
    ev.preventDefault(); straighten(ev.key === "Home" ? 0 : session().settings.angle + (ev.key === "ArrowLeft" ? -0.5 : 0.5)); endAngle();
  });

  function refresh() {
    const s = session();
    overlay.hidden = !s; $("crop-dial-wrap").hidden = !s;
    if (!s) { drag = null; dialDrag = null; return; }
    aspect.value = s.settings.aspect;
    $("crop-custom").hidden = s.settings.aspect !== "custom";
    const portrait = s.settings.aspect === "custom" ? s.settings.custom[1] > s.settings.custom[0] : s.portrait;
    for (const [id, on] of [["crop-landscape", !portrait], ["crop-portrait", portrait]]) { $(id).setAttribute("aria-pressed", on); $(id).disabled = ["freeform", "square"].includes(s.settings.aspect); }
    for (const [i, id] of ["crop-width", "crop-height"].entries()) if (document.activeElement !== $(id)) $(id).value = s.settings.custom[i];
    angle.value = s.settings.angle; angle.style.setProperty("--f", (s.settings.angle + 45) / 90);
    if (document.activeElement !== $("crop-angle-value")) $("crop-angle-value").value = s.settings.angle.toFixed(1);
    $("crop-angle-reset").disabled = !s.settings.angle;
    const source = exportSize(), size = Crop.presentation(s.settings, s.rotation, source.width, source.height);
    $("crop-size").textContent = `${size.width} × ${size.height} px`;
    dial.setAttribute("aria-valuenow", s.settings.angle);
    $("crop-dial-value").textContent = `${s.settings.angle.toFixed(1)}°`;
    // Ruler: seven CSS pixels per degree, matching the native dial.
    const dw = dial.clientWidth, dh = 24, dpr = devicePixelRatio;
    dial.width = Math.round(dw * dpr); dial.height = Math.round(dh * dpr);
    const dc = dial.getContext("2d"); dc.scale(dpr, dpr);
    for (let i = -45; i <= 45; i++) {
      const x = dw / 2 + (i - s.settings.angle) * 7, h = i === 0 ? dh : i % 5 === 0 ? dh * 0.7 : dh * 0.4;
      dc.strokeStyle = `rgba(232,230,227,${Math.max(0, 1 - ((x - dw / 2) / (dw / 2)) ** 2) * (i % 5 === 0 ? 0.75 : 0.4)})`;
      dc.beginPath(); dc.moveTo(x, (dh - h) / 2); dc.lineTo(x, (dh + h) / 2); dc.stroke();
    }
    dc.strokeStyle = "#e0a458"; dc.lineWidth = 2; dc.beginPath(); dc.moveTo(dw / 2, 0); dc.lineTo(dw / 2, dh); dc.stroke();
    const { width, height } = view.size(); overlay.width = width; overlay.height = height;
    const ctx = overlay.getContext("2d"), { r, crop: c, dpr: d, scale } = layout();
    const polygon = Crop.corners({ x: 0, y: 0, ...s.photo }).map((p) => Crop.turn(p, s.settings.angle, { x: s.photo.width / 2, y: s.photo.height / 2 })).map((p) => ({ x: r.x + p.x * scale, y: r.y + p.y * scale }));
    ctx.beginPath(); polygon.forEach((p, i) => i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)); ctx.closePath(); ctx.rect(c.x, c.y, c.width, c.height);
    ctx.fillStyle = "rgba(0,0,0,.55)"; ctx.fill("evenodd");
    const lines = s.intended ? 9 : drag ? 3 : 0;
    ctx.strokeStyle = "rgba(255,255,255,.5)"; ctx.lineWidth = 0.5 * d; ctx.beginPath();
    for (let i = 1; i < lines; i++) { const x = c.x + c.width * i / lines, y = c.y + c.height * i / lines; ctx.moveTo(x, c.y); ctx.lineTo(x, c.y + c.height); ctx.moveTo(c.x, y); ctx.lineTo(c.x + c.width, y); }
    ctx.stroke(); ctx.strokeStyle = "white"; ctx.lineWidth = d; ctx.strokeRect(c.x, c.y, c.width, c.height);
    const length = Math.min(22 * d, c.width / 2, c.height / 2);
    ctx.lineWidth = 3 * d; ctx.shadowColor = "#000"; ctx.shadowBlur = 3 * d; ctx.beginPath();
    for (const [x, y, dx, dy] of [[c.x, c.y, 1, 1], [c.x + c.width, c.y, -1, 1], [c.x, c.y + c.height, 1, -1], [c.x + c.width, c.y + c.height, -1, -1]]) { ctx.moveTo(x + dx * length, y); ctx.lineTo(x, y); ctx.lineTo(x, y + dy * length); }
    ctx.stroke();
  }
  return { refresh };
}
