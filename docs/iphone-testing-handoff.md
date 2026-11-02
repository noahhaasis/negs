# Handoff: physical iPhone Safari / WebGPU testing

Recorded 2026-10-29, last connection check 12:58 UTC / 13:58 Europe/Berlin.
Repository: `/Users/noahhaasis/Projects/Negs-wasm`.

## Request and immediate objective

The owner wants another agent to try testing Negs on their connected iPhone.
They will consider buying another cable if the available connection methods
cannot be made to work. Continue from the evidence below; do not ask them to
repeat the same cable/trust steps without a new reason. No new app fix was
implemented during this iPhone session.

Separate two problems: unreliable Mac-to-phone inspection/automation, and two
real automatic page reloads in ordinary iPhone Safari. Wi-Fi testing worked
without Finder or a working automation connection. Do not assume a new cable
will fix the app reloads.

## What actually happened

The owner kept Safari on the Negs editor, then ran the ordinary-Safari runner
twice. Both runs rendered the RAW with WebGPU, then reloaded automatically.
The owner explicitly confirmed the reload was automatic, not a manual refresh.

| Run / host log | Coverage completed | Failure |
| --- | --- | --- |
| `web/results/device-mvtjn4wf-65ef1c8b.jsonl` | Import, −5/+5/0 EV, rapid edits, reset; 12 frames; smoke phase passed | Requested 100% zoom at 12:53:34 UTC; unexpected reload at 12:53:35 |
| `web/results/device-mvtjog4m-6f2bcc3a.jsonl` | Import, −5/+5/0 EV; 4 frames | Requested rapid exposure inputs at 12:54:18 UTC; unexpected reload at 12:54:19 |

Neither reached soak or export. These are failed runs, not successful tests.
Both used `wgpu (GPU) · 4 threads`, secure context and cross-origin isolation,
and fitted GPU frames of 1212 × 808 with `presented: true`. One engine instance
and no logged JS error/panic preceded each reload. That does not exclude a
WebKit/GPU process crash or OS termination.

Last completed renders reported 1044 MiB WASM linear memory. This is not live
physical/process memory, and excludes other browser/GPU allocations. Memory
pressure is plausible but unconfirmed. Seek a matching crash/Jetsam report
around **13:53–13:54 Europe/Berlin / 12:53–12:54 UTC**. The second failure means
a zoom-only workaround would not establish stability.

## Device and connection history

- Name `NSA Spy (2)`, product `iPhone14,3`.
- UDID `00008110-000240403AD1801E`.
- Device service: iOS `26.6.2`, build `23G90`.
- Actual test user agent: Safari `Version/26.6.1`, with an iPhone OS `18_7`
  compatibility token. Record these separately; do not derive OS from the UA.
- Finder never showed the phone. Owner already unlocked/reconnected, was asked
  to accept Trust, and tried another port/data cable connection; still missing.
- USB enumeration (`ioreg` / System Information) showed no phone.
- `pymobiledevice3` briefly saw a **Network** usbmux connection and could inspect
  startup/create a Safari automation page. Later the phone disappeared.
- Latest `target/ipad-diagnostics/bin/pymobiledevice3 usbmux list` returned `[]`.
- Bonjour discovery previously reported no route to host and suggested the
  host app's macOS Local Network permission. Investigate that separately from
  USB trust and from app crashes; no permission change was verified.
- `xcrun devicectl` / `xctrace` showed only an old, offline iPad. Do not test or
  identify that device as this phone: its UDID is
  `00008103-001405890204C01E`, iPad Air 5, iPadOS 18.2.1.

## Working route: ordinary Safari over local Wi-Fi

The existing server is alive and responded HTTP 200 at handoff:

`https://192.168.178.88:8443/`

It sends `Cache-Control: no-store`, COOP `same-origin`, COEP `require-corp`.
Phone startup directly confirmed secure context, isolation, and `navigator.gpu`.
Certificates are in `/private/tmp/negs-ipad-certs`, outside the served tree.
Do not copy or print certificate private keys. Mac address may change later.

If the server is no longer running, start from the repo root:

```sh
NEGS_DEV_CERTS=/private/tmp/negs-ipad-certs node web/serve.mjs 8443 --https
```

Previously used test URL (the page says iPad but also works on iPhone):

```text
https://192.168.178.88:8443/checks/ipad.html?phases=smoke,zoom,soak,export,after-export
```

The owner taps **Start tests**, keeping Safari in front. No remote driver is
needed once open. The runner POSTs pre-action and result records to
`/device-log?run=<id>`; the server appends `web/results/device-<id>.jsonl`.
Unexpected reloads are detected through sessionStorage. Preserve failures.
Consider shorter, isolated phases to gather useful evidence instead of
repeating the same full crashing sequence unchanged.

Sources: `web/checks/ipad.html`, `ipad-runner.mjs`, `ipad.mjs`.
Phases: `smoke`, `extremes`, `zoom`, `heavy`, `limited-export`, `soak`,
`lifecycle`, `export`, `after-export`.

**Runner limitation:** it currently sets the editor iframe URL to
`/?device-test=<run>`. Adding `backend=cpu` or `threads=0` to the outer test
page does not forward those settings! Explicitly implement forwarding or use
a real controller/direct editor URL for comparisons, and verify the actual
engine label/thread count. The editor itself supports `?backend=cpu` and
`?threads=0`.

