# Linux engine and integration validation — in progress

This report records actual Linux bring-up. UI-only evidence is separate in
[linux-ui-validation.md](linux-ui-validation.md). The full port is still in
progress; the checks below do not establish completed UI integration or a
production package.

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
pass. Actual forced-budget fallback and final exact matrices remain to run.

First bounded 24 MP hardware smoke: two processes, three in-process iterations
each, no diagnostic raw dump. All six GPU-resident pipelines complete and both
TIFF hashes match. Pipelines: 1111/938/938 ms and 1066/974/976 ms; wall time
including load/save is 3.68/3.87 s. Whole-device memory samples peak at
1741/1742 MiB from a 580 MiB baseline. CPU compilation was running concurrently,
so these are initial functionality/memory observations, not final quiet-machine
performance measurements. The benchmark's watchdog-events query is Windows-only
and returns null on Linux; no Linux device-reset claim follows from that field.
Evidence: `target/linux-validation/core-full-resolution/` and its log.

## Pending

Final exact baseline comparison; forced GPU memory fallback and Linux device-log
checks; full-resolution/RAW validation; GTK helper
integration, exports, cache/session/error/shutdown behavior; production package;
and final real-desktop functional/startup/performance checks.
