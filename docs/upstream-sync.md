# Python parity audit — 2026-10-16

> **Decision, 2026-10-25: Negs targets upstream `experimental`, not main.** Noah decided that
> Negs should behave like Python spektrafilm's experimental branch (pinned `28bf883`). The port
> is planned in [the experimental port plan](experimental-port-plan.md) (ticket 16).
>
> **Status, 2026-10-26: ported on branch `experimental-port`, not yet on main.** There the f64
> engine matches 28bf883 at every stage within 4.4e-8, over the whole image, on Portra 400 →
> Endura, Gold 200 → Crystal Archive II, Provia 100F scan, Double-X → 2302 and Vision3 500T →
> 2383, LUTs off and on, at experimental's defaults (grain and glare off) and per effect.
> Exceptions: the diffusion filter stays f64, and grain is gated statistically. Results:
> `scripts/parity/README.md`, "Result after integration". Until the branch is merged, main
> still matches upstream main `3bb2c2d` as audited below; after it, this audit is
> historical.

This records the audit **before** the subsequent B&W stock port. The
[B&W handoff](handoff-bw-stocks.md) documents the updated three profiles,
loading fixes, and additional passing experimental comparisons with gamut
compression explicitly disabled. The original main/experimental results
below remain the historical baseline for the unchanged colour stocks.

The deterministic CPU/f64 print chain still matches upstream **main** on
the tested fixtures. The local sibling Python checkout is on
**experimental**, and its output differs materially. These are distinct
porting targets, even though both identify themselves as version 0.3.4.

## Revisions and method

| Implementation | Revision | Status |
|---|---|---|
| Rust | `0bcac46d05332dc4c21df5128b6871465d3354a9` | Existing local GUI/packaging changes; numerical pipeline unchanged |
| Python main | `3bb2c2d2801ff68b92019cf1dbcbb133d60832bc` | Fresh clean checkout; remote tip verified with `git ls-remote` |
| Python experimental | `28bf883e1672e884307edc75852549376e13644e` | Sibling checkout; remote tip verified; local GUI-only tracked modification |
| Python dev | `6cd00c8d4f30b5b550f50f4bbd3753c9f2a48507` | Remote tip verified; not numerically tested |

Use commit IDs, not just `0.3.4`, to identify a reference. The sibling
checkout's `main` branch already points at the verified main revision;
there is no need to update or switch the user's experimental working tree.

The original port compared stage arrays and decoded pixels, with tiny
floating-point tolerances; its own measurements disprove literal byte
identity. See [the harness documentation](../scripts/parity/README.md) for
the original probes, their limitations, and commands for the new runner.

The audit uses three deterministic fixtures, both LUT modes, linear sRGB
input/output, disabled stochastic/spatial effects, and the actual CPU/f64
CLI. Shared inputs are rounded to float32 before either implementation
sees them. Acceptance is absolute error ≤ `1e-6` at every stage component,
with zero relative tolerance. Each implementation keeps its own profiles
and remaining defaults; this measures shipped behavior, not algorithm-only
parity with artificially synchronized data.

## Measured results

| Python reference | Film → paper | Passing cases | Max final linear RGB error |
|---|---|---:|---:|
| main `3bb2c2d` | Portra 400 → Portra Endura | 6/6 | `3.420e-9` |
| main `3bb2c2d` | Gold 200 → Fuji Crystal Archive II | 6/6 | `3.631e-9` |
| experimental `28bf883` | Portra 400 → Portra Endura | 0/6 | `5.580e-2` |

Against main, the film/print intermediate differences are at most
`2.109e-15` across both stock pairs. All tested final pixels are identical
after shared 8-bit **linear RGB** quantization. Float buffers are not
byte-identical. This is not a reproduction of the old large RAW-image
benchmark or its encoded-PNG statistics.

Against experimental, the first failing stage is `log_e_film` in all six
cases. Worst absolute errors for Portra/Endura:

| Stage | Maximum error |
|---|---:|
| Film log exposure | `3.736e-2` |
| Film CMY density | `2.517e-2` |
| Print CMY density | `3.632e-2` |
| Final linear RGB | `5.580e-2` |

Shared 8-bit linear quantization differs by up to 15 levels. This is too
large to classify as floating-point noise. It is also not evidence that
recent Rust changes regressed: the older main reference still passes.

An additional historical probe at Python `eac6b8574771f7d0625efdde8b0c907813824536`
(May 28 CAT16/gamut changes, package version 0.3.2) matches through print
density but differs in final RGB by up to `4.057e-3` (2/6 cases pass).
That probe illustrates the changed output-stage baseline; it is not the
original unrecorded Python checkout used for the initial port's figures.

