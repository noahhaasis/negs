# Handoff: fix iPhone offline engine startup

## Goal and scope

Fix offline startup of the **default threaded browser app** on the physical iPhone. The user requested this handoff after an actual offline test failed. They have not supplied their friend's anonymous deployment URL or commit; reproduce and fix the current checkout first. Do not deploy or commit unless asked.

An agent should be able to investigate this: there is a repeatable physical-device failure and a passing single-thread control. **The exact cause is not known.** Do not start by globally disabling threads or claim the cache itself is incomplete.

## Read first

1. `target/offline-iphone-20261031/SUMMARY.md` — full results, build identity, evidence and caveats.
2. `docs/apple-device-testing.md` — device setup, regression methodology and controller pitfalls.
3. `docs/iphone-ipad-stability-followup.md` — recent physical-device work and known engine/GPU memory lifetime issues.
4. `docs/web-wasm-handoff.md`, section “The service worker in development”; `docs/web-wasm.md`, item 19.
5. Source listed below. `docs/web-app-install-handoff.md` is historical design context, not proof of current behavior.

Ignored evidence under `target/` is local to this Mac and will not accompany a Git-only checkout. Copy/share that directory separately if moving machines.

## Confirmed evidence

Tested 2026-10-31, physical iPhone14,3, iOS 26.6.2 (23G90), Safari 26.6.1. UDID `00008110-000240403AD1801E`. Default actual engine online: `wgpu (GPU) · 4 threads`. Secure context and cross-origin isolation verified.

Fresh static package from checkout `cd3bc2bd2a5ff8671f8b1c35e9940bf66f37fe29`, with existing user changes recorded in `package/build-info.json`. Runtime asset hashes, not the revision alone, identify the tested build.

Evidence root: `target/offline-iphone-20261031/`.

- `package/`: exact tested static package, 170 files, 63.8 MiB; full hashes in `build-info.json`.
- SW version `ec42bff2543c`; SW SHA-256 `d4dd24cacbe5aa23496492cf6747c3bc99d2c96a69619b24ef5fd6c8c674d3e3`.
- `events.jsonl`: chronological host and phone events, startup probes, cache status, import/edit/export results.
- `ready-default.log`, `ready-crossover.log`, `online-snapshot.log`: complete installation/cache evidence.
- `offline-single-smoke.log`, `offline-single-export.log`: successful single-thread offline editing/export.
- `ready.mjs`, `snapshot.mjs`, `startup.mjs`: useful host-side test scripts. **`startup.mjs` only observes startup:** its controller `suite-pass` does NOT mean the engine started. Inspect the recorded engine label/error and assert readiness explicitly in new regression tests.

### Failure

1. Load the package over HTTPS with its server online.
2. Wait for a controlling service worker AND footer `Works offline · 29 MB saved.`
3. Worker status confirmed `shellMissing: 0`, `saved: 30737840`, `total: 63151498`.
4. Stop the origin server completely. Initial test verified host-side connection refused.
5. Navigate/reload normally at the same origin (not a bypass-cache reload).
6. Page shell/JS load from cache, isolation remains true, but engine stays at `Starting…`. No visible error or captured post-navigation JS error. Reproduced with fresh sessions/full installs; the first failure persisted over a minute.

**Important control:** In a fresh session, first load online with `?threads=0`, wait for complete install, stop server, then navigate to `/` without `threads=0`. Default startup still hangs. An outgoing threaded engine is therefore NOT required to reproduce it.

### Passing control

With `?threads=0`, online install followed by offline reload reaches `wgpu (GPU) · 1 thread`. With the origin server still stopped:

- Opened a real 6048 × 4032 DNG through the application's file-input change handler.
- Exposure −5/+5/0 EV, rapid inputs and reset passed; 15 total completed/presented renders.
- Export produced a 3,629,306-byte JPEG Blob.
- Same page epoch and engine; no monitored errors.

Fixture is the existing redistributable `web/sample/DSC_6377.dng`, 3,362,272 bytes, SHA-256 `3017ee5e90ad64b1b090dc6f7904447dfdf12c661a24f5e3779df4038e1448bd`. No private large RAW is needed for this bug.

### Do not overinterpret

