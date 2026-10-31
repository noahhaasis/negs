# iPhone/iPad stability follow-up — 2026-10-31

Continuation of [the bounded RAW regression handoff](iphone-leica-bounded-regression-handoff.md).

**Status: the final candidate passed two short physical iPhone sequences;
NOT a complete reliability sign-off or a deployed release.**
After the earlier failed session requests, the user unlocked the iPhone and
asked to retry. The final revision passed the 24 MP default-route control and
60 MP Leica bounded sequence, including zoom/edit, export and post-export
reopening. Earlier iPad results remain separate: its first-candidate Leica
sequence lost its session, and the final revision is still untested on iPad.
Those failures are preserved below, not replaced with the later iPhone pass.

At the end of testing, no commit, push, deployment, device settings change,
reboot, or user-app termination had been performed. The user subsequently
requested a source commit. Bounded RAW development remains opt-in. Do not
enable it automatically or claim that iPhone stability is established.

## Checkout and build identities

Branch `wasm-investigation`, starting at `8693fcc`. Preserve the pre-existing
changes to `scripts/prepare-web-samples.py`, `web/sample/README.md`,
`web/sample/catalog.json`, and the untracked user documents/files/public folder.
The root `public/` was NOT replaced.

Ignored evidence root: `target/iphone-stability-20261031/`.

- `negs-hosted-build-info.json`, `negs-hosted-headers.txt`: public deployment.
- `hosted-verified-assets.json`: six actual hosted JS/WASM SHA-256 values
  matched that manifest. Public source revision was `8693fcc`.
- `release-candidate/`: first candidate, physically tested on the iPad.
- `release-candidate-v2/`: final candidate, locally tested and now short-run
  tested on the iPhone; still untested on iPad. Use its `build-info.json` and
  the evidence root's `source-hashes.json` for identity. The packages were
  built from uncommitted changes; their embedded pre-commit Git revision
  alone does not identify the tested runtime.
- `public/` under the evidence root is an earlier intermediate package,
  superseded by the two explicitly named release candidates.

Temporary proprietary fixture symlinks were removed from BOTH packages.
Neither release package contains the Leica/Sony fixtures or device certificates.

## Implemented fixes

### First candidate (physically exercised)

1. **Explicit WASM result destruction** in `web/engine.js`. Render, Original,
   border, and Balance Print results owned Rust allocations even after their
   getters copied pixels into JS. They now call `free()` deterministically,
   including missing-data retries and getter failures. Preset result cleanup
   is also protected with `finally`. This removes dependence on worker-side
   `FinalizationRegistry` scheduling; it is not proof all tab memory is live
   Rust allocations.
2. **GPU buffer lifetime cleanup** in
   `crates/spektrafilm-gpu/src/wgpu_backend.rs`: clear the render pool when
   forgetting a photograph; release obsolete same-label buffers before
   allocating replacements; explicitly destroy grain reduction buffers after
   submitting their work. Previously those reductions waited for JS GC.
3. **Release the JPEG encoding canvas** in `web/app.js`, resetting its dimensions
   in `finally`, after the JPEG has its own encoded bytes, also on failure.
4. Evict other pooled sizes before allocating an oversized render rather than
   waiting until that render finishes. The first revision still discarded the
   entire oversized set afterward; the final revision below fixes that cliff.

### Final revision — local and short physical iPhone verification

The physical trace showed a large second-zoom spike despite small engine WASM.
Four 12 MP RGB buffers need about **549 MiB**, just beyond the existing
**512 MiB** pool limit. Previously the whole set was evicted: every zoom edit
allocated all four again, even when Safari had not physically reclaimed the
preceding buffers.

- The pool now retains **partial sets within the SAME 512 MiB limit**, newest
  first, instead of evicting an entire oversized set. It preserves reusable
  buffers including the scan used for Post edits. `take()` reuses a matching
  partial set and evicts *other* sizes before a large render.
- New `releaseRenderBuffers` WASM/client API drops GPU scratch and uploaded
  inputs **before** a capped-path decoder starts. Unlike `forget`, it keeps
  the CPU source and display textures. A failed RAW decode can still restore
  the existing edit/display without pretending the old source was forgotten.
