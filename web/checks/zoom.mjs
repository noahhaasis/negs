// Zoom and pan (web/view.js): opens a photograph (PHOTO, default the 24 MP
// Sony ARW) and reports, with Chrome's GPU memory after each step
// (nvidia-smi; 0 without an NVIDIA GPU):
//  - the fitted frame, the status line's "shown at" notice, and whether the
//    canvas draws the frame 1:1;
//  - Actual Size: the frame it renders (all the pixels, within the device's
//    cap) and the notice;
//  - wheel pans at 100%: each display pass's time, and that none rendered;
//  - the canvas at 100% against an export render of the same edit (under
//    the cap, the frame at its own size against a render at that size),
//    pixel for pixel, where the canvas shows it;
//  - zooming out to 25% (drawn from mip levels, no render), Ctrl-wheel
//    zooming back in (no render), a pinch by touch, and Fit (renders the
//    fitted size again).
//   node web/cdp.mjs http://localhost:8137/ web/checks/zoom.mjs 3000
//   VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/zoom.mjs 3000   (a phone)
import { execFileSync } from "node:child_process";

const gpuMiB = () => {
  try {
    const pids = new Set(execFileSync("pgrep", ["-f", "chrome.*--type=gpu-process"]).toString().trim().split("\n"));
    const rows = execFileSync("nvidia-smi", ["--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"]).toString().trim().split("\n");
    return rows.map((r) => r.split(",").map((s) => s.trim())).filter(([pid]) => pids.has(pid)).reduce((sum, [, mib]) => sum + Number(mib), 0);
  } catch {
    return 0;
  }
};

