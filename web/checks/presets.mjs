// Presets in a real page (docs/web-presets-design.md): the step-2 controls
// (development times, Color Management), the ⋯ menu (and its Export LUT), importing a file with
// and without a photograph, refusals, Reset after an import, the export
// dialog's file and link, and an incoming link (arrival, Download, Close,
// Try, with Sample 1 open already, keyboard). Each step reports; any failed
// check fails the run.
//   node web/serve.mjs 8137 &
//   node web/cdp.mjs http://localhost:8137/ web/checks/presets.mjs 3000
//   VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/presets.mjs 3000   (phone)
export default async ({ send, evaluate }) => {
  const log = [], failures = [];
  const check = (name, ok, detail) => { log.push({ name, ok: !!ok, ...(detail === undefined ? {} : { detail }) }); if (!ok) failures.push(name); };
  const helpers = `
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.$ = (id) => document.getElementById(id);
    globalThis.until = async (f, ms = 60000) => { for (const end = Date.now() + ms; Date.now() < end; await wait(25)) { const v = await f(); if (v) return v; } return false; };
    globalThis.settle = async () => until(async () => { const s = negs.source(); if (!(s && s.current && !s.busy && negs.idle() && !negs.presetBusy() && !document.querySelector("#busy.on"))) return false; await wait(80); return negs.source().current && negs.idle(); }, 120000);
    globalThis.engineReady = () => until(() => $("engine-info").textContent.includes("·"));
    // Sample photographs fetched (the picker's own catalog.json isn't one).
    globalThis.samples = () => performance.getEntriesByType("resource").filter((r) => r.name.includes("/sample/") && !r.name.endsWith(".json")).map((r) => r.name.split("/").at(-1));
    globalThis.msg = () => !$("message").hidden && $("message").textContent;
    // Every message shown, as a slow engine's frames may hide one before a check reads it.
    globalThis.messages = [];
    new MutationObserver(() => !$("message").hidden && messages.push($("message").textContent)).observe($("message"), { childList: true, characterData: true, subtree: true, attributes: true });
    globalThis.choose = (select, value) => { select.value = value; select.dispatchEvent(new Event("change")); };
    globalThis.ctl = (label) => [...document.querySelectorAll(".ctl.choice")].find((r) => r.querySelector("label span")?.textContent === label);
    globalThis.importFile = (name, text) => { const dt = new DataTransfer(); dt.items.add(new File([text], name, { type: "application/json" })); $("preset-file").files = dt.files; $("preset-file").dispatchEvent(new Event("change")); };
    // Downloads made with a link's click: their names and texts.
    globalThis.downloads = [];
    const click = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function () { if (this.download) { const a = this; downloads.push(fetch(a.href).then((r) => r.text()).then((text) => ({ name: a.download, text }))); return; } return click.call(this); };
    globalThis.key = (k, target = document.activeElement) => target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  `;
  const ev = (body) => evaluate(body);
  await ev(helpers);

  // Page load: no sample request; the menu works without a photograph.
  const start = await ev(`await engineReady(); await wait(200);
    return { samples: samples(), more: !$("more").disabled, items: [...$("preset-menu").querySelectorAll("button")].map((b) => b.textContent) };`);
  check("no sample request on page load", start.samples.length === 0, start.samples);
  check("⋯ menu has exactly Import preset, Export preset and Export LUT", JSON.stringify(start.items) === JSON.stringify(["Import preset…", "Export preset…", "Export LUT…"]), start.items);
  check("menu enabled without a photograph", start.more);
  const keys = await ev(`
    $("more").focus(); key("ArrowDown", $("more")); await wait(20);
    const first = document.activeElement.id, open = !$("preset-menu").hidden;
    key("ArrowDown"); const second = document.activeElement.id;
    key("Escape"); await wait(20);
    const closed = $("preset-menu").hidden, back = document.activeElement.id;
    $("more").click(); await wait(20); const reopened = !$("preset-menu").hidden;
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); await wait(20);
    return { first, open, second, closed, back, reopened, outside: $("preset-menu").hidden, expanded: $("more").getAttribute("aria-expanded") };`);
  check("menu keyboard: ArrowDown opens on first item, moves, Escape closes and restores focus",
    keys.open && keys.first === "import-preset" && keys.second === "export-preset" && keys.closed && keys.back === "more", keys);
  check("menu closes on an outside press", keys.reopened && keys.outside && keys.expanded === "false", keys);

  // Export LUT: desktop's 33³ CUBE of the film/paper look, downloaded as
  // <film>_<paper>.cube, with or without a photograph.
  const lut = await ev(`
    downloads.length = 0; messages.length = 0;
    $("more").click(); await wait(20); $("export-lut").click();
    const busy = $("export-lut").disabled;
    await until(() => downloads.length, 60000);
    const [file] = await Promise.all(downloads);
    const rows = file?.text.split("\\n").filter((l) => /^[0-9]/.test(l)).length;
    const s = negs.state();
    return { busy, name: file?.name, expected: s.film + "_" + s.paper + ".cube", size: /^LUT_3D_SIZE 33$/m.test(file?.text ?? ""), rows,
             messages: [...messages], enabled: !$("export-lut").disabled };`);
  check("Export LUT downloads <film>_<paper>.cube, 33³ rows", lut.name === lut.expected && lut.size && lut.rows === 33 ** 3, lut);
  check("Export LUT is disabled while it runs and reports when done", lut.busy && lut.enabled && lut.messages.some((m) => m.startsWith("Exported ")), lut);

  // Step 2: development times and Color Management.
  const controls = await ev(`
    const row = $("film-time-row"), portra = row.hidden;
    choose($("film"), "kodak_doublex"); await wait(50);
    const options = [...$("film-time").options].map((o) => o.textContent);
    choose($("film-time"), "6.5"); await wait(50);
    const set = negs.state().params.film_render.development_time;
    choose($("film"), "kodak_portra_400"); await wait(50);
    const reset = negs.state().params.film_render.development_time;
    const gamut = ctl("Output gamut"), input = ctl("Input gamut"), spectral = ctl("Spectral upsampling");
    const d = negs.state().params;
    const defaults = [gamut.querySelector("select").value, input.querySelector("select").value, spectral.querySelector("select").value];
    const before = structuredClone(d.io.output_gamut_compress);
    choose(gamut.querySelector("select"), "cam16ucs");
    const after = negs.state().params.io.output_gamut_compress;
    gamut.querySelector(".reset").click();
    return { portra, options, set, reset, defaults, keptKnee: JSON.stringify(after.knee) === JSON.stringify(before.knee), algo: after.algorithm,
      resetAlgo: negs.state().params.io.output_gamut_compress.algorithm, cat16: !!ctl("CAT16 adaptation"),
      spectralOptions: [...spectral.querySelectorAll("option")].map((o) => o.value), paperRow: $("paper-time-row").hidden };`);
  check("development picker hidden for a single-time stock", controls.portra && controls.paperRow);
  check("development picker: Profile default and each time in minutes", JSON.stringify(controls.options) === JSON.stringify(["Profile default", "4 min", "5 min", "6.5 min", "9 min", "12 min"]), controls.options);
  check("development picker sets the time; a film change unsets it", controls.set === 6.5 && controls.reset === null, controls);
  check("Color Management choices with engine defaults", controls.defaults.every(Boolean) && controls.defaults[2] === "hanatos2025", controls.defaults);
  check("gamut algorithm change keeps the hidden knee; reset restores", controls.algo === "cam16ucs" && controls.keptKnee && controls.resetAlgo === controls.defaults[0], controls);
  check("no CAT16 control; three spectral methods", !controls.cat16 && controls.spectralOptions.join() === "hanatos2025,arctic2026beta04,mallett2019", controls);

  // A preset file with stocks, hidden values, Infinity and a RAW WB.
  const PRESET = JSON.stringify({
    simulation: { film_stock: "kodak_doublex", print_paper: "kodak_2302", print_exposure: 1.2345678 },
    film_chemistry: { development_time: 9 }, chemistry: { development_time: 3.5 },
    couplers: { langmuir_donor_k_rgb: ["INF", 0.37, "INF"] }, halation: { scatter_tail_weight: 0.123 },
    input_image: { rgb_to_raw_method: "mallett2019" }, output_gamut_compress: { algorithm: "aces_rgc" },
    load_raw: { white_balance: "tungsten" },
  }).replaceAll('"INF"', "Infinity");

  // Import with an empty editor: applies, then opens Sample 1.
  const empty = await ev(`
    negs.edit("post.brightness", 20);
    $("import-preset").click(); // the chooser; the file arrives as a change
    importFile("Warm afternoon.json", ${JSON.stringify(PRESET)});
    await until(() => negs.presetBusy()); const settled = await settle();
    const s = negs.state();
    return { settled, film: s.film, paper: s.paper, dev: [s.params.film_render.development_time, s.params.print_render.development_time],
      knee: s.params.film_render.dir_couplers.langmuir_donor_k_rgb, wb: s.params.io.raw_white_balance.mode, post: s.params.post.brightness,
      ui: [$("film").value, $("film-time").value, $("paper-time").value, ctl("Output gamut").querySelector("select").value, ctl("Spectral upsampling").querySelector("select").value],
      source: negs.source(), samples: samples(), msg: messages.find((m) => m.startsWith("Applied")), sample: $("sample-select").value };`);
  check("import in an empty editor opens Sample 1", empty.settled && empty.samples.join() === "DSC_6377.dng" && empty.sample === "DSC_6377", empty);
  check("import applies stocks without pairing, dev times, hidden values and Infinity", empty.film === "kodak_doublex" && empty.paper === "kodak_2302"
    && empty.dev.join() === "9,3.5" && empty.knee[0] === null && empty.knee[1] === 0.37, empty);
  check("import keeps Post", empty.post === 20, empty.post);
  check("imported RAW WB develops the sample", empty.wb === "tungsten" && empty.source.wb.mode === "tungsten" && empty.source.current, empty.source);
  check("inspector shows the imported values", empty.ui.join() === "kodak_doublex,9,3.5,aces_rgc,mallett2019", empty.ui);
  check("import acknowledged by name", /Applied “Warm afternoon”/.test(empty.msg), empty.msg);

  // Refusals leave the edit as it was.
  const refused = await ev(`
    const before = JSON.stringify(negs.state());
    const results = [];
    for (const [name, text] of [["Broken.json", "{"], ["Jz.json", '{"output_gamut_compress": {"algorithm": "jzazbz"}}'], ["Unknown.json", '{"simulation": {"film_stock": "kodak_nonexistent"}}']]) {
      importFile(name, text); await until(() => negs.presetBusy(), 500); await until(() => !negs.presetBusy());
      results.push(msg());
    }
    return { same: JSON.stringify(negs.state()) === before, results, samples: samples().length };`);
  check("refused files leave the edit unchanged", refused.same && refused.samples === 1, refused);
  check("refusals say why", /valid JSON/.test(refused.results[0]) && /jzazbz/.test(refused.results[1]) && /doesn't have the film “kodak_nonexistent”/.test(refused.results[2]), refused.results);

  // Reset after an import: the stocks' defaults, Post reset, stocks kept, RAW developed at As Shot.
  const reset = await ev(`
    negs.rotate(1); await settle();
    negs.reset(); const settled = await settle(); const s = negs.state();
    return { settled, film: s.film, paper: s.paper, rot: s.params.io.rotate_cw, dev: s.params.film_render.development_time, wb: s.params.io.raw_white_balance.mode,
      source: negs.source().wb.mode, post: s.params.post.brightness, algo: s.params.io.output_gamut_compress.algorithm, method: s.params.settings.rgb_to_raw_method };`);
  check("Reset after import keeps film, paper and rotation", reset.settled && reset.film === "kodak_doublex" && reset.paper === "kodak_2302" && reset.rot === 1, reset);
  check("Reset after import clears Lab overrides and Post", reset.dev === null && reset.post === 0 && reset.method === "hanatos2025", reset);
  check("Reset after import develops the RAW at As Shot", reset.wb === "as_shot" && reset.source === "as_shot", reset);

  // Export dialog: snapshot, name, link and file.
  const exp = await ev(`
    importFile("Warm afternoon.json", ${JSON.stringify(PRESET)}); await until(() => negs.presetBusy(), 1000); await settle();
    $("more").click(); $("export-preset").click();
    const opened = $("export-dialog").open, name = $("preset-name").value;
    await until(() => $("preset-link").value);
    const link = $("preset-link").value;
    $("preset-name").value = "Été · 夕方"; $("preset-name").dispatchEvent(new Event("input"));
    await until(() => $("preset-link").value && $("preset-link").value !== link);
    const renamed = $("preset-link").value;
    $("download-preset").click();
    const file = await downloads.at(-1);
    $("preset-name").value = "bad/name"; $("preset-name").dispatchEvent(new Event("input")); await wait(200);
    const invalid = { download: $("download-preset").disabled, copy: $("copy-link").disabled, problem: $("preset-name-problem").textContent };
    const rotBefore = negs.state().params.io.rotate_cw;
    key("r", $("preset-name")); key("r", $("download-preset")); await wait(50);
    const rot = negs.state().params.io.rotate_cw === rotBefore;
    $("export-dialog").querySelector("footer .btn").click(); await wait(50);
    return { opened, name, link, renamed, file, invalid, rot, closed: !$("export-dialog").open, focus: document.activeElement.id, samples: samples().length };`);
  check("export dialog prefilled with the imported name", exp.opened && exp.name === "Warm afternoon", exp.name);
  check("link prepared automatically, for this page", /^http:\/\/localhost:\d+\/#preset=[A-Za-z0-9_-]+$/.test(exp.link), exp.link);
  check("renaming updates the link", exp.renamed !== exp.link);
  check("download is <name>.json in the desktop format", exp.file.name === "Été · 夕方.json" && /"simulation"/.test(exp.file.text) && /Infinity/.test(exp.file.text) && !/"name"/.test(exp.file.text.slice(0, 40)), exp.file.name);
  check("invalid name disables download and link", exp.invalid.download && exp.invalid.copy && /can’t contain/.test(exp.invalid.problem), exp.invalid);
  check("dialog keys don't rotate the photograph", exp.rot, exp.rot);
  check("Close dismisses and restores focus to ⋯", exp.closed && exp.focus === "more", exp);
  const roundTrip = await ev(`
    const before = negs.state();
    importFile("Round trip.json", (await downloads.at(-1)).text); await until(() => negs.presetBusy(), 1000); await settle();
    const after = negs.state();
    return { same: JSON.stringify(after.params) === JSON.stringify(before.params) && after.film === before.film && after.paper === before.paper };`);
  check("exported file imports back to the same edit", roundTrip.same, roundTrip);

  // Incoming link, while Sample 1 is open: modal, Download and Close don't apply or fetch samples.
  const incoming = await ev(`
    negs.reset(); await settle();
    const before = JSON.stringify(negs.state()), sampleCount = samples().length;
    location.hash = new URL(${JSON.stringify(exp.renamed)}).hash;
    await until(() => $("incoming-dialog").open && !$("incoming-try").disabled);
    const shown = { name: $("incoming-name").textContent, stocks: $("incoming-stocks").textContent, hash: location.hash };
    $("incoming-download").click(); const file = await downloads.at(-1);
    key("c", $("incoming-download")); await wait(50);
    const cropping = !!negs.cropSession();
    $("incoming-dialog").querySelector(".dismiss").click(); await wait(100);
    return { shown, file: file.name, cropping, closed: !$("incoming-dialog").open, unchanged: JSON.stringify(negs.state()) === before, samples: samples().length - sampleCount };`);
  check("incoming link shows its name and stocks; fragment cleared", incoming.shown.name === "Été · 夕方" && /Double-X.* · /.test(incoming.shown.stocks) && !incoming.shown.hash, incoming.shown);
  check("incoming Download gives the file", incoming.file === "Été · 夕方.json", incoming.file);
  check("incoming Close applies nothing, no sample requests, no editor shortcuts", incoming.closed && incoming.unchanged && incoming.samples === 0 && !incoming.cropping, incoming);
  const tried = await ev(`
    location.hash = new URL(${JSON.stringify(exp.renamed)}).hash;
    await until(() => $("incoming-dialog").open && !$("incoming-try").disabled);
    $("incoming-try").click(); await until(() => negs.presetBusy(), 1000); const settled = await settle(); const s = negs.state();
    return { settled, film: s.film, wb: negs.source().wb.mode, sample: $("sample-select").value, closed: !$("incoming-dialog").open, msg: msg() };`);
  check("Try with Sample 1 already open still applies and develops", tried.settled && tried.film === "kodak_doublex" && tried.wb === "tungsten" && tried.sample === "DSC_6377" && tried.closed, tried);

  // A broken link explains and continues to the editor.
  const broken = await ev(`
    location.hash = "#preset=AQ-broken"; await until(() => $("incoming-dialog").open && !$("incoming-problem").hidden, 5000);
    const reason = $("incoming-reason").textContent;
    $("incoming-problem").querySelector(".btn").click(); await wait(50);
    return { reason, closed: !$("incoming-dialog").open };`);
  check("broken link gives a reason and continues", /damaged|incomplete/.test(broken.reason) && broken.closed, broken);

  // Fresh tab arriving with a link: the modal, no sample request until Try, which opens Sample 1.
  await send("Page.navigate", { url: exp.renamed.replace(/#/, "?fresh=1#") });
  await new Promise((r) => setTimeout(r, 3000));
  await ev(helpers);
  const fresh = await ev(`
    await engineReady(); await until(() => $("incoming-dialog").open && !$("incoming-try").disabled);
    await wait(300);
    const before = samples().length;
    $("incoming-try").click(); await until(() => negs.presetBusy(), 1000); const settled = await settle();
    return { before, settled, samples: samples(), film: negs.state().film, wb: negs.source()?.wb.mode };`);
  check("fresh tab: modal with zero sample requests before Try", fresh.before === 0, fresh);
  check("fresh tab: Try opens Sample 1 with the look", fresh.settled && fresh.samples.join() === "DSC_6377.dng" && fresh.film === "kodak_doublex" && fresh.wb === "tungsten", fresh);

  return { passed: log.filter((l) => l.ok).length, failed: failures.length, failures, log };
};
