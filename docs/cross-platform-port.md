# Cross-platform port: Linux, Omarchy, Windows

Notes from the portability review on 2026-10-22, based on `main` at a5c360f
(Release 0.7.0), followed by measurements on macOS and Windows. The Windows
engine now builds and passes the calibrated f64/GPU gates on a GTX 1660 SUPER.
The historical alternatives below are superseded by the decisions in
`windows-agent-handoff.md`.

Plan (updated 2026-10-22): Windows first, then Linux and Omarchy (Arch +
Hyprland). Each platform gets a native UI. Launch speed is a hard requirement;
dark mode on Windows is not. Same input must give the same output.

## Full-resolution measurements (2026-10-22/23)

Input: one Nikon Z 6 photo, 6048 × 4024 (24.337 MP), decoded with rawler into
linear float TIFF. Independent Mac and Windows decodes have the same SHA-256:
`d3aa7348d100d9d0545c9b35a46469e7d167357a3f975e67ade25cd60e98516d`.
Portra 400 → Endura, default effects including grain; linear sRGB input,
no input transfer-function decoding. Two fresh processes per engine.

| Measurement | M1 Pro / Metal | Ryzen 5 3600 / GTX 1660 SUPER / Vulkan |
|---|---:|---:|
| GPU pipeline | 724–757 ms | 891–935 ms |
| GPU pipeline per MP | 29.7–31.1 ms | 36.6–38.4 ms |
| f64 CPU pipeline | 15,961–16,395 ms | 35,111–35,479 ms |
| GPU process wall, including TIFF and diagnostic raw dump | 1.79–1.84 s | 2.81–3.04 s |
| f64 process wall, including TIFF and diagnostic raw dump | 17.37–18.32 s | 37.22–37.23 s |
| GPU whole-device peak / idle baseline | not measured | 1510 / 351 MiB |

Pipeline timings exclude image load, device creation and output encoding.
Wall timings include the 584 MB diagnostic dump and are **not** ordinary
export latency. The original 22 s Mac measurement used a different input
path; it is not the denominator for this comparison. The NVIDIA memory
numbers are whole-device samples every 100 ms, including the desktop, not
per-process allocation counts. They can miss short peaks.

Both repeats and the separately profiled GPU run are byte-identical within
each machine. The full-resolution f64 TIFF is also identical between machines.
The raw f64 comparison passes the existing gate: max absolute delta
2.87e-13, normalised RMS 1.34e-14, worst value 0.0925× the gate, no changed
16-bit codes. No tolerance or numerical implementation changed.

Separate GPU stage profile (`SPEKTRAFILM_GPU_PROFILE=1`, ms):

| Stage | M1 Pro | GTX 1660 SUPER |
|---|---:|---:|
| Upload | 1.5 | 52.4 |
| Front | 52.8 | 11.5 |
| Halation | 168.9 | 155.1 |
| DIR couplers | 118.1 | 121.1 |
| Grain | 101.8 | 30.4 |
| Print spectral | 39.2 | 24.2 |
| Scan spectral | 40.0 | 24.2 |

This mode submits and waits between stages, so it changes scheduling and
cannot establish watchdog safety for a normal export. It includes stage
setup/compilation overhead, not just GPU execution. Windows f64 stage timings
under a WPR CPU recording were: expose 5.218 s, develop 9.277 s (DIR 3.690 s,
grain 5.361 s), printing 8.800 s, scanning 11.799 s. The private ETL is retained
under `target/windows-perf/`; no CPU math optimization has been made. Inspect
sampled stacks before attributing the print/scan cost specifically to BLAS.

### Capacity and Windows watchdog

Deterministic synthetic float TIFFs from `scripts/golden/capacity_input.py`,
same stock/effects, normal unprofiled submissions. These timings describe
stress inputs and are not directly comparable with the photo above.

| Size | Pipeline | Peak device memory | Result |
|---|---:|---:|---|
| 54 MP (9000 × 6000) | 2.046 s | 2916 MiB | completed; no matching display-reset event |
| 96 MP (12000 × 8000) | 3.144 s | 4919 MiB | completed; no matching display-reset event |
| 144 MP (14400 × 10000) | 5.756 s | 5924 MiB | exit 0 **but display driver reset** |
| 178.95 MP (15000 × 11930) | 6.410 s | 5892 MiB | exit 0 **but display driver reset** |
| 180 MP (15000 × 12000) | — | 2416 MiB before failure | validation panic, exit 101; no CPU fallback |

