# Web presets — approved implementation handoff

Status: **approved scope; ready for an agent to implement**. Noah approved the
additional development-time, gamut and spectral-upsample controls as well as the
preset feature. This is the start-here document for the complete work; no further
scope approval is needed for the items below.

Step 1 is RAW white balance, including UI, assigned to a separate **Sol 6.1 /
xhigh** agent. Give that agent [handoff-web-white-balance.md](handoff-web-white-balance.md),
then integrate its result before completing the remaining steps. The WB handoff
is part of this specification, not optional background. Coordinate ownership of
shared files such as `web/app.js`; do not overwrite another agent's changes.

Desktop contract: [presets-design.md](presets-design.md), with the current
`crates/spektrafilm-core/src/presets.rs` as the implementation authority. The
desktop document's preset-library UI/storage plan does not apply to the web.

## Confirmed decisions

- A three-dot menu immediately after the photograph's Export button contains
  **Import preset…** and **Export preset…**. No separate Share preset menu item.
- One preset is imported/applied at a time. No collection, preset panel, history,
  batch import, or browser-persisted preset library. Another import replaces the
  current Lab look; this is not a lifetime limit of one import.
- Reuse desktop-compatible spektrafilm GUI-state `.json` files and the shared
  Rust importer/exporter. Lab only, with the same exclusions as desktop.
- The Export preset dialog also displays a generated preset link and a Copy link
  button. No separate share dialog, Create link action, or native link-share UI.
- Opening a shared link shows a modal over the editor. The recipient can download
  the preset or try it on a sample. Trying always opens **Sample 1**.
- No new Revert/Undo feature. The existing **Reset Adjustments** is the way to
  return to defaults, keeping the chosen film and paper.
- First implementation task: RAW white balance, including UI, assigned to a
  separate **Sol 6.1 / xhigh** agent. This document does not launch that agent.
- Also add film and paper development-time pickers, input/output gamut choices,
  and spectral upsampling selection. These controls are approved scope, not a
  later optional enhancement.

## Reset behavior

`web/app.js::resetAdjustments` replaces the complete `state.params` with engine
catalog defaults, keeping `io.rotate_cw` and `io.photo_long_side_px`. Film and
paper live outside params and remain selected. `io.scan_film` returns to the
chosen film's default. Nullable stock-dependent values reset to null, so the
selected film supplies its grain, DIR and halation defaults again.

This clears every preset-carried Lab value, including fields without a control.
Crop and border are separate state and stay as they were. Existing behavior also
resets Post adjustments; importing a preset, in contrast, leaves Post untouched.
Keep that distinction and the existing desktop-style Reset behavior.

Reset means default settings for the selected stocks, not restoration of the
pre-import edit. No pre-import snapshot is required. Once RAW WB is implemented,
Reset must also actually re-develop a loaded RAW when its WB changes to As Shot;
changing the JSON alone is insufficient.

## UI and flows

### Toolbar menu

```text
Negs        [Open…] [Export] [⋯]
                            ┌─────────────────────┐
                            │ Import preset…      │
                            │ Export preset…      │
                            └─────────────────────┘
```

Follow existing dark surfaces, borders, accent and typography. Anchor inward so
the menu fits both the desktop sidebar and mobile bottom sheet. Support keyboard
navigation, Escape, outside dismissal and focus restoration. Hide alongside the
normal export controls during crop editing. Enable preset actions after their
settings/engine prerequisites are ready; they do not require a loaded photo.

### Import preset

1. Open a single-file `.json` chooser, not the photograph chooser.
2. Read and validate the file before mutating the edit. A failed import leaves
   the photograph and settings unchanged, with a useful error.
3. Apply the normalized film, paper and params directly through the shared
   desktop apply policy. Do not run film-selection auto-pairing or stock-reset
   handlers over imported values.
4. Synchronize the inspector and re-render. Re-develop a RAW if its WB changed,
   preserving geometry and viewing state.
5. Briefly acknowledge the applied file name. No active-preset list or matching
   checkmark: subsequent editing is simply editing the current settings.

