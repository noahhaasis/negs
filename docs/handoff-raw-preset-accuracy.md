# Handoff: RAW routing fix → comparison site → close the accuracy gap

## Mandate and sequencing

The user wants the web and macOS outputs to be the same for two supplied RAWs and all nine supplied presets. After the first accuracy run they requested three separate tasks:

1. **Fix the web DNG being decoded as a thumbnail. Completed and verified below.**
2. **A new agent runs fresh comparisons, also against Python's experimental branch, and produces a site showing images side by side with numerical differences.** Include the native macOS version, which has been preserved.
3. **A subsequent agent investigates the differences and closes the gap.** Use the comparison site and reproducible captures as the baseline, rather than mixing fixes into task 2.

**Latest user priority for the next session:** the visibly different **IMG_0316 +
09 COOKED** output is the **first debugging target**. Follow the focused
[IMG_0316 / COOKED debugging handoff](handoff-img0316-cooked-debug.md) before the
generic task-3 triage order below. That handoff was written without starting
an investigation or changing rendering code.

Do not deploy the site, publish the supplied files, change reference goldens, loosen tolerances, or change production RAW/color defaults merely to obtain a passing report. This is a local comparison/review project unless the user separately authorizes publication.

## Environment and inputs

Repository: `/Users/noahhaasis/Projects/Negs-wasm` (Rust/WASM + JS, native macOS Swift app). Starting HEAD was `cd3bc2b` (`Fix browser WASM and GPU resource lifetimes`). The task-1 fix is in the working tree, not a new commit.

Exact inputs:

```text
~/Downloads/tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng
~/Downloads/tag @signatureeditsco - free raws from signatureedits.com -IMG_0316.CR2
~/Downloads/Presets/*.json
```

Nine preset names, in order:

1. `01 KODACHROME.json`
2. `02 P800push2_EXTRAhalation.json`
3. `03 P800 vintage.json`
4. `04 GOLD200_billydeeinsp.json`
5. `05 P400 natural.json`
6. `06 VERITA harman match.json`
7. `07 C200 modern.json`
8. `08 TRIX.json`
9. `09 COOKED.json`

Always quote the RAW/preset paths. Hash inputs and verify nine presets / 18 combinations. The previous run's full paths and SHA-256s are in `target/preset-accuracy/cases.json`. All presets have grain enabled. All output spaces were encoded sRGB. Most select Daylight RAW WB; TRIX selects Custom 5500 K / tint 1. Do not replace them with default As Shot or assume a single WB applies to every case.

Tools already available:

- Rust/Cargo; `target/ipad-tools/bin/wasm-bindgen` is the matching CLI.
- Chrome at `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`.
- `~/Projects/spektrafilm-env/bin/python` (Python 3.13); numpy, scipy, colour, numba, OpenImageIO, rawpy and PySide6 were all found.
- `node web/serve.mjs 8137` serves the editor with cross-origin isolation. A server was already listening on 8137; check before starting a second one.
- `web/cdp.mjs` starts a fresh Chrome profile and runs an async browser script or a Node `.mjs` driver. A fresh profile avoids stale service-worker assets.
- Symlinks in ignored `web/results/`: `accuracy-DSC_2498.dng`, `accuracy-IMG_0316.CR2`, pointing at the original Downloads files.

### Preserve other work

There were pre-existing changes to `scripts/prepare-web-samples.py`, `web/sample/README.md`, `web/sample/catalog.json`, and many untracked docs, `public/`, root package files, etc. Another handoff (`docs/iphone-offline-fix-handoff.md`) also appeared during this task. Do not revert, stage, overwrite or publish unrelated work. `public/` was **not** regenerated. Build outputs in `web/pkg` and `web/pkg-threads` are ignored/local assets.

## Task 1 — completed RAW/thumbnail fix

### Root cause

`crates/spektrafilm-wasm/src/lib.rs::decode_photo_mode` tried the ordinary image decoder first, and only attempted RAW development if raster decoding failed. A DNG is a TIFF container; the supplied Nikon DNG has a valid RGB thumbnail in IFD0. Raster decoding succeeded at **171×256**, reported untagged sRGB / `raw: false`, and bypassed the RAW developer entirely.

Native path-based loading recognizes the `.dng` extension and develops the actual **4912×7360** RAW. A temporary loading placeholder in the editor is fine; it must not become the developed source or exported image.

