# Golden test: same input, same output within a tight delta

Cross-platform correctness check for the port (decided 2026-10-22): within one
platform and build, output must be bit-identical; across platforms, every value
must stay within a tight, stage-aware numeric gate. Byte-identical output across
platforms is not required. The GPU engine is what almost all users run, so it
is a first-class engine here.

## Pieces

| File | What it does |
|---|---|
| `crates/spektrafilm-cli/src/bin/golden_inputs.rs` | Writes the fixed linear float-TIFF inputs |
| `crates/spektrafilm-cli/src/bin/golden_compare.rs` | Compares two raw dumps (and TIFFs) against a gate profile; prints JSON |
| `crates/spektrafilm-cli/src/bin/golden_probes.rs` | Platform math functions and grain samplers on fixed inputs |
| `scripts/golden/run.py` | `run`, `compare`, `quality`, `probe`, `compare-probe` |
| `scripts/golden/gates.json` | Every gate, with where its numbers come from |
| `scripts/golden/test_run.py` | Tests that comparisons can't pass by accident |
| `scripts/golden/manifests/` | Manifests (hashes, sizes, GPU adapter, provenance) per machine |

Diagnostic dumps in the engine (no-ops unless the variable is set), in
pipeline order: `SPEKTRAFILM_DUMP_FILM_SENSITIVITY`, `_TC_LUT`, `_FILM_RAW`,
`_FILM_PRE_LOG`, `_FILM_LOG_RAW`, `_FILM_PRE_GRAIN`, `_FILM_DENSITY`,
`_PRINT_DENSITY`. They bypass the GPU-resident path.

## Inputs

