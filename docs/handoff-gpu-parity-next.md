# Handoff: GPU parity follow-ups (written 19 October 2026)

You are picking up after a session that made the wgpu GPU engine match the
f64 CPU reference. Read this file fully, then
[gpu-vs-cpu-quality.md](gpu-vs-cpu-quality.md) (the measured evidence), before
changing anything. The user cares about accuracy above everything: users choose
this project for quality. Never trade measured accuracy for speed without
saying so and showing the numbers.

## Where things are

- **Branch:** `gpu-reference-parity`, checked out in the worktree
  `/Users/noahhaasis/Projects/spektrafilm-rs/.claude/worktrees/gpu-parity`.
  Work and commit only there. Do **not** touch the main checkout
  (`/Users/noahhaasis/Projects/spektrafilm-rs`): other Claude sessions work
  and commit on `main` concurrently. Don't touch their worktrees under
  `.claude/worktrees/` or `/private/tmp/...` either. Don't merge, rebase onto
  main, or push; the user decides that.
- **First commit on the branch** holds the parity work: `RefBlur`
  (`crates/spektrafilm-gpu/src/wgpu_backend.rs`, shaders
  `crates/spektrafilm-shaders/wgsl/ref_blur_fir.wgsl` and `ref_blur_iir.wgsl`),
  per-channel halation/scatter, exponential tails for scatter and DIR, exact
  grain samplers (`grain.wgsl`), the GPU grain tests, the measurement harness
  (`scripts/gpu_cpu_quality/`), and the docs.
- **Commits:** one per task, on this branch, with a message body saying what
  was measured. End every commit message with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Don't commit
  unrelated files.

## How the app renders today

- **Preview:** macOS app (`macos/Sources/Spektrafilm/*.swift`, SwiftUI) talks
  to `spektrafilm-native` (Rust, `crates/spektrafilm-native`), which renders
  on the GPU (wgpu/Metal) at the "Working resolution" slider (`io.upscale_factor`).
- **Export:** `ExportRunner.run` in `macos/Sources/Spektrafilm/Engine.swift`
  spawns `spektrafilm-f64 process ... --params params.json` with
  `SPEKTRAFILM_BACKEND=cpu`. `EnginePaths.locate()` finds it: env
  `SPEKTRAFILM_F64_CLI`, then the app bundle's `Contents/MacOS`, then
  `target/release`.
- The app bundle **already contains** the f32 CLI `spektrafilm` next to
  `spektrafilm-f64` (`scripts/package_macos_app.sh`, lines ~20–35).
- **egui GUI** (`crates/spektrafilm-gui`) is kept for Windows/Linux; the user
  wants it to keep working. Its export spawns `spektrafilm-f64` too (search
  `Command::new` near lines ~2255 and ~2490), and it **forces**
  `settings.use_enlarger_lut/use_scanner_lut = true` (~line 944). The macOS
  app does not; it uses `RuntimeParams::default()` with the LUTs off. The
  parity study measured the LUTs-off configuration.
- **CLI backend choice:** `SPEKTRAFILM_BACKEND=cpu|wgpu|cuda`
  (`spektrafilm_gpu::select_backend`). The f32 binary with `wgpu` is the GPU
  engine. If no adapter is found it falls back to the CPU (the log says
  `Backend: CPU (rayon)`).

## The measurement harness (use it after every rendering change)

Everything is in `scripts/gpu_cpu_quality/` (scripts use paths relative to
the checkout they live in).

- Python with the needed packages: `PYTHON=$HOME/Projects/spektrafilm-env/bin/python`
  (numpy, scipy, colour-science 0.4.7, tifffile, Pillow). The system
  `python3` has no numpy.
- Build first, in this worktree:
  ```bash
  cargo build --release -p spektrafilm-cli --bin spektrafilm --bin decode_raw_gui
  cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
  ```
- Test photos (local, private; never commit or upload them):
  `~/Downloads/Z 6/DSC_4990.NEF` (portrait), `DSC_4997.NEF` (blue seats, window
  light), `DSC_5003.NEF` (stripes, skin), `DSC_5009.NEF` (brick bridge, sky,
  water; the hardest for halation), `DSC_4985.NEF` (grey street), and the
  Leica Q3 60 MP file `~/Downloads/7858115920.dng`.
