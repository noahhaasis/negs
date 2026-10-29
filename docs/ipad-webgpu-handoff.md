# Handoff: iPad WebGPU testing on iPadOS 26, and retained memory across engines

Recorded 2026-10-29, about 14:30 UTC / 15:30 Europe/Berlin.
Repository: `/Users/noahhaasis/Projects/Negs-wasm`, branch `wasm-investigation`.

## Where things stand

The owner updated the iPad Air 5 to iPadOS 26 so WebGPU could be tested on a
device that connects reliably. Over USB, Apple's `safaridriver` drove the full
device suite on the Fuji RAW with the automatically selected WebGPU engine.
Every phase passes in a fresh tab, including zoom and rapid edits, the two
points where the iPhone reloaded (see `docs/iphone-testing-handoff.md`).

The commit that adds this handoff also fixes two engine-switch races in
`startEngine` (`web/app.js`), found by the lifecycle phase. It was committed,
not pushed. Results and the fix are written up in
`docs/apple-device-testing.md`, section "Physical iPad WebGPU runs".

**The open problem:** memory builds up across engine lifetimes in one Safari
tab until WebGPU fails with `Out of memory`. Closing the tab recovers. This is
the most promising lead for the iPhone's automatic reloads, but it is not
confirmed.

## Device and connection

- iPad Air 5, `iPad13,16`, name `NSA 0-Spy (2)`,
  UDID `00008103-001405890204C01E`. iPadOS 26.7.2 (23H128), Safari 26.6.3
  (from the driver session). The UA claims macOS; don't derive the OS from it.
- USB works: `target/ipad-diagnostics/bin/pymobiledevice3 usbmux list` and
  `xcrun devicectl list devices` both show it, and it is paired and trusted.
- After the OS update, **Settings → Apps → Safari → Advanced → Web Inspector and
  Remote Automation** had to be re-enabled. Before that, session creation
  timed out with "waiting for its RWIApplication to appear" and
  `pymobiledevice3 webinspector` hung. If that happens again, check these
  first.
- The iPad was iPadOS 18.2.1 earlier; that CPU-only baseline can't be
  reproduced on it any more (results remain in `docs/ipad-stability.md`).

## How to run

Server and driver (both were running at handoff; recheck):

```sh
NEGS_DEV_CERTS=/private/tmp/negs-ipad-certs node web/serve.mjs 8443 --https
safaridriver --port 18138 --diagnose
```

Wait for `curl -s http://127.0.0.1:18138/status` to report ready before
`create`, since an immediate `create` fails with `fetch failed`.

```sh
export SAFARI_TEST_DIR=target/ipad26-tests SAFARI_DEVICE_TYPE=iPad \
  SAFARI_DEVICE_UDID=00008103-001405890204C01E
node web/safari.mjs create --insecure
node web/safari.mjs navigate "https://192.168.178.88:8443/?device-test=<id>"
node web/safari.mjs open-url results/0955097116.raf
IPAD_PHASE=smoke node web/safari.mjs run web/checks/ipad.mjs
node web/safari.mjs end   # closes the tab, which also frees the retained memory
```

