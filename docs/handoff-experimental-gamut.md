# Handoff: experimental gamut port

**Update (ticket 16.8):** these settings are now the defaults, as in
experimental. The `data/presets/experimental-gamut.json` preset and the
`check_parity.py --experimental-gamut` flag were removed; run the commands
below without them. The text below describes the original opt-in port.

Continues [the B&W stock handoff](handoff-bw-stocks.md). The new smooth
input-gamut boundary and fixed D65 compression center match Python
experimental `28bf883e1672e884307edc75852549376e13644e`. They are available
through explicit parameters; the existing main-compatible defaults and
colour-reference expectations are preserved.

## Changes and use

`InputGamutCompressParams` now accepts:

- `boundary`: `locus` (legacy default) or `inscribed_hull`.
- `hull_detail`: positive finite Gaussian Fourier width, default `5.0`.
- `center`: `reference` (legacy default) or `D65`.

The inscribed hull follows Python's 1024-angle radial sampling, Gaussian
Fourier filtering, and scaling by the worst overshoot. The ray intersection
uses upstream's endpoint epsilon to handle rays through dense hull vertices.
The pipeline passes the complete configuration into both supported spectral
LUT builders. Compression is still baked into the LUT once per pipeline build.
The existing `build(algorithm, knee)` API retains legacy behavior.

Use the bundled preset for the pinned experimental gamut settings:

```bash
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
SPEKTRAFILM_BACKEND=cpu ./target/release/spektrafilm-f64 process input.tif \
  --output doublex.png --film kodak_doublex --paper kodak_2302 --data-dir data \
  --params data/presets/experimental-gamut.json
```

The preset uses input xy compression, the inscribed hull at detail 5, a D65
center, and knee `[0.815, 1, 1.2]`. Output uses the already-ported OkLCh
compressor, knee `[0.95, 1, 1.6]`, and lightness `[0.95, 1, 1.6]`.
These values come from the actual dataclass defaults at the pinned commit;
upstream's surrounding prose mentions older output choices.

Python hardcodes the D65 center in its Hanatos LUT builder rather than
exposing it on `InputGamutCompressSpec`. The parity adapter omits Rust's
`center` field when replacing the frozen Python dataclass. At ordinary input
colors, both the reference-white and D65 compressors can stay in their
identity regions; saturated inputs were necessary to detect this distinction.

## Independent checks (initial port)

`check_parity.py --experimental-gamut` applies the preset to both languages.
It is mutually exclusive with `--gamut-off`. `--gamut-stress` adds a 4×4
fixture containing primaries, secondaries, near-primary colors, imaginary
RGB values with negative channels, and above-white gray. Each report records
these options and the effective parameter files.

All **24 CPU/f64 comparisons** pass: four inputs × two LUT modes × three
routes. Every exposure/density/RGB component is finite and within `atol=1e-6`,
`rtol=0`.

| Route | Maximum final linear RGB error |
|---|---:|
| Double-X → 2302 | `2.184e-11` |
| Tri-X 7266 direct reversal scan | `5.313e-10` |
| Double-X direct negative scan | `4.505e-12` |

The worst intermediate error is `2.168e-9` in Tri-X film density. Ordinary
gradient and gray inputs remain within the earlier B&W stock tolerances.
Passing artifacts are under `/tmp`:

- `spektrafilm-gamut-doublex-stress-v2/report.json`
- `spektrafilm-gamut-trix-stress-v2/report.json`
- `spektrafilm-gamut-doublex-scan-stress-v2/report.json`

Earlier stress runs without the `-v2` suffix exposed the missing D65 center;
they are diagnostic failures, not final results.

`tests/fixtures/gamut_python_28bf883.json` retains independent Python hull
samples, complete synthetic LUT remaps at three white/detail configurations,
output-compressor samples, and six B&W gradient/stress final-RGB buffers.
It records the source commit/tree hashes, gamut module hash, profile hashes,
dependencies, input pixels, and settings. Tests check hull enclosure and
vertex intersections, legacy defaults, invalid parameters, compressor
references, and complete B&W pipelines with LUTs off/on. Float tolerances
remain `1e-6` in f64 and the project's existing `3e-3` in f32.

Final workspace checks passed **120 tests** in the default build and **122
tests** with f64 enabled, with zero failures. Each leaves one camera-RAW
smoke test ignored unless `SPEKTRAFILM_TEST_RAWS` is supplied. All four
Python comparator tests passed. Existing colour-reference values were
unchanged and passed in both builds. Logs:
`/tmp/spektrafilm-gamut-workspace-f32-final.log` and
`/tmp/spektrafilm-gamut-workspace-f64-final.log`.

## Reproduce

Create an archive of the pinned Python source, without switching the sibling
working tree. Use fresh output directories for live runs:

```bash
mkdir -p /tmp/spektrafilm-gamut-python-28bf883
git -C ../spektrafilm archive 28bf883e1672e884307edc75852549376e13644e \
  | tar -x -C /tmp/spektrafilm-gamut-python-28bf883

../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source /tmp/spektrafilm-gamut-python-28bf883 \
  --film kodak_doublex --paper kodak_2302 --experimental-gamut --gamut-stress \
  --output-dir /tmp/spektrafilm-gamut-doublex-stress-v2
```

Repeat with `--film kodak_trix --scan-film` and
`--film kodak_doublex --scan-film`, saving to the other directories above.
Then freeze **Python outputs only**, using the sibling Git object database
to verify the pinned gamut implementation:

```bash
../spektrafilm-env/bin/python scripts/parity/freeze_gamut_reference.py \
  --python-source /tmp/spektrafilm-gamut-python-28bf883 \
  --bw-runs /tmp/spektrafilm-gamut-doublex-stress-v2 \
    /tmp/spektrafilm-gamut-trix-stress-v2 \
    /tmp/spektrafilm-gamut-doublex-scan-stress-v2 \
  --output crates/spektrafilm-core/tests/fixtures/gamut_python_28bf883.json

cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64
PYTHONDONTWRITEBYTECODE=1 ../spektrafilm-env/bin/python \
  -m unittest discover -s scripts/parity -p 'test_*.py'
```

## Boundaries and next work

This closes the tested B&W gamut difference through an explicit preset.
Experimental colour normalization, the other 28 profile updates, neutral
print-filter calibration, grain, and DIR changes remain separate work. The
optional gamut preset alone does not establish colour-stock parity.

The follow-up [adversarial review](review-bw-gamut.md) found and fixed three
issues, expanded validation to 50 f64 and 18 f32 CPU/Metal release comparisons,
and rebuilt and packaged the app on 2026-10-17. The GUI completed Metal
preview smoke checks; it has no new preset button, so CLI JSON remains the
way to opt in. See that review for the final ZIP, hashes, evidence, and limits.
No commits were made; the prior handoff's working-tree cautions still apply.
