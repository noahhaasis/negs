# Adversarial review: B&W stocks, gamut, and macOS package

Date: 2026-10-17. Reference: Python experimental
`28bf883e1672e884307edc75852549376e13644e`.

This is reproducible test evidence, not a formal proof over every input,
parameter combination, GPU, or operating-system version. The review actively
tried to falsify the earlier parity claims and found a numerical edge case.

## Findings and fixes

1. **Near-black scene energy was discarded.** The CPU, WGSL, and CUDA
   Hanatos paths returned zero whenever adapted XYZ summed to at most
   `1e-10`. Python floors only the chromaticity division denominator and
   retains the original brightness. A float32-representable `1e-12` gray
   exposed a `0.0164401574` log-exposure error in Tri-X; the old small
   fixtures missed it. All implementations now preserve that energy and
   floor only the divisor. An analytical constant-LUT regression checks
   the CPU implementations at zero and on both sides of the threshold.
2. **CLI setup errors silently changed the render.** Any spectral-pipeline
   construction error triggered a simplified pipeline with fresh default
   settings. Invalid gamut configuration or a missing LUT could therefore
   produce a successful export that discarded the user's requested model.
   Export now fails with the underlying setup error. A subprocess regression
   verifies nonzero exit and absence of output for invalid configurations.
3. **Fixture provenance checks could be bypassed.** The generator checked
   only one Python module, trusted saved pass flags/arrays, and used Python
   `assert` for validation (disabled by `python -O`). It now compares the
   complete reference tree against Git objects, requires every expected
   stage and case at the original tolerance, uses unconditional exceptions,
   and recomputes the Python final outputs before freezing them. Forged
   flags, omitted stages, relaxed tolerances, modified tree hashes, and
   nonfinite/excessive errors are rejected in tests run with `python -O`.

The refreshed fixture's input-hull, output-compressor, and B&W pipeline
values are exactly equal to the earlier frozen values. Only its provenance
metadata changed. Full pinned reference tree SHA-256:
`d03e1807ecfa9b23e7537c26ff25407aab14369cf7767b076206cccf4afe4a53`.

## Validation

- Default workspace suite: **122 passed**, zero failures, one ignored
  camera-RAW smoke test requiring external images.
- f64 workspace suite: **124 passed**, zero failures, the same ignored test.
- Python comparison/provenance checks: **7 passed**, including under `-O`.
- Earlier colour and B&W reference expectations are preserved.
- New `--adversarial-input` adds 1024 deterministic pixels (NumPy generator
  seed `20261016`): signed/imaginary chromaticities, HDR, near-black, exact
  black, and pure primaries. Settings, source/data/binary hashes, all
  intermediate arrays, and errors are retained by the parity runner.

The extracted release passed **50 CPU/f64 Python comparisons**, covering five
inputs, LUTs on/off, and five routes: Double-X → 2302 at default, short
(4/2 minute), and long (12/9 minute) development; Double-X direct scan;
and Tri-X reversal scan. Across 13,130 pixels the maximum final linear-RGB
error was `1.1088312823481772e-8`; the maximum named intermediate-stage
error was `3.597949760347774e-8`, below `atol=1e-6`, `rtol=0`.

The extracted f32 executable passed **18 additional comparisons**: nine CPU
and nine real Apple M1 Pro Metal runs. Maximum final-RGB errors were
`2.0340388506845386e-7` (CPU) and `9.555254092852294e-6` (Metal), below
the existing f32 tolerance of `3e-3`. GPU checks reject CPU fallback.
Missing-LUT and invalid-center exports failed without creating output.
Two default-effects smoke exports produced finite pixels.

The rebuilt GUI initialized Metal, created preview textures for 4×4 and
32×32 inputs, and stayed alive until deliberately terminated. This checks
the rendering path, not a visual inspection or every GUI interaction. An
earlier sandbox launch could not access the window server, and an earlier
unsandboxed launch exited without a recorded exit code; both final cases
completed with preview-ready logs and a live process.

`Verification/` in the release contains all 50 comparison cases, reference
arrays, settings, backend checks, GUI logs, negative tests, and test-suite
logs. The workspace suites preceded only an informational GUI preview-ready
log addition; the final GUI was rebuilt and smoke-tested afterward.

## Artifact verification

`scripts/verify_macos_package.py` extracts the ZIP into a fresh directory,
verifies the app's strict code signature, checks actual Mach-O deployment
targets against Info.plist, rejects non-system dynamic libraries, hashes
every packaged data file against the source, and checks the accompanying
source snapshot byte-for-byte. It records binary hashes so numerical
results can be tied to the executable being shared.

The parity runner accepts `--rust-binary` and `--rust-data` to test the
extracted release rather than rebuilding a development executable. The
release includes both f32 and f64 CLIs, updated B&W profiles, the optional
gamut preset, the license, and a source snapshot.

Final release: `dist/spektrafilm-0.1.2-macos-arm64-bw-20261017.zip`.
The adjacent `.sha256` and `.verification.json` files record the final ZIP
checksum and extraction audit. Tested executable SHA-256 digests:

| Executable | SHA-256 |
|---|---|
| f64 CLI | `0dc846a4c3481654a049751b5472bb1c45830cdc753765cabfc2344c9f91b011` |
| f32 CLI | `988b2ed5bec88d2e2b2ccdd26b3b4a7ac943c98deeaf9816430b8fb6b54c4a39` |
| GUI | `33875e7775553e6d6a367fa2def6b289c24f54e6a039736016d47bb43d5e7ae2` |

## Scope limits

The explicit experimental gamut preset is tested with deterministic effects
disabled. Experimental colour-profile normalization, stochastic grain/DIR
parity, camera-RAW decoding, and other operating systems remain separate
work. CUDA source was updated consistently but cannot be executed on this
Mac. The GUI still starts with the existing gamut defaults; the experimental
preset is an explicit CLI option.

The macOS app is ad-hoc signed, not Developer ID signed or notarized.
Recipients must follow the included first-launch Gatekeeper instructions.
