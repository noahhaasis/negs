# Bounded RAW regression follow-up — PAUSED, 2026-10-30

**2026-10-31 continuation:** the user subsequently authorized device work and
connected an iPad. See [the stability follow-up](iphone-ipad-stability-followup.md)
for local memory fixes, an actual 24 MP iPad pass, an incomplete iPad Leica
sequence, and later **short physical iPhone passes** on the final candidate
(24 MP default route and Leica bounded, including export/reopen). These are
not a full reliability sign-off. No default enablement or deployment was made.
The historical pause below is preserved rather than rewritten as successful testing.

**User requested a pause after reporting that they could not unlock their phone.**
Do not start device automation again until they explicitly confirm the phone is
usable and ready. Stop short of claiming no regressions or a reliability pass.
The user subsequently requested a **WIP commit**. The implementation, checks and
handoffs are checkpointed on `wasm-investigation` (commit subject:
`WIP: checkpoint opt-in bounded RAW development`). No push or default-route change
was made. Device testing remains paused; this checkpoint is not a reliability pass.

All assistant test servers (8443/8444), driver (18138), samplers, syslog captures
and benchmark jobs are stopped. Temporary fixture symlinks in the two isolated
worktrees were removed. The pre-existing root server on **8137 was left alone**.
No phone reboot, erase, settings change, or user-app termination was requested
through a tool. The user toggled Safari Remote Automation themselves once.

## Why this follow-up happened

The user conditionally requested integration if there were no regressions and
the change merely reduced the likelihood of the worst case. The previous report
compared two rebuilt routes, not an original production-allocator binary.
This follow-up adds a genuine original-versus-candidate comparison.

Read [the implementation/results](iphone-leica-streaming-prototype-results.md)
and [the original brief](iphone-leica-streaming-prototype-handoff.md) first.
The bounded RAW implementation is still **opt-in** (`?rawDevelop=bounded`);
ordinary desktop/default decoding is not intentionally switched to it.

## New evidence obtained

### Three immutable arms

- **A:** original commit `89af24c`, original saved JS/WASM packages.
- **B:** candidate code, legacy/default route.
- **C:** candidate code, bounded API enabled (>40 MP selection; small RAWs bypass banding).

Detached worktrees remain at ignored paths:
`target/q3-bounded/controlled/{original,candidate}`. They are build snapshots,
not additional feature branches. The feature branch remains `wasm-investigation`.
Its history now also includes the separate numbered-samples change `8cb9ca1`;
the frozen regression snapshots predate that change.
Candidate source and package hashes were verified against the working tree when
snapshotted. **Additional unrelated working-tree edits appeared later**; do not
assume all current root sources still match the frozen candidate or revert them.

The original server initially lacked the pre-existing untracked `credits.html`
that tracked `offline.mjs` expects. Both worktrees were supplied the identical
file and their 150 runtime assets preflighted. Early `controlled/phone/` attempts
failed/interrupted during this setup issue and are not usable app comparisons.

### Native production-allocator timing: medians reassuring, tails unresolved

Identical harness source was linked against original/candidate dependencies,
with matched release settings and four rayon threads. Each arm had a separate
persistent process, one warmup and balanced A/B/C ordering. No phone import or
other benchmark was running concurrently with the final native timing batch.

| Fixture | Samples/arm | Original A median | Current legacy B median | Flagged C median |
|---|---:|---:|---:|---:|
| Sony Bayer 24 MP | 100 | 272.7 ms | 274.1 ms | 274.0 ms |
| Linear DNG 24 MP | 30 | 141.2 ms | 141.2 ms | 141.8 ms |
| X-Trans 26 MP | 30 | 435.9 ms | 435.1 ms | 434.4 ms |
| Leica Q3 60 MP | 30 | 998.2 ms | 994.5 ms | **1255.0 ms** |

Paired median-ratio bootstrap 95% intervals (descriptive timing uncertainty,
not a formal proof of equivalence) for both rebuilt arms on the three small
RAWs stayed within approximately **−1.1% to +1.1%**. Q3 banding was **25.5%
slower** by paired median (interval 24.1–26.1%) in this experiment. The earlier
17% figure was a different run; do not erase or mix those measurements.