Windows System event 4101 (`nvlddmkm stopped responding`) occurred at
22:00:57 and 22:02:50 UTC on October 22, during the 144 MP and 178.95 MP
renders respectively. Their success exit codes do **not** establish correct
output or device stability. The benchmark now rejects runs with a matching
display-reset event. Largest tested size without a reset: **96 MP**; this is
not a guarantee for other effects, drivers or images.

The adapter accepts at most 2,147,483,647 bytes in one storage binding:
178,956,970 RGB f32 pixels theoretically. At 180 MP the 2,160,000,000-byte
image binding is rejected by `Device::create_bind_group` (`bg_hanatos`). The
test at 178.95 MP approaches that binding limit and the dedicated VRAM ceiling;
it is not a supported operating size. Host-memory spill is plausible from
the memory plateau; per-process shared-memory accounting was not measured.

These measurements prompted the admission guard below. **Still unresolved:**
handle watchdog/device loss without claiming a successful export, including
long-running effects that fit in memory. Investigate shorter submissions with
unchanged shader math and exact-output checks. Do not raise Windows TDR
timeouts or loosen the image gates to hide these failures.

A subsequent 178.95 MP run with per-stage profiling also triggered event 4101
(22:16:12 UTC), then stalled with zero GPU utilization, about 5892 MiB device
memory and 7.6 GB process working set. It was explicitly terminated after
125 s. This diagnostic overlapped the small CPU correctness cases, so its
timing is not a capacity benchmark; it nevertheless shows that per-stage
waits alone did not prevent a reset/stall. Use a bounded `--timeout` for
further experiments.

### Capacity admission guard (October 23)

Windows Vulkan now checks the image buffer against both the adapter's binding
and allocation limits before entering the render. On discrete GPUs it also
queries the largest Vulkan device-local heap and reserves 20% for the desktop,
other applications and driver overhead. The estimate includes four RGB f32
buffers, 128 MiB for LUTs/small allocations, and all camera/enlarger diffusion
FFT buffers (both padded complex grids and the kernel spectrum). FFT bindings
are checked separately. These are conservative estimates, not a live memory
budget or a guarantee against a watchdog reset.

Rejected work uses the CPU backend for the **whole** pipeline. Merely declining
the resident fast path would otherwise fall through to individual GPU stages
and repeat the allocation failure. Working-resolution scaling still occurs
exactly once. The helper's borrowed resident path declines rejected work so
its existing fallback can route through the same CPU path. Metal retains its
existing rendering behavior.

`SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB` can only lower the Windows admission budget
for diagnostics. On the Windows test box, forcing image rejection with 0 MiB
and FFT rejection with 160 MiB both produced raw buffers and TIFFs identical
to explicit f32 CPU rendering. Three pure capacity tests and an integration
test checking whole-pipeline fallback and single rescaling pass on Mac and
Windows. CPU fallback retains the existing f32 CPU grain behavior; its
cross-platform grain decision remains deferred.

### Vulkan pipeline cache

The backend now requests `PIPELINE_CACHE` only when supported on Vulkan and
passes a disk-backed cache to compute pipeline creation. It uses wgpu's
adapter key and driver/ABI validation, plus a bounded, checksummed envelope
and atomic file replacement. Missing, stale, corrupt or unwritable cache
files do not prevent ordinary compilation. A completed resident render saves
new pipelines; Drop also saves standalone pipeline use. Metal does not use it.

Default storage: `%LOCALAPPDATA%/Negs/Cache/gpu` on Windows;
`$XDG_CACHE_HOME/negs/gpu` or `~/.cache/negs/gpu` on Linux.
`SPEKTRAFILM_PIPELINE_CACHE_DIR` selects an isolated test directory;
`SPEKTRAFILM_DISABLE_PIPELINE_CACHE=1` disables the feature.

