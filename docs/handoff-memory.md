# Memory optimization handoff

## User decisions — authoritative scope

The goal is to **reduce overall memory usage**, not meet an absolute 8 GB limit. The working CPU prototype and optional serial-grain UI setting are included in this change. The next agent should proceed in this order:

1. Reuse GPU scratch buffers across sequential effects.
2. Further consume/reuse CPU stage buffers.

**Keep full-resolution previews.** Do not implement a preview cap, an automatic lower-resolution mode, a change to the Working resolution default, or a new precision default. Preserve image calculations and default export speed. Serial grain is strictly opt-in because its memory savings cost substantial time. Tiling, f32 exports and smaller previews were researched alternatives, not approved next tasks.

The user authorized committing the working prototype and preparing this handoff. GPU/CPU follow-up implementation is intentionally left for the next agent. No remote push or release packaging was requested. Do not touch the user's unrelated untracked `TODO.txt`, `problem.txt`, or `dist/` artifacts.

## Working baseline

Before optimization the source was `3704ff2`. This commit includes:

- `core/src/pipeline.rs`: release input after filming expose, log exposure after develop, film density after printing.
- `core/src/stages/filming.rs`: release exposure-adjusted RGB before optical effects.
- `io/src/lib.rs`: release sensor data before converting developed RGB.
- `model/src/couplers.rs`: release silver-density copy early; blend/write one correction channel at a time.
- `model/src/grain.rs`: recompute probability/saturation scalars instead of retaining two f64 planes per concurrent channel; preserve whole-image Poisson-before-binomial RNG order; scalar particle blur.
- `model/src/glare.rs`: blur the scalar field directly instead of three identical RGB channels.
- `settings.serial_grain: bool`, default **false**, explicitly passed to CPU grain scheduling. This replaces the initial `SPEKTRAFILM_SERIAL_GRAIN` experiment flag; the environment flag no longer controls the application.
- GUI **Output → Serial grain for export (less RAM)**. The normal export JSON carries the choice. `App::preview_params()` clears it for all previews, including CPU fallback, without changing the user's export choice. Toggling it does not trigger a render; Reset settings turns it off. Grain-disabled renders naturally ignore it.

Both release executables must be rebuilt separately when changing the GUI and export code. Building the GUI alone leaves the f64 companion stale. Do not build the GUI with the f64 feature as the normal application artifact.

## Measurements and artifacts

Read [the investigation](memory-study-q3.md), [measurement data](memory-study-q3-results.json), and [chart](memory-study-q3.png). The investigation contains historical options; the scope at the top of this handoff supersedes the original preview recommendation.

Input: `target/memory-study/leica-q3.dng`, copied from `~/Downloads/7858115920.dng`; verified Leica Q3, 9520 × 6336 (60.32 MP). One f64 RGB frame is 1.348 GiB; f32 is 0.674 GiB. Test host: M1 Pro / 10 CPU cores / 32 GB. Kodak Gold 200 → Portra Endura, default effects, GUI-equivalent LUT CPU export settings.

| CPU configuration | Live Rust heap peak (instrumented) | macOS footprint peak (separate normal run) | Normal elapsed time |
|---|---:|---:|---:|
| Before prototype | 12.14 GiB | 15.05 GiB | 40.0 s |
| Prototype, parallel grain | 8.10 GiB | 11.82 GiB | 34.2 s |
| Prototype, serial grain | 6.75 GiB | 9.96 GiB | 56.7 s |

All three full-resolution TIFF outputs were byte-identical. These times are illustrative; the first baseline run took 55.8 s. Host compression/cache pressure changes RSS and timings. **Do not compare allocator-instrumented timings with normal binaries.** Instrumentation counts Rust-requested live bytes, not native caches, stacks, driver or BLAS memory. macOS footprint includes accounting for compressed pages and differs from RSS. CPU process measurements exclude the GUI; CLI GPU measurements exclude GUI textures and previous outputs.

Default full-resolution GPU render through CLI: 8.27 GiB footprint, 6.41 s including load/save, 4.88 s pipeline. This is useful for follow-up comparison, but preserve a freshly built current binary before changing GPU code for a matched baseline.

Local ignored artifacts are under `target/memory-study/`: original `baseline-f64`, prototype `final-f64`, copied `baseline-src/`, params JSON, CSV samples, logs, parity fixtures and TIFFs. They are not guaranteed to exist in another checkout. Do not commit RAW files, TIFF outputs or binaries. Reconstruct the original source from `3704ff2` in an isolated checkout if necessary. The committed results JSON preserves the key measurements and hashes.

## First task: GPU scratch reuse

Start in `crates/spektrafilm-gpu/src/wgpu_backend.rs`, resident chain and its effect builders:

- `buf_a` and `buf_b`: two primary images.
- `build_halation_state`: two full-image scratch buffers (`halation_c`, `halation_d`).
- `build_dir_state`: three full-image scratch buffers (`dir_correction`, `dir_gaussian`, `dir_mix`).
- `build_unsharp_state`: two (`unsharp_out`, `unsharp_blur_dst`).
- `build_glare_state`: one (`glare_scratch`).

Ten default full-image f32 buffers total ~6.74 GiB for this sample. They are allocated up front and retained for a single command buffer, but effect scratch lifetimes do not all overlap logically. A shared pool of roughly three scratch images plus the two primary images may save up to five frames (~3.37 GiB). **This is a source-based opportunity, not an implemented/measured saving.**