- No quality cap, RAW pixel algorithm, native/default decode route, or pool
  byte limit was increased. Small fitted-frame pooling is retained. There can
  still be upload/reallocation costs after explicitly releasing caches; no
  claim of literally zero timing regression is made.

## Physical device evidence

### iPhone: initial blockers, then two short final-candidate passes

USB discovery identified `iPhone14,3`, iOS 26.6.2, UDID
`00008110-000240403AD1801E`. The initial Safari session timed out while requesting
an automation session. No app import/render was performed in that attempt.
The later successful physical tests below are distinct from the local tests.

**User-requested iPhone retry, 2026-10-31 12:42 UTC:** discovered the same
phone over USB and network. A fresh, screenshot-free Safari session request
failed with **`device is locked`**, before any app page opened. Stopped the
assistant's new driver; no reconnection loop, settings change or app test.
Evidence: `iphone-retry-v2/` under the evidence root. Ask the user to unlock
the iPhone and leave Safari foregrounded before another attempt.

**User-requested retry succeeded, 12:45–12:49 UTC.** Safari 26.6.1 created real
sessions on the same iPhone with **WebGPU / 4 threads**, secure context,
cross-origin isolation and the unchanged 12 MP working-copy cap. All
**169 packaged asset hashes** were verified before opening the candidate.
No runtime source/build changes were needed after the earlier local tests.

Each fixture used a fresh automation tab, with editing/export/reopening in
that same tab. Both passed: open; -5/+5/0 EV; rapid exposure inputs; Reset;
zoom/edit/fit/Reset; JPEG export; post-export edit/Reset; reopen; Original.
Each kept **one page epoch and one engine**, recorded **22 frames** with
presentation checked at completed stages, and no JS error, panic or visible
editor error. Final probe on Leica confirmed
it was still idle and healthy before explicitly ending the session.

| Final-candidate physical test | 24 MP linear DNG | Actual 60 MP Leica Q3 |
|---|---:|---:|
| Actual route (both opens) | legacy, default query | bounded, explicit opt-in |
| Worker decode times | 691 / 629 ms | 2177 / 2052 ms |
| Import-to-first-frame, excluding download | 1424 / 1225 ms | 2963 / 2631 ms |
| JPEG bytes | 3,629,306 | 2,765,063 |
| Sampled WebContent peak | **1372.5 MiB** | **1724.1 MiB** |
| First zoom peak | 1257.3 MiB | 1724.1 MiB |
| Following zoom edit peak | 1207.3 MiB | 1277.9 MiB |
| Export peak | 1372.5 MiB | 1579.5 MiB |
| Post-export reopen peak | 1260.8 MiB | 1320.4 MiB |

Active WebContent PIDs are inferred from the fresh session's new high-footprint
process and activity: **9485** (control), **9500** (Leica). Samples were about
0.5 s apart; short peaks can be missed. Other Safari processes were sampled,
but not interacted with. Both exports are from the existing **12 MP working
copy**, not full 24/60 MP exports; the helper's historical “full-resolution”
step label does NOT mean full source resolution. The “zoom Fuji” label is also
generic/historical: it operated on the fixture listed in that run, not a Fuji.
Engine WASM was 211 MiB before export and 303 MiB afterward. Decoder WASM was
604 MiB for the control, 543 MiB for bounded Leica; these are not physical
process-memory figures. Leica's 12–14 s open steps include the large LAN
fixture transfer; they are NOT decode latency.

Evidence: `iphone-retry-v2-2/` and `iphone-q3-v2/`, including raw events, syslog,
footprint, fixture hashes, build identity, per-run `summary.json` and
`stage-memory.json`; combined `iphone-device-summary.json` at the evidence
root. Both sessions were explicitly ended, capture jobs/driver/server stopped,
and the temporary Leica fixture symlink removed from the release package.

