# Linux port of the 5 November macOS changes

Prepared 2026-11-05. Implementation and validation are recorded in
[the port results](linux-macos-nov5-port.md). The original survey and plan follow.

Surveyed local `main` at `c84cd0d`, which merges the Mac's `main` at `5872dfc`.
The Mac's branch tip was independently confirmed over SSH. A fresh origin fetch
returned `ecdd855`; that commit is already an ancestor of local `main`.
This inventory uses the Mac branch's committed changes dated 5 November and
checks their final behavior against the current GTK source. The two copies of
the earlier Linux Prep port (`ecdd855` / `0631b9e`) represent the same feature
work and are counted once.

## What changed, and what Linux still needs

| Change | Source commits | Linux status and action |
|---|---|---|
| Search the film grid | `b8ad592` | Missing in GTK. Port filtering and keyboard behavior to `stock_browser.rs`. |
| Keep the previous frame visible while a brightness match is pending | `e4a9f92` | Missing. GTK currently presents the rendered exposure before solving. Add a held-frame lifecycle. |
| Render a canvas-size frame before the settled frame after a discrete edit | `30de494`, finalized by `8a73944`; Linux handoff `5a1fa3c` | Missing. GTK uses canvas-size frames only during drags and renders a revision once. Port explicit render steps and matching coordination. The final Mac version starts settling immediately; its original 400 ms settling delay was removed. |
| Cache input-gamut remap coordinates | `b35f754` | Already merged in shared Rust core. No GTK algorithm port; rebuild and validate on Linux. Cache holds at most eight coordinate maps. |
| Share built pipelines across main renders, stock/preset thumbnails, tone graph and exposure placement | `ab76fd6` | Already merged in the native helper used by GTK. One 40-entry cache, with the main renderer's last four keys pinned; releasing thumbnails keeps pinned entries. Validate reuse and release on Linux. Render replies now include `cached_pipeline` and `pipeline_ms`. |
| Four initial favourite films | `5872dfc` | Already changes Linux preferences: Gold 200, Portra 400, Ektar 100 and C200. Existing saved lists, including an empty list, must remain respected. Papers start empty. Verify migration/persistence. |
| Smaller development builds, storage cleanup, hashes-only golden runs | `0fa2b7c` | Shared tooling is present. Root Cargo dev builds use `debug = 1`; GTK's separate workspace does not inherit that setting. This is development housekeeping, separate from runtime performance. |
| Web favourites, film search and long-name sidebar sizing | `a5b789d`, `6f2c6f5`, `1ba2419` | Web changes, not further GTK ports. GTK already has favourites and ellipsizes its current-stock label. Keep long names in the search/layout checks. |

The earlier functional port is already implemented: Prep / Lab / Output,
brightness matching, resizable histogram/waveform, Print Curves graph, favourites,
viewer background and physical 1:1 Fit. See [results](linux-prep-port.md).
Today's Fotufilm integration is also shared and already present: 52 films and
10 papers, with the ported films' grain presets. Neither needs reimplementation.

“Already merged” describes source availability. This survey does not establish
which helper is installed or prove that the combined Mac changes pass Linux
tests. The previous [validation report](linux-prep-port.md) predates this merge.

## Implementation order

### 1. Establish the Linux baseline and validate the merged helper

Build the GTK app, native helper and both exporters from the same source with
`bash linux/build.sh`. Identify their paths and source commit; use an isolated
`NEGS_PROFILE_DIR` for all app checks.

- Run the existing gamut-remap bit-identity tests, including the expanded
  ignored case, and native pipeline-cache pin/eviction tests. Run the native
  quick-before-settled byte-identity test already added by the Mac work.
- Exercise a thumbnail followed by choosing that film, repeated film/paper A/B
  switches, tone-graph requests and brightness matching. Record cache-hit
  replies and pipeline-build times. Disable previews and confirm unpinned
  pipelines are released while recent main pipelines remain reusable. Collapsing
  a grid should stop its optional requests.
- Measure the current one-frame GTK behavior with the merged helper: first
  visible changed frame, current settled frame, helper resident/peak memory and
  GTK presentation cost. Use a large RAW with matching off and on, plus a
  smaller photograph. Keep cold and warm runs separate.