### Changes

- **New** `crates/spektrafilm-io/src/raw/dng.rs`: `is_dng(&[u8])` checks the TIFF 6.0 byte order/header and IFD0's DNGVersion tag (50706). Checked offsets/slices; no image allocation, signature search through pixel data, or filename requirement. DNGVersion is required in IFD0 even if the sensor image is in a SubIFD.
- `crates/spektrafilm-io/src/raw/mod.rs`: exports this helper.
- `crates/spektrafilm-wasm/src/lib.rs::decode`: refuses to treat an identified DNG as a developed raster, sending it through the existing RAW/WB/orientation/resize path. Shared by `decode`, `Engine.load` and `decodeBounded`. Once identified as DNG, broken RAW data never falls back to the thumbnail.
- **New** `web/checks/raw-thumbnail.test.mjs`: builds a decodable RGB IFD0 around a bundled sample's RAW SubIFD; no private fixture is required for CI. Verifies successful RAW routing/pixels/full size in all three APIs, WB changes and invalid-WB rejection, ordinary TIFF handling, and fail-closed behavior with no RAW SubIFD. An environment variable adds the real camera-file regression.

Scope: fixes DNG container identification, not an exhaustive redesign of detection for every TIFF-based proprietary RAW family. No RAW development math, native Apple default, grain RNG, precision, or color-management defaults were changed.

### Reproduce validation

From the repository root:

```bash
PATH="$PWD/target/ipad-tools/bin:$PATH" bash web/build.sh --threads
cargo test --locked -p spektrafilm-io
RAW_THUMBNAIL_DNG="$HOME/Downloads/tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng" \
  node --test web/checks/raw-thumbnail.test.mjs
NEGS_NATIVE="$PWD/target/preset-accuracy/native-binaries/spektrafilm-native" \
  node --test web/checks/raw-wb.test.mjs web/checks/presets.test.mjs
```

Completed results:

- New regression reproduced the old bug before rebuilding: successful-thumbnail RAW routing and damaged-RAW refusal both failed.
- New regression after fix: **4 passed, 0 failed, 0 skipped**, including the private Nikon file.
- Related preset/WB tests: **14 passed, 0 failed, 0 skipped**.
- Entire native IO suite: **105 passed, 0 failed, 2 ignored** (existing opt-in tests).
- Rebuilt **both** single-thread and threaded WASM assets.
- Real desktop Chrome, actual editor `applyPreset` + `exportBytes`, KODACHROME: `source.raw === true`, source summary `Camera RAW, developed by rawler`, full **4912×7360**, backend `wgpu (GPU)`.

Logs and evidence: `target/raw-thumbnail-fix/`:

- `red-tests.log`, `green-tests.log`, `related-tests.log`, `io-tests.log`, `build.log`
- `browser.mjs`, `browser.log`, `browser.json`
- `DSC_2498-01.fixed-web.ppm` — actual full-size post-fix web export
- `provenance.json` — source/WASM hashes

To repeat that real browser smoke test (dev server must be running):

```bash
node web/cdp.mjs 'http://localhost:8137/?engine=gpu' \
  target/raw-thumbnail-fix/browser.mjs 3000
```

This is **not** a new 18-case post-fix accuracy result and is not deployed/Safari/phone validation.

## Existing accuracy run — pre-fix evidence, not the next baseline

`target/preset-accuracy/` contains the completed previous run:

- `REPORT.md`, `summary.json`: 18 combinations × two native export modes; **0/36 exact-equality passes**.
- `cases.json`, `provenance.json`: input/preset, source/WASM and binary identities.
- `*.web.ppm`, `*.web.json`: actual old browser export and editor state before/after independent preset application.
- `*.native-import.json`, `*.params.json`: independent native import results.
- `*.fast.tiff`, `*.reference.tiff`, `*.log`: native GPU/f64 CPU exports and backend logs.
- `*.comparison.json`, `*.contact.jpg`: numerical results and side-by-side images.
- `browser.mjs`, `native.py`, `compare.py`: local, ad-hoc harnesses; useful as implementation examples only.

Findings from that run:

