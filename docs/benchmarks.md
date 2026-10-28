# Benchmarks: Negs, spektrafilm-rs and Python spektrafilm

Measured 28 October 2026. Full-resolution exports of a 24 MP and a 60 MP photograph, timing every engine on the same input, the same machine and the same film. Raw runs and medians: [benchmarks-2026-10.json](benchmarks-2026-10.json).

## Results

Median of three runs, each a fresh process, after one warm-up per configuration. Ranges are min–max. **Wall time** is the whole export as a user waits for it: process start, reading the TIFF, rendering and writing a 16-bit TIFF. **Engine time** is the render alone, as each engine reports it. **Peak memory** is the kernel's lifetime peak physical footprint of the process (`/usr/bin/time -l`); on Apple silicon it includes the GPU's Metal buffers, so CPU and GPU rows compare directly. 1 GiB = 2³⁰ bytes, the unit Activity Monitor shows as "GB".

### Nikon Z 6, 24.5 MP (4040 × 6064)

| Engine | Path | Wall time | Engine time | Peak memory |
|---|---|---:|---:|---:|
| **Negs 0.10** | **GPU (f32, Metal)** | **1.9 s** (1.8–1.9) | **0.9 s** | **2.1 GiB** |
| spektrafilm-rs 0.1.2 | GPU (f32, Metal) | 2.0 s (2.0–2.0) | 1.3 s | 3.6 GiB |
| **Negs 0.10** | **CPU reference (f64)** | **14.1 s** (14.1–14.1) | 13.2 s | **7.1 GiB** (6.7–7.7) |
| Negs 0.10 | CPU reference, *Use less memory* | 38.4 s (38.4–38.4) | 37.5 s | 3.9 GiB |
| Negs 0.10 | CPU (f64, LUTs)² | 9.7 s (9.7–9.8) | 8.8 s | 5.1 GiB (4.5–5.3) |
| spektrafilm-rs 0.1.2 | CPU (f64, LUTs) | 10.6 s (10.5–10.7) | 9.7 s | 7.5 GiB |
| Python spektrafilm | CPU (f64, NumPy + Numba, LUTs) | 46.1 s (46.1–46.1) | 43.4 s | 13.7 GiB |

### Leica Q3, 60.5 MP (9536 × 6344)

| Engine | Path | Wall time | Engine time | Peak memory |
|---|---|---:|---:|---:|
| **Negs 0.10** | **GPU (f32, Metal)** | **4.4 s** (4.4–4.5) | **2.1 s** | **5.0 GiB** |
| spektrafilm-rs 0.1.2 | GPU (f32, Metal) | 6.0 s (5.9–6.0) | 4.1 s | 8.9 GiB |
| **Negs 0.10** | **CPU reference (f64)** | **37.8 s** (37.5–38.0) | 35.3 s | **13.9 GiB** (13.5–14.8) |
| Negs 0.10 | CPU reference, *Use less memory* | 101.5 s (101.4–101.8) | 99.2 s | 9.7 GiB (9.7–10.0) |
| Negs 0.10 | CPU (f64, LUTs)² | 26.4 s (26.4–26.7) | 24.1 s | 10.7 GiB (10.2–11.1) |
| spektrafilm-rs 0.1.2 | CPU (f64, LUTs) | 28.0 s (27.9–30.1) | 25.9 s | 17.2 GiB (17.2–18.5) |
| Python spektrafilm | CPU (f64, NumPy + Numba, LUTs) | 112.8 s (108.5–118.4) | 107.6 s | 27.8 GiB¹ (27.8–28.4) |

¹ Python needed more than the machine could keep in RAM: macOS swapped out 3.8–7.3 GiB to disk during each 60 MP run, so its time includes paging. No other configuration swapped, apart from 0.1 GiB in the Negs CPU run that directly followed a Python run.

² Not how the app exports: Negs with the enlarger and scanner LUTs the other two engines export with, for a like-for-like CPU comparison. Measured right after the main run (same builds, machine and protocol; `negs-cpu-luts` in the results).

## What the numbers say

