# Web MVP: handoff for piece 4

For the agent picking up the browser version of Negs at piece 4 of
`docs/web-mvp-plan.md`. Read the plan, `docs/web-wasm.md`,
`docs/web-wasm-handoff.md` (build, serve, tools, gotchas) and
`docs/web-feature-inventory.md` first; this file adds where things stand
and what piece 4 touches.

## Where things stand

Worktree `/home/noah/Work/Negs-wasm`, branch `wasm-investigation`. Commit
each finished piece there; **don't push and don't merge to `main`** until
the owner says so (this overrides AGENTS.md's commit-to-main rule for this
branch). Commits end with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

| Piece | Commit | State |
|---|---|---|
| 1. Full params and the engine catalog | `9807e6c` | done |
| 2. All Lab controls, desktop layout | `feda605` | done, 🛑 reviewed |
| HTTPS for device testing | `d350950` | done |
| 3. iPad and phone layout | `842da9c` | done; the owner moved on to piece 4 |
| 4. Enlarger shifts without a rebuild, Balance Print | see `git log` | done (`docs/web-wasm.md` item 12) |
| **5. Colour management and output colour space** | | **next** |
| 6. Resident scan, zoom, true-resolution preview | | 🛑 after it |
| 7. Placeholder while developing | | Stage 1 🛑 after it |

`docs/web-wasm.md` items 9–11 describe pieces 1–3 with their measurements.

Open from piece 3, for the next report rather than for piece 4:

- **Nobody has opened it on a real iPad or iPhone yet.** `web/dev-cert.sh`
  and `node web/serve.mjs --https` are ready (steps in the handoff, "On an
  iPad or iPhone"), but this machine's ufw blocks ports 8137 and 8443
  from the LAN; opening them is the owner's call. The plan says that if
  Safari has no WebGPU in workers, moving the GPU engine to the main
  thread comes before more UI work. If the owner reports that, do it
  before piece 4.
- The 24 MP ARW took 954 MiB in the engine at a phone's preview size
  (piece 6's memory caps).

## Ground rules that bite in piece 4

- **Native output must stay byte-identical.** Piece 4 changes the engine
  (`spektrafilm-core`), so render the CLI before and after with
  `SPEKTRAFILM_BACKEND=wgpu` and `cpu` and compare, and run
  `cargo check --workspace --all-targets` plus the core, gpu, math, model,
  io and spektrafilm-native tests. The CLI command is in the handoff under
  "Build, run, test"; `source linux/core-env.sh` first.
- **Frames stay presented:** no `ChainOutput::Scan` readback and no CPU
  fallback for an sRGB preview. `negs.renders` and the stage list show it.
- **Benchmark before and after:** test card at 200, 1200 and 2048 px and
  the Sony ARW (`web/bench.sh`). Baselines are in `web/results/base-*`,
  and the latest numbers in `docs/web-wasm.md` item 11 (card 9–11 /
  28–31 / 57–74 ms, ARW 72–83 ms). Default renders must stay
  byte-identical to `web/results/base-*.ppm` (`node web/compare.mjs`).
- **Desktop parity:** follow macOS where the apps differ, and note it.

## Piece 4: what the plan asks

> Y/M filter shifts, preflash and print exposure compensation are in
> `preview_pipeline_cache_key`, so in the browser every move rebuilds the
> spectral pipeline. Measure the rebuild first. Then update a built
> pipeline instead, as `Pipeline::set_print_filter_shifts` (`pub(crate)`)
> does for the balance solver, extended to preflash and compensation if
> they allow it, and key the pipeline cache without them. The result must
> equal a full rebuild with the same params (byte-identical, or explain
> the difference). Balance Print: export the helper's `balance_print`
> (`spektrafilm_core::print_balance`) from wasm and add the button.
>
> Done when: a shift slider moves at frame rate with no rebuild in the
> stage list; the rebuilt-vs-updated comparison is in the docs.

There is no 🛑 after piece 4: commit it and go on to piece 5. Stop at the
🛑 after piece 6.

### The rebuild, already measured (piece 2)

Test card at a 1200 px view, from `web/checks/controls.js` (a `pipeline`
stage is a rebuild):

| Control | Rebuild | Frame |
|---|---|---|
| Exposure compensation (toggle) | 146 ms | 165 ms |
| Magenta shift, Yellow shift | 117 ms | 137–158 ms |
| Preflash exposure, magenta, yellow | 117 ms | 151–188 ms |

Other controls are Resident at 13–56 ms warm. Re-measure on your machine
before changing anything.

### What I found in the code (not yet acted on)

- **The key:** `preview_pipeline_cache_key` in
  `crates/spektrafilm-core/src/preview.rs`. Its `enlarger` block holds
  `m_filter_shift`, `y_filter_shift`, `preflash_exposure`,
  `preflash_m_filter_shift`, `preflash_y_filter_shift`,
  `normalize_print_exposure` and `print_exposure_compensation`.
  - It has three callers: the wasm render
    (`crates/spektrafilm-wasm/src/lib.rs`, `render`, around line 318), the
    native helper (`crates/spektrafilm-native/src/main.rs` around line 440,
    which the macOS and Linux apps run) and `crates/spektrafilm-gui/src/main.rs:551`.
  - Changing the shared key changes the desktop helper's caching too. That
    should be a speed-up with identical output if updating equals
    rebuilding, but verify the helper's output is unchanged. A web-only key
    is the alternative; say which you chose and why.
- **A pipeline is reused per render:** the wasm render clones the cached
  pipeline and calls `Pipeline::with_params(params)`
  (`crates/spektrafilm-core/src/pipeline.rs`). That already re-applies the
  film curves morph and reruns `compute_print_exposure_factor`, which reads
  `normalize_print_exposure`, `print_exposure_compensation` and the
  exposure EV from the params.
  - So **exposure compensation and normalize may need no new code**, only
    leaving the key.
  - The test `with_params_recalibrates_print_exposure_like_a_new_pipeline`
    in `crates/spektrafilm-core/src/stages/mod.rs` already checks that for
    compensation. Confirm nothing else built in `new_with_spectral` reads
    them.
- **M/Y shifts:** `new_with_spectral` builds `print_illuminant` from the
  neutral CMY (`neutral_cmy`) plus the shifts
  (`enlarger_filtered_illuminant_f64`, around line 565).
  `set_print_filter_shifts` (around line 233) redoes exactly that and then
  the exposure factor. `with_params` today swaps the params without
  re-filtering, so the illuminant must follow the new shifts there (or in a
  new method the render calls).
- **Preflash:** `preflash_raw` comes from `enlarger::compute_preflash_raw`
  (around line 594). Its inputs are `params.enlarger` (preflash exposure
  and shifts), the neutral CMY, `film.data.base_density` and
  `print_sensitivity`, all of which a built pipeline keeps. Recomputing it
  should be cheap.
- **Who reads these values:** `print_illuminant`, `print_exposure_factor`
  and `preflash_raw` feed `stages/printing.rs` and
  `color_reference.rs`.
  - `color_reference` is built per process ("up front", `pipeline.rs`
    around line 812).
  - Check whether the GPU chain (`spektrafilm-gpu`, `wgpu_backend.rs`)
    caches anything derived from them across renders, keyed on something
    that wouldn't change. If it does, an updated pipeline would present a
    stale print.
- **Precision:** the shifts are `f32` in the params and `f64` in
  `set_print_filter_shifts`. A rebuild goes through `f32` → `f64`. Make the
  updated path take the same route, so the bytes match.

### How to prove it

- **Core test:** for several shift, preflash and compensation values,
  compare a pipeline that was built, then updated, with one built fresh.
  Compare the exposure factor bits, the illuminant and `preflash_raw`, and
  the processed bytes, on the CPU backend, as the existing test does. Add
  a GPU check if the gpu tests can run here.
- **Browser:** render the same params rebuilt (a fresh page with
  `?params=`) and updated (move the sliders there via `negs.edit`). Compare
  with `web/compare.mjs`; they should be byte-identical.
  - Then run `web/checks/controls.js`: the shift, preflash and compensation
    rows should show no `pipeline` stage, at frame-rate times.
  - Put the before/after table and the comparison in `docs/web-wasm.md`
    (a new item 12) and update the rebuild table in item 10.
- **Native:** the CLI renders through `Pipeline::new_with_spectral`, so it
  should be unaffected. Still render it before and after (wgpu and cpu) and
  run the helper's tests, since the key it caches on may change.

### Balance Print

- **Desktop:** macOS puts a "Balance Print" button at the end of the
  Enlarger group (`macos/Sources/NegsApp/Inspector.swift`, `.balancePrint`).
  - Its help text: "Sets the magenta and yellow shifts so a mid-grey prints
    neutral with the current film, bases, paper and enlarger. Use it after
    changing the film base."
  - `EditorModel.balancePrint` (`EditorModel.swift` around line 920) sends
    `balance_print` with the film, paper and params, then sets
    `enlarger.m_filter_shift` and `y_filter_shift` as one edit.
  - The status line then reads "Print balanced: magenta 1.23, yellow 4.56"
    (two decimals). While it runs, the title is "Balancing…".
  - It is disabled while balancing and while scanning film
    (`canBalancePrint`).
- **Helper:** `Command::BalancePrint` in `crates/spektrafilm-native/src/main.rs`
  (around line 199) calls
  `spektrafilm_core::print_balance::solve_neutral_filter_shifts(film, paper,
  &params, data, MIDGRAY)` and answers
  `{m_filter_shift, y_filter_shift, elapsed_ms}`. Export the same from wasm.
- **Missing data:** profiles and LUTs arrive on demand. `render` returns
  `Rendered::missing(paths)`, and `web/engine.js` fetches those files,
  registers them and retries.
  - The solver builds its own pipeline, so it can hit missing files too.
    Give the export the same contract (return what's missing, and let the
    worker fetch and retry) rather than a hard error.
  - `data_fs::take_missing()` / `missing()` in the wasm crate are how
    `render` does it.
- **Web side:**
  - Add the call to `web/engine.js` and `web/worker.js`, a button kind to
    `web/controls.js` (the end of Enlarger), and wiring in `web/app.js`.
  - Disable the button while scanning or balancing. Set both shifts in one
    change, then `sync()` and `schedule("settled")`.
  - The page has no status line, so show the result with `showMessage`.
  - Measure the solver's time in wasm (native timings come back as
    `elapsed_ms`).
  - Check that the result matches the native helper's for the same input
    (send the helper a `balance_print` request, or write a small Rust test).

## Practical notes

- The dev server runs at `node web/serve.mjs` (port 8137). Restart it after
  editing `serve.mjs`. Kill it by port:
  `kill $(ss -ltnpH 'sport = :8137' | grep -o 'pid=[0-9]*' | cut -d= -f2)`.
  This session also left `serve.mjs --https` running on 8443; stop it the
  same way.
- After changing Rust, run `web/build.sh --threads`. It also regenerates
  `web/catalog.json`.
- Driving the page: `node web/cdp.mjs <url> <script> [waitMs]`.
  - A `.js` script runs in the page.
  - A `.mjs` script runs in Node with `{ send, evaluate }`.
  - `SHOT=x.png` saves a screenshot, and `VIEW=390x844@3m` emulates a phone.
  - The page's `negs` hook has `state()`, `edit(path, value)`,
    `renders`, `idle()` and `open(url)`.
  - Checks: `web/checks/controls.js` (every control's frame and stages),
    `inspector.js` (desktop wiring) and `touch.mjs` (the phone layout).
    Rerun the last two after UI changes.
- Keep temporary files in the session's scratchpad, not `/tmp` or the
  repository. For bench and cdp profiles, use
  `BENCH_PROFILE=$(mktemp -d -p <scratch>)` and `CDP_TMP=<scratch>`.
- The shell's working directory resets between commands. Use absolute
  paths, or `cd /home/noah/Work/Negs-wasm &&` first.
- Chrome in these test profiles fails `blob()` on a large response; read an
  `arrayBuffer()` instead. A Chrome left running through many renders can
  lose its GPU state; restart it between benchmark runs.
- The sheet layout's media query is written twice: in `web/index.html` and
  as `SHEET` in `web/app.js`. Keep them the same.
- Don't open an upstream rawler PR without asking.
