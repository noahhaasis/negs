// Presentation geometry, ported from macOS Crop.swift and io/src/crop.rs.
// Coordinates are y-down; settings stay in the unrotated frame. Neither a
// crop nor a turn changes what the film develops.
export const FULL = { x: 0, y: 0, width: 1, height: 1 };
export const defaults = () => ({ ...FULL, angle: 0, aspect: "original", custom: [5, 4] });
export const ASPECTS = [["original", "Original"], ["freeform", "Freeform"], ["square", "Square · 6×6"],
  ["16:9", "16:9"], ["3:2", "3:2 · 35mm"], ["4:3", "4:3 · 645"], ["5:4", "5:4 · 4×5"], ["7:5", "7:5 · 5×7"], ["custom", "Custom"]];
export const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), Math.max(lo, hi));
export const turns = (n) => ((n % 4) + 4) % 4;
const right = (r) => r.x + r.width, bottom = (r) => r.y + r.height;
const center = (r) => ({ x: r.x + r.width / 2, y: r.y + r.height / 2 });
export const corners = (r) => [{ x: r.x, y: r.y }, { x: right(r), y: r.y }, { x: right(r), y: bottom(r) }, { x: r.x, y: bottom(r) }];
export function shown(rect, rotation) {
  let r = { ...rect };
  for (let i = 0; i < turns(rotation); i++) r = { x: 1 - r.y - r.height, y: r.x, width: r.height, height: r.width };
  return r;
}
export const unrotated = (r, rotation) => shown(r, -rotation);
export const turnedSize = (w, h, rotation) => turns(rotation) % 2 ? { width: h, height: w } : { width: w, height: h };
export function pixels(c, w, h) {
  if (c.angle) return { x: c.x * w, y: c.y * h, width: Math.max(1, Math.round(c.width * w)), height: Math.max(1, Math.round(c.height * h)) };
  const edge = (f, n) => clamp(Math.round(f * n), 0, n);
  const x1 = Math.min(Math.max(edge(right(c), w), edge(c.x, w) + 1), w);
  const y1 = Math.min(Math.max(edge(bottom(c), h), edge(c.y, h) + 1), h);
  const x0 = Math.min(edge(c.x, w), x1 - 1), y0 = Math.min(edge(c.y, h), y1 - 1);
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}
export function turn(p, angle, c) {
  const a = angle * Math.PI / 180, s = Math.sin(a), co = Math.cos(a), x = p.x - c.x, y = p.y - c.y;
  return { x: c.x + x * co - y * s, y: c.y + x * s + y * co };
}
export function bounds(photo, angle) {
  const ps = corners({ x: 0, y: 0, ...photo }).map((p) => turn(p, angle, { x: photo.width / 2, y: photo.height / 2 }));
  const xs = ps.map((p) => p.x), ys = ps.map((p) => p.y), x = Math.min(...xs), y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}