This establishes a scheduler baseline that already includes the shared cache
optimizations. A separate pre-cache comparison needs an identified older build;
do not attribute the caches' gains to the later GTK scheduler change.

### 2. Hold frames until brightness matching is resolved

Primary files: `linux/src/runtime.rs` and `linux/src/brightness.rs`.

Today, render completion immediately calls `set_developed` and presents the
frame. `brightness::pump` then requires `current_revision == revision` before
starting the solve. Holding that frame without changing the solve gate would
deadlock.

- Retain a completed, non-drag frame with its revision, source generation,
  helper render ID, metadata and optional preview-cache key while its match
  needs solving. Keep the previous visible frame on the canvas.
- Allow matching to start from the current held frame's source metadata/decode.
  Distinguish “needs a match solution” from the broader `pending` predicate,
  which also waits for the final render for Save/Export.
- If matching changes exposure, invalidate the held frame and schedule the new
  revision. If it leaves exposure unchanged, fails or is turned off, release a
  still-current held frame once the in-flight operation is resolved.
- Reject held frames after source changes, newer edits, undo or shutdown.
  Publish retained Save state and the disk preview cache only when the settled
  frame is current and matching no longer makes it provisional.

Gate: previous image goes directly to the matched image; no intervening old
exposure. Cover unchanged solutions, per-photo/per-look failures, disabling the
match mid-solve, rapid source changes, undo, and deferred Save/Export. Complete
this lifecycle before adding quick frames.

### 3. Add quick / drag / settled render steps

Primary files: `runtime.rs`, `canvas.rs`, `brightness.rs`, `inspector.rs`,
`tone_graph.rs`, `stock_browser.rs`; tests may use a dedicated module.

Use the final Mac [model](../macos/Sources/NegsApp/EditorModel.swift),
[tests](../macos/Tests/NegsTests/QuickFrameTests.swift) and
[Linux handoff](handoff-quick-frame-linux.md) as behavioral references.

- Introduce explicit step state per `(source_generation, revision)`. Replace or
  extend `render_next`'s one-request-per-revision guard so a quick step can be
  followed by a settled step of the same edit without a synthetic history edit.
- Choose quick only when a developed frame of this photo exists, it is not the
  restored launch cache, the photo is at least 4.5 MP, canvas scale is between
  zero and one and its square is at most 0.5, this edit has not completed its
  quick step, and the next request cannot reuse the helper's retained output.
  Compute scale in physical display pixels using the existing canvas path.
- Build the helper-reuse key from the exact outgoing Linux render command,
  removing only `preview_path` and neutralizing `params.post`. Include source,
  generation, crop, size/limit and all other processing inputs. Clear the
  remembered retained-output key before a render; set it only after a successful
  settled request. Invalidate it on helper/source resets. A post-only edit must
  retain its existing fast path.
- Present a successful quick frame with a Preview/dimensions/time status, or
  hold it for matching using step 2. Do not set `retained_id`/`current_revision`
  or save it to the launch cache. Keep settled preview-scale notices stable.
- Start the settled render immediately after quick completion when no match or
  held frame blocks it. Start matching from a current quick frame, held or
  shown, before doing an export-size render at the old exposure.
- Permit superseded quick frames from the same source generation to appear
  during rapid edits; the newest edit renders next. Settled frames retain the
  existing current-revision rule. Source-generation rejection always applies.
- A failed quick step silently advances to one settled attempt, which reports
  failure once. Mark step completion on failure too, preventing retry loops.
- Slider and tone-graph release/cancel paths must go directly to settled after
  drag frames. Preserve one undo action per gesture and the existing deferred
  GTK gesture-cancellation handling.
- Give required matching and main rendering priority over optional work. The
  stock browser's `requested_revision == revision` check no longer proves a
  settled frame exists; require settled completion, as the tone graph already
  does. Keep this priority when matching is disabled too.

Gate: real-helper lifecycle tests with a deterministic photo of at least 6 MP
and a fixed canvas-scale test hook. Cover quick then settled, byte-identical
settled output, fast switching, undo after quick, post-only edits before/after
film changes, drag/release/cancel, exactly one final settled render after a
brightness solve, match disable/failure, invalid requests, Save/Export gating,
and absence of optional requests between quick and settled. Also cover small
photos, zoom/HiDPI thresholds, first open, cached restoration, Original held,
and crop/rotation/border presentation.