**Scope:** two short successful sequences, not a matched baseline/candidate
comparison, soak test, native Files/gesture check, CPU/single-thread test, or
proof that previous jetsams/long-session retention are eliminated. Lower
figures than the earlier iPad run do not isolate the pool fix: hardware,
viewport, GC behavior and sequence differ. Keep bounded opt-in and retain all
prior failures. The complete historical four-open Leica sequence was not run.

### iPad: actual first-candidate tests

USB `iPad13,16`, UDID `00008103-001405890204C01E`, iPadOS **26.7.2** (23H128),
Safari **26.6.3**. Trusted local HTTPS static package, secure and isolated,
coarse pointer, **WebGPU / 8 threads**, existing 12 MP working-copy cap.

All device captures were screenshot-free. URL imports exercise the app's
import path, **not the native Files picker**. A new automation session/tab was
used for each fixture, with reopens deliberately in the same tab.

| Fixture / evidence directory | Coverage | Outcome |
|---|---|---|
| 24 MP linear DNG, `ipad-smoke/` | Open; -5/+5/0 EV; rapid inputs; Reset; zoom/edit/fit; JPEG; post-export edit/Reset; reopen; Original | **Passed**, one engine, presented frames, no monitor loss or logged error |
| Actual Leica Q3, bounded, `ipad-q3/` | Open twice; zoom/edit/fit/Reset; JPEG; post-export edit/Reset | Completed these stages, then **session lost on open-3**; incomplete, not a pass |
| Final revision, `ipad-q3-v2/` | Session creation only | **Blocked: Web Inspector is not enabled on device. No RAW opened.** |

24 MP presented frames were 1512 x 1008 fitted and 4242 x 2828 zoomed. The
JPEG was **3,629,336 bytes**. Engine WASM rose from 231 to 323 MiB after export.
The active test WebContent process was PID 2165 (inferred from its new-session
appearance/activity; other sampled WebContent processes stayed small).

Leica dimensions were 9520 x 6336, with 1512 x 1006 fit and 4246 x 2826 zoom
frames. Both opens logged the actual **bounded** route, decoder WASM **551 MiB**,
worker decode **1698 / 1656 ms**, and import-to-first-frame **2456 / 2158 ms**.
The roughly 8.8 s driver open steps include fetching 70 MiB over the LAN and
polling; do NOT call those decoder latencies. The JPEG was **2,764,991 bytes**
from the existing **12 MP working copy**, NOT a full 60 MP export.
Engine WASM was 231 MiB before export, then 277 MiB.

Sampled WebContent footprint (MiB; OS samples about 0.5 s apart despite the
250 ms requested interval, so brief peaks can be missed):

| Stage | 24 MP, PID 2165 | Leica bounded, PID 2236 |
|---|---:|---:|
| First open | 772 | 1030 |
| Second pre-zoom open | Not in this short control | 1395 |
| First zoom | 1279 | 1478 |
| Zoom edit | 1869 | 2013 |
| Return to fit / Reset | 1880 / 837 | 1943 / 2129 |
| Export | 1566 | **2806** |
| Post-export reopen | 1147, completed | 1608 sampled before disconnect; incomplete |

Leica PID 2236 is correlated with the new Safari WebContent launch in syslog
and the active process's footprint. Its 2806 MiB peak is about **2.74 GiB**.
The GPU process also reached 461 MiB; it logged a **non-fatal** 300 MB soft-limit
warning, not a recorded kill. Do not add/interpret these metrics as if all OS
attribution and shared-memory accounting were established.

At **05:52:41 UTC**, WebDriver returned `invalid session id`; both syslog and
the instruments tunnel disconnected too. No matching kernel WebContent kill
was captured. The iPad remained discoverable over USB afterward. This does
**not** establish either a jetsam or a harmless controller-only failure.
Preserve `sequence.log`, `events.jsonl`, syslog, sampler tracebacks and
`stage-memory.json`. Do not retry until a pass and discard this attempt.

The next and only attempted final-revision **iPad** session reported Web Inspector
not enabled. No setting was changed by the assistant; no repeated toggles or
reconnection loop was attempted. Device jobs were stopped.

## Local verification and its limits

- JS lifetime regressions reproduce on the original client and pass on the
  patched client, including missing-data fetch failures and throwing getters.
