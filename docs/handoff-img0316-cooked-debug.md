# Next-session handoff: debug IMG_0316 + COOKED first

## Resolution (2026-10-31)

**Pair and symptom.** The user confirmed that native Reference CPU and web CPU are
the accurate targets; **Fast GPU, native and web, was far off.** In the
frozen deterministic track, GPU vs CPU was 6.19 ΔE00 on IMG_0316-09
(DSC_2498-09: 9.86), with GPU brighter in every channel. Python vs CPU was 0.32 ΔE00.
Presets 02–07 also showed an ≈1 ΔE GPU gap. Presets 01 (preflash off) and 08
(matching GPU/CPU) did not.

**Cause.** The enlarger preflash in the GPU-resident chain. The CPU computes
`(raw × factor + preflash) × print_exposure × bw` (`stages/printing.rs`
`expose_calibrated`). Without enlarger diffusion, the GPU fast path folds
`print_exposure × bw` (`FilmChainParams::print_exposure_scale`) into the
`print_spectral` normalization, but it added the preflash **unscaled**. The
scale is ≠ 1 whenever `print_exposure ≠ 1` or scanner white correction is on
(COOKED: `white_correction = true`). Ablation: setting only
`enlarger.preflash_exposure = 0` collapsed the COOKED GPU/CPU max
scan error from 0.247 to 3e-6. Disabling print base/morph, film base/morph,
couplers or halation each left the gap in place.

**Fix.** `wgpu_backend.rs` (`start_film_chain`) and `cuda_backend.rs` now
scale the preflash by `print_exposure_scale` whenever that scale is folded
into the normalization. The enlarger-diffusion branch is unchanged: there the
scale is applied after `print_spectral`. CPU output is unchanged.

**Regression.** `crates/spektrafilm-core/tests/preflash_gpu.rs` covers wgpu vs
CPU with preflash on, in three variants: white correction, print_exposure 0.8,
and white correction plus enlarger diffusion. It asserts that the exposure
scale really differs from 1. Without the fix: max |Δ| 4e-2, failing. With the
fix: ≤ 3e-6, passing.

**Evidence** (`target/img0316-cooked-gpu-debug-20261031/`, frozen run untouched):

| Check | Before | After |
|---|---|---|
| Deterministic GPU vs CPU, all 18 cases, max scan \|Δ\| | up to 0.247 | ≤ 1e-5 (`matrix/deterministic`) |
| Application IMG_0316-09, native GPU vs CPU, ΔE00 mean | 6.49 | 0.49 (grain noise) |
| Application DSC_2498-09, native GPU vs CPU, ΔE00 mean | 9.90 | 0.81 (grain noise) |
| Deterministic web (Chrome WebGPU) vs native CPU, COOKED, max scan \|Δ\| | 0.247 / 0.242 | 1.8e-6 / 1.5e-6 (`web-run/`) |
| Application web vs native CPU, DSC_2498-09 | 9.89 | 0.81 |
| Application web vs native CPU, IMG_0316-09 | 6.31 | 1.41 |

**What remains.** IMG_0316 web vs native application is still about 1.4 ΔE00
for COOKED and 2.7 for KODACHROME (01, no preflash, unchanged by this fix).
This is the Canon RAW decode difference between web (Rawler) and native (Apple
RAW), not the GPU path. The `matrix/` and `web-run/` outputs are focused
checks. A full regenerated review site has not been produced.

Scripts: `target/img0316-cooked-gpu-debug-20261031/scripts/{run.sh,ablate.py,matrix.py,application.py}`.
The web pkg was rebuilt with `web/build.sh --threads`, which needs `~/.cargo/bin` on `PATH`
for wasm-bindgen 0.2.121.

---

## User instruction and session boundary

The user reviewed the comparison site and said:

> The difference for IMG_0316 and the COOKED preset is very visible. That's the first debugging target.

**Start task 3 with case `IMG_0316-09`**, not a broad catalog sweep, C200, or a
redesign of RAW development. The goal remains matching the web and macOS
outputs, with Python experimental as an independent comparison.

This handoff-writing session did **not** investigate the cause, run new
renders, or change rendering code. Findings below are from the already
completed comparison session, not a new diagnosis. The next agent should
investigate, establish a failing regression, and fix the actual cause.

Read alongside:

1. [Full accuracy handoff](handoff-raw-preset-accuracy.md), especially the
   completed task-2 baseline, preservation rules, and task-3 probes.