With a photograph already open, use that photograph. With an empty editor,
apply the preset and open Sample 1, reusing the existing sample flow.
Opening another photo or switching samples keeps the applied Lab look, just as
normal sample changes keep adjustments today.

### Export preset + link

```text
Export preset                               ×

Name
[Warm afternoon                         ]

Preset link
[https://…/#preset=…                     ]
                              [Copy link]

Includes Lab settings only. Your photo,
Post adjustments, crop and border are not included.

                       [Close] [Download preset]
```

- Prefill the imported name when available, otherwise a useful film-based name.
  Sanitize the download file name using desktop naming policy where applicable.
- Export the current Lab settings, including edits made after import; do not
  simply re-download the imported bytes.
- Prepare the link automatically when the modal opens. Show a loading state
  while preparing; no separate Create link action.
- Name changes update the link's display name and download file name. The modal
  captures one settings snapshot, so its link and file describe the same look.
- Use a readonly, selectable URL field and Copy link. Provide visible copied/error
  feedback and manual selection if clipboard access is unavailable.
- Download `<name>.json` in the desktop format, without a web-only wrapper or
  metadata. Keep the dialog open so the user can also copy the link.
- Retain an iOS-compatible file save fallback where direct downloads are not
  supported. This is not a separate native link-share feature.

### Incoming link

```text
Shared preset                               ×

Warm afternoon
Kodak Gold 200 · Fuji Crystal Archive

[          Try on a sample photo          ]
[            Download preset             ]

No photograph is included in this link.
```

- Validate/decode the link and show the modal over the existing editor route.
- No sample download, photo decode or film render until Try is clicked.
- **Try on a sample photo:** apply the preset, open Sample 1, dismiss the modal
  and leave the user in the normal editor with the existing sample switcher.
  Opening their own photo afterward retains the look.
- **Download preset:** download the desktop-compatible file without opening a
  photograph; keep the modal available for trying afterward.
- **Close:** dismiss without applying. Do not let an unrelated later photo-open
  event silently apply a dismissed link.
- A link encountered in an active editor must not replace the user's photograph
  before an explicit Try action. Guard stale asynchronous operations.
- Broken/unsupported links show a specific reason and a way to continue into the
  editor. Handle navigation to a different preset fragment intentionally too.

## Compatibility audit

Audit sources: the frozen GUI-state preset shape, `presets.rs::FIELDS` and
`SECTIONS`, `web/controls.js`, `web/app.js`, and the current macOS inspector.
This is a source-level inventory, not a claim of completed browser render parity.

### Desktop-exposed preset settings missing from the web UI

| Setting | Runtime paths | Current situation |
| --- | --- | --- |
| RAW white balance | `io.raw_white_balance.{mode,temperature,tint}` | No web controls; web RAW load/decode currently hardcodes default WB. Needs UI and decode/re-decode plumbing. |
| Film development time | `film_render.development_time` | Shared engine supports it; no web picker. |
| Paper development time | `print_render.development_time` | Shared engine supports it; no web picker. |
| Input gamut algorithm | `io.input_gamut_compress.algorithm` | Shared engine supports it; no web picker. |
| Output gamut algorithm | `io.output_gamut_compress.algorithm` | Shared engine supports it; no web picker. |
| Spectral upsampling method | `settings.rgb_to_raw_method` | Shared engine supports it; no web picker. |

**Approved: add all the remaining pickers after WB**, so a user can inspect/edit
the same preset-facing controls as desktop. Do not make import dependent on a
field having UI.

### Required additional controls

- **Development**, beneath the Film picker in the existing Film/Paper area
  (Stock tab on mobile), bound to `film_render.development_time`.
- **Print development**, beneath the Paper picker, bound to
  `print_render.development_time`. Hide with Paper when scanning film directly.
- Both development pickers use the stock's `development_times` from the engine
  catalog (the macOS `Stock.times` property is an adapter name). Show only when
  more than one time is listed. Offer **Profile default** (stores `null`) and
  each listed time labelled in minutes. Reflect imported normalized times. Stock
  changes reset the applicable development-time override, as desktop does.
