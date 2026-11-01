// Optional, local-only film walkthrough. The adapter owns photographs and
// rendering; this module owns copy, session transitions and accessible UI.
export const TUTORIAL_VERSION = 1;
export const STEPS = [
  { id: "film", tab: "stock", target: "film", title: "Choose your film", text: "Each film responds differently to light and colour. Your choice shapes the photo’s colours, contrast, and grain. Start here to find a character you like—then use the other controls to fine-tune it." },
  { id: "paper", tab: "stock", target: "paper", title: "Choose your print", text: "In a darkroom, light passes through the negative onto photographic paper. The paper adds its own colour and contrast, changing the final photo. Try a few papers with the same film—the difference can be surprisingly large." },
  { id: "metering", tab: "camera", paths: ["camera.auto_exposure_method"], title: "Decide where to measure", text: "Like a camera’s light meter, this decides which parts of the photo guide automatic exposure. Center-weighted favours the middle; Highlight-weighted favours bright areas and usually gives a darker result. Try another mode if the starting exposure feels off." },
  { id: "exposure", tab: "camera", paths: ["camera.exposure_compensation_ev"], title: "Change how the film sees light", text: "This changes how much light reaches the simulated film, affecting colour, contrast, and grain. With the enlarger’s Exposure compensation on, print brightness stays roughly steady. Use Print exposure later to make the finished photo lighter or darker." },
  { id: "format", tab: "camera", paths: ["camera.film_format_mm"], title: "Small film, bigger texture", text: "Smaller film needs more enlargement to make the same-sized picture, so grain and glow appear larger. Lower this value for more visible texture; raise it for a finer look. It doesn’t crop your photo." },
  { id: "halation", tab: "film", group: "Film/Halation", paths: ["film_render.halation.halation_amount", "film_render.halation.halation_spatial_scale"], title: "Let highlights glow", text: "Some light passes through film and reflects back into it, creating a warm glow around bright highlights. Raise Amount for a stronger glow, or Spatial scale to spread it wider. Look around lamps, reflections, and bright edges." },
  { id: "dir", tab: "film", group: "Film/DIR Couplers", paths: ["film_render.dir_couplers.amount"], title: "Colour shaped by chemistry", text: "During development, chemicals released in one colour layer slow development in nearby layers. This helps separate colours and makes edges look crisper. Lower Amount for gentler colour and less edge contrast." },
  { id: "print", tab: "print", group: "Print/Enlarger", paths: ["enlarger.print_exposure"], title: "Set the final brightness", text: "An enlarger shines light through the negative to make a print. On ordinary photographic paper, more light makes the print darker. Lower Print exposure to brighten your photo; raise it to darken it." },
  { id: "preset", tab: "stock", target: "preset-menu-anchor", title: "Keep a look you like", text: "Save your settings as a preset to reuse them on other photos. Export it to share the look, or send it as a link.", hint: "Open ⋯ → Export preset. Presets include Lab settings, not your photo, Post adjustments, crop or border." },
];

export function tutorialChoice(value) {
  return !!value && typeof value === "object" && ["started", "dismissed", "completed"].includes(value.status);
}

// End aborts immediately, but restoration waits for in-flight decode/render
// work to drain. No late sample can then replace the restored photograph.
export function createTutorialSession(adapter, changed = () => {}) {
  let phase = "idle", index = 0, error = "", saved = null, controller = null, task = null, ending = null;
  const state = () => ({ phase, index, error });
  const notify = () => changed(state());
  const choice = (status) => adapter.remember?.({ version: TUTORIAL_VERSION, status });
  function run(operation) {
    controller = new AbortController();
    const signal = controller.signal;
    task = (async () => {
      try {
        await operation(signal);
        signal.throwIfAborted();
        phase = "active";
      } catch (e) {
        if (signal.aborted) return;
        error = e.message || String(e);
        phase = "error";
      } finally { notify(); }
    })();
    return task;
  }
  function start() {
    if (!["idle", "error"].includes(phase)) return task;
    error = "";
    if (saved === null) saved = adapter.capture();
    index = 0;
    phase = "preparing";
    choice("started");
    notify();
    return run(async (signal) => {
      await adapter.enter(signal);
      signal.throwIfAborted();
      await adapter.step(STEPS[0], signal);
    });
  }
  function go(next) {
    if (phase !== "active" || next < 0 || next >= STEPS.length) return task;
    index = next;
    phase = "transitioning";
    notify();
    return run((signal) => adapter.step(STEPS[index], signal));
  }
  function end(completed = false) {
    if (ending) return ending;
    if (phase === "idle") return Promise.resolve();
    controller?.abort();
    adapter.cancel();
    phase = "restoring";
    error = "";
    choice(completed ? "completed" : "dismissed");
    notify();
    ending = (async () => {
      try {
        await task;
        await adapter.restore(saved, { completed });
        saved = null;
        phase = "idle";
        if (completed) adapter.completed?.();
      } catch (e) {
        error = `Couldn't restore the editor: ${e.message || e}`;
        phase = "restore-error";
      } finally { ending = null; notify(); }
    })();
    return ending;
  }
  return { state, start, go, end, reset: () => go(index), retry: () => phase === "restore-error" ? end() : start() };
}

