# Cross-platform port: Linux, Omarchy, Windows

Notes from the portability review on 2026-10-22, based on `main` at a5c360f
(Release 0.7.0). Nothing here was built on Linux or Windows yet; only the macOS
targets are installed on the Mac.

Plan (updated 2026-10-22): Windows first, then Linux and Omarchy (Arch +
Hyprland). Each platform gets a native UI. Launch speed is a hard requirement;
dark mode on Windows is not. Same input must give the same output.

## Where the code stands

| Area | macOS today | Elsewhere today |
|---|---|---|
| UI | Swift app (`macos/`), uses Core Image in `Original.swift` | egui GUI builds but is unmaintained |
| Engine process | `spektrafilm-native`, JSON lines over stdin/stdout, previews written to a path | Same binary; portable as is |
| GPU | wgpu, `Backends::PRIMARY`, all 28 shaders WGSL | Vulkan on Linux, DX12 or Vulkan on Windows; falls back to CPU if no adapter |
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
4. **DX12 shader compiles.** wgpu 24 uses FXC on DX12 unless DXC is configured
   and shipped. Prefer Vulkan on Windows, or bundle DXC.
5. **Adapter choice and testing.** `HighPerformance` behaves differently on
   hybrid laptops. CI has no GPUs; WARP (Windows) and lavapipe (Linux) are CPU
   implementations that are enough to test correctness.
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
- Windows: NVIDIA, AMD and Intel drivers include Vulkan; DX12 is always there.

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

- Accept a one-time last-bit change in macOS exports for cross-platform
  determinism?
- Windows UI: WinUI 3 or Win32 + custom drawing (after the startup spike).
- Keep or drop the CUDA backend (recommendation: drop, wgpu only).
- Which GPU is in the target machine (NVIDIA proprietary vs Mesa).