- All nine DNG cases failed dimensions (171×256 web vs 4912×7360 native).
- CR2 matched dimensions (3648×5472), but not pixels/colors. Mean CIEDE2000 ranged 0.242–3.379 vs native GPU and 0.907–6.312 vs native f64 CPU. Smoothed diagnostics still showed color drift, not only different grain realizations.
- Web and native imported preset settings matched in **18/18**, after f32 normalization. Native JSON widens f32 numbers; web JSON writes shortest decimals.
- Both native backend choices were verified; no silent GPU→CPU fallback was accepted.
- Input developers already differ: web Rawler vs native macOS Apple RAW. Python uses rawpy/LibRaw. Do not attribute all drift to film arithmetic without isolating decoding first.

**Important traps:** the old local harness hard-codes its output directory, skips existing captures/exports, and its report generator hard-codes the old DNG failure explanation. Do not rerun it unchanged, use `FORCE` to overwrite the old evidence, or reuse an old capture under the new WASM hash. Implement a configurable fresh-run harness in tracked scripts for task 2. Preserve the old run as historical evidence.

## Preserved native macOS versions — available for task 2

### Exact executables used in the previous source-build comparison

```text
target/preset-accuracy/native-binaries/spektrafilm
target/preset-accuracy/native-binaries/spektrafilm-f64
target/preset-accuracy/native-binaries/spektrafilm-native
target/preset-accuracy/native-binaries/data/
target/preset-accuracy/native-binaries/manifest.json
```

The three executable hashes were verified against the original run's `provenance.json`. The matching repository data directory has also been copied and hashed. `spektrafilm-native` was a debug build used for preset import; the two exporter CLIs were release builds. These are the executable paths invoked by macOS export code (`macos/Sources/NegsApp/Engine.swift`), not a GUI-click automation claim.

### Actual installed app snapshots

Both full bundles (including their own data/resources) were preserved without modifying the installed originals:

| Identity | Original | Preserved snapshot |
|---|---|---|
| **Negs 0.12.0, build 13** | `/Applications/Negs.app` | `target/preset-accuracy/macos-apps/system/Negs.app` |
| Legacy Negs 0.3.0, build 3 | `~/Applications/Negs.app` | `target/preset-accuracy/macos-apps/user/Negs.app` |

`target/preset-accuracy/macos-apps/manifest.json` records version/build and all four executable hashes per bundle. The shipped app comparison has **not** yet been run. Prefer **0.12.0 build 13** for the installed-native column; the old user app is an optional historical diagnostic, not today's source build.

Executables: `<app>/Contents/MacOS/{spektrafilm,spektrafilm-f64,spektrafilm-native}`. Data: `<app>/Contents/Resources/data`. Explicitly pass each binary's own data directory, preserve its import semantics, inspect its `--help` before using newer CLI flags, and record compatibility errors rather than quietly substituting the current binary/data. Keep **installed app** and **current-source native** identities separate in the site.

macOS Fast export uses f32 CLI + `SPEKTRAFILM_BACKEND=wgpu`; Reference export uses f64 CLI + `SPEKTRAFILM_BACKEND=cpu`. Fast is the current source app's default. Keep both modes in the results because random fields and precision differ.

These artifacts live under gitignored `target/`: they exist on this machine but will not travel in a normal git checkout. Preserve/copy them if handing off elsewhere; never commit executable bundles or supplied RAWs/presets to the repository.

## Task 2 — fresh comparisons and local review site (completed; baseline below)

### 2.1 Freeze identities and build a repeatable harness

- Create a **new** run directory, e.g. `target/preset-accuracy-post-raw-fix-<run-id>/`. Never mutate task-1 evidence or the pre-fix baseline.
- Put reusable runner/comparator/site-generation source in tracked `scripts/` or `web/checks/`, generated/private artifacts in ignored directories. Accept `--output-dir`, `--python-source`, input/preset/native-binary paths and engine selections.
- Record git commit + dirty patch hash, engine asset/binary/source/data/profile hashes, dependency versions, camera metadata, source/output dimensions, WB, color spaces, actual backend/thread count, limits/resizes, and effective imported params.
- Cache only by **all** relevant identities (input + preset/effective params + binary/WASM/data + decoder/WB + working size). Missing/stale output, fallback, nonfinite scan, or incompatible dimensions must be a visible failure, never a pass.
- Before the matrix, assert the Nikon source is RAW and full-size. Do not confuse `negs.photo()` dimensions, retained-source dimensions and a displayed preview's dimensions. Match intentional working sizes explicitly.

### 2.2 Pin Python experimental, not an installed package by accident

