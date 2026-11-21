# Windows parity with the redesigned macOS app: survey and plan

Prepared 2026-11-18 against `main` at `83e8c79` (Release 0.17.0).

## Where things stand

- **Windows was last level with macOS on 7 November** (`0b696b6`, win-nov5: film search,
  held and quick frames). Since then macOS has had about 140 UI commits across 30 merges.
  The Windows app has had only release bumps and help text.
- **The Windows PC** (`ssh pc`, Windows 10 22H2, Ryzen 5 3600): `main` in
  `C:\Users\Noah\Projects\negs-presets` is fast-forwarded to `83e8c79`. The main checkout
  is on `crop-reuse-key` with untracked `docs/windows-nov5-port/`, so leave it alone.
- **Windows app:** `windows/`, about 29k lines of Rust. It uses Win32 common controls and
  paints its own controls through a GDI back buffer with its own anti-aliasing
  (`ui/draw.rs`). The canvas is Vulkan (wgpu). It already has a custom title band and
  light/dark themes. Startup medians are 106 ms to the first window and 257 ms to the
  inspector.
- **macOS app:** about 28k lines of Swift. The spec for the redesign is
  `design/proposals/IMPLEMENTATION.md` (T2 · Label) plus `IMPLEMENTATION-THEMES.md`. The
  reasons behind each decision are in `FEEDBACK.md`.
- **Engine work is already shared.** Negative scan, colour filters, the film-side cache,
  chroma NR, Markesteijn, thumbnail reads and match decodes are all in `crates/`.
  Windows only needs the front end for each.

## What Windows is missing

### The look (T2 · Label, themes, and later rounds)

| Area | macOS now | Windows now |
|---|---|---|
| Type and tokens | Newsreader (bundled, variable `opsz`) + system font. Pencil-yellow marks only, warm token table | Segoe UI, system light/dark |
| Themes | 5: Darkroom (default), Contact sheet, Light table, Daylight, Plain (+ Plain dark). View › Theme, Settings › Look | System light/dark |
| Window | No toolbar. A 38 pt title bar with the file name; everything else in menus, the status line, the leader and the pane header | Toolbar, menu bar, title band |
| Canvas | Caption (EXIF) above the photo. Under it the label "Film ⌄ printed on Paper ⌄" with the paper lock. Trim marks start crop. F = Image Only, hold T = upside down. Viewer sheets + light table. Fit view uses an exact Lanczos downscale | Plain canvas; crop from the toolbar; trilinear minify (ticket 38) |
| Film and paper | A tray along the bottom: try, keep or go back, one undo step, ← → step stocks, favourites, search, cached tile first | Fold-out grid in the inspector |
| Inspector | Styled scroll view: Prep · Lab · Output with a pencil underline and a count of edits per tab. Changed values in bold paper with the pencil under them, sliders filled from their default, small local reset, "N more" per module, hints, Export foot | Owner-drawn common controls |
| Status line | Words: size, develop time, library notes, export progress, Original ⌄, Fit and 100 % | Status bar |
| Small states | Empty window, dropping, the Developing tag, can't develop on the canvas, and an alert card under the title bar | Task dialogs |
| Presets | Tiles | List |
| Settings | Three tabs in the inspector's style (Photographs · Speed · Upkeep) + Look | Dialog |

### Features

| Feature | Ticket or merge | Depends on |
|---|---|---|
| **Library:** `.negs` sidecars, ratings, flags, filter, per-photo edits, Copy/Paste/Paste from Previous/Duplicate/Reset Edits, delete to the bin, batch export, xmp:Rating | 30 | — |
| **Developed thumbnails:** pump, 1 GB cache, outward from the view, ±32 read ahead, opens on last develop, Low Power | 35, 36 | Library |
| **Strip and grid:** strip at true proportions with the others dimmed, the leader, marks (pick box, hand-drawn stars, reject cross, edited mark, +JPG), the grid sheet with folding flight, filter sentence, click selects and double-click opens, G/E | T2 step 5, 36, #18 | Library, thumbnails |
| **Compare** two photographs (side by side or slider); Original ⌄ holds, splits or compares | 46 | Strip/grid selection |
| **RAW + JPEG pairs** as one photo; Original ⌄ can show the camera JPEG | 47 | Library |
| **Open in Lightroom / Lightroom Classic** (16-bit TIFFs) | 48 | Batch export |
| **Your negative** (print a scanned colour negative) | 33 | — |
| **B&W colour filters** in Lab › Camera | #21 | — |
| **Enlarger › Shifts move** (M/Y link) | — | — |
| **Paper lock**; a new paper prints a negative as Auto does | — | Label |
| **Fotufilm films setting** (All / From datasheets / None) | — | Settings |
| **Faster open:** match solved before anything renders, solved matches kept across photos and launches, preview-cache name worked out during the render | 39 | — |
| **Release the film-side cache** on memory pressure and on going back to the grid | 41 | Grid |
| **Exact fitted view** (Lanczos in linear light) | 38 (written for Windows) | — |
| **Window reopens at the size it was left at** | `2f620c5` | — |

