# Local RAW / preset accuracy review

Task 2 of [`docs/handoff-raw-preset-accuracy.md`](../../docs/handoff-raw-preset-accuracy.md).
This harness does **not** change RAW/color defaults, goldens, tolerances, RNGs,
or production assets. Supplied photographs/presets and generated outputs stay
under ignored `target/` and `web/results/`. Do not deploy the review site.

## Prerequisites

- Build the fixed single-thread and threaded WASM assets as in the handoff.
- A local `node web/serve.mjs 8137` server, current Chrome, Node with WebSocket.
  For private inputs, prefer `NEGS_DEV_HOST=127.0.0.1` when starting a new server.
- Explicit Python 3.13 environment with the experimental reference's dependencies
  (including exiv2, lensfunpy, PySide6/qtpy), plus Pillow and numba for review.
- The preserved native tools/app snapshots described in the handoff. Their
  manifests are checked **before** running exports. No rebuild/substitution.
- About 20 GB free for a full review, potentially more with additional runs.

## Run a new matrix

From the repository root, using the reference environment (not system Python):

```bash
PY="$HOME/Projects/spektrafilm-env/bin/python"
RUN="$PWD/target/preset-accuracy-post-raw-fix-$(date +%Y%m%dT%H%M%S)"

# New directory only. This archives the PINNED experimental commit, never the
# dirty checkout. It verifies 2 RAWs × 9 presets, hashes, native manifests,
# saves each CLI's help, hashes all data/assets and records the dirty patch.
"$PY" -I scripts/accuracy/run.py prepare \
  --output-dir "$RUN" \
  --python-source "$HOME/Projects/spektrafilm" \
  --preset-dir "$HOME/Downloads/Presets"

# End-to-end first. Python creates each unique RAW/WB fixture as a SECOND copy;
# its primary render is full resolution, never that diagnostic copy.
"$PY" -I scripts/accuracy/run.py render --output-dir "$RUN" --tracks application

# Alternatively pre-create all four independent RAW/WB diagnostic fixtures:
"$PY" -I scripts/accuracy/run.py fixtures --output-dir "$RUN"
"$PY" -I scripts/accuracy/run.py render --output-dir "$RUN" \
  --tracks same-input deterministic

# New review directory only; all relevant engine pairs and every RGB pixel.
"$PY" -I scripts/accuracy/audit_params.py --run "$RUN"
"$PY" -I scripts/accuracy/compare.py --run "$RUN" --output-dir "$RUN/review-01"
"$PY" -I scripts/accuracy/freeze.py --run "$RUN" --review "$RUN/review-01"
python3 -m http.server 8140 --bind 127.0.0.1 --directory "$RUN/review-01"
# Open http://127.0.0.1:8140/
```

`run.py all` is prepare + all renders (Python must precede diagnostic tracks).
Use `--engines web python installed-gpu installed-cpu source-gpu source-cpu`
to select engines; defaults include all six. `--tracks` explicitly selects
tracks. Overrides for private locations:

```text
--input 'DSC_2498=/absolute/path/to/file.dng'  (repeat twice, labels unique)
--preset-dir /absolute/path/to/nine-presets
--installed-app /path/to/preserved/Negs.app
--native-dir /path/to/preserved/native-binaries
--python-source /path/to/git-checkout --python-commit 28bf883...
--server http://localhost:8137 --diagnostic-side 768 --timeout 1200
```

`--python-source` is a Git checkout used ONLY for `git archive` at the pinned
commit, not imported directly. The extracted tree is imported and asserted.
All Python/Qt/numba caches stay in the run directory. A new process per Python
case bounds memory and prevents edit/RNG carry-over. Python renders can take
minutes at full resolution; run sequentially on memory-constrained machines.

**No export cache exists.** Rendering an already-existing engine/case directory
fails, rather than trusting a stale PNG or rerendering over evidence. A failed
case's result is retained and shown. Make a new run to correct an adapter or
change an engine. The report generator can produce a new, explicitly incomplete
review while a matrix is running; missing engines are failures, not passes.
`compare.py --wait-seconds 2400` can wait for in-flight results up to a global
deadline; it does not treat still-missing results as successful.
`compare.py --tracks application --case DSC_2498-01` makes an explicitly partial
smoke review. Do not call it the complete matrix. `report.json` includes every
requested engine pair, including missing and incompatible outputs.

## Tracks and exact meanings

