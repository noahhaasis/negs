# Handoff: film and paper previews in the Windows app

Prepared 2026-11-01 for an agent on Noah's Windows test box (checkout
`C:\Users\Noah\Projects\negs`). Port the Linux feature described in
[stock-previews.md](stock-previews.md) (read that first; it's the spec) to the Win32 app in
`windows/`. A macOS agent may be porting it at the same time on the same branch.

## Update 2026-11-02: presets too, and you finish the feature

Since this was written, the Mac app got **preset previews** (a thumbnail of the photograph
with each preset applied, in the Presets panel), the setting was renamed to cover them, and
the code names went neutral. Please:

1. **Build preset previews on Windows as well** (`ui/presets.rs`), after or together with
   the film/paper grids. Spec: [Preset previews](stock-previews.md#preset-previews-macos-done-linux-and-windows-to-follow).
   The helper side is done (`thumbnail` takes `"preset"`; the catalog lists `preset_kept`).
   Reference: the Mac app (`macos/Sources/NegsApp/Previews.swift`: `PreviewRules.presetCommand`
   / `presetKey` / `renderOrder`; the preset part of `EditorModel.pumpPreviews`;
   `PresetThumbnail` in `PresetInspector.swift`) and [its results](handoff-stock-previews-macos.md#preset-previews-macos).
   Key points: key preset tiles on the kept settings only (Lab edits mustn't redraw them),
   wait until the engine has checked every preset file, and on a RAW render presets with
   another white balance last, grouped by white balance.
2. **Setting:** label "Film, paper and preset previews", subtitle "Show each film, paper and
   preset on your photograph. Turn off to use simple menus and lists on slower computers."
   (let it wrap). Name the preference neutrally: `previews` (the Mac uses the key
   `previews`), not `stock_previews`. Off also gives the plain preset list.
3. **You're the last agent on this branch, so finish it** (section
   [Finishing the feature](#finishing-the-feature-last-agent) at the end of this file):
   remove every experiment-only hook on all three platforms and tell Noah what's left.

## Where to work

- Branch: `stock-previews` on `origin` (github.com/noahhaasis/negs). Check `git status`
  first and don't disturb uncommitted work; use a worktree:
  `git -C C:\Users\Noah\Projects\negs fetch origin` then
  `git -C C:\Users\Noah\Projects\negs worktree add ..\negs-stock-previews stock-previews`.
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
4. [windows-app.md](windows-app.md) and [windows-agent-handoff.md](windows-agent-handoff.md)
   (building the helper, tests, packaging), then the sources: `model.rs` (`choose_film`,
   `choose_paper`, `pair_film`, the `render` request around line 1616, render scheduling,
   source generation), `engine.rs` (`request`, `timeout_for`), `ui/inspector.rs`
   (the Film/Paper owner-drawn combos, `fill_stocks`, `select_stock`, row layout and
   `When` visibility), `ui/dialogs.rs` (`settings`: currently only the update task dialog),
   `session.rs` (`Preferences`), and `thumbnail.rs` / `ui/strip.rs` (how the film strip
   already draws bitmaps; reuse that).

## What to build

1. **Rebuild the helper** from the branch first: the new commands live in it, and the
   parallel gamut remap is in `spektrafilm-core`. Run the existing Windows tests, and
   check that the main render is unchanged on a couple of photos (hash the preview buffer
   against `main`'s helper, as was done on Linux).
2. **Thumbnail requests from `Model`.** For a stock, take the current `EditState`, apply the
   same change `choose_film` / `choose_paper` makes (factor out a pure function rather than
   duplicating it), build the params exactly as the main render does, and add `crop` when
   there is a crop or straighten. Key each tile by that request plus the source generation.
   Send one request at a time, only when the main render is idle and current and no slider is
   being dragged. Resume after each main render finishes, and mark tiles stale (dim / blank)
   as soon as a render starts. Decode the RGBA like the main preview, through the same ICC
   path.
3. **UI in `ui/inspector.rs`:** a foldout row per stock (title, current stock name next to a
   chevron, collapsed by default). The inspector is laid out by hand, so an expanded
   foldout must push the rows below it down and update the scroll range. Inside: section
   headings and a two-column grid of owner-drawn tiles (image cropped to fill, rounded,
   grey placeholder, caption with ellipsis, tooltip, accent outline when selected; dimmed
   when stale). Tiles need keyboard focus and Enter/Space to choose, with accessible
   names. Keep the combo path for the setting-off case.
4. **Setting:** Windows has no real Settings dialog yet (`dialogs::settings` only covers
   updates). Add the "Film, paper and preset previews" checkbox there with the subtitle text
   from the spec, persisted in `session::Preferences` as `previews` with
   `#[serde(default)]` → on. Off: show the combos and the plain preset list, free the
   bitmaps, send `release_thumbnails`.
5. **Tests** (`cargo test --locked --manifest-path windows/Cargo.toml`): the pure "state
   with this stock chosen" function (film pairs its paper and resets presets; paper keeps the
   film), the tile key changing with crop and with the photograph, the stale-vs-placeholder
   decision, and that old sessions without the preference load with previews on.

## Measure and report

Same rows as the Linux table in [stock-previews.md](stock-previews.md), on the Windows box,
with a 24 MP photo: first open (all 33), after an exposure edit, with an 80 % + 3° crop,
memory (app and helper, private working set) with the grids collapsed and open vs the setting
off, and startup with `windows/measure_startup.ps1`, previews on vs `main`.
`linux/bench_stock_previews.py` runs against the Windows helper if you change `helper` to
`target\release\spektrafilm-native.exe` (it only uses the JSON protocol; the memory readout
is Linux-only, so use Task Manager or `Get-Process` for that). Add a "Windows results" section
to this file with the numbers and screenshots' paths, commit, push the branch, and tell Noah.

Watch for: on Windows the BLAS-free `dgemm` fallback and the DX12/Vulkan backend may change
calibration and `process` times. If first open is much slower than ~2 s, report it before
tuning.

## Finishing the feature (last agent)

Do this after the Windows results are committed, as its own commit.

**Remove the experiment-only hooks** (measurement aids, not product features; the spec
lists them too). Keep the per-thumbnail *error* line on stderr, it's ordinary diagnostics.

- Linux: `NEGS_STOCK_EXPAND`, `NEGS_STOCK_THUMB_PX`, `NEGS_STOCK_THUMB_LOG`,
  `NEGS_WIDGET_CAPTURE_MS` (grep `linux/src`), and the scripts `linux/bench_stock_previews.py`
  and `linux/measure_stock_browser.py`.
- macOS:
  - `NEGS_STOCK_EXPAND` (the grids opened at launch in `Previews.init`; they start
    collapsed without it).
  - `NEGS_STOCK_THUMB_LOG`: both log lines in `EditorModel.pumpPreviews`, with the
    `Previews.batch` / `lastBatch` bookkeeping that only feeds them.
  - `macos/Sources/NegsApp/StockPreviewBenchmark.swift` and its
    `StockPreviewBenchmark.start(model)` call in `NegsApp.swift`. The file compiles only
    with `-DSPEKTRAFILM_BENCHMARK`, so a plain build won't notice if you miss the call.
- Windows: whatever hooks you added while measuring.
- Docs: drop the "Experiment-only hooks" paragraphs in [stock-previews.md](stock-previews.md)
  and [the Mac handoff](handoff-stock-previews-macos.md), and the "measure with" tool
  mentions that point at the removed scripts.

**Verify what you can't build:**
- Linux: run the Linux workflow on the branch, `gh workflow run linux.yml --ref
  stock-previews`, and check that it passes.
- macOS: there is no CI for branch pushes. Keep the Swift edits to plain deletions and
  tell Noah they need a Mac build: `swift build --package-path macos`, the benchmark-flag
  build (`-Xswiftc -DSPEKTRAFILM_BENCHMARK`), and
  `swift test --package-path macos --filter PreviewTests`.

Linux already has preset previews, the new label and the `previews` preference
(2026-11-02), so nothing is left there besides the hooks above.

**Tell Noah what's left**, and which platforms' builds you couldn't run. Don't merge into
main; Noah decides that.
