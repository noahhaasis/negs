# Handoff: B&W stock port

Follow-up: [experimental gamut port](handoff-experimental-gamut.md) adds an
explicit preset and passing B&W comparisons with compression enabled,
including saturated inputs. The original stock-port results below remain
the record of the gamut-disabled calibration work.

## User request and outcome

The user asked whether the preceding parity audit ported anything (it did
not), then requested new Python film/paper stocks, especially B&W, plus a
handoff for the next agent.

Completed: imported the latest experimental calibrations for all three
B&W stocks, fixed two loading issues required for Python parity, extended
the comparison runner to B&W/film scans/development-time selection, and
added frozen Python regression fixtures to Cargo tests.

Both catalogs already contained the same **31 stock IDs**. There were no
missing colour stocks or separate B&W photographic-paper profiles to add.
These three B&W profiles were already bundled as older 0.3.3 data; they
are now exact copies of the upstream 0.3.4 data. Do not describe this as
three newly introduced stock names or call Kodak 2302 photographic paper.

| Stock ID | Upstream name / role | Development times (minutes) |
|---|---|---|
| `kodak_doublex` | Kodak Double-X Negative Film 5222, B&W camera negative | 4, 5, **6.5 default**, 9, 12 |
| `kodak_trix` | Kodak Tri-X Reversal Film 7266, B&W positive/reversal | Single profile, stored family value 1 |
| `kodak_2302` | Kodak Print Film 2302, B&W print film | 2, 3.5, **5 default**, 7, 9 |

Tri-X here is **7266 reversal**, not Tri-X 400 negative. Its single stored
family value is not a new user-selectable development series.

## Exact source and file provenance

Python source: sibling `../spektrafilm`, branch `experimental`, commit
`28bf883e1672e884307edc75852549376e13644e`. Verified remote refs during this
task: main `3bb2c2d2801ff68b92019cf1dbcbb133d60832bc`, dev
`6cd00c8d4f30b5b550f50f4bbd3753c9f2a48507`, experimental as above.

The imported bytes came from `git show <commit>:src/spektrafilm/data/profiles/<stock>.json`,
not an unpinned working-tree copy. All metadata, credit, and CC BY-SA 4.0
license text were retained unchanged. SHA-256:

| File under `data/profiles/` | SHA-256 |
|---|---|
| `kodak_doublex.json` | `4c36da545d10463b6249074bf644501082a11853aa08e8fbc1a8282b3d6709e5` |
| `kodak_trix.json` | `223b6c940b7536558ff952ade5069c58d4ef1fd7a98949d3160e4e92d3b7869c` |
| `kodak_2302.json` | `145d7ec07f070c2985d8c5f5845206903e00a6c2a5fed39d402368acbc8252b4` |

The other 28 stock profiles and the colour neutral-filter database were
not replaced: their experimental updates belong with the broader colour
normalization/gamut port described in [upstream-sync.md](upstream-sync.md).

## Implementation

- `crates/spektrafilm-core/src/profile.rs`: deserialize missing log
  sensitivities as NaN. Spectral consumers already implement Python's
  `nan_to_num(10 ** log_sensitivity)`. The old null→0 conversion incorrectly
  made unmeasured wavelengths fully sensitive (`10^0 = 1`). The B&W
  profiles have 30–56 missing wavelengths; this caused large exposure and
  tone errors. No bundled colour profile contains null log sensitivities.
- The same file now evaluates B&W `cdfs`/`norm_cdfs` fitted models over the
  full development-time family on load, including sublayer curves and the
  sign reversal for positive film. Python uses the model as authoritative;
  the stored sampled curves alone differed by roughly `1e-5`. Validate
  model shapes, finite parameters, and positive sigmas before evaluation.
  B&W profiles without a fitted model retain the legacy sampled fallback.
  Existing colour-profile behavior is preserved.
- `print_morph.rs`: exposes the existing normal CDF within the crate, so
  profile loading uses the same numerical implementation.
- `resolve_for_render` still selects the nearest development time,
  broadcasts single-channel sensitivity/density into three equal engine
  lanes, and encodes dye spectra as `[dye, 0, 0]`. The existing broadcast
  test now compares float bits because preserved NaNs are unequal to
  themselves under ordinary `==`.
- `scripts/parity/check_parity.py`: adds `--scan-film`, development-time
  options, `--gamut-off`, and explicit expansion of single-channel Python
  stage outputs. Both film and print glare are disabled. Frozen Python
  chemistry dataclasses are updated using `dataclasses.replace`.
- `crates/spektrafilm-core/tests/bw_profiles.rs` and
  `tests/fixtures/bw_python_28bf883.json`: independent Python final-RGB
  references for five configurations, exercised with Rust LUTs off/on.
  The 4×4 colour-gradient input is rounded to float32 before either
  implementation. The fixture includes its source commit, recipe,
  dependencies, settings, and profile hashes. Do not regenerate from Rust.

## Validation and boundaries

