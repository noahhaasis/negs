// Measurement only: how frame size and the kept working copy's size change
// frame times and memory. Runs inside the editor page:
//   await (await import("/checks/preview-bench.js")).default({ image: "/results/fuji.raf", tag: "x" })
// Every record is POSTed to /device-log?run=<tag> as it happens (so a killed
// tab keeps what it got), with `at` (ms since the epoch) to line up with
// web/checks/footprint.py's samples. `globalThis.__bench` holds progress.
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

export default async function bench({ image, tag = "bench", sizes = [1200, 2048, 3072, 4096, 8000], runs = 3, prep = true, films = true, exportFrame = true } = {}) {
  const negs = globalThis.negs;
  const state = (globalThis.__bench = { tag, done: false, records: [], error: null });
  const log = async (event, data = {}) => {
    const row = { at: Date.now(), event, ...data };
    state.records.push(row);
    await fetch(`/device-log?run=${encodeURIComponent(tag)}`, { method: "POST", body: JSON.stringify(row) }).catch(() => {});
  };
  const idle = async () => { while (!negs.idle() || negs.source()?.busy) await wait(50); };
  try {
    await log("start", { ua: navigator.userAgent, cores: navigator.hardwareConcurrency, isolated: crossOriginIsolated,
      engine: document.getElementById("engine-info")?.textContent, kept: negs.keptPixels(), dpr: devicePixelRatio,
      screen: [screen.width, screen.height], image });
    const t0 = performance.now();
    await negs.open(image);
    await negs.whenDeveloped();
    await idle();
    const photo = negs.photo();
    const first = negs.renders.at(-1);
    await log("open", { ms: performance.now() - t0, photo, firstFrame: first,
      timeline: negs.timeline.map((e) => ({ event: e.event, t: Math.round(e.t - t0) })).filter((e) => e.t >= 0) });
    const long = Math.max(photo.width, photo.height);
    // Settled frames vary the print exposure, so each renders again without
    // re-metering; interactive ones are the editor's slider-drag frames.
    const cases = [["plain", {}]];
    if (prep) {
      cases.push(["prepExposure", { prep: { exposure_ev: 0.7 } }]);
      cases.push(["prepFull", { prep: { exposure_ev: 0.7, highlights: -50, shadows: 30, midtone_detail: 40 } }]);
      // A Prep slider moving: Prep runs again every frame.
      cases.push(["prepSlider", { prep: { exposure_ev: 0.7, highlights: -50, shadows: 30, midtone_detail: 40 } },
        (i) => ({ prep: { exposure_ev: 0.7, highlights: -50, shadows: 30 + i, midtone_detail: 40 } })]);
    }
    const base = negs.state().params.enlarger.print_exposure;
    for (const side of [...new Set(sizes.map((s) => Math.min(s, long)))]) {
      await negs.frame(side); // the source resampled for this size, once
      for (const [name, patch, vary] of cases) {
        for (const interactive of [false, true]) {
          const frames = [];
          for (let i = 0; i < runs; i++) {
            const step = vary ? vary(i + (interactive ? runs : 0)) : { enlarger: { print_exposure: base + 0.01 * (i + 1) } };
            frames.push(await negs.frame(side, interactive, { ...patch, ...step }));
          }
          const f = frames.at(-1);
          await log("frame", { side, case: name, interactive, width: f.width, height: f.height, mp: +(f.width * f.height / 1e6).toFixed(2),
            wallMs: Math.round(median(frames.map((x) => x.wallMs))), engineMs: Math.round(median(frames.map((x) => x.elapsedMs))),
            all: frames.map((x) => Math.round(x.wallMs)), stages: f.stages, presented: f.presented, backend: f.backend, memoryMiB: f.memoryMiB });
        }
      }
      if (films && side === Math.min(sizes[0], long)) {
        // A new spectral pipeline each: what a film change costs at this size.
        for (const [film, paper] of [["kodak_gold_200", "kodak_portra_endura"], ["fujifilm_pro_400h", "fujifilm_crystal_archive_typeii"], ["kodak_portra_400", "kodak_portra_endura"]]) {
          const f = await negs.frame(side, false, {}, film, paper);
          await log("film", { side, film, wallMs: Math.round(f.wallMs), engineMs: Math.round(f.elapsedMs), stages: f.stages, memoryMiB: f.memoryMiB });
        }
      }
    }
    if (exportFrame) {
      for (const [name, patch] of prep ? [["plain", {}], ["prepFull", cases.find((c) => c[0] === "prepFull")[1]]] : [["plain", {}]]) {
        const t = performance.now();
        const f = await negs.frame(long, false, patch);
        await log("full", { case: name, side: long, wallMs: Math.round(performance.now() - t), engineMs: Math.round(f.elapsedMs), stages: f.stages, memoryMiB: f.memoryMiB });
      }
    }
    await log("done");
  } catch (e) {
    state.error = String(e?.stack ?? e);
    await log("error", { error: state.error });
  } finally {
    state.done = true;
  }
  return state.records;
}