- Add a **Color Management** disclosure to Scan & Color / the existing mobile
  Scan tab, using existing choice/reset conventions:
  - **Output gamut**, `io.output_gamut_compress.algorithm`:
    `off`, `oklch`, `oklrab`, `cam16ucs`, `aces_rgc`.
  - **Input gamut**, `io.input_gamut_compress.algorithm`: `off`, `xy`.
  - **Spectral upsampling**, `settings.rgb_to_raw_method`:
    `hanatos2025`, `arctic2026beta04`, `mallett2019`.
- Use existing human-readable option titles, engine defaults, null semantics and
  parameter-change rendering/data-loading paths. Do not hardcode a second set of
  defaults, clamp imported precision to slider steps, or reset hidden gamut
  settings just because the algorithm picker changed.
- Controls must synchronize after import, Reset and stock changes, work without
  a loaded photo, and keep current desktop/mobile layout conventions. Import
  installs film/paper/params atomically; stock-change handlers must not erase the
  imported development times or other overrides.

The desktop CAT16 toggle is also absent in the web inspector, but it is NOT a
preset-carried setting (`presets::KEPT`), so it is not a preset compatibility gap
and is not required by this task. Do not expand this into unrelated output/input
colour-space UI or advanced controls hidden on desktop as well.

### Preset-carried values hidden in both the current web and macOS inspector

These are not additional missing algorithms to reimplement. Preserve their
normalized runtime values when importing/exporting, even without adding UI:

- Spectral settings: `settings.apply_hanatos2025_adaptation_window`,
  `settings.apply_hanatos2025_adaptation_surface`, `settings.spectral_gaussian_blur`.
- Grain: `film_render.grain.density_min`.
- Halation/scatter: `scatter_core_um`, `scatter_tail_um`, `scatter_tail_weight`,
  `boost_ev`, `boost_range`, `protect_ev`, `halation_strength`,
  `halation_first_sigma_um`, `halation_renormalize` under `film_render.halation`.
- DIR: `inhibition_samelayer`, `inhibition_interlayer`, `gamma_samelayer_rgb`,
  `gamma_interlayer_r_to_gb`, `gamma_interlayer_g_to_rb`,
  `gamma_interlayer_b_to_rg`, `langmuir_donor_k_rgb`, `langmuir_receiver_k_rgb`
  under `film_render.dir_couplers`.
- Camera: `camera.color_filter`, `camera.diffusion_filter.core_size`.
- Enlarger: `enlarger.illuminant`.
- Input gamut: `boundary`, `hull_detail`, `knee` under `io.input_gamut_compress`.
- Output gamut: `knee`, `lightness_compression` under `io.output_gamut_compress`.

Known shared-desktop refusal cases (for example jzazbz, non-sRGB preset output,
unsupported channel swaps) must stay refusals with the same reasons, not become
silent web fallbacks. Desktop compatibility means the same format and mapping;
it does not promise identical RAW pixels to Apple's desktop decoder.

## Technical design

- Reuse `presets::import`, `presets::apply`, `presets::export`; do not reproduce
  the mapping in JavaScript. Keep Python-style Infinity parsing in Rust.
- Add WASM entry points and worker/client adapters. Integrate stock/profile data
  prerequisites into the existing registered-data fetch/retry mechanism.
  `presets.rs::load_stock` currently calls `Path::is_file`; that native filesystem
  check needs a browser-compatible solution without weakening stock-ID validation
  or breaking native callers.
- Preserve every `presets::KEPT` field on apply, and all separate presentation
  state. Shared logic, not a second hand-maintained JS exclusion list, is the
  authority. Apply the incoming film/paper without automatic pairing side effects.
- Link format: a versioned compressed envelope containing a display name and
  desktop preset text, encoded URL-safely in `#preset=…`. Keeping the preset as
  text accommodates Infinity without round-tripping its fields through JS JSON.