export function fits(r, angle, photo) {
  const c = { x: photo.width / 2, y: photo.height / 2 }, eps = 1e-6 * Math.max(photo.width, photo.height, 1);
  return corners(r).every((p) => { const q = turn(p, -angle, c); return q.x >= -eps && q.y >= -eps && q.x <= photo.width + eps && q.y <= photo.height + eps; });
}
export function constrain(start, target, angle, photo) {
  if (fits(target, angle, photo)) return target;
  if (!fits(start, angle, photo)) return start;
  const lerp = (t) => Object.fromEntries(["x", "y", "width", "height"].map((k) => [k, start[k] + (target[k] - start[k]) * t]));
  let lo = 0, hi = 1;
  for (let i = 0; i < 24; i++) { const mid = (lo + hi) / 2; if (fits(lerp(mid), angle, photo)) lo = mid; else hi = mid; }
  return lerp(lo);
}
export function inscribe(r, angle, photo) {
  const c = { x: photo.width / 2, y: photo.height / 2 }, mid = center(r);
  const half = corners({ x: -r.width / 2, y: -r.height / 2, width: r.width, height: r.height }).map((p) => turn(p, -angle, { x: 0, y: 0 }));
  const candidates = Array.from({ length: 21 }, (_, i) => {
    const m = { x: mid.x + (c.x - mid.x) * i / 20, y: mid.y + (c.y - mid.y) * i / 20 }, q = turn(m, -angle, c);
    let s = q.x < 0 || q.y < 0 || q.x > photo.width || q.y > photo.height ? 0 : 1;
    for (const v of half) {
      if (v.x > 0) s = Math.min(s, (photo.width - q.x) / v.x); else if (v.x < 0) s = Math.min(s, q.x / -v.x);
      if (v.y > 0) s = Math.min(s, (photo.height - q.y) / v.y); else if (v.y < 0) s = Math.min(s, q.y / -v.y);
    }
    return { m, s: Math.max(s, 0) };
  });
  const best = Math.max(...candidates.map((v) => v.s)), chosen = candidates.find((v) => v.s >= best * 0.98);
  if (!best) return r;
  const { m, s } = chosen;
  return { x: m.x - r.width * s / 2, y: m.y - r.height * s / 2, width: r.width * s, height: r.height * s };
}
export function move(r, dx, dy, b) {
  return { ...r, x: clamp(r.x + dx, b.x, right(b) - r.width), y: clamp(r.y + dy, b.y, bottom(b) - r.height) };
}
export function resize(r, handle, point, ratio, b, minimum) {
  const [hx, hy] = handle, p = { x: clamp(point.x, b.x, right(b)), y: clamp(point.y, b.y, bottom(b)) };
  const anchor = { x: hx < 0 ? right(r) : r.x, y: hy < 0 ? bottom(r) : r.y };
  const room = { width: hx > 0 ? right(b) - anchor.x : hx < 0 ? anchor.x - b.x : b.width,
    height: hy > 0 ? bottom(b) - anchor.y : hy < 0 ? anchor.y - b.y : b.height };
  const pulled = { width: Math.max(hx * (p.x - anchor.x), 0), height: Math.max(hy * (p.y - anchor.y), 0) };
  let { width, height } = r;
  if (ratio) { width = Math.min(Math.max(hx ? pulled.width : 0, hy ? pulled.height * ratio : 0, minimum, minimum * ratio), room.width, room.height * ratio); height = width / ratio; }
  else { if (hx) width = clamp(pulled.width, Math.min(minimum, room.width), room.width); if (hy) height = clamp(pulled.height, Math.min(minimum, room.height), room.height); }
  const origin = (h, a, n, mid, lo, hi) => h > 0 ? a : h < 0 ? a - n : clamp(mid - n / 2, lo, hi - n);
  return { x: origin(hx, anchor.x, width, center(r).x, b.x, right(b)), y: origin(hy, anchor.y, height, center(r).y, b.y, bottom(b)), width, height };
}
export function handleAt(p, r, touch = false) {
  const edge = touch ? 18 : 8, reach = Math.min(touch ? 28 : 18, r.width / 3, r.height / 3);
  if (p.x < r.x - edge || p.x > right(r) + edge || p.y < r.y - edge || p.y > bottom(r) + edge) return null;
  const side = (v, lo, hi, tolerance) => Math.abs(v - lo) <= tolerance ? -1 : Math.abs(v - hi) <= tolerance ? 1 : 0;
  const x = side(p.x, r.x, right(r), reach), y = side(p.y, r.y, bottom(r), reach);
  return x && y ? [x, y] : [side(p.x, r.x, right(r), edge), side(p.y, r.y, bottom(r), edge)];
}

