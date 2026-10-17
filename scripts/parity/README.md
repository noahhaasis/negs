# Python ↔ Rust parity checks

Use `check_parity.py` for a live comparison against an **explicit Python
checkout**. The original scripts below were manual diagnostic probes, not
an automated byte-for-byte regression gate.

## Run

Use Python 3.13 with the chosen reference's runtime dependencies installed
(including numpy, scipy, colour-science, numba, and OpenImageIO), plus Cargo.
The existing sibling `spektrafilm-env` environment works on the audit machine.
The runner imports directly from `--python-source`; it does not install or
modify that checkout. Python caches go into the output directory unless
their environment variables are already set.

From the Rust repository root:

```bash
# Choose a NEW output directory for every run.
../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source ../spektrafilm \
  --output-dir /tmp/spektrafilm-parity-run

# Another film/paper pair:
../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source /path/to/python-reference \
  --film kodak_gold_200 --paper fujifilm_crystal_archive_typeii \
  --output-dir /tmp/spektrafilm-parity-gold

# Existing frozen Python reference tests and the full Rust suite:
cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64

# Validate the comparator itself:
PYTHONDONTWRITEBYTECODE=1 ../spektrafilm-env/bin/python \
  -m unittest discover -s scripts/parity -p 'test_*.py'
```

The runner builds the current `spektrafilm-f64` CLI with the locked Cargo
dependencies and forces `SPEKTRAFILM_BACKEND=cpu`. It does not trust a
potentially stale release executable. It requires the Python topology API
(`SimulationPipeline.process(..., collect=...)`), present in the audited
0.3.4 revisions; older Python APIs may require an adapter.

## What is compared

- Identical float32-representable linear sRGB input, promoted to f64 in both
  pipelines. Python consumes the array; Rust loads its lossless float TIFF.
  RAW decoder, orientation, and white-balance differences are excluded.
- Three deterministic fixtures: 1×1 midgray, 4×4 colour gradient, 16×16 gray
  ramp. Portra 400 → Portra Endura is the default pair.
- Auto-exposure, grain, halation, DIR couplers, print glare, lens blur, and
  unsharp masking are disabled. Output is linear sRGB. Other reference
  defaults and each implementation's own profiles remain in effect, so
  changes to those are deliberately visible.
- Film log exposure, film CMY density, print CMY density, final RGB.
- Both enlarger/scanner LUTs off and on, at resolution 17 (`--lut` selects
  a single mode if desired).

Every float must be finite and within absolute tolerance `1e-6`, with no
relative tolerance. This matches the existing Rust f64 reference tests.
`--atol 0` requires numerical equality; the separately reported
`byte_identical` field compares canonical little-endian f64 buffers (even
signed zeros can differ bytewise).

Exit status is **0 for pass, 1 for numerical drift, 2 for execution errors**.
Shape mismatches, NaNs, missing dumps, or Rust's simplified-pipeline fallback
are errors. The comparison never accepts a small mean error in place of
checking every component.

`report.json` records commits, dirty status, source/data/binary hashes,
dependency versions, maximum/mean differences, exact equality, and the
first stage outside tolerance. Per-case files retain Python arrays, Rust
f64 dumps, the input TIFF, parameter JSON, and Rust logs. The 8-bit metric
quantizes **linear RGB** identically on both sides; it is not a comparison
of encoded PNG file bytes or the historical sRGB-encoded PNG experiment.

`--scan-film` skips printing and compares a direct film scan. B&W exposure
and density taps are single-channel in Python and three identical lanes
in Rust: the runner explicitly repeats the Python channel and checks all
three Rust lanes. Final RGB is always compared as three channels. Film
glare is disabled as well as print glare.

`--film-development-time` and `--print-development-time` select B&W family
members, mapping experimental's `*.chemistry.development_time` to Rust's
`*.development_time`. `--gamut-off` explicitly disables both input/output
gamut compressors in both implementations; this isolates stock parity
from the branches' different gamut algorithms/defaults. The report records
these options, so such a pass is not confused with default-settings parity.

`--experimental-gamut` applies the pinned experimental `28bf883` gamut
settings to both implementations, including Rust's explicit D65 center.
It is mutually exclusive with `--gamut-off`. Add `--gamut-stress` to compare
saturated, imaginary, and above-white inputs as well as the ordinary three
fixtures. See the [gamut handoff](../../docs/handoff-experimental-gamut.md)
for the preset, passing B&W routes, and Python fixture regeneration.

`--adversarial-input` adds 1024 seeded HDR/imaginary/near-black pixels.
`--rust-binary /path/to/spektrafilm-f64 --rust-data /path/to/data` checks an
existing release and its own data instead of building a development binary.
See the [adversarial review](../../docs/review-bw-gamut.md) for the near-black
bug this exposed and the hardened fixture-provenance checks.

```bash
../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source ../spektrafilm --film kodak_doublex --paper kodak_2302 \
  --film-development-time 4 --print-development-time 2 --gamut-off \
  --output-dir /tmp/spektrafilm-bw-print-check

../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source ../spektrafilm --film kodak_trix --scan-film --gamut-off \
  --output-dir /tmp/spektrafilm-bw-slide-check
```

These small tests do not establish parity for every profile, GPU execution,
RAW decoding, spatial effects, or stochastic grain. See the
[B&W stock port handoff](../../docs/handoff-bw-stocks.md) for the tested B&W
routes and frozen Python fixtures.

## Original methodology and surviving probes

The initial release (`b97a682`) records Python v0.3.2 measurements: decode
RAW once to a shared float TIFF, render both implementations, dump
intermediate f64 arrays, and compare stage errors and final 8-bit pixels.
It reports float differences around `1e-15`, 99.9962% identical pixels,
and a maximum one-level pixel difference. Those are numerical parity
measurements, **not strict byte identity**. Grain textures were explicitly
excluded from pixelwise parity because rejection sampling amplifies small
upstream numerical differences.

The later `81518fb` commit adds frozen 0.3.4 reference assertions, including
full-chain 1×1/4×4 cases and a stage-by-stage gray-card case. These run under
`cargo test`: f64 tolerance `1e-6`, f32 tolerance `3e-3`. Some older
`debug_compare.rs` functions only print reference values and have no
numerical assertions. The release workflow builds packages but does not
run the test suite or live Python comparisons.

The following files remain as historical probes, superseded by the runner:

- `spektra_compare.py`: generates Python output and prints a Rust command;
  it does not run Rust, compare results, or enforce a threshold. The printed
  command omits matching colour-space/effect settings and explicit f64/CPU
  selection.
- `py_bisect.py`: hard-coded `/tmp/cmp`, profile pair, and private stage API.
- `rs_bisect.rs`: not a Cargo target, has obsolete signatures and absolute
  author paths, and downcasts stage values to f32 before writing f64.
- `py_stages.py`: prints hand-computed diagnostics; references an external
  Rust probe that was never checked in and an import removed on experimental.

The former README's May 23 print-stage drift was an intermediate finding,
superseded by the initial release's later measurements. The exact original
RAW fixture, environment lock, and comparison command were not retained,
so those historical figures cannot be reproduced verbatim from this repo.

See [the October 2026 audit and porting baseline](../../docs/upstream-sync.md)
for fresh results and the distinction between upstream `main` and
`experimental`.