Photos are decoded with **rawler** (never Apple's decoder), shrunk by an
integer box filter to at most 1024 px, and written as linear 32-bit float
TIFFs, which load without any gamma maths. Four synthetic patterns (ramp,
gamut, noise, edges) use integer arithmetic and a fixed order of IEEE
operations.

```bash
cargo build --release --locked -p spektrafilm-cli --bin golden_inputs
target/release/golden_inputs target/golden/inputs \
  ~/Downloads/"Z 6"/DSC_4990.NEF ~/Downloads/"Z 6"/DSC_4997.NEF \
  ~/Downloads/"Z 6"/DSC_5003.NEF ~/Downloads/"Z 6"/DSC_5009.NEF \
  ~/Downloads/"Z 6"/DSC_4985.NEF ~/Downloads/7858115920.dng
```

On another machine, copy the Mac's `target/golden/inputs` rather than
regenerating: `compare` refuses manifests whose input hashes differ.
Regenerating and diffing the hashes separately tests rawler.

## Matrix (184 cases, 484 renders)

- every input × 7 stock pairs, default params (grain on): f64, f32 CPU, GPU;
- every input × 7 stock pairs, grain and glare off (`det`): f64, GPU — this is
  what `quality` uses to check the GPU against the reference pixel by pixel;
- one photo and the noise pattern × every preset in
  `scripts/gpu_cpu_quality/params` (grain, colour, every effect): all engines;
- one photo × 35mm border, keyline + Instagram, crop with rotation, resize.

The matrix is fixed in `run.py`, not discovered from the input folder.

## Run

```bash
cargo build --release --locked -p spektrafilm-cli \
  --bin spektrafilm --bin golden_compare --bin golden_probes
cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64

# Every render, then GPU vs f64 on this machine:
python3 -I scripts/golden/run.py run --label NAME \
  --out scripts/golden/manifests/NAME.json --dump-dir target/golden/dumps/NAME
python3 -I scripts/golden/run.py quality scripts/golden/manifests/NAME.json \
  --dumps target/golden/dumps/NAME

# Against the Mac reference (needs both dump folders on one machine):
python3 -I scripts/golden/run.py compare scripts/golden/manifests/macos-arm64.json \
  scripts/golden/manifests/NAME.json \
  --dumps-a target/golden/dumps/macos-arm64 --dumps-b target/golden/dumps/NAME

# Where do two machines first differ?
python3 -I scripts/golden/run.py probe --label NAME --out-dir target/golden/probe/NAME
python3 -I scripts/golden/run.py compare-probe target/golden/probe/macos-arm64 \
  target/golden/probe/NAME

python3 -I -m unittest discover -s scripts/golden -p 'test_*.py'
```

`run` takes about 2.5 minutes on an M1 Pro and 11 on the Windows box (October 23, default `--jobs`) and keeps about 6 GB of dumps.
Every render runs with a clean environment (no inherited `SPEKTRAFILM_*`,
`WGPU_*`, `RAYON_NUM_THREADS`, `RUST_LOG`), a timeout, and a log on failure.
GPU renders must report the wgpu backend and the GPU-resident path, or they
fail instead of silently measuring the CPU. Every TIFF must equal the
quantisation of its own dump. Any failure makes the command exit non-zero.

## Gates (`gates.json`)

| Profile | Used for | Gate |
|---|---|---|
| `f64` | f64 reference across machines | every value `≤ 1e-12 + 1e-11·max(|a|,|b|)`; normalised RMS ≤ 1e-12; ≤ 1 16-bit code |
| `f64_border` | f64 cases with a border (status PASS-BORDER) | about 4 f32 ulps: the frame is drawn in f32 |
| `f32cpu` | f32 CPU engine across machines | `≤ 1e-5 + 1e-5·max`; RMS ≤ 2e-7; ≤ 1 16-bit code |
| `cpu_grain` | CPU case with grain whose strict gate failed (status PASS-GRAIN) | `≤ 1e-3` per value, ΔE00 max 0.1, ≤ 1 8-bit step |
| `gpu` | GPU across machines, grain off | ΔE00 p99.9 ≤ 0.01, max ≤ 0.02, ≤ 1 8-bit step |
| `gpu_grain` | GPU across machines, grain on | ΔE00 mean ≤ 0.06, p99 ≤ 1.0; ≤ 1 % of pixels over 1, ≤ 0.3 % over 2; RMS ≤ 2e-3. No per-pixel limit |
| `gpu_vs_ref` | GPU vs f64 on one machine, grain off | ΔE00 max ≤ 0.05, p99 ≤ 0.02, ≤ 1 8-bit step |

NaN/Inf and size differences always fail. ΔE00 is computed like
`scripts/gpu_cpu_quality/compare.py` (agrees with colour-science to about 4
significant digits). Statuses other than PASS/IDENTICAL are printed with their
counts so a relaxed gate is never silent.

## Results on 2026-10-22 (M1 Pro; x86_64 build under Rosetta, SSE4.2 only)

**arm64 vs x86_64, tolerance mode: everything passes.**

| Engine | Result | Worst |
|---|---|---|
| f64 | 182 PASS, 2 PASS-BORDER | strict cases: ×0.50 of the gate (`dir_wide`, max 6.2e-13), RMS 1e-14, no 16-bit code changed |
| f32 CPU | 108 PASS, 8 PASS-GRAIN | strict cases ×0.81; grain cases up to 5.2e-4, ΔE00 0.043, 34 16-bit codes, ≤ 1 8-bit step |
| GPU | 182 IDENTICAL, 2 PASS | border and resize (CPU-side, 1 f32 ulp) |

**GPU vs f64 on the M1 Pro (`quality`): 76/76 pass.** Worst ΔE00 0.0054, p99
0.0040, p99.9 0.0046, at most 1 step in 8 bit.

**Same machine:** a second GPU run gave 184/184 identical hashes; for the CPU
engines, two runs and 1/3/10 threads gave identical hashes (on the first,
smaller matrix). The GPU path being bit-identical across the two CPU
architectures shows that CPU-side setup differences (~1e-15 in the LUTs) do not
reach the GPU's f32 output.

**Probes (`compare-probe`, arm64 vs x86_64):**

- First divergence: `10^x` in the film sensitivity (1 of 243 values, 1 ulp).
  Apple's `pow` differs between arm64 and x86 in 162/65 536 probe inputs. The
  TC LUT then differs in 28 % of values (≤ 7e-15) through Accelerate's dgemm,
  and every later stage carries ~1e-15 differences.
- `exp`, `pow`, `cbrt`, `sin`, `cos`, `exp10_inplace` (vvpow) and the f32
  `powf`/`exp` all differ somewhere; `ln`, `log10` and `atan2` agree.
- The numpy-compatible grain samplers are bit-identical for identical inputs,
  including at their algorithm switches (λ = 10, p = 0.5, n·p = 30).
- f32 CPU with grain: pre-grain differences of ~4e-7 become up to 2e-4 after
  grain on about 1 % of values (flipped grain counts). This is why
  `cpu_grain` exists.

## Results on 2026-10-22: Windows vs the Mac

Windows 10, Ryzen 5 3600, GTX 1660 SUPER on Vulkan, MSVC build at f596979
(manifest `windows-gtx1660s-vulkan.json`), against the M1 Pro reference.

| Engine | Result | Worst |
|---|---|---|
| f64 | 182 PASS, 2 PASS-BORDER | strict cases ×0.49 of the gate (`dir_wide`), no 16-bit code changed |
| GPU, grain off | 76 PASS | ΔE00 max 0.0020, p99.9 0.0008, 1 8-bit step |
| GPU, grain on | 108 PASS under `gpu_grain` | ΔE00 mean 0.030, p99 0.60; 0.49 % of pixels over 1; single clumps up to ΔE00 20 |
| f32 CPU, grain off | 8 PASS | 4.7e-6 (×0.47), 1 8-bit step |
| f32 CPU, grain on | 26 PASS/PASS-GRAIN, 82 FAIL-GRAIN | the grain pattern diverges; see below |

**GPU vs f64 on Windows (`quality`): 76/76 pass**, the same worst numbers as
on the M1 Pro (ΔE00 max 0.0054, p99 0.0040).

**Same machine:** two full Windows runs gave 484/484 identical hashes.

What the first run found, and what fixed it:

- Auto-exposure computed its EV and scale with f32 `log2`/`powf`. MSVC's and
  Apple's differ by an ulp, which moved every pixel of three photos by ~1e-7
  (all f64 failures). Both are now computed in f64 and rounded once. Apple's
  `powf(2, x)` is not always correctly rounded either, so on macOS the scale
  moves by one ulp for about 0.6 % of exposures (none in this matrix);
  accepted 2026-10-22.
- Git for Windows checks text files out with CRLF, which changed the film
  data and so its hash. `.gitattributes` keeps LF everywhere.
- `tree_hash` sorted `Path`s, which compare case-insensitively on Windows; it
  now sorts the path strings (the Mac hashes are unchanged).

**GPU grain.** Each pixel has its own PCG stream, so nothing cascades, but the
Poisson and binomial samplers use the GPU's `log`/`exp`, whose precision
differs by vendor. Some draws come out differently and the grain blur spreads
each one: 0.1–1.3 % of pixels move by more than ΔE00 0.5. Accepted
(2026-10-22): `gpu_grain` bounds it statistically. It rejects all 108 grain cases when the GPU render is
compared with the f64 render of the same case (a different grain pattern:
ΔE00 mean ≥ 0.139, against ≤ 0.030 for the cross-vendor drift).

**f32 CPU grain.** The CPU engines draw grain from one MT19937 stream for the
whole image. The f32 libm differs (probes: MSVC's `log10f` and `powf(2, x)`
are not correctly rounded, Apple's `powf`/`expf` in ~0.6 % of inputs), the
first differing value flips one count, and every later draw shifts. 82 cases
end up with a different grain pattern. The f64 engine's differences (~1e-15)
are too small to flip a draw on this matrix.

**Probes:** the first divergence is the film sensitivity (`10^x`, 1 of 243
values); every f64 math probe differs somewhere, `ln`/`log10` least. The grain
samplers are bit-identical.

Running `compare` needs both dump folders on one machine. The dumps are float
noise and the repo is public: transfer them directly, never through git or a
release. See `docs/windows-agent-handoff.md`.

## Watch next

- The engine uses Vulkan or Metal only; Windows has no DX12 path (decided
  2026-10-22).
- The GPU gates come from two vendors (Apple, NVIDIA); AMD, Intel and
  software Vulkan may need more room.
- Windows full-resolution capacity is now measured: 96 MP completed without
  a display reset; 144 and 178.95 MP returned success while Windows logged a
  driver reset; 180 MP panicked at the storage-binding limit. Since
  f27722c an admission check renders work that doesn't fit on the CPU, and
  the CLI's `Backend:` line (printed after the render) names the backend that
  actually rendered. Watchdog and device-loss recovery for work that fits is
  still open. See `docs/cross-platform-port.md`. None of this is a reason to
  loosen a correctness gate.

## Full-resolution performance and startup

`benchmark.py` uses the same linear-input parameters and engine selection as
the golden matrix. It checks the GPU backend and resident path, hashes repeated
outputs, and records per-process/pipeline timings. On NVIDIA it samples
whole-device memory at 100 ms; on Windows it also checks System event 4101 and
fails a run with a display reset, even if the CLI returned success.

```bash
target/release/golden_inputs --long-edge 100000 target/windows-perf/inputs INPUT.NEF
python3 -I scripts/golden/benchmark.py target/windows-perf/inputs/photo_INPUT.tif \
  --out target/windows-perf/full --label MACHINE --raw --profile

# Empty vs populated application cache: reuse a new, private cache directory.
# These are fresh processes; the OS and GPU driver's own caches may be warm.
python3 -I scripts/golden/benchmark.py target/golden/inputs/photo_DSC_4990.tif \
  --out target/windows-perf/startup-cache --label MACHINE --engines gpu \
  --repeats 4 --pipeline-cache-dir target/windows-perf/new-cache
python3 -I scripts/golden/benchmark.py target/golden/inputs/photo_DSC_4990.tif \
  --out target/windows-perf/startup-disabled --label MACHINE --engines gpu \
  --repeats 4 --disable-pipeline-cache

# Synthetic capacity stress, generated with bounded host RAM.
python3 -I scripts/golden/capacity_input.py target/windows-perf/capacity.tif 12000 8000
python3 -I scripts/golden/benchmark.py target/windows-perf/capacity.tif \
  --out target/windows-perf/capacity --label MACHINE --engines gpu --repeats 1
```

Use `--preset` to exercise the golden effects and `--profile-only` to diagnose
GPU stages without first repeating an unsafe normal submission. Profiling
waits between stages and **cannot** establish normal-submit watchdog safety.
Pass `--timeout 120` for capacity experiments so a stalled render is killed.
`--raw` adds a diagnostic dump to wall time; omit it for export/startup latency.
Keep logs, images, dumps and WPR traces in `target/`. Only reviewed reports,
hash manifests and documentation belong in this public repository.

The 2026-10-23 performance report is in
`scripts/golden/reports/windows-performance-2026-10-23.json`. Early capacity
runs predated the event-log check; their two apparent successes have been
explicitly marked failed after matching Windows' display-reset events.

## Not covered yet

RAW decoding (rawler vs Apple), PNG and integer TIFF inputs, JPEG/PNG output,
alternate colour spaces, post adjustments, quarter-turn rotations, odd and
full-resolution sizes, and the GPU's grain against the CPU's statistically
(`scripts/gpu_cpu_quality/grain.py` does that).
