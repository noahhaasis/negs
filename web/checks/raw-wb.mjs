// RAW white balance in the editor (docs/handoff-web-white-balance.md), in a
// real page: the controls, re-development from the original bytes, the
// stale-result guards, Reset, failure rollback, Show Original, export, an
// engine switch and rasters. Each step reports; any failed check fails the run.
//   node web/serve.mjs 8137 &
//   node web/cdp.mjs http://localhost:8137/ web/checks/raw-wb.mjs 3000
//   node web/cdp.mjs 'http://localhost:8137/?backend=cpu' web/checks/raw-wb.mjs 3000
//   VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/raw-wb.mjs 3000   (phone: decoder worker route)
export default async ({ evaluate }) => {
  const log = [], failures = [];
  const check = (name, ok, detail) => { log.push({ name, ok: !!ok, ...(detail === undefined ? {} : { detail }) }); if (!ok) failures.push(name); };
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.$ = (id) => document.getElementById(id);
    // Settled: the photograph is the edit's and nothing renders.
    globalThis.settle = async () => {
      await wait(50);
      for (let i = 0; i < 7200; i++) { // The CPU engine's 100% frames take seconds.
        const s = negs.source();
        if (s && s.current && !s.busy && negs.idle() && !document.querySelector("#busy.on")) { await wait(80); if (negs.source().current && negs.idle()) return true; }
        await wait(25);
      }
      return false;
    };
    globalThis.hash = (rgba) => { let h = 2166136261; for (let i = 0; i < rgba.length; i++) h = Math.imul(h ^ rgba[i], 16777619); return (h >>> 0).toString(16) + ":" + rgba.length; };
    globalThis.orig = async () => hash((await negs.readOriginal(256)).rgba);
    globalThis.frame = async () => hash((await negs.read(256)).rgba);
    globalThis.wb = () => negs.state().params.io.raw_white_balance;
    globalThis.group = () => [...document.querySelectorAll("details")].find((d) => d.querySelector("summary span")?.textContent === "RAW White Balance");
    globalThis.row = (label) => [...group().querySelectorAll(".ctl")].find((r) => r.textContent.includes(label));
    globalThis.mode = (m) => { const s = group().querySelector("select"); s.value = m; s.dispatchEvent(new Event("change")); };
    globalThis.typeValue = (label, text, key = "Enter") => {
      row(label).querySelector("button.value").click();
      const f = row(label).querySelector(".value-field");
      f.value = text;
      f.dispatchEvent(new KeyboardEvent("keydown", { key }));
    };
    globalThis.engineReady = async () => { for (let i = 0; i < 300 && !$("engine-info").textContent.includes("·"); i++) await wait(100); };
    globalThis.samples = () => performance.getEntriesByType("resource").filter((r) => r.name.includes("/sample/")).map((r) => r.name.split("/").at(-1));
  `);
  const ev = (body) => evaluate(body);

  // Page load: no sample requests; no RAW group without a photograph.
  const start = await ev(`await engineReady(); return { samples: samples(), hidden: group().hidden, wb: wb(), coarse: matchMedia("(pointer: coarse)").matches };`);
  check("no sample request on page load", start.samples.length === 0, start.samples);
  check("RAW group hidden without a photograph", start.hidden);
  check("default WB As Shot 5500 K / 1", start.wb.mode === "as_shot" && start.wb.temperature === 5500 && start.wb.tint === 1, start.wb);

  // A RAW opens at the current WB, as shot.
  const opened = await ev(`
    await negs.open("sample/DSC_6028.dng"); const settled = await settle();
    return { settled, source: negs.source(), hidden: group().hidden, temp: row("Temperature").hidden, tint: row("Tint").hidden,
      orig: await orig(), frame: await frame(), frame2: await frame() };`);
  check("RAW opens and settles", opened.settled && opened.source.raw && opened.source.colour.source === "raw" && opened.source.wb.mode === "as_shot", opened.source);
  check("RAW group shows for a RAW; custom fields hidden", !opened.hidden && opened.temp && opened.tint);
  check("renders are deterministic (for comparisons)", opened.frame === opened.frame2);
  const asShot = opened.orig;

  // Presentation, view and Post settings that a WB change must keep.
  const before = await ev(`
    negs.rotate(1); negs.border({ enabled: true, style: "solid" }); negs.edit("post.brightness", 20); negs.edit("camera.exposure_compensation_ev", 0.5);
    await settle(); negs.view.actualSize(); await wait(300); await settle();
    return { rotate: negs.state().params.io.rotate_cw, border: negs.border(), post: negs.state().params.post.brightness, zoom: $("zoom-level").textContent,
      film: negs.state().film, paper: negs.state().paper, engines: negs.engineInstances(), size: negs.photo(), frame: await frame() };`);

  // Daylight through the control: develops again, everything else stays.
  const daylight = await ev(`
    const opens = () => negs.timeline.filter((t) => t.event === "open").length, loads = opens();
    mode("daylight");
    const pending = { current: negs.source().current, needs: negs.source().needsDevelop };
    const ok = await settle();
    return { ok, pending, source: negs.source(), wb: wb(), temp: row("Temperature").hidden, reset: !row("White balance").querySelector(".reset").hidden,
      rotate: negs.state().params.io.rotate_cw, border: negs.border(), post: negs.state().params.post.brightness, zoom: $("zoom-level").textContent,
      engines: negs.engineInstances(), size: negs.photo(), opens: opens() - loads, orig: await orig(), frame: await frame() };`);
  check("a WB edit is pending until developed", !daylight.pending.current && daylight.pending.needs, daylight.pending);
  check("Daylight develops again", daylight.ok && daylight.source.wb.mode === "daylight" && daylight.source.current, daylight.source);
  check("Daylight changes the source (Show Original) and the frame", daylight.orig !== asShot && daylight.frame !== before.frame);
  check("mode's per-control reset shows; custom fields stay hidden", daylight.reset && daylight.temp);
  check("rotation, border, Post, zoom, size and engine survive a WB change",
    daylight.rotate === before.rotate && JSON.stringify(daylight.border) === JSON.stringify(before.border) && daylight.post === before.post
    && daylight.zoom === before.zoom && JSON.stringify(daylight.size) === JSON.stringify(before.size) && daylight.engines === before.engines && daylight.opens === 0,
    { before, after: daylight });

  // Custom: its fields show; typing 6504 K with tint 1 is exactly Daylight.
  const custom = await ev(`
    mode("custom");
    const shown = !row("Temperature").hidden && !row("Tint").hidden;
    typeValue("Temperature", "6504");
    await settle();
    const r = { shown, wb: wb(), source: negs.source(), orig: await orig(), resetShown: !row("Temperature").querySelector(".reset").hidden };
    // Escape discards typing.
    typeValue("Tint", "2", "Escape");
    r.afterEscape = wb().tint;
    return r;`);
  check("Custom shows Temperature and Tint", custom.shown);
  check("typed Custom 6504 K / 1 develops to exactly Daylight", custom.source.wb.temperature === 6504 && custom.orig === daylight.orig, custom.source.wb);
  check("Temperature's reset shows off its default; Escape discards typing", custom.resetShown && custom.afterEscape === 1);

  // Inactive custom values don't develop again; per-control reset does.
  const inactive = await ev(`
    mode("daylight"); await settle();
    const n = negs.source().failures, o = await orig();
    negs.edit("io.raw_white_balance.temperature", 3000);
    return { needs: negs.source().needsDevelop, same: o === await orig(), kept: wb().temperature };`);
  check("inactive Custom values don't re-develop and are kept", !inactive.needs && inactive.same && inactive.kept === 3000);
  await ev(`mode("custom"); await settle(); row("Temperature").querySelector(".reset").click(); await settle();`);
  const resetTemp = await ev(`return { wb: wb(), source: negs.source() };`);
  check("Temperature's reset returns to 5500 K and develops it", resetTemp.wb.temperature === 5500 && resetTemp.source.wb.temperature === 5500 && resetTemp.source.current, resetTemp);

  // Rapid edits: only the last WB ends up developed, and it matches a fresh opening at that WB.
  const rapid = await ev(`
    for (const t of [3000, 3400, 3800]) { negs.edit("io.raw_white_balance.temperature", t); await wait(30); }
    negs.edit("io.raw_white_balance.mode", "tungsten"); await wait(200);
    negs.edit("io.raw_white_balance.mode", "custom"); negs.edit("io.raw_white_balance.temperature", 4200); negs.edit("io.raw_white_balance.tint", 0.9);
    await settle();
    const live = { source: negs.source(), orig: await orig() };
    await negs.open("sample/DSC_6028.dng"); await settle();
    return { live, fresh: await orig(), freshWb: negs.source().wb };`);
  check("rapid WB edits settle on the newest WB", rapid.live.source.current && rapid.live.source.wb.mode === "custom" && rapid.live.source.wb.temperature === 4200 && rapid.live.source.wb.tint === 0.9, rapid.live.source);
  check("…whose pixels equal a fresh opening at that WB (no stale or stacked development)", rapid.live.orig === rapid.fresh, rapid);

  // Reset during a development: As Shot, film/paper and presentation kept, Post reset.
  const reset = await ev(`
    negs.rotate(1); negs.border({ enabled: true }); negs.edit("post.contrast", 30); await settle();
    const film = negs.state().film, paper = negs.state().paper, rotate = negs.state().params.io.rotate_cw;
    negs.edit("io.raw_white_balance.mode", "daylight");
    await wait(200);
    const pending = negs.source().needsDevelop || negs.source().busy;
    negs.reset();
    await settle();
    const s = negs.state();
    return { pending, source: negs.source(), wb: s.params.io.raw_white_balance, film: s.film === film, paper: s.paper === paper, rotate: s.params.io.rotate_cw === rotate,
      border: negs.border().enabled, post: s.params.post.contrast, orig: await orig() };`);
  check("Reset during a development returns to As Shot and develops it", reset.source.current && reset.source.wb.mode === "as_shot" && reset.wb.mode === "as_shot" && reset.orig === asShot, reset);
  check("Reset keeps film, paper, rotation and border; resets Post", reset.film && reset.paper && reset.rotate && reset.border && reset.post === 0, reset);

  // Programmatic replacement (as a preset applies) takes the same path.
  const programmatic = await ev(`
    const s = negs.state();
    s.params.io.raw_white_balance = { mode: "daylight", temperature: 5500, tint: 1 };
    negs.replaceSettings(s);
    await negs.whenDeveloped(); await settle();
    return { source: negs.source(), orig: await orig() };`);
  check("programmatic settings replacement re-develops", programmatic.source.wb.mode === "daylight" && programmatic.orig === daylight.orig, programmatic.source);

  // Failure: reported, only the WB returns, export still works, retry succeeds.
  const failure = await ev(`
    const n = negs.source().failures;
    negs.edit("io.raw_white_balance", { mode: "custom", temperature: 99999, tint: 1 });
    negs.edit("post.saturation", 25);
    for (let i = 0; i < 7200 && negs.source().failures === n; i++) await wait(25);
    await wait(50);
    const message = $("message").hidden ? "" : $("message").textContent;
    await settle();
    const stillShown = !$("message").hidden;
    const r = { failures: negs.source().failures - n, message, stillShown, wb: wb(), source: negs.source(), saturation: negs.state().params.post.saturation, orig: await orig() };
    const exported = await negs.exportBytes().then((e) => ({ width: e.width, height: e.height }), (e) => ({ error: e.message }));
    r.exported = exported;
    negs.edit("io.raw_white_balance", { mode: "tungsten", temperature: 5500, tint: 1 });
    await settle();
    r.retry = negs.source();
    return r;`);
  check("a failed development is reported", failure.failures === 1 && /temperature must be between 1667 and 25000 K/.test(failure.message), failure.message);
  check("…and the report stays up over the frames that follow", failure.stillShown);
  check("…restores only the WB (unrelated newer edit kept) and the source it matches", failure.wb.mode === "daylight" && failure.source.current
    && failure.source.wb.mode === "daylight" && failure.saturation === 25 && failure.orig === daylight.orig, failure);
  check("…export still works afterwards", failure.exported.width > 0, failure.exported);
  check("…and trying again succeeds", failure.retry.current && failure.retry.wb.mode === "tungsten", failure.retry);

  // Export right after a WB edit waits for the matching development.
  const exp = await ev(`
    negs.border({ enabled: false }); negs.rotate(-negs.state().params.io.rotate_cw);
    await settle();
    negs.edit("io.raw_white_balance.mode", "daylight");
    const asked = negs.source().current;
    const e = await negs.exportBytes();
    const after = negs.source();
    await settle();
    const r = await negs.read(Math.max(e.width, e.height));
    return { asked, after, same: hash(e.rgba) === hash(r.rgba), sizes: [e.width, e.height, r.width, r.height] };`);
  check("export right after a WB edit waits for its development; preview and export agree",
    !exp.asked && exp.after.wb.mode === "daylight" && exp.same, exp);

  // Show Original held across a WB change is remade from the new development.
  const shown = await ev(`
    await negs.prepareOriginal(); negs.original(true); await wait(200);
    const was = negs.showingOriginal();
    negs.edit("io.raw_white_balance.mode", "as_shot");
    await settle();
    for (let i = 0; i < 200 && !negs.originalMade(); i++) await wait(25);
    const r = { was, made: !!negs.originalMade(), showing: negs.showingOriginal(), orig: await orig() };
    negs.original(false);
    return r;`);
  check("Show Original refreshes from the new development", shown.was && shown.made && shown.showing && shown.orig === asShot, shown);

  // Opening another photograph during a development: the new one, at the selected WB.
  const reopen = await ev(`
    negs.edit("io.raw_white_balance.mode", "daylight");
    await wait(200);
    const opening = negs.open("sample/DSC_6008.dng");
    await opening; await settle();
    const live = { source: negs.source(), orig: await orig(), size: negs.photo() };
    await negs.open("sample/DSC_6008.dng"); await settle();
    return { live, fresh: await orig() };`);
  check("opening another photo during a development keeps the WB and installs only the new photo",
    reopen.live.source.current && reopen.live.source.wb.mode === "daylight" && reopen.live.orig === reopen.fresh, reopen);

  // Engine switch during a development: coherent on the new engine; CPU and
  // WebGPU develop the same source.
  const engines = await ev(`
    const first = $("engine").value, other = first === "gpu" ? "cpu" : "gpu";
    const before = await orig();
    negs.edit("io.raw_white_balance.mode", "tungsten");
    await wait(100);
    $("engine").value = other; $("engine").dispatchEvent(new Event("change"));
    await wait(300); await engineReady(); await settle();
    const a = { engine: $("engine").value, source: negs.source(), orig: await orig() };
    $("engine").value = first; $("engine").dispatchEvent(new Event("change"));
    await wait(300); await engineReady(); await settle();
    return { before, a, b: { engine: $("engine").value, source: negs.source(), orig: await orig() } };`);
  check("engine switch during a development ends coherent at the selected WB",
    engines.a.source.current && engines.a.source.wb.mode === "tungsten" && engines.b.source.current && engines.b.source.wb.mode === "tungsten", engines);
  check("both engines hold the same developed source", engines.a.orig === engines.b.orig && engines.a.orig !== engines.before, engines);

  // A raster: no RAW group, WB kept for the next RAW, pixels unaffected.
  const raster = await ev(`
    const c = new OffscreenCanvas(300, 200), g = c.getContext("2d");
    g.fillStyle = "#c86428"; g.fillRect(0, 0, 300, 200); g.fillStyle = "#2864c8"; g.fillRect(100, 50, 120, 80);
    const url = URL.createObjectURL(await c.convertToBlob({ type: "image/png" }));
    await negs.open(url); await settle();
    const daylightOrig = await orig();
    const r = { source: negs.source(), hidden: group().hidden, wb: wb() };
    negs.edit("io.raw_white_balance.mode", "as_shot"); await settle();
    r.same = daylightOrig === await orig(); r.needs = negs.source().needsDevelop;
    negs.edit("io.raw_white_balance.mode", "daylight");
    await negs.open("sample/DSC_6028.dng"); await settle();
    r.next = { source: negs.source(), orig: await orig(), hidden: group().hidden };
    return r;`);
  check("a raster isn't RAW and hides the RAW group", !raster.source.raw && raster.hidden, raster.source);
  check("…keeps the WB selection, ignores it in its pixels", raster.wb.mode === "tungsten" && raster.same && !raster.needs, raster);
  check("…and the next RAW opens at the selected WB", raster.next.source.wb.mode === "daylight" && raster.next.orig === daylight.orig && !raster.next.hidden, raster.next);

  // Phone layout: the group is in the Camera tab; typing doesn't zoom iOS.
  const layout = await ev(`
    const section = group().closest("fieldset.section");
    group().open = true;
    mode("custom");
    row("Temperature").querySelector("button.value").click();
    const field = row("Temperature").querySelector(".value-field");
    const r = { tab: section.dataset.tab, font: parseFloat(getComputedStyle(field).fontSize), height: field.getBoundingClientRect().height,
      coarse: matchMedia("(pointer: coarse)").matches, samples: [...new Set(samples())] };
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    mode("as_shot"); await settle();
    return r;`);
  check("RAW White Balance is in the Camera section/tab", layout.tab === "camera", layout);
  if (layout.coarse) check("phone: typed values are ≥16 px (no iOS focus zoom)", layout.font >= 16, layout);
  check("only the samples opened were downloaded", layout.samples.every((s) => ["DSC_6028.dng", "DSC_6008.dng"].includes(s)), layout.samples);

  const summary = { passed: log.filter((l) => l.ok).length, failed: failures.length, coarse: start.coarse, log };
  if (failures.length) throw new Error(`${failures.length} RAW WB checks failed: ${failures.join("; ")}\n${JSON.stringify(summary, null, 1)}`);
  return summary;
};
