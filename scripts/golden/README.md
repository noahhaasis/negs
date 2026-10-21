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

`run` takes about 15 minutes on an M1 Pro and keeps about 6 GB of dumps.
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
| `f32cpu` | f32 CPU engine across machines | `≤ 2e-6 + 2e-6·max`; RMS ≤ 2e-7; ≤ 1 16-bit code |
| `cpu_grain` | CPU case with grain whose strict gate failed (status PASS-GRAIN) | `≤ 1e-3` per value, ΔE00 max 0.1, ≤ 1 8-bit step |
| `gpu` | GPU across machines | ΔE00 p99.9 ≤ 0.05, max ≤ 1.0, ≤ 0.01 % of pixels over 0.5, ≤ 2 8-bit steps — **provisional** |
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

## Watch on Windows

- `dir_wide` already uses half the f64 gate on Rosetta; Windows adds rustfft's
  AVX path, MSVC's math library and the naive dgemm.
- The `gpu` gate is a guess until a second GPU has been measured.
- `WGPU_*` variables are stripped: choosing DX12 vs Vulkan needs an explicit
  engine setting first (the backend hardcodes `Backends::PRIMARY`).
- Copy `target/golden/inputs`, the Mac manifest and its dump folder to the
  Windows machine (or the Windows dumps back) to run `compare`. The dumps are
  float noise (gzip saves ~5 %) and the repo is public: transfer them
  directly, never through git or a release. See `docs/windows-agent-handoff.md`.

## Not covered yet

RAW decoding (rawler vs Apple), PNG and integer TIFF inputs, JPEG/PNG output,
alternate colour spaces, post adjustments, quarter-turn rotations, odd and
full-resolution sizes, and the GPU's grain against the CPU's statistically
(`scripts/gpu_cpu_quality/grain.py` does that).