- **Pixel matrix:** `OUT=<scratch dir> PYTHON=... scripts/gpu_cpu_quality/matrix.sh det`
  runs 6 photos × 7 stock pairs (f64 CPU vs GPU). Use `colour` instead of `det`
  for the effects-off mode. Summarize with
  `$PYTHON -I scripts/gpu_cpu_quality/summarize.py $OUT/results-det.txt $OUT/runs summary.json`.
  It takes about 45–60 min and each run writes 0.6–1.4 GB dumps (deleted
  unless `KEEP_DUMPS=1`). Use your session's scratchpad directory for `OUT`.
- **Grain statistics:** `scripts/gpu_cpu_quality/grain_case.sh RAW FILM PAPER|scan [_coarse]`.
  It renders CPU realization A, CPU realization B (input shifted one pixel),
  and the GPU, with grain on and off, then prints per-tone statistics. B vs A
  is the yardstick for "same grain, different random numbers".
- **Pass criteria (current state; keep it):**
  - `det` and `colour` matrices: worst pixel ΔE00 < 0.25, p99 < 0.05, max
    8-bit step 1.
  - grain: every metric within about the CPU A-vs-B spread (σ deviation
    ≲ 0.7%, tone shift ≲ 0.025 L*, spectrum deviation excluding the lowest
    band ≲ 4%).
- **Unit tests:** `cargo test --release -p spektrafilm-gpu -p spektrafilm-math -p spektrafilm-core -p spektrafilm-model`.
  The GPU tests include `grain_matches_poisson_binomial_moments` and
  `grain_dispatch_grid_covers_every_pixel_within_limits`.

## Pitfalls found the hard way

1. **Glare is read from different params on the scan path.** With
   `--scan-film`, glare comes from `film_render.glare`, not
   `print_render.glare`. Deterministic comparisons must switch off both (the
   params files do).
2. **Metal silently drops oversized dispatches.** A heavy shader at
   `@workgroup_size(1024)` produced no output and no error. Grain therefore
   runs at 256. Any new heavy per-pixel pass needs a moment or parity test
   that would fail on "did nothing".
3. **65535 workgroups per dimension.** At 256 threads, 60 MP needs a 2D grid
   (see `grain_dispatch_grid`). Task 1 below is the same problem for every
   other pass.
4. **f32 IIR at large σ.** Young–van Vliet coefficients for σ ≈ 150 px sum to
   within about 1e-6 of 1; rounding them to f32 changed the DC gain by
   percents. Fixed by deriving `a` from the rounded b's. (Later replaced by
   the cascade form, `IirCascade`, which has unit gain by construction and
   far less rounding error; see Results.)
5. **Benchmarks.** Before timing, check `pgrep -fl spektrafilm` (other
   sessions run renders), `pmset -g | grep lowpowermode` and
   `pmset -g batt`. The Mac is often on battery in Low Power Mode, and
   timings are then only relative. Alternate old/new runs, use `--iters 6`,
   take the best warm runs, and report the conditions. Use `caffeinate -i` for
   long runs; the Mac slept overnight once mid-matrix.
6. **Don't `pkill` by broad patterns** like `spektrafilm process`; other
   sessions' renders match too. Stop your own background tasks by ID.
7. **`git diff` uses an external side-by-side tool.** Use
   `git diff --no-ext-diff` for patches.
8. **Precision is not a quality factor.** f32 vs f64 on the same code measured
   max ΔE00 0.11. Differences come from algorithms, so compare algorithms.

## Tasks, in order

### Task 1 — Lift the ~67 MP GPU limit

