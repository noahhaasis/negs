// Show Original (piece 8): opens a photograph (PHOTO, default the test
// card) and reports:
//  - the pointer reaching the button makes the original (its size and time);
//  - a mouse press and release: each switch's display passes and their
//    times, that nothing rendered, the badge, and the view unchanged;
//  - the canvas while it shows against the original's bytes, pixel for
//    pixel, fitted and at 100% (where it's remade at the larger size);
//  - the full-size original against the native helper's `original`, when
//    HELPER names its RGBA bytes under web/ (see below);
//  - a pan and a slider moved while it shows: display passes and renders,
//    and that the original stays on the canvas;
//  - `\` held (with key repeat) and let go, and ignored in a text field;
//  - with VIEW=…m, a tap toggles it on and another off.
//   node web/cdp.mjs http://localhost:8137/ web/checks/original.mjs 3000
//   PHOTO=results/sony.arw HELPER=results/helper-arw.rgba node web/cdp.mjs … web/checks/original.mjs 3000
//   VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/original.mjs 3000
//   node web/cdp.mjs 'http://localhost:8137/?backend=cpu' web/checks/original.mjs 3000
// The helper's bytes: send it `{"id":1,"command":"original","input":"<photo>",
// "params":{},"limit_preview":false,"preview_path":"<web/results/helper-….rgba>"}`
// (`spektrafilm-native --data-dir data`).
export default async ({ send, evaluate }) => {
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.quiet = async () => { await wait(400); for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25); await wait(100); };
    globalThis.stage = document.getElementById("stage");
    globalThis.button = document.getElementById("original");
    globalThis.wheel = (dx, dy) => { const r = stage.getBoundingClientRect();
      stage.dispatchEvent(new WheelEvent("wheel", { deltaX: dx, deltaY: dy, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, bubbles: true, cancelable: true })); };
    globalThis.rect = () => { const r = negs.view.rect(); return [r.x, r.y, r.width, r.height].map((v) => +v.toFixed(2)); };
    // What a switch did: display passes (WebGPU) and renders since n, v.
    globalThis.since = (n, v) => ({ renders: negs.renders.length - n, passes: negs.views.length - v, passMs: negs.views.slice(v).map((x) => +x.toFixed(1)) });
    globalThis.now = () => ({ showing: negs.showingOriginal(), badge: negs.badge(), pressed: button.getAttribute("aria-pressed"), zoom: document.getElementById("zoom-level").textContent, rect: rect() });
    // The canvas where it shows the photograph against \`bytes\` (width,
    // height, rgba), pixel for pixel where drawn 1:1: the engine's canvas, or the page's,
    // which places it as paint() does.
    globalThis.compareCanvas = async (bytes) => {
      const gpu = document.getElementById("gpu-canvas"), presenting = gpu && !gpu.hidden;
      let width, height, left, top, dw, dh;
      if (presenting) [width, height, left, top, dw, dh] = await negs.shown();
      else {
        const r = negs.view.rect();
        [width, height] = [bytes.width, bytes.height];
        const exact = Math.abs(r.width - width) <= 1 && Math.abs(r.height - height) <= 1;
        [left, top, dw, dh] = exact ? [Math.round(r.x + (r.width - width) / 2), Math.round(r.y + (r.height - height) / 2), width, height] : [r.x, r.y, r.width, r.height];
      }
      if (bytes.width !== width || bytes.height !== height) return { error: "sizes differ", shown: [width, height], bytes: [bytes.width, bytes.height] };
      // Sampled (the CPU engine's capped frame at 100%), it can't match pixel for pixel.
      if (dw !== width || dh !== height) return { canvas: presenting ? "engine" : "page", oneToOne: false, compared: "not 1:1" };
      const shown = await createImageBitmap(presenting ? gpu : document.getElementById("canvas"));
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
      return { canvas: presenting ? "engine" : "page", oneToOne: dw === width && dh === height, compared: w + "x" + h, exact: (100 * same / (w * h)).toFixed(3) + "%", max: most };
    };
    // Bytes against bytes (RGBA), channel by channel.
    globalThis.compareBytes = (a, b) => {
      if (a.length !== b.length) return { error: "lengths differ", lengths: [a.length, b.length] };
      let same = 0, within = 0, most = 0;
      for (let i = 0; i < a.length; i++) { if (i % 4 === 3) continue; const d = Math.abs(a[i] - b[i]); same += d === 0; within += d <= 1; most = Math.max(most, d); }
      const n = a.length * 3 / 4;
      return { exact: (100 * same / n).toFixed(3) + "%", within1: (100 * within / n).toFixed(3) + "%", differ: n - same, max: most };
    };`);
  const out = {};
  const photo = process.env.PHOTO ?? "results/card.jpg";
  const center = (id) => evaluate(`const r = document.getElementById(${JSON.stringify(id)}).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 };`);
  const mouse = (type, { x, y }) => send("Input.dispatchMouseEvent", { type, x, y, button: type === "mouseMoved" ? "none" : "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
  const key = (type, autoRepeat = false) => send("Input.dispatchKeyEvent", { type, key: "\\", code: "Backslash", text: type === "keyDown" ? "\\" : undefined, windowsVirtualKeyCode: 220, autoRepeat });

  out.opened = await evaluate(`
    for (let i = 0; i < 150 && !document.querySelector("#engine-info").textContent.includes("·"); i++) await wait(100);
    const disabled = button.disabled;
    await negs.open(${JSON.stringify(photo)});
    for (let i = 0; i < 2400 && (!negs.renders.length || !negs.idle()); i++) await wait(25);
    await quiet();
    return { photo: negs.photo(), disabledBeforeAPhoto: disabled, disabled: button.disabled, renderSide: negs.renderSide(), engine: document.getElementById("engine-info").textContent };`);

  // The pointer arriving makes it.
  const b = await center("original");
  await mouse("mouseMoved", { x: b.x - 60, y: b.y });
  await mouse("mouseMoved", b);
  out.hover = await evaluate(`for (let i = 0; i < 400 && !negs.originalMade(); i++) await wait(25); return negs.originalMade();`);

  // Pressed and released.
  await evaluate(`await quiet(); globalThis.mark = [negs.renders.length, negs.views.length, rect()];`);
  await mouse("mousePressed", b);
  out.pressed = await evaluate(`await wait(300); return { ...now(), ...since(mark[0], mark[1]), viewKept: JSON.stringify(rect()) === JSON.stringify(mark[2]) };`);
  out.pressed.canvas = await evaluate(`return compareCanvas(await negs.readOriginal(negs.originalMade().side));`);
  await evaluate(`globalThis.mark = [negs.renders.length, negs.views.length, rect()];`);
  await mouse("mouseReleased", b);
  out.released = await evaluate(`await wait(300); return { ...now(), ...since(mark[0], mark[1]), viewKept: JSON.stringify(rect()) === JSON.stringify(mark[2]) };`);
  out.released.canvas = await evaluate(`return compareCanvas(await negs.read(negs.renderSide()));`);

  // At 100%, held: the original is remade at the larger size; a pan while
  // it shows draws it, and a slider renders without covering it.
  out.actual = await evaluate(`
    document.getElementById("actual").click();
    await quiet();
    const n = negs.renders.length, v = negs.views.length;
    negs.original(true);
    for (let i = 0; i < 400 && negs.originalMade()?.side !== negs.renderSide(); i++) await wait(25);
    await quiet();
    const shown = now(), made = negs.originalMade();
    return { ...shown, made, ...since(n, v), canvas: await compareCanvas(await negs.readOriginal(made.side)) };`);
  out.panWhileShowing = await evaluate(`
    const n = negs.renders.length, v = negs.views.length, before = rect();
    for (let i = 0; i < 20; i++) { wheel(i < 10 ? 40 : -40, 25); await new Promise(requestAnimationFrame); }
    await quiet();
    return { ...since(n, v), moved: JSON.stringify(rect()) !== JSON.stringify(before), showing: negs.showingOriginal(), canvas: await compareCanvas(await negs.readOriginal(negs.originalMade().side)) };`);
  out.sliderWhileShowing = await evaluate(`
    const n = negs.renders.length, v = negs.views.length;
    negs.edit("camera.exposure_compensation_ev", 1);
    await quiet();
    return { ...since(n, v), showing: negs.showingOriginal(), canvas: await compareCanvas(await negs.readOriginal(negs.originalMade().side)) };`);
  out.letGoAt100 = await evaluate(`
    await quiet();
    const n = negs.renders.length, v = negs.views.length, before = rect();
    negs.original(false);
    await wait(300);
    const r = await negs.read(negs.renderSide());
    return { ...now(), ...since(n, v), viewKept: JSON.stringify(rect()) === JSON.stringify(before), canvas: await compareCanvas(r) };`);

  // The full-size original against the helper's.
  if (process.env.HELPER) {
    out.helper = await evaluate(`
      const p = negs.photo(), long = Math.max(p.width, p.height);
      const ours = await negs.readOriginal(long);
      const theirs = new Uint8Array(await (await fetch(${JSON.stringify(process.env.HELPER)})).arrayBuffer());
      return { size: ours.width + "x" + ours.height, ms: Math.round(ours.elapsedMs), ...compareBytes(ours.rgba, theirs) };`);
  }

  // Back to fit; then by keyboard: \\ held with key repeat, let go, and
  // ignored while typing in a field.
  await evaluate(`negs.edit("camera.exposure_compensation_ev", 0); document.getElementById("fit").click(); await quiet(); document.activeElement?.blur();
    globalThis.mark = [negs.renders.length, negs.views.length, rect()];`);
  await key("keyDown");
  for (let i = 0; i < 5; i++) { await new Promise((r) => setTimeout(r, 40)); await key("keyDown", true); }
  out.keyHeld = await evaluate(`await wait(300); return { ...now(), ...since(mark[0], mark[1]) };`);
  await evaluate(`globalThis.mark = [negs.renders.length, negs.views.length, rect()];`);
  await key("keyUp");
  out.keyUp = await evaluate(`await wait(300); return { ...now(), ...since(mark[0], mark[1]) };`);
  // (A value field opens on desktop; the phone's sheet shows another tab.)
  if (!process.env.VIEW?.endsWith("m")) {
    await evaluate(`
      document.querySelector(".ctl.slider .value").click();
      await wait(50);`);
    await key("keyDown");
    out.keyInField = await evaluate(`await wait(200); const field = document.activeElement; const r = { focused: field?.className, typed: field?.value, showing: negs.showingOriginal() };
      field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); return r;`);
    await key("keyUp");
  }

  // A tap toggles on a touch screen.
  if (process.env.VIEW?.endsWith("m")) {
    const t = await center("original");
    const tap = async () => {
      await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: t.x, y: t.y, id: 1 }] });
      await new Promise((r) => setTimeout(r, 80));
      await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    };
    await evaluate(`await quiet(); globalThis.mark = [negs.renders.length, negs.views.length, rect()];`);
    await tap();
    out.tapOn = await evaluate(`for (let i = 0; i < 200 && !negs.showingOriginal(); i++) await wait(25); await wait(500); return { ...now(), ...since(mark[0], mark[1]) };`);
    await evaluate(`globalThis.mark = [negs.renders.length, negs.views.length, rect()];`);
    await tap();
    out.tapOff = await evaluate(`await wait(500); return { ...now(), ...since(mark[0], mark[1]) };`);
  }
  return out;
};