**Scope of that slowdown:** this is a **native Mac measurement of Q3 RAW decode,
development, reduction to 12 MP and orientation**, not a measured 26% iPhone
slowdown or a 26% slowdown of editing/export. The extra cost belongs to the
experimental bounded algorithm, not to phones inherently. Current browser routing
opts into it only on the capped, coarse-pointer path (normally phones/tablets),
with `?rawDevelop=bounded` and a selected RAW **>40 MP** by sensor/crop pixel count.
Small RAWs bypass banding; uncapped desktop and no-switch defaults remain legacy.
An explicit native/API caller can select banding too; benchmark `force` can also
band small Bayer RAWs. The unresolved Sony tails below mean we still cannot
promise literally zero performance regression elsewhere.

**Do not equate stable medians with no tail regression.** Sony p95 was
**366 / 556 / 606 ms** for A/B/C; mean was 298 / 314 / 316 ms. More long pauses
occurred in rebuilt runs, despite similar median process CPU time. The cause
is not established. An initial experiment shared one process between B/C and
also showed spikes; the separate-process follow-up did not eliminate them.
Do not dismiss the spikes as allocator history or claim they are proven code
regressions. Host had 32 GiB RAM, nonzero pre-existing swap, 76–78% reported free
memory and no recorded thermal warning. CPU/fault/context-switch metrics were
saved to support further diagnosis.

### Actual original-versus-current WASM pixels

Enhanced `web/checks/bounded-pixels.mjs` can take a baseline package directory:

```sh
node web/checks/bounded-pixels.mjs FILE bounded BASELINE_PKG_DIR
```

It now compares original legacy against **both** current legacy and current
bounded, checks lengths/dimensions/finite values, and waits for each worker to
exit before starting the next. Q3, Sony, linear DNG, Fuji and JPEG all passed:
**10 comparisons, zero changed Float32 scalars, zero max/mean error.**
These are single-thread WASM pixels, not a threaded phone pixel readback.

### Phone testing: useful diagnoses, NOT a completed matched matrix

The planned matrix had six A/B/C sessions per 24 MP control, followed by six
A/C Leica pairs. It repeatedly stopped on controller failures. **No new matched
Leica sequence was reached.** Original earlier Leica results remain in the
implementation report; do not present them as new repetitions.

Observed new phone outcomes:

- `phone-v2/001-sony-A-b0`: original, full sequence passed.
- `phone-v2/002-sony-B-b0`: rebuilt legacy, full sequence passed.
- `phone-v2/003-sony-C-b0`: actual legacy/bypassed route, WebContent jetsam
  during zoom edit, **1688 MiB**, PID 4723.
- `phone-v2/004-linear-A-b0`: original, WebContent jetsam at zoom,
  **1548 MiB**, PID 4731.
- `phone-v2/005-linear-B-b0` and `phone-v3/006-linear-C-b0`: editing through
  zoom/return-to-fit/reset completed; controller failed **while requesting a
  screenshot**. Kernel logged **webinspectord**, not WebContent, killed at
  those instants (16 / 20 MiB). These are incomplete runs, not established app
  crashes and certainly not passes.
- `phone-v4/001-sony-A-b0`, phase screenshots disabled: original WebContent
  jetsam at export, **1588 MiB**, PID 5589.
- `phone-v4/002-sony-B-b0`: lost window during first import; no matching captured
  WebContent kill. Cause unresolved. Following setup attempts could not connect.
- `phone-v5/`: a fresh-driver-per-case attempt could not establish either of
  its first two sessions; the batch stopped without any RAW opens.

The user reported inability to unlock the phone and requested the pause.
Do not keep trying to seize the device or ask for repeated toggles as a substitute
for a stable, bounded test plan. We have not established what caused the phone's
UI problem or that stopping the host processes alone resolves it.

## Tooling lessons / changes

