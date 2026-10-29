# Apple device testing: lessons and next runs

Recorded 2026-10-29. Detailed results and artifacts: [iPad stability investigation](ipad-stability.md).

## Physical iPhone WebGPU runs: two unexpected reloads

On 2026-10-29, a physical iPhone reporting `iPhone14,3`, iOS 26.6.2
(23G90), opened the local build at `https://192.168.178.88:8443/`.
Direct Web Inspector evaluation confirmed `isSecureContext === true`,
`crossOriginIsolated === true`, `navigator.gpu` present, and the actual engine
label `wgpu (GPU) · 4 threads`.

Safari's driver could not discover the
phone; a network pairing through usbmux allowed page creation and direct
inspection, but later script commands timed out and the device disappeared
from usbmux. The user also reported that Finder could not see the phone.
These controller/connectivity failures alone do not establish an engine crash.

The user then started the Wi-Fi runner twice in ordinary Safari. Both runs
imported the 56.2 MB Fuji RAW at 6240 × 4160 and completed GPU renders with
`presented: true`, at a fitted size of 1212 × 808. The browser user agent
reports Safari `Version/26.6.1`; the device service reported iOS 26.6.2.

| Run | Completed coverage | Last requested action | Outcome |
| --- | --- | --- | --- |
| `mvtjn4wf-65ef1c8b` | Import; −5/+5/0 EV; rapid inputs; reset; 12 renders | Zoom Fuji to 100%, 12:53:34 UTC | Unexpected page reload at 12:53:35 UTC |
| `mvtjog4m-6f2bcc3a` | Import; −5/+5/0 EV; 4 renders | Rapid exposure inputs, 12:54:18 UTC | Unexpected page reload at 12:54:19 UTC |

The owner confirmed both reloads happened automatically, without a manual
refresh or navigation. Both runs are failures. Neither reached soak or export.
There was one engine instance and no recorded JavaScript error or panic before
each reload; this does not rule out WebKit/GPU process termination. The last
completed frames reported 1044 MiB of WASM linear memory, which is not total
physical browser/GPU memory. Memory pressure is a working hypothesis, not a
confirmed cause without a matching device crash or Jetsam report.

The current touch GPU preview cap is 12 MP, but the fitted frames above were
under 1 MP. A zoom request can increase the working set substantially. Browser
GPU admission currently checks buffer limits without a process-memory budget.
The second failure during ordinary rapid inputs means a zoom-only mitigation
would not establish stability.

The host records are in `target/iphone-tests/`: `preflight.json` contains
the device identity and local asset/fixture hashes; `events.jsonl` records
failed driver/controller attempts. The two physical-device run logs are
`web/results/device-mvtjn4wf-65ef1c8b.jsonl` and
`web/results/device-mvtjog4m-6f2bcc3a.jsonl`. Preserve both failed runs when
evaluating a fix; completed export and prolonged-editing coverage remain open.

## Physical iPad WebGPU runs: full suite passes after an engine-switch fix

Later on 2026-10-29 (about 14:02–14:25 UTC), the same iPad Air 5
(`iPad13,16`, UDID `00008103-001405890204C01E`) had been updated to iPadOS
26.7.2 (23H128). Apple's `safaridriver` created a session over USB once Web
Inspector and Remote Automation were re-enabled after the update; the session
reported Safari 26.6.3. The user agent reports macOS (`Intel Mac OS X
10_15_7`), so as on the iPhone, take versions from the device service and
session, not the UA. The app selected `wgpu (GPU) · 8 threads` automatically,
with secure context and cross-origin isolation, and the Fuji RAW rendered as
presented 1512 × 1008 fitted frames at 1052 MiB of WASM memory.

| Phase | Outcome |
| --- | --- |
| smoke (−5/+5/0 EV, rapid inputs, reset) | Pass, the iPhone's second failure point |
| zoom ("100%", clamped to 68% by the 12 MP touch GPU cap; edit; fit) | Pass, the iPhone's first failure point |
| heavy, soak, extremes (207 control cases) | Pass |
| export, after-export | Pass: 5,632,535-byte JPEG blob, then edits |
| lifecycle | Failed, then passed after the fix below |

The page kept one epoch within each tab (no reload), every frame had
`presented: true`, and passing runs logged no error. Phases ran one at a time
through `web/safari.mjs` with `IPAD_PHASE`, not the outer Wi-Fi runner.

Lifecycle selects CPU and then immediately GPU. On iPadOS 18 the GPU selection
was a no-op; with WebGPU it really replaces the engine, which exposed two races
in `startEngine` (`web/app.js`): the replaced CPU engine's photo reload
rejected with `engine restarted`, shown as an error, and, once that was
ignored, the first selection still scheduled a render on the new engine before
its photo load, failing with `no image loaded`. A superseded start inside the
`try` would also have shown a false "WebGPU isn't available" and fallen back to
CPU. A replaced start now returns `false`, and the selection handler leaves
rendering to the start that replaced it. Lifecycle, smoke, export, and
after-export then passed in a fresh session.