Confirmed checkout: `~/Projects/spektrafilm`, branch `experimental`, commit:

```text
28bf883e1672e884307edc75852549376e13644e
```

It has unrelated local modifications in `src/spektrafilm_gui/widget_editors.py` and untracked notes. Do not reset or alter it. Prefer an immutable `git archive` of the commit:

```bash
# Pick a fresh run directory first.
mkdir -p "$RUN/python-experimental"
git -C "$HOME/Projects/spektrafilm" archive \
  28bf883e1672e884307edc75852549376e13644e | tar -x -C "$RUN/python-experimental"
```

Use `~/Projects/spektrafilm-env/bin/python`, prepend that explicit tree's `src`, and assert `spektrafilm.__file__` belongs to it. Keep numba/Qt/cache files in the run directory (`NUMBA_CACHE_DIR`, `MPLCONFIGDIR`, `XDG_CACHE_HOME`, `QT_QPA_PLATFORM=offscreen`).

Read these before implementing the adapter:

- `scripts/parity/README.md`
- `scripts/parity/check_presets.py` — exact GUI preset loading/mapping and Python render semantics
- `scripts/parity/check_parity.py`, `scripts/parity/compare_raw_decoders.py`
- Python `spektrafilm_gui/{persistence,params_mapper,controller}.py`
- Python `spektrafilm/utils/raw_file_processor.py`

Python preset route: `gui_state_from_dict` → `build_params_from_state` → `digest_params(..., apply_stocks_specifics=False)` for the loaded preset's values. Verify actual GUI behavior; do not reapply stock presets after import and erase the user's custom values.

Python end-to-end RAW route: `load_and_process_raw_file(...)`, with the preset's `gui_only.load_raw` WB/temperature/tint/lens-correction settings, using the same interpretation as the controller. This developer defaults to linear ACES2065-1; align its array metadata and runtime input-color fields, not just the array values. **Do not blindly label it ProPhoto/sRGB.** Save effective runtime params, because preset source-color assumptions and app-managed RAW color interpretation differ.

### 2.3 Required comparison tracks

**A. Actual end-to-end application outputs (primary site view)**

All 18 original RAW/preset combinations, unchanged photographic settings:

- Web's real Chrome editor preset import and full-size export (WebGPU; identify actual backend).
- Python experimental's real GUI-equivalent RAW/preset/render path.
- Installed **macOS 0.12.0 build 13**, both Fast GPU and Reference CPU export paths, including its own importer and data.
- Also retain/re-run current-source native GPU/CPU as separately labelled diagnostic engines if possible; the previous run's exact tools/data are preserved above. Do not call them the shipped 0.12.0 app.

Use fresh edits per case; match RAW WB, source interpretation, metering frame, full working size, crop/rotation, output space and profile, post settings and borders. No thumbnail/capped working source may count as a full-resolution result. If Python/Apple/Rawler have differing active areas or orientation, show the size mismatch and preserve the originals. Any aligned crop/resize is a **separate diagnostic**, with the transformation stated.

Capture lossless RGB pixels before JPEG; keep float scans where feasible for investigation. No monitor screenshots as the numeric reference. Use sRGB PNGs/thumbnails for site viewing, with downloadable originals/full-resolution crops. Clearly distinguish source RGB8 export comparison from full-precision pipeline comparison.

**B. Identical-developed-input film-engine isolation (separate diagnostic view)**

The three RAW developers need not agree. Decode once into a lossless float32-representable linear RGB fixture with explicit color space, WB, orientation and size; feed exactly those pixels to each supported engine. Verify fixture transfer hashes/values. This removes decoder differences without concealing them in track A.

Run the original presets unchanged first. Add a separately labelled deterministic variant with only grain and film/print glare disabled identically in all engines, and record every override. Do not silently disable LUTs or switch arithmetic/color defaults; if needed for isolation, create another labelled variant. `check_presets.py` is useful adapter code but its existing deterministic synthetic-fixture test is **not** the requested original-RAW full-preset run.

### 2.4 Metrics and acceptance

The user's expectation is "same output"; report strict equality rather than inventing a new tolerance:

- For matching dimensions: exact RGB pixel fraction, within-one-code fraction, max and mean absolute channel error (state bit depth), full-pixel CIEDE2000 mean/p99/p99.9/max and percent over 1/2.
- For float isolation: max/mean absolute and relative errors with defined zero handling, nonfinite counts and applicable existing gates. Preserve unclipped values; compare display quantization separately.
- Report all relevant engine pairs, not just web vs one favorite reference. Mark errors/missing cases/dimension failures explicitly.
- Keep smoothed color bias and grain statistics separate from unmodified per-pixel results. State blur/window/sampling. Grain RNGs can differ; statistical similarity does not pass exact equality. Consider within-engine repeated renders to distinguish nondeterminism from cross-engine changes.
- Existing goldens/gates remain authoritative for their original scope. Do not rewrite them or silently adopt an approximate-perceptual pass in place of the user's equality request.

### 2.5 Site requirements

Produce a locally served static review site, not just a markdown report:

- All 18 cases, with image/preset and engine filters; clear engine/version/decoder labels.
- Default side-by-side web / Python / native views, plus native CPU/GPU selection and any source-build/installed-native distinction.
- Synchronized zoom/pan, fit view and **1:1 pixel crops**, ideally blink/wipe for selected pairs.
- Absolute-difference heatmap with explicit gain/color legend and per-pair numbers. Unscaled differences must remain accessible. No automatic per-image contrast normalization that hides drift.
- Primary full-preset view versus same-input/deterministic diagnostics are visibly distinct.
- Dimensions, WB, color space, actual backend, hashes/run identity and errors available per case; machine-readable JSON/CSV downloads and links to lossless outputs.
- Lazy-load previews/tiles; do not make the browser decode all full-resolution images at once. Thumbnails are for navigation only and never the metric source.
- Explain how to run it locally, e.g. `python -m http.server PORT --directory <site>`. Avoid using the production `public/` directory. Do not upload original files or presets without permission/license review.

Task-2 completion: reproducible runner + independent Python and native adapters, complete matrix including visible failures, local site with images/differences, JSON/CSV report, exact run commands and provenance. Do not promise the results agree; finding and presenting discrepancies is this task's purpose.

## Task 2 implementation — fresh local run (2026-10-31)

Reusable source is now in **`scripts/accuracy/`**. Start with
[`scripts/accuracy/README.md`](../scripts/accuracy/README.md) for exact commands,
track semantics, dependencies, export quantization, and limits. Key entry points:

- `run.py`: fresh-run preparation, immutable Python `git archive`, native manifest
  verification, configurable engine/track selection, and no export-cache reuse.
- `browser.mjs`, `python_adapter.py`: independent browser/Python adapters;
  native adapters use each preserved version's own importer, exporter and data.
- `audit_params.py`: independent Python GUI preset mapping versus recorded imports.
- `metrics.py`, `compare.py`: full-pixel all-pair metrics, lossless previews/tiles,
  strict-equality JSON/CSV, and a local static side-by-side/difference site.
- `freeze.py`: validates final identities and writes an evidence manifest only
  after all planned case/engine results exist.
- `test_accuracy.py`, `site-check.mjs`: independent numerical and real-Chrome
  viewer checks. No production defaults, tolerances or goldens were changed.

New run (the old run/task-1 evidence remains untouched; the directory suffix is
an opaque ID, not the capture date—`provenance.json.created_utc` is authoritative):

```text
target/preset-accuracy-post-raw-fix-20260724-01/
  config.json, provenance.json, dirty.patch, executions/
  parameter-audit.json
  fixtures/                         # four RAW/WB float32 linear-sRGB fixtures
  renders/{application,same-input,deterministic}/<case>/<engine>/
  attempts/                         # retained diagnostic adapter bring-up errors
  review-full/                      # complete review candidate; check generation_status
```

Local review server: **http://127.0.0.1:8141/**, loopback only. Restart with:

```bash
python3 -m http.server 8141 --bind 127.0.0.1 \
  --directory target/preset-accuracy-post-raw-fix-20260724-01/review-full
```

**Completed and frozen:** `review-full/report.json` has `generation_status:
"complete"`; **`frozen.json` exists**. All **324/324 renders succeeded** (18
cases × 6 engines × 3 tracks), with no execution errors or missing results.
The full report contains **720 numeric pair comparisons and 90 explicit
dimension failures** (810 planned pair slots). **108 exact-equality passes**
are exclusively installed-native versus preserved-source-native in the same
mode; there are no web/Python/native cross-engine exact passes.

