// The Post tab: opens a photograph (PHOTO, default the test card) and
// reports:
//  - the Lab | Post switch, and the Post pane's controls;
//  - each Post slider dragged through the page's own slider: every frame it
//    asked for and how it was made (`post` is a display pass from the scan
//    the GPU kept, no film chain), with their times;
//  - the canvas after the drags against the same edit rendered and read
//    back (the film chain with Post in its display pass), pixel for pixel;
//  - a Lab change with Post set (a film render that applies Post), and
//    Reset Adjustments (Post back to zero);
//  - at 100%, where a frame is larger, the same comparison.
//   node web/cdp.mjs http://localhost:8137/ web/checks/post.mjs 3000
//   node web/cdp.mjs 'http://localhost:8137/?backend=cpu' web/checks/post.mjs 3000
//   VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/post.mjs 3000
export default async ({ evaluate }) => {
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.quiet = async () => { await wait(400); for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25); await wait(100); };
    globalThis.frameOf = (r) => r && { frame: r.width + "x" + r.height, ms: Math.round(r.elapsedMs), presented: r.presented, stages: r.stages };
    globalThis.made = (r) => r.stages.includes("post") ? "post" : r.stages.includes("gpu") ? "film (gpu)" : r.stages.includes("cpu") ? "film (cpu)" : r.stages;
    globalThis.slider = (label) => document.querySelector('#post-sections input[type=range][aria-label="' + label + '"]');
    // Drag a slider through \`values\` as a person would: input events a
    // frame apart, then change.
    globalThis.drag = async (label, values) => {
      const range = slider(label), n = negs.renders.length;
      for (const v of values) { range.value = v; range.dispatchEvent(new Event("input", { bubbles: true })); await wait(16); }
      range.dispatchEvent(new Event("change", { bubbles: true }));
      await quiet();
      const frames = negs.renders.slice(n);
      const kinds = {};
      for (const r of frames) (kinds[made(r) + (r.interactive ? ", interactive" : ", settled")] ??= []).push(Math.round(r.elapsedMs));
      return { value: negs.state().params.post[label.toLowerCase()], frames: frames.length, kinds };
    };
    // The canvas against \`bytes\` (the frame read back), where it shows the frame.
    globalThis.compareCanvas = async (bytes) => {
      if (!bytes.presented && !document.getElementById("gpu-canvas")) return compareBitmap(bytes);
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
      return { oneToOne: dw === width && dh === height, compared: w + "x" + h, exact: (100 * same / (w * h)).toFixed(3) + "%", max: most };
    };
    // The CPU engine draws on the page's canvas: compare its frame instead.
    globalThis.compareBitmap = async (bytes) => ({ note: "CPU engine: the page paints the bytes it gets; nothing to compare" });
    globalThis.read = async () => { const r = negs.renders.at(-1); return negs.read(Math.max(r.width, r.height)); };`);
  const out = {};
  const photo = process.env.PHOTO ?? "results/card.jpg";
  out.open = await evaluate(`
    for (let i = 0; i < 150 && !document.querySelector("#engine-info").textContent.includes("·"); i++) await wait(100);
    await negs.open(${JSON.stringify(photo)});
    await quiet();
    negs.pane("post");
    // On the phone's sheet, its Post tab.
    const tab = document.querySelector('#tabs [data-tab=post]');
    if (tab?.offsetParent) tab.click();
    const side = document.getElementById("side");
    const shown = [...document.querySelectorAll(".stock, .section")].filter((e) => e.offsetParent).map((e) => e.querySelector("h2")?.textContent ?? e.className);
    return { ...frameOf(negs.renders.at(-1)), pane: side.dataset.pane, tabs: [...document.querySelectorAll("#tabs button")].filter((b) => b.offsetParent).map((b) => b.textContent), visible: shown,
      controls: [...document.querySelectorAll("#post-sections .ctl")].filter((e) => e.offsetParent).map((e) => e.querySelector(".label, label > span")?.textContent ?? e.querySelector("[aria-label]")?.getAttribute("aria-label")),
      footer: document.querySelector("#post-sections .footer")?.textContent };`);
  out.drags = {};
  for (const [label, values] of [["Brightness", [5, 15, 25, 35, 45]], ["Contrast", [10, 20, 30, 40]], ["Whites", [-5, -10, -15]], ["Blacks", [5, 10]], ["Saturation", [10, 20, 30, 40, 50]]]) {
    out.drags[label] = await evaluate(`return drag(${JSON.stringify(label)}, ${JSON.stringify(values)});`);
  }
  out.canvasVsRender = await evaluate(`return compareCanvas(await read());`);
  out.labChange = await evaluate(`
    const n = negs.renders.length;
    negs.edit("camera.exposure_compensation_ev", 0.5);
    await quiet();
    const frames = negs.renders.slice(n).map((r) => made(r));
    return { frames, canvasVsRender: await compareCanvas(await read()) };`);
  out.postAfterLab = await evaluate(`return drag("Contrast", [20, 0]);`);
  out.reset = await evaluate(`
    const n = negs.renders.length;
    document.getElementById("reset").click();
    await quiet();
    return { frames: negs.renders.slice(n).map((r) => made(r)), post: negs.state().params.post, canvasVsRender: await compareCanvas(await read()) };`);
  out.actual = await evaluate(`
    negs.edit("post.saturation", 30);
    await quiet();
    document.getElementById("actual").click();
    await quiet();
    const before = frameOf(negs.renders.at(-1));
    const drag1 = await drag("Brightness", [10, 20, 30]);
    return { before, drag: drag1, canvasVsRender: await compareCanvas(await read()) };`);
  return out;
};