The Mac's address was `192.168.178.88`; it may change. Phases: `smoke`,
`extremes`, `zoom`, `heavy`, `soak`, `lifecycle`, `export`, `after-export`
(`limited-export` exists but wasn't run). **Start each comparison in a new
session** (`end` then `create`); after an out-of-memory failure, a navigation
alone does not recover.

## Results (fresh tab, WebGPU, 6240 × 4160 Fuji RAW)

All passed: smoke, zoom ("100%" is clamped to 68% by the 12 MP touch GPU cap),
heavy, soak, extremes (207 cases), export (5,632,535-byte JPEG), after-export,
and lifecycle after the fix. The page kept one epoch within each tab, every
frame was presented, and passing runs logged no error. WASM memory was 1052 MiB
throughout.

## The fix in this commit

Lifecycle selects CPU and then GPU without waiting. On iPadOS 18 the GPU choice
did nothing; with WebGPU it replaces the engine while the CPU engine is
still reloading the RAW:

1. The replaced engine's `load` rejected with `engine restarted`, which the
   selection handler showed as an error.
2. With that ignored, the first selection still called `schedule("settled")`,
   rendering on the new engine before its load: `Render failed: no image loaded`.
3. A restart inside the `try` would have shown "WebGPU isn't available" and
   fallen back to CPU, because `kind` was already `"gpu"`.

A superseded `startEngine` now returns `false`, and the `#engine` change
handler skips its render when that happens.

The commit holds only these hunks. `web/app.js` still has earlier,
**uncommitted** iPad-stability changes from previous sessions: the
no-`navigator.gpu` guard in `startEngine`, `BROWSER_RENDER_LIMIT` handling in
`failed`, `negs.engineInstances`, and the same-backend early return in the
change handler. The tests here ran with those present. Similarly,
`docs/apple-device-testing.md` and the other device docs and checks
(`docs/ipad-stability.md`, `docs/iphone-testing-handoff.md`, `web/checks/ipad*`,
`web/safari.mjs`) are untracked and were not committed. They were updated or
used in place. Don't reset or clean the checkout.

## Open: `Out of memory` after several engine lifetimes in one tab

What was observed, in order, in one automation tab:

1. Page A: one GPU engine, the full suite except lifecycle, which ran last and
   hit the false error above.
2. Navigation to page B (fix applied), RAW opened, lifecycle: GPU → CPU → GPU.
   The new GPU engine failed: `Render failed: Out of memory`
   (`run-lifecycle-2.log`).
3. Navigation to page C: **opening the RAW itself** failed with
   `Couldn't open 0955097116.raf: Out of memory` (`run-slow-switch.log`).
4. `end` + `create` (new tab): the RAW opened, and one slow CPU → wait 5 s → GPU
   switch passed (`run-slow-switch-2.log`).

`Out of memory` is not produced by app code (grep finds nothing). It likely
comes from WebKit's WebGPU or wgpu (device or buffer allocation). Each engine
has about 1 GB of shared WASM memory plus GPU buffers. Terminated workers and
navigated pages apparently don't release that memory promptly within the same
tab or WebContent process. `negs.engineInstances()` counts engines started,
not live ones.

Suggested next steps:

1. Measure: in a fresh tab, switch engines N times slowly, or navigate N times
   and open the RAW each time, until it fails. Record N for each pattern, and
   whether waiting (seconds to minutes) helps.
2. Check whether the GPU engine destroys its `GPUDevice`/buffers before the
   worker is terminated. `kill()` in `web/app.js` just calls
   `worker.terminate()`; a graceful `dispose` call (destroy device and buffers,
   drop the source) before terminating may release memory sooner. See the
   buffer lifetimes in `crates/spektrafilm-gpu/src/wgpu_backend.rs` and
   `Engine.load`/`source_at` in `crates/spektrafilm-wasm/src/lib.rs`.
3. Relate it to the iPhone: its two reloads happened with **one** engine
   instance, so retained memory across engines doesn't explain them directly.
   It does show the GPU path's memory headroom is thin. Measuring per-engine
   GPU allocation and comparing it against the 6 GB phone remains open.
4. Not yet done on iPadOS 26: forced CPU (`?backend=cpu`) and `?threads=0`
   comparisons. The outer Wi-Fi runner still doesn't forward these parameters
   to its iframe; the `safari.mjs` route navigates directly, so it can pass
   them.

## Evidence

`target/ipad26-tests/` (ignored): `events.jsonl` covers all sessions, plus one
`run-<phase>.log` per phase. Re-runs carry `-2`, `-3`, `-4` suffixes. The passing
lifecycle is `run-lifecycle-4.log`. Screenshots sit alongside. The ad-hoc slow
switch script lived in the session scratchpad and isn't kept; it selected CPU,
waited 5 s, selected GPU, and edited exposure.