Every per-pixel pass dispatches `ceil(n / 1024)` workgroups in x only
(`let workgroup_size = 1024u32;` in two places in `wgpu_backend.rs`; about 18
`dispatch_workgroups` calls of the form `(n + workgroup_size - 1) / workgroup_size`;
about 20 shaders with `@workgroup_size(1024)` that index with `gid.x`). Above
65535 × 1024 ≈ 67.1 MP, wgpu rejects the dispatch ("Each current dispatch group
size dimension ... must be less or equal to 65535"). A GFX100 / X2D file
(100 MP) would fail in the preview and in any GPU export.

- Generalize `grain_dispatch_grid` into one helper used by every linear pass.
  In each shader, take `@builtin(num_workgroups)` and compute
  `idx = gid.y * groups.x * WG + gid.x`. Keep the `idx >= n` guards.
- Check `max_storage_buffer_binding_size` / `max_buffer_size` for 100 MP × 3 ×
  f32 = 1.2 GB per buffer on this Mac. If a binding can't be that large,
  report it instead of working around it silently.
- Test: build a synthetic ~100 MP float TIFF (for example upscale
  `decode_raw_gui` output of the Q3 to 12288 × 8192 with tifffile + scipy
  zoom, in your scratchpad) and run `pair.sh` in `colour` and `det` modes.
  It must render without validation errors and pass the criteria. RAM: the
  f64 CPU reference at 100 MP needs roughly 14 GB; check free memory first.
- Extend the grid unit test to the general helper.

### Task 2 — Export engine option (macOS app, then egui)

The user wants to choose between export methods now that the GPU matches.

- macOS: add a persistent setting (UserDefaults) in the export UI, for
  example "Export engine: Reference (CPU, 64-bit)" / "Fast (GPU)". Default:
  **Reference**. The user hasn't decided otherwise, so tell them they can
  flip it.
  - `EnginePaths`: add the f32 CLI (env override `SPEKTRAFILM_F32_CLI`, then
    `executableDir/spektrafilm`, then `target/release/spektrafilm`).
  - `ExportRunner.run`: pick the binary and `SPEKTRAFILM_BACKEND` (`cpu` for
    reference, `wgpu` for fast). Keep cancel, staging and crash cleanup
    (`ExportRecord`) working for both.
  - If the fast export's log shows a CPU fallback, tell the user rather than
    passing it off as a GPU export.
  - Memory: a 60 MP GPU export peaked at about 5 GiB on this 32 GiB Mac.
    Smaller Macs may need the reference path; at least surface failures
    clearly.
  - Swift tests live in `macos/Tests/SpektrafilmTests` (see `EngineTests.swift`);
    add coverage for the path lookup and the argument/env choice. App
    docs: `docs/native-macos-ui.md`; packaging check:
    `scripts/check_native_macos.py` / `scripts/verify_macos_package.py`.
  - Verify end to end: export the same photo both ways with grain off and
    compare with `compare.py` (expect matrix-level agreement); with grain on,
    expect statistically equal grain, not identical pixels.
- egui (`crates/spektrafilm-gui`): add the same option if it fits cleanly.
  Note it forces the enlarger/scanner LUTs on for export. The GPU ignores
  those LUTs, so either measure GPU vs f64 with LUTs on, or ask before
  changing that behaviour.
- Update README "GUI" sections for the option.

### Task 3 — Remaining blurs onto `RefBlur`, and untested effects

The 18/19 October matrices only covered **default** settings. Effects that are
off by default were never compared:
- camera/enlarger diffusion filters (CPU uses an FFT convolution; GPU uses a
  downsampled blur — expect differences)
- camera/scanner lens blur
- highlight boost
- preflash
- print-curve morph
- enlarger diffusion
- non-default halation sizes

1. Add params files that switch each one on (realistic values from the GUI
   defaults or presets in `data/presets/`), run `pair.sh` on a few photos, and
   record the numbers.
2. Move the GPU's remaining 3σ-FIR blurs (`gaussian_blur_gpu`, `make_blur_job`
   users: unsharp mask, lens blurs, glare blur, grain blur, diffusion filter)
   to `RefBlur` or an equivalent where the CPU uses a Gaussian. Unsharp: CPU
   radius `int(3σ + 0.5)`, GPU `ceil(3σ)`. Check the CPU grain blur's exact
   semantics (`spektrafilm_model::grain`: per-layer `blur_particle` and the
   final `grain_blur`) before changing the GPU grain blur.
3. Fix whatever the comparisons show, then re-run the full `det` + `colour`
   matrices and the grain cases.

### Task 4 — Speed, guarded by the harness

Only after 1–3. Measure per-pass GPU time first: wgpu timestamp queries if
the adapter supports them, otherwise ablations. Candidates are in
[vkdt-filmsim-notes.md](vkdt-filmsim-notes.md):
- transpose for the IIR row pass so reads coalesce;
- one dispatch for all FIR channels;
- fusing per-pixel passes.

Every speed change must keep the pass criteria; report both numbers.

### Not in scope