export class Session {
  constructor(settings, width, height, rotation) {
    this.settings = structuredClone(settings); this.rotation = rotation; this.source = { width, height };
    this.portrait = this.shown.height > this.shown.width; this.intended = null;
    if (this.settings.aspect === "custom" && (this.settings.custom[1] > this.settings.custom[0]) !== this.portrait) this.settings.custom.reverse();
  }
  get photo() { return turnedSize(this.source.width, this.source.height, this.rotation); }
  get shown() { const r = shown(this.settings, this.rotation), p = this.photo; return { x: r.x * p.width, y: r.y * p.height, width: r.width * p.width, height: r.height * p.height }; }
  set shown(r) { const p = this.photo; Object.assign(this.settings, unrotated({ x: r.x / p.width, y: r.y / p.height, width: r.width / p.width, height: r.height / p.height }, this.rotation)); }
  get ratio() {
    const s = this.settings;
    if (s.aspect === "freeform") return null;
    if (s.aspect === "custom") return s.custom[0] / s.custom[1];
    const ratio = s.aspect === "original" ? Math.max(this.photo.width, this.photo.height) / Math.min(this.photo.width, this.photo.height)
      : s.aspect === "square" ? 1 : s.aspect.split(":").map(Number).reduce((a, b) => a / b);
    return this.portrait ? 1 / ratio : ratio;
  }
  refit() {
    if (!this.ratio) return;
    const p = this.photo, mid = center(this.shown), width = Math.min(p.width, p.height * this.ratio), height = width / this.ratio;
    this.shown = inscribe({ x: clamp(mid.x - width / 2, 0, p.width - width), y: clamp(mid.y - height / 2, 0, p.height - height), width, height }, this.settings.angle, p);
  }
  choose(aspect) { this.settings.aspect = aspect; if (aspect === "original") this.portrait = this.photo.height > this.photo.width; this.refit(); }
  orient(portrait) {
    this.portrait = portrait;
    if (this.settings.aspect === "custom" && (this.settings.custom[1] > this.settings.custom[0]) !== portrait) this.settings.custom.reverse();
    this.refit();
  }
  rotate(n) { this.rotation = turns(this.rotation + n); if (turns(n) % 2) { this.portrait = !this.portrait; this.settings.custom.reverse(); } this.intended = null; }
  straighten(angle) { this.intended ??= this.shown; this.settings.angle = clamp(Math.sign(angle) * Math.round(Math.abs(angle) * 10) / 10, -45, 45); this.shown = inscribe(this.intended, this.settings.angle, this.photo); }
  reset() { this.settings = { ...defaults(), custom: [...this.settings.custom] }; this.portrait = this.photo.height > this.photo.width; this.intended = null; }
  committed() {
    const s = structuredClone(this.settings);
    if (!s.angle && Math.abs(s.x) < 1e-3 && Math.abs(s.y) < 1e-3 && Math.abs(right(s) - 1) < 1e-3 && Math.abs(bottom(s) - 1) < 1e-3) Object.assign(s, FULL);
    return s;
  }
}