### 4. Port film search

This is independent of steps 2–3 and can ship as a small separate commit.
Primary file: `linux/src/stock_browser.rs`; use `catalog.rs` for a pure matching
helper if that keeps filtering testable.

- Add a native `gtk::SearchEntry` above the Film grid, outside its bounded
  scroller. Paper retains its existing control. Keep the current one-open-grid
  behavior and available adjustment space.
- Match every whitespace-separated word against name, ID with underscores
  treated as spaces, and group. Match case and accents consistently using
  GLib normalization/case folding. Empty/whitespace query restores all films;
  remove empty group headings and show a clear no-results message.
- During search, list each result once under its catalog group, in catalog
  order, matching macOS. Outside search, preserve GTK's existing favourites
  ordering rather than changing its layout as part of this port.
- Return chooses the first listed result; Escape/clear empties the query.
  Scroll to the first result for a search and back to the current film when
  cleared. Typing filters only; it must not change the photo or undo history.
- Reuse tile widgets and rendered thumbnails. Schedule only included films in
  the filtered grid, yielding to main rendering and matching as before. Keep
  star actions and selection functional while results change.

Gate: pure query cases mirroring Mac tests, plus accents, ID/group matches and
all-word mismatches; GTK checks for typing, Return/Escape, no results, scroll
restoration, favourite toggles, retained thumbnails, long labels, focus and
accessible names. Verify plain menus when previews are disabled still work.

### 5. Validate, measure and deliver

- Run Linux release unit tests and affected core/native/IO tests. Use the
  rebuilt helper for opt-in desktop lifecycle tests, controls and engine audits,
  rather than relying only on scheduler unit tests. Exercise CPU fallback and
  this machine's Vulkan path, with bounded BLAS threads for repeatability.
- Compare the baseline from step 1 with the completed port on identical RAWs,
  window size, scale, settings and power state. Report first visible change and
  settled latency separately, matching off/on, warm A/B changes, search-driven
  selection, and idle/peak memory. Confirm optional previews do not delay the
  settled render. Instrument render steps and actual GTK presentation; existing
  startup milestones alone do not measure click latency.
- Keep final pixels and export dimensions unchanged. Test retained Save against
  an independent Fast render, normal f64 Reference export, and quick-frame
  exclusion from disk restoration. No reference/golden updates are expected.
- Verify default favourites with a new profile, a missing preferences field,
  an existing customised list and an explicitly empty saved list.
- Run the Arch/Flatpak/AppImage workflow against the final committed source,
  including the existing tooltip source-archive fix. Direct pushes to `main`
  do not automatically trigger this workflow; dispatch it explicitly for the
  implementation. Check matching's file access in Flatpak and package the same
  helper/app revisions.
- Update `docs/linux-app.md`, implementation/results and unreleased notes.
  Commit and push completed work directly to `main` per `AGENTS.md`. The older
  handoff's branch/no-push instructions are superseded by that workflow. Keep
  existing unrelated work and other worktrees intact.

The housekeeping commit does not require runtime work. If smaller GTK debug
builds are desired, add a separate `[profile.dev] debug = 1` in
`linux/Cargo.toml`; do not run storage deletion as part of this port.

## Performance expectations and limits

The handoff reports these Mac measurements on a 60 MP photo; they are evidence
for the design, not Linux predictions:

| Measurement | Before | Quick-frame implementation |
|---|---:|---:|
| First changed frame, matching off | 2355 ms | 163 ms |
| Settled frame, matching off | 2355 ms | 2498 ms |
| First changed frame, matching on | 4921 ms | 597 ms |
| Settled frame, matching on | 4921 ms | 2916 ms |

Quick rendering can add total work when matching is off. The Mac helper's idle
footprint grew by about 211 MB on this photo for a canvas-size source copy, also
allocated by dragging. Measure the corresponding Linux cost before calling
the change complete. The shared-cache commit reports pipeline builds dropping
from about 16 to 5.5 ms and warm quick-size A/B switches from about 76 to 60 ms;
Linux hardware/backend results remain unmeasured.

No app code, tests, installations or release publishing were performed during the
initial survey. Existing `linux/run.sh` mode changes and the untracked highlights/shadows
draft are outside this planning change.