Fixture: `web/results/0955097116.raf`, 56,201,536 bytes, 6240 × 4160 Fuji RAW.
SHA-256 `9d5ff2f21f466b9e3177cbc61969ff6a9faa6bf4ac25bf99e2c33089be82faab`.
Catalog, normal/threaded WASM and fixture endpoints were checked successfully.

## Automation methods tried, and traps

Apple `safaridriver` is running on port 18140, PID 38126 at handoff. Its status
says ready, but physical-iPhone session creation failed: devices found but
could not be used, including with the explicit phone UDID/type. Mac Safari
reported 26.5.2; that is not the phone Safari version.

The existing `web/safari.mjs` now supports `SAFARI_DEVICE_TYPE=iPhone` (default
is iPad). Use a **new** result directory, because `target/iphone-tests/session.json`
belongs to the experimental bridge, not Apple's driver:

```sh
export SAFARI_DRIVER_URL=http://127.0.0.1:18140
export SAFARI_DEVICE_TYPE=iPhone
export SAFARI_DEVICE_UDID=00008110-000240403AD1801E
export SAFARI_TEST_DIR=target/iphone-retry
node web/safari.mjs create
```

Keep these environment settings on later `navigate`, `probe`, and `run`
commands. See the controller's source for commands. Creation above previously
failed; this is a valid retry only after resolving discovery/connection.

The existing Python venv at `target/ipad-diagnostics/` includes
`pymobiledevice3`; no install is required. Direct Web Inspector startup
evaluation worked briefly, returning GPU/isolation/backend evidence. However:

- `target/iphone-tests/network-driver.py` creates a custom automation session
  and HTTP bridge on 18141. Page navigation worked; subsequent script
  evaluation hung/timed out. PID 38462 is still running at handoff, likely with
  a stale device connection. Its `/status` response is not proof of a healthy
  phone. Stopping an automation session can close its phone tab.
- `tab-driver.py` tried attaching an existing inspectable page on 18142.
  Initial direct evaluation worked, but HTTP-handler evaluation hung even for
  trivial expressions. That process was stopped.
- `tab_probe.py` is an obsolete experiment: its last expression imports
  `/checks/iphone.mjs`, which **does not exist**. Do not run unchanged.
- `session.json` is a synthetic bridge session (`iphone-network`); its
  `browserVersion` field was filled from the OS version and is misleading.
- `failure-1793277412275.png` is a black/about:blank automation screenshot,
  not evidence of a failed Negs render.
- `events.jsonl` contains controller failures, not the two actual Wi-Fi runs.
- `preflight.json` predates the two runs; its zero completed tests / awaiting
  runner status is historical. `run-summary.json` summarizes the later runs.

No relevant phone crash report was retrieved. The local MobileDevice crash
report directory was absent. Avoid treating unrelated Mac diagnostic reports
or older iPad reports as evidence for this phone.

## Suggested next steps and completion criteria

1. Check server and phone discovery anew. Prefer a working ordinary-Safari
   Wi-Fi path while investigating inspection. Avoid prolonged repeated
   connection/controller attempts that already failed identically.
2. If possible capture the matching phone crash/Jetsam report, or Web Inspector
   console/device-loss information during an isolated reproduction.
3. Compare verified default WebGPU with forced CPU and single-thread operation
   using the same RAW. Preserve separate logs and tested asset identities.
4. If a fix is justified, inspect the actual GPU/source memory lifetime before
   changing policy. Recheck smoke, rapid edits, zoom, prolonged editing, export
   and an edit after export. Require presented frames and actual JPEG blobs;
   startup success alone does not satisfy testing.
5. If connection attempts remain unsuccessful, report exactly which route is
   unavailable and what Wi-Fi tests did accomplish. The owner already offered
   to buy another cable; do not claim cable failure is proven.

Potential code starting points, not a diagnosed fix: `web/app.js` render
queue/`renderSide` (touch GPU cap 12 MP, fitted phone frames under 1 MP), WASM
`Engine.load`/`source_at` in `crates/spektrafilm-wasm/src/lib.rs` (full-resolution
f64 RAW source plus resampled copies), GPU input/transient buffer lifetimes in
`crates/spektrafilm-gpu/src/wgpu_backend.rs`, and `capacity.rs` browser admission
(buffer limits, no process memory budget). Existing CPU FFT allocation guards
do not establish GPU safety.

## Preserve the checkout and evidence

The checkout was already dirty across Rust, app, server, and test files. Do not
reset, clean, or overwrite it. This iPhone session only added the device-type
option in `web/safari.mjs`, updated `docs/apple-device-testing.md`, and saved
ignored test artifacts. No app crash fix, commit, or deployment was made.

Full background: `docs/apple-device-testing.md`, `docs/ipad-stability.md`.
The earlier successful iPad CPU suite is a different device/backend. Likewise
`device-mvtd24gg-690ec874.jsonl` was a desktop Chrome preflight, not an iPhone
pass. Preserve both actual failed iPhone logs and `target/iphone-tests/`.

Processes alive at handoff (recheck before acting): server PID 38164,
safaridriver PID 38126, experimental network bridge PID 38462.
