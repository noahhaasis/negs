# Physical iPad stability investigation — 2026-10-29

Reusable setup lessons, failure classification, confidence, and the next device
matrix: [Apple device testing](apple-device-testing.md).

## Device and inputs

- Physical iPad Air (5th generation), iPadOS 18.2.1, Safari 18.2.
- Safari WebDriver reports `safari:useSimulator: false` and `safari:deviceType: iPad`.
- CPU (rayon), eight threads. WebGPU is unavailable on this device's current Safari.
- Fuji X100V RAF: `~/Downloads/0955097116.raf`, 56,201,536 bytes, decoded 6240 × 4160.
- SHA-256: `9d5ff2f21f466b9e3177cbc61969ff6a9faa6bf4ac25bf99e2c33089be82faab`.
- Hosted baseline: <https://silent-boat-5aea.haasis-noah.workers.dev/>, build-info revision `a0bf203837eb146d45d0ac39edc07b6424fab14d`.
- Local source started at `86949b5`. The hosted JS and WASM differ from the local build; results must be attributed separately.

The test fixture and diagnostic artifacts are gitignored. Nothing was deployed.

## Confirmed failure and fix

On the physical iPad, the Fuji loaded and ordinary slider edits succeeded. With
the combined large-effect settings in `web/checks/ipad.mjs` (`heavy`), a
full-resolution export failed with an engine panic:

> capacity overflow … library/alloc/src/raw_vec/mod.rs:28

The page restarted the worker and displayed an export failure. This is a failed
test, even though Safari itself remained open.

The exact CPU diffusion path builds reflect-padded f64 complex FFT planes.
For this image and the maximum PSF radius (2079), one plane requires
2,916,000,000 bytes. That already exceeds wasm32's maximum single Vec allocation;
convolution additionally holds a second plane and a transpose.

The WASM CPU renderer now queries the model's actual PSF before resampling or
constructing it. It rejects edits requiring a complex FFT plane larger than
512 MiB, leaving room for the other planes, PSF, decoded image, and pipeline in
the 4 GiB WASM address space. Both camera and print diffusion are checked; print
diffusion is skipped when scanning film. Inactive and zero-strength filters do
not consume this budget.

The UI displays an actionable memory-limit message and keeps the engine and
preview. The renderer does not silently reduce export resolution or approximate
the filter. Full-resolution diffusion exports outside this budget remain
unsupported in the browser; smaller spatial scales or the desktop app are the
current alternatives.

The iPad retest of the failing edit produced the limit message immediately,
kept `engineInstances() === 1`, recorded no panic/error, and completed the next
exposure edit. Normal export is checked separately from an expected limit.

A later lifecycle test found a second failure: explicitly selecting WebGPU on
Safari 18.2, which has no `navigator.gpu`, caused unnecessary worker replacement
and an `Out of memory` worker error. `startEngine` now detects the absent API
before allocating a worker and retains the CPU engine. Selecting the already
active backend also does nothing. The physical-device retest passed backend
selection, repeated imports, ten editing rounds, and two full-resolution exports
with the same engine instance. The failed session was closed before retesting.

## Verification so far

| Check | Result |
| --- | --- |
| Hosted Fuji import and first real engine render | Passed |
| Hosted native slider drag, −5/+5 EV, rapid input, reset | Passed |
| Hosted full-resolution JPEG generation | Passed; 5,630,749-byte JPEG |
| Hosted editing after export | Passed; WASM high-water mark 3231 MiB |
| Local native slider drag, endpoints, rapid input, reset | Passed before allocation fix |
| Local large-effect preview | Passed before and after fix |
| Local previously crashing export and continued editing | Passed as a guarded limit; no worker restart |
| Full 207-case control sweep | Passed all 207 cases plus reset in one 24-minute run; no restart/error/reload; memory peaked at 1377 MiB |
| 100% zoom, editing zoomed, return to fit | Passed |
| Repeated imports, backend-selection fix, Balance Print, typed values | Passed; same engine instance |
| Prolonged session | Passed ten rounds across four stocks, with diffusion/halation/glare |
| Two local full-resolution exports after prolonged editing | Passed; queued edits preserved; memory peaked at 3627 MiB; no engine restart |
| Single-threaded physical-device comparison | Passed Fuji import, native slider drag, exposure endpoints, rapid edits, reset; 1036 MiB; same engine instance |
| Rust engine regression suites | 336 passed, 0 failed, 3 ignored |
| `cargo check --workspace --all-targets` | Passed |
| Both single-threaded and threaded WASM builds | Passed |
| Reference card, 512 px CPU render before/after | Byte-identical PPM |

WASM `memoryMiB` is linear memory's high-water mark, not the complete Safari or
device memory footprint. A placeholder is not counted as a completed render.

