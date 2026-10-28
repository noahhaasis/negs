# Standalone Goose grain experiment

Start with [the agent handoff](../../docs/handoff-goose-grain.md) and
[the agreed plan](../../docs/goose-cpu-rewrite-plan.md).

Frozen preparation inputs (the implementation and commands are described below):

- `capture-params.json`: reproducible recipe overrides for capturing pre-grain density.
- `source-baseline.json`: hashes of the source/reference files inspected during preparation.

Run `python3 -I scripts/goose/preflight.py` from the repository root to inventory
the current tools, original-source hashes and available private photo candidates.
The local report goes under ignored `target/goose/`.

## Capture recipe

Use `kodak_gold_200`, with `kodak_portra_endura` during fixture capture, at 35 mm
film format. The capture settings turn on all grain subeffects while retaining
stock-derived RMS granularity, layer scales, density floors and uniformity.

Microstructure is `[20.0, 1000.0]`: blur width in micrometers and clump size in
nanometers. At a 6048-pixel long side, grain sees 35000/6048 micrometers per pixel,
so the activation quantity is approximately 0.173 (>0.05), and its blur is about
3.46 pixels (>0.4). At 9520 pixels both branches remain active. Grain pixel blur
0.89 exercises FIR; multiplicative sharpening sigma 3.5 exercises IIR. Dye-cloud
blur also remains enabled. Verify the actual resolved dimensions, particle scales
and branch decisions rather than treating these estimates as captured evidence.

These overrides do not replace the resolved settings in the fixture. Capture
exactly what the existing Rust pipeline passes to `apply_grain`. Do not set a
working-size reduction or preview cap for the primary full-resolution fixtures.

## Fixture v1 contract

The implemented capture and native loaders use this validated contract. Private
fixture directories live under `target/goose/fixtures/`:

```text
metadata.json
density.f64le
curves-total.f64le
curves-layers.f64le
```

`metadata.json` must contain:

| Field | Meaning |
| --- | --- |
| `format`, `version` | `negs-grain-fixture`, integer `1` |
| `width`, `height`, `channels`, `layout`, `dtype` | Positive dimensions, `3`, `HWC`, `f64le` |
| `pixel_size_um`, `positive`, `monochrome` | Exact arguments to `apply_grain` |
| `settings` | Every resolved field of Rust `GrainSettings`, not just overrides |
| `n_samples`, `n_layers` | Curve dimensions |
| `files` | Relative filename, exact byte count and SHA-256 for each binary payload |
| `provenance` | Source revision/local diff digest, input hash, profile/preset hashes, recipe and capture-command/build details |
| `effects` | Verified activation of each required grain effect, with derived thresholds/sigmas |

All payloads use little-endian IEEE-754 binary64:

- Density: exactly `width*height*3` values, row-major, interleaved CMY.
- Total curves: exactly `n_samples*3` values, `[sample][channel]`.
- Layer curves: exactly `n_samples*n_layers*3` values, `[sample][layer][channel]`.

For the single-layer model, write one layer equal to the total curve and record
`n_layers=1`, matching `GrainCurves::single_layer`. Resolve positive and monochrome
behaviour from the pipeline's film/parameters. Curves are binary to preserve exact
bits and avoid a language-specific JSON floating parser changing the reference.
Encode finite scalar settings with round-trippable JSON numbers; report unexpected
nonfinite scalar settings rather than silently replacing them. Keep any special
curve bit patterns in the binary payload unchanged.

Verify hashes, all dimensions and lengths before loading data into native arrays.
Reject missing/trailing payload data or inconsistent dimensions. Settings/curves
are loaded before timing; layer calibration derived from them is timed on both
sides. The original density fixture stays immutable and is restored before each
measurement.

Worker count, iteration count and `serial_channels` are benchmark controls recorded
in the result manifest. Run equivalent settings on both sides; they do not alter
the frozen density or curves. Preserve the existing per-layer and microstructure
seed scheme rather than introducing a new global seed.

Reference outputs are separate from inputs. Persist the resulting f64 density and
per-iteration checksum outside timing; validate before interpreting performance.
Do not commit photographs, full-size fixtures, generated C, toolchains or binaries.

