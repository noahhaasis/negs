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

## Pending

Immutable full numerical baseline, quality and historical-reference comparison;
Linux GPU memory admission/fallback; full-resolution/RAW validation; GTK helper
integration, exports, cache/session/error/shutdown behavior; production package;
and final real-desktop functional/startup/performance checks.