## Approach

### How the new look is drawn on Windows

(Decided; see "Decided with Noah" at the end. The strip, grid and tray tiles go on wgpu.)
**Keep the Win32 windows and switch their painting from GDI to Direct2D +
DirectWrite.** Both ship with Windows 10. Every control stays a real HWND, so focus,
keys and UI Automation names don't change; only `draw.rs` and its callers do. GDI
can't do Newsreader's `opsz` axis, a smooth pencil stroke, rotated rings or the hand-drawn
stars and crosses. DirectWrite loads the bundled `Newsreader-Negs*.ttf` from memory
(`IDWriteInMemoryFontFileLoader`) without installing it, and sets the axes with
`IDWriteTextFormat3`. The existing back buffer can take an `ID2D1DCRenderTarget`, so
painting moves over one control at a time. The strip and grid get a
hardware `ID2D1HwndRenderTarget` each, so the flight animates smoothly. The canvas
stays wgpu.

The shapes are ported from the Swift as exact geometry: pencil path, rings,
`HandStar`, `RejectCross` (picked per file name, as on macOS). Each theme is one token
table in `ui/theme.rs`, matching `AppTheme.swift`.

### Where Windows differs (leaning into Windows)

- **Title bar:** the file name plus a compact menu row (File Edit Photo View Help),
  with Windows' own minimise, maximise and close on the right. Alt and F10 open the
  menus as usual. Snap layouts work on Windows 11.
- **Shortcuts:** ⌘ → Ctrl, ⌥ → Alt, ⌘⌫ → Del (to the Recycle Bin), F11 full screen,
  and the F6/F7 pane toggles stay. Plain keys (0–5, P X U, G E F, \\, T) stay as on macOS.
  Ctrl + wheel and precision-touchpad pinch resize the grid.
- **Plain theme:** on Windows it follows Windows' light/dark setting and Segoe UI
  Variable/Segoe UI. It's the "near native" theme, as Plain is on macOS.
- **Delete** uses `IFileOperation` to the Recycle Bin. **Low Power** maps to
  Windows Battery/Energy saver. **Memory pressure** comes from
  `CreateMemoryResourceNotification`. **Lightroom** is found through the App Paths
  registry entries.
- Dialogs that are Windows' own stay native: open and save, and the colour picker.

### The library as a shared crate

Put the sidecar store and the library logic in a new platform-free crate
(`crates/negs-library`). That covers reading and writing per `docs/sidecar-format.md`
(atomic, merge, unknown keys kept, refuse unreadable), ratings and flags, filter,
duplicate, paste, pairing, the thumbnail filing key and xmp:Rating. Its tests run on
the Mac in seconds, against `docs/sidecar-fixtures/`. Linux reuses it later. The app
keeps only the glue between Win32 and its own `state.rs`.

### Working loop (fast)

- Edit on the Mac in a worktree on branch `windows-parity`. Type-check with
  `cargo check --target x86_64-pc-windows-msvc --manifest-path windows/Cargo.toml`
  (the target is installed here).
- To run it, the Windows checkout runs `git fetch mac`, then builds with
  LTO off and 16 codegen units, the same as the Mac sessions.
- Take screenshots of the window only, in Noah's console session through a scheduled
  task, with `NEGS_PROFILE_DIR` set to a scratch folder and `NEGS_ALLOW_MULTIPLE=1`.
  The same photos are captured on an isolated macOS launch for side-by-side checks.

### Testing, kept lean

- **On every step:** a cross-check on the Mac, and a Windows build.
- **Pure logic** (library crate, tray model, layout maths) gets Rust unit tests that
  run in seconds on the Mac.
- **At the end of each phase:** one screenshot set compared with macOS, and one
  `measure_startup.ps1` run (6 runs) against the 0.17.0 baseline.
- **Only for phases that touch the frame pipeline (4, 8):** the slow Windows
  lifecycle suite (`model/lifecycle_tests.rs`).
- **Library:** a random-action state test in the shared crate, after macOS
  `LibraryStateTests`. Before Phase 6 ships, check that Mac-written sidecars open on
  Windows and Windows-written ones open on the Mac.
