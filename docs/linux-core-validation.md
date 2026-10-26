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

The GTK-side Rust suite passes **18 tests**, including source hard-link protection,
serial EOF/bad-ID/timeout recovery, running exporter cancellation/reaping, queued
helper cancellation without execution, persistent helper lifetime across submitting
thread exit, RGBA validation, lossless cache corruption/
identity checks, and live-default reset consistency. Clippy passes with warnings
as errors. IO/native tests and the existing 22-test golden harness are recorded
in `core-final-tests.log` and `golden-harness-tests.log` (one existing IO test is
ignored). Unit tests use a private `XDG_RUNTIME_DIR` under the validation directory;
the sandbox cannot write the real desktop runtime directory.

Earlier cache diagnostic timings are retained under `engine-ui-post-cache/`,
but its developed-image timing is superseded: installed testing exposed an
Original-release focus handler that could mark cached pixels as newly developed.
The fixed handler does nothing when Original is already off, and a separate
`saveable_render_ready_ms` milestone records actual current float output.
Final repeated installed-app results are recorded below. The cache key also
includes available GPU/driver identity and Vulkan-selection environment.

## Remaining hardware coverage

Same-revision Mac output has not been transferred. Other GPU vendors, multi-monitor
mixed scaling, calibrated wide-gamut output and driver device-loss recovery remain
unverified. This machine's explicit sRGB Wayland path and NVIDIA Vulkan execution
are the tested configuration. No driver timeouts were altered.

## Installed desktop and helper lifetime

Normal single-instance startup exposed Linux's thread-specific parent-death
signal semantics: a persistent helper forked from a retiring GIO worker could
be killed even while the GUI process was alive. The helper now forks on a
dedicated thread held for the connection's lifetime; parent-process death
protection remains enabled. A regression test fails before this change and
passes after it (`helper-thread-regression-before.log`, `core-ui-tests.log`).
Independent export children already keep their creating worker alive while
waiting for exit.

The desktop check uses the actual GTK portal process for saving, with target
PID/address verification before keyboard input. In-process audit exports are
separate evidence from this native picker flow.

## Final installed startup and interaction (41ed8c6)

These runs use the **installed** `~/.local/bin/negs`, its own helpers/data/BLAS,
real unlocked Hyprland/Wayland, GskVulkanRenderer on NVIDIA, 1320×900 logical
pixels at scale 1.25, Rust 1.93.1. Builds and golden rendering were stopped.
The first RAW launch has an empty application pipeline-cache directory and no
rendered-preview cache; OS/driver caches were already populated. This is not a
cold OS/driver-cache measurement. Four subsequent launches restore the same
isolated session. The source is the unchanged 24 MP NEF.

| Milestone | Median | Range | Runs |
|---|---:|---:|---:|
| First frame actually presented | 428 ms | 427–451 ms | 5 |
| First injected input received | 503 ms | 453–563 ms | 5 |
| Cached developed preview presented | 624 ms | 611–660 ms | 4 restored |
| Current saveable float render ready | 2.271 s | 2.213–2.621 s | 5 |
| Newly developed frame presented | 2.421 s | 2.344–2.752 s | 5 |

Input delivery after injection ranges 15–117 ms; this includes the startup
workload and compositor/GTK dispatch. These timings supersede the earlier
cached-frame reporting bug. Evidence: `installed-startup-final/{summary.json,lazy-*.json}`.

Three connected interaction runs drive a native exposure adjustment and zoom
through 240 frame-clock updates: 4.099/4.138/4.163 s, approximately 58 GTK updates/s.
The engine presents 66/70/70 interactive proxy frames, separately from those GTK
updates. Callback p95 is 1.423/1.926/1.649 ms. Full saveable pixels arrive
917/967/921 ms after release. These are programmatic native-control updates,
not physical pointer input or a claim of 60 film renders/s.
Evidence: `installed-interaction-final/live-summary.json` and per-launch JSON.

A separate 760×900 light-theme RAW launch works with the collapsed inspector;
artifacts are under `installed-narrow-final/`. The earlier dark/narrow/crop/border
visual checks remain in the UI report. One real display at fractional scale is
covered; mixed-display hardware was unavailable.

## Final 24 MP performance and GPU logs

Quiet installed f32 exporter, three processes with three in-process iterations
each, no raw diagnostics and normal TIFF output enabled:

| Process | Pipeline iterations | Wall time including load/save |
|---|---|---:|
| 1 | 880 / 779 / 782 ms | 3.171 s |
| 2 | 867 / 791 / 786 ms | 3.075 s |
| 3 | 858 / 788 / 780 ms | 3.069 s |

Every run uses the resident hardware GPU path and produces the same TIFF SHA-256,
`bd80cee24e9dc7f9c9d2523a0a8e700eaa2af5947fd053513a796815d4f19d7e`.
Whole-device memory peaks at 1854 MiB from a 691 MiB baseline, sampled at 100 ms;
this includes the desktop and is not process-only VRAM. Report:
`core-performance-quiet/report.json`. The earlier `core-performance-final` run
overlapped an app audit and is superseded for performance claims.

The Linux kernel journal covering validation from 07:00 UTC through the final
benchmark has no matching NVRM/Xid/GPU-reset messages (`kernel-validation-window.log`).
The benchmark's Windows watchdog field is null on Linux and is not used as
Linux device-health evidence. Device-loss recovery remains unfinished.

## Packaging and actual installation

App/package source revision: **41ed8c6**, with the pinned Rust toolchain and
locked workspaces. Later documentation/test-only commits do not change those
product binaries. Full source-archive makepkg build succeeds; incremental final
UI fixes were rebuilt through the same recipe. `--nodeps` was necessary locally
because OpenBLAS is an unpacked official package rather than registered in
pacman. The recipe declares its real GTK/OpenBLAS/Vulkan/Python dependencies;
normal installations retain dependency checking. No root package installation
was performed on this password-protected machine.

- Arch artifact: `target/linux-package/negs-0.7.0-1-x86_64.pkg.tar.zst` (73 MiB).
- Per-user bundle: `target/linux-package/negs-0.7.0-linux-x86_64.tar.gz` (88 MiB).
- Linux source archive: `target/linux-package/negs-0.7.0.tar.gz` (30 MiB).
- Checksums/byte counts: `target/linux-validation/package-provenance.json`.

The source archive contains the Linux client, Rust crates, data, build/validation
scripts, relevant docs and licenses. It excludes transferred photos, caches,
dumps, the untracked machine handoff and unrelated historical review images.
Desktop entry validation succeeds and installed binaries have no development
RPATH. The bundle includes the official OpenBLAS library/license; it uses the
machine's GTK/libadwaita and Fortran/OpenMP runtimes.

The manifest installer passes idempotency, unique/backed-up Hyprland include,
modified-file refusal, and uninstall preservation of modified/unowned files and
settings (`package-install.json`). Actual installation under `~/.local` succeeds.
No global MIME defaults are changed. The per-user entry and GApplication both
forward JPEG/TIFF/PNG opens to one existing window; Ctrl+S saves real pixels
through **xdg-desktop-portal-gtk**; graceful quit saves the session and reaps the
helper (`installed-desktop/result.json`).

Both the installed bundle and extracted final Arch application pass all 11
RAW/preview/export/cancellation integration checks. Arch validation supplies its
declared OpenBLAS dependency from the local official-package sysroot. Checkout
helper fallback is disabled during installed checks. Evidence:
`installed-engine-final/` and `arch-engine-audit/`.

The exporters extracted from the package repeat all 484 golden renders with
zero failures and exact baseline hashes: 184 GPU, 184 f64, 116 f32 CPU.
Final package exporter SHA-256 values match this run's binary provenance.
Evidence: `linux-packaged{,-exact}.{json,log}`.

Focused/inactive compositor captures of the installed **dev.negs.app** preserve
all 209,308 configured border pixels at RGB (35,52,69), plus 10,050 visible
photo samples, exactly. No default-opacity tag remains. The focus probe covers
a different part of the canvas; comparison samples exclude it. Evidence:
`installed-opacity/{result.json,focused.png,inactive.png}`. The initial UI-only
alpha-zero input test is separately documented in the UI report.

The installed editor is left open on DSC_5003.jpg with the earlier review
adjustments and white border restored. The prior empty installed session was
backed up to `target/linux-validation/installed-session-before-review.json`.
Final screenshot: `target/linux-validation/installed-review.png`; startup/backend
and window records use the `installed-review-*` prefix. The process remained
responsive after helper worker-thread retirement.