export default async ({ send, evaluate }) => {
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    // The next frame after the n-th and the engine idle (a view settles 200 ms after its last change).
    globalThis.settle = async (n) => { await wait(300); for (let i = 0; i < 2400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(50); return negs.renders.at(-1); };
    globalThis.quiet = async () => { await wait(400); for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25); };
    globalThis.stage = document.getElementById("stage");
    globalThis.wheel = (dx, dy, opts = {}) => { const r = stage.getBoundingClientRect();
      stage.dispatchEvent(new WheelEvent("wheel", { deltaX: dx, deltaY: dy, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, bubbles: true, cancelable: true, ...opts })); };
    globalThis.frameOf = (r) => r && { frame: r.width + "x" + r.height, ms: Math.round(r.elapsedMs), presented: r.presented, wasmMiB: r.memoryMiB, stages: r.stages };
    globalThis.statusLine = () => document.getElementById("hud").textContent;
    globalThis.stats = (xs) => { const s = [...xs].sort((a, b) => a - b), q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))]?.toFixed(1);
      return { n: s.length, median: q(0.5), p90: q(0.9), max: s.at(-1)?.toFixed(1) }; };
    // The canvas where it shows the frame against \`bytes\` (the frame read back, its size), pixel for pixel.
    globalThis.compareCanvas = async (bytes) => {
      const [width, height, left, top, dw, dh] = await negs.shown();
      if (bytes.width !== width || bytes.height !== height) return { error: "sizes differ", shown: [width, height], bytes: [bytes.width, bytes.height] };
      const shown = await createImageBitmap(document.getElementById("gpu-canvas"));
      const ctx = new OffscreenCanvas(shown.width, shown.height).getContext("2d");
      ctx.drawImage(shown, 0, 0);
      const [x0, y0] = [Math.max(0, left), Math.max(0, top)];
      const [w, h] = [Math.min(shown.width, left + width) - x0, Math.min(shown.height, top + height) - y0];
      const seen = ctx.getImageData(x0, y0, w, h).data;
      let same = 0, most = 0;
      for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
        const a = (y * w + x) * 4, b = ((y + y0 - top) * width + (x + x0 - left)) * 4;
        const d = Math.max(Math.abs(seen[a] - bytes.rgba[b]), Math.abs(seen[a + 1] - bytes.rgba[b + 1]), Math.abs(seen[a + 2] - bytes.rgba[b + 2]));
        same += d === 0; most = Math.max(most, d);
      }
      return { oneToOne: dw === width && dh === height, at: [left, top], compared: w + "x" + h, exact: (100 * same / (w * h)).toFixed(3) + "%", max: most };
    };`);
  const out = { gpuStartMiB: gpuMiB() };
  const photo = process.env.PHOTO ?? "results/sony.arw";

  // Fitted.
  out.fit = await evaluate(`
    for (let i = 0; i < 150 && !document.querySelector("#engine-info").textContent.includes("·"); i++) await wait(100);
    const n = negs.renders.length;
    await negs.open(${JSON.stringify(photo)});
    const r = await settle(n);
    const shown = await negs.shown();
    return { ...frameOf(r), status: statusLine(), zoom: document.getElementById("zoom-level").textContent, canvas: shown.length ? { size: shown.slice(0, 2), at: shown.slice(2) } : "not presented",
      device: [stage.clientWidth, stage.clientHeight, devicePixelRatio], coarse: matchMedia("(pointer: coarse)").matches };`);
  out.fit.gpuMiB = gpuMiB();

  // Actual Size renders the photograph's pixels.
  out.actual = await evaluate(`
    const n = negs.renders.length;
    document.getElementById("actual").click();
    const r = await settle(n);
    return { ...frameOf(r), renders: negs.renders.length - n, status: statusLine(), zoom: document.getElementById("zoom-level").textContent };`);
  out.actual.gpuMiB = gpuMiB();

  // Pans by wheel: display passes, no renders.
  out.pan = await evaluate(`
    await quiet();
    const n = negs.renders.length, v = negs.views.length, started = performance.now();
    for (let i = 0; i < 60; i++) { wheel(i % 20 < 10 ? 40 : -40, i % 30 < 15 ? 25 : -25); await new Promise(requestAnimationFrame); }
    const elapsed = performance.now() - started;
    await quiet();
    return { wheelEvents: 60, overMs: Math.round(elapsed), passes: negs.views.length - v, renders: negs.renders.length - n, passMs: stats(negs.views.slice(v)) };`);

  // 100% shows the export's pixels. Under the device's cap, the frame at
  // its own size (zoomed to it, which renders nothing) shows those of a
  // render at that size.
  out.actualVsExport = await evaluate(`
    const side = negs.renderSide(), long = Math.max(negs.photo().width, negs.photo().height), n = negs.renders.length;
    if (side < long) { negs.view.actualSize(side / long); await quiet(); }
    const r = await negs.read(side);
    return { against: (side === long ? "export " : "capped frame ") + r.width + "x" + r.height, renders: negs.renders.length - n, ...(await compareCanvas(r)) };`);
  out.actualVsExport.gpuMiB = gpuMiB();

  // Zoomed out to 25%: sampled from mip levels, without rendering.
  out.quarter = await evaluate(`
    await quiet();
    const n = negs.renders.length, v = negs.views.length;
    negs.view.actualSize(0.25 * devicePixelRatio / devicePixelRatio);
    await quiet();
    const shown = await negs.shown();
    return { renders: negs.renders.length - n, passMs: negs.views.slice(v).map((x) => x.toFixed(1)), drawn: shown.slice(4).map(Math.round), zoom: document.getElementById("zoom-level").textContent };`);

  // Ctrl-wheel back in around the middle: no render while the frame has the pixels.
  out.wheelZoom = await evaluate(`
    const n = negs.renders.length;
    for (let i = 0; i < 6; i++) { wheel(0, -50, { ctrlKey: true }); await new Promise(requestAnimationFrame); }
    await quiet();
    return { zoom: document.getElementById("zoom-level").textContent, renders: negs.renders.length - n };`);

  // A pinch by touch (on a touch screen), out around the middle.
  if (process.env.VIEW?.endsWith("m")) {
    const c = await evaluate(`const r = stage.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, zoom: negs.view.zoom() };`);
    const touch = (type, points) => send("Input.dispatchTouchEvent", { type, touchPoints: points.map(([x, y], id) => ({ x, y, id })) });
    await touch("touchStart", [[c.x - 120, c.y], [c.x + 120, c.y]]);
    for (let i = 1; i <= 10; i++) { await touch("touchMove", [[c.x - 120 + 8 * i, c.y], [c.x + 120 - 8 * i, c.y]]); await new Promise((r) => setTimeout(r, 16)); }
    await touch("touchEnd", []);
    out.pinch = await evaluate(`await quiet(); return { from: ${c.zoom}, to: negs.view.zoom(), ratio: +(negs.view.zoom() / ${c.zoom}).toFixed(3), zoom: document.getElementById("zoom-level").textContent };`);
  }

  // Fit: the fitted size again, drawn 1:1.
  out.refit = await evaluate(`
    const n = negs.renders.length;
    document.getElementById("fit").click();
    const r = await settle(n);
    const shown = await negs.shown();
    return { ...frameOf(r), status: statusLine(), zoom: document.getElementById("zoom-level").textContent,
      oneToOne: shown.length ? shown[0] === shown[4] && shown[1] === shown[5] : "not presented" };`);
  out.refit.gpuMiB = gpuMiB();
  return out;
};
