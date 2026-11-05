# Linux feature port after 0.13.0

Implemented 2026-11-04/05 against `81578d0` (the approved
[feature inventory and plan](linux-prep-port-plan.md)). This ports the current
macOS/Windows functionality into GTK/libadwaita. It does not include the later
visual polish pass or a new release. The final integration includes the newer
Fotufilm work on `main` at `9fa749b`, preserving its 52 films and 10 papers.

## Features

- Prep / Lab / Output, with Prep first and film/paper pinned above the tabs.
  Stock grids scroll within a bounded height, open on the current stock and
  open one at a time. Direct scanning shows that paper is skipped.
- Prep Exposure, Highlights, Shadows, Contrast, Lift, Gain, Color Boost,
  Saturation and Midtone Detail; RAW white balance, input interpretation and
  lens corrections live in Prep too. All use the existing shared processing.
- Live Match camera/original brightness. The Exposure control edits an offset
  above the solved value. Look changes re-solve; user Prep edits do not.
  Superseded replies cannot apply, failures have short notes, and saves/exports
  wait for the match and its settled preview. Automatic matching is a setting.
- Lab basics and Show all controls, with changed advanced controls kept visible;
  stills film formats and Custom; Negative exposure naming. Output contains
  Image Size and Border.
- Start fresh / Keep the previous edits; preview-limit and low-memory export
  settings outside undo/reset. Low-memory grain applies to exports only.
- Resizable Histogram / RGB waveform, remembered mode/height, bounded worker
  sampling and crop/straightening/quarter-turn handling.
- Print Curves graph with RGB/R/G/B channels, exposure and gamma handles,
  local prediction while awaiting the helper, drag/reset/keyboard support and
  one undo step per drag. Hidden graphs submit no helper work.
- Favourite films/papers in grids and simple menus, accessible star/context
  actions and favourite-first thumbnail scheduling.
- Shared control/group tooltips, including the new controls; native numeric
  fields and sliders expose the same help as accessible descriptions. Border,
  crop, export and settings controls have help too.
- Fit capped at physical 1:1; usable window/sidebar minimums; optional custom
  viewer background through GTK's colour chooser.

## State and compatibility

Linux session schema is now 4. Older Post tone/saturation values are removed
from both saved params and the control-value overrides. Restoring such a session
neutralizes those finished-image adjustments, matching macOS/Windows. There is
no exact conversion to pre-film Prep; release notes state this explicitly.

Old sessions without brightness-match metadata keep matching off. A saved match
is retained on restoration if its dependencies are current. Explicit new opens
and in-place source changes use a new reference/solution; an in-place refresh
otherwise keeps the current edit. Start fresh retains film/paper, Image Size and
output colour, while Keep carries the user's exposure offset to the next photo.
Both policies reset framing, lens corrections and input interpretation.

Preferences are projected into current state and worker/cache snapshots after
undo/reset. In particular, the preview limit used by the launch cache agrees
with the actual render request. Preset import keeps Prep, match metadata and
preview/export preferences. LUT export continues to exclude Prep.

## Helper integration

The GTK workspace still does not link the processing/RAW dependency graph.
The native helper advertises `brightness_reference` alongside `place_exposure`:

```json
{"command":"brightness_reference","input":"photo.nef",
 "reference_path":"scratch/reference.png","expected_width":6048,"expected_height":4032}
```

Dimensions describe the oriented decoded source. A usable result contains
`usable: true`, reference width/height and path; an unsuitable reference returns
`usable: false`, a human-readable `reason` and `for_photo: true`. Ordinary I/O
failures retain the normal helper error reply.

RAW extraction uses `spektrafilm_io::brightness::camera_preview`: the actual
embedded preview, orientation once, shape/letterbox checks and the common sRGB/
Adobe RGB/unsupported-space rules. JPEG/PNG/TIFF references use the shared
profile-aware loader, are bounded to 512 pixels and are written with an sRGB
profile. Alpha/floating-point references are refused. Creating the reference
does not replace the retained render or decoded source.

Main previews and required matching precede optional graph/stock work. Matching
coalesces edits, applies no independent undo action, preserves the user's offset
and cancels delayed output when its source is superseded. Output waits are
bounded to 30 seconds; a timeout asks the user to wait and save again.

## Validation

Local checks use isolated `NEGS_PROFILE_DIR` directories. Private RAW pixels,
screenshots, logs and scratch files remain outside git, under `target/` or `/tmp`.