2. [Harness README](../scripts/accuracy/README.md).
3. `scripts/parity/README.md`, `scripts/parity/check_parity.py`,
   `scripts/parity/check_presets.py`, and `docs/wasm-correctness.md` before
   implementing stage comparisons.

## Exact first target

Repository: `/Users/noahhaasis/Projects/Negs-wasm`

```text
Case ID: IMG_0316-09
RAW:     ~/Downloads/tag @signatureeditsco - free raws from signatureedits.com -IMG_0316.CR2
Preset:  ~/Downloads/Presets/09 COOKED.json
```

Always quote these paths. Verify their hashes against the case entry in the
frozen run's `config.json`; do not substitute a sample photo or reconstructed
preset. This preset has grain enabled in the application/original-effects
tracks. Use its recorded WB, exposure, film/print route, color interpretation,
post settings, and all other photographic values—not defaults or a similarly
named stock preset.

**The user did not specify which engine pair or native mode they were viewing.**
Identify and record that pair before describing the visible discrepancy.
Do not assume CPU/f64, GPU, or Python is automatically the correct reference.

## Frozen baseline and review site

```text
BASE=/Users/noahhaasis/Projects/Negs-wasm/target/preset-accuracy-post-raw-fix-20260724-01
```

The directory suffix is an opaque run ID, not the capture date. Provenance
records the actual capture date. The run is complete and has `frozen.json`.

Review: **http://127.0.0.1:8141/**. If it is no longer running:

```bash
cd /Users/noahhaasis/Projects/Negs-wasm
# Check for an existing listener first; do not launch a duplicate.
lsof -nP -iTCP:8141 -sTCP:LISTEN
python3 -m http.server 8141 --bind 127.0.0.1 \
  --directory target/preset-accuracy-post-raw-fix-20260724-01/review-full
```

In the site:

1. Select **IMG_0316** and **09 COOKED**.
2. Start with **A · Application RAW → full preset**.
3. Compare Web / Python / Installed Negs; select native **Fast GPU**, then
   **Reference CPU**, or **Both**. The preserved source build is a separate
   optional column, not the shipped app.
4. Inspect fit, synchronized 1:1 crops, blink, and fixed-gain differences.
5. Then select **B · Identical input · grain & glare off ONLY** and compare
   Web vs Installed GPU, Web vs Installed CPU, and Python vs those modes.

The diagnostic view is **not** the full-resolution RAW export. Its common
linear-sRGB float32 fixture was explicitly resized to a maximum 768 px.

### Evidence locations for this case

Relative to `$BASE`:

```text
review-full/report.json                    # every pair, all three tracks
review-full/metrics.csv
review-full/provenance.json
config.json, provenance.json, frozen.json
parameter-audit.json
renders/application/IMG_0316-09/<engine>/
renders/same-input/IMG_0316-09/<engine>/
renders/deterministic/IMG_0316-09/<engine>/
```

Engine keys:

```text
web
python
installed-gpu
installed-cpu
source-gpu
source-cpu
```

Each engine directory has `result.json` and a lossless `output.ppm` (web) or
`output.png` (others). Native directories also retain `import.json`,
`params.json`, `export.log`, and `export.log.command.json` with the actual CLI
arguments/environment overrides. Diagnostic directories retain `scan.f32`
(web) or `scan.f64` (native/Python).

Read parameters **and** source/color metadata. Submitted/imported params are
not by themselves proof of effective runtime input interpretation after RAW
loading. Keep that distinction when comparing native JSON to web/Python state.

Find this case's `fixture_id` in `config.json`, then use:

```text
fixtures/<fixture_id>.json                  # dimensions, WB, transform, hashes
fixtures/<fixture_id>.f32                   # interleaved little-endian float32 RGB
fixtures/<fixture_id>.tiff                  # exact same values; verified TIFF transfer
```

The original fixture and its hashes are the common-input reference. Do not
redecode it differently for each engine or silently change its color label.

## Established observations—not a root-cause diagnosis

- All **324 baseline renders succeeded**. The site/report includes all 18
  RAW/preset cases, six engines, and three tracks. Missing outputs/fallbacks
  were not accepted as successes.
- For this Canon RAW, web/native application outputs are **3648×5472**;
  Python LibRaw outputs are **3670×5496**. End-to-end Python comparisons are
  dimension failures, not aligned comparisons. Do not silently crop/resize
  the primary outputs to hide this.
- **On identical pixels, with grain and both film/print glare disabled,
  `IMG_0316-09` Web vs Installed Reference CPU still has mean ΔE00
  6.193083985846258.** The earlier viewer check also recorded maximum RGB8
  channel error **63** and unclipped scan max absolute error
  **0.24704019590546827**. These numbers are for this diagnostic pair/track,
  **not** a claim about the pair the user viewed or a root cause.
