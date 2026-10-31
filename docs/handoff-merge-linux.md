# Handoff: validate the web-app merge on Linux

Prepared 2026-10-31 for an agent on Noah's Omarchy machine (`noah@192.168.178.33`;
Ryzen 5 3600, GTX 1660 SUPER on Vulkan, NVIDIA driver; clone `~/Work/Negs`).
Windows has its own handoff, [handoff-merge-windows.md](handoff-merge-windows.md).

`main` now contains `wasm-investigation`, the browser app: 48 web commits rebased onto
`5ac8200` (Release 0.12.0 + AppStream fix), then "Build the merged engine on every
platform" and "Gate the GPU preflash fix in the golden matrix". Everything below was checked on
the M1 Pro. On the Mac, `linux/flatpak/cargo_sources.py --check` and
`cargo metadata --locked` pass for both workspaces. **No Linux build, package or GPU
run has happened yet.** This handoff runs them.

## Where to work

- `git -C ~/Work/Negs status` first and don't disturb another branch or uncommitted work.
  Use a worktree:
  `git -C ~/Work/Negs fetch origin && git -C ~/Work/Negs worktree add ~/Work/Negs/.worktrees/merge-check origin/main`.
- This is validation. Commit only the result files listed under "Record" below, on a branch
  `linux-merge-validation`. **Don't push to main** unless Noah asks. Ask before anything
  outward-facing. If something fails, stop and report it. Don't loosen a gate.
- End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## What changed for Linux

1. **Vendored rawler.** The root workspace patches crates.io rawler with `vendor/rawler`
   (`[patch.crates-io]`, see `vendor/README.md`). The Flatpak's `dir` source includes
   `vendor/`, and `cargo-sources.json` was regenerated (529 crates, adding `web-time`,
   `wasm-bindgen` and friends). The new crates are built only for `wasm32`, but the
   offline Flatpak build has to vendor them. The GTK workspace (`linux/Cargo.toml`) is
   unchanged. Check that the Flatpak builds offline and that the AppImage and Arch
   package still build.
2. **Engine changes** (shared through `spektrafilm-native` and the CLIs):
   - **GPU preflash fix** (09a66ad): with a preflash plus print exposure ≠ 1 or scanner
     white correction, the wgpu chain added the preflash unscaled. On the Mac, main's
     GPU was off from f64 by up to 14 ΔE00 (print exposure 0.7) and 61 ΔE00 (white
     correction, `synth_noise`). Now ≤ 0.0003.
   - The CPU IIR blur keeps coefficients in f64. f32 CPU output moves *towards* f64 on every
     case measured (`dir_wide` 0.03–0.28 → ≤ 0.0017 ΔE00). With grain on, this re-rolls the
     f32 CPU grain. The Flatpak's CPU check (`SPEKTRAFILM_BACKEND=cpu` in `verify.sh`) runs
     this path.
   - The enlarger's M/Y shifts and preflash no longer rebuild the preview pipeline.
   - `wgpu_backend.rs` was reworked heavily (pooled buffers, explicit frees, display pass).
     On the Mac, native GPU output is bit-identical to main on all 184 old golden cases.
     Vulkan on NVIDIA is untested.
3. **Golden matrix:** two grain-free presets, `preflash_exposure` and `preflash_white`.
   The matrix is now 188 cases and 496 renders, and `quality` gates the preflash fix on
   every GPU. Old Linux manifests can't be compared to new ones, so make new ones.
4. **Golden inputs changed.** The photo inputs differ from the ones on this machine,
   because main's colour-management work (33bc69d) changed decoding before this merge.
   Use the Mac's new inputs (below).

## Data you need from the Mac

Ask Noah to copy these from the Mac (`~/Projects/Negs-wasm/target/golden/`) into the
worktree's `target/golden/`, for example with `rsync -a` from the Mac:

- `inputs/` (10 float TIFFs, the reference inputs), required.
- `dumps/macos-arm64/` (the M1 Pro reference dumps for `scripts/golden/manifests/macos-arm64.json`,
  made from commit 53186e4, 6 GB). Only needed for step 5.

## Steps

