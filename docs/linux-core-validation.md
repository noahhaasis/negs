# Linux engine and integration validation

This report records actual Linux bring-up. UI-only evidence is separate in
[linux-ui-validation.md](linux-ui-validation.md). The connected editor and package work continue below; historical bring-up
measurements retain their original scope.

## Toolchain and Linux BLAS bootstrap

Rust 1.93.1, locked root workspace, Omarchy 4.0.6, Linux 7.2.8-9-omarchy,
Ryzen 5 3600 / 16 GiB, GTX 1660 SUPER / 6 GiB, NVIDIA 615.78.08.
The untouched f32 engine and golden tools built in 4m34s. OpenBLAS's source
build automatically used `NOFORTRAN=1` on this machine and created a ZEN-tuned
shared library inside `target/release/build`. The executables would depend on
that private build directory's library unless its location were supplied.

A Linux-only `openblas-src/system` feature now uses the distro OpenBLAS.
No portable numerical function or summation algorithm changed. Root lockfile
and macOS/Windows dependency selection remain unchanged. Arch packages will
declare `openblas` as a runtime dependency.

`sudo -n pacman -S --needed --noconfirm openblas` requires a password here.
Development continues with the official `openblas-0.3.34-1-x86_64` package
unpacked under ignored `target/linux-toolchain/sysroot`; libgfortran,
libgomp and libgcc are already installed. Package SHA-256:
`854099273915eadb8d5e94e88104c4a14fe26a275ecd71d4590cdbc8545765a3`.
Source `linux/core-env.sh` to select the local Rust/OpenBLAS paths when the
system packages are unavailable. Its relocated pkg-config file affects
OpenBLAS only; GTK continues using system include/library paths.

Separate locked builds succeeded for f32 CLI/golden tools, f32 native helper,
and the f64 Reference CLI. Logs:
`target/linux-validation/{core-build,core-system-build}.log`.
Existing warnings concern an unused Linux RAW helper and a Mac-only mutable
workgroup variable; they have not been silenced or changed for this build.

## First helper checks

Both of these pass on the actual machine:

```sh
source linux/core-env.sh
python3 -I scripts/check_native_macos.py --expected-backend wgpu
SPEKTRAFILM_BACKEND=cpu python3 -I scripts/check_native_macos.py --expected-backend cpu
```

Each completes 21 requests, loads 33 profiles, checks real LUT output,
retained render IDs, crop/straighten/rotation, error recovery, source generation,
Reference export, byte-identical retained-preview/Fast export with grain,
and EOF shutdown. Reported backends are respectively `wgpu (GPU)` and
`CPU (rayon)`. Logs: `core-helper-gpu.log`, `core-helper-cpu.log` under
`target/linux-validation/`.

The existing golden-harness unit suite passes 22 tests. Three initial Linux
client tests pass: invalid RGBA dimensions/payloads are rejected, hard-link
source aliases are protected, and the serial client recovers after EOF,
incorrect response IDs and timeouts. These were first compiled independently
while another agent still owned the GTK sources; rerun through Cargo after
the module is integrated.

## Immutable baseline and quality

At bootstrap commit `eed0a98`, copied all six engine/helper/comparison binaries
and the exact OpenBLAS shared library into
`target/linux-validation/baseline-bin/`. Their hashes/toolchain are in
`baseline-provenance.json`. All 62 transferred files match `transfer.json`;
see `input-integrity.json`. The numerical implementation was unchanged.

The full baseline completed **184 cases / 484 renders, zero render failures**
(116 f32 CPU, 184 f64, 184 GPU), using `--jobs 3 --threads 4`. Manifest and
dumps: `target/linux-validation/linux-baseline.json`,
`target/golden/dumps/linux-baseline/`. Preserve them unchanged.

GPU versus f64 deterministic quality: **76/76 pass** under the existing gates.
Worst ΔE00 max is 0.005353; worst 8-bit code difference is 1. Report/log:
`target/linux-validation/linux-quality.{json,log}`.

Historical Mac smoke comparison (49 outputs across 19 cases):

| Engine | Reported statuses |
|---|---|
| f64 | 17 PASS, 1 FAIL, 1 FAIL-GRAIN |
| GPU | 17 PASS, 2 FAIL |
| f32 CPU | 9 PASS, 2 FAIL-GRAIN |

All f64/GPU failures are the two Velvia cases whose historical reference
predates the documented DIR-gamma correction. The f32 failures are Velvia
default and noise/Portra/halation_big with grain. These are not labeled PASS;
the latter is consistent with the deferred shared-stream CPU grain issue.
This historical comparison does not prove same-revision cross-platform parity.
Report: `target/linux-validation/mac-historical-vs-linux.{json,log}`.

