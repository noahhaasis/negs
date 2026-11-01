// Borders (piece 11; web/border.js, the view's border pass, the export's
// `addBorder`). Reports:
//  - geometry: for a grid of photograph sizes, every style with and without
//    Instagram, the canvas size and the photograph's offset web/border.js
//    lays out against what the engine's add_border makes (an image whose
//    pixels name their coordinates, so the output's centre says where the
//    photograph went);
//  - for each photograph (PHOTOS, default the card, the Sony ARW and a 3:4
//    crop of the card, web/results/card-34.png): each style with and
//    without Instagram (35mm only where it fits, and its button off where
//    it doesn't) changed at fit: the display passes' times and that nothing
//    rendered; the export's size and offset against the layout, and its
//    bytes and the unbordered ones saved as
//    web/results/border-<photo>-<style>[-ig].ppm and border-<photo>.ppm for
//    web/checks/border-native.sh; the wasm memory after the export;
//  - for Solid, Keyline and Instagram, the canvas at 100% against the
//    export's bytes over everything it shows of the canvas, at its top-left
//    and bottom-right corners; for Film and 35mm, screenshots at fit and at
//    100% (SHOTS=1, into web/results/);
//  - which border textures downloaded, and how often.
//   node web/cdp.mjs http://localhost:8137/ web/checks/border.mjs 3000
//   QUICK=1 VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/border.mjs 3000   (a phone: the display passes only)
import { writeFileSync } from "node:fs";

const STYLES = ["solid", "keyline", "film", "35mm"];
const OPTIONS = { width: 0.071, filmWidth: 0.05, line: 0.013, color: "#204060", lineColor: "#e0a458" };

