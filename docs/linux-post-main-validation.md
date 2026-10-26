# Linux validation after the main merge

2026-10-26, Linux branch. Product source includes engine main `1b6c99e`, Linux
UI changes through `0ecd157`, compatibility/pipeline commit `a1e0423`, and
Vulkan correction `b253c81`. Validation files and private photographs remain
under ignored `target/`.

## What changed

The experimental engine and macOS/Windows controls had landed on main. The
separate GTK client still bound removed grain fields and failed its live
helper schema check. Linux now binds the current scalar grain controls and
uses the Arctic Beta identifier. Per-stock grain arrays, film/print bases,
camera colour filters and Balance Print remain separate Linux UI additions;
unexposed parameters keep the engine's defaults.

Numerical validation found a second, independent problem: the first fresh
matrix completed 484 renders, but only 1/76 GPU-versus-f64 quality comparisons
passed. The worst difference was Delta E00 37.16, and 74/76 maximum-error
coordinates were on the first row or column. A repeat was 484/484 identical,
so these were deterministic errors, not unstable output.

The existing independent GPU blur-border test reproduced a 1.324 maximum
absolute error at sigma 0.75. Both FIR shaders now fold negative reflected
coordinates before taking an unsigned remainder. This preserves the intended
edge-repeating reflection and avoids signed remainder on negative GPU
coordinates. The existing test passes all 45 size/sigma/channel combinations,
including one-pixel dimensions and kernels wider than the image; its worst
absolute error is approximately 1.3e-6. All 19 GPU tests pass; the performance
benchmark remains explicitly ignored. No numerical tolerance was changed.

Initial failing evidence is retained in
`target/linux-validation/post-main/{baseline,repeat,quality,exact}.{json,log}`
and `target/golden/dumps/linux-post-main-host-baseline/` plus
`linux-post-main-repeat/`. These initial results must not be counted as passes.

## Environment and numerical inputs

Rust 1.93.1, locked workspaces, Omarchy 4.0.6 / Linux 7.2.8-9-omarchy,
AMD Ryzen 5 3600, NVIDIA GTX 1660 SUPER using hardware Vulkan, Hyprland/Wayland,
one 3440x1440 display at 1.25 scale. Matrix runs use three jobs and four Rayon
threads. All ten transferred TIFF inputs are unchanged.

The current macOS manifest from `13e672e` has exactly the same input, preset,
Cargo.lock and data-tree hashes as this new Linux matrix. Its corresponding
current raw dumps are not present on this machine; the transferred Mac dumps
are from the older engine. Therefore a fresh cross-platform tolerance result
cannot be inferred from the manifest alone. The same-revision Mac/Windows
comparison remains separate from the Linux repeatability and GPU/f64 gates.

## Desktop and RAW checks

The rebuilt GTK app passed all 11 real Wayland integration checks:

- Photo-independent LUT export.
- Native exposure changes pixels and invalidates stale Save.
- Original preserves the retained developed image.
- Cropped, straightened, rotated keyline Save matches expected pixels/geometry.
- Independent Fast export matches the retained preview byte for byte.
- Reference export uses the separate f64 process and retains framing.
- Remembered JPEG quality affects the actual output.
- Cancellation preserves the destination and removes staging files.
- Real NEF development retains the full 6048x4024 source dimensions.
- RAW custom white balance changes pixels and invalidates Original.
- Saved RAW JPEG carries Nikon source metadata.

Evidence: `target/linux-validation/post-main/desktop/{result,startup}.json`.
The run uses an isolated settings profile and the hardware GPU backend.

The current RAW decoder's float TIFF is byte-identical to the transferred
6048x4024 TIFF, SHA-256
`d3aa7348d100d9d0545c9b35a46469e7d167357a3f975e67ade25cd60e98516d`.
Evidence: `post-main/raw-decode.{tif,log}`.

The helper schema gate passes. The initial Linux unit run passed 21/21; three
integration tests are explicitly ignored in the ordinary run. The golden harness passes
22 tests and the release generator passes five tests. Sandbox-only GPU/runtime
access failures were rerun with the actual hardware/session available; they
are not numerical or product failures.

## Final package and gates

The corrected full matrix completes all 184 cases / 484 renders with zero
failures. Hardware GPU-versus-f64 quality passes **76/76** with unchanged gates:
worst Delta E00 max **0.001555**, p99 **0.0002782**, p99.9 **0.0005132**, at most
two 16-bit codes and one 8-bit step. Reports:
`post-main/{fixed,fixed-quality}.{json,log}`; immutable diagnostic dumps:
`target/golden/dumps/linux-post-main-fixed/`.

The corrected repeat is **484/484 byte-identical**: 184 GPU, 184 f64 and 116
f32 CPU. Evidence: `post-main/{fixed-repeat,fixed-exact}.{json,log}`.

The first source-package test run exposed a concurrent scratch-construction
race: a new `lock` file was visible to crash cleanup before its owner acquired
the lock. The file is now created under a pending name and renamed to `lock`
only after acquisition. A 16-thread regression test passes. The final package
is being rebuilt with that independent lifecycle correction. The initial
package failure remains in `post-main/makepkg.log`.

The installed-app audit and longer startup/session measurements will use the
final package. The measurement tool now supports a longer launch lifetime so
a 24 MP image can develop before the test closes the window.

No release or repository package has been published. A clean GitHub CI build
requires uploading this source branch; that approval is pending. Local
`makepkg` validation uses the existing development OpenBLAS/toolchain cache and
does not establish the clean container result.
