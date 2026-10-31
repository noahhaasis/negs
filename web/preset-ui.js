// The presets' menu and dialogs (docs/web-presets-design.md): one preset
// file imported and applied at a time, the edit exported as a file and a
// link, and an incoming link's dialog. No library, list or undo; Reset
// Adjustments returns to the stocks' defaults. The shared Rust importer and
// exporter (the engine's importPreset, exportPreset) read and write the
// files; app.js owns the edit and passes what this needs.
import { MAX_FILE_BYTES, LinkError, decodeLink, encodeLink, linkUrl, linkValue, nameFrom, nameProblem, stem } from "./presets.js";

export function createPresets(app) {
  const $ = (id) => document.getElementById(id);
  const menu = $("preset-menu"), more = $("more");
  const exportDialog = $("export-dialog"), incomingDialog = $("incoming-dialog");
  const items = () => [...menu.querySelectorAll("[role=menuitem]")];
  // Applying runs one at a time; a newer one makes an older one's result stale.
  let applying = null, applies = 0;
  // The name of the preset last imported, to name an export after.
  let importedName = null;
  // A LUT export is running (one at a time).
  let lutting = false;

  const ready = () => app.ready() && !applying;

  // ---- The ⋯ menu --------------------------------------------------------------

  function openMenu(focus = 0) {
    sync();
    menu.hidden = false;
    more.setAttribute("aria-expanded", "true");
    const enabled = items().filter((b) => !b.disabled);
    (enabled.at(focus) ?? menu).focus?.();
  }
  function closeMenu(restore = true) {
    if (menu.hidden) return;
    menu.hidden = true;
    more.setAttribute("aria-expanded", "false");
    if (restore) more.focus();
  }
  more.addEventListener("click", () => (menu.hidden ? openMenu() : closeMenu()));
  more.addEventListener("keydown", (ev) => {
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      openMenu(ev.key === "ArrowDown" ? 0 : -1);
    }
  });
  menu.addEventListener("keydown", (ev) => {
    const enabled = items().filter((b) => !b.disabled);
    const i = enabled.indexOf(document.activeElement);
    const move = { ArrowDown: 1, ArrowUp: -1 }[ev.key];
    if (move) enabled[(i + move + enabled.length) % enabled.length]?.focus();
    else if (ev.key === "Home") enabled[0]?.focus();
    else if (ev.key === "End") enabled.at(-1)?.focus();
    else if (ev.key === "Escape") closeMenu();
    else if (ev.key === "Tab") closeMenu(false);
    else return;
    ev.preventDefault();
    ev.stopPropagation();
  });
  document.addEventListener("pointerdown", (ev) => {
    if (!menu.hidden && !$("preset-menu-anchor").contains(ev.target)) closeMenu(false);
  });
  $("import-preset").addEventListener("click", () => {
    closeMenu();
    if (!ready()) return;
    $("preset-file").value = "";
    $("preset-file").click();
  });
  $("export-preset").addEventListener("click", () => {
    closeMenu(false);
    void showExport();
  });
  $("export-lut").addEventListener("click", () => {
    closeMenu();
    void exportLut();
  });

  function sync() {
    $("preset-menu-anchor").hidden = app.cropping();
    more.disabled = !app.ready();
    for (const b of items()) b.disabled = !ready();
    $("export-lut").disabled ||= lutting;
  }

  // ---- Applying ---------------------------------------------------------------------

  // Applies preset `text` named `name`: validated and mapped by the shared
  // importer before anything changes, then the film, paper and params at
  // once (no film pairing or stock resets), keeping what a preset doesn't
  // carry. With `sample`, or no photograph open, Sample 1 opens with it.
  // Resolves to whether it applied.
  async function apply(text, name, { sample = false } = {}) {
    if (applying) return false;
    const apply = ++applies;
    let done;
    applying = new Promise((r) => { done = r; });
    sync();
    try {
      await app.beforeApply?.();
      // Over the edit as it is when the result arrives: an edit meanwhile
      // (a Post slider) is what the preset keeps.
      let r, current;
      do {
        current = JSON.stringify(app.state().params);
        r = await app.call("importPreset", text, current);
        if (apply !== applies) return false;
      } while (JSON.stringify(app.state().params) !== current);
      const opening = sample || !app.hasPhoto() ? app.openSample() : null;
      app.install({ film: r.film, paper: r.paper, params: JSON.parse(r.params) });
      importedName = name;
      await opening;
      if (app.hasPhoto()) {
        try {
          await app.developed();
        } catch (e) {
          app.message(`Applied “${name}”, but its RAW white balance couldn’t be applied: ${e.message}. The white balance is back to the photograph’s.`, true);
          return true;
        }
      }
      app.message(`Applied “${name}”.`, false, 4000);
      return true;
    } catch (e) {
      app.message(`Couldn’t apply “${name}”: ${e.message}`, true);
      return false;
    } finally {
      applying = null;
      done();
      sync();
    }
  }

  $("preset-file").addEventListener("change", async () => {
    const file = $("preset-file").files[0];
    if (!file) return;
    const name = nameFrom(stem(file.name));
    if (file.size > MAX_FILE_BYTES) {
      app.message(`Couldn’t apply “${name}”: the file is too large to be a preset.`, true);
      return;
    }
    let text;
    try {
      text = await file.text();
    } catch (e) {
      app.message(`Couldn’t read “${file.name}”: ${e.message}`, true);
      return;
    }
    await apply(text, name);
  });

  // ---- Saving a file ------------------------------------------------------------------

  // Downloads `text` as `name`.json (or `filename` of `type`); a home-screen
  // app on iOS, which has no downloads, shares the file instead (as exports
  // do).
  function save(name, text, { filename = `${name}.json`, type = "application/json", what = "the preset" } = {}) {
    const file = new File([text], filename, { type });
    if (app.shareFiles() && navigator.canShare?.({ files: [file] })) {
      navigator.share({ files: [file] }).catch((e) => e.name !== "AbortError" && app.message(`Couldn’t save ${what}: ${e.message}`, true));
      return;
    }
    const a = Object.assign(document.createElement("a"), { href: URL.createObjectURL(file), download: file.name });
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
  }

  // ---- LUT export --------------------------------------------------------------------------

  // The edit's film/paper look as desktop's Export LUT writes it
  // (spektrafilm_core::lut_export): 33³, sRGB in and out, image-independent.
  async function exportLut() {
    if (!ready() || lutting) return;
    const { film, paper, params } = app.state();
    const filename = `${film}_${params.io?.scan_film ? "direct_scan" : paper}.cube`;
    lutting = true;
    sync();
    app.message("Exporting LUT…");
    try {
      const text = await app.call("exportLut", film, paper, JSON.stringify(params), 33);
      save(filename, text, { filename, type: "application/octet-stream", what: "the LUT" });
      app.message(`Exported ${filename}.`, false, 4000);
    } catch (e) {
      app.message(`Couldn’t export the LUT: ${e.message}`, true);
    } finally {
      lutting = false;
      sync();
    }
  }

  // ---- Export dialog --------------------------------------------------------------------

  // One snapshot of the edit per dialog: its file and link describe the
  // same look while the name changes.
  let exportText = null, exports = 0, links = 0, linkTimer = 0;
  const nameField = $("preset-name"), linkField = $("preset-link"), linkStatus = $("preset-link-status");

  async function showExport() {
    if (!ready() || exportDialog.open) return;
    const { film, paper, params } = app.state();
    const opened = ++exports;
    exportText = null;
    nameField.value = importedName ?? nameFrom(app.stockName(film) ?? film);
    linkField.value = "";
    $("copy-link").disabled = true;
    $("download-preset").disabled = true;
    linkStatus.textContent = "Preparing link…";
    showName();
    exportDialog.showModal();
    nameField.select();
    try {
      const text = await app.call("exportPreset", film, paper, JSON.stringify(params));
      if (opened !== exports) return;
      exportText = text;
      showName();
      makeLink();
    } catch (e) {
      if (opened !== exports) return;
      linkStatus.textContent = "";
      showProblem(`Couldn’t export the preset: ${e.message}`);
    }
  }
  function showProblem(text) {
    $("preset-name-problem").textContent = text ?? "";
    $("preset-name-problem").hidden = !text;
  }
  function showName() {
    const problem = nameProblem(nameField.value);
    showProblem(problem);
    nameField.setAttribute("aria-invalid", String(!!problem));
    $("download-preset").disabled = !exportText || !!problem;
  }
  function makeLink() {
    clearTimeout(linkTimer);
    const made = ++links;
    linkField.value = "";
    $("copy-link").disabled = true;
    if (!exportText) return;
    if (nameProblem(nameField.value)) {
      linkStatus.textContent = "Enter a valid name for a link.";
      return;
    }
    linkStatus.textContent = "Preparing link…";
    const name = nameField.value, text = exportText;
    linkTimer = setTimeout(async () => {
      try {
        const value = await encodeLink(name, text);
        if (made !== links || !exportDialog.open) return;
        linkField.value = linkUrl(location, value);
        linkStatus.textContent = "";
        $("copy-link").disabled = false;
      } catch (e) {
        if (made !== links) return;
        linkStatus.textContent = e instanceof LinkError ? e.message : `Couldn’t make a link: ${e.message}`;
      }
    }, 120);
  }
  nameField.addEventListener("input", () => { showName(); makeLink(); });
  nameField.addEventListener("keydown", (ev) => {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    $("download-preset").click();
  });
  $("download-preset").addEventListener("click", () => {
    if (!exportText || nameProblem(nameField.value)) return;
    save(nameField.value, exportText);
  });
  $("copy-link").addEventListener("click", async () => {
    const url = linkField.value;
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      linkStatus.textContent = "Link copied.";
    } catch {
      // Without clipboard access, select it to copy by hand.
      linkField.focus();
      linkField.select();
      linkStatus.textContent = document.execCommand?.("copy") ? "Link copied." : "Select the link and copy it.";
    }
  });
  linkField.addEventListener("focus", () => linkField.select());
  exportDialog.addEventListener("close", () => {
    exports++;
    links++;
    clearTimeout(linkTimer);
    exportText = null;
    more.focus();
  });

  // ---- Incoming link ----------------------------------------------------------------------

  // A link's preset shows over the editor; nothing opens, downloads or
  // renders until Try. Each link (or closing) makes an earlier one's
  // pending work stale.
  let incoming = null, arrivals = 0, returnFocus = null;

  async function arrive() {
    const value = linkValue(location.hash);
    if (value === null) return;
    // Read once: reloading or sharing the address afterwards doesn't bring
    // it back, and a later link to the same preset is a new arrival.
    history.replaceState(history.state, "", location.pathname + location.search);
    const arrival = ++arrivals;
    incoming = null;
    showIncoming({ loading: true });
    try {
      const { name, text } = await decodeLink(value);
      if (arrival !== arrivals) return;
      // The shared importer validates it (against the edit as it is; the
      // edit doesn't change).
      await app.engineReady();
      const r = await app.call("importPreset", text, JSON.stringify(app.state().params));
      if (arrival !== arrivals) return;
      incoming = { name, text, arrival };
      const film = app.stockName(r.film) ?? r.film, paper = app.stockName(r.paper) ?? r.paper;
      showIncoming({ name, stocks: JSON.parse(r.params).io.scan_film ? `${film} · scanned directly` : `${film} · ${paper}` });
    } catch (e) {
      if (arrival !== arrivals) return;
      showIncoming({ problem: e instanceof LinkError ? e.message : `This preset can’t be used: ${e.message}` });
    }
  }
  function showIncoming({ loading = false, name = "", stocks = "", problem = null }) {
    $("incoming-preset").hidden = !!problem;
    $("incoming-problem").hidden = !problem;
    $("incoming-reason").textContent = problem ?? "";
    $("incoming-name").textContent = loading ? "Reading the preset…" : name;
    $("incoming-stocks").textContent = stocks;
    $("incoming-try").disabled = loading;
    $("incoming-download").disabled = loading;
    if (!incomingDialog.open) {
      if (exportDialog.open) exportDialog.close();
      returnFocus = document.activeElement;
      incomingDialog.showModal();
    }
    (problem ? incomingDialog.querySelector("#incoming-problem .btn") : $("incoming-try")).focus();
  }
  $("incoming-try").addEventListener("click", async () => {
    const preset = incoming;
    if (!preset || preset.arrival !== arrivals) return;
    incoming = null;
    incomingDialog.close();
    // Waits for an apply already running, rather than dropping the user's
    // Try; a link arriving meanwhile supersedes it.
    const tried = arrivals;
    while (applying) await applying;
    if (tried !== arrivals) return;
    await apply(preset.text, preset.name, { sample: true });
  });
  $("incoming-download").addEventListener("click", () => {
    if (incoming) save(incoming.name, incoming.text);
  });
  incomingDialog.addEventListener("close", () => {
    arrivals++;
    (returnFocus?.isConnected ? returnFocus : more).focus?.();
  });
  addEventListener("hashchange", () => void arrive());

  return {
    sync,
    arrive,
    // For checks (web/checks/presets.mjs).
    apply,
    busy: () => !!applying,
  };
}