| Track | Cases | Successful renders | Compared pairs | Dimension failures | Exact passes |
|---|---:|---:|---:|---:|---:|
| Application RAW/full preset | 18 | 108 | 180 | 90 | 36 |
| Identical developed input, original effects | 18 | 108 | 270 | 0 | 36 |
| Identical input, grain/glare disabled | 18 | 108 | 270 | 0 | 36 |

`review-smoke/` is one application case; `review-diagnostic-smoke/` intentionally
retains an earlier adapter sizing failure. Neither is the baseline. See
`attempts/README.md` before reusing any bring-up output. The full run occupies
about **9.6 GB**, entirely local/ignored.

Confirmed observations and validation:

- All 18 web exports and all 72 native application exports succeeded on their
  requested GPU/CPU backends. The Nikon web source/export is full RAW
  **4912×7360**, not the old 171×256 thumbnail.
- Python LibRaw gives **4924×7378** for the Nikon versus web/native
  **4912×7360**; for Canon it gives **3670×5496** versus **3648×5472**.
  Original dimensions are preserved, not silently aligned. All 18 primary
  Python cases therefore have five incompatible engine pairs each (90 total).
- Python's controller explicitly requests the preset's input color space from
  the RAW loader. These presets request **linear ProPhoto RGB**, so the adapter
  performs the loader's actual ACES→ProPhoto conversion and labels it correctly.
- Independent photographic preset import audit: **54/54** match (18 cases ×
  web / installed-native / source-native, within the existing f32 mapping scope).
- Application-specific runtime defaults are deliberately retained: Python's GUI
  mapper enables scanner/enlarger LUTs and fast stats, while Negs' imported
  settings keep those off. The 54/54 audit covers photographic preset values,
  **not** equivalence of these runtime policies. Ablate this in a separate
  labelled diagnostic during task 3; do not change the primary baseline.
- The Python GUI writer **truncates** RGB8 exports while Rust rounds; float scan
  metrics remain separate. Do not "fix" this by changing a reference export.
- New metric unit tests: **6 passed**; CIEDE2000 independently checked against
  colour-science and a published Sharma pair. An actual 128×128 Nikon crop's
  RGB/ΔE metrics also matched an independent colour-science implementation
  (ΔE agreement <1e-9): `independent-crop-validation.json`.
- Rechecked task-1 regression: **4 passed**, including the supplied DNG;
  related WB/preset tests **14 passed**; IO **105 passed, 2 existing ignored**.
- Real Chrome site smoke verified 3 default/6 selectable engine panels, 1:1
  tiles, difference readback, and **zero full-original image requests** during
  normal viewing. Initial evidence: `site-check.log`, `site-smoke.png`. Final
  full-site validation is outside the frozen run in
  **`target/accuracy-review-validation/`** (`site-check.log`, `full-site.png`,
  `manifest-check.json`). The review server's continuing HTTP logs also live
  there, not in the frozen baseline.

### Results and next-agent priorities

- Application web versus installed Fast GPU: **0/18 exact passes**; mean ΔE00
  ranges **0.145–3.379**. Versus installed Reference CPU: **0/18**, mean ΔE00
  **0.907–9.887**. These are unmodified full-pixel RGB8 measurements.
- Both installed/native-source same-mode pairs are RGB8-identical in **18/18
  cases in each track**. Their identities remain separately labelled.
- Identical-input deterministic web versus installed GPU is extremely close
  but **not equal**: at most one RGB8 code apart; exact pixel fraction
  **99.9893–99.9959%**. Unclipped scan max absolute error ranges
  **1.34e-6–1.16e-5**. This is not an end-to-end RAW equality pass.
- Important isolation finding: deterministic web/GPU versus native CPU still
  has large differences, especially **COOKED (09)** (mean ΔE00 **9.859** Nikon,
  **6.193** Canon) and **C200 modern (07)** (~1.54/~1.41). Grain and glare are
  off and the developed pixels match: do not blame these gaps solely on RAW
  developers or random grain. Locate the first divergent stage next.
- Triage order: preserve this baseline; review RAW active-area/development
  policy; isolate deterministic GPU/f64 routes 09/07 stage by stage; separately
  ablate GUI LUT/fast-stat defaults and RGB8 rounding/truncation. No root cause
  has been "fixed" or production default changed as part of task 2.

