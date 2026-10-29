// Real-device editor regression/stress cases. Run with web/safari.mjs.
// IPAD_PHASE=smoke|extremes|soak|export|limited-export|after-export|zoom|heavy|lifecycle.
// IPAD_NATIVE=1 includes a native slider drag (Safari's iOS input support varies).
// A completed render, no monitor loss, and no engine error are required.
import { SECTIONS } from "../controls.js";

export default async function (test) {
  const { execute, step, click, call, wait, probe, screenshot, log } = test;
  const env = test.env ?? globalThis.process?.env ?? {};
  const phase = env.IPAD_PHASE ?? "smoke";
  const initial = await execute("return negs.state();");
  const valueAt = (path, from = initial.params) => path.split(".").reduce((o, k) => o?.[k], from);
  const start = (edits = {}, film = initial.film, paper = initial.paper) => execute("negs.start(arguments[0], arguments[1], arguments[2]);", film, paper, edits);
  const input = (label, value, type = "change") => execute(`
    const input = [...document.querySelectorAll('input[type=range]')].find(e => e.getAttribute('aria-label') === arguments[0]);
    if (!input) throw new Error('Missing slider ' + arguments[0]);
    input.value = arguments[1];
    input.dispatchEvent(new Event(arguments[2], { bubbles: true }));
    return input.value;
  `, label, value, type);

  if (phase === "smoke") {
    await step("show Camera controls", () => execute("document.querySelector('#tabs [data-tab=\"camera\"]').click();"), { settle: false });
    await wait(400);
    if (env.IPAD_NATIVE === "1") await step("native exposure slider drag", async () => {
      const box = await execute(`
        const track = document.querySelector('input[aria-label="Exposure (EV)"]').parentElement;
        track.scrollIntoView({ block: 'center' });
        const r = track.getBoundingClientRect();
        if (r.width < 20 || r.height < 5 || r.y < 0 || r.bottom > innerHeight) throw new Error('Exposure track is not visible: ' + JSON.stringify(r.toJSON()));
        return { x: Math.round(r.x + r.width * .35), y: Math.round(r.y + r.height / 2), dx: Math.round(r.width * .18) };
      `);
      const actions = [
        { type: "pointerMove", duration: 0, x: box.x, y: box.y, origin: "viewport" },
        { type: "pointerDown", button: 0 },
        ...Array.from({ length: 8 }, (_, i) => ({ type: "pointerMove", duration: 80, x: box.x + Math.round(box.dx * (i + 1) / 8), y: box.y, origin: "viewport" })),
        { type: "pointerUp", button: 0 },
      ];
      await call("POST", "/actions", { actions: [{ type: "pointer", id: "finger", parameters: { pointerType: "mouse" }, actions }] });
    });
    for (const value of [-5, 5, 0]) await step(`exposure slider ${value}`, () => input("Exposure (EV)", value));
    await step("rapid exposure inputs", async () => {
      for (const value of [-4, 4, -3, 3, -2, 2, -1, 1, 0]) {
        await input("Exposure (EV)", value, "input");
        await wait(50);
      }
      await input("Exposure (EV)", .7);
    });
    const finalEV = await execute("return negs.state().params.camera.exposure_compensation_ev;");
    if (finalEV !== .7) throw new Error(`Rapid edits ended at ${finalEV}, expected 0.7`);
    await step("reset after slider edits", () => execute("document.getElementById('reset').click();"));
  } else if (phase === "extremes") {
    const first = Number(env.IPAD_CASE_START ?? 0);
    let index = 0;
    const check = async (label, action) => {
      const current = index++;
      if (current < first) return;
      log("control-case", { index: current, label });
      await step(label, action);
    };
    const catalog = await execute("return fetch('catalog.json').then(r => r.json());");
    const film = catalog.profiles.find(p => p.id === initial.film);
    for (const section of SECTIONS) {
      for (const group of [{ title: section.title, controls: section.controls ?? [] }, ...section.groups]) {
        if (group.enable) {
          for (const enabled of [false, true]) await check(`${group.title}: enabled = ${enabled}`, () => start({ [group.enable]: enabled }));
        }
        for (const control of group.controls) {
          if (control.kind === "action") continue;
          const needs = {};
          if (group.enable) needs[group.enable] = true;
          if (section.print) needs["io.scan_film"] = false;
          if (control.when) needs[control.when] = true;
          if (control.path === "scanner.white_level") needs["scanner.white_correction"] = true;
          if (control.path === "scanner.black_level") needs["scanner.black_correction"] = true;
          const values = control.kind === "choice" ? control.options : control.kind === "toggle" ? [false, true] : [control.min, control.max];
          for (const value of values) {
            let set = value;
            if (control.kind === "stock") {
              const preset = valueAt(control.path) ?? film.grain[control.path.split(".").at(-1)];
              if (control.index >= preset.length) continue;
              set = [...preset];
              set[control.index] = value;
            }
            await check(`${group.title}: ${control.label} = ${JSON.stringify(value)}`, () => start({ ...needs, [control.path]: set }));
          }
        }
      }
    }
    await step("reset after control extremes", () => start());
  } else if (phase === "soak") {
    const rounds = Number(env.IPAD_ROUNDS ?? 10);
    const films = ["kodak_portra_400", "kodak_trix", "fujifilm_velvia_100", "kodak_vision3_500t"];
    for (let round = 0; round < rounds; round++) {
      for (const film of films) {
        await step(`round ${round + 1}: ${film}`, () => start({}, film));
        await step(`round ${round + 1}: diffusion and halation`, () => start({
          "camera.diffusion_filter.active": true,
          "enlarger.diffusion_filter.active": true,
          "film_render.halation.active": true,
          "film_render.halation.halation_n_bounces": 5,
          "print_render.glare.active": true,
        }, film));
      }
    }
    await step("reset after soak", () => start());
  } else if (phase === "lifecycle") {
    await step("reset for lifecycle checks", () => start());
    await execute("document.querySelector('#tabs [data-tab=\"camera\"]').click();");
    await step("typed exposure clamps below its minimum", () => execute(`
      const range = document.querySelector('input[aria-label="Exposure (EV)"]');
      range.closest('.slider').querySelector('.value').click();
      const field = document.querySelector('input.value-field');
      field.value = '-999999';
      field.dispatchEvent(new KeyboardEvent('keydown', {key:'Enter',bubbles:true}));
    `));
    if (await execute("return negs.state().params.camera.exposure_compensation_ev;") !== -5) throw new Error('Typed exposure was not clamped');
    await step("invalid typed exposure leaves the edit intact", () => execute(`
      const range = document.querySelector('input[aria-label="Exposure (EV)"]');
      range.closest('.slider').querySelector('.value').click();
      const field = document.querySelector('input.value-field');
      field.value = 'Infinity';
      field.dispatchEvent(new KeyboardEvent('keydown', {key:'Enter',bubbles:true}));
    `), {settle:false});
    if (await execute("return negs.state().params.camera.exposure_compensation_ev;") !== -5) throw new Error('Invalid typed exposure changed the edit');
    await step("reset typed exposure", () => start());
    await step("Balance Print action", () => execute(`
      const button = [...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Balance Print');
      if(!button || button.disabled) throw new Error('Balance Print is unavailable');
      button.click();
    `));
    await step("choosing the active CPU engine keeps it running", () => execute(`
      const select = document.getElementById('engine');
      select.value = 'cpu'; select.dispatchEvent(new Event('change'));
    `), {settle:false});
    await step("unsupported GPU selection falls back safely", () => execute(`
      const select = document.getElementById('engine');
      select.value = 'gpu'; select.dispatchEvent(new Event('change'));
    `));
    // Queue a new import behind a running render, as selecting a new photo
    // while a slider's settled render is working would do.
    await step("replace RAW during a render", () => execute(`
      negs.edit('camera.exposure_compensation_ev',1);
      negs.open('results/card.jpg').catch(e=>console.error(e));
    `));
    await test.waitFor("negs.photo()?.width === 2752 && negs.idle() && negs.renders.at(-1).width === negs.renderSide()");
    for (let round = 0; round < Number(env.IPAD_IMPORT_ROUNDS ?? 3); round++) {
      for (const [path,width] of [["results/0955097116.raf",6240],["results/card.jpg",2752]]) {
        await step(`import round ${round+1}: ${path}`, () => execute("negs.open(arguments[0]).catch(e=>console.error(e));",path));
        await test.waitFor(`negs.photo()?.width === ${width} && negs.idle() && negs.renders.at(-1).width === negs.renderSide()`);
      }
    }
    await step("restore Fuji after repeated imports", () => execute("negs.open('results/0955097116.raf').catch(e=>console.error(e));"));
    await test.waitFor("negs.photo()?.width === 6240 && negs.idle()");
    await step("reset after lifecycle checks", () => start());
  } else if (phase === "zoom") {
    await step("zoom Fuji to 100%", () => execute("negs.view.actualSize();"));
    await step("edit at 100% zoom", () => input("Exposure (EV)", 1));
    await step("zoom Fuji back to fit", () => execute("negs.view.fit();"));
    await step("reset after zoom", () => start());
  } else if (phase === "heavy") {
    await step("combined effects at maximum sizes", () => start({
      "camera.film_format_mm": 4,
      "camera.lens_blur_um": 100,
      "camera.diffusion_filter.active": true,
      "camera.diffusion_filter.strength": 2,
      "camera.diffusion_filter.spatial_scale": 3,
      "camera.diffusion_filter.halo_size": 3,
      "camera.diffusion_filter.bloom_size": 3,
      "enlarger.diffusion_filter.active": true,
      "enlarger.diffusion_filter.strength": 2,
      "enlarger.diffusion_filter.spatial_scale": 3,
      "enlarger.diffusion_filter.core_size": 4,
      "enlarger.diffusion_filter.halo_size": 4,
      "enlarger.diffusion_filter.bloom_size": 4,
      "film_render.halation.active": true,
      "film_render.halation.halation_n_bounces": 5,
      "film_render.halation.halation_spatial_scale": 5,
      "film_render.halation.scatter_spatial_scale": 5,
      "film_render.dir_couplers.active": true,
      "film_render.dir_couplers.diffusion_size_um": 100,
      "film_render.dir_couplers.diffusion_tail_um": 400,
      "film_render.grain.active": true,
      "print_render.glare.active": true,
      "print_render.glare.blur": 5,
      "scanner.lens_blur": 5,
      "scanner.unsharp_mask.0": 3,
    }));
  } else if (phase === "after-export") {
    await step("edit after full-resolution export", () => input("Exposure (EV)", 1));
    await step("reset after full-resolution export", () => execute("document.getElementById('reset').click();"));
  } else if (phase === "limited-export") {
    const before = await probe();
    log("limited-export-start", { state: before });
    await execute("document.getElementById('export').click();");
    const after = await test.waitFor("!document.getElementById('export').disabled && negs.idle() && !document.getElementById('message').hidden");
    if (!after.message.includes('needs too much memory') || after.exports.length !== before.exports.length) throw new Error('Expected the browser memory limit message');
    if (after.engineInstances !== before.engineInstances) throw new Error('Memory rejection restarted the engine');
    log("limited-export-pass", { state: after });
    await step("edit after limited export", () => input("Exposure (EV)", .3));
  } else if (phase === "export") {
    const before = await probe();
    log("export-start", { state: before });
    await execute("document.getElementById('export').click();");
    await test.waitFor("!document.getElementById('export').disabled && negs.idle()", { timeout: 600000 });
    const after = await probe();
    test.assertHealthy(after);
    if (after.exports.length <= before.exports.length || after.exports.at(-1).bytes === 0) throw new Error('No JPEG blob was produced');
    log("export-pass", { state: after });
  } else throw new Error(`Unknown IPAD_PHASE: ${phase}`);
  return { phase, final: await probe(), screenshot: await screenshot(`${phase}-${Date.now()}`) };
}
