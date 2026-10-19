# spektrafilm-rs

![banner](docs/card.jpg)

A Rust port of [andreavolpato/spektrafilm](https://github.com/andreavolpato/spektrafilm) — a spectral simulator for analogue colour film and the print-and-scan chain.

The same spectral integration the original Python project does (RGB → film dye density → enlarger illuminant → print paper → scanner RGB), reimplemented in Rust with a GPU preview and a CPU export targeting numerical parity with a pinned Python reference at f64 precision.

---

## What it does

- **Spectral pipeline.** Hanatos2025 RGB→raw spectral upsampling, full 81-wavelength film/print/scanner spectral integration, density-curve interpolation, halation, DIR couplers, grain (bit-exact numpy `MT19937` port), glare, output CCTF encoding.
- **Live interactive preview** on the GPU (wgpu / Metal on macOS) — slider drags update at 60 fps on a 6 MP working image.
- **Reference-quality export** on the CPU at f64 — deterministic print-chain comparisons against Python main pass within `1e-6`; see [scope and measured results](docs/upstream-sync.md).
- **Decoupled preview + export.** GUI uses f32 GPU for iteration, then shells out to a CLI for the final write: the f64 CPU binary (Reference, the default) or the f32 GPU binary (Fast), which [measures within ΔE00 0.25 of the reference](docs/gpu-vs-cpu-quality.md). The export runs in a worker thread with a cancel button and proper child-process lifecycle.
- **Profiles bundled.** 30+ film and paper profiles in `data/profiles/` — Kodak Gold/Portra/Ektar, Fuji Velvia/Provia, Kodak Endura papers, Fuji Crystal Archive papers.
- **B&W stocks.** Updated upstream Double-X 5222, Tri-X Reversal 7266, and Kodak 2302 print-film calibrations, including development-time families. See [usage, provenance, and parity scope](docs/handoff-bw-stocks.md).

## Build

Requires Rust stable (≥ 1.89) and the standard system toolchain.

```bash
git clone <this-repo> && cd spektrafilm-rs

# Native macOS app (macOS 14+, Xcode / Swift 5.9+)
bash scripts/package_macos_app.sh
open dist/spektrafilm-macos/Spektrafilm.app

# Cross-platform GUI (wgpu preview, eframe; Windows/Linux or legacy macOS)
cargo build --release -p spektrafilm-gui

# f32 CLI — fast batch processor, defaults to GPU backend
cargo build --release -p spektrafilm-cli

# f32 CLI with experimental native CUDA backend option
cargo build --release -p spektrafilm-cli --features spektrafilm-gpu/cuda-backend

# f64 CLI — reference precision (CPU only; WGSL has no f64)
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64

# Parity helper: decodes a RAW exactly like the GUI and writes a float TIFF
# (EXIF orientation applied; pass --no-orient to skip it)
cargo build --release -p spektrafilm-cli --bin decode_raw_gui
```

Build the GUI and f64 companion separately when updating: building only `spektrafilm-gui` does not rebuild the export executable. The GUI auto-detects the f64 binary via `$SPEKTRAFILM_F64_CLI`, then next to its own executable, then `spektrafilm-f64` on `PATH`, then `target/release/spektrafilm-f64`.

Windows builds use the WGSL/wgpu backend by default. The CPU fallback path avoids requiring a system OpenBLAS install on Windows, while macOS and Unix-like targets still use native BLAS providers.

WGSL is the default GPU backend and can be selected explicitly with `SPEKTRAFILM_BACKEND=wgpu`. An experimental native CUDA backend can be built with `--features spektrafilm-gpu/cuda-backend` and selected with `SPEKTRAFILM_BACKEND=cuda`. It uses CUDA 12 driver/NVRTC bindings through dynamic loading, so the NVIDIA driver and NVRTC runtime DLLs must be available. Set `SPEKTRAFILM_CUDA_DEVICE=1` (or another zero-based index) to pick a non-default CUDA device. Both GPU paths have a resident preview implementation: front pass, highlight boost, camera diffusion, camera lens blur, halation, DIR couplers, grain, density curves, enlarger diffusion, print/scan spectral reductions, glare, output gamut compression, scanner lens blur, unsharp, and one readback.

## Usage

### GUI

The macOS package uses a native **SwiftUI and AppKit interface** with a standard
toolbar, adjustment inspector, menus, undo/redo, native file dialogs, and
color-managed image display. Pinch to zoom, scroll or drag to pan, and double-click
to fit. Use **⌘O** to open, **⌘S** to save the preview, **⇧⌘E** to export,
**⌘R** to rotate, **⌘0 / ⌘1** for fit / actual size, and **⌥⌘I** to toggle adjustments.
Export remains cancellable and uses the original image at the selected working
resolution. The export panel's **Engine** pop-up chooses **Reference (CPU,
64-bit)**, the default, or **Fast (GPU)**, which renders like the preview and is
several times quicker (Q3 60 MP: about 6 s instead of 33 s). Fast matches the
reference within ΔE00 0.25 at default settings, at most one 8-bit step; grain is
statistically the same but drawn from different random numbers. The choice is
remembered. If no GPU is available, Fast falls back to the 32-bit CPU engine and
the status bar says so. All existing adjustment groups are available in the
inspector.