export function createTutorial(adapter) {
  const $ = (id) => document.getElementById(id);
  const root = $("app"), guide = $("tutorial-guide"), invitation = $("tutorial-invitation"), normal = $("empty-normal");
  const confirm = $("tutorial-confirm");
  let known = tutorialChoice(adapter.read()), suppressed = false, previous = "idle";
  function dismissInvitation(persist = false) {
    suppressed = true;
    invitation.hidden = true;
    normal.hidden = false;
    root.classList.remove("tutorial-welcome");
    if (persist) {
      known = true;
      adapter.remember({ version: TUTORIAL_VERSION, status: "dismissed" });
    }
  }
  function invite() {
    invitation.hidden = known || suppressed || adapter.launchIntent();
    normal.hidden = !invitation.hidden;
    root.classList.toggle("tutorial-welcome", !invitation.hidden);
  }
  function viewportChanged() {
    const viewport = window.visualViewport;
    root.style.setProperty("--tutorial-height", `${viewport && viewport.scale === 1 ? viewport.height : innerHeight}px`);
    if (session.state().phase !== "idle") adapter.resize?.();
  }
  const session = createTutorialSession({ ...adapter, remember(value) {
    known = true;
    dismissInvitation();
    adapter.remember(value);
  } }, (s) => {
    const active = s.phase !== "idle", busy = ["preparing", "transitioning", "restoring"].includes(s.phase);
    root.classList.toggle("tutorial-mode", active);
    root.classList.toggle("tutorial-busy", busy || s.phase.endsWith("error"));
    guide.hidden = !active;
    $("tutorial-heading").hidden = !active;
    $("tutorial-nav").hidden = !active;
    $("tutorial").hidden = active;
    $("tour-progress").textContent = `Tutorial · ${s.index + 1} of ${STEPS.length}`;
    const step = STEPS[s.index];
    $("tour-title").textContent = s.phase === "preparing" ? "Loading sample…" : s.phase === "restoring" ? "Returning to the editor…" : s.phase.endsWith("error") ? "Something went wrong" : step.title;
    $("tour-copy").textContent = s.error || (s.phase === "preparing" ? "Uses a sample photo. About 2 minutes, or take your time. Try the controls as you go. Each step starts fresh." : s.phase === "restoring" ? "Restoring your photo, edits and layout." : step.text);
    $("tour-hint").textContent = step.hint || "";
    $("tour-hint").hidden = !step.hint || s.phase !== "active";
    $("tour-next").textContent = s.index === STEPS.length - 1 ? "Finish" : "Next";
    for (const id of ["tour-next", "tour-back", "tour-reset"]) $(id).disabled = s.phase !== "active";
    $("tour-back").disabled ||= s.index === 0;
    $("tour-reset").hidden = s.index === STEPS.length - 1;
    $("tour-end").disabled = s.phase === "restoring";
    $("tour-end").textContent = s.phase === "preparing" ? "Cancel" : "End tour";
    $("tour-retry").hidden = !s.phase.endsWith("error");
    $("tour-status").textContent = s.phase === "active" ? `Step ${s.index + 1} of ${STEPS.length}: ${step.title}` : $("tour-title").textContent;
    if (s.phase === "active" && previous !== "active") $("tour-title").focus({ preventScroll: true });
    if (!active && previous !== "idle") $("tutorial").focus({ preventScroll: true });
    previous = s.phase;
  });
  function launch() {
    const reason = adapter.unavailable();
    if (reason) return adapter.message(reason, true);
    if (adapter.hasPhoto()) confirm.showModal();
    else void session.start();
  }
  $("tutorial").addEventListener("click", launch);
  $("start-tutorial").addEventListener("click", launch);
  $("dismiss-tutorial").addEventListener("click", () => dismissInvitation(true));
  $("confirm-tutorial").addEventListener("click", () => { confirm.close(); void session.start(); });
  $("tour-next").addEventListener("click", () => session.state().index === STEPS.length - 1 ? session.end(true) : session.go(session.state().index + 1));
  $("tour-back").addEventListener("click", () => session.go(session.state().index - 1));
  $("tour-reset").addEventListener("click", () => session.reset());
  $("tour-end").addEventListener("click", () => session.end());
  $("tour-retry").addEventListener("click", () => session.retry());
  for (const id of ["open", "open-empty", "open-sample"]) $(id).addEventListener("click", () => dismissInvitation());
  document.addEventListener("keydown", (ev) => {
    if (ev.defaultPrevented || ev.key !== "Escape" || session.state().phase === "idle" || document.querySelector("dialog[open]") || !$("preset-menu").hidden || ev.target.matches("input:not([type=range], [type=checkbox]), select, textarea, [contenteditable]")) return;
    ev.preventDefault();
    void session.end();
  });
  window.visualViewport?.addEventListener("resize", viewportChanged);
  addEventListener("resize", viewportChanged);
  viewportChanged();
  invite();
  return { ...session, suppressInvitation: dismissInvitation };
}
