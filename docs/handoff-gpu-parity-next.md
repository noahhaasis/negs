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
   percents. Fixed by deriving `a` from the rounded b's. Keep that if you
   touch the IIR.
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