## Linux GPU admission and first large render

The existing Windows Vulkan capacity guard now also applies on Linux:
binding/allocation limits, conservative device-local memory estimate with a
20% reserve, and padded FFT buffers. It routes rejected work through the
whole CPU pipeline. Software Vulkan adapters are rejected on Linux so a
lavapipe CPU adapter cannot be reported as hardware acceleration.
Metal behavior and Windows admission calculations are unchanged.

Three capacity unit tests and the whole-pipeline CPU fallback/rescale test
pass. The final helper also passes 21 requests each on real GPU, explicit CPU,
and forced zero-budget whole-pipeline CPU fallback. Logs: `core-helper-final-{gpu,cpu,fallback}.log`.

First bounded 24 MP hardware smoke: two processes, three in-process iterations
each, no diagnostic raw dump. All six GPU-resident pipelines complete and both
TIFF hashes match. Pipelines: 1111/938/938 ms and 1066/974/976 ms; wall time
including load/save is 3.68/3.87 s. Whole-device memory samples peak at
1741/1742 MiB from a 580 MiB baseline. CPU compilation was running concurrently,
so these are initial functionality/memory observations, not final quiet-machine
performance measurements. The benchmark's watchdog-events query is Windows-only
and returns null on Linux; no Linux device-reset claim follows from that field.
Evidence: `target/linux-validation/core-full-resolution/` and its log.

## Final numerical and RAW checks (2026-10-26)

The final matrix repeats all **184 cases / 484 renders**, with three jobs and
four Rayon threads. Zero render failures. Exact comparison against the immutable
Linux baseline: **184/184 GPU, 184/184 f64, 116/116 f32 CPU IDENTICAL**.
Artifacts: `target/linux-validation/linux-{final,exact}.{json,log}`.
The initial attempt lacked device access inside the sandbox; the reported final
run uses the real NVIDIA device. The golden runner now creates its successful
GPU-log directory, so the documented fresh-directory command works.

The separate quality report remains **76/76 PASS** (worst ΔE00 max 0.005353,
maximum 8-bit error 1). Historical Mac mismatch counts above remain unchanged;
they are not converted into passes by the same-machine exact result.

`decode_raw_gui` decodes the real `DSC_4990.NEF` to 6048×4024 float TIFF.
SHA-256 `d3aa7348d100d9d0545c9b35a46469e7d167357a3f975e67ade25cd60e98516d`
is exactly the transferred rawler-decoded TIFF's SHA-256. Inputs stay unchanged.
Artifacts: `raw-decode.log`, `inputs/linux-raw-decode.tif`.

## Connected GTK behavior

The real Wayland/NVIDIA GTK audit passes **11 checks**, including LUT without a
photo, native exposure control changing pixels, stale Save disablement,
Original preserving the retained render, cropped/straightened/rotated keyline
Save Preview, byte-identical independent Fast output, separate f64 Reference,
actual JPEG quality, cancellation preserving the previous destination and
removing staging, real NEF source dimensions, custom RAW WB changing pixels,
and Nikon source metadata in JPEG. Nonzero Post contrast catches operation-order
errors around straightening. Evidence: `engine-ui-final/{result.json,startup.json,app.log}`
and `engine-ui-final.log` under `target/linux-validation/`.

The Linux client opts into Post-before-crop for retained saves. The helper's new
optional fields preserve legacy save defaults and operation order for existing
macOS/Windows clients; capabilities detect mismatched installations. Default
quality and metadata behavior remain byte-identical in IO tests.

The GTK-side Rust suite passes **17 tests**, including source hard-link protection,
serial EOF/bad-ID/timeout recovery, running exporter cancellation/reaping, queued
helper cancellation without execution, RGBA validation, lossless cache corruption/
identity checks, and live-default reset consistency. Clippy passes with warnings
as errors. IO/native tests and the existing 22-test golden harness are recorded
in `core-final-tests.log` and `golden-harness-tests.log` (one existing IO test is
ignored). Unit tests use a private `XDG_RUNTIME_DIR` under the validation directory;
the sandbox cannot write the real desktop runtime directory.

A restored connected session displayed the cached developed preview at 376 ms
and rebuilt current saveable GPU output at 1077 ms. This is an initial diagnostic
launch, not a final quiet-machine median. Evidence:
`engine-ui-post-cache/{restored.json,restored.png}`. The later cache key also
includes available GPU/driver identity and Vulkan-selection environment.

## Remaining hardware coverage

Same-revision Mac output has not been transferred. Other GPU vendors, multi-monitor
mixed scaling, calibrated wide-gamut output and driver device-loss recovery remain
unverified. This machine's explicit sRGB Wayland path and NVIDIA Vulkan execution
are the tested configuration. No driver timeouts were altered.