Thirty live CPU/f64 comparisons passed (three inputs × two LUT modes ×
five configurations), with **both gamut compressors off** and stochastic/
spatial effects disabled:

| Configuration | Max final RGB error vs Python |
|---|---:|
| Double-X → 2302, default development times | `2.903e-12` |
| Tri-X direct film scan | `3.856e-13` |
| Double-X direct negative scan | `1.240e-13` |
| Double-X 4 min → 2302 2 min | `3.610e-13` |
| Double-X 12 min → 2302 9 min | `2.894e-12` |

Each run checks intermediate exposure/density and final RGB at `atol=1e-6`,
`rtol=0`; it reports errors rather than accepting NaNs or wrong shapes.
Rust frozen references use `1e-6` in f64 and the existing project's `3e-3`
allowance in f32. The comparator's four unit tests include corrupt data,
NaNs, shape mismatches, and a bad replicated B&W lane.

**This is stock-model parity, not full experimental-default parity.**
Experimental has different input gamut compression and output gamut
defaults. With each branch's own gamut settings, B&W RGB still differs.
Grain/DIR model updates, GPU parity, and camera-RAW decoding are outside
this comparison. The GUI/packaged app was not manually exercised or
rebuilt; the source-level changes are shared by preview and export.

Run the complete checks from the Rust repo root:

```bash
cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64
PYTHONDONTWRITEBYTECODE=1 ../spektrafilm-env/bin/python \
  -m unittest discover -s scripts/parity -p 'test_*.py'
```

The camera-RAW smoke test is ignored unless `SPEKTRAFILM_TEST_RAWS` is set.
Final full-suite results: **116 passed** in the default build, **118 passed**
with f64 enabled, zero failures, and one ignored RAW smoke test in each.
All four Python comparator unit tests passed. Existing colour-reference
tests remained unchanged and passed in both Rust configurations.
Final verification logs are `/tmp/spektrafilm-bw-workspace-f32-final.log`
and `/tmp/spektrafilm-bw-workspace-f64-final.log`. Live passing run directories
(each includes `report.json`, params, TIFF input, Python arrays, Rust dumps):

- `/tmp/spektrafilm-bw-doublex-stock-parity`
- `/tmp/spektrafilm-bw-trix-verified`
- `/tmp/spektrafilm-bw-doublex-scan-verified`
- `/tmp/spektrafilm-bw-short-verified`
- `/tmp/spektrafilm-bw-long-verified`

Earlier `/tmp/spektrafilm-bw-*` runs include diagnostic failures before
the fixes; they are not the final results. Temp files are disposable;
the frozen fixture and this note retain the useful evidence.

## Usage and next-agent commands

The GUI discovers profiles from the data directory automatically. Double-X
and Tri-X appear under Film stock; 2302 appears under the paper/print
selector because its stage is `printing`, despite its support being film.
Choosing Tri-X enables direct film scan. For Double-X → 2302, explicitly
select 2302 and leave “Scan film (skip printing)” off; upstream Double-X
has no `target_print`. The existing development-time selectors become
available for each multi-time family.

CLI examples (normal effect/gamut defaults):

```bash
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
SPEKTRAFILM_BACKEND=cpu ./target/release/spektrafilm-f64 process input.tif \
  --output doublex.png --film kodak_doublex --paper kodak_2302 --data-dir data
SPEKTRAFILM_BACKEND=cpu ./target/release/spektrafilm-f64 process input.tif \
  --output trix.png --film kodak_trix --scan-film --data-dir data
```

Development-time JSON overrides passed with `--params`:

```json
{
  "film_render": { "development_time": 4.0 },
  "print_render": { "development_time": 2.0 }
}
```

For a fresh live parity run (choose a new output directory):

```bash
../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source ../spektrafilm --film kodak_doublex --paper kodak_2302 \
  --gamut-off --output-dir /tmp/bw-parity-next
```

Use `--scan-film --film kodak_trix` for reversal, or add
`--film-development-time 4 --print-development-time 2` for short development.
To reproduce frozen references, take `gradient4-lut-off/python-rgb_out.npy`
from each of the five configurations; the fixture must remain Python-derived.

For follow-on work, port experimental gamut behavior and colour-profile
normalization separately, with explicit settings and pinned references.
Do not weaken the old colour-reference tests to accommodate that migration.
If shipping the app, rebuild both GUI and f64 CLI and package the new data;
old `dist/` bundles still contain old binaries/data.

## Working-tree cautions

Nothing was committed. At the start of this task the user already had
edits in `crates/spektrafilm-gui/src/main.rs` and
`scripts/package_macos_app.sh`, plus untracked `TODO.txt`, `problem.txt`,
and `dist/`. These were not changed by this stock port. Earlier-turn audit
changes in README, `scripts/parity/`, and `docs/upstream-sync.md` were also
already present and were extended here. The sibling Python checkout has
its own GUI changes; do not reset or switch that working tree.