Frozen-manifest verification checked **13,383 files**. Every captured output,
metric/report, fixture and immutable backend log matched its recorded hash.
Only root `review-full.log` differs: the finalizer printed its own final
`Frozen:` line after hashing that launcher log. This known bookkeeping issue
is recorded in `target/accuracy-review-validation/manifest-check.json`; the
original finalizer is preserved there as `freeze-used.py`. Future `freeze.py`
now excludes root launcher/server `.log` files, while retaining case export
logs. The frozen evidence/manifest was **not rewritten** to conceal this.

Known scope limits: diagnostic fixtures are explicitly resized to a maximum
768 px, not full-resolution film-isolation runs; native/Python exports use the
GUI-equivalent engine paths, not GUI-click automation; native/web application
captures are RGB8 and do not establish nonfinite/full-precision pipeline parity.
The isolated tracks retain/check float scans. Within-engine repeated grain
renders and phone/Safari validation have not been added. Initial application
web captures recorded cross-origin isolation/hardware concurrency, but not the
actual engine-info thread-count string (the runner now records it for future
captures); do not present that inferred thread count as directly measured.

## Task 3 — investigate and close gaps (subsequent agent)

Start from task 2's immutable baseline/site. Work in a separate branch/worktree if practical; do not modify the frozen Python reference or the review data while investigating.

1. **Triage by source of error.** Compare decoded sources before film processing. Check active area, orientation, demosaic, black/white levels, highlight/headroom handling, WB/adaptation, RAW-to-RGB matrices, lens correction and metering. Apple RAW vs Rawler vs rawpy/LibRaw is a real algorithmic difference, not an f32 tolerance issue.
2. **Verify parameter mapping.** Compare effective Python and Rust params, including stock-specific preset application order, explicit per-channel gammas, spectral method, color interpretation, gamut compression, scan route, print balance, exposure, defaults and LUT choices. The earlier 18/18 web/native import match does not establish Python equivalence.
3. **Find the first divergent stage on identical pixels.** Python `SimulationPipeline.process(..., collect=...)` supports stage collection. Rust probes include `SPEKTRAFILM_DUMP_FILM_LOG_RAW`, `SPEKTRAFILM_DUMP_FILM_DENSITY`, `SPEKTRAFILM_DUMP_PRINT_DENSITY`, and CLI `--raw-out`. See `scripts/parity/check_parity.py` and `docs/wasm-correctness.md` for existing WASM float/GPU harnesses and gates.
4. **Isolate deterministic math before stochastic effects.** Check calibration, spectral LUTs/interpolation, f32/f64 behavior, DIR/halation/blur/diffusion kernels and edge handling, scanner/post/output encoding. Native f64 CPU and matching-input tests help locate whether drift is RAW, shared Rust, WASM, or GPU-specific.
5. **Handle randomness deliberately.** Repeated captures and grain/glare statistics, seed/stream identity and cross-platform sampling must be investigated. A reproducibility change needs explicit policy and regression tests; don't just reseed or relax an exact gate until it appears to pass.
6. **Choose any cross-platform RAW policy explicitly.** If matching Python requires LibRaw or changing macOS away from Apple, document compatibility, color/quality/performance/memory and licensing/distribution implications and get approval before changing platform defaults. Same-input success is not proof of end-to-end equality.
7. **Fix one root cause at a time.** Add a minimal failing regression, preserve old/new captures and stage metrics, run existing Rust/WASM/parity tests, regenerate a **new** site run and show whether improvement generalizes to both RAWs/all presets. Never overwrite the baseline or bless changed goldens to conceal errors.
8. **Finish with a bounded claim.** State which engines/tracks now pass exact or existing numerical gates, what stochastic/perceptual differences remain, and any browser/phone/export limits. If literal equality cannot be achieved without a decoder/RNG policy decision, identify that decision instead of claiming parity.

## Task-2 completion checklist

- [x] Read this handoff and confirm task-1 changes/tests are present; do not redo or revert the fix.
- [x] Verify preserved native tools/app snapshots and Python commit before any rebuild.
- [x] Create a new run directory and configurable tracked harness; keep pre-fix data immutable.
- [x] Run all 18 cases against web, Python experimental and native macOS (identify each build/mode).
- [x] Add same-input/deterministic diagnostics without substituting them for real end-to-end results.
- [x] Generate and locally inspect the side-by-side/difference site; validate metrics independently on a small case.
- [x] Hand over run path, site URL/serve command, reports, provenance, known failures and prioritized hypotheses to the gap-closing agent.
