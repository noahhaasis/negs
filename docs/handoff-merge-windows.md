# Handoff: validate the web-app merge on Windows

Prepared 2026-10-31 for an agent on Noah's Windows test box (Windows 10, Ryzen 5 3600,
GTX 1660 SUPER on Vulkan; the desktop session, checkout `C:\Users\Noah\Projects\negs`).
Linux has its own handoff, [handoff-merge-linux.md](handoff-merge-linux.md).

`main` now contains `wasm-investigation`, the browser app: 48 web commits rebased onto
`5ac8200` (Release 0.12.0 + AppStream fix), then "Build the merged engine on every
platform" and "Gate the GPU preflash fix in the golden matrix". Everything below was checked on
the M1 Pro. The Windows app and its tests were only type-checked from the Mac
(`cargo check --target x86_64-pc-windows-msvc --all-targets`). **Nothing has run on
Windows yet.** This handoff runs it there.

## Where to work

- `git -C C:\Users\Noah\Projects\negs status` first and don't disturb uncommitted work.
  If the checkout is busy, use a worktree:
  `git -C C:\Users\Noah\Projects\negs fetch origin` then
  `git -C C:\Users\Noah\Projects\negs worktree add ..\negs-merge-check origin/main`.
- This is validation. Commit only the result files listed under "Record" below, on a branch
  `windows-merge-validation`. **Don't push to main** unless Noah asks. If something fails,
  stop and report it. Don't loosen a gate to make it pass.
- End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## What changed for Windows

1. **The Windows workspace patches rawler.** `spektrafilm-io` now calls
   `RawDevelop::develop_intermediate_owned`, which only exists in the vendored
   `vendor/rawler` (see `vendor/README.md`). `windows/Cargo.toml` has
   `[patch.crates-io] rawler = { path = "../vendor/rawler" }`. Without the patch the app
   didn't compile. The Windows app's own RAW decoding uses the same patched crate. Its
   pixels should be unchanged (the Mac decodes all 6 golden RAWs bit-identically), but check.
2. **Engine changes** (shared through `spektrafilm-native` and the CLIs):
   - **GPU preflash fix** (09a66ad): with a preflash plus print exposure ≠ 1 or scanner
     white correction, the wgpu/CUDA chain added the preflash unscaled. On the Mac,
     main's GPU was off from f64 by up to 14 ΔE00 (print exposure 0.7) and 61 ΔE00
     (white correction, `synth_noise`). Now ≤ 0.0003.
   - The CPU IIR blur keeps coefficients in f64. f32 CPU output moves *towards* f64 on every
     case measured (`dir_wide` 0.03–0.28 → ≤ 0.0017 ΔE00). With grain on, this re-rolls the
     f32 CPU grain, so expect many f32 CPU FAIL-GRAIN cases against old manifests.
   - The enlarger's M/Y shifts and preflash no longer rebuild the preview pipeline.
     The new test `with_params_refilters_the_enlarger_like_a_new_pipeline` covers it.
   - `wgpu_backend.rs` was reworked heavily (pooled buffers, explicit frees, display pass).
     On the Mac, native GPU output is bit-identical to main on all 184 old golden cases.
3. **Golden matrix:** two grain-free presets, `preflash_exposure` and `preflash_white`.
   The matrix is now 188 cases and 496 renders, and `quality` gates the preflash fix on
   every GPU. Old Windows manifests can't be compared to new ones, so make new ones.
4. **Golden inputs changed.** The photo inputs differ from the ones on this machine,
   because main's colour-management work (33bc69d) changed decoding before this merge.
   Use the Mac's new inputs (below), not regenerated or old ones.

## Data you need from the Mac

Ask Noah to copy these from the Mac (`~/Projects/Negs-wasm/target/golden/`) into the
checkout's `target\golden\`:

- `inputs\` (10 float TIFFs, the reference inputs), required.
- `dumps\macos-arm64\` (the M1 Pro reference dumps for `scripts/golden/manifests/macos-arm64.json`,
  made from commit 53186e4, 6 GB). Only needed for step 5.

## Steps

1. **Build and unit tests.**
   ```
   cargo build --release --locked -p spektrafilm-native
   cargo build --release --locked -p spektrafilm-cli --bin spektrafilm --bin golden_compare --bin golden_probes
   cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
   cargo test --release --locked --workspace
   cargo test --locked --manifest-path windows/Cargo.toml
   cargo build --release --locked --manifest-path windows/Cargo.toml
   ```
   On the Mac the workspace gave 373 passed, 0 failed. Windows had 56/56 app tests at dd870f0.
2. **Preflash regression test on the GPU, strict:**
   `$env:SPEKTRAFILM_REQUIRE_GPU=1; cargo test --release --locked -p spektrafilm-core --test preflash_gpu -- --nocapture`.
   It must report three cases with `max |gpu - cpu|` < 1e-4. Without the variable, a
   missing GPU adapter makes it skip instead of fail.
3. **Native bridge:**
   `python -I scripts/check_native_macos.py --expected-backend wgpu` (pass the `.exe`
   paths, see its `--help`), then with `$env:SPEKTRAFILM_BACKEND="cpu"` and
   `--expected-backend cpu`. Then `python -I windows/check_original.py INPUT.NEF` with a Z 6 NEF.
4. **Golden run and same-machine quality**
   ([scripts/golden/README.md](../scripts/golden/README.md)):
   ```
   python -I scripts/golden/run.py run --label windows-gtx1660s-vulkan --inputs target/golden/inputs `
     --out scripts/golden/manifests/windows-gtx1660s-vulkan.json --dump-dir target/golden/dumps/windows-gtx1660s-vulkan
   python -I scripts/golden/run.py quality scripts/golden/manifests/windows-gtx1660s-vulkan.json `
     --dumps target/golden/dumps/windows-gtx1660s-vulkan
   ```
   Expect 496 renders, 0 failed, and **`quality` 80/80 pass**: the 76 old grain-free
   cases plus the 4 new preflash ones. The Mac got 80/80 at ΔE00 max 0.0018. A
   `preflash_*` failure means the GPU fix isn't working on this driver, which is the
   important finding. Run `run` a second time with label `windows-ui` and
   `compare --exact` the two (expect 496/496 identical).
5. **Against the Mac** (needs the Mac dumps):
   ```
   python -I scripts/golden/run.py compare scripts/golden/manifests/macos-arm64.json `
     scripts/golden/manifests/windows-gtx1660s-vulkan.json `
     --dumps-a target/golden/dumps/macos-arm64 --dumps-b target/golden/dumps/windows-gtx1660s-vulkan `
     --report scripts/golden/reports/windows-vs-macos.json
   ```
   Before the merge (dd870f0) the result was: f64 180 PASS + 2 PASS-BORDER + 2 PASS-GRAIN,
   GPU 184 PASS, and f32 CPU with 82 FAIL-GRAIN from the known shared-MT19937 divergence
   (deferred). Expect f64 and GPU to pass the same way, the 4 new cases included. The
   f32 CPU grain count may change, because the f64 IIR shifts the pre-grain values. A
   non-grain f32 CPU failure or any f64/GPU failure is a real finding.
6. **The app.** Install and launch the release build. Open a NEF, move Exposure and the
   enlarger's M/Y shift, set a preflash with Print Exposure 0.7, export with the GPU
   and with the f64 exporter, and compare the two exports by eye (they should match).

## Record

On `windows-merge-validation`: the two new manifests, `reports/windows-vs-macos.json`, the
exact report, and a short "Results on 2026-1x-xx: Windows after the web merge" section
in `scripts/golden/README.md` in the style of the existing ones. Then tell Noah the
results, including anything that failed.
