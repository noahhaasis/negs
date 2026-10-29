// Presentation borders, as desktop's (macOS Border.swift): their settings,
// and where each lands around the photograph. The geometry mirrors the
// exporter's, `crates/spektrafilm-io/src/border.rs` and `film35.rs`, so the
// preview at 100% puts every pixel where an export does; keep them in sync.
// Sizes are in the photograph's own pixels.

// macOS BorderSettings: Solid and Keyline share the mat's width and colour;
// widths are fractions of the photograph's long edge.
export const DEFAULT_BORDER = Object.freeze({
  enabled: false, style: "solid", export: true, instagram: false,
  width: 0.03, filmWidth: 0, line: 0.004, color: "#ffffff", lineColor: "#000000",
});

// In macOS BorderStyle's order. `id` is the exporter's name.
export const STYLES = [
  { id: "solid", title: "Solid", help: "A plain mat around the photograph." },
  { id: "film", title: "Film", help: "A scanned 120 film frame with an irregular black edge." },
  { id: "keyline", title: "Keyline", help: "A mat with a thin line around the photograph." },
  { id: "35mm", title: "35mm", help: "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph." },
];

// The 120 frame's texture, and its size (data/borders/film.png).
export const FILM_TEXTURE = { path: "borders/film.png", width: 2518, height: 1550 };

// The 35mm frame a photograph fills, by its aspect ratio within 1%
// (border.rs FilmFormat::of): "full" (36 × 24 mm, 3:2), "half" (18 × 24 mm,
// 3:4), or null.
export function filmFormat(width, height) {
  const ratio = Math.max(width, height) / Math.max(1, Math.min(width, height));
  if (Math.abs(ratio / 1.5 - 1) <= 0.01) return "full";
  if (Math.abs(ratio / (4 / 3) - 1) <= 0.01) return "half";
  return null;
}
export const fits = (style, width, height) => style !== "35mm" || filmFormat(width, height) !== null;

// The 35mm strip's preview texture for `film` (data/borders/35mm/<format>/;
// generic.png for films without an edge print).
export const stripPath = (format, film) => `borders/35mm/${format}/${film}.png`;
export const genericStrip = (format) => `borders/35mm/${format}/generic.png`;

// A photograph's aspect ratio as people name it (macOS Inspector.ratio).
export function ratioName(width, height) {
  const ratio = Math.max(width, height) / Math.min(width, height);
  const named = [[1, "1:1"], [5 / 4, "5:4"], [7 / 5, "7:5"], [16 / 9, "16:9"], [2, "2:1"], [21 / 9, "21:9"]];
  const found = named.find(([r]) => Math.abs(ratio / r - 1) <= 0.01);
  return found ? found[1] : `${ratio.toFixed(2)}:1`;
}

// The border a photograph shows, if its style fits it (macOS `shown(for:)`),
// and the one an export gets.
export const shown = (b, width, height) => (b.enabled && fits(b.style, width, height) ? b : null);
export const exported = (b, width, height) => (b.export ? shown(b, width, height) : null);

// The exporter's options (BorderOptions in border.rs; macOS `options`).
export function options(b) {
  const o = { line: b.line, color: b.color, line_color: b.lineColor };
  const width = styleWidth(b);
  if (width !== null) o.width = width;
  return o;
}
function styleWidth(b) {
  if (b.style === "solid" || b.style === "keyline") return b.width;
  if (b.style === "film") return b.filmWidth;
  return null;
}

// border.rs: margin, keyline_width, film_geometry.
const margin = (w, h, fraction) => Math.floor(Math.max(w, h) * fraction);
const keylineWidth = (w, h, fraction) => Math.max(1, Math.round(Math.max(w, h) * fraction));
function filmGeometry(w, h) {
  const short = Math.min(w, h);
  return { inset: Math.max(1, Math.round(short * 0.003)), pad: Math.max(2, Math.round(short * 0.025)) };
}

