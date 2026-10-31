# Handoff: the iPhone tab is killed for memory

Date: 2026-10-29. Branch `wasm-investigation`. Read this first, then
`docs/ipad-webgpu-handoff.md` (next-step item 1 has the full history) and
`docs/apple-device-testing.md` (how the devices are driven).

## The problem

On an iPhone 13 Pro (6 GB, iOS 26.6.2, Safari 26.6.1), the editor reloads
while working on a full-size RAW (`web/results/0955097116.raf`, Fuji,
6240 × 4160). Confirmed over USB with the phone's syslog: iOS kills the tab's
WebContent process at its per-process limit.

```
memorystatus: killing_highwater_process pid 523 [com.apple.WebKit.WebContent] (highwater 100 67s rf:- type:app) 2718659KB
WebPageProxy::processDidTerminate: (pid 523), reason=Crash
```

So the limit is about **2.6 GiB** for the whole tab. That covers the WASM
memory, the JS heap, decoded images and bitmaps, and WebGPU allocations that
WebKit charges to the page. No jetsam or crash report is written for these
kills, which is why the AirDropped reports showed nothing. The iPad Air 5
passes the same tests.

## The goal

Make a full-size RAW fit comfortably on the phone. **Everything is on the
table**: a smaller working image or preview instead of the whole image,
fewer or smaller buffers, f16 instead of f32, freeing the decoded source,
tiling, or whatever else works. The environment is constrained. **Don't make
desktop worse**: a device with lots of memory should still use the full
image (full-resolution preview, fast zoom to 100%). Choose by measured or
reported capability, not by user agent. The existing split
`MAX_PIXELS = coarse ? 12e6 : 40e6` in `web/app.js` (around line 731) shows
the pattern, though a pointer type is only a rough proxy for memory.
Exports should still develop every pixel. Developing the export in tiles or
bands is acceptable if a single pass doesn't fit.

## What is measured

| Run (USB, 19:20 to 19:34Z) | Result |
|---|---|
| `card.jpg` (2752 × 1536) smoke | passed, WASM memory 126 MiB |
| RAW smoke, same tab | passed, WASM memory 1044 MiB |
| RAW open, same tab, third navigation | **killed** at 2.6 GiB |
| RAW smoke + extremes, fresh tab each, ×3 | all passed |

- A tab's memory carries over across navigations. The killed process had
  served all three pages. Over Wi-Fi, the test runner reloads the app in an
  iframe on the same page, so it fails more often. In real use, opening a
  second RAW (or reopening one) in the same tab is the same situation.
- WASM linear memory never shrinks. The 1044 MiB is the high-water mark of
  the decode and the first renders, and it stays reserved.
- Earlier Wi-Fi results: CPU backend passed (but drew few frames); WebGPU
  with 1 thread still reloaded; `?present=0` moved the kill from the rapid
  slider edits into the RAW open. All of this fits a footprint limit rather
  than a specific bug.
- Not measured yet: **how much of the 2.6 GiB is WASM, how much is GPU, and
  how much is anything else, per step.** Do this first.

## Measured per step (20:53Z, USB, fresh tab)

Footprint of the tab's WebContent process from
`web/checks/footprint.py` (sysmon over a no-root userspace tunnel; WebKit's
GPU process stays at 4 MB, so WebGPU memory counts against WebContent):

| Step | Tab footprint | WASM memory |
|---|---|---|
| App loaded, idle | ~100 MB | – |
| RAW opened | 1.35 GB | 1044 MiB |
| smoke | peak 1.45 GB | 1044 MiB |
| extremes | peak 1.82 GB | 1044 MiB |
| export (full size) | 2.21 GB within 0.5 s, then **killed** at 2.35 GB | – |

So a full-size export kills even a fresh tab, and the limit varied (2.24
GiB this time, 2.59 GiB before).

