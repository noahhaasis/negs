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

**Update:** the memory build-up across engines is found and fixed (uncommitted;
see "Cause and fix" below). A separate WebContent crash in Safari's Wasm
compiler is now the leading lead for the iPhone's reloads.

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

### Cause and fix (later the same day, uncommitted)

With Developer Mode on, `pymobiledevice3 developer dvt sysmon process single`
samples per-process memory over USB (no tunnel needed on this iPad). A
sampler logged every WebKit process to `target/ipad26-tests/memory.jsonl`
while a script switched engines slowly (CPU, settle, GPU, settle, ...):

- **Threaded build, before the fix:** WebContent's footprint grew about 1 GB
  per engine (1456, 2434, 3653 MiB) and never dropped while idle; the fourth
  engine's worker died at start and the page showed `Out of memory`. The GPU
  process stayed near 20 MiB, so this is WASM memory, not WebGPU buffers.
- **`?threads=0`:** flat at about 1.2 GB through 9 engines.
- **Terminating rayon's pool workers** before the engine worker (a `dispose`
  message): no change. The page can't reach them; `worker.js` now records
  the workers the pool makes and ends them itself.
- **Terminating them and then waking them** (`wakeThreadPool`, a
  `rayon::spawn_broadcast` of an empty job): flat at 1.2 to 1.5 GB through
  17 engines in one tab. Idle pool threads wait in `memory.atomic.wait`; the
  likely mechanism is that Safari doesn't end a terminated worker parked
  there, so the shared memory stays referenced until the tab closes.
  Lifecycle and smoke pass with the fix.

The fix is in `web/worker.js` (pool tracking, `dispose`), `kill()` in
`web/app.js` (sends `dispose`, terminates after 10 s if the worker is still
busy), and `wakeThreadPool` in `crates/spektrafilm-wasm` (lib.rs, Cargo.toml:
`rayon` under the `threads` feature). `web/pkg*` were rebuilt with
`PATH=$PWD/target/ipad-tools/bin:$PATH bash web/build.sh --threads`. That
build also picked up the working tree's other uncommitted Rust changes.

### New: WebContent crashes in JSC's Wasm tier-up

Three WebContent crashes, all `EXC_BREAKPOINT` (PAC exception) on the "Wasm
Worklist Helper Thread" in `JSC::Wasm::CalleeGroup::installOptimizedCallee`
from `BBQPlan::work()`: Safari's background compiler installing optimized
code, not memory. Reports: `target/ipad26-tests/crashes/com.apple.WebKit.WebContent-*.ips`
(`pymobiledevice3 crash pull`). In Safari a WebContent crash shows as a page
reload, which is **what the iPhone did**; its reports were never pulled.

- All three fell within 4 minutes right after the reboot that enabled
  Developer Mode (14:45 to 14:49 UTC); none in 13 sessions after.
- One happened with a **single engine** while opening the RAW, so it doesn't
  need an engine teardown. The other two were during switches, with the fix
  or `dispose` alone.
- Tally: wake fix 2 of 5 runs crashed (both early), `dispose` without wake 1
  of 4, the pre-session code 0 of 3 (it fails with the leak instead).

Next steps:

1. Pull the iPhone's crash reports when it's connected
   (`pymobiledevice3 crash ls`/`pull --udid 00008110-000240403AD1801E`) and
   look for this signature at its reload times. If it matches, the iPhone
   reloads are this JSC bug, not memory.
   Done partly: the owner AirDropped the phone's 2026-10-29 reports. Today
   had only one event, five `ExcUserFault_com.apple.WebKit.GPU` files from
   10:45:18 +0100 (simulated, non-fatal `EXC_GUARD`, coalition
   `com.atebits.Tweetie2`, the X app's web view). It is unrelated, about three
   hours before the reloads at 12:53:35Z and 12:54:19Z. The one
   `JetsamEvent` (01:18:46 +0100) killed SpringBoard at its per-process
   limit, also unrelated. No report covers the reloads, so neither the JSC
   crash nor a memory kill is confirmed on the phone. Copies are in
   `target/iphone-tests/crashes-2026-10-29/`.
   Rerun on the phone with the leak fix (16:56 to 16:58 +0100, runs
   `mvtq7hak`, `mvtq8svl`, `mvtq9m40` in `web/results/`): all three still
   reloaded, twice during "rapid exposure inputs" in smoke and once in
   extremes at the glimmerglass diffusion case. Each had **one engine**
   (`engineInstances: 1`, WebGPU, 1044 MiB WASM), so the reloads don't need an
   engine replacement and the leak fix can't address them. The owner's
   `?threads=0` run actually ran 4 threads: the Wi-Fi runner dropped the
   page's query when loading the app frame. `web/checks/ipad-runner.mjs` now
   forwards `threads`, `backend` and `present`.
   With that, smoke at 17:01 to 17:04 +0100: `?threads=0` (WebGPU, 1 thread,
   run `mvtqd7xk`) reloaded in "rapid exposure inputs" as before;
   `?backend=cpu` (4 threads, `mvtqg5bz`) passed. So threads and the JSC
   tier-up crash are not needed; WebGPU is. The CPU run is not equal load
   (4 s per frame, one interactive frame during the rapid inputs). No report
   is written for these reloads, which fits WebKit ending WebContent itself
   (e.g. over its memory limit) rather than jetsam or a crash; unconfirmed.
   `?present=0` (WebGPU, 4 threads, the page draws the engine's bytes),
   17:07 to 17:08: 2 of 4 runs passed smoke (`mvtqlq0c`, `mvtqmoi6`), and
   2 reloaded while **opening** the RAW (`mvtqljgb`, `mvtqmfod`), not in the
   rapid inputs. `mvtqmd8j` failed with "Can't find variable: negs": a Start
   tapped while the previous run's frame was still loaded, a runner race,
   not counted. So turning off presentation moves the reload rather than
   removing it, which fits a footprint limit more than a presentation bug.
   (`mvtqh4xc` stopped at startup, "Editor reloaded or lost its monitor", a
   tap right after the CPU pass; not counted.)
   **Confirmed over USB (19:20 to 19:34Z, Developer Mode on, logs in
   `target/iphone-usb/`, syslog in `target/iphone-usb/syslog.txt`):** the
   reload is the kernel killing WebContent at its per-process limit:
   `killing_highwater_process pid 523 [com.apple.WebKit.WebContent]
   (highwater 100 ...) 2718659KB` (about 2.6 GiB), which Safari logs as
   `processDidTerminate: reason=Crash`. No jetsam report was written. That
   process had served three navigations in one tab (`card.jpg` smoke, RAW
   smoke, then the RAW open of the third), so memory from earlier pages in
   the tab counts. In a fresh tab per run, RAW smoke plus extremes passed
   3 of 3 (`usb6` to `usb8`; WASM memory 1044 MiB, against 126 MiB for
   `card.jpg`). Next: sample WebContent's footprint per step (a mounted DDI
   and `dvt sysmon`, or the syslog), then reduce the RAW's resident memory
   (full-resolution GPU buffers, decoded copies) so one tab has headroom.
2. Reproduce the tier-up crash on purpose: reboot, then run sessions
   immediately. Compare fewer pool threads and `?threads=0`, since each pool
   worker instantiates the same module.
3. Report it to WebKit with the `.ips` files if it reproduces.
4. Not yet done on iPadOS 26: forced CPU (`?backend=cpu`) comparisons.

## Evidence

`target/ipad26-tests/` (ignored): `events.jsonl` covers all sessions, plus one
`run-<phase>.log` per phase. Re-runs carry `-2`, `-3`, `-4` suffixes. The passing
lifecycle is `run-lifecycle-4.log`. Screenshots sit alongside. The ad-hoc slow
switch script lived in the session scratchpad and isn't kept; it selected CPU,
waited 5 s, selected GPU, and edited exposure.