- Stopping the app origin is a strong no-network test for app/worker downloads, but the phone retained other network connectivity; `navigator.onLine` stayed true. Airplane Mode/offline UI and Home Screen standalone behavior remain untested.
- A later server-restored/default reload reported `Out of memory` after failed engine lifetimes. Concurrent diagnostic worker probes also hit allocation errors. Neither establishes the original hang's cause. Avoid concurrent engine probes; use clean sessions for comparisons.
- Early unavailable-server attempts and an attempt stopped during installation are SETUP failures. Do not confuse them with the completed-cache reproductions.
- New automation sessions appeared not to retain the previous session's offline state. Reinstall and wait for activation in each fresh session. Do not use a fresh session with the server already stopped to claim persistent-cache failure without verifying that cache exists.
- Sample 1 is saved on first use or “Save everything,” not precached. Other sample photographs deliberately remain online-only. This is separate from engine startup.
- File input was populated by WebDriver, not the native Files picker. JPEG Blob creation was checked, not native saving to Files.

## Key source and investigation points

- `web/app.js`: `startHost`, `startEngine`, `startDecoder`, `startOffline`, `ask`, `offlineStatus`.
- `web/worker.js`: static engine import; dynamic threaded module import; `wasm.default()`; `wasm.initThreadPool()`; engine creation.
- `web/decoder.js`: analogous startup and its separate thread pool.
- `web/engine.js`: data requests and offline error reporting.
- `web/sw.js`: install/cache/hash validation; fetch responses; `isolated`; client/build pinning.
- `web/offline.mjs`: asset roles, generated manifest; `web/package-static.mjs`: static package, Rayon directory-import patch.
- Generated `pkg-threads/index.js` and `pkg-threads/snippets/wasm-bindgen-rayon-*/src/workerHelpers.js`, **inside the package actually served**.
- `web/safari.mjs`; `web/checks/ipad.mjs`; `web/checks/safari-controller.test.mjs`; `web/checks/engine-lifetime.test.mjs`.

Suggested approach:

1. Reproduce untouched baseline; also compare a **service-worker-controlled online reload** in a fresh, separately installed session. We did not establish a clean online-controlled reload baseline; the later OOM run was confounded. This distinguishes offline fetch failure from cached-response/thread startup behavior or navigation lifetime problems.
2. Add temporary diagnostic milestones before/after threaded JS import, WASM fetch/compile/instantiate, thread pool initialization, each nested worker ready, and GPU creation. Capture diagnostics from the beginning, including `error`, `unhandledrejection`, nested-worker `error` and `messageerror`. The normal monitor is often attached after page loading has begun.
3. Find the exact unresolved promise. Rayon's generated `waitForMsgType` only waits for a ready message; inspect what happens if a pool worker fails to load/import/initialize. Do not assume it rejects by itself.
4. Verify actual worker/nested-worker request URLs, cache keys, MIME and isolation headers, module resolution, and shared-memory availability. Asset presence plus `shellMissing: 0` does not prove a worker consumed the cached response successfully.
5. Keep Safari allocation/lifetime hypotheses separate from fetch/isolation hypotheses. No photo is required, so start with an empty editor. Avoid repeatedly stacking abandoned pools in one tab.
6. Implement the smallest justified fix in maintained source/build logic; do not rely on hand-editing generated output only. Add automated coverage of the failure mechanism. Add explicit error/timeout handling if needed so startup cannot silently wait forever; a timeout message alone is not a working-offline fix.

## Reproduction setup

At handoff, assistant-created driver/server and automation session were stopped. The user's existing `node web/serve.mjs 8137` was left alone. Probe current processes/ports before starting anything; stale PID files are historical, not authority to kill processes.

- iPhone and iPad were both connected. Set device type and exact UDID; never accidentally test the iPad.
- Ask the user to unlock the iPhone, leave Safari foregrounded, and keep it available for the run. If locked/disconnected, pause instead of reconnection loops or device settings changes.
- Existing certificate directory: `/private/tmp/negs-ipad-certs`. Keep keys outside web/package roots and never print/commit them. Device sessions used `--insecure`; secure context/isolation were still verified.
- Previous Mac LAN IP: `192.168.178.88`; verify with `ipconfig getifaddr en0`.
- Use NEW ignored result and package directories, not `public/` or old evidence directories.

Example commands (choose free ports; keep the environment on every controller command):