The RAW decode alone (`cargo run --release -p spektrafilm-io --example
decode_peak -- web/results/0955097116.raf`, native, same code): file 54
MiB, image 297 MiB, **peak 809 MiB above the file**. With the file's copy in
WASM, that is about 860 of the 1044 MiB floor. Copies in the chain:
`develop_intermediate` clones the raw and makes the f32 mosaic twice
(`apply_scaling`, then `as_f32().into_owned()`), demosaics to 297 MiB, then
`intermediate_to_image_buf` makes another 297 MiB while the intermediate
lives, and `oriented` may make one more.

A full-size GPU frame is 297 MiB per image buffer (26 MP × 3 × f32): the
resident input, `img_a`, `img_b` and the readback, plus the diffusion and
halation FFT buffers. That's the export's spike.

## Where the memory likely goes (unverified leads)

- Decode: rawler's output, then the f32 source `ImageBuf` (26 MP × 3 × 4 B
  is about 311 MB; with alpha, more), plus any intermediate copies during
  `Engine::load` (`crates/spektrafilm-wasm/src/lib.rs` around line 431).
  The RAW file bytes (56 MB) are also copied into WASM.
- `source_at` caches the source resampled for the last two render sizes,
  plus the metering frame (`lib.rs` around lines 252 to 260 and 571).
- GPU: `crates/spektrafilm-gpu/src/wgpu_backend.rs` creates image-sized
  storage buffers (`img_bytes`, around lines 775 to 850), readback buffers,
  and FFT buffers for diffusion and halation
  (`cpu_diffusion_fft_buffer_bytes` in `crates/spektrafilm-core/src/preview.rs`
  gives the CPU side's size). On a phone these are capped at 12 MP by
  `renderSide()`, but there are several of them, and the uploaded source may
  be full-size.
- The page side: `ImageBitmap`s and RGBA readbacks in `web/app.js`, and the
  original (before) view's upload (`set_original_from_input`).
- Each rayon pool worker is a separate JS realm instantiating the same
  module (memory is shared, but each has its own JS heap and compiled code).

## Options to consider

1. **Smaller working source on constrained devices**: decode, downsample
   once to the largest size the device will display (for example 12 MP or
   less), drop the full-size buffer, and develop the export separately
   (redecode the RAW at export time, or in tiles). This is probably the
   biggest win: the full-size f32 source and everything sized from it go
   away while editing.
2. Keep the full-size source compact (f16 or u16) and expand per render.
3. Don't keep the resampled-size cache, the metering frame or the original
   view at full size; release GPU buffers when the size changes.
4. Make sure the decode's peak is low (avoid holding the file bytes, the raw
   mosaic and the developed image at the same time), because WASM never
   gives that peak back.
5. Find a memory budget: `navigator.deviceMemory` is not in Safari. Options
   are a coarse-pointer or screen heuristic, the GPU adapter's limits, or an
   app setting. Desktop keeps today's behaviour.

## How to test

- Devices: iPhone UDID `00008110-000240403AD1801E`, now on **USB with
  Developer Mode on**, Web Inspector and Remote Automation enabled. iPad Air 5
  UDID `00008103-001405890204C01E`, also on USB.
- Servers already running in the earlier session (check before starting new
  ones): `NEGS_DEV_CERTS=/private/tmp/negs-ipad-certs node web/serve.mjs 8443 --https`
  and `safaridriver --port 18138 --diagnose`. The Mac's address is
  `192.168.178.88`.
- A run in a fresh tab (zsh: wrap in a function, a variable doesn't split):

  ```sh
  export SAFARI_DEVICE_TYPE=iPhone SAFARI_DEVICE_UDID=00008110-000240403AD1801E SAFARI_TEST_DIR=target/iphone-usb
  node web/safari.mjs create --insecure
  node web/safari.mjs navigate "https://192.168.178.88:8443/?device-test=<id>"
  node web/safari.mjs open-url results/0955097116.raf
  IPAD_PHASE=smoke node web/safari.mjs run web/checks/ipad.mjs
  IPAD_PHASE=extremes node web/safari.mjs run web/checks/ipad.mjs
  node web/safari.mjs end
  ```

  A killed tab shows up as `no such window` and then `invalid session id`.
  Delete `target/iphone-usb/session.json` and create a new session.
- The kill itself: stream the syslog and grep for it.
  `target/ipad-diagnostics/bin/pymobiledevice3 syslog live --udid <udid> | grep --line-buffered -iE 'memorystatus|jetsam|WebContent'`
  (an earlier capture is in `target/iphone-usb/syslog.txt`; there may still
  be a capture running from the earlier session).
- Footprint over time: mount the developer disk image
  (`pymobiledevice3 mounter auto-mount`) and use
  `pymobiledevice3 developer dvt sysmon process monitor` (or `single`) for
  the WebContent pid. Not tried yet. Also record the HUD's `memoryMiB` (WASM
  memory, from `memory_bytes()`) per step. The difference between the two
  is GPU and page memory.