On the 1008 × 670 golden photo, a new application cache took 142 ms for the
first pipeline; three subsequent processes took 112 ms each. With application
caching disabled the same build took 105–113 ms. Approximate start-to-image
inside the CLI (one iteration, `Total − Saved`, excludes process teardown):
343 ms with an empty cache, 293–303 ms with a populated cache, 284–295 ms with
the cache disabled. The resulting cache was 256,069 bytes. All these TIFFs
were identical. **No demonstrated warm-start speedup on this NVIDIA driver.**
Its own shader cache and OS file caches were already warm; these are fresh
process / empty-application-cache measurements, not reboot-cold measurements.
Before this change, in-process repeat pipelines took 23–26 ms.

Validation: **484/484 outputs byte-identical on each machine** against its
previous manifest (184 GPU, 184 f64, 116 f32 CPU). The existing cross-platform
f32 CPU grain differences remain deferred; this change introduces none.
Cache unit tests pass on Mac and Windows; corrupt and unwritable cache paths
also produce the same golden TIFF on Windows. The 22 harness tests pass.
Updated full manifests are `macos-arm64-pipeline-cache.json` and
`windows-gtx1660s-vulkan-pipeline-cache.json` in `scripts/golden/manifests/`.

The Win32 first-paint, first-interaction and first-usable-image measurements
remain pending the UI spike result and approval of the UI plan.

## Where the code stands

| Area | macOS today | Elsewhere today |
|---|---|---|
| UI | Swift app (`macos/`), uses Core Image in `Original.swift` | egui GUI builds but is unmaintained |
| Engine process | `spektrafilm-native`, JSON lines over stdin/stdout, previews written to a path | Same binary; portable as is |
| GPU | wgpu, Vulkan or Metal only, all 28 shaders WGSL | Vulkan on Linux and Windows (no DX12, decided 2026-10-22); falls back to CPU if no adapter |
| RAW decode | Apple decoder (`raw/apple_raw.m`), rawler fallback | rawler only |
| BLAS (dgemm) | Accelerate | OpenBLAS on Linux, naive triple loop on Windows (`cpu_backend.rs:72`) |
| `exp10_inplace` | vForce `vvpow` (`spektrafilm-math/src/vforce.rs`) | scalar `powf` loop |
| CUDA backend | n/a | 2.3k lines, behind `cuda-backend`; only kept compiling (TODO(cuda)) |

The `.metal` shaders in `spektrafilm-shaders/metal/` are not used.

## GPU risks off Apple Silicon, worst first

1. **Buffer size and VRAM.** Image buffers are bound whole;
   `device_can_bind_a_100_mp_image` (`wgpu_backend.rs:5127`) only asserts the
   reported limits and allocates nothing. wgpu-hal 24 reports ~2 GiB on DX12
   (correction: not tied to VRAM); Vulkan's `maxStorageBufferRange` varies by
   driver. The real constraint is peak VRAM (several image buffers, scratch,
   FFT padding, staging) on 4–8 GB cards. Start with a conservative capacity
   check and CPU fallback; tiling changes results for FFT, recursive blurs,
   global exposure and the sequential grain stream.
2. **Results differ between vendors.** WGSL `exp`/`log`/`pow` precision is up to
   each implementation, the shader compile chain differs per API, and drivers
   differ in fusing multiply-adds. On the Mac the GPU already differs from the f64
   engine (worst pixel ΔE00 0.19, at most one 8-bit step, commit 36ad0ce). Aim for
   that tolerance on every GPU, not bit equality.
3. **Watchdogs and lost devices.** Windows resets the GPU when one piece of work
   runs about 2 s; Linux drivers have similar timeouts. Large blur/FFT passes on a
   weak iGPU at high megapixels could hit it. No device-lost recovery exists yet.
4. **No DX12.** Decided 2026-10-22: Windows uses Vulkan only. Every GPU worth
   running on has it (NVIDIA and AMD since 2012, Intel since Skylake, 2015; every
   Windows 11 PC); the gap is Haswell/Broadwell iGPU laptops, which get the CPU
   engine. That avoids FXC (slow) or shipping DXC, and wgpu's pipeline cache is
   Vulkan-only.
5. **Adapter choice and testing.** `HighPerformance` behaves differently on
   hybrid laptops. CI has no GPUs; lavapipe is a CPU Vulkan that is enough to
   test correctness. On a machine whose only Vulkan device is lavapipe/llvmpipe,
   wgpu would pick it over the CPU engine; decide whether to reject
   `DeviceType::Cpu` adapters (Linux).