// film35.rs Strip: the strip's length along the film and its gate, in mm.
const FILM_WIDTH = 35, GATE_HEIGHT = 24, PERF_PITCH = 4.75;
const stripSize = (format) => (format === "full" ? [8 * PERF_PITCH, 36] : [4 * PERF_PITCH, 18]);
// Whether the strip turns a quarter to hold a `w` × `h` photograph.
function stripTurned(format, w, h) {
  const [, gate] = stripSize(format);
  return (gate > GATE_HEIGHT) !== (w > h);
}
// The strip scaled so its gate covers the photograph (Strip::canvas).
function stripCanvas(format, w, h) {
  const [length, gate] = stripSize(format);
  let [tw, th] = [length, FILM_WIDTH];
  let [ww, wh] = [gate / length, GATE_HEIGHT / FILM_WIDTH];
  if (stripTurned(format, w, h)) [tw, th, ww, wh] = [th, tw, wh, ww];
  const k = Math.max(w / (ww * tw), h / (wh * th));
  return [Math.round(tw * k), Math.round(th * k)];
}

// The 3:4 canvas Instagram shows whole around a `w` × `h` bordered canvas
// (border.rs instagram_canvas): a white margin around the film borders
// first, then the short way as little as it can.
function instagramCanvas(w, h, style) {
  const m = style === "film" || style === "35mm" ? margin(w, h, 0.03) : 0;
  const [cw, ch] = [w + 2 * m, h + 2 * m];
  return cw * 4 > ch * 3 ? [cw, Math.ceil((cw * 4) / 3)] : [Math.ceil((ch * 3) / 4), ch];
}

const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const BLACK = [0, 0, 0], WHITE = [1, 1, 1];

// Where border `b` puts everything around a `w` × `h` photograph, in the
// final canvas's pixels: its size, the photograph's top-left corner, and
// what the view draws. `texture` is the texture's { width, height } when it
// has arrived; without the strip's, the 35mm canvas is the photograph alone
// (as macOS shows it until the strip loads). Rectangles are [x, y, w, h].
export function layout(b, w, h, texture = null) {
  let width, height, x, y, frame, line = null, patch = null;
  if (b.style === "solid" || b.style === "keyline") {
    const m = margin(w, h, b.width);
    const l = b.style === "keyline" ? keylineWidth(w, h, b.line) : 0;
    [width, height, x, y] = [w + 2 * (m + l), h + 2 * (m + l), m + l, m + l];
    if (l) line = [m, m, w + 2 * l, h + 2 * l];
  } else if (b.style === "film") {
    const { inset, pad } = filmGeometry(w, h);
    const extra = margin(w, h, b.filmWidth);
    const [iw, ih] = [w + 2 * inset, h + 2 * inset];
    [width, height, x, y] = [iw + 2 * extra, ih + 2 * extra, inset + extra, inset + extra];
    // Laid out landscape, turned for a portrait photograph; corners keep
    // `pad` pixels (overlay_nine_patch).
    if (texture) {
      const scale = Math.min(iw, ih) / Math.min(texture.width, texture.height);
      patch = { kind: 1, rect: [extra, extra, iw, ih], turned: h > w, pad, scale };
    }
  } else {
    const format = filmFormat(w, h);
    if (texture && format) {
      [width, height] = stripCanvas(format, w, h);
      [x, y] = [Math.floor((width - w) / 2), Math.floor((height - h) / 2)];
      patch = { kind: 2, rect: [0, 0, width, height], turned: stripTurned(format, w, h), pad: 0, scale: 0 };
    } else {
      [width, height, x, y] = [w, h, 0, 0];
    }
  }
  frame = [0, 0, width, height];
  const film = b.style === "film" || b.style === "35mm";
  const mat = film ? BLACK : rgb(b.color);
  let fill = mat;
  if (b.instagram) {
    const [iw, ih] = instagramCanvas(width, height, b.style);
    const [dx, dy] = [Math.floor((iw - width) / 2), Math.floor((ih - height) / 2)];
    const move = (r) => r && [r[0] + dx, r[1] + dy, r[2], r[3]];
    [frame, line] = [move(frame), move(line)];
    if (patch) patch.rect = move(patch.rect);
    [width, height, x, y] = [iw, ih, x + dx, y + dy];
    fill = film ? WHITE : mat;
  }
  return { width, height, x, y, frame, line, patch, fill, mat, lineColor: rgb(b.lineColor) };
}

