# Handoff: the web stock previews' memory on the iPhone

**For:** the next agent, with the iPhone reconnected by USB. **Branch:** `web-prep-port`
(worktree `.claude/worktrees/web-preview-size`), WIP commits; don't push.
**Background:** [web Prep port](web-prep-port-plan.md#built-2026-11-04) (what was built and
measured), [stock previews](stock-previews.md#web-branch-web-prep-port) (the feature),
[Apple device testing](apple-device-testing.md) (device rules).

## Result (2026-11-04)

Measured; see [Where the memory goes](web-prep-port-plan.md#built-2026-11-04). The
+150–180 MiB was the spike while the grids fill. Filled grids hold +88–114 MiB settled, 23
of it a first calibration any stock change pays. Neither an export (≤ 903 MiB) nor opening
another photograph with the grids open (1241 MiB) comes near the kill. Noah chose on
by default, off after a memory crash (Settings sheet; see the plan). ~40–50 MiB is still
unattributed. The rest of this note is the
brief as it was.


The film and paper previews (Stock › Film and Paper foldouts, `web/stocks.js`, `pumpPreviews`
in `web/app.js`, `Engine.thumbnail` in `crates/spektrafilm-wasm`) pass the speed gates on the
iPhone 13 Pro but add **150–180 MiB** to the tab's footprint with both grids filled (≈650–685
MiB vs ≈490–530 MiB with the previews off, same steps). The plan's gate is **+100 MiB**. So
they're off by default on touch screens for now (`previewsOn` in `app.js`).

Find where the memory goes, cut it below the gate if it can be done without changing any
pixels, and then measure an **export with both grids open**: the export peak at the 4.4 MP
touch cap was 1.33 GiB without previews, and the tab gets killed from about 1.4–1.6 GiB. Then
report to Noah whether the previews can be on by default on phones (decision 5 in the plan).

## What is known

| Run (iPhone, Fuji RAF, fitted 1200 px) | WebContent footprint | wasm memory |
|---|---|---|
| Previews off | ≈490–530 MiB | 124 → 147 MiB |
| Previews on, 40 cached pipelines | ≈650–685 MiB | 124 → 170–193 MiB |
| Previews on, 4 cached pipelines | ≈650–700 MiB | 130 → 153 MiB |

- The pipeline cache isn't it: 4 pipelines instead of 40 left the footprint where it was.
- wasm grows 46–69 MiB and never shrinks (wasm memory doesn't); calibrating stocks needs
  transient room even with 4 cached.
- The run without painting tiles (to separate out the canvases) lost its samples when the
  phone dropped off USB. Redo it first.

Suspects, and how to test each:

1. **Small GPU buffer sets.** `ImagePool::put` in `crates/spektrafilm-gpu/src/wgpu_backend.rs`
   now keeps up to three sets of ≤ 512² px beside the two frame sizes, so tiles and the
   brightness match don't evict a frame's buffers. A 320 px tile's set holds several f32
   images (~10 MB?). Try 1 slot, or 0 (the old behaviour), and measure what eviction costs
   (the first frame after the tiles).
2. **The 33 tile canvases** (2D, ~320×213 each, `Stocks.paint`). Skip painting for one run
   (a temporary knob, as the earlier `?previewPaint=0` was), or paint into ImageBitmaps.
3. **Garbage from the tiles' bytes.** Every tile's RGBA crosses from wasm to the worker's
   JS heap, is transferred to the page and wrapped as ImageData (~270 KB each, ~110 tiles per
   run). JSC may hold it until a GC. Check the footprint a minute after the last tile, and
   compare with the bytes reused.
4. **GPU uploads per stock.** The tc LUT is cached once (`tc_lut_cache`), so every film's
   tile uploads its own and releases the last. Does Safari give released buffers back
   promptly? Also check the GPU process's footprint (it hardly moved: 75 → 90 MiB).
5. **wasm growth.** Measure what calibration allocates (`memoryMiB` per tile is in
   `negs.previews().stats`).

Rules: keep exports and previews **pixel-identical** (no approximations for memory or
speed). A tile must still equal the interactive render of its edit
(`web/checks/stocks.test.mjs`).

## Setup

1. iPhone by USB, unlocked, Safari › Advanced › Web Inspector and Remote Automation on.
   `xcrun devicectl list devices` should show it; `target/ipad-diagnostics/bin/python -m
   pymobiledevice3 usbmux list` must list it too (the footprint sampler needs USB).
2. Build: `web/build.sh --threads`, with `~/.cargo/bin` (wasm-bindgen 0.2.121) and
   `~/.nix-profile/bin` (cargo, rustup) on the PATH.
3. Serve over HTTPS on the LAN (threads need cross-origin isolation, and the phone a
   secure context): `NEGS_DEV_CERTS=~/.local/share/negs-dev-certs node web/serve.mjs 8443
   --https`. The keys stay outside `web/`; never copy or print them. The phone trusts the
   CA already. The Mac's LAN address was `192.168.178.88`.
4. `safaridriver --port 18138` (leave it running).
5. Footprint sampler, in the background:
   `target/ipad-diagnostics/bin/python web/checks/footprint.py 00008110-000240403AD1801E
   target/iphone-preview/footprint.jsonl 500`. It lags 20–30 s behind and dies when the
   device connection drops (`ConnectionTerminatedError`, `DeviceNotFoundError`): check its
   last line's time before reading a run, and restart it after a crash.
6. The test RAF: `web/results/fuji.raf` is a symlink to `~/Downloads/0955097116.raf`
   (Fuji, 6240×4160; `web/results/` is ignored, so it's local only).

## A run

```sh
export SAFARI_DEVICE_TYPE=iPhone SAFARI_DEVICE_UDID=00008110-000240403AD1801E \
  SAFARI_TEST_DIR=target/iphone-preview SAFARI_SCREENSHOTS=0
rm -f target/iphone-preview/session.json web/results/device-iphone-stocks.jsonl
node web/safari.mjs create
node web/safari.mjs navigate "https://192.168.178.88:8443/"            # or …/?previews=off
node web/safari.mjs run web/checks/stocks-phone.mjs
node web/safari.mjs end
python3 web/checks/footprint_by_step.py web/results/device-iphone-stocks.jsonl \
  target/iphone-preview/footprint.jsonl
```

Each run is a fresh tab (`create` … `end`). The runner sets the previews on or off itself
(the setting is remembered per browser). Copy each run's `device-iphone-stocks.jsonl`
aside: the next run deletes it. In a worktree session, put loops and `$VAR`s in a script
file rather than the command line.

## When done

Write the numbers into [the plan's Built section](web-prep-port-plan.md#built-2026-11-04)
and [stock previews](stock-previews.md#web-branch-web-prep-port), adjust the touch default
if the gate passes, and commit as WIP on `web-prep-port` only if Noah asks. The rest of what's
left on the branch is in [Left to do](web-prep-port-plan.md#left-to-do).