The native Metal test was also corrected to check only the single `main`
compute-entry shaders its pipeline helper supports. Mixed display shaders use
their own layouts. This change is confined to the test.

## Connectivity and device diagnostics

At 09:30:18 UTC the control sweep lost its Safari window during the +5 EV case,
without a recorded engine panic. The owner confirmed the iPad was untouched.
A subsequent session also disappeared. Apple device services, usbmux, and the
Mac USB registry subsequently reported the iPad unavailable/absent. The owner
restored the USB connection and the sweep was restarted.

Only relevant reports from the device were copied, preserving originals:

- The available JetsamEvent is dated 09:26:42 **local time**, before these tests,
  and names another process as its victim. WebKit being the largest process in
  that report does not make it the victim.
- A WebContent CPU-resource report covers 10:12–10:14 local time and says
  `Action taken: none`.
- No available WebKit crash or memory-kill report matches the later window
  interruptions. This does not prove that no process termination occurred.

Filtered device events are recorded during the resumed run. Connectivity
interruptions are kept distinct from completed product tests.

## Repeating the physical-device tests

Start the HTTPS server with certificates outside `web/`:

```sh
NEGS_DEV_CERTS=/private/tmp/negs-ipad-certs bash web/dev-cert.sh
NEGS_DEV_CERTS=/private/tmp/negs-ipad-certs node web/serve.mjs 8443 --https
safaridriver --port 18139
```

The certificate script discovers LAN addresses on macOS and Linux using Node.
The dev server sends `Cache-Control: no-store` so reloads use one build's JS
and WASM. `NEGS_DEV_HOST=127.0.0.1` binds a Mac-only preflight server to loopback.

Enable Web Inspector and Remote Automation on the iPad, trust the Mac, keep it
unlocked, and disable Auto-Lock. Then:

```sh
SAFARI_DRIVER_URL=http://127.0.0.1:18139 SAFARI_DEVICE_UDID=<device-udid> node web/safari.mjs create --insecure
node web/safari.mjs navigate https://<Mac-LAN-IP>:8443/
node web/safari.mjs open-url results/0955097116.raf
IPAD_NATIVE=1 node web/safari.mjs run web/checks/ipad.mjs
IPAD_PHASE=extremes node web/safari.mjs run web/checks/ipad.mjs
IPAD_PHASE=zoom node web/safari.mjs run web/checks/ipad.mjs
IPAD_PHASE=heavy node web/safari.mjs run web/checks/ipad.mjs
IPAD_PHASE=limited-export node web/safari.mjs run web/checks/ipad.mjs
```

`IPAD_CASE_START` can resume a control sweep after a confirmed connection
interruption. Cases reset parameters individually. A product panic or reload
must be investigated, not skipped. `soak`, `export`, and `after-export` are
additional phases; reset the edit before a normal full-resolution export.

The runner injects the RAF into the file input and dispatches its normal change
handler. This exercises the editor's import path but does not test the native
Files picker. Native gestures are explicitly separated from programmatic input.
Portrait/landscape changes, background/resume, native two-finger pinches, and
native file selection were not verified by this automation. WebGPU and the
iPhone were not tested. These results establish the tested behavior on this
iPad/Safari version, not a guarantee for every device or possible edit.

## Wi-Fi runner

`https://<Mac-LAN-IP>:8443/checks/ipad.html` runs the same cases in an iframe on
the device and saves each action to the Mac before performing it. It includes a
screen wake lock where Safari supports one. The iPad must trust the local HTTPS
certificate; no system trust settings were changed by the automated tests.

Tap **Start tests** in ordinary Safari. The default sequence loads the Fuji,
runs slider/control/zoom/limit tests, ten editing rounds, then normal export and
post-export editing. An unexpected reload of the test page is logged rather
than automatically retried. Logs stream to `web/results/device-<run>.jsonl`.

A desktop Chrome preflight with `?phases=smoke&image=results/card.jpg` passed
seven actions, produced 18 log events including pre-action records, and had a
clean console. This validates the runner; it is not an iPad stability result.

## Artifacts

- `target/ipad-tests/events.jsonl`: USB-driver action log, including failures.
- `target/ipad-tests/build-comparison.json` and `hosted/build-info.json`: build identity.
- `target/ipad-tests/crash-params.json`: exact edit from the confirmed export panic.
- `target/ipad-tests/*.png`: device screenshots and runner preflight.
- `target/ipad-tests/engine-tests.log`, `workspace-check.log`: native checks.
- `target/ipad-tests/crashes/`: relevant copied device reports.
- `target/ipad-tests/device-events.jsonl`: filtered live device diagnostics.
- `web/results/device-*.jsonl`: Wi-Fi runner events.