// A layout as the engine's view takes it (BorderView in wgpu_backend.rs):
// 36 numbers, rectangles from the photograph's top-left corner.
export function viewOf(l, w, h) {
  const rect = (r) => (r ? [r[0] - l.x, r[1] - l.y, r[2], r[3]] : [0, 0, 0, 0]);
  const p = l.patch;
  return [
    w, h, 0, 0,
    ...rect([0, 0, l.width, l.height]),
    ...rect(l.frame),
    ...rect(l.line),
    ...l.fill, 1, ...l.mat, 1, ...l.lineColor, 1,
    ...rect(p?.rect),
    p ? p.kind : 0, p?.turned ? 1 : 0, p?.pad ?? 0, p?.scale ?? 0,
  ];
}

// The same border drawn with 2D canvas calls (the page's own canvas, for
// the CPU engine): the canvas `l` around photograph rectangle `r` (device
// pixels), then, once `draw` has put the photograph there, the texture over
// it. `image` is the texture's bitmap.
export function paintUnder(ctx, l, r, w) {
  const s = r.width / w;
  const at = (q) => [r.x + (q[0] - l.x) * s, r.y + (q[1] - l.y) * s, q[2] * s, q[3] * s];
  const fill = (c, q) => {
    ctx.fillStyle = `rgb(${c.map((v) => Math.round(v * 255)).join(" ")})`;
    ctx.fillRect(...at(q));
  };
  fill(l.fill, [0, 0, l.width, l.height]);
  fill(l.mat, l.frame);
  if (l.line) fill(l.lineColor, l.line);
}
export function paintOver(ctx, l, r, w, image) {
  const p = l.patch;
  if (!p || !image) return;
  const s = r.width / w;
  const [x, y, pw, ph] = [r.x + (p.rect[0] - l.x) * s, r.y + (p.rect[1] - l.y) * s, p.rect[2] * s, p.rect[3] * s];
  ctx.save();
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  // A turned texture: the 120 frame rotate270'd (a quarter turn
  // anticlockwise), the strip as film35.rs's Renderer turns it, the same.
  ctx.translate(x, y);
  let [dw, dh] = [pw, ph];
  if (p.turned) {
    ctx.translate(0, ph);
    ctx.rotate(-Math.PI / 2);
    [dw, dh] = [ph, pw];
  }
  if (p.kind === 2) {
    ctx.drawImage(image, 0, 0, dw, dh);
  } else {
    // Nine patches: corners `pad` canvas pixels, edges and centre stretched.
    const [tw, th] = [image.width, image.height];
    const pad = p.pad * s, tp = p.pad / p.scale;
    const xs = [[0, tp, 0, pad], [tp, tw - 2 * tp, pad, dw - 2 * pad], [tw - tp, tp, dw - pad, pad]];
    const ys = [[0, tp, 0, pad], [tp, th - 2 * tp, pad, dh - 2 * pad], [th - tp, tp, dh - pad, pad]];
    for (const [sx, sw, ex, ew] of xs) {
      for (const [sy, sh, ey, eh] of ys) {
        if (sw > 0 && sh > 0 && ew > 0 && eh > 0) ctx.drawImage(image, sx, sy, sw, sh, ex, ey, ew, eh);
      }
    }
  }
  ctx.restore();
}