- The URL fragment is not sent in the normal HTTP page request. It is not secret
  or encrypted; anyone with the link has the settings. Include no photograph,
  thumbnail, original photo file name, debug query parameters or other user data.
- No backend, short-link service or account. Bound encoded and decompressed sizes
  and validate before applying. Render names as text, never HTML. Measure real
  preset link sizes; if a preset exceeds the supported link budget, file download
  remains available with an explanation instead of a broken/truncated link.
- Add new runtime modules to offline/static packaging. Reuse Sample 1's existing
  offline policy; a preset link does not itself provide missing image/data assets.

## Implementation order and gates

1. **White balance — separate Sol 6.1 / xhigh task.** UI, shared-decoder plumbing,
   re-decode coordination, Reset and export correctness. Handoff linked above.
2. **Remaining desktop-exposed preset controls — approved and required.** Film
   and paper development, gamut algorithms, spectral upsampling, as specified
   above. No need to add controls for every advanced field hidden on desktop.
3. **WASM preset bridge.** Native/browser stock lookup, shared import/apply/export,
   lazy data dependencies, round-trip and refusal tests.
4. **Local preset UI.** Menu, one-file import, export modal, naming/download,
   reset-after-import tests. No library or new undo/revert system.
5. **Link in export modal + incoming modal.** Codec/version/limits, copy fallback,
   shared-link download, Sample 1 preview and navigation/cancellation behavior.
6. **Cross-device verification.** Desktop/browser interoperability, full-precision
   hidden values and null/Infinity handling; CPU/WebGPU; JPEG and RAW; mobile
   memory and iOS saving; keyboard/focus; offline errors; zero sample requests
   before an explicit Try; malformed/oversized input; import while busy.

## Implementation map

Read the current code before editing; this inventory describes the checkout at
handoff time, not permission to replace unrelated work.

| Area | Starting points |
| --- | --- |
| Shared preset mapping, validation and exclusions | `crates/spektrafilm-core/src/presets.rs`, its tests, `data/presets/spektrafilm-gui-default-28bf883.json` |
| Desktop apply/export integration | `crates/spektrafilm-native/src/main.rs` preset commands; `macos/Sources/NegsApp/EditorModel.swift` preset methods |
| Browser data abstraction | `crates/spektrafilm-core/src/data_fs.rs`, `crates/spektrafilm-wasm/src/lib.rs` |
| WASM worker calls and lazy dependencies | `web/engine.js`, `web/worker.js`; existing Balance Print flow is a useful model |
| UI state, Reset, rendering, photo open/export | `web/app.js` |
| Lab controls and option labels | `web/controls.js`; desktop reference `macos/Sources/NegsApp/Inspector.swift` |
| Toolbar, styles, mobile layouts and new dialogs | `web/index.html` |
| Sample 1 and switching/cancellation | `web/samples.js`, `web/sample/README.md` |
| Build, static/offline shell | `web/build.sh`, `web/offline.mjs`, `web/package-static.mjs` |
| Unit/browser checks | `web/checks/`, `web/cdp.mjs`, `web/safari.mjs`, `web/node-test.mjs` |

Keep preset serialization/link codec/UI concerns in focused modules where that
fits the existing plain-JS architecture; do not add a framework or backend.

## Acceptance criteria

### White balance and added controls

- Complete the linked WB task and its processing, UI, race, memory and export
  checks first. A JSON-only implementation is not complete.
- Development pickers show applicable catalog times, retain null for Profile
  default, reflect imported values, and disappear for fixed-time stocks.
  Cover both a multi-time film and a multi-time printing stock, plus scan-film
  mode and stock-change resets.
- Input/output gamut and spectral-upsample choices update live params, render
  correctly on CPU/WebGPU, load required assets, and reflect imported values.
  A failed/offline dependency must be reported rather than silently substituting
  an algorithm. Per-control Reset and Reset Adjustments restore defaults.

### Presets and fidelity

- A single desktop-compatible file can be imported on a local photo or Sample 1;
  importing a second replaces the Lab look without creating a list.