6. **Performance numbers don't carry over.** ~33 ms/MP and the blur tuning were
   measured on Apple GPUs. Discrete GPUs use the staging upload path
   (`MAPPABLE_PRIMARY_BUFFERS` is only requested on integrated GPUs).

## Vulkan availability

- Vulkan ships with the GPU driver: Mesa (RADV for AMD, ANV for Intel, NVK for
  NVIDIA) or NVIDIA's proprietary driver. Ubuntu and Fedora install it by
  default; plain Arch needs `vulkan-radeon` / `vulkan-intel` etc.
- **Omarchy installs it by default.** `install/hardware/vulkan.sh` (run from
  `install/hardware/all.sh`) picks `vulkan-intel`, `vulkan-radeon` or
  `vulkan-asahi` from `lspci`; NVIDIA gets it via `nvidia-utils` in
  `install/hardware/nvidia.sh`. `omarchy-hw-vulkan` checks for an ICD. (Read on
  the `quattro` default branch of basecamp/omarchy.)
- Windows: NVIDIA, AMD and Intel drivers include Vulkan (Intel from Skylake).

## Linux / Omarchy UI

- **GTK4 + libadwaita via gtk4-rs.** Omarchy ships Nautilus (GTK4/libadwaita)
  and its theme switcher (`omarchy-theme-set-gnome`) sets Adwaita or
  Adwaita-dark plus `color-scheme`, so a libadwaita app follows Omarchy themes
  and is native on GNOME. Theme colours live in
  `~/.local/state/omarchy/current/theme/colors.toml` if accents should match.
- Hyprland tiles windows: the layout must work at half-screen and narrow widths
  (libadwaita breakpoints).
- Packaging: pacman/AUR for Omarchy, Flatpak for other distros.
- Startup: create the wgpu device and compile pipelines on a background thread
  after the first frame. Vulkan gets wgpu's `PipelineCache` and the driver's
  disk shader cache.
- Same structure as Ghostty: shared core, per-platform native UI.

## Windows UI

Two candidates. Decide with a startup spike on the Windows machine before
building anything.

**WinUI 3 (Windows App SDK), C#, Native AOT.**
- Powerful enough: the Windows Photos app is built on it. Has Fluent controls,
  Mica, light/dark/accent, virtualised lists for the film strip, and
  `SwapChainPanel` for a GPU-rendered image.
- Weak exactly where Negs is sensitive: cold start and per-control cost of XAML,
  the same pattern as the SwiftUI `Form` on macOS. Also runtime deployment
  (Windows App SDK runtime or self-contained) and a younger, quirkier framework.

**Plain Win32 (Rust via windows-rs) with custom-drawn controls.**
- Fast: a Win32 window appears in tens of milliseconds, with no XAML or .NET.
- Native for free: title bar (dark via `DWMWA_USE_IMMERSIVE_DARK_MODE`), Mica via
  `DWMWA_SYSTEMBACKDROP_TYPE` (Windows 11 22H2+), menus, context menus, tooltips,
  `IFileDialog`, DPI scaling.
- Not native for free: the common controls look like Windows 7–10 and have no
  supported dark mode. A dark photo editor has to draw its sliders, dropdowns,
  toggles and inspector itself (Direct2D/DirectWrite) in the Fluent style, and
  implement UI Automation, keyboard focus and IME for them. Pro apps (Lightroom,
  Capture One, Resolve) draw their own controls anyway.
- wgpu renders straight into the `HWND`, with no interop layer.
- Matches the hybrid approach preferred for macOS (AppKit shell, SwiftUI views,
  Core Animation where it matters): Win32 shell, custom-drawn hot UI, XAML
  Islands only where startup doesn't matter (settings, dialogs).

Update after Noah's answer (launch speed hard, dark mode unimportant): **Win32
with the standard common controls.** In light mode they are native, each one
is cheap to create, and accessibility, focus and text input come for free.
Custom-draw only what needs it (image view via wgpu, possibly the film strip).
Noah runs the startup spike (blank window + ~40-slider inspector, cold and
warm, Win32 vs WinUI 3) on the Windows machine.

Either way, the UI starts `spektrafilm-native` and speaks the same JSON
protocol as the Swift app. Correction: that is not the whole export path — the
Reference export launches `spektrafilm-f64` separately (`Engine.swift`), and
`Save` in the helper only saves the cached render. Port both.