## Implemented experiment (2026-10-27)

The directory now contains a separate locked Cargo package, the Goose numerical
implementation and native drivers. Production sources, the workspace manifest and
the workspace lockfile remain unchanged. The baseline calls
`spektrafilm_model::grain::apply_grain` directly in f64 mode.

From the repository root, on macOS arm64:

```sh
python3 scripts/goose/toolchain.py
python3 scripts/goose/build.py
python3 scripts/goose/fixtures.py cases target/goose/fixtures/tiny
python3 scripts/goose/validate.py
# Choose local RAW files; these commands generate fixtures, never benchmark renders.
target/release/goose-grain-experiment capture /path/to/24mp.raw target/goose/fixtures/nikon24
target/release/goose-grain-experiment capture /path/to/60mp.raw target/goose/fixtures/leica60
python3 scripts/goose/fixtures.py provenance target/goose/fixtures/nikon24
python3 scripts/goose/fixtures.py provenance target/goose/fixtures/leica60
python3 scripts/goose/full_validate.py nikon24 leica60
python3 scripts/goose/run_comparison.py
```

Do not run builds, capture, validation and benchmarks concurrently. Full-resolution
f64 fixtures and output verification require several GB of free disk and memory.
`capture` uses Rawler, then a newly built CPU pipeline and its existing pre-grain
dump hook; settings and normalized curves come from that same `Pipeline`.
The two tested photo fixtures retain all required effects. Synthetic cases use an
explicit full-resolution physical pixel size, independent of their tiny dimensions.

`benchmark.py` runs AB/BA native process batches, each with one untimed warmup and
seven timed calls per process (fourteen total per variant). The reported durations come from internal monotonic timers;
`/usr/bin/time -l` contributes **only maximum RSS**, never the subpath duration.
Load, SHA-256 verification, input restoration, pool startup and checksums are out
of timing. Full grain includes calibration, interpolation, RNG seeding, scratch,
all subeffects, internal worker copies/dispatch and final HWC assembly.

Goose's `workers` argument specifies the total CPU budget: 1 runs directly on the
main thread; 4 starts three persistent channel workers plus the coordinator.
Rust's same argument specifies Rayon workers. No more than three channel RNG
streams can run concurrently; Rust additionally parallelizes blur/interpolation.
Goose filters remain serial inside each channel. The direct one-thread path also
packs inputs for the same planar channel routine; packing is timed. `serial` after the sigma placeholder (for example, `grain fixture 4 1 out.f64le 0 serial`)
forces serial channels in either executable; the differential suite checks it.
Goose jobs are ordinary copied flat values, with channel packing and queue copies
inside timing. All output data is consumed outside timing, and final outputs must
have exactly equal SHA-256 hashes before benchmark results are accepted.

`samplers` times the entire 146,960-value corpus, including output construction,
per-seed initialization and the next-draw checks. Standalone blur times one plane
including allocation, with plane extraction outside timing on both sides.

The executable support is currently macOS-specific (CommonCrypto SHA-256 and Mach
RSS). Windows/Linux results have not been measured. The Rust baseline preserves
workspace dependency versions in the separate experiment lock, adding only the
hashing dependencies. See `NOTICE.md` for translated code provenance and the one
shared native scalar math primitive (`libm` 0.2.16 lgamma).


## Archived measurements and cleanup

The measured source revision is `38722c36f72ae3c6a0113c492b98a4c39032359e`
(0.8.0). Publication onto main updated only the five local workspace package
versions in this experiment's lockfile to 0.9.0; registry dependency versions
and the measured grain, RNG and blur sources are unchanged.

The report and per-call measurements are committed under `docs/goose-cpu-results.*`.
Sanitized metadata in `fixtures/nikon24.json` and `fixtures/leica60.json` preserves
resolved settings, payload hashes, capture provenance and original metadata hashes,
without private input paths. Generated toolchains, pixel dumps and experiment
build artifacts were removed after archiving. Run the reproduction commands to
recreate them with suitable local RAWs; the historical photo hashes will only
match the original private inputs.
