// Physical-device sequence for the bounded RAW prototype. Start a FRESH tab
// with ?rawDevelop=legacy|bounded|force and save its asset hashes separately.
//   RAW_FIXTURE=results/3980774776.dng node web/safari.mjs run web/checks/raw-bounded.mjs
// Open includes a URL download, NOT the native Files picker. `timeline` measures
// import-to-first-frame separately from that download and driver polling.
// RAW_PHASES=open-1,smoke runs a short batch instead of the full sequence.
// SAFARI_SCREENSHOTS=0 disables all controller captures (including on failure).
// RAW_SCREENSHOTS=0 also skips ipad.mjs's end-of-phase captures: on this iPhone
// webinspectord was killed while taking those screenshots, not while rendering.
import editorSuite from "./ipad.mjs";

export default async function (test) {
  const fixture = process.env.RAW_FIXTURE;
  if (!fixture) throw new Error("Set RAW_FIXTURE to a served RAW URL");
  const allowed = ["open-1", "open-2", "smoke", "zoom", "export", "after-export", "open-3", "open-4", "original"];
  const phases = process.env.RAW_PHASES?.split(",").map(p => p.trim())
    ?? allowed.filter(p => p !== "smoke");
  if (!phases.length || phases.some(p => !allowed.includes(p))) throw new Error("Invalid RAW_PHASES; choose from " + allowed.join(","));
  const initial = await test.probe();
  async function capture(label) {
    const state = await test.probe();
    test.assertHealthy(state);
    if (state.engineInstances !== initial.engineInstances) throw new Error("Unexpected engine replacement");
    if (initial.engine.startsWith("wgpu") && state.recentFrames.at(-1)?.presented !== true) throw new Error("GPU frame was not presented");
    if (label.startsWith("open") && process.env.RAW_EXPECT_ROUTE) {
      const decoded = state.recentLogs.filter(l => l.text.startsWith("decoded in a worker")).at(-1)?.text;
      const route = decoded?.match(/RAW develop: (\w+)/)?.[1] ?? (decoded ? "original" : "missing");
      if (route !== process.env.RAW_EXPECT_ROUTE) throw new Error(`Expected ${process.env.RAW_EXPECT_ROUTE} route, got ${route}`);
      test.log("raw-route", { label, route, decoded });
    }
    test.log("raw-metrics", { label, result: await test.execute(`return {
      timeline: negs.timeline, frames: negs.renders.slice(-2), original: negs.originalMade(),
      query: location.search, engine: document.getElementById('engine-info').textContent,
      resources: performance.getEntriesByType('resource').filter(r => ['.wasm', '.js', '.mjs'].some(ext => r.name.split('?')[0].endsWith(ext))).map(r => r.name)
    };`) });
  }
  for (const phase of phases) {
    if (phase.startsWith("open")) {
      await test.step(`${phase} ${fixture}`, () => test.execute("negs.open(arguments[0]).catch(e => console.error(e));", fixture));
    } else if (phase === "original") {
      // Separate from the primary A/B sequence: original view must work too,
      // but do not add it before export on only one arm of the comparison.
      await test.execute("negs.prepareOriginal().catch(e => console.error(e));");
      await test.waitFor("!!negs.originalMade()");
      await test.execute("negs.original(true);");
      await test.wait(300);
      await test.execute("negs.original(false);");
    } else {
      await editorSuite({ ...test, env: { IPAD_PHASE: phase },
        ...(process.env.RAW_SCREENSHOTS === "0" ? { screenshot: async (name) => {
          test.log("screenshot-skipped", { name, reason: "RAW_SCREENSHOTS=0" });
          return null;
        } } : {}) });
    }
    await capture(phase);
  }
  return { fixture, phases };
}