- The pass condition: a fresh tab can open the RAW, run smoke, extremes and
  an export, and then open the RAW **again in the same tab** at least twice
  without a kill. Check desktop (Chrome/Safari on the Mac) still renders at
  full resolution and as fast as before.
- macOS has no `timeout`: use `perl -e 'alarm N; exec @ARGV' <cmd>`.

## State of the checkout (all uncommitted, don't reset or clean)

- Leak fix, iPad-verified: replaced engines kept rayon's pool workers and
  so about 1 GB each. `web/worker.js` (`dispose`), `web/app.js` `kill()`,
  `wake_thread_pool` in `crates/spektrafilm-wasm/src/lib.rs`, `rayon` in
  `crates/spektrafilm-wasm/Cargo.toml`. `web/pkg*` were rebuilt with
  `PATH=$PWD/target/ipad-tools/bin:$PATH bash web/build.sh --threads`.
- `web/checks/ipad-runner.mjs` now forwards `threads`, `backend` and
  `present` to the app frame. Known race, not fixed: after `frame.src` is set,
  `waitFor` can see the previous frame's `negs`.
- Many other changes from earlier sessions; see `git status`.
- Rules: commit only when asked, never push, end commits with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Don't copy or
  print the certificate keys in `/private/tmp/negs-ipad-certs`. Keep
  `web/results/device-*.jsonl`, `target/iphone-tests/` and
  `target/iphone-usb/`.

## Steps 1 and 2 (21:20Z): fewer decode copies, a working copy on touch devices

1. The RAW decode copies less (vendored rawler's `develop_intermediate_owned`
   and `crop_in_place`; in place `calibrate`; `flatten_rgb` hands the buffer
   to `ImageBuf`). Bit-identical pixels on all 112 sample RAWs, and faster:
   Fuji RAF peak 809 → 461 MiB (330 → 275 ms), Nikon landscape 735 → 409,
   portrait 735 → 594, 60 MP DNG 1.8 → 1.0 GiB.
2. `Engine::load(bytes, max_dim, max_pixels)` keeps a smaller copy and turns
   a RAW upright after shrinking it; it drops the last photograph before
   decoding. The page asks for `keptPixels()` (the coarse-pointer budget,
   12 MP; 0 elsewhere, so desktop is unchanged) and loads in full for an
   export, then the copy again (`exportRender`, `reloadPhoto`).

iPhone, fresh tab, same steps as above:

| Step | Before | After |
|---|---|---|
| RAW opened | 1.35 GB, WASM 1044 MiB | 1.05 GB, WASM 645 MiB |
| smoke peak | 1.45 GB | 1.13 to 1.16 GB |
| extremes peak | 1.82 GB | 1.52 to 1.57 GB |
| export | killed | killed (step 3) |

- The kill isn't at a fixed size: 2.59, 2.24, 3.13 and 1.58 GB (the last
  during extremes, with 1.3 GB free on the phone). It reads as iOS killing the
  largest tab over a soft limit when the phone is short of memory, so aim
  well under 1.5 GB.
- Diffusion (Lens or Print) adds about 500 MB at a 1212 px frame and holds it
  while on: the GPU FFT grid is `(h + 2r) × (w + 2r) × 20 B`, and the PSF
  radius r grows with the frame's width. At a full-size export that is GBs.
- A slow creep of about 1 MB/s during extremes, not yet explained.

