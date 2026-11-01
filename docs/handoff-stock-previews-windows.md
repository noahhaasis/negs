# Handoff: film and paper previews in the Windows app

Prepared 2026-11-01 for an agent on Noah's Windows test box (checkout
`C:\Users\Noah\Projects\negs`). Port the Linux feature described in
[stock-previews.md](stock-previews.md) (read that first; it's the spec) to the Win32 app in
`windows/`. A macOS agent may be porting it at the same time on the same branch.

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
   updates). Add the "Film and paper previews" checkbox there with the subtitle text from the
   spec, persisted in `session::Preferences` with `#[serde(default)]` → on. Off: show the
   combos, free the bitmaps, send `release_thumbnails`.
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