// Inverse affine map: normalized displayed rectangle -> normalized source.
// The GPU uses these same rows; the CPU canvas uses their inverse. In crop
// mode the whole photo shows, without clipping at its unturned bounds.
export function presentation(crop, rotation, w, h, editing = false) {
  const r = editing ? { x: 0, y: 0, width: w, height: h } : pixels(crop, w, h);
  const size = turnedSize(r.width, r.height, rotation);
  const at = (x, y) => {
    let p = { x, y };
    for (let i = 0; i < turns(rotation); i++) p = { x: p.y, y: 1 - p.x };
    return turn({ x: r.x + p.x * r.width, y: r.y + p.y * r.height }, -crop.angle, { x: w / 2, y: h / 2 });
  };
  const o = at(0, 0), a = at(1, 0), b = at(0, 1);
  const matrix = [(a.x - o.x) / w, (b.x - o.x) / w, o.x / w, 0, (a.y - o.y) / h, (b.y - o.y) / h, o.y / h, 0,
    size.width / w, size.height / h, editing ? 0 : 1, crop.angle ? 1 : 0];
  return { ...size, matrix };
}
export function paint(ctx, bitmap, rect, geometry) {
  const m = geometry.matrix, [a, b, c, , d, e, f] = m, det = a * e - b * d;
  const w = bitmap.width * m[8], h = bitmap.height * m[9];
  const exact = !m[11] && Math.abs(rect.width - w) <= 1 && Math.abs(rect.height - h) <= 1;
  const r = exact ? { x: Math.round(rect.x + (rect.width - w) / 2), y: Math.round(rect.y + (rect.height - h) / 2), width: w, height: h } : rect;
  ctx.save();
  if (m[10]) { ctx.beginPath(); ctx.rect(r.x, r.y, r.width, r.height); ctx.clip(); }
  ctx.imageSmoothingEnabled = !exact; ctx.imageSmoothingQuality = "high";
  ctx.transform(r.width * e / det / bitmap.width, -r.height * d / det / bitmap.width,
    -r.width * b / det / bitmap.height, r.height * a / det / bitmap.height,
    r.x + r.width * (b * f - e * c) / det, r.y + r.height * (d * c - a * f) / det);
  ctx.drawImage(bitmap, 0, 0); ctx.restore();
}

// Finishing the web export's display bytes, in the worker. Matches native
// Crop::apply's edge/side rounding, inverse pixel-centre map, bilinear
// sampling and repeated edge pixels; quarter turns are lossless.
export function applyRgba(rgba, width, height, crop, rotation) {
  if (!turns(rotation) && !crop.angle && crop.x === 0 && crop.y === 0 && crop.width === 1 && crop.height === 1) return { width, height, rgba };
  const g = presentation(crop, rotation, width, height), m = g.matrix;
  const out = new Uint8Array(g.width * g.height * 4);
  if (!crop.angle) {
    const r = pixels(crop, width, height), n = turns(rotation);
    if (!n) {
      for (let y = 0; y < r.height; y++) { const i = ((y + r.y) * width + r.x) * 4; out.set(rgba.subarray(i, i + r.width * 4), y * r.width * 4); }
    } else {
      for (let y = 0; y < g.height; y++) for (let x = 0; x < g.width; x++) {
        const sx = n === 1 ? y : n === 2 ? r.width - 1 - x : r.width - 1 - y;
        const sy = n === 1 ? r.height - 1 - x : n === 2 ? r.height - 1 - y : x;
        const src = ((r.y + sy) * width + r.x + sx) * 4, dst = (y * g.width + x) * 4;
        out[dst] = rgba[src]; out[dst + 1] = rgba[src + 1]; out[dst + 2] = rgba[src + 2]; out[dst + 3] = rgba[src + 3];
      }
    }
    return { width: g.width, height: g.height, rgba: out };
  }
  for (let y = 0; y < g.height; y++) for (let x = 0; x < g.width; x++) {
    const u = (x + 0.5) / g.width, v = (y + 0.5) / g.height;
    const sx = clamp((m[0] * u + m[1] * v + m[2]) * width - 0.5, 0, width - 1);
    const sy = clamp((m[4] * u + m[5] * v + m[6]) * height - 0.5, 0, height - 1);
    const x0 = Math.floor(sx), y0 = Math.floor(sy), x1 = Math.min(x0 + 1, width - 1), y1 = Math.min(y0 + 1, height - 1), fx = sx - x0, fy = sy - y0;
    const dest = (y * g.width + x) * 4;
    for (let c = 0; c < 3; c++) {
      const top = rgba[(y0 * width + x0) * 4 + c] * (1 - fx) + rgba[(y0 * width + x1) * 4 + c] * fx;
      const low = rgba[(y1 * width + x0) * 4 + c] * (1 - fx) + rgba[(y1 * width + x1) * 4 + c] * fx;
      out[dest + c] = Math.round(top * (1 - fy) + low * fy);
    }
    out[dest + 3] = 255;
  }
  return { width: g.width, height: g.height, rgba: out };
}