- **Not ported:** the TLA+ model. It describes the Swift `EditorModel`.

### Performance rules

- The startup medians must not regress: about 106 ms to the first window and 257 ms
  to the inspector.
- Build the inspector after the first frame and only the current tab. Don't build it
  in the empty state.
- Build the tray, grid, Settings and alert card on first use.
- Create the DirectWrite factory and load the font off the first-frame path.
- Cache text layouts and geometry; painting allocates nothing.
- Develop thumbnails off the UI thread, in a pool sized to the PC's cores and RAM.
  Only on-screen rows are painted in the grid.

## Order

Design first, as asked. The strip and grid come once the library they display exists.

| # | Phase | Contents | Size |
|---|---|---|---|
| 0 | Baseline | Windows `main` built. Startup baseline. Reference screenshots of both apps on the same photos. The scheduled-task screenshot helper | S |
| 1 | Drawing foundation | Direct2D/DirectWrite in `draw.rs`, Newsreader from memory, 5-theme token table, pencil, rings, stars and cross. No visible change yet; measure launch | M |
| 2 | Window shell | 38 DIP title bar with the file name and menu row. No toolbar; its items move to menus (Ctrl shortcuts). Status line in words. Alert card replaces task dialogs. Small states | M |
| 3 | Inspector | Row styles (slider, tick box, choice, readout, hint, number field, colour well, modules, N more). Pane header with Prep · Lab · Output, edit counts and struck tab. Export foot. Focus box. Presets tiles. Settings as tabs + Look (themes, sheets, Fotufilm films setting) | L |
| 4 | Canvas | Caption and label (paper lock), trim-mark crop + crop panel, Image Only (F), hold T, viewer sheets + light table, exact Lanczos fit (ticket 38), window size restored | M |
| 5 | Tray | Film/paper tray with try, keep or back, one undo, Esc order, stock arrows, favourites, search, cached tile, the stand-in turned like the photo. The inspector's fold-out goes | M |
| 6 | Library | `crates/negs-library` + glue: sidecars, ratings and flags (keys), per-photo edits, Copy/Paste/Duplicate/Reset, Recycle Bin, undo, filter, RAW+JPEG pairs, batch export + xmp:Rating | L |
| 7 | Strip and grid | Strip at true proportions with the leader and marks. Grid sheet with packed rows, filter sentence, foot slider, selection, the fold flight, G/E/Esc. Developed thumbnails (pump, cache, prefetch, Low Power). Opens on the last develop | L |
| 8 | Remaining features | Compare + Original ⌄ (hold, split, compare, camera JPEG); Your negative; colour filters; Shifts move; faster open (ticket 39); film-side release (41); last of all, Open in Lightroom | L |
| 9 | Validation and release | Full Windows suite once, library random test with 25 seeds, Mac↔Windows sidecar round trip, startup medians, a package build, side-by-side screenshots, release notes | M |

Phases 1–5 can ship to users on their own, as a restyled Windows app without the
library. Phase 6 is the riskiest for data, because sidecars travel between machines.
That's why it gets the shared crate and the random test.

## Decided with Noah (2026-11-18)

1. **Painting:** Direct2D/DirectWrite for the controls, panels, title bar and text, on
   the existing windows. The strip, grid, flight and tray tiles are drawn on the
   existing wgpu device, sharing thumbnails as textures with the canvas, with words
   rendered by DirectWrite into textures. Phase 7 settles this by measuring it against a
   GPU-backed Direct2D surface; pick whichever is simpler at equal speed.
   Background: Rex for Windows (Mitchell Hashimoto's tweet of 2026-11-17) uses native
   widgets where possible plus one custom DirectX pass. We take the same split. We don't
   use a GPU pass for the whole UI, because the window and inspector would then wait
   for Vulkan, PCs without Vulkan would need a second UI renderer, and screen-reader
   support would need doing by hand.
   Direct2D's costs: the axes and loading fonts from memory need Windows 10 1803
   (fall back to the font's fixed instances before that). Factories cost a few ms.
   A hardware render target creates a D3D device (10–30 ms, so create it lazily) and
   must handle a lost device.
2. **Menus** go in the title bar.
3. **Plain** follows Windows' light/dark setting and uses Segoe UI. **If that takes much
   extra code, drop it** and say so.
4. **Work on local `main`**; a briefly broken main is acceptable. Don't push. Work in
   parallel only where it pays for its cost.
5. **Open in Lightroom** comes to Windows, but last, as the final item of phase 8.

Execution handoff: [windows-parity-coordinator.md](windows-parity-coordinator.md).
Progress log: [windows-parity-progress.md](windows-parity-progress.md).