- Shared-Rust normalization/application covers all mapped fields, including
  hidden fields, stock overrides, B&W values, nulls and infinite Langmuir knees.
  Compare normalized render inputs or deterministic renders as appropriate;
  exported stock-resolved values need not be textually equal to unset originals.
- Exported files load through the native importer, and native-exported files
  apply through the browser bridge. Do not bundle unlicensed community presets
  as fixtures; use generated test presets unless licensing permits otherwise.
- Applying preserves Post and every shared `KEPT` field plus separate crop,
  rotation, border and viewing state. Verify no auto-pair/reset handlers corrupt
  the imported film, paper, direct-scan route or overrides.
- Reset after import clears all Lab overrides (even hidden ones), resets Post as
  it already does, preserves film/paper and presentation settings, and actually
  re-develops a RAW when needed.
- Syntax/type/shape errors, unsupported settings and unknown stock IDs produce
  useful errors before mutating the edit. Preserve shared desktop sparse-file
  and Infinity semantics; do not invent stricter/different JS parsing rules.
- Import/apply/export and photo-open transactions must be coherent while busy.
  Disable conflicting actions or sequence them with generation guards. On a RAW
  re-decode failure, do not leave an apparently applied preset paired with old
  pixels; report failure and leave a coherent edit. No new visible Undo/Revert.

### Export dialog and shared links

- The menu has exactly Import preset and Export preset; no separate Share item.
  The export modal automatically offers one current-settings snapshot as both
  desktop `.json` and a copyable link. Naming updates keep file/link consistent;
  stale asynchronous link generation cannot overwrite a newer name/payload.
- Exercise the versioned codec with Unicode names, full-precision preset text
  and Infinity; verify both compressed and decompressed size limits, malformed
  encoding, truncated input, unknown versions and safe text rendering.
- Set and document the codec/version and link-size limits based on measured
  generated presets. Use a browser-compatible compression implementation; if
  relying on platform compression APIs, test target browsers and provide a clear
  file-download fallback rather than an unhandled missing-API failure.
- Copied links open in a new editor tab with the incoming modal; downloading
  requires no photograph. Trying opens Sample 1 with the look, never the sender's
  sample selection. Switching samples/opening a local photo retains the look.
- Initial shared-link arrival, Download and Close cause zero sample-photo
  requests. Test Try when Sample 1 is already open as well: its picker may skip
  reopening the same ID, but the new preset must still apply/re-render.
- Escape/Close do not apply. Fragment navigation, a newer link, photo open or
  dismissal cannot allow a stale pending operation to apply later. Existing
  photograph/edits are untouched until the user explicitly chooses Try.
- Keyboard operation, focus trapping/restoration, screen-reader labels, outside
  menu dismissal and narrow/coarse-pointer layout work. Dialog keyboard events
  must not also trigger the editor's global crop/rotate/original shortcuts.
- Check clipboard fallback, iOS file saving, offline cached/missing-asset cases,
  normal/threaded WASM builds, and static/offline packaging of new modules.

## Verification and delivery

Use existing harnesses and add focused Node unit tests plus browser checks for
the new feature. Useful baseline commands (run from the repo root):

```sh
cargo test -p spektrafilm-core presets
cargo test -p spektrafilm-io
bash web/build.sh --threads
node --test web/checks/*.test.mjs
node web/checks/samples.mjs
```

The WASM build is required to check wasm32-gated code; a native-only check is not
enough. Run the added browser checks with the existing CDP/Safari harnesses when
available, and include exact commands. Do not claim browser/device verification
that was not actually performed. Record remaining limitations and unavailable
physical-device checks.

Deliver a concise summary of changed paths, completed phases, tests/results,
link codec/size decisions, any compatibility limits and screenshots of the menu,
export modal, incoming modal and added controls if a browser is available.

Implementation is approved for the receiving agent. This handoff preparation
changed documentation only and did not launch an agent. Preserve unrelated user
changes in the dirty checkout. Do not commit, push or deploy without approval.