```sh
export SAFARI_DRIVER_URL=http://127.0.0.1:18141
export SAFARI_DEVICE_TYPE=iPhone
export SAFARI_DEVICE_UDID=00008110-000240403AD1801E
export SAFARI_TEST_DIR=target/iphone-offline-fix-<unique-run>
export SAFARI_SCREENSHOTS=0
mkdir -p "$SAFARI_TEST_DIR"

# Start /usr/bin/safaridriver --port 18141 in a persistent process.
node web/safari.mjs create --insecure

node web/package-static.mjs "$SAFARI_TEST_DIR/package"
# Start persistently, with logs and saved PID:
NEGS_DEV_CERTS=/private/tmp/negs-ipad-certs \
  node web/serve.mjs 8451 --https --static "$SAFARI_TEST_DIR/package"

node web/safari.mjs navigate https://192.168.178.88:8451/
node web/safari.mjs run target/offline-iphone-20261031/ready.mjs
# Only after active controller + Works offline + shellMissing == 0:
# stop YOUR package server and verify origin connection refused.
node web/safari.mjs page https://192.168.178.88:8451/
node web/safari.mjs run target/offline-iphone-20261031/startup.mjs
```

In this harness, a background server started in the same shell as a command that later timed out disappeared. Python `subprocess.Popen(..., start_new_session=True, stdin=DEVNULL, stdout=<log>, stderr=STDOUT)` kept the server alive between tool calls. Verify listening state instead of assuming `nohup ... &` survived. Do not background and foreground the server command in the same shell and expect following test commands to run.

For offline photo coverage after readiness:

```sh
node web/safari.mjs upload web/sample/DSC_6377.dng
IPAD_PHASE=smoke node web/safari.mjs run web/checks/ipad.mjs
IPAD_PHASE=export node web/safari.mjs run web/checks/ipad.mjs
IPAD_PHASE=after-export node web/safari.mjs run web/checks/ipad.mjs
node web/safari.mjs end
```

The suite is historically named `ipad.mjs` but also runs on the iPhone. Do not relabel programmatic file import as native Files coverage.

## Acceptance criteria

- Exact candidate build identity and physical-device capabilities recorded; fresh baseline failure preserved.
- Default online install finishes; repeat service-worker-controlled online startup passes.
- Default offline reload reaches the actual GPU/thread label (normally four threads on this phone), with isolation retained and no silent hang.
- With origin still stopped, local photo import, rendered edits/reset, JPEG export and subsequent edit pass. Require presented frames, same page epoch, and no unexpected engine replacement, panic or visible error.
- Single-thread route still passes. Do not mask a threaded failure by silently turning all users into single-thread users unless explicitly agreed as a limited fallback.
- Add regression tests for the diagnosed cause and startup failure reporting; run relevant existing tests. At least two clean physical default-route repetitions, not just recovery after a failed run.
- If feasible, fresh tab/reopen with verified retained storage; manual Airplane Mode, native Files import and Home Screen startup with the user's help. Report untested cases explicitly.
- No unsaved optional data promised offline; preserve build pinning/hash integrity and correct offline/update errors.
- Close test session and stop only your own temporary jobs. Preserve all pre-existing user changes, old evidence, `public/`, and device settings. No commit/deployment without approval.

## What would help most from the user

1. Keep the physical iPhone unlocked, connected by a reliable cable, Safari foregrounded during requested tests.
2. A short manual check in ordinary Safari and, if used, Add to Home Screen: does it show the shell, stall at Starting, or show Safari's offline error page? A screenshot of that state helps distinguish failure classes.
3. Later, the friend's exact anonymous URL and whether their issue is Safari-tab or Home Screen startup. Useful for deployment validation, **not required to fix the local repro**.
4. The exact steps: whether the footer had reached Works offline, whether a sample was opened/saved first, whether the tab was reloaded or browser relaunched, and whether they disabled Wi-Fi only or cellular too.

## Status — fixed locally 2026-10-31 (not committed or deployed)

Two causes, both confirmed on the physical iPhone (evidence: `target/iphone-offline-fix-20261031b/SUMMARY.md`):

1. Safari 26.6 sends only the first of several same-URL nested workers through the service worker. rayon's other pool workers load from the network; offline they never load, with no error, and `initThreadPool` waits forever. Fix: `web/pool.js` gives each pool worker its own `?pool=<n>` URL. It also turns a pool that doesn't start into an error after 30 s.
2. Thread pools outlived a navigation, so the 2nd–3rd reload in a tab (online too) ran out of memory. Fix: `pagehide` disposes the engine and decoder pools (`web/app.js`).

Two clean physical runs passed: offline reload at `wgpu (GPU) · 4 threads`, then offline DNG import, edits, JPEG export, after-export edits and another offline reload. The single-thread route passes. The unfixed package fails the same script. Still untested: reopening a closed tab offline, Airplane Mode, the Home Screen app, the native Files picker and the friend's deployment.
