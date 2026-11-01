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