Build a pass/lifetime table before editing. Thread reusable buffers into state builders. Preserve distinct source/destination storage where a single dispatch requires it; verify wgpu binding/usage and read/write aliasing constraints. GPU lifetime follows command execution and bind-group references, so dropping a Rust handle is insufficient. Keep a single submission if feasible to avoid extra synchronization/readbacks. Account for optional camera/enlarger diffusion, scanner lens blur, highlight boost, direct film scans, disabled effects, mapped-primary-buffer and readback paths. Do not merely cache all existing buffers, which could increase retained memory.

Validate default and selected effect combinations, a second image size, repeated renders, and scan-film paths. Compare GPU output against the **same GPU backend before the change**, not f64 CPU output: their existing math and stochastic implementations differ. Measure both peaks and retained idle memory. The existing CLI `--iters` holds/clones extra images and previous output, so do not conflate its peak with a single render or the actual GUI.

## Second task: CPU ownership and scratch reuse

Read `core/src/stages/{filming,printing,scanning}.rs`, `model/src/{grain,couplers,diffusion,glare}.rs`, and `math/src/gaussian.rs`.

Remaining opportunities:

- Let expose consume the input and mutate exposure in place instead of cloning a borrowed RGB frame.
- Let development release log exposure immediately after DIR, before grain; it is currently retained throughout `develop`.
- Consume/reuse density and correction buffers in DIR rather than retaining multiple full images around blur scratch.
- Consume scanning input before glare/unsharp where possible; reuse destinations for per-pixel transforms.
- Reuse single-channel Gaussian/exponential scratch and avoid redundant interleaved/channel copies. Keep channel order and summation order unchanged.
- Investigate preallocated indexed RAW conversion instead of parallel `flat_map_iter().collect()`. Decode/demosaic still happens at full resolution; preserve RAW white balance, crop, orientation, monochrome and FourColor behavior.

The CPU spectral contraction is already tiled (262,144 pixels, ~162 MiB scratch at 81 wavelengths), and GUI exports enable LUTs. Do not start by redesigning it as though it allocated a full spectral cube. Preserve RNG sequence and precision: per-pixel Poisson/binomial interleaving or naïve tiling changes grain.

No fixed numerical memory target is required. Measure each change, retain exact output where algorithms are unchanged, and reject substantial default export slowdowns. Keep serial grain optional even if later reductions make it less useful.

## Validation and reproduction

Commands are run from the repository root:

```sh
cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64

cargo build --release --locked -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
cargo build --release --locked -p spektrafilm-gui -p spektrafilm-cli --bin spektrafilm-gui --bin spektrafilm

# Exact before/after f64 regression, requires numpy + tifffile:
python3 scripts/parity/check_memory_optimizations.py \
  --baseline target/memory-study/baseline-f64 \
  --candidate target/release/spektrafilm-f64 \
  --output-dir target/memory-study/next-parity

# Measure normal CPU export. export.json enables enlarger/scanner LUTs.
SPEKTRAFILM_BACKEND=cpu SPEKTRAFILM_STAGE_TIMINGS=1 \
  python3 scripts/profile_memory.py target/memory-study/next-cpu \
  target/release/spektrafilm-f64 process target/memory-study/leica-q3.dng \
  --film kodak_gold_200 --params target/memory-study/export.json \
  -o target/memory-study/next-cpu.tiff

# Independent live Rust heap measurement; timings are instrumented:
cargo build --release --locked -p spektrafilm-cli --example memory_profile --features precision-f64
# Substitute target/release/examples/memory_profile for the executable above.
```

Minimal CPU export params:

```json
{"settings":{"use_enlarger_lut":true,"use_scanner_lut":true,"serial_grain":false}}
```

Set `serial_grain` to true to test the optional mode. For GPU comparison, build the default f32 CLI, use `SPEKTRAFILM_BACKEND=wgpu` and default `{}` params. Do not run performance benchmarks concurrently with compiles or other benchmarks. Keep diagnostic full-image dumps off during memory measurements.

The committed regression harness compares four synthetic cases (colour, LUT-off, B&W, slide; different grain layer counts and glare blur) against the original binary, in both serial and parallel modes. Model tests compare exact grain bits under a three-thread pool. GUI tests cover opt-in/backward-compatible params, preview exclusion, export subprocess JSON and resetting the setting. The existing camera RAW smoke test is ignored unless `SPEKTRAFILM_TEST_RAWS` supplies local fixtures.

## Final validation of the UI/parameter implementation

- `cargo test --workspace --locked`: 126 passed, 1 ignored.
- The same workspace suite with `spektrafilm-cli/precision-f64`: 128 passed, 1 ignored.
- Release GUI, f32 CLI and f64 companion builds succeeded.
- `scripts/parity/check_memory_optimizations.py`: all four cases match original f64 output bytes in both serial and parallel mode (8 comparisons).
- Full Q3 export using `settings.serial_grain: true` matches the original TIFF byte-for-byte (`commit-serial.tiff`, log under `target/memory-study/`). This was a correctness run alongside compilation, not a new performance benchmark.
- GUI tests exercise parameter flow and the export subprocess contract headlessly. No manual window interaction was performed.
- `git diff --check` passed before commit.
