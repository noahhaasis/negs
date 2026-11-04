# Handoff: film and paper previews in the macOS app

Prepared 2026-11-01 for an agent on Noah's Mac. Port the Linux feature described in
[stock-previews.md](stock-previews.md) (read that first; it's the spec) to the SwiftUI app
in `macos/`. A Windows agent may be porting it at the same time on the same branch.

## Where to work

- Branch: `stock-previews` on `origin` (github.com/noahhaasis/negs). Fetch first. If the
  main checkout has uncommitted work, don't disturb it; use a worktree
  (`git worktree add ../negs-stock-previews stock-previews`).
- Commit on `stock-previews`. Fetch and rebase onto the other agent's commits before you
  push (never force-push over them). **Don't merge into main.** Pushing the branch is fine.
- End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Noah is the product owner: report what users get and the measured costs, not film-science
  detail.

## Read first

1. [stock-previews.md](stock-previews.md): behaviour, helper protocol, Linux numbers.
2. The Linux implementation as the behavioural reference: `linux/src/stock_browser.rs`
   (scheduling in `pump`, staleness, setting) and its call sites in `inspector.rs`,
   `preferences.rs`, `runtime.rs`.
3. The helper side: `Command::Thumbnail` / `ReleaseThumbnails` and `Session::thumbnail` in
   `crates/spektrafilm-native/src/main.rs`.
4. [native-macos-ui.md](native-macos-ui.md) and the Mac sources:
   `Inspector.swift` (`Section("Film & Paper")`, the two `Picker`s around line 473),
   `EditorModel.swift` (`chooseFilm` / `choosePaper` / `pairFilm`, the `render` request
   around line 803, render scheduling, `previewCurrent`, interaction state, source
   generation), `Engine.swift` (helper requests), `MainMenu.swift` / `Updates.swift`
   (`showSettings`, `UpdateSettings`: the Settings window), `Session.swift` (preferences).

## What to build

1. **Rebuild the helper** from the branch first: the new commands live in it, and the
   parallel gamut remap is in `spektrafilm-core`. Check that the main render is unchanged
   on a couple of photos (hash the preview buffer against `main`'s helper, as was done on
   Linux).
2. **Thumbnail requests from `EditorModel`.** For a stock, take the current `EditState`, apply
   the same change `chooseFilm` / `choosePaper` makes (factor out a pure function rather than
   duplicating it), build the params exactly as the main render does, and add `crop` when
   there is a crop or straighten. Key each tile by that request plus the source generation.
   Send one request at a time, only when the main render is idle and current and no slider is
   being dragged. Resume after each main render finishes, and mark tiles stale (dim / blank)
   as soon as a render starts. Decode the RGBA like the main preview, with the same colour
   space handling.
3. **UI in `Inspector.swift`:** a `DisclosureGroup` (collapsed by default) per stock, with the
   current stock name trailing on the header row, containing a two-column `LazyVGrid` with
   section headings. Tiles are buttons: image (aspect fill, rounded, grey placeholder),
   caption, tooltip (`.help`), accent outline when selected. Match the Mac's native look
   rather than copying GTK pixel for pixel. Keep the Picker path for the setting-off case.
4. **Setting:** a "Film and paper previews" toggle in the Settings window, persisted with
   the other preferences, default on. Off: show the Pickers, drop the images, send
   `release_thumbnails`.
5. **Tests** in `macos/Tests/NegsTests`: the pure "state with this stock chosen" function
   (film pairs its paper and resets presets; paper keeps the film), the tile key changing with
   crop and with the photograph, and the stale-vs-placeholder decision.

## Measure and report

Same rows as the Linux table in [stock-previews.md](stock-previews.md), on the Mac, with a
24 MP photo: first open (all 33), after an exposure edit, with an 80 % + 3° crop, memory
(app and helper) with the grids collapsed and open vs the setting off, and startup (time
to window and to first developed image, previews on vs `main`). `linux/bench_stock_previews.py`
runs against the Mac's helper unchanged (it starts `target/release/spektrafilm-native`;
needs Python 3). Add a "macOS results" section to this file with the numbers and
screenshots' paths, commit, push the branch, and tell Noah.