- **CUDA backend** (`crates/spektrafilm-gpu/src/cuda_backend.rs`, `TODO(cuda)`):
  it still has the pre-fix approximations. There's no NVIDIA GPU here; leave
  it, but keep it compiling (`cargo check -p spektrafilm-gpu --features cuda-backend`).
- LUT export: the user decided against it for now.

## When you finish (or stop)

Write a short results section at the end of this file: what was done, the
numbers, what's left, and anything you could not verify. Commit it. The user
reads these handoffs to decide what to merge.

## Results (19 October 2026, second session)

Six commits on `gpu-reference-parity` after `af08025`, each with its
measurements in the message body. Measured on this M1 Pro (32 GiB, Metal).
Numbers are f64 CPU reference vs GPU unless stated; the full tables are in
[gpu-vs-cpu-quality.md](gpu-vs-cpu-quality.md#later-on-19-october-every-blur-and-the-diffusion-filter).

| Commit | What |
|---|---|
| `45deabc` | Task 1: lift the 67 MP dispatch limit |
| `56178b4` | Recursive blur in cascade form (found by Task 1's 100 MP test) |
| `bc4f614` | Task 2: export engine option (macOS app and egui) |
| `bc23c8d` | Task 3: remaining blurs on RefBlur, FFT diffusion filter, effects harness |
| `f90c926` | Task 4: faster RefBlur, per-stage GPU profiler |
| this one | Results |

### Task 1 — 67 MP limit (done)

- Every flat pass goes through `linear_dispatch_grid` / `dispatch_linear`
  (2D grid); the 1024-wide shaders compute
  `idx = gid.y * groups.x * 1024 + gid.x`; `max_reduce` wraps its blocks.
  Highlight boost (one thread per value) already crashed above **21.8 MP**:
  a 24 MP Z6 file with `boost_ev = 1` panicked on the old binary.
- Limits on this Mac: `max_buffer_size` 20.1 GB, binding 4.29 GB; a 100 MP
  image buffer is 1.21 GB.
- Synthetic 100.7 MP TIFF (Q3 upscaled to 12288 × 8192): colour mode worst
  pixel 0.021; det mode worst 0.214 but **p99 0.089, above the 0.05
  criterion**. Cause: the f32 direct-form recursive filter at σ ≈ 190 px.
  Fixed in `56178b4` by running the same filter as a first- plus
  second-order section in difference form (`IirCascade`): 100 MP det worst
  0.008, p99 0.0036; Q3 det worst 0.19 → 0.012. Below 67 MP the dispatch
  change alone was bit-identical to the old binary.
- 100 MP GPU render: 3.5–7 s, peak footprint 9.2 GB. The f64 reference at
  100 MP took 54–63 s and ran without trouble on 32 GiB.
- Tests: grid maths up to 302 M values; wrapped dispatch and max-reduce on
  the GPU with a forced 2D grid (fails with the old indexing); device limits.

### Task 2 — export engine option (done)

- macOS: Engine pop-up in the export panel, "Reference (CPU, 64-bit)"
  (default) or "Fast (GPU)", stored in UserDefaults `exportEngine`.
  `EnginePaths` finds `spektrafilm` via `SPEKTRAFILM_F32_CLI`, the bundle,
  `target/release`; a missing f32 tool only disables Fast. CPU fallback is
  detected from the log (`Backend: wgpu` missing) and shown in the status
  bar; a failed fast export suggests the reference engine. Staging, cancel
  and crash cleanup use the chosen binary.
- **The default is still Reference.** You can flip it in the export panel;
  it is remembered.
- End to end through `ExportJob` with the real binaries (XCTest
  `testRealExportsFromJobList`, 16-bit TIFF): grain off, DSC_5009, worst
  pixel 0.020, p99 0.0058 (16-bit quantization included), 12.5 s vs 1.6 s;
  grain on, DSC_4990: the GPU's grain statistics match CPU A as closely as
  CPU B does.
- egui: an engine box next to the format picker (session setting, like
  the format; egui has no persistence). Reference keeps forcing the LUTs on;
  Fast keeps the preview settings (the GPU computes spectra exactly).
- README and `docs/native-macos-ui.md` updated. Swift and egui unit tests
  added and passing.

### Task 3 — remaining blurs and untested effects (done)

- New harness: `effects.sh` with 15 parameter sets in `params/fx/`
  (diffusion ×3 families, enlarger diffusion, lens blurs small/large,
  scanner lens blur small/large, unsharp at σ 2.5 and 3, highlight boost,
  preflash, print-curve morph, big halation, wide DIR). `pair.sh` takes
  `GPU_BIN`.
- Before: diffusion filters worst pixel 7–16 (up to 62% of pixels > ΔE 1);
  lens blurs and unsharp at σ ≥ 3 worst 16–24; the rest ≤ 0.08.
- Unsharp, lens blurs, glare blur, grain blur and the non-resident blur now
  use RefBlur. The diffusion filter (camera and enlarger) is now an FFT
  convolution with the CPU's exact PSF and reflect padding, in f32.
- After: every effect worst pixel ≤ 0.005, p99 ≤ 0.0037. Default det and
  colour matrices (42 cases each): worst pixel 0.0048, p99 0.0040 (before
  0.19 and 0.13). Grain, 10 cases: within the CPU A-vs-B spread
  (σ dev ≤ 0.46%, tone shift ≤ 0.023 L*, spectrum ≤ 2.9%).
- Cost: with diffusion on, 100 MP peak footprint 13.7 GB (+4.5 GB) and
  8–13 s; 24 MP about 2.5–3 s.

### Task 4 — speed (done, more is possible)

- Per-stage profiler: `SPEKTRAFILM_GPU_PROFILE=1`.
- FIR: one dispatch for all channels (bit-identical). IIR columns: one
  invocation per channel (bit-identical). IIR rows: segmented recursion
  (differs by ≤ 5.7e-6; det matrix unchanged at worst 0.0048, p99 0.0040).
- 24 MP 1100 → 755 ms, 60 MP 3800 → 2020 ms (AC power, LPM off,
  alternating runs).
- Left: the row pass is still the largest blur cost (~29 ms × 8 per frame
  at 24 MP), then grain (~110 ms) and the two spectral passes (~40 ms each).
  Packing two channels per FFT would roughly halve the diffusion filter's
  cost. Fusing the small per-pixel passes would save ~20–30 ms.

### Open questions

1. **egui reference export uses the LUTs; the GPU doesn't.** GPU vs f64 with
   LUTs on: worst pixel 0.13, **p99 0.117** (DSC_4990, Velvia scan; 0.016 on
   DSC_5009 Portra), so the egui Fast export differs from the egui Reference
   export by more than the 0.05 p99 criterion. That is the LUT
   approximation, not the GPU (the GPU matches the exact spectra). Should
   egui's reference export drop the LUTs, as the macOS app does?
2. Make Fast the default? The numbers now support it for deterministic
   effects (≤ 0.005); grain is statistically equal but not pixel-identical.
3. The FFT diffusion filter needs ~4.5 GB extra at 100 MP. Smaller Macs may
   fail there; the error is surfaced (export failed, with the reference
   suggested), but there is no automatic fallback.
4. The macOS panel text says the GPU "matches the reference within
   ΔE 0.25"; measured is now 0.005. Kept conservative; tighten if wanted.

Answers (user, 2026-10-19):

1. egui is unmaintained; leave its export as it is. The focus is the macOS
   app, which will later be ported to other platforms.
2. Not yet: Reference stays the default.
3. No automatic fallback, since Fast is not the default; the error message
   suggesting Reference is enough.
4. Keep the wording for now; the user will fine-tune the UI after approving
   the change. The branch stays unmerged until the user has tested it.

### Not verified / not covered

- No real 100 MP raw (GFX100, X2D): only the upscaled Q3 TIFF.
- The macOS app UI itself was not clicked through: the export panel's
  Engine pop-up compiles and its logic is unit-tested, and `ExportJob` ran
  real exports, but nobody looked at the panel. The Swift integration tests
  that need `target/release/spektrafilm-native` were skipped (not built in
  this worktree); packaging scripts were not run.
- The egui option was unit-tested but not run interactively; Windows and
  Linux untested.
- Settings not in the matrices: output gamut compression modes, mallett and
  arctic upsampling, CAT16, scanner white/black correction, B&W print paths
  beyond Double-X → 2302.
- `apply_diffusion_filter_blur` (the GPU backend's non-resident path, used
  only when the resident chain can't run) still downsamples.
- CUDA backend untouched (compiles with `--features cuda-backend`).
- One machine and one GPU (M1 Pro, Metal).