- **Negs' GPU render is the fastest and lightest path everywhere.** In the app it is the full-resolution frame rendered when a slider is released (while dragging, only the canvas's pixels are rendered); *Fast* export saves that frame, so it only renders like this when the preview can't be reused (e.g. *Limit preview to 12 MP* on, or another colour space). At 60 MP it finishes in 4.4 s with 5.0 GiB, against 6.0 s and 8.9 GiB for spektrafilm-rs: 1.4× faster with 44% less memory. At 24 MP the wall times are within 0.1 s, because process start and file I/O dominate; the render itself is 0.9 s against 1.3 s, and memory is 42% lower.
- **Against Python spektrafilm, Negs' GPU export is 24–26× faster with one sixth of the memory**, and the CPU reference export is 3× faster with half the memory.
- **spektrafilm-rs's CPU export is faster than Negs' CPU reference, but does less work.** Negs' *Reference* export integrates the full spectrum for every pixel; spektrafilm-rs's and Python's GUIs speed up the enlarger and scanner integrations with small 17³ lookup tables over dye density (Python: `spektrafilm_gui/params_mapper.py` turns them on for every render, full-resolution Scan included), an approximation Negs deliberately leaves off. They aren't film-look LUTs: grain, halation and the other effects are still simulated. With the same LUTs, Negs' CPU render takes **8.8 s / 5.1 GiB** at 24 MP and **24.1 s / 10.7 GiB** at 60 MP, ahead of spektrafilm-rs's 9.7 s / 7.5 GiB and 25.9 s / 17.2 GiB.
- **Use less memory** (Output section, `settings.serial_grain`) cuts the CPU reference's peak by 45% at 24 MP (7.1 → 3.9 GiB) and 30% at 60 MP (13.9 → 9.7 GiB), with identical output, but the export takes 2.7× as long. It only affects the CPU reference: the GPU grain shader doesn't use it, and the GPU export already needs less memory than either.

## Setup

- **Machine:** MacBook Pro (MacBookPro18,3), Apple M1 Pro (8 performance + 2 efficiency cores, 16-core GPU), 32 GiB, macOS 26.5.2. On AC power, Low Power Mode off, no other renders running. The 6 configurations × 2 images × 3 runs plus warm-ups took 22 minutes.
- **Engines:**
  - Negs 0.10.0 (`a9126b5`), `spektrafilm-f64` (CPU) and `spektrafilm` (GPU), the two command-line tools the macOS app runs for *Reference* and *Fast* export, with the app's default parameters.
  - [spektrafilm-rs](https://github.com/turbasvin/spektrafilm-rs) v0.1.2 (`9dd59b0`), its own `spektrafilm-f64` and `spektrafilm`. The CPU runs enable enlarger and scanner LUTs, as its GUI does for its f64 export.
  - [Python spektrafilm](https://github.com/andreavolpato/spektrafilm), branch `experimental` at `28bf883` (clean `git archive`), Python 3.13.5, NumPy 2.4.6, Numba 0.65.1. It has no command-line tool, so [a headless driver](../scripts/benchmark/python_spektrafilm.py) builds the parameters from its GUI's default state (enlarger, scanner and convert LUTs on) and runs the full-resolution float64 simulation, as the GUI's Scan does.
  - Both Rust projects built with Rust 1.93.1, `--release` (thin LTO, one codegen unit) and `MACOSX_DEPLOYMENT_TARGET=14.0`.
- **Input:** one RAW per camera, decoded once with Python spektrafilm's own RAW loader (rawpy, as-shot white balance, no lens correction) into a linear ProPhoto RGB 16-bit TIFF. That is the input Python spektrafilm and spektrafilm-rs assume by default; Negs reads it with `--input-colour "ProPhoto RGB" --input-linear`. RAW decoding is therefore excluded; it costs every engine the same.
- **Render:** Kodak Gold 200 printed on Kodak Portra Endura, each engine's default effects (grain, halation, DIR couplers and so on), full resolution, 16-bit sRGB TIFF out.
- **Caveat:** the engines implement different generations of the model. spektrafilm-rs follows Python spektrafilm's `main`; Negs is partway to `experimental`. The renders are the same workload, not identical images.

## Reproduce

```sh
# Build both Rust projects the same way
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --release -p spektrafilm-cli --bin spektrafilm --target-dir target/bench
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64 --target-dir target/bench
# (and the same two commands, without --target-dir, in a spektrafilm-rs v0.1.2 checkout)

# Shared inputs, decoded with Python spektrafilm's RAW loader
python -I scripts/benchmark/prepare_inputs.py PY_SRC target/benchmarks/inputs nikon-z6=PATH.NEF leica-q3=PATH.DNG

python3 scripts/benchmark/run.py --negs-bin target/bench/release --rs-bin RS/target/release \
  --py-src PY_SRC --py-python PY_VENV/bin/python --inputs target/benchmarks/inputs --out target/benchmarks/RUN
python3 scripts/benchmark/summarize.py target/benchmarks/RUN/results.json
```

`PY_SRC` is the `src` directory of a clean Python spektrafilm checkout; `PY_VENV` a virtualenv with its dependencies. `run.py` refuses to start on battery, in Low Power Mode or beside other spektrafilm processes.