Rust validation: `cargo test --workspace --locked` passed **114 tests**;
the same command with `--features spektrafilm-cli/precision-f64` passed
**116 tests**. Both leave one camera-RAW smoke test ignored because it
requires local files via `SPEKTRAFILM_TEST_RAWS`. Three comparator unit tests
also pass, including deliberately corrupted, non-finite, and mismatched
outputs. The live experimental comparisons return failure rather than
silently accepting drift. Upstream's pytest suite was not run: this
environment has no pytest installed.

Environment: macOS ARM64, Python 3.13.5, numpy 2.4.6, scipy 1.18.0,
colour-science 0.4.7, numba 0.65.1, OpenImageIO 3.1.14.1. These are the
installed audit dependencies, not a reconstruction of the original
author's environment. Full run artifacts are under `/tmp`:

- `spektrafilm-parity-current-main/report.json`
- `spektrafilm-parity-main-gold/report.json`
- `spektrafilm-parity-local-python/report.json`
- `spektrafilm-parity-historical/report.json`

These temporary artifacts include effective Python parameters, Rust
parameter overrides, dependency versions, hashes, and all stage outputs.
This document retains the results after temporary files are removed.

## Porting from experimental

The git history and source explain several independent changes worth
porting separately. This is a prioritized starting point, not a claim
that every upstream difference has been classified.

1. **Spectral normalization and profile calibration.** Commit `e301791`
   replaces green-only normalization with channel-wise white balance and
   references the Hanatos adaptation window to the reconstructed neutral
   spectrum. Later commits change profile neutralization and the neutral
   print-filter database. These are relevant to the observed first-stage
   drift; a controlled ablation/bisect is still needed to attribute its
   exact size. Port and test calibration changes before new effects.
2. **Gamut defaults and output behavior.** Experimental changes the default
   output compressor and input/output knees (`85717fb`, `8e54f3b`,
   `4260c7b`). Explicitly freeze these settings in algorithm-level tests
   and test the new defaults separately.
   Follow-up: the [experimental gamut port](handoff-experimental-gamut.md)
   now provides an explicit preset, smooth input boundary, and fixed D65
   center. B&W routes pass with compression enabled, including saturated
   inputs. Since ticket 16.8 these settings are the defaults, and the
   preset is gone.
3. **Additional spectral recovery.** Reflectance LUT methods, including
   Arctic 2026, Otsu 2018, and Jakob 2019, add data/descriptor handling and
   normalization rules. Although experimental's GUI history changes its
   preferred method, `init_params()` in the tested checkout still uses
   `hanatos2025`; the failure above is not simply a different method name.
4. **Chemistry, grain, and DIR couplers.** Experimental adds chemistry
   parameters, RMS-granularity/preset grain controls, multiplicative
   unsharp masking, and Langmuir coupler saturation. Rust already has
   partial B&W/development-time and print-morph support, so map existing
   implementations before adding fields. Grain needs distribution-level
   tests in addition to exact RNG primitive tests.
5. **Workflows and LUT creation.** Experimental adds generalized workflow
   routing, scanned-film conversion, film/print-base controls, and LUT
   baking with parameters. Test each route at named stage boundaries;
   the current live runner only covers the three-channel print route.

Keep `3bb2c2d` as the known-good compatibility reference, and use a separate
pinned `28bf883` target for experimental work. Preserve the existing Rust
reference values while introducing intentional behavior changes behind
explicit configuration or a documented baseline migration. For each
feature, add a deterministic input, explicit settings, the upstream
commit/data identity, and assertions at the earliest affected stage.
Do not raise tolerances or regenerate old expected values merely to make
a changed implementation pass.

The Python reference's own current snapshot suite uses `.npz` arrays with
`numpy.testing.assert_allclose(rtol=1.5e-3, atol=1e-6)` in
`tests/regression_baselines.py`; it also is not byte-for-byte comparison.
Those self-regression snapshots do not establish cross-language parity.

## Reproduce the passing baseline without switching checkouts

```bash
# From the Rust repo root; use new temporary directory names if they exist.
mkdir -p /tmp/spektrafilm-python-main-ref
git -C ../spektrafilm archive 3bb2c2d2801ff68b92019cf1dbcbb133d60832bc \
  | tar -x -C /tmp/spektrafilm-python-main-ref
../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source /tmp/spektrafilm-python-main-ref \
  --output-dir /tmp/spektrafilm-main-check
```

An archive has no Git metadata, so its report records a null commit and a
source-tree hash. Retain the archive commit command alongside the report,
or use an isolated checkout for automatic commit recording.