`web/checks/raw-bounded.mjs` now rejects engine replacement, checks GPU frames
were presented, optionally asserts the actual decoder route with
`RAW_EXPECT_ROUTE=original|legacy|bounded`, and records resource URLs.
`RAW_SCREENSHOTS=0` skips the generic iPad suite's end-of-phase screenshots.
The screenshot/daemon correlation is a diagnostic finding, not proof all lost
windows were screenshot-induced; v4 still failed without those screenshots.

The Mac `safaridriver` launcher PID is **not necessarily the listening service
PID**. The actual HTTP service is a WebDriver-framework XPC process. It ignored
SIGTERM in this investigation and kept 18138 bound, so several attempted restarts
were ineffective. Verify ownership with `lsof`/`ps`. A forced stop of the owned
service restored session creation once without another user toggle. Never kill
an unrelated driver, Safari itself, or user applications to obtain a pass.

Per-case sysmon/syslog captures and a sampler-staleness watchdog preserved gaps.
ReportedSystemMemory can replay reports about earlier victims: correlate actual
kernel kill timestamps/PIDs, rather than calling every report a current failure.

## Exact evidence and reproducibility

Ignored root: **`target/q3-bounded/controlled/`**.

- `candidate.patch`, `source-hashes.json`, `identity.txt`: snapshot identities.
- `bench.rs`, `bench-v1.rs`, `bench-{original,candidate}/`: harness/build inputs.
- `*-native-v2`: production-allocator executables with CPU/fault/switch counters.
- `sony-isolated.jsonl`, `native-isolated.jsonl`: final native timing data.
- `native-summary.json`, `analyze.py`: statistics/analysis.
- `native.jsonl`, `sony-tail.jsonl`, `native-shared.py`: earlier shared-process runs.
- `pixels-wasm.jsonl`: ten original-versus-current bitwise comparisons.
- `phone-v2/` through `phone-v5/`: all subsequent device attempts and failures.
- `phone.py`, `phone-*-plan.json`, per-case `capture.jsonl`: protocol and attempts.
- `phone-summary.json`, `analysis.txt`: generated summary; regenerate with
  `python3 target/q3-bounded/controlled/analyze.py`.

Application prototype source was not changed in this follow-up. Only its two
check scripts were strengthened. All unrelated user changes remain untouched.

## Suggested next session — only after explicit device readiness

1. Review native tail behaviour before declaring no performance regression.
   The median question is now much better answered; tail behaviour is not.
2. Agree on acceptable large-RAW latency trade-off and what “no regression” means.
3. First establish one short, screenshot-free, fresh-driver phone smoke test.
   Use **small bounded batches with user check-ins**, not another unattended
   48-session queue. Stop immediately if the phone becomes hard to use.
4. If device reliability remains a blocker, pause phone testing and investigate
   retained GPU/render/open allocations offline. Existing baseline failures show
   the problem is not unique to the new decoder, but do not exonerate it either.
5. Native Files-picker import, touch/background lifecycle and a completed
   statistically useful phone A/B remain open. Keep the feature opt-in meanwhile.

## Copyable resumption prompt

> Resume the bounded Leica RAW investigation in `/Users/noahhaasis/Projects/Negs-wasm`.
> Read `docs/iphone-leica-bounded-regression-handoff.md`, then the linked prototype
> results and original brief. We paused because the phone became difficult to use.
> Do not start or connect device automation until I explicitly confirm readiness.
> All assistant device jobs and test servers were stopped. First inspect git status
> and preserve all unrelated/concurrent edits; feature work is checkpointed as WIP
> on `wasm-investigation`, with frozen original/candidate build worktrees under
> `target/q3-bounded/controlled/`. Original baseline is `89af24c`.
> Review the saved native Sony latency tails before claiming no regressions; the
> 26% figure is native Q3 capped-decode overhead, not a universal phone penalty.
> If phone tests resume, agree on one small screenshot-free batch and stop on
> device usability problems. Keep the feature opt-in. The WIP commit does not
> authorize automatic enablement; resolve or explicitly accept the remaining
> limits before changing defaults or claiming a reliability pass.