| Check | Result |
|---|---|
| Release helper, Fast exporter, separate f64 Reference exporter and GTK app | Built with locked, offline dependencies |
| Linux unit suite | 65 passed, 10 opt-in tests ignored |
| Core/native/IO suites, including Prep/GPU/grain tests | 320 passed, 3 opt-in IO tests ignored |
| Live helper schema, preset import/export and desktop matching lifecycle | 4 passed |
| Desktop scopes/graph interactions | Passed, including deferred gesture cancellation under a history read borrow |
| Packaged-engine-style CPU smoke | 40 requests passed across the 62-profile catalog, including Fast/f64 exports |
| Release generator tests | 6 passed |
| Flatpak Cargo sources and release metadata | Passed |
| Full-app controls/crop/undo/session audit | 13 checks passed |
| Desktop engine/RAW/export audit | 14 checks passed |
| Viewer/export smoke | 7 checks passed, including real 1600 × 1067 output |
| Native GTK stock/favourites test | Passed |
| Scrolled Output layout regression | Passed |
| Clean Flatpak and AppImage CI | Passed, including installation/runtime smoke checks |
| Arch CI at `0631b9e` | GTK test compilation failed on an omitted tooltip reference; fixed locally in `9dd3b0f` |
| Exact Linux source archive after the local fix | 65 passed, 10 ignored, including strict tooltip parity |

The broader shared test run uses `OPENBLAS_NUM_THREADS=1`, as Linux CI does,
with normal device/runtime access. An earlier sandboxed run segfaulted during
GPU grain tests; no core was recorded. The normal-access run passed all 13 grain
tests and the rest of the shared-test matrix. No engine math or numerical gate was
changed in response. Linux helper/temp-file tests also require runtime-directory
write access that the workspace sandbox does not provide.

### RAW brightness checks

Portra 400 / Portra Endura, grain disabled, CPU helper:

| Local sample | Oriented source | Reference | Solved exposure |
|---|---|---|---|
| NEF | 6048 × 4032 | 512 × 341 | −0.33 EV |
| Portrait ARW | 4000 × 6000 | 342 × 512 | +0.78 EV, highlight-limited |
| RW2 | 6000 × 4000 | 512 × 341 | −0.37 EV |
| Specific ORF | 5184 × 3888 | Unavailable from shared reader | Correct per-photo failure note |

Retained Save was byte-identical before/after reference creation and solving
(or refusal) for all four. Report:
`target/linux-validation/prep-port/brightness-raw/results.json`.

### Limits

- A RAW that the shared reader cannot extract a usable embedded preview from
  cannot be matched; manual Prep remains usable. This is not a fallback RAW
  development masquerading as a camera reference.
- The tone graph retains the upstream prototype's limits: its film histogram
  ignores crop/spatial effects, side-handle prediction is approximate, and
  agreement of the curve with GPU output remains unverified.
- Full GPU Prep is still a separate experiment. This port uses the existing
  shared CPU spatial processing, prepared-frame cache and exposure-only GPU fold.
- Mixed-monitor fractional scales, other GPU vendors and a full screen-reader
  walkthrough require additional hardware/user validation.
- The clean Arch/Flatpak/AppImage workflow and installed-package checks must
  identify the committed source they validate. Local development builds alone
  do not establish those distribution results.

### Final validation update

The final integration re-ran the Linux and shared suites, live helper/preset/
matching tests and CPU smoke against the combined source. The stock GTK test
covers real expanded/star callbacks, one open grid, favourite-first menus/grids,
unchanged selection/history, retained thumbnails and persistence. The viewer
smoke confirms exact custom-background compositor pixels, native colour chooser,
partial SpinButton input and a real 1600 × 1067 portal export.

The desktop engine audit checks retained Save versus independent Fast byte for
byte, f64 Reference export, crop/straighten/rotation/border geometry, P3/16-bit
output, JPEG quality, cancellation, RAW custom white balance and Nikon metadata.
Its new-source guard waits for asynchronous Open before testing RAW dimensions.
The Output layout test uses a bounded real scroller because Output is shorter
than the old Lab layout.

The early pre-port startup capture had an unavailable helper and is not a valid
processing-performance baseline. Final working-build timings are recorded
separately under `target/linux-validation/prep-port/after/`; no before/after
speedup claim is made. Two warm launches on `docs/card.jpg` (1320 × 900,
1.25× display scale) presented their first frame at 371–376 ms and a developed
image at 1.05–1.14 s; the restored cached frame appeared at 421 ms.

[Linux CI run 41988059600](https://github.com/noahhaasis/negs/actions/runs/41988059600)
validated `0631b9e`. Flatpak built, installed and passed its sandbox checks;
AppImage passed Ubuntu, Fedora and Arch runtime smoke checks. The Arch job
compiled/tested the shared engine, then failed to compile the GTK tests because
the source archive omitted `macos/Sources/NegsApp/ControlHelp.swift`.

Local fix `9dd3b0f` adds only that Swift text reference to the archive allowlist,
preserves the strict tooltip parity test and adds a release regression assertion
that no other macOS files are included. The Linux port documentation is included
as well. All six release-generator tests pass. The exact archive was generated
from an isolated checkout, extracted, and its release GTK suite passed 65 tests
(10 opt-in tests ignored). Evidence is under
`target/linux-validation/prep-port/archive-local-fix/` and `archive-tests.log`.
This packaging fix is local; remote Arch CI has not been rerun with it.