export default async ({ send, evaluate }) => {
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.settle = async (n) => { await wait(300); for (let i = 0; i < 4800 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(50); return negs.renders.at(-1); };
    globalThis.quiet = async () => { await wait(400); for (let i = 0; i < 4800 && !negs.idle(); i++) await wait(25); };
    globalThis.stage = document.getElementById("stage");
    globalThis.wheel = (dx, dy) => { const r = stage.getBoundingClientRect();
      stage.dispatchEvent(new WheelEvent("wheel", { deltaX: dx, deltaY: dy, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, bubbles: true, cancelable: true })); };
    globalThis.stats = (xs) => { const s = [...xs].sort((a, b) => a - b), q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))]?.toFixed(1);
      return { n: s.length, median: q(0.5), max: s.at(-1)?.toFixed(1) }; };
    globalThis.textureLoads = () => performance.getEntriesByType("resource").filter((e) => e.name.includes("/data/borders/")).map((e) => e.name.split("/data/")[1]);
    // The border's texture, when it needs one, then the engine idle.
    globalThis.bordered = async (settings) => {
      const { style } = negs.border(settings);
      for (let i = 0; i < 1200 && ["film", "35mm"].includes(style) && !negs.borderTexture(); i++) await wait(25);
      await quiet();
    };
    // The engine's canvas against \`bytes\` (the bordered export) over all it
    // shows of the bordered canvas, pixel for pixel.
    globalThis.compareCanvas = async (bytes) => {
      const l = negs.borderLayout(), r = negs.view.rect();
      // The photograph's frame lands at whole pixels at 100%, and the border with it.
      const [left, top] = [Math.round(r.x) - l.x, Math.round(r.y) - l.y];
      const shown = await createImageBitmap(document.getElementById("gpu-canvas"));
      const ctx = new OffscreenCanvas(shown.width, shown.height).getContext("2d");
      ctx.drawImage(shown, 0, 0);
      const [x0, y0] = [Math.max(0, left), Math.max(0, top)];
      const [w, h] = [Math.min(shown.width, left + bytes.width) - x0, Math.min(shown.height, top + bytes.height) - y0];
      const seen = ctx.getImageData(x0, y0, w, h).data;
      // Apart: what a texture covers, which the preview draws from desktop's
      // textures, not the export's (docs/web-wasm.md item 18).
      const t = l.patch?.rect, photo = negs.photo();
      const parts = { flat: { n: 0, same: 0, max: 0, border: 0 }, texture: { n: 0, same: 0, max: 0, border: 0 } };
      for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
        const a = (y * w + x) * 4, cx = x + x0 - left, cy = y + y0 - top, b = (cy * bytes.width + cx) * 4;
        const d = Math.max(Math.abs(seen[a] - bytes.rgba[b]), Math.abs(seen[a + 1] - bytes.rgba[b + 1]), Math.abs(seen[a + 2] - bytes.rgba[b + 2]));
        const part = parts[t && cx >= t[0] && cx < t[0] + t[2] && cy >= t[1] && cy < t[1] + t[3] ? "texture" : "flat"];
        part.n++; part.same += d === 0; part.max = Math.max(part.max, d);
        part.border += !(cx >= l.x && cx < l.x + photo.width && cy >= l.y && cy < l.y + photo.height);
      }
      const report = (p) => p.n ? { pixels: p.n, borderPixels: p.border, exact: (100 * p.same / p.n).toFixed(3) + "%", max: p.max } : undefined;
      return { at: [left, top], compared: w + "x" + h, flat: report(parts.flat), texture: report(parts.texture) };
    };
    for (let i = 0; i < 150 && !document.querySelector("#engine-info").textContent.includes("·"); i++) await wait(100);
    negs.pane("post");`);
  const out = { texturesAtStart: await evaluate(`return textureLoads();`) };
  const quick = !!process.env.QUICK;

  // The layout against the engine, on images whose pixels name their place.
  if (!quick) {
    out.geometry = await evaluate(`
      const sizes = [[2752, 1536], [1536, 2752], [1000, 1000], [3000, 2000], [2000, 3000], [1200, 1600], [1600, 1200], [1152, 1536],
        [301, 400], [997, 661], [661, 997], [64, 96], [1, 500], [500, 1], [7, 5], [1, 1]];
      const { layout, options, fits } = await import("./border.js");
      const DEFAULT = negs.border({ enabled: false });
      const rows = [], bad = [];
      for (const [w, h] of sizes) {
        const rgba = new Uint8Array(w * h * 4);
        for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
          const i = (y * w + x) * 4;
          rgba.set([x & 255, y & 255, ((x >> 8) & 15) | (((y >> 8) & 15) << 4), 255], i);
        }
        for (const style of ${JSON.stringify(STYLES)}) {
          if (!fits(style, w, h)) continue;
          for (const custom of [false, true]) for (const instagram of [false, true]) {
            const b = { ...DEFAULT, enabled: true, style, instagram, ...(custom ? ${JSON.stringify(OPTIONS)} : {}) };
            // The strip's texture size doesn't matter to the canvas, only that it's there.
            const l = layout(b, w, h, { width: 2518, height: 1550 });
            const r = await negs.addBorder(rgba, w, h, style, JSON.stringify(options(b)), instagram, "kodak_portra_400");
            const [cx, cy] = [Math.floor(r.width / 2), Math.floor(r.height / 2)];
            const i = (cy * r.width + cx) * 4;
            const [sx, sy] = [r.rgba[i] | ((r.rgba[i + 2] & 15) << 8), r.rgba[i + 1] | ((r.rgba[i + 2] >> 4) << 8)];
            // The centre shows the photograph untouched, unless a texture covers it.
            const inside = cx >= l.x && cx < l.x + w && cy >= l.y && cy < l.y + h;
            const offset = inside && sx < w && sy < h ? [cx - sx, cy - sy] : null;
            const row = { size: w + "x" + h, style, custom, instagram, canvas: [r.width, r.height], layout: [l.width, l.height, l.x, l.y], offset,
              ok: r.width === l.width && r.height === l.height && (!offset || (offset[0] === l.x && offset[1] === l.y)) };
            rows.push(row);
            if (!row.ok) bad.push(row);
          }
        }
      }
      return { cases: rows.length, sizeAndOffset: rows.filter((r) => r.ok && r.offset).length, sizeOnly: rows.filter((r) => r.ok && !r.offset).map((r) => r.size + " " + r.style + (r.instagram ? " ig" : "")), bad };`);
  }

  const photos = (process.env.PHOTOS ?? (quick ? "results/card.jpg" : "results/card.jpg,results/sony.arw,results/card-34.png")).split(",");
  out.photos = {};
  const cases = [];
  for (const photo of photos) {
    const name = photo.split("/").at(-1).replace(/\.[^.]+$/, "");
    const p = (out.photos[name] = {});
    // The 3:4 photograph takes other widths and colours than desktop's.
    const custom = name === "card-34" ? OPTIONS : {};
    p.open = await evaluate(`
      const n = negs.renders.length;
      await negs.open(${JSON.stringify(photo)});
      await settle(n);
      const photo = negs.photo();
      negs.border({ enabled: true });
      const enabled = Object.fromEntries([...document.querySelectorAll(".style")].map((b) => [b.dataset.style, !b.disabled]));
      negs.border({ enabled: false });
      const note = document.querySelector(".styles .note");
      return { photo, styleButtons: enabled, note: note.hidden ? null : note.textContent };`);
    if (!quick) {
      // The unbordered export, once per photograph.
      await evaluate(`
        const r = await negs.read(Math.max(negs.photo().width, negs.photo().height));
        await fetch("result?name=border-${name}&w=" + r.width + "&h=" + r.height, { method: "POST", body: r.rgba });`);
    }
    for (const style of STYLES) {
      if (!p.open.styleButtons[style]) { p[style] = "doesn't fit: off"; continue; }
      for (const instagram of [false, true]) {
        const key = style + (instagram ? "-ig" : "");
        const c = (p[key] = {});
        // Changed at fit: display passes only.
        c.change = await evaluate(`
          negs.view.fit();
          await quiet();
          const n = negs.renders.length, v = negs.views.length;
          await bordered({ ...${JSON.stringify({ ...custom })}, enabled: true, style: ${JSON.stringify(style)}, instagram: ${instagram} });
          // Then its width, a step at a time, as a slider would.
          const k = { solid: "width", keyline: "line", film: "filmWidth" }[${JSON.stringify(style)}];
          if (k) for (let i = 1; i <= 10; i++) { negs.border({ [k]: negs.border()[k] + 0.001 }); await new Promise(requestAnimationFrame); }
          if (k) { await quiet(); negs.border({ [k]: negs.border()[k] - 0.01 }); await quiet(); }
          const l = negs.borderLayout();
          return { renders: negs.renders.length - n, passes: negs.views.length - v, passMs: stats(negs.views.slice(v)), canvas: [l.width, l.height], photoAt: [l.x, l.y], texture: negs.borderTexture()?.path ?? null };`);
        if (process.env.SHOTS) {
          const shot = await send("Page.captureScreenshot", { format: "png" });
          writeFileSync(`web/results/border-${name}-${key}-fit.png`, Buffer.from(shot.result.data, "base64"));
        }
        if (quick) continue;
        // The export: its size and the photograph's offset in it.
        c.export = await evaluate(`
          const e = await negs.exportBytes(), u = e.unbordered, l = negs.borderLayout();
          await fetch("result?name=border-${name}-${key}&w=" + e.width + "&h=" + e.height, { method: "POST", body: e.rgba });
          // A block in the photograph's middle, where it is in the export.
          let same = true;
          const [bx, by] = [Math.floor(u.width / 2) - 32, Math.floor(u.height / 2) - 32];
          for (let y = by; y < by + 64 && same; y++) for (let x = bx; x < bx + 64; x++) {
            const a = ((y + l.y) * e.width + x + l.x) * 4, b = (y * u.width + x) * 4;
            if (e.rgba[a] !== u.rgba[b] || e.rgba[a + 1] !== u.rgba[b + 1] || e.rgba[a + 2] !== u.rgba[b + 2]) { same = false; break; }
          }
          globalThis.exported = e;
          return { size: [e.width, e.height], sizeMatches: e.width === l.width && e.height === l.height, photoAt: [l.x, l.y], photoThere: same, borderMs: Math.round(e.elapsedMs), wasmMiB: e.memoryMiB };`);
        cases.push({ photo: name, image: photo, style, instagram, ...(await evaluate(`const { options } = await import("./border.js"); const s = negs.state();
          return { options: options(negs.border()), film: s.film, paper: s.paper, params: s.params };`)) });
        // 100%: the canvas at the top-left and bottom-right corners, against the export.
        const flat = style === "solid" || style === "keyline" || instagram;
        if (flat || process.env.SHOTS) {
          await evaluate(`const n = negs.renders.length; negs.view.actualSize(1); await settle(n);`);
          c.actual = {};
          for (const [corner, d] of [["topLeft", -1e6], ["bottomRight", 1e6]]) {
            c.actual[corner] = await evaluate(`wheel(${d}, ${d}); await quiet(); return ${flat} ? await compareCanvas(exported) : "screenshot";`);
            if (process.env.SHOTS) {
              const shot = await send("Page.captureScreenshot", { format: "png" });
              writeFileSync(`web/results/border-${name}-${key}-100-${corner}.png`, Buffer.from(shot.result.data, "base64"));
            }
          }
          await evaluate(`negs.view.fit(); await quiet();`);
        }
      }
    }
    await evaluate(`const { DEFAULT_BORDER } = await import("./border.js"); negs.border({ ...DEFAULT_BORDER }); delete globalThis.exported;`);
  }
  out.texturesAtEnd = await evaluate(`return textureLoads();`);
  if (!quick) writeFileSync("web/results/border-cases.json", JSON.stringify(cases, null, 1));
  return out;
};
