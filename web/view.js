// The photograph's view, as the desktop canvas behaves (macOS
// PreviewCanvas.swift): fitted to the viewport until zoomed; pinch,
// Ctrl/⌘- or Option-wheel zoom around the pointer, a drag or a scroll pans,
// and a double-click or double-tap toggles fit and 100%. 100% is one
// photograph pixel per device pixel, as desktop's Actual Size. A photograph
// smaller than the view is centred; a larger one can't be dragged off it.
//
// Positions are in the stage's device pixels: `rect()` is where the canvas
// draws the whole photograph.

// Desktop's magnification range, in CSS pixels per photograph pixel.
const MIN_ZOOM = 0.005, MAX_ZOOM = 16;
// Zoom per wheel pixel (macOS: per scrolling delta), and the most one event
// zooms, which a mouse wheel's notch reaches.
const WHEEL_ZOOM = 0.01, WHEEL_MAX = 50;

// `stage` holds the canvases and takes the gestures; `frame` is the view
// whose padding (safe areas) a photograph stays inside. `changed` runs
// after every change, `settled` once the view stops changing.
export function createView(stage, frame, { changed, settled }) {
  let photo = null;
  let size = { width: 1, height: 1, dpr: 1 };
  // Device pixels per photograph pixel, and the photograph's top-left corner.
  let zoom = 1, x = 0, y = 0, fitting = true;
  let settleTimer = 0;

  // Where the photograph may be: inside the view's padding. A fitted one
  // also keeps a margin from its edges.
  function bounds(margin = 0) {
    const style = getComputedStyle(frame), d = size.dpr;
    const [left, top, right, bottom] = ["Left", "Top", "Right", "Bottom"].map((s) => (parseFloat(style[`padding${s}`]) || 0) + margin);
    return {
      left: left * d,
      top: top * d,
      width: Math.max(1, size.width - (left + right) * d),
      height: Math.max(1, size.height - (top + bottom) * d),
    };
  }

  // At most 100% in CSS pixels, as before zoom came.
  function fitZoom() {
    const b = bounds(stage.clientWidth < 500 ? 12 : 32);
    return Math.min(b.width / photo.width, b.height / photo.height, size.dpr);
  }

  function clamp() {
    const b = bounds(), w = photo.width * zoom, h = photo.height * zoom;
    x = w <= b.width ? b.left + (b.width - w) / 2 : Math.min(b.left, Math.max(b.left + b.width - w, x));
    y = h <= b.height ? b.top + (b.height - h) / 2 : Math.min(b.top, Math.max(b.top + b.height - h, y));
  }

  function update() {
    if (fitting) zoom = fitZoom();
    clamp();
    cursor();
    changed();
    clearTimeout(settleTimer);
    settleTimer = setTimeout(settled, 200);
  }

  // A hand where the photograph can be dragged.
  function cursor() {
    const b = bounds();
    const pannable = photo && (photo.width * zoom > b.width + 0.5 || photo.height * zoom > b.height + 0.5);
    stage.style.cursor = pannable ? (pointers.size ? "grabbing" : "grab") : "";
  }

  // Zoom by `factor` around (px, py), keeping the photograph's point there
  // in place, and pan by (dx, dy). Panning alone keeps a fitted photograph
  // fitted (it can't move).
  function move(factor, px, py, dx = 0, dy = 0) {
    if (!photo) return;
    const z = Math.min(MAX_ZOOM * size.dpr, Math.max(MIN_ZOOM * size.dpr, zoom * factor));
    x = px - ((px - x) * z) / zoom + dx;
    y = py - ((py - y) * z) / zoom + dy;
    fitting = factor === 1 && fitting;
    zoom = z;
    update();
  }

  const center = () => { const b = bounds(); return [b.left + b.width / 2, b.top + b.height / 2]; };
  const atFit = () => fitting || Math.abs(zoom - fitZoom()) < 1e-3 * zoom;
  // Fit, or 100% around the point.
  function toggle(px, py) {
    if (!photo) return;
    if (atFit()) return move(1 / zoom, px, py);
    fitting = true;
    update();
  }

  // A pointer's position on the stage, in device pixels.
  function at(ev) {
    const box = stage.getBoundingClientRect();
    return [(ev.clientX - box.left) * size.dpr, (ev.clientY - box.top) * size.dpr];
  }

  // The stage's size in device pixels: the browser's own count where it
  // gives one that agrees with the pixel ratio (Chrome's device emulation
  // reports CSS pixels there), else the ratio's.
  function measure(css, device) {
    if (!css.width || !css.height) return;
    const ratio = devicePixelRatio;
    const fits = (n, length) => n !== undefined && Math.abs(n - length * ratio) <= 1;
    const width = fits(device?.inlineSize, css.width) ? device.inlineSize : Math.round(css.width * ratio);
    const height = fits(device?.blockSize, css.height) ? device.blockSize : Math.round(css.height * ratio);
    // A zoomed photograph keeps its point at the centre.
    const [cx, cy] = center();
    size = { width, height, dpr: width / css.width };
    if (!photo) return;
    const [nx, ny] = center();
    x += nx - cx;
    y += ny - cy;
    update();
  }
  const resized = ([entry]) => measure(entry.contentRect, entry.devicePixelContentBoxSize?.[0]);
  // Safari has no device-pixel box, and so no resize when only the pixel
  // ratio changes (another display, the page's zoom): watch the ratio too.
  try {
    new ResizeObserver(resized).observe(stage, { box: "device-pixel-content-box" });
  } catch {
    new ResizeObserver(resized).observe(stage);
  }
  (function watchRatio() {
    matchMedia(`(resolution: ${devicePixelRatio}dppx)`).addEventListener("change", () => {
      measure(stage.getBoundingClientRect());
      watchRatio();
    }, { once: true });
  })();

  // Wheel: Ctrl/⌘ (a trackpad's pinch arrives so) or Option zooms, else pans.
  stage.addEventListener("wheel", (ev) => {
    ev.preventDefault();
    if (!photo) return;
    const unit = ev.deltaMode === 1 ? 16 : ev.deltaMode === 2 ? stage.clientHeight : 1;
    if (ev.ctrlKey || ev.metaKey || ev.altKey) {
      const dy = Math.max(-WHEEL_MAX, Math.min(WHEEL_MAX, ev.deltaY * unit));
      move(Math.exp(-dy * WHEEL_ZOOM), ...at(ev));
    } else {
      const [px, py] = center();
      move(1, px, py, -ev.deltaX * unit * size.dpr, -ev.deltaY * unit * size.dpr);
    }
  }, { passive: false });

  // One pointer pans; two pinch, zooming around their middle and panning
  // with it.
  const pointers = new Map();
  let pinch = null, tap = null, lastTap = null, lastTouch = -Infinity;
  const pair = () => {
    const [a, b] = [...pointers.values()];
    return { mid: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], distance: Math.hypot(a[0] - b[0], a[1] - b[1]) };
  };
  stage.addEventListener("pointerdown", (ev) => {
    if (!photo || (ev.pointerType === "mouse" && ev.button !== 0)) return;
    stage.setPointerCapture(ev.pointerId);
    pointers.set(ev.pointerId, at(ev));
    pinch = pointers.size === 2 ? pair() : null;
    tap = pointers.size === 1 && ev.pointerType === "touch" ? { at: at(ev) } : null;
    cursor();
  });
  stage.addEventListener("pointermove", (ev) => {
    const last = pointers.get(ev.pointerId);
    if (!last) return;
    const now = at(ev);
    pointers.set(ev.pointerId, now);
    if (tap && Math.hypot(now[0] - tap.at[0], now[1] - tap.at[1]) > 10 * size.dpr) tap = null;
    if (pointers.size === 1) {
      move(1, ...now, now[0] - last[0], now[1] - last[1]);
    } else if (pinch) {
      // The point under the fingers' old middle goes to their new one.
      const next = pair();
      move(next.distance / Math.max(1, pinch.distance), ...pinch.mid, next.mid[0] - pinch.mid[0], next.mid[1] - pinch.mid[1]);
      pinch = next;
    }
  });
  const end = (ev) => {
    if (!pointers.delete(ev.pointerId)) return;
    pinch = pointers.size === 2 ? pair() : null;
    if (ev.pointerType === "touch") lastTouch = ev.timeStamp;
    // A double tap: two taps close in time and place.
    if (tap && ev.type === "pointerup") {
      const second = lastTap && ev.timeStamp - lastTap.time < 300 && Math.hypot(tap.at[0] - lastTap.at[0], tap.at[1] - lastTap.at[1]) < 30 * size.dpr;
      lastTap = second ? null : { ...tap, time: ev.timeStamp };
      if (second) toggle(...tap.at);
    }
    tap = null;
    cursor();
  };
  stage.addEventListener("pointerup", end);
  stage.addEventListener("pointercancel", end);
  // Mouse and pen; a browser may also send one after a double tap.
  stage.addEventListener("dblclick", (ev) => ev.timeStamp - lastTouch > 600 && toggle(...at(ev)));

  return {
    // A new photograph, fitted.
    setPhoto(width, height) {
      photo = { width, height };
      fitting = true;
      update();
    },
    // The same photograph at another size (a placeholder's, then the
    // developed one's): it keeps its place on the stage.
    resize(width, height) {
      if (!photo) return this.setPhoto(width, height);
      zoom *= photo.width / width;
      photo = { width, height };
      update();
    },
    fit() {
      if (!photo) return;
      fitting = true;
      update();
    },
    // 100% around the view's centre, or another zoom (device pixels per
    // photograph pixel).
    actualSize(z = 1) {
      move(z / zoom, ...center());
    },
    // The photograph's rectangle on the stage, in device pixels.
    rect: () => ({ x, y, width: photo.width * zoom, height: photo.height * zoom }),
    // The stage's size in device pixels, which the canvas takes.
    size: () => ({ width: size.width, height: size.height }),
    zoom: () => zoom,
    fitZoom: () => (photo ? fitZoom() : 1),
    fitting: () => fitting,
    atFit,
  };
}