1. **application**: actual Chrome editor `applyPreset` + full `exportBytes`,
   Python GUI persistence → params mapper → RAW loader → pipeline → GUI writer,
   each native version's own `preset_import` + app-used exporter CLI/data.
   Native modes are forced and log-verified: GPU f32 and CPU f64. No GUI click
   automation is claimed for native/Python. Full RAW dimensions are asserted
   before the Nikon web capture. No automatic alignment is applied.
2. **same-input**: Python's RAW loader produces an explicitly colour-converted
   linear-sRGB fixture, resized **separately** with scipy.ndimage.zoom order=1
   to a maximum 768 px (configurable), then rounded once to float32. RAW WB,
   orientation, transformation and source dimensions are recorded. Identical
   values go to Python arrays, web `loadDecoded`, and native manual float TIFF.
   TIFF read-back and browser transfer SHA-256 are checked. This is diagnostic,
   not a substitute for the full-resolution RAW comparison.
3. **deterministic**: the same fixture and original preset settings, with only
   film grain, film glare and print glare disabled. No LUT/color-math override.
   Web uses the same GPU film engine's `renderRaw`, in a separately labelled
   single-thread WASM instance, not the editor's RAW path.

Python mirrors `controller.load_raw_image` by **requesting** the preset's input
colour space/curve from `load_and_process_raw_file`. For the supplied presets
this is genuinely converted linear ProPhoto RGB, NOT mislabelled ACES data.
Loaded photographic settings are digested with `apply_stocks_specifics=False`
to preserve user values. LUT/fast-stats choices are **not overridden**: Python's
GUI mapper enables its LUTs/fast stats, whereas the native/web imports retain
Negs' exact-mode defaults (LUTs/fast stats off). This runtime-policy difference
is recorded, not hidden by the photographic mapping audit. Python's GUI
PNG writer truncates to uint8, unlike Rust's rounding; this is retained and
can cause a one-code export discrepancy even with near-identical float scans.

## Metrics and site

- RGB8 metrics: exact **pixel** fraction (all RGB channels equal), fraction with
  all channels within one code, max/mean absolute channel error, signed channel
  bias, full-pixel CIEDE2000 mean/p99/p99.9/max and percent above 1/2.
- Lab uses the colour-science sRGB matrix, sRGB CCTF and D65. CIEDE2000 is a
  parallel numba implementation tested against colour-science on 4096 random
  pairs and a published Sharma pair. No sampling or Gaussian smoothing in
  reported per-pixel metrics. Signed global bias is separate from absolute error.
- Diagnostic scans preserve unclipped encoded-sRGB f32 (web) / f64
  (native/Python). Reports include absolute and symmetric relative errors,
  `abs(a-b)/max(abs(a),abs(b),1e-12)`, and explicit nonfinite failures.
  The informational 1e-6 float check does not redefine existing test gates.
- Primary native/web application captures are RGB8: they do **not** establish
  full-precision/nonfinite pipeline parity. Python additionally scans its float
  output before saving. The float diagnostic track is separately identified.
- Exact RGB8 equality is the only equality pass. Differing grain realizations,
  statistical similarity, and within-one-code results do not pass it. Repeated
  within-engine renders/grain statistics are not yet part of this runner.
- Site has case/image filters, native mode/source-build selection, synchronized
  zoom/pan, true 1:1 source tiles, blink, fixed-gain grayscale absolute error,
  and unscaled RGB absolute-difference crop download. No full images load for
  ordinary browsing: PNG previews and visible 512 px lossless tiles only.
- Fit heatmaps are **max-pooled** channel errors, not auto-normalized; the
  block size and gain are shown. At ≥30% zoom differences use original RGB8
  tiles. One source pixel = one CSS pixel at 1:1 (Retina physical pixels differ).
- JSON/CSV, effective params, backend logs, provenance, lossless originals and
  float diagnostics are available locally. RAWs/presets are not site downloads.
  Original export symlinks require preserving the run on this machine; copy
  their targets if moving a review. Do not publish them without permission.

## Validation

```bash
"$PY" -I -m unittest discover -s scripts/accuracy -p 'test_*.py'
node --check scripts/accuracy/browser.mjs
node --check scripts/accuracy/site.js
RAW_THUMBNAIL_DNG="$HOME/Downloads/tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng" \
  node --test web/checks/raw-thumbnail.test.mjs
```

Keep task-1/pre-fix evidence immutable. Never replace installed-app results
with current-source binaries. Freeze a finished run's manifest and retain it
as the baseline before investigating production fixes (task 3).