- Across all 18 deterministic cases, Web vs Installed Fast GPU was much
  closer: at most one RGB8 code apart, with float max errors in the range
  1.34e-6–1.16e-5. Read this case's exact row rather than substituting an
  aggregate. Near equality is not strict equality.
- Installed-native versus preserved-source-native outputs are RGB8-identical
  within each native mode for all cases/tracks. Both identities are retained.
- The **54/54 photographic preset-mapping audit passed**, but it does not
  establish equivalence of runtime policies or arithmetic.
- Runtime defaults intentionally differ: Python's GUI mapper enables
  enlarger/scanner LUTs and fast stats; Negs retains those settings off.
  The baseline did not override them to manufacture agreement.
- Python's GUI RGB8 writer truncates; Rust rounds. This is a known export
  quantization distinction, not an established explanation for the large
  visible COOKED discrepancy. Compare unclipped scans separately.
- Diagnostic scans are **unclipped encoded-sRGB outputs**, not automatically
  linear RGB. A native GPU `scan.f64` is a widened dump; its file dtype does
  not turn GPU calculations into f64 arithmetic.

The same-input deterministic discrepancy means RAW-developer differences
and random grain **cannot alone explain that particular diagnostic failure**.
It does not prove the full-resolution application discrepancy has only one
cause. No first divergent stage or root cause has been established yet.

## Proposed work order for the next agent

1. **Verify and reproduce the chosen case/pair.** Inspect the frozen lossless
   images and numbers, verify identities, and reproduce into a **new** ignored
   output directory. Establish which visible difference is being addressed.
   Keep full-preset application output as the user-facing acceptance target.
2. **Use the existing deterministic fixture to localize the film-engine gap
   before expanding scope.** Verify identical transfer values, dimensions,
   input/output encoding, metering frame, and effective params. Compare
   GPU/f32, native f64/CPU, and Python separately; distinguish platform from
   backend/precision differences.
3. **Find the first divergent stage.** Use the existing Python stage collection
   and Rust probes below, adding narrow instrumentation if required. Confirm
   instrumentation actually observes the requested backend: do not accidentally
   force a GPU render through a CPU/debug fallback and call it equivalent.
4. **Ablate one candidate at a time, in labelled diagnostics only.** Runtime
   LUT/fast-stat policies, color/spectral interpretation, f32/f64 arithmetic,
   spatial stages, scan/post/encoding, and imported values are things to test,
   not conclusions. Preserve the original full-preset baseline and list every
   diagnostic override. Do not start by disabling lots of effects until the
   images happen to look similar.
5. **Add a minimal failing regression, then fix one demonstrated cause.**
   Prefer a small reproducible fixture derived from the identified issue;
   do not commit private supplied pixels/presets as a test fixture. Preserve
   before/after stage metrics and lossless captures under new run identities.
6. **Verify the actual IMG_0316 + COOKED application export improves**, with
   original photographic settings restored, not just its resized/grain-off
   diagnostic. Then cross-check Nikon + COOKED and the complete two-RAW,
   nine-preset matrix for regressions before claiming general parity.
7. Regenerate a **new** local review run/site, report exact equality plus
   existing numerical gates in their proper scope, and state what remains.
   Do not move to C200 or broader cleanup before this first target is
   localized/addressed, unless evidence from this case requires it.

Stage probe entry points already identified in the previous handoff:

```text
Python: SimulationPipeline.process(..., collect=...)
Rust:   SPEKTRAFILM_DUMP_FILM_LOG_RAW
        SPEKTRAFILM_DUMP_FILM_DENSITY
        SPEKTRAFILM_DUMP_PRINT_DENSITY
CLI:    --raw-out
Web:    Engine.renderRaw (final float scan; not a substitute for stage taps)
```

See `scripts/parity/check_parity.py` for stage names, layouts, B&W expansion,
and dump handling. Verify which taps each backend actually supports before
using absence of a dump as numerical evidence.

## Tools, identities, and safe reproduction

- Python: `~/Projects/spektrafilm-env/bin/python`.
- Frozen Python source: `$BASE/python-experimental`, archived from experimental
  **`28bf883e1672e884307edc75852549376e13644e`**. Assert the imported module belongs
  to that explicit tree. Keep caches in a new debug directory, never the source.