See the [native UI plan, architecture, and development guide](docs/native-macos-ui.md).
The cross-platform egui UI is **unmaintained**: it still builds and is the only
UI on Windows and Linux for now, but it gets no new work or fixes. Development
focuses on the macOS app, which will later be ported to other platforms. It
runs with:

```bash
./target/release/spektrafilm-gui [optional/path/to/image.orf]
```

- **Open…** — load a TIFF, PNG, or camera RAW (DNG/CR2/CR3/NEF/ARW/ORF/RW2/etc., decoded via [`rawler`](https://github.com/dnglab/dnglab)).
- **RAW white balance** — As shot (unchanged default), Daylight, Tungsten, or Custom temperature/tint. Edits re-develop the RAW for preview and final export. Disabled for non-RAW inputs; independent of Scanner → White correction. See [controls and decoder differences](docs/raw-white-balance.md).
- **Sliders** — exposure, film format, halation, DIR couplers, grain, glare, scanner, enlarger, output. All live-updating against the GPU preview.
- **Profiles** — film stock and print paper combo boxes; picking a film auto-selects its paired paper (`target_print` in the profile).
- **Zoom** — scroll wheel or trackpad pinch over the preview (cursor-anchored), click-drag to pan, double-click to reset.
- **Export…** — re-runs the pipeline at full resolution and writes a PNG/TIFF/JPEG. The engine box next to the format picks **Reference (CPU, 64-bit)** (default; `spektrafilm-f64` with the enlarger and scanner LUTs on) or **Fast (GPU)** (`spektrafilm` on wgpu, like the preview; located via `$SPEKTRAFILM_F32_CLI`, next to the GUI, on `PATH`, or `target/release`; not bundled in the embedded Windows build). Status bar shows elapsed time and reports a Fast export that fell back to the CPU; **Cancel** kills the child cleanly. Closing the GUI mid-export also kills the child (no orphans).
- **Serial grain for export (less RAM)** — optional checkbox under Output, off by default. Processes grain channels one at a time during CPU export: less peak memory, identical output, but slower. Previews keep parallel grain. CLI users can set `settings.serial_grain: true` in their params JSON.
- **Save…** — write the f32 GPU preview directly (instant, less precise).
- **Limit preview to 12 MP** — optional checkbox under Output, off by default. Caps the image before preview processing to reduce memory and rendering time while preserving aspect ratio. Smaller images are not enlarged. Export still uses the Working resolution setting (full resolution at 1.0); Save writes the reduced preview. Disable the cap to inspect fine grain and detail at full working resolution. The full decoded source remains in memory, and RAW loading and full-resolution export peaks are unaffected.

### CLI

```bash
# f32 GPU (fast)
./target/release/spektrafilm process input.ORF -o out.png \
    --film kodak_gold_200 --paper kodak_portra_endura --data-dir data

# f32 native CUDA, when built with spektrafilm-gpu/cuda-backend
SPEKTRAFILM_BACKEND=cuda \
    ./target/release/spektrafilm process input.ORF -o out.png \
    --film kodak_gold_200 --paper kodak_portra_endura --data-dir data

# f64 CPU (reference)
SPEKTRAFILM_BACKEND=cpu \
    ./target/release/spektrafilm-f64 process input.ORF -o out.png \
    --film kodak_gold_200 --paper kodak_portra_endura --data-dir data

# Override any params via JSON (matches RuntimeParams struct)
... --params my_params.json

# Opt into pinned experimental 28bf883 gamut settings
... --params data/presets/experimental-gamut.json

# RAW input WB (also available via io.raw_white_balance in --params)
./target/release/spektrafilm process input.ORF -o out.png \
    --film kodak_gold_200 --raw-wb custom --raw-temperature 3200 --raw-tint 0.85

# List available film + paper profiles
./target/release/spektrafilm list-profiles --data-dir data
```

## Parity

The optional [experimental gamut preset](docs/handoff-experimental-gamut.md)
adds the smooth input boundary and D65 compression center. All 24 tested
B&W cases pass against pinned Python `28bf883`, including saturated inputs,
with maximum final RGB error `5.313e-10`. The existing runtime defaults are
preserved; colour-profile normalization remains a separate port.

The [repeatable parity runner](scripts/parity/README.md) checks intermediate f64 arrays and final RGB against an explicitly selected Python checkout. The [October 2026 audit](docs/upstream-sync.md) passes all 12 tested cases against upstream main `3bb2c2d`, with maximum final linear RGB error `3.631e-9`. The sibling Python checkout is on experimental `28bf883`, which changes the model and fails all six tested Portra/Endura cases. Version `0.3.4` alone does not identify a stable reference.

Historical measurements from the initial port against Python v0.3.2 (bare chain, no stochastic FX) follow. They are numerical comparisons, not byte identity, and were not reproduced on the original image in the current audit:

| Stage | Max diff | Mean diff | Identical pixels |
|---|---|---|---|
| `log_raw` (post-Hanatos + log10) | **8.9 × 10⁻¹⁵** | 3.4 × 10⁻¹⁶ | — |
| Film density CMY | **1.1 × 10⁻¹⁵** | 1.7 × 10⁻¹⁶ | — |
| Print density CMY | **3.1 × 10⁻¹⁵** | 3.0 × 10⁻¹⁶ | — |
| Final PNG (8-bit) | **1 / 255** | 0.00004 / 255 | **99.9962 %** |

With grain on, the binomial sampler's rejection step is sensitive to upstream ULP shifts and the rendered grain texture diverges per pixel — this is by design (matches numpy's behaviour) and produces the same average tone with a different grain pattern.

The LUT path (`use_enlarger_lut` + `use_scanner_lut`, the typical export config) uses a bit-exact port of Python's PCHIP 3D interpolation (`crates/spektrafilm-math/src/pchip3d.rs` ↔ `spektrafilm/utils/fast_interp_lut.py`). Parity numbers above hold for both the LUT and non-LUT paths.

## Performance

The [Leica Q3 memory investigation](docs/memory-study-q3.md) measures the current
memory reductions and optional serial grain. The [memory optimization handoff](docs/handoff-memory.md)
records the agreed next steps: GPU scratch reuse, then CPU buffer ownership/reuse,
while retaining full-resolution previews.

f64 CPU export on a 16 MP Olympus ORF (kodak_gold_200 → kodak_portra_endura, full FX):

| | Wall time | Notes |
|---|---|---|
| Python reference (numpy + numba) | 22 s | LUT enabled, default config |
| **spektrafilm-rs (f64 CPU)** | **14 s** | **35 % faster than Python** |

What gets it there:

- **PCHIP LUTs** for the spectral integrations (enlarger + scanner) — same approximation Python uses, same accuracy budget.
- **`vForce vvpow`** for `10^x` on the spectral chain — Accelerate's SIMD pow, bit-identical to libm `pow(10, x)`.
- **Accelerate BLAS dgemm** for the spectral reductions — a single `cblas_dgemm` per contraction, parallelised internally by Accelerate. (It is not safe to call concurrently from multiple threads, so the matmul is never split across rayon.)
- **Parallelised hot per-pixel loops** in the printing and scanning post-stages.

GPU preview path uses wgpu compute shaders (`crates/spektrafilm-shaders/wgsl/`). All shaders are f32 (WGSL has no f64); the GPU path drives the live preview while the export reaches for f64 CPU. End-to-end preview render: ~250 ms at 6 MP, ~700 ms at 16 MP on Apple Silicon.

## Layout

For an interactive, source-linked walkthrough of RAW decoding, stock simulation, printing, scanning, and export, open [the Anatomy explainer](docs/anatomy/index.html) in a browser. Its charts use samples from the bundled profile JSON; regenerate it with `node docs/anatomy/build.mjs` after profile changes.

```
crates/
  spektrafilm-math/    f64 reference math (spectral, interp, PCHIP, RNG, vForce bindings)
  spektrafilm-model/   stochastic + physical models (grain, halation, DIR couplers, glare)
  spektrafilm-core/    pipeline orchestration, profiles, stage definitions
  spektrafilm-gpu/     ComputeBackend trait + CPU (rayon + BLAS) and wgpu backends
  spektrafilm-shaders/ WGSL / Metal / CUDA compute shaders
  spektrafilm-io/      image + camera RAW loading (rawler) and PNG/JPEG/TIFF saving
                       with the source's camera EXIF, shared by the CLI and GUI
  spektrafilm-cli/     `spektrafilm` (process, list-profiles, gen-lut) + `decode_raw_gui`
  spektrafilm-gui/     egui/eframe preview, unmaintained (wgpu renderer, Metal-backed on macOS)
  spektrafilm-native/  persistent image-engine helper for the native macOS app
macos/                SwiftUI interface, AppKit image canvas, and Swift tests
data/
  profiles/            film + paper JSON profiles (spectral sensitivities, density curves, etc.)
  luts/                Hanatos2025 spectral basis + standard observer CMFs (.npy)
  filters/             neutral-print enlarger filter database
scripts/parity/        Python ↔ Rust comparison harness (spektra_compare.py, etc.)
docs/upstream-sync.md  verified Python references, parity results, porting backlog
```

## Credits

Original Python implementation by Andrea Volpato — [andreavolpato/spektrafilm](https://github.com/andreavolpato/spektrafilm). All spectral data, film/paper profiles, and pipeline architecture come from there. This port owes its existence to Andrea and its work.

The spectral-upsampling LUT (`hanatos2025_*`) is named after [Johannes Hanatos](https://github.com/hanatos), author of [vkdt](https://github.com/hanatos/vkdt), who provided the upstream Python project with the LUT files and sample code that drive the RGB → spectrum step.

PCHIP 3D LUT, MT19937 binomial sampler, and CIE 1931 observer constants are ported from numpy / scipy / scikit-image / colour-science.

Claude code for being this awesome.

## License

GPL-3.0, matching the upstream Python project. See [LICENSE](LICENSE) for the full text.
