# Linux feature port after 0.13.0

Status: functionality implemented and locally validated (2026-11-05).
See [implementation and results](linux-prep-port.md). The inventory and sequence
below preserve the approved scope.
Prepared 2026-11-04 against `main` at `bb7ccd9` after fetching `origin`.
Baseline: [Negs 0.13.0](https://github.com/noahhaasis/negs/releases/tag/v0.13.0),
tag commit `a06ba88`, published 2026-11-02 (marked prerelease, as the 0.x releases are).
Survey: `git log v0.13.0..bb7ccd9` and the corresponding source diff, checked
against the current Linux implementation. No committed files under `linux/`
changed in that range.

Goal: bring Linux's functionality up to the current macOS/Windows apps using
the existing GTK/libadwaita UI. Build the controls and interactions needed to
use the features now. Visual polish follows after the port.

## Feature inventory

The macOS Prep work is `fafda8f` through `6b4d456`, merged in `c721589`;
the tone graph is `d351348`. Windows ports are `e38d1b5` (tone graph) and
`a897f6c` (Prep), including their subsequent merges and fixes. The later
viewer features are `c92b539` on macOS and `c0a5793` on Windows.
Windows' later painting fixes through `bb7ccd9` are considered separately below.

| # | Feature now on macOS/Windows | Linux today | Required port |
|---|---|---|---|
| 1 | Prep adjustments before film simulation | Shared engine can render `prep.*`; no Linux controls or explicit bindings | Exposure, Highlights, Shadows, Contrast, Lift, Gain, Color Boost, Saturation and Midtone Detail; defaults, reset, undo and sessions |
| 2 | Prep / Lab / Output, opening on Prep | Lab / Post, defaults to Lab | Three functional tabs; migrate old Post selection to Output; distinguish an old automatic Lab default from a deliberate choice |
| 3 | Input preparation together in Prep | White balance, input colour, lens corrections, Auto exposure and Metering are in Lab | Move existing controls, retaining RAW/non-RAW and lens-support visibility rules |
| 4 | Output contains Image Size and Border | Image Size is in Lab; Post has finished-image tone/saturation plus Border | Move Image Size; remove old Post rows and neutralize their restored values, following Mac/Windows; explain the appearance change in release notes |
| 5 | Film/paper available on every tab | Pickers and preview grids are inside Lab | Pin pickers above tabs; bound grid height and scroll to the selected stock; retain previews and simple-menu setting |
| 6 | Clearer Lab controls | All controls exposed; Exposure and continuous Film format slider | Negative exposure label; stills-format menu and Custom slider; basic controls plus changed controls, with Show all controls; Scan film directly at top of Print and a paper-skipped indication |
| 7 | New photographs: Start fresh / Keep previous edits | Always carries adjustments to the next photograph | Preference defaulting to Start fresh; implement exact carry/reset behavior and preserve restored sessions |
| 8 | Preview limit and low-memory export are app settings | Stored as per-photo choices and exposed in Lab | Move to Settings; migrate legacy values; keep them outside Undo/Reset, and serial grain out of previews |
| 9 | Live Match camera/original brightness | Solver exists in helper; no Linux reference extraction, switch or lifecycle | Per-photo switch, automatic matching on new photographs, exposure offset, re-solving, failure notes and Save/Export waiting |
| 10 | Shared camera-preview colour/reference rules | Startup RAW thumbnail is a JPEG scan in `photo.rs`, unsuitable as a trusted brightness reference | Reuse `spektrafilm_io::brightness` for embedded previews, orientation, letterbox trimming, colour declarations and reduction; colour-correct file references for JPEG/PNG/TIFF |
| 11 | Histogram / RGB waveform, resizable and remembered | Fixed 82 px histogram; developed-frame count is already on a worker and crop/straighten aware | Mode switch, 88–420 logical-pixel resize, reset/keyboard handling, waveform, quarter-turn orientation and strictly bounded sampling |
| 12 | Interactive Print Curves tone graph | Helper already supplies `tone_curve` and `film_histogram`; Linux ignores them | Draw graph and histogram, RGB/R/G/B channels, four drag behaviors, reset and synchronized sliders/history |
| 13 | Favourite films and papers | Preview grids and menus exist, without favourites | Persistent favourites, star/context action, favourites first in grids/menus and thumbnail scheduling |
| 14 | Help for every control and group | Partial tooltips | Port the help table and cover all controls, groups and new actions |
| 15 | Fit stops at 1:1 | Fit scale can exceed 1 | Cap fit at one photograph pixel per physical screen pixel; account for GTK scale, crop, rotation and borders |
| 16 | Window minimum preserves both columns | Explicit minimum is only 360 × 480; sidebar can resize 280–560 | Derive a minimum from usable canvas/sidebar widths, respect monitor work area, grow before showing sidebar, preserve sidebar drag limits |
| 17 | Optional custom viewer background | Canvas fills a fixed dark colour | Settings toggle and colour chooser, persistent colour, immediate redraw; keep appearance redesign for later |
| 18 | Typed export-size correction on Mac | GTK SpinButton already provides numeric input; behavior has not been exercised in this survey | Verify partial typing, focus changes, clamping and actual output size; fix only if Linux reproduces the defect |

### Already shared or already present

- Prep math, parameter validation, CPU spatial processing, prepared-frame caching
  and exposure-only GPU folding are in the shared engine/helper. Use them; do not
  write another processing implementation. Full GPU Prep is still an unmerged
  experiment and is outside this port.
- `place_exposure`, the common camera-preview colour rules, `tone_curve`, and the
  Prep-aware `film_histogram` are implemented. Linux needs integration, not new solvers.
- Film, paper and preset previews, their setting, scheduling, staleness handling
  and preset support already exist on Linux. Port favourites and the pinned
  placement on top of that implementation.
- Apple RAW 9 → 8 → rawler fallback is macOS-specific. Linux continues to use rawler.
- Win32/GDI slider painting, title-bar integration, rounded corners, theme/flicker
  workarounds and crop-window painting fixes are platform-specific. Check their
  user-visible behaviors in Linux; do not copy the Windows mechanisms.
- Slider fill from default, typography, spacing, colours, hover treatments,
  animations and other visual refinements belong to the later polish pass.
  Keyboard focus remaining visible, stable crop framing and no old-photo flash
  belong in functional regression checks now.
- The separate draft `docs/highlights-shadows-plan.md` is an older proposal for
  finished-image Post processing. It is not the implemented Prep feature and
  must not be used as the specification for this port.

## Implementation order

### 1. State, preferences and migration

Primary files: `linux/src/model.rs`, `parameters.rs`, `preferences.rs`,
`actions.rs`, and session loading/saving in `main.rs`.

- Add explicit Prep bindings and ranges. Exposure is −3…+3 EV, step 0.05;
  Highlights is −180…100; the other seven Tone/Color/Detail adjustments are
  −100…100, step 1. All nine controls default to zero.
- Add per-photo brightness-match metadata (solved EV, dependency snapshot,
  highlight-limit flag) to the undoable state. Missing metadata means off for
  an old session; do not automatically re-match restored edits.
- Add app preferences for new-photo policy, automatic matching (default on),
  preview limit, low-memory export, Lab advanced controls, scope mode/height,
  graph channel, favourites and custom background. Use serde defaults for old files.
- Extend the current session migration (`SESSION_SCHEMA = 3` today). Clear old
  Post overrides in both `State.params` and the `Post.*` values map, so the
  latter cannot silently reapply them in `parameters::runtime`. There is no exact
  Post-to-Prep conversion: this deliberately follows current Mac/Windows behavior
  and changes a restored photograph that used those edits. Document it explicitly.
- Migrate legacy preview/memory choices once, then project current preferences
  into rendering/export snapshots. Undo, Reset and preset import must not put
  old preferences back. Low-memory grain applies to exports only.
- Default new sessions and old automatic Lab selections to Prep, using an explicit
  tab-chosen marker going forward; map a saved Post selection to Output.
- Start fresh resets Prep/Lab/Post, white balance and border; keep film/paper,
  Image Size and output colour/encoding. Reapply the film's scan-mode default.
  Both opening policies reset crop, straightening, rotation, lens corrections
  and input interpretation. Keep carries the user's exposure offset, with a
  fresh match for the new photograph. Session restore and an in-place source
  refresh preserve current edits instead of acting as fresh opens.
- Film-look presets continue to preserve the photograph's Prep; LUT export
  continues to exclude it. Check the Linux preset merge path as well as the helper.

Gate: model tests for old sessions, state round trips, reset/undo, preset
preservation, preferences and both opening policies. Do not remove Post controls
before their migration is ready.

### 2. Expose Prep and the functional inspector structure

Primary files: `linux/src/fixtures.rs`, `parameters.rs`, `inspector.rs`,
`image_size.rs`, `stock_browser.rs`, `preferences.rs` and `validation.rs`.
Reference: current `macos/Sources/NegsApp/Inspector.swift` and
`windows/src/state.rs` / `ui/inspector.rs`.

- Add Prep / Lab / Output using existing GTK controls. Prep contains White
  Balance, Brightness, Tone, Color, Detail, Lens corrections and Input Colour.
  Leave room for the live match switch/note added in step 3; manual Prep is
  usable at this milestone.
- Pin film/paper above the tabs, with a bounded scrolling preview grid opening
  on the current stock. Retain existing development-time controls and stock
  selection semantics. Indicate when paper is skipped for direct scanning.
- Output contains Image Size and Border. Move preview/memory controls to Settings.
- Port the upstream per-group basic-control rules; changed advanced controls
  remain reachable with Show all controls off. Retain B&W and enable/disable rules.
- Rename the displayed Camera Exposure to Negative exposure without changing
  its engine path. Use the upstream film-format table: 110 = 17, half frame = 24,
  135 = 35, 6×4.5/6×6 = 56, 6×7 = 70, 6×9 = 84, 4×5 = 120 mm. Custom retains
  the 4–120 mm slider and remains visible when explicitly chosen at a preset value.
- Add favourite actions and ordering to existing grids/menus. A star action
  must not choose the stock or create a photo-edit undo step. Schedule favourite
  thumbnails first while preserving the main preview's priority.
- Port the current help text from `windows/src/help.rs` / `ControlHelp.swift`
  to GTK tooltips and accessible labels, with coverage for rows and groups.

Gate: use all nine Prep controls with real photographs, including slider reset,
undo/redo, presets, film changes, session restoration and both output engines.
Update existing control audits and UI test tab names instead of adding duplicate
audits. No styling redesign is required.

### 3. Reference extraction and live brightness matching

Primary files: `linux/src/runtime.rs`, `model.rs`, `parameters.rs`, `inspector.rs`,
`actions.rs`, `exports.rs` and `preview_cache.rs`; shared helper/IO as needed.
References: `windows/src/model.rs`, `state.rs`, `brightness.rs`,
`macos/Sources/NegsApp/EditorModel.swift` (Camera brightness), and
`crates/spektrafilm-io/src/brightness.rs`.

This is the largest behavioral change; implement reference creation before the
async match lifecycle.

- Keep GTK's dependency graph separate from the engine, as today. Prefer a
  small capability-advertised helper command for brightness-reference creation,
  calling the existing shared IO functions, rather than linking the whole IO/
  rawler graph into `linux/Cargo.toml` or copying the RAW parser. This command
  is proposed work: it does not exist yet. Preserve the current `place_exposure`
  interface and other clients.
- For RAW, take the embedded camera preview, apply orientation exactly once,
  trim valid letterbox bars only for differing shapes, reduce to ≤512 px and
  apply the common colour rule: untagged means sRGB, Adobe RGB converts to
  sRGB, other declared spaces are refused. Missing/tiny/wrongly framed previews
  produce the same human-readable failure notes as upstream.
- For JPEG/PNG/TIFF, make a colour-managed reference from the original file,
  not the developed image or a RAW decoder rendering. Respect orientation and
  profile; refuse transparent/floating-point references as Mac/Windows do.
  Store scratch references with source identity, and clean them with the session's
  scratch files. Reference creation must not replace the retained render/decode.
- Add Match camera brightness / Match original brightness and the new-photo
  setting. `prep.exposure_ev` holds total exposure; the slider displays the
  user offset above the solved value. Reset resets the offset. Follow upstream
  ±3 EV clamping, highlight-limit note and failure/retry behavior.
- Port the exact match-dependency exclusions from current `state::match_inputs`
  / `EditorModel.matchInputs`: user Prep edits, print exposure, Post, grain,
  crop/rotation, output colour/encoding and serial grain do not initiate a new
  solve. Film, paper and relevant look/decode changes do. Include source identity.
- Queue one solve only after a current settled preview, never during a drag or
  crop. Coalesce look changes; reject replies for an old source/settings/switch
  state. Apply the result without a second undo step; undo restores its matching
  metadata and re-solves only if needed. Keep the user offset across re-solves.
- Save Preview, Fast export and Reference export wait for the pending match
  and its current preview before taking the final snapshot, with the upstream
  bounded wait (30 seconds), then recheck output eligibility. Failed matches
  keep the existing exposure and show the upstream failure note. Cancel a waiting
  output if its photograph is superseded; never reuse an obsolete retained
  render. Keep LUT's existing contract.
- Include effective Prep in launch-cache/thumbnail identities. Preserve helper
  scheduling priority: current previews and required matching before optional
  tone-graph/stock requests. An optional request already in flight remains bounded.

Gate: real RAW and JPEG/PNG/TIFF tests; Adobe RGB, untagged and unsupported
references; letterboxing and portrait orientation; rapid photo/film/paper
changes; offsets and undo; failed matches; Save/Export while a match is pending.
Verify CPU fallback and Vulkan, and that matching never invalidates retained Save
state by itself.

### 4. Scopes and Print Curves graph

Primary files: `linux/src/runtime.rs`, `inspector.rs`, `main.rs`, with new
`scopes.rs` and `tone_graph.rs` modules. Reuse the platform-independent math in
`windows/src/histogram.rs` and `windows/src/tone_graph.rs`, adapting their JSON
and state access to Linux. These can be separate commits.

- Compute histogram and waveform together from the developed, border-free
  frame on a worker; account for crop, straightening and displayed quarter turns.
  Bound both sampling dimensions, including thin panoramas. Keep the existing
  stale-generation rejection and cover cached previews and source refreshes.
- Add Histogram/Waveform selection and a remembered 88–420 logical-pixel
  height, default 88; resize grip, double-click reset and keyboard adjustment.
- Consume helper `film_histogram` from current render replies. This is a
  different measurement from the display histogram; do not substitute one for
  the other.
- Add the graph inside Print Curves with persisted RGB/R/G/B selection.
  Middle-handle horizontal drag sets Print exposure; side-handle vertical drags
  set Slow/Fast gamma; dragging the remaining curve sets Gamma. Each drag is
  one undo action, contrast drags activate Print Curves, double-click resets,
  and the existing sliders stay synchronized.
- Request `tone_curve` only while the graph is mapped, its group/tab visible
  and cropping inactive. One request in flight, coalesced updates, no duplicate
  requests and no old-photo histogram. Use upstream local prediction while
  waiting; keep authoritative replies separate. Disable/dim for direct scanning.
- Preserve the prototype's known limits: film histogram ignores crop/spatial
  effects, side-handle prediction is approximate, and curve agreement with GPU
  output is not established. Porting does not resolve those engine questions.

Gate: sample-budget/crop/rotation tests; real resize/mode persistence; graph
geometry, prediction, stale-response and drag/undo tests; rapid slider edits
with graph visible; no optional helper work when hidden.

### 5. Viewer behavior and export-size check

Primary files: `linux/src/canvas.rs`, `main.rs`, `preferences.rs`, `export_ui.rs`.

- Cap Fit at 1:1 in physical pixels, using source/working dimensions rather
  than the interactive texture's reduced size. Check small images, HiDPI,
  crop/quarter turns, borders and the existing explicit 100% action together.
- Give the canvas and sidebar usable minimums and fit the combined minimum to
  the monitor work area. Showing the sidebar makes room first; hiding it permits
  a narrower window. Test restored undersized windows, splitter dragging,
  maximized/full-screen and small-screen behavior using GTK's own layout.
- Add the custom-background preference and GTK colour chooser; redraw the
  canvas immediately and persist its chosen value. Keep this a small functional
  setting using existing controls.
- Type a complete export size such as 1600 from an empty field, leave focus,
  save and inspect the output dimensions. Also test below/above-range values
  and the aspect-limited edge. Apply a correction only if the GTK field fails.
- Check Tab scrolls the focused control into view, crop mode keeps the framing
  stable and editing never resurfaces the startup camera thumbnail. Fix any
  reproduced Linux regression through the existing GTK/render state paths.

### 6. Integration, performance and delivery

- Rebuild helper, Fast and Reference exporters and GTK app from the same
  source (`bash linux/build.sh`); run the Linux unit suite via
  `bash linux/cargo.sh test --release --locked` and affected core/native/IO tests.
  Run the live-helper schema test against that rebuilt helper.
- Update/run `linux/check_controls.py` and `linux/check_engine.py`, then
  desktop checks for RAW open, Prep, crop/rotation/border, session restore,
  Save Preview and both export engines. Use `NEGS_PROFILE_DIR` for app tests.
- Compare preview/Fast/Reference output using the existing parity tolerances;
  keep numerical engine gates unchanged. Exercise neutral and active Prep,
  matching on/off and preset thumbnails. Broaden shared engine/GPU validation
  if implementation changes those paths; a UI port alone does not need a new
  numerical algorithm or regenerated reference images.
- Measure startup, tab switching, Prep drag/settled render, repeated Lab edits
  with Prep active, scope counting and graph-visible editing. Establish a Linux
  baseline before implementation; historical Mac/Windows timings are context,
  not Linux targets. Confirm prepared-frame reuse and thumbnail priority survive.
- Validate package builds with the Linux workflow (Arch, Flatpak, AppImage),
  including any helper capability or dependency changes. Check installed helper
  compatibility and file access for brightness references inside Flatpak.
- Update `docs/prep.md`, `docs/linux-app.md` and release notes with completed
  features, migrations, measured results and remaining limitations. Commit and
  push completed changes to `main`, per the repository workflow. Publishing a
  release and installing over the user's current app are separate tasks.

Completion means all inventory features work through the Linux app, not merely
through helper requests. The UI polish pass begins after these functional gates.

## Survey evidence and limits

This document is based on source/history inspection, a fresh fetch, and the
published v0.13.0 release metadata. It records planned checks, not passing test
results or an implementation estimate. The current worktree's existing
`linux/run.sh` modification and untracked Post highlights/shadows draft are
outside this planning change.

Useful upstream notes: [Prep](prep.md), [brightness placement](exposure-placement.md),
[Windows port inventory](windows-prep-port-plan.md),
[Windows implementation/results](windows-app.md), and
[existing Linux stock/preset previews](stock-previews.md#linux-reference-implementation).
When those earlier plans or prose disagree with the current source, follow the
current source and its tests; for example, matching now applies to new photographs
including supported non-RAW files, and Start fresh is the default.