Watch for: the Mac's helper backend (Metal via wgpu) may make `process` faster or slower than
Linux's ~7 ms. If first open is much slower than ~2 s, report it before tuning.

## macOS results

Done 2026-11-01 on Noah's M1 Pro (on AC power, Low Power Mode off). Another Claude session
was launching Negs builds at the same time (load average 3–7), so treat timings as ±10 %.
Photographs: `~/Downloads/DSC_0144.jpg` and `DSC_3471.jpg`, both 6048 × 4032 JPEGs (24 MP).
23 films + 10 papers, 320 px thumbnails, release builds with `MACOSX_DEPLOYMENT_TARGET=14.0`.

What users get: Film and Paper are collapsed foldouts with the current stock on the right.
Opened, each shows a two-column grid of the photograph under every stock, grouped like the
old menus, with the current one outlined in the accent colour. A click is the same "Change
Film" / "Change Paper" undo step as the menu. Settings has "Film and paper previews" (on by
default); off brings back the menus at once.

Screenshots: [grids open](stock-previews-macos/grids-open.jpg),
[tiles following a 50 % crop](stock-previews-macos/cropped-50.jpg),
[collapsed](stock-previews-macos/collapsed.png),
[setting off](stock-previews-macos/setting-off.png).

### In the app

| | macOS | Linux (for comparison) |
|---|---|---|
| First open, all 33 thumbnails | **1.2 s** (37 ms each), starting once the main preview is done | ~2 s |
| After an exposure edit (pipelines cached) | **0.5 s** for all 33 (15 ms each) | ~0.25 s |
| Crop 80 % + 3° / tight 50 % crop | **0.6 s / 0.7 s** for all 33 | ~0.4 s / ~0.7 s |
| Slider drag with both grids open | first drag frame waits for at most one thumbnail (≤ ~40 ms); 29–30 live frames in a 4 s drag vs 31–32 with the grids closed; main-thread hitches unchanged (p99 ~23 ms) | ≤ ~50 ms |
| Memory, grids collapsed vs setting off | **+0** (app 374 vs 375 MB, helper 823 vs 822 MB) | +0 |
| Memory, both grids open | app **+15–45 MB**, helper **+30–45 MB** (app 388–418 MB, helper 851–866 MB) | helper ≈ +60 MB, app ≈ +15 MB |
| Window on screen (from exec, median of 5, alternating) | **497 ms** vs 508 ms on `main` | unchanged |
| First developed image (from launch, median of 5) | **1.99 s** vs 2.05 s on `main` | — |

Memory is the physical footprint (Activity Monitor's column) 8 s after things settle. The
first developed image got slightly faster on the branch because of the helper's parallel
gamut remap (main render 1.36 s vs 1.42 s), not because of the previews. The Mac's
thumbnails are slower per tile than Linux's after an edit (Metal `process` ≈ 12–14 ms vs
~7 ms) but much faster cold (pipeline build ≈ 16 ms vs ~45 ms), so first open is quicker.

### Helper only (`linux/bench_stock_previews.py DSC_0144.jpg 320`)

| | films (23) | papers (10) | per thumbnail |
|---|---|---|---|
| Cold | 718 ms | 306 ms | median 31 ms (pipeline 16, process 14) |
| After an exposure edit | 331 ms | 137 ms | median 14 ms |
| Crop 80 % + 3° | 346 ms | 131 ms | median 15 ms |
| Crop 50 % | 389 ms | 166 ms | median 16 ms |

The main re-render after all thumbnails was still a cache hit (82 ms), and the main preview
buffer is byte-identical to `main`'s helper (2 photos × 2 stocks, glare off, SHA-256).
The script needed a macOS memory fallback (`ps` + `footprint`); `/proc` doesn't exist here.

### Implementation