**Open: memory retained across engine lifetimes in one tab.** After several
engines in a single automation tab (navigations plus switches), WebGPU began
failing with `Out of memory` (from the GPU stack, not app code); eventually even
opening the RAW on a freshly navigated page failed. Ending the session (closing
the tab) and creating a new one recovered it, matching the earlier iPadOS 18
note that navigation did not restore startup but a new session did. One slow
CPU→GPU switch in a fresh tab passed. This plausibly relates to the iPhone
reloads (6 GB versus this iPad's 8 GB), but that is unconfirmed. Logs:
`target/ipad26-tests/` (`events.jsonl`, `run-<phase>.log`; `run-lifecycle-2.log`
and `run-slow-switch.log` record the out-of-memory failures).

## What the completed run establishes

The fixed local build was exercised on a physical iPad Air (5th generation),
iPadOS 18.2.1 / Safari 18.2, with a 56.2 MB Fuji X100V RAF (6240 × 4160).
The engine used CPU rendering with eight threads; single-threaded import and
slider behavior were also checked.

- All 207 control boundary/choice cases plus reset passed in one 24-minute run.
- Native slider input, 100% zoom, typed-value validation, Balance Print,
  repeated imports, and replacing a photo during rendering passed.
- Ten rounds across four film stocks and heavy effects passed.
- Two full-resolution JPEG exports after prolonged editing passed, including
  retaining edits queued during export.
- The post-fix lifecycle/export session kept one engine instance throughout.
- The control sweep reached 1377 MiB of WASM linear memory. Repeated export
  reached 3627 MiB. These are high-water marks, not total or current physical
  browser memory measurements.

These results apply to the patched local checkout. The public deployment tested
as the baseline was an older build and has not received these fixes.

## Failure patterns to keep in regression coverage

### Preview success does not establish export safety

The large-effect preview worked, but the same edit exported at full resolution
requested a 2,916,000,000-byte complex FFT plane. One allocation exceeded
wasm32's single-Vec limit; convolution needs additional planes and PSF storage.

Keep both a normal full-resolution export test and an expected memory-limit
test. For a guarded request, require a clear message, no engine restart, and a
successful subsequent edit. An unavailable export is a documented limit, not
successful JPEG generation. Do not silently lower resolution to make a test pass.

The current FFT guard applies to the **CPU** WASM path. It is not proof that the
GPU path stays within a phone's GPU/process memory limits.

### Unsupported backend selection must not allocate another pool

Selecting WebGPU on Safari without `navigator.gpu` produced an out-of-memory
worker failure during unnecessary replacement. Keep tests for:

- Selecting the already active backend: retain the engine.
- Selecting GPU when the API is absent: retain the working CPU engine.
- GPU API present but adapter/initialization unavailable: test separately;
  the API-absent fix does not establish this case.
- Switching supported backends repeatedly with a large RAW loaded.

After the failure, a page navigation did not restore startup reliably. Closing
the failed automation session and starting a new one did. Record this recovery
as part of the failure; do not discard it and count only the later successful run.

### Recovery is not a crash-free pass

Fail a run on an unexpected worker replacement, panic, runtime trap, tab reload,
blank canvas, or unresponsive editor even if automatic recovery follows.
Log the requested edit before submitting it. A fatal operation may never append
a completed render record. Check both image/parameter state and completed
renders; a placeholder alone is insufficient.

## Reliable device setup and tooling

1. Record hardware, OS/Safari, build identity, fixture checksum/dimensions,
   actual backend, thread count, and preview dimensions. Save hosted
   `build-info.json` and local asset hashes; do not mix their results.
2. Preflight `/catalog.json`, JS, WASM MIME types, and required data files.
   This checkout initially lacked the generated catalog, which looked like a
   stuck startup. `node web/make-catalog.mjs` restored it from the prebuilt engine.
3. Use trusted HTTPS and COOP/COEP for device testing. Verify `isSecureContext`
   and `crossOriginIsolated` in the device, rather than assuming LAN HTTP is
   equivalent to localhost. Keep certificate keys outside `web/`.
4. Enable iPad/iPhone Web Inspector and Remote Automation. Pair/trust the Mac,
   keep the device unlocked, disable Auto-Lock, and use a stable direct data
   cable. Safari's built-in WebDriver sufficed for these browser tests;
   Appium/WebDriverAgent was not required.
5. Serve local builds with cache disabled. Replace/rebuild matching JS and WASM
   together, and verify that the tested page actually loaded the new build.
6. Use `web/safari.mjs` with `web/checks/ipad.mjs`. Keep its host-side JSONL log.
   `engineInstances()` helps distinguish planned backend changes from recovery.
   The initial controller and temporary processes can disappear between turns;
   probe session/device state before dependent commands.

Native automation lessons:

- The controller defaults to iPad. For a physical iPhone session, set
  `SAFARI_DEVICE_TYPE=iPhone` and a separate `SAFARI_TEST_DIR=target/iphone-tests`.
  Keep those environment settings on subsequent controller commands so its
  session and results stay separate from the iPad run.

- Inspect control visibility and bounds before a gesture. A hidden slider
  returned a zero-sized rectangle; never send its coordinates as a real drag.
- Do not add an unnecessary `DELETE /actions` after an explicitly released
  pointer. That cleanup stalled this Safari 18.2 session.
- The runner's file-input injection exercises the editor import handler, not
  the native Files picker. Label programmatic, native, and manual coverage.
- Start a large asynchronous import, then poll from the host. Awaiting the
  whole import inside one WebDriver script hit the script timeout while the
  page continued working; that was a controller timeout, not a proven hang.
- Use the Wi-Fi runner at `/checks/ipad.html` for long ordinary-Safari runs when
  appropriate. Its pre-action logs reach the Mac through `/device-log` and an
  unexpected page reload is recorded. It still needs trusted HTTPS and physical
  device validation; the completed desktop preflight alone proves no iPad result.

## Classify failures before fixing them

| Observation | Evidence to collect | Avoid concluding |
| --- | --- | --- |
| WASM panic or trap | Exact parameters, instance, worker message, last requested frame | A recovered worker means success |
| Tab/window disappears | USB/device availability, Safari logs, crash and jetsam reports at the same time | Every lost WebDriver window is an app crash |
| Jetsam report exists | Timestamp, page size, and the process carrying `reason` | `largestProcess` necessarily names the victim |
| CPU-resource report | Time interval and `Action taken` | A CPU warning with `Action taken: none` proves termination |
| Slow CPU render | Main-thread responsiveness, frame progress, eventual completion | A slow settled frame is automatically a freeze |
| Memory metric rises | WASM high-water mark versus physical/process/GPU measurements | All retained linear memory is a live allocation leak |

The completed run had real USB interruptions. An earlier jetsam report named a
different victim and predated the tests; a WebContent CPU report took no action.
Neither explained a later interruption. Preserve unresolved classifications.
Copy relevant reports without erasing originals; filter live logs to the test.

## Confidence on newer Apple devices

This is a qualitative engineering assessment, not a measured failure rate.

| Target | Current confidence | Main reason |
| --- | --- | --- |
| Tested iPad, Safari 18.2, same RAW and covered operations | High within this scope | Physical-device regression evidence |
| Newer iPads, ordinary editing | Moderate, promising | Shared app logic is exercised, but browser/backend and memory behavior can differ |
| Tested iPhone, Safari 26.6.1 WebGPU editing | Stability failed | Two automatic reloads with the Fuji RAW: zoom and rapid editing |
| Other newer iPhones, ordinary editing | Limited pending device tests | The tested iPhone failed; resource headroom and browser/backend behavior vary |
| Large RAWs/full-resolution exports across iPhones/iPads | Limited until tested | Already high WASM usage on the iPad; GPU path and process budgets are unmeasured on phones |
| Safari 26+ WebGPU stability | Failed on the tested physical iPhone | Real GPU renders succeeded, but both ordinary-Safari test runs reloaded before completion |
| Tested iPad, iPadOS 26.7.2 / Safari 26.6.3 WebGPU, same RAW | High within this scope, per fresh tab | Full suite passed after the engine-switch fix; memory builds up across engine lifetimes in one tab |

Safari 26 added WebGPU. The app automatically selects GPU when its capability
probe succeeds, so an OS upgrade can change the renderer even on the same
hardware. The touch GPU preview cap is 12 MP; the CPU uses a roughly 4 MP
baseline policy, allowing a larger fitted frame when needed. A newer device
can therefore request more work, not merely execute the tested work faster.
More chip performance alone does not establish memory safety.

## Next test matrix, in priority order

1. A physical iPhone on the shipping OS/Safari: default automatic backend,
   then forced CPU, then `threads=0`, with the same Fuji fixture.
2. Done for the iPad Air 5 on iPadOS 26.7.2 (see above). Still open there:
   how many engine lifetimes one tab survives before `Out of memory`, and
   forced CPU / `threads=0` comparisons.
3. Another iPhone with a different resource class, followed by a larger-display
   iPad. Do not infer model RAM or per-process limits from branding.
4. On each device, exercise native Files import, two-finger pinch/pan, rotation,
   keyboard/value entry, background/foreground, and lock/unlock. These remain
   manual/native gaps in the completed automation.
5. Repeat heavy zoom/export/import cycles with other tabs/apps present and
   after a longer warm session. Include larger RAW dimensions, not just larger
   compressed file sizes. Capture GPU loss/validation errors and OS memory kills.
6. After deployment, run a smaller smoke suite against the exact public build
   and check its asset hashes and headers. A passing local build is not a
   production deployment check.

Use the existing `smoke`, `extremes`, `zoom`, `lifecycle`, `heavy`,
`limited-export`, `soak`, `export`, and `after-export` phases. Keep default-GPU
and forced-CPU results separate. For real GPU availability/failure tests, verify
that the resource guard and fallback behavior apply to the path actually used.

References checked 2026-10-29:

- [Safari 26 release notes](https://developer.apple.com/documentation/safari-release-notes/safari-26-release-notes)
- [WebGPU implementation status](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status)
- [Apple: identifying high-memory use with jetsam reports](https://developer.apple.com/documentation/xcode/identifying-high-memory-use-with-jetsam-event-reports)