- A synthetic **80 x 1024 x 768 Original** burst against the SAME WASM binary:
  original JS grew WASM from **17.75 to 259.69 MiB**; patched JS from **17.75 to
  20.81 MiB**. Identical RGBA checksum `493689145`. This is a synchronous
  lifetime diagnostic, not a phone/process-memory benchmark or GC guarantee.
- First candidate: Chrome touch-viewport diagnostic recorded explicit GPU
  buffer create/destroy high-water **836.6 -> 799.5 MiB** versus the pre-edit
  root build. This excludes textures, staging, WASM, and delayed physical
  reclamation. The final revision retains more reusable buffers on purpose;
  its retained byte count is NOT a live physical-process measurement.
- Three Chrome touch-viewport open/zoom/edit/export/reopen cycles passed in
  one page, with three nonempty JPEGs, no engine replacement, no logged error,
  released export canvases and zero undestroyed grain reductions. This also
  passed with the final pool revision. The later short physical iPhone passes
  above do not establish equivalent long-session/soak behavior.
- Final revision passed **37 browser WB/recovery checks**, including invalid-WB
  rollback, export, Original, concurrent opening, and backend changes.
- Actual Q3 single-thread WASM: original saved package versus current legacy
  and opt-in bounded output, **zero changed Float32 scalars / max error 0**.
- Browser pixel SHA-256 comparisons against the actual hosted deployment:
  default, exposure/Post, diffusion and daylight, rendered bytes plus Original.
  Both the first candidate and final revision matched **8 GPU + 8 CPU** hashes.
  The final revision's actual Q3 legacy/bounded scalar checks also passed.
- Real Chrome JPEG success -> injected encoder failure -> edit -> JPEG retry
  passed, with the same engine/photo and every temporary canvas reduced to
  1 x 1. GPU control used 12 MP RAW; single-thread CPU control used a 512 x 384
  synthetic PNG. An earlier full-size single-thread CPU attempt was interrupted
  by the host command timeout: **incomplete, not a crash or export pass**.
- Both WASM packages rebuilt successfully. Native IO tests: **103 passed,
  2 ignored**. Final native GPU tests: **24 passed, 1 ignored**. Final JS unit
  tests: **55 passed, 1 optional fixture test skipped**. Syntax checks,
  **169 packaged asset hashes**, and `git diff --check` passed. Exact final
  results are in `final-summary.json` and the corresponding logs.

These results do not resolve the earlier native Sony latency tails and are
not a new native original/candidate decode-timing comparison. Keep the previous
handoff's caveats. No no-regression claim or full-resolution browser export
claim is justified.

## Safe resumption

1. Confirm the target device is usable, Web Inspector/Remote Automation available,
   and the user wants another short device run. The previous run's simultaneous
   controller/capture disconnection is unresolved; do not assume a crash cause.
2. Use **`release-candidate-v2`**, not the physically tested first candidate.
   Serve it with trusted HTTPS and no cache; verify hashes, backend, cap,
   actual route, frame presentation, one engine and one page epoch.
3. First run a short default 24 MP control, then one fresh-tab Leica bounded
   sequence. Save correct-PID footprint and syslog, including sampler gaps.
   Stop on usability or controller trouble; do not queue dozens of sessions.
4. Specifically compare repeated zoom edits, export and post-export reopen.
   The short iPhone runs now provide physical measurements, but not a matched
   baseline proving which fix reduced repeated-allocation overlap. Retained
   GPU memory can make a later decode worse if reclamation is delayed; measure
   repeated cycles and warm-session behavior too.
5. Complete native Files import, touch gestures/backgrounding, CPU/single-thread
   and a matched physical baseline/candidate comparison separately. iPad
   results never establish iPhone safety.
6. Do not deploy or label a stable release until the unresolved physical
   memory/reopen behavior is measured and accepted. No default enablement of
   bounded RAW is authorized by this work.

All assistant device samplers, syslog captures, drivers and test servers were
stopped. The pre-existing root server on **8137** was left running. No device
certificate keys were copied into the repository or release packages.