1. **Build and tests.** As `linux/build.sh` does, with the machine's environment
   (`source linux/core-env.sh`, system OpenBLAS):
   ```sh
   bash linux/build.sh
   cargo build --release --locked -p spektrafilm-cli --bin spektrafilm --bin golden_compare --bin golden_probes
   cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
   cargo test --release --locked --workspace
   bash linux/cargo.sh test --release --locked
   python3 linux/flatpak/cargo_sources.py --check
   ```
   On the Mac the workspace gave 373 passed, 0 failed.
2. **Preflash regression test on the GPU, strict:**
   `SPEKTRAFILM_REQUIRE_GPU=1 cargo test --release --locked -p spektrafilm-core --test preflash_gpu -- --nocapture`.
   It must report three cases with `max |gpu - cpu|` < 1e-4. Without the variable, a
   missing Vulkan adapter makes it skip instead of fail.
3. **Native bridge and the app:**
   `python3 -I scripts/check_native_macos.py --expected-backend wgpu`, then
   `SPEKTRAFILM_BACKEND=cpu python3 -I scripts/check_native_macos.py --expected-backend cpu`
   (the script is cross-platform despite its name). Then `python3 linux/check_engine.py --raw PATH.NEF`
   in the Wayland session. Also launch the app and move Exposure and the M/Y shift. Set a
   preflash with Print Exposure 0.7 and compare a GPU export with an f64 export.
4. **Golden run and same-machine quality**
   ([scripts/golden/README.md](../scripts/golden/README.md)):
   ```sh
   python3 -I scripts/golden/run.py run --label linux-gtx1660s-vulkan --inputs target/golden/inputs \
     --out scripts/golden/manifests/linux-gtx1660s-vulkan.json --dump-dir target/golden/dumps/linux-gtx1660s-vulkan
   python3 -I scripts/golden/run.py quality scripts/golden/manifests/linux-gtx1660s-vulkan.json \
     --dumps target/golden/dumps/linux-gtx1660s-vulkan
   ```
   Expect 496 renders, 0 failed, and **`quality` 80/80 pass**: the 76 old cases plus the
   4 new preflash ones. The Mac got 80/80 at ΔE00 max 0.0018. A `preflash_*` failure
   means the fix isn't working on this GPU, which is the important finding. Run `run`
   again with label `linux-ui` and `compare --exact` (expect 496/496 identical).
5. **Against the Mac** (needs the Mac dumps):
   ```sh
   python3 -I scripts/golden/run.py compare scripts/golden/manifests/macos-arm64.json \
     scripts/golden/manifests/linux-gtx1660s-vulkan.json \
     --dumps-a target/golden/dumps/macos-arm64 --dumps-b target/golden/dumps/linux-gtx1660s-vulkan \
     --report scripts/golden/reports/linux-vs-macos.json
   ```
   Before the merge: f64 180 PASS + 2 PASS-BORDER + 2 PASS-GRAIN, GPU 184 PASS, and f32 CPU
   with 5 FAIL-GRAIN (deferred shared-MT19937 divergence). Expect f64 and GPU to pass the
   same way, the new cases included. The f32 CPU grain count may change. A non-grain
   failure is a real finding.
6. **Packages:**
   ```sh
   bash linux/flatpak/build.sh target/flatpak/out
   xvfb-run -a bash linux/flatpak/verify.sh target/flatpak/out/negs-*-linux-x86_64.flatpak   # or in the desktop session
   python3 linux/release.py prepare --out target/appimage/release
   bash linux/appimage/package.sh target/appimage/release/assets/negs-VERSION.tar.gz target/appimage/out
   bash linux/appimage/smoke.sh target/appimage/out/Negs-VERSION-x86_64.AppImage ubuntu fedora arch
   bash linux/package.sh
   ```
   See [linux-distribution.md](linux-distribution.md). The Flatpak build is offline: a
   missing crate there means `cargo-sources.json` is wrong. Regenerate it with
   `linux/flatpak/cargo_sources.py` and say so.

## Record

On `linux-merge-validation`: the two new manifests, `reports/linux-vs-macos.json`, the exact
report, and a short "Results on 2026-1x-xx: Linux after the web merge" section in
`scripts/golden/README.md` in the style of the existing ones. Then tell Noah the
results, including anything that failed.