- `macos/Sources/NegsApp/Previews.swift`: `PreviewRules` (the pure "state with this
  stock chosen", now also used by `chooseFilm` / `choosePaper`; the thumbnail request; the
  current / dimmed / placeholder decision), `PreviewTile` (one observable per tile, so a
  thumbnail arriving redraws only its tile), `Previews` (tiles, open state, setting in
  UserDefaults `previews`, missing = on) and the SwiftUI foldout and grid.
- `EditorModel.pumpPreviews()`: the Linux `pump`. Called after every render finishes,
  on every edit (dims at once; a new photograph blanks at once), when a crop changes (no
  main render, but the thumbnails change), when a drag ends and when a grid opens or
  appears. It sends one `thumbnail` at a time, only when the preview is current, nothing is
  rendering or pending and no slider is dragged. Thumbnails become sRGB- or
  Rec. 2020-tagged `CGImage`s, the same colour handling as the main preview.
- `Inspector.swift` shows the foldouts or the Pickers; `Updates.swift` (Settings window) has
  the toggle (`EditorModel.setPreviews`; off clears the tiles and sends
  `release_thumbnails`). SwiftUI builds only the visible control, which is cheap here: a
  collapsed foldout builds no tiles, and the pop-up it replaces is the expensive control.
- Tests: `macos/Tests/NegsTests/PreviewTests.swift` (choosing a film pairs its paper and
  resets presets, a paper keeps the film, the tile key follows crop/straighten/photograph/
  edits but not the border, dim vs blank, failures aren't retried, missing setting = on).
  The full suite passes except the two `InspectorLayoutTests` that fail on `main` too.

The app numbers came from a benchmark build (`-DSPEKTRAFILM_BENCHMARK`) with
`StockPreviewBenchmark.swift`, `NEGS_STOCK_EXPAND` and `NEGS_STOCK_THUMB_LOG`, all
removed on 2026-11-02 once all three platforms were measured; startup and drags used the existing `SPEKTRAFILM_BENCHMARK_PHOTO` drag
benchmark, with a `CGWindowList` poller for exec → window.

Not done: no notarized/packaged bundle was built, and an Intel Mac wasn't tried.

## Preset previews (macOS)

Added 2026-11-01/02 (spec: [Preset previews](stock-previews.md#preset-previews-macos-linux-and-windows-done)).
Measured with Noah's 12 presets (8 daylight, 3 as-shot, 1 custom white balance), release
builds, on AC power.

| | |
|---|---|
| 24 MP JPEG, all 12 thumbnails | **0.48 s** (30–57 ms each) |
| 60 MP DNG, all 12 thumbnails | **3.2 s**: the 3 that share the photo's white balance first (~0.1 s), then one ~1.5 s RAW decode per other white balance (2 here) followed by the presets it serves (~30 ms each) |
| Crop change on that DNG | **15–21 ms** per preset (no new decode); main render still a cache hit afterwards |
| Helper memory on that DNG | **3.8 GB vs 2.1 GB** with previews off: the extra decodes' freed buffers stay with the allocator; the kept copies are ~20 MB |
| Pixels vs a full-resolution reduction | within 1/255 (0.3 % of values differ by 1) |
| Lab edit | preset tiles untouched (they depend only on the kept settings) |

Screenshot: [presets on a RAW](stock-previews-macos/presets-raw.jpg).

Decided with Noah (2026-11-02): keep exact thumbnails for presets with another white
balance, but render them last and don't decode again after a crop. While such a decode
runs (~1.5 s, once per white balance per photograph) the helper is busy, so a slider drag
started then waits for it. The extra memory stays (mimalloc would likely return it; see
the macOS malloc notes).

Implementation: `PreviewRules.presetCommand` / `presetKey`, `PreviewTile` (shared by
stock and preset tiles), `Previews.presets` / `moveTile`, `EditorModel.presetTexts`,
the preset part of `pumpPreviews`, `PresetThumbnail` in `PresetInspector.swift`,
`PreviewRules.renderOrder`; the helper's `thumbnail` (`preset`, `own_decode`, `WbSource`),
`RawWhiteBalance::effective` and its test
`preset_thumbnails_apply_the_preset_like_preset_import`.