## Same input, same output

### What can be exact

- **f64 CPU engine:** byte-identical across macOS and Linux is achievable once
  the math is portable.
- **GPU (preview and GPU exports):** tolerance only, as above.
- **RAW:** Apple's decoder and rawler differ. Test with rawler on both
  machines (pure Rust, so it should be deterministic) and with float EXR/TIFF
  inputs.

### Blockers in the f64 engine today

- ~126 calls to `powf`/`exp`/`ln`/etc. in core/math/model/io go to the system
  math library (Apple libm vs glibc), plus `vvpow` in `vforce.rs`.
- dgemm via Accelerate vs OpenBLAS: different summing order.
- `FftPlanner::new()` in `spektrafilm-math/src/fft_conv.rs:63` and
  `spektrafilm-core/src/input_gamut.rs:76` picks AVX/SSE/NEON at runtime, so
  results already differ between Apple Silicon and Intel.
- No thread-count-dependent float reductions were found.

Fix: one deterministic implementation on every platform — correctly rounded
math functions (or the `libm` crate, already a dependency), an in-house dgemm
with fixed summing order, `FftPlannerScalar`. This changes macOS exports once in
rare last bits and drops exact numpy parity on macOS. Linux can't match today's
macOS output byte for byte because Accelerate and Apple libm are closed.
**Decision needed from Noah before doing this.**

### Cost of portable math (measured 2026-10-22, M1 Pro, AC power)

Full-resolution f64 CPU render (24.3 MP NEF via rawler, Portra 400 → Endura,
default params incl. grain): 22.0 s. `sample` profile of active CPU time:

| Share | Where |
|---|---|
| 35.5 % | own code (rayon closures) |
| 27.2 % | vForce `vvpow` (10^x) |
| 21.8 % | system libm: `pow` 12.4 %, `atan2` 3.0 %, `log10` 1.7 %, `log` 1.4 %, `exp`, `sincos`, `exp10`, … |
| 11.1 % | grain RNG (numpy binomial/Poisson) |
| 3.6 % | Accelerate BLAS |
| <0.1 % | rustfft (not in this render's hot path) |

Single-thread microbenchmark, ns per value (Apple vs the pure-Rust `libm`
crate 0.2.16, already a dependency):

| Function | Apple | `libm` crate |
|---|---|---|
| 10^x | 4.1 (`vvpow`), 5.5 (`pow`) | 38.2 (`pow`), 7.4 (`exp10`) |
| pow(b, x) | 10.5 | 40.2 |
| log10 | 3.5 | 5.8 |
| exp | 5.4 | 6.4 |
| atan2 | 18.8 | 15.9 |

Bit agreement Apple vs `libm` crate: pow 90 %, exp 90 %, log10 99 %, atan2 79 %.

Estimate (corrected after review): swapping in the `libm` crate as is makes f64
exports roughly 3–3.6× slower (`pow` dominates). Using a deterministic `exp10`
for the 10^x calls alone is about +22 % by the same model. A fast deterministic
general `pow` (ARM optimized-routines or CORE-MATH) is unmeasured; its FMA and
non-FMA variants are different algorithms, so it must be tested on native x86
without FMA. Portable dgemm is not "a few percent": BLAS is 3.6 %, so a 10×
slower kernel would add ~32 %. These are sample-share models, not end-to-end
measurements. GPU shaders are unaffected, but GPU paths share CPU-built LUTs, so
their uploaded values and setup cost would change. Portable math improves
reproducibility, not accuracy: Apple's libm is already accurate to about 1 ulp.
Also needed for real determinism: rustfft twiddles use system `sin`/`cos`, the
`libm` crate has arch-specific code paths, and the f64 engine has f32 maths
(auto-exposure `powf` in `stages/filming.rs`, borders in `film35.rs`).

### Alternative: accept a delta

Zero performance cost, keeps numpy parity on macOS. Observed cross-arch
difference is ~1e-14 absolute in the raw f64 output, ~11 orders below a
visible change and ~9 below one 16-bit step. A tight numeric tolerance on the
raw f64 dumps (e.g. relative 1e-9) still catches any real porting bug. Visual
ΔE thresholds would be too loose to catch bugs. Risk: discrete steps (grain
counts, thresholds, f32 conversions such as the 35mm border) can turn a
last-bit difference into a larger local one, so the metric needs a rule for
rare outliers. Byte-identical exports across platforms would then hold for most
images but not be guaranteed. Expected chance of a single 16-bit code flip from
a 1e-14 difference: about 1e-14 / 1.5e-5 per value, so about 5 % of 24 MP
exports would have one flipped value (ignoring amplifiers).

### Decision proposed after review

Tight, stage-aware numeric equivalence across platforms; exact hashes within one
platform/build. No portable-math rewrite as a prerequisite for the port. Grain
is handled separately: its numpy-compatible sampler consumes one shared random
stream, so a last-bit difference that flips one rejection decision shifts every
later draw. Initial gates (to validate on Windows): f64 values
`|a−b| ≤ 1e-12 + 1e-11·max(|a|,|b|)` everywhere plus normalised RMS ≤ 1e-12;
f32 CPU `2e-6 + 2e-6·max` with RMS ≤ 2e-7; dimensions exact; NaN/Inf fail;
encoded pixels exact except a one-code difference where the raw values straddle
the rounding boundary. Full review: `docs/cross-platform-port-review-codex.md`.

### Test harness

Existing pieces: f64 CLI build, `SPEKTRAFILM_BACKEND=cpu`, the CLI's raw f64
dump (`spektrafilm-cli/src/main.rs:376`), stage dumps via
`SPEKTRAFILM_DUMP_FILM_LOG_RAW` / `_FILM_DENSITY` / `_PRINT_DENSITY` in
`spektrafilm-core/src/pipeline.rs`, `scripts/parity/rs_bisect.rs`, and the
synthetic inputs from the B&W validation (ramp16, midgray, gradient4,
gamut-stress4, adversarial32).

Done (2026-10-22): `scripts/golden/` with tolerance gates, GPU coverage,
stage dumps and probes; review fixes applied. Decision: tight numeric delta,
GPU path first. arm64 vs x86_64 passes everywhere; first divergence is Apple's
`pow` in the film sensitivity. Details and gates: `scripts/golden/README.md`.

Windows (2026-10-22, GTX 1660 SUPER on Vulkan): f64 and the GPU without grain
pass against the Mac after fixing auto-exposure's f32 libm calls and CRLF data;
the GPU with grain passes a statistical gate (`gpu_grain`); f32 CPU with grain
diverges (shared random stream). Two Windows runs are bit-identical.

Decided 2026-10-22 (Noah): keep the auto-exposure fix, although about 0.6 % of
auto-exposed CPU exports on macOS move by one ulp of exposure; accept GPU grain
differences across vendors, gated statistically by `gpu_grain`.
The original plan follows.

1. A `golden` script: fixed matrix of real photos (float EXR/TIFF and RAW via
   rawler) + synthetic inputs × several stocks × effects (grain, halation,
   borders). Writes a manifest of SHA-256 for every final f64 buffer and every
   stage dump. `--locked`, same pinned toolchain (CI uses 1.93.1).
2. Stage dumps after every stage, so the first divergent stage is obvious.
3. GPU variant of the matrix compared by ΔE against the f64 engine and the Mac
   GPU, not by hash.

Workflow with the Mac and the target machine:
1. On the Mac, make the math portable, show the diff in macOS output, and
   regenerate the reference manifest.
2. On the target, run the matrix and diff manifests; bisect inside the first
   divergent stage.
3. Run both in CI so it doesn't drift.

Cheap extra coverage: Linux arm64 in Docker/OrbStack on the Mac (glibc vs Apple
libm), GitHub Ubuntu runners (x86_64/AVX), the Rosetta x86_64 slice already in
CI, lavapipe as a second Vulkan implementation, and an occasional rented NVIDIA
GPU.

## Open decisions

- f32 CPU grain across platforms: accept a different grain pattern, or give
  rows/tiles their own random streams (changes macOS f32 CPU grain)? Deferred.
- Windows UI: Win32 with standard common controls is selected; the startup
  spike result and go-ahead are still required before implementing the app.
- Keep or drop the CUDA backend (recommendation: drop, wgpu only).
- Capacity fallback and watchdog recovery on Windows (measured failures above).