- Python GUI preset route: `gui_state_from_dict` → `build_params_from_state` →
  `digest_params(..., apply_stocks_specifics=False)` to preserve loaded values.
  The GUI RAW loader requests the preset's input space; these presets use
  genuinely converted linear ProPhoto RGB, not relabelled ACES values.
- Installed **Negs 0.12.0 build 13** snapshot:
  `target/preset-accuracy/macos-apps/system/Negs.app`.
  Exporters/importer: `Contents/MacOS/{spektrafilm,spektrafilm-f64,spektrafilm-native}`;
  own data: `Contents/Resources/data`.
- Preserved source-build tools/data: `target/preset-accuracy/native-binaries/`.
  Keep them distinct from a newly rebuilt working-tree binary.
- Native Fast: `SPEKTRAFILM_BACKEND=wgpu`; Reference:
  `SPEKTRAFILM_BACKEND=cpu`. Verify the **actual** backend in each log and pass
  each binary its own data directory. Inspect its help before newer flags.
- Real editor dev server: port **8137** (check before starting). Chrome CDP:
  `web/cdp.mjs`; fresh profiles prevent stale service-worker/WASM assets.
- Reusable runner/adapters: `scripts/accuracy/{run.py,browser.mjs,python_adapter.py}`.
  **`compare.py --case IMG_0316-09` filters an existing review, not rendering.**
  Do not invent a supported `run.py --case` flag. If a focused render selector
  is needed, add one explicitly or write a narrow probe; retain original
  full-matrix validation and never edit the frozen config to select one case.

Example of a focused **read-only re-review**, not fresh render reproduction:

```bash
BASE="$PWD/target/preset-accuracy-post-raw-fix-20260724-01"
DEBUG="$PWD/target/img0316-cooked-debug-$(date +%Y%m%dT%H%M%S)"
mkdir -p "$DEBUG"
"$HOME/Projects/spektrafilm-env/bin/python" -I scripts/accuracy/compare.py \
  --run "$BASE" --output-dir "$DEBUG/frozen-case-review" --case IMG_0316-09
```

A new worktree/branch is useful if practical, but task-1 fixes and the accuracy
harness are uncommitted: a clean worktree at HEAD alone will not contain them.
Preserve/carry over the intended changes deliberately without resetting,
stashing, staging, or overwriting unrelated work.

## Preservation and known traps

- **Do not overwrite** `$BASE`, its Python reference, supplied files, prior
  pre-fix run, task-1 evidence, preserved app bundles, or preserved tools/data.
  Build current-source candidates elsewhere and label their identities.
- Do not change goldens, relax tolerances, silently crop/resize, reseed RNGs,
  switch LUT/color defaults, or choose a different RAW decoder just to pass.
  A platform RAW/RNG policy change requires explicit approval and tradeoffs.
- The DNG-thumbnail routing fix is already completed and tested. Do not undo
  or redo it while addressing this Canon case.
- `attempts/` and `review-diagnostic-smoke/` retain **invalid early adapter
  attempts**, not valid comparison evidence. One used an invalid working-size
  zero; another rendered 1×1 because render `max_dim=0` is not uncapped.
  Corrected diagnostic web captures assert the fixture's actual dimensions.
- The frozen manifest's only verified drift was root `review-full.log`: the
  finalizer appended its own `Frozen:` stdout line after hashing the log.
  All capture/metric/fixture/immutable backend-log hashes matched. See
  `target/accuracy-review-validation/manifest-check.json`; do not rewrite the
  manifest or discard the baseline because of that known launcher-log issue.
- `target/` artifacts are private/ignored and do not travel with a Git checkout.
  Preserve them when moving machines. No deployment or publishing of inputs,
  presets, or review outputs has been authorized. Leave `public/` alone.
- Respect existing unrelated working-tree changes; see the full handoff.

Validation available from the completed session: metric unit tests **6 passed**,
RAW regression **4 passed**, WB/preset tests **14 passed**, IO **105 passed / 2
existing ignored**, independent real-crop metric validation, and Chrome viewer
checks. Re-run the relevant tests after a fix; use existing parity/WASM gates
without rewriting their reference values.

## Expected hand-back

- The exact engine pair and symptom addressed for **IMG_0316-09**.
- Reproduction commands and identities, first divergent stage, and demonstrated
  cause (or a bounded blocker if still unresolved).
- Minimal regression and focused fix, with stage and full-application
  before/after evidence—not just a visually improved thumbnail.
- New review/report paths, all-18-case regression results, remaining differences,
  and any required decoder/RNG policy decision. Do not claim strict parity if
  only a deterministic or approximate comparison passes.