## Step 3 (22:20Z): 12 MP export on touch devices, diffusion fixes

- Touch devices export the copy the engine keeps (`photo.exportWidth`,
  `exportNote()` says so in the HUD and on the Export button); desktop
  exports every pixel as before.
- iOS Safari keeps a destroyed GPU buffer's memory until GC: each render
  with diffusion on added a new FFT grid (+48 MB at 1212 px) until the tab
  was killed. `build_diffusion_state` now takes its grids from the render's
  `ImageSet` (`grid` closure), so they're reused. Measured flat on the phone.
- `WgpuBackend::set_diffusion_grid_budget` (JS `setDiffusionGridBudget`,
  160 MiB on touch devices, 0 elsewhere; natively
  `SPEKTRAFILM_DIFFUSION_GRID_BUDGET`): a grid past it convolves the frame
  shrunk k× (fft_conv.wgsl modes 5 and 6). 12 MP default Pro-Mist → k = 2,
  max ΔE00 0.45 with grain off. Comparison page:
  https://claude.ai/artifact/Y3XyNnMkfvxGGTNNqkW98A (made with
  `crates/spektrafilm-cli/examples/diff_maps.rs`, renders in
  `target/diffusion-compare/`).
- iPhone: the full scenario (open, smoke, extremes, export, after-export,
  open + smoke twice) passes. Export with diffusion peaks at 1.96 GB (was
  2.30). After the second open WASM memory is 944 MiB (was 645 after the
  first) and the tab ~1.3 to 1.4 GB: the second decode doesn't fit the freed
  space. Next: decode in a short-lived worker, so the editing engine's
  memory holds only the 12 MP copy.
- Not yet checked: desktop in a browser, the iPad.

Steps 1 to 3 are committed (580344d).

## Step 4 (22:50Z): decode in a short-lived worker on touch devices

- `web/decoder.js` decodes and shrinks the photograph (wasm `decode`, the
  same code as `Engine::load`) and ends; the engine takes the kept copy
  (`Engine::loadDecoded`, after `forget`, so the copy takes the last one's
  room). The page starts the next decoder after each decode and when the
  engine starts, so the module has compiled by the next photograph. It
  runs rayon as the engine does and ends its pool workers the same way
  (terminate, `wakeThreadPool`). Desktop (`keptPixels()` 0) decodes in the
  engine as before.
- Same render bytes as `Engine::load` (RAF and a 24 MP JPEG, at 12 MP, in
  Node). Open times on the phone about as before: 1.1 s (single-threaded
  and compiled on demand it was 3.3 s).
- iPhone, full scenario, all pass (runs `w5`, `w6`):

| Step | Step 3 | Step 4 |
|---|---|---|
| WASM after open 1 / 2 / 3 | 645 / 944 / 944 MiB | 211 to 217 MiB, flat |
| extremes peak | 1.57 GB | 0.95 GB |
| export peak | 1.82 GB | 1.43 to 1.45 GB |
| idle after an open | 1.3 to 1.4 GB | 0.5 GB (0.85 to 1.0 after an export) |
| open peak (transient) | 1.03 to 1.48 GB | 1.09 to 1.66 GB |

- The open peaks are the decoder (645 MiB of WASM: the RAW's bytes, its
  decode and the 12 MP copy) beside the engine; it's gone seconds later.
  After an export the tab stays at ~0.9 GB: GPU buffers waiting for GC (the
  export's buffer set is over the pool's 512 MiB, so the pool doesn't keep
  it; releasing the pool explicitly changed nothing and was removed).
- Desktop Chrome on the Mac: desktop decodes in the engine (6240 × 4160
  kept, 505 ms); with a touch viewport (`VIEW=390x844@3m`) in the worker
  (507 ms, engine 221 MiB, exports 4242 × 2828). Mac Safari needs "Allow
  remote automation" for safaridriver, not enabled.
- Next leads: the decoder's peak (drop the RAW's bytes after the raw decode;
  shrink during develop), and the open-after-export peak. Not yet checked:
  the iPad.
