// node web/cdp.mjs http://localhost:8137/ web/checks/crop.mjs 3000
// Also run with ?backend=cpu, ?present=0 and VIEW=390x844@3m.
// Assertions exercise real mouse/touch crop drags and the public controls.
export default async ({ send, evaluate }) => {
  const mobile = process.env.VIEW?.endsWith("m");
  const check = (ok, message) => { if (!ok) throw new Error(message); };
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.quiet = async () => { await wait(450); for (let i = 0; i < 2400 && !negs.idle(); i++) await wait(25); await wait(100); };
    globalThis.chooseAspect = (aspect) => { const e = document.getElementById('crop-aspect'); e.value = aspect; e.dispatchEvent(new Event('change')); };
    globalThis.setAngle = (angle) => { const e = document.getElementById('crop-angle'); e.value = angle; e.dispatchEvent(new Event('input')); e.dispatchEvent(new Event('change')); };
    globalThis.Crop = await import('./crop.js');
    const canvas = new OffscreenCanvas(96, 64), ctx = canvas.getContext('2d'), image = ctx.createImageData(96, 64);
    for (let y = 0; y < 64; y++) for (let x = 0; x < 96; x++) { const i = (y * 96 + x) * 4; image.data.set([x * 2, y * 3, (x + y) % 255, 255], i); }
    ctx.putImageData(image, 0, 0);
    const url = URL.createObjectURL(await canvas.convertToBlob());
    await negs.open(url); URL.revokeObjectURL(url); await quiet();
    negs.start('kodak_portra_400', 'kodak_portra_endura', { 'film_render.grain.active': false }); await quiet();
    globalThis.initial = negs.state(); globalThis.count = negs.renders.length;
  `);
  const cancelled = await evaluate(`
    negs.beginCrop(); chooseAspect('square'); setAngle(7.5); negs.rotate(1);
    const draft = negs.cropSession(), locked = document.getElementById('original').disabled && document.getElementById('export').hidden;
    negs.endCrop(false); await quiet();
    return { draft, locked, crop: negs.crop(), rotation: negs.state().params.io.rotate_cw, renders: negs.renders.length - count };
  `);
  check(cancelled.locked && cancelled.draft.settings.angle === 7.5, "crop draft controls/leveling failed");
  check(cancelled.rotation === 0 && cancelled.crop.width === 1 && cancelled.renders === 0, "Cancel did not restore crop/rotation without rendering");

  // A real handle drag and a real inside drag (the normal view must not pan).
  const point = (where) => evaluate(`
    const s = negs.cropSession(), p = negs.photoGeometry(), r = negs.view.rect(), stage = document.getElementById('stage').getBoundingClientRect(), d = negs.view.size().width / stage.width;
    const scale = r.width / p.width, c = s.shown;
    return { x: stage.x + (r.x + (c.x + c.width * ${where === "corner" ? 1 : 0.5}) * scale) / d,
      y: stage.y + (r.y + (c.y + c.height * ${where === "corner" ? 1 : 0.5}) * scale) / d };
  `);
  const drag = async (p, dx, dy) => {
    if (mobile) {
      await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ ...p, id: 1 }] });
      for (let i = 1; i <= 10; i++) await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: p.x + dx * i / 10, y: p.y + dy * i / 10, id: 1 }] });
      await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    } else {
      await send("Input.dispatchMouseEvent", { type: "mousePressed", ...p, button: "left", buttons: 1, clickCount: 1 });
      for (let i = 1; i <= 10; i++) await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: p.x + dx * i / 10, y: p.y + dy * i / 10, button: "left", buttons: 1 });
      await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: p.x + dx, y: p.y + dy, button: "left", buttons: 0, clickCount: 1 });
    }
  };
  await evaluate(`negs.beginCrop(); chooseAspect('freeform');`);
  await drag(await point("corner"), -25, -20);
  const resized = await evaluate(`return negs.cropSession();`);
  check(resized.settings.width < 1 && resized.settings.height < 1, "corner drag did not resize the crop");
  await drag(await point("center"), 12, 10);
  const moved = await evaluate(`return negs.cropSession();`);
  check(moved.settings.x > resized.settings.x && moved.settings.y > resized.settings.y, "inside drag did not move the crop");
  const dialPoint = await evaluate(`const r = document.getElementById('crop-dial').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 };`);
  await drag(dialPoint, -28, 0);
  const leveled = await evaluate(`return negs.cropSession();`);
  check(leveled.settings.angle === 4, "dial did not straighten by four degrees");

  const result = await evaluate(`
    negs.rotate(1); document.getElementById('crop-done').click(); await quiet();
    const noRenders = negs.renders.length === count;
    const c = negs.crop(), p = negs.photo(), raw = await negs.read(96), expected = Crop.applyRgba(raw.rgba, raw.width, raw.height, c, 1);
    const exported = await negs.exportBytes();
    let delta = 0; for (let i = 0; i < expected.rgba.length; i++) delta = Math.max(delta, Math.abs(expected.rgba[i] - exported.rgba[i]));
    negs.original(true); for (let i = 0; i < 100 && !negs.showingOriginal(); i++) await wait(25);
    const originalGeometry = negs.photoGeometry(); negs.original(false);
    negs.border({ enabled: true, style: 'solid', width: 0.1 }); await quiet();
    const bordered = await negs.exportBytes(), layout = negs.borderLayout();
    return { noRenders, crop: c, rotation: negs.state().params.io.rotate_cw,
      geometry: negs.photoGeometry(), originalGeometry, export: [exported.width, exported.height], expected: [expected.width, expected.height], delta,
      bordered: [bordered.width, bordered.height], borderSize: [layout.width, layout.height], error: document.getElementById('message').classList.contains('error') && document.getElementById('message').textContent };
  `);
  check(result.noRenders, "presentation edits reran the film pipeline");
  check(result.rotation === 1 && result.crop.angle === 4, "Done did not commit leveling and turning");
  check(result.export.join() === result.expected.join() && result.delta <= 1, "export does not match crop-before-turn bytes");
  check(result.bordered.join() === result.borderSize.join(), "border used the uncropped dimensions");
  check(JSON.stringify(result.geometry) === JSON.stringify(result.originalGeometry), "Show Original lost finishing geometry");
  check(!result.error, `page error: ${result.error}`);

  // For a non-leveled crop, both canvas paths must match lossless turned
  // export bytes at 100% (no interpolation or film processing involved).
  const canvas = await evaluate(`
    negs.border({ enabled: false }); negs.beginCrop(); document.getElementById('crop-reset').click(); chooseAspect('square'); negs.endCrop(true);
    negs.view.actualSize(); await quiet();
    const r = await negs.exportBytes(), g = negs.photoGeometry(), rect = negs.view.rect();
    const gpu = document.getElementById('gpu-canvas'), element = gpu && !gpu.hidden ? gpu : document.getElementById('canvas');
    const bitmap = await createImageBitmap(element), ctx = new OffscreenCanvas(bitmap.width, bitmap.height).getContext('2d'); ctx.drawImage(bitmap, 0, 0); bitmap.close();
    const bytes = ctx.getImageData(Math.round(rect.x), Math.round(rect.y), r.width, r.height).data;
    let max = 0; for (let i = 0; i < bytes.length; i++) max = Math.max(max, Math.abs(bytes[i] - r.rgba[i]));
    return { canvas: element.id, max, size: [r.width, r.height], g };
  `);
  check(canvas.max <= 1, `100% turned canvas differs from export by ${canvas.max}`);
  const reopened = await evaluate(`await negs.open('sample/DSC_6377.dng'); await quiet(); return { crop: negs.crop(), rotation: negs.state().params.io.rotate_cw, ready: !document.getElementById('edit-tools').hidden };`);
  check(reopened.crop.width === 1 && reopened.crop.angle === 0 && reopened.rotation === 0 && reopened.ready, "new photo did not reset finishing edits");
  return { backend: await evaluate(`return document.getElementById('engine-info').textContent;`), mobile: !!mobile, cancelled, resized, moved, leveled, result, canvas, reopened };
};
