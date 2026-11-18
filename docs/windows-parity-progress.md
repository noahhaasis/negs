# Windows parity: progress log

Newest first. One entry per phase and per merge: what landed (sha), the gate results,
decisions made with their reasons, and what was carried over. See
[the plan](windows-parity-plan.md) and [the coordinator handoff](windows-parity-coordinator.md).

## Status

| # | Phase | State |
|---|---|---|
| 0 | Baseline | done |
| 1 | Drawing foundation | done, merged `e142fba` |
| 2 | Window shell | done, merged `a9a0763` |
| 3 | Inspector | done: 3a and 3b merged (see log) |
| 4 | Canvas | done, merged `8f5bdde` |
| 5 | Tray | not started |
| 6 | Library | crate merged; glue built on the Mac (`win-p6b-library-glue`), now on the PC for its gates |
| 7 | Strip and grid | not started |
| 8 | Remaining features (Open in Lightroom last) | not started |
| 9 | Validation and release | not started |

## 2026-11-19: phase 4 merged, the canvas (`8f5bdde`, branch tip `5726c97`)

- **What users see:**
  - The caption above the print (EXIF facts left, the file name right; the name goes first
    when the line is short; a portrait's caption is centred).
  - The label under it: "Film ⌄ printed on Paper ⌄". Each name opens the fold-out
    tiles, or a menu when previews are off or in Image Only.
  - The paper lock on "printed on". **Keep paper and the custom background settings are
    gone**, as on the Mac.
  - The Mac's 48/54/68 layout; the empty window's words are centred on the print.
  - Cropping: trim marks start it, a straighten dial with quarter-turn arrows, and the crop
    panel in the ink style (Cancel, yellow Done, "return applies · esc cancels").
  - F is Image Only, holding T shows the photograph upside down, and Esc leaves Image Only
    first.
  - Settings › Look › Viewer › Sheet: the theme's own plus the 7 sheets. The caption and
    label take the sheet's ink; the light table glow is contrast-gated against the sheet.
  - The exact fit (ticket 38): a fitted photograph is Lanczos-resampled in linear light,
    on the GPU or on a thread for the software canvas, after a 150 ms pause in sizing.
  - The window reopens at its last size, within 1 px at 125% (it used to drift a pixel
    per launch).
  - Phase 2 carry-overs: the status line and strip hide while a file is dragged in; the
    Developing tag's moving line works on the software canvas.
- **Gates:**
  - Mac cross-check clean. PC build passed; `test-parity.cmd` 203 passed, 0 failed.
  - Exact fit against Pillow LANCZOS in linear light: CPU mean |Δ| 0.0000/255, max 1;
    GPU (6016×4016 develop, three sizes) mean ≤ 0.054/255, max 1, detail ratio 1.0000–1.0001.
  - Startup ABBA, 8 each, medians in ms (base b360278 → head):
    `first_window_composited` 85.7 → 86.6, `inspector_shown` 181.8 → 179.5,
    `gpu_ready` 410.2 → 420.1, `first_image_on_screen` 1170.4 → 1203.7.
    Three head runs had `gpu_ready` ≈ 540 and pulled the last median up; the others
    were ≈ 1170, like base. Defender was scanning the freshly copied exes. To be
    settled by the full ABBA after phase 6.
  - Screenshots, Darkroom and Daylight: `~/.negs/windows-parity/phase4/`. Photo and crop
    views match the Mac's layout and type closely.
- **Decided (agent, accepted):** Rotate starts a crop; the hand cursor over the label's
  targets; the Developing tag on the theme background; `text.rs` replaced by DirectWrite;
  the exact fit only when fitted, settled, unrotated, not cropping or sizing, and
  zoomed out; the crop shade blends encoded colour as Core Animation does.
- **Mismatches left:**
  - No Flip in the crop panel: the Windows engine has no `io.flip` parameter, and that
    needs an engine change (Noah's call).
  - While cropping, the status line still shows the size and develop time; the Mac shows
    "Cropping · Original · 6016 × 4016 px" and dims Fit/100%. Carry to phase 7.
  - The window size can be 1 px off at fractional scales.
- **For Noah:** a tooltip on the paper lock? An `io.flip` engine parameter for Flip?

## 2026-11-19: speed-ups agreed with Noah

Phases were slow: most of the time went to chasing startup noise, retaking
screenshots, polishing commits, and the single PC. Agreed:

1. **Phase 6 glue runs alongside phase 4.** It is built on the Mac with the cross-check
   and gets the PC only at the end, for its build and the Mac↔Windows round trip.
   Phase 4 owns the PC until then. Merge conflicts in `model.rs`/`ui/mod.rs` get
   resolved at merge.
2. **Startup gates only where launch code changes.** One full ABBA after phase 6.
3. **Leaner briefs:** screenshots in Darkroom plus one light theme, taken once;
   commits as they come, no history polishing; the Windows test suite only in phases
   4 and 8.
4. **Phases 4 and 5 stay separate agents,** so neither runs out of context.

## 2026-11-19: phase 3b, presets tiles and Settings (branch `win-p3b-presets-settings`)

- **What users see:**
  - Presets are tiles in two columns, as in `PresetInspector.swift`: the photograph
    developed through each preset, its name in Newsreader 15, its film, and "printed
    on" its paper (or "not printed · film scan"). The preset applied last to the
    photograph has the pencil ring (`applied_presets` in the model, by photograph, as
    the Mac's `appliedPresets`). A click applies, a drag sorts live, dropped files
    import before the tile under the pointer, and the foot says "click to apply · drag
    to sort · drop files to import" (or "drop to import" with the loose drop ring).
  - Empty: "No presets yet", the Mac's sentence with "New" upright, Import Presets….
  - Each tile's menu: Apply, Rename (F2), Export…, Show in Folder, Delete… (Del, the
    alert card). Arrows move between tiles; Enter or Space applies; Shift+F10 opens the
    menu. Each tile is a push button named by the preset, with "Applied last. film,
    printed on paper" as its MSAA value. Renaming is a field over the name with the
    pencil under it.
  - Only the tiles on screen and a row either side are developed
    (`Model::set_visible_presets`); the pane is still built on first show.
  - Settings is an owned, themed window (its own 38 DIP band, Close only), made on the
    first Ctrl+, and hidden by Esc, Ctrl+W or Close. Tabs as the inspector's:
    Photographs · Speed · Upkeep · Look, with the Mac's wording. Changes apply at once.
    It reopens on the last tab.
  - Look: the six themes as miniatures with the BOX ring on the chosen one and
    `AppTheme`'s hint under them.
  - Photographs › Films › Fotufilm films: All / From datasheets / None, stored as the
    Mac's `fotufilmFilms`. It filters the film menu, the grid and the film previews in
    the Windows app (no `crates/` change); the film in use stays listed.
- **Where the Windows settings went:**
  - Photographs › New photographs: Edits (Keep the previous edits / Start fresh) and
    Match to camera brightness. The Mac has Film/Paper and sidecar Edits here
    instead; Edits stays until sidecars (phase 6).
  - Photographs › Films: Fotufilm films, and Keep paper when changing film. The Mac
    replaced Keep paper with the paper lock (phase 4).
  - Speed: previews, Limit preview to 12 MP, Use less memory for export.
  - Upkeep: updates (automatic, Check Now with the last check), or "not configured".
  - Look › Viewer: the custom viewer background and its colour. **This is the slot
    for phase 4's viewer sheet picker**, which replaces it.
  - Left out until their features exist: RAW + JPEG, sidecar Edits, the thumbnail
    cache, Lightroom, Remove All App Data and Quit….
- **Startup**, one ABBA (main `aea6dff` vs branch, 6 runs, Defender idle: MsMpEng
  0 cpu-s in the 5 s before and after). `inspector_shown` medians 173.4 / 189.1 ms
  (+15.7). First window 77.5 / 78.5 ms; first image 1158 / 1150 ms. B was bimodal
  (170–175 and 203–206). An earlier 8-run ABBA in the same session gave 182.3 /
  180.1 ms (−2.2), so this is noise. Settings and the Presets pane are not built at
  launch.
- **Slider drag (3a's WARP path vs the pre-3a GDI path),** dragging Exposure with a
  temporary trace (not committed): no dispatch over 16.7 ms on either. The inspector's
  paints per mouse move are about 1.6 ms new against 0.8 ms old: trackbar 0.32 ms
  both, the value field 0.30 / 0.18, the rows 0.17 / 0.15, plus two 0.31 ms buttons
  (value box and reset). The film strip repaints on every move on both paths (2.6 ms):
  that's the largest cost, and it isn't 3a's.
- **Tests:** 190/190 on the PC with a scratch `NEGS_PROFILE_DIR`. The Mac cross-check
  is clean (the same 14 warnings as main).
- **Tooling:** `mac_shot.sh` has `shotfront` and `clickfront` (`winshot front`). In a
  `pc_shot` job, `"front": true` targets the app's frontmost window, and a `"drag"`
  step presses, moves and lets go. `shot.ps1` posts WM_CLOSE to the main window:
  `CloseMainWindow` hit a tooltip's SysShadow window while a tip was up.
- **Merged** after the coordinator's `12f5da4`. **Incident:** 3a's `test-parity.cmd` ran
  the suite without `NEGS_PROFILE_DIR`, and a test overwrote Noah's real
  `%APPDATA%\Negs\settings.json` (19 Nov 09:04; recent files became a temp file,
  everything else defaults). The earlier contents can't be recovered. Fixed:
  `test-parity.cmd` now sets a scratch profile, and test builds default to a temp
  profile in `session.rs`. Noah was told.
- **Decided (coordinator):** Keep paper goes when the paper lock lands in phase 4, as on
  the Mac. The strip repainting on every slider move goes to phase 7 (the strip
  rewrite), not before.
- **Mismatches left vs macOS:**
  - Keep paper and the Edits choice are in Settings until the paper lock and sidecars
    (Keep paper went in phase 4; the Edits choice goes with phase 6's glue).
  - The custom background is where the viewer sheet goes (replaced in phase 4).
  - The rename field types in Segoe UI 15 (GDI EDIT), with the Newsreader pencil
    under it.
  - Tiles and tab words show the focus box after a click once keyboard cues are on,
    as Windows does.
  - The theme swatches tick the chosen theme even when `NEGS_THEME` forces another.
    This only happens in the harness.
  - Scrolling with many presets and the "can't be read" tile were not shot on the PC.
- Screenshots: `~/.negs/windows-parity/phase3b/`, with the Mac Settings references in
  `phase3b/mac/`.

## 2026-11-19: phase 3a merged (branch tip `786d7f8`)

- **What users see:** the Mac's inspector.
  - Pane header: Adjustments · Presets as words, with the edit count and Reset.
  - Histogram · Waveform as words. The light-theme histogram is now added up and laid
    over the well, as the Mac's is.
  - Prep · Lab · Output in Newsreader 21 with per-tab counts. A count opens a menu with
    Reset for each edit and Reset Tab.
  - Every row style. Changed values are 700 paper with the pencil under them.
  - "N more" lines, with View › Show All Controls in place of Advanced.
  - The focus box. The Export foot, on Output only.
  - MSAA names, values and roles were checked.
- **Painting:** every Canvas paint now goes to a single-threaded WARP device, created on
  its own thread at launch. The old Direct2D DC path is the fallback. Screenshots of
  the inspector are pixel-identical between the two paths.
- **Startup:** the cause is confirmed. Direct2D's worker threads are starved while the
  engine starts: delaying the background threads by 400 ms closed the gap to +3 ms.
  ABBA medians for `inspector_shown`:
  - quiet PC: +4.9 ms (199.3 / 204.2);
  - Defender busy after a reboot: +42.6 ms (186.7 / 229.3).

  First window +2.6 to +7.5 ms. **Noah accepted up to +40 ms for now; recheck after
  phase 6.** Next options:
  - text through `IDWriteBitmapRenderTarget` with no D3D at all;
  - engine thread priorities during startup (needs Noah's OK).
- **Tests:** 185/186 on the PC. The one failure was a "graphics driver reset" in a GPU
  export lifecycle test this branch doesn't touch; the 40 lifecycle tests re-run all
  pass. An earlier run had 186/186. The Mac cross-check is clean.
- **"Struck tab":** macOS never strikes a tab. It strikes Print and its modules while
  scanning (Windows matches), and Camera and Grain/Halation/DIR for a negative scan.
  That part comes with "Your negative" in phase 8.
- **Mismatches left vs macOS:**
  - the tone graph's segmented buttons;
  - the border samples, still GDI;
  - the rounded Longest side field;
  - value-box text sits high while typing;
  - no Input Colour summary;
  - the scroll bar is always shown;
  - wheel up lowers a slider's value (as before).
- Screenshots: `~/.negs/windows-parity/phase3a/`.

## 2026-11-18: phase 2 merged (`a9a0763`, branch tip `171cd81`)

- **What users see:** six themes live (View › Theme, saved as `appTheme`, Darkroom
  default; Plain follows Windows light/dark; `NEGS_THEME` forces a name, light/dark =
  Plain/Plain dark). 38 DIP title bar (Direct2D): menu row File Edit Photo View Help,
  file name Newsreader 15/600, themed caption buttons, snap layouts kept. No toolbar or
  canvas pill (`chrome.rs` deleted); items in menus (shortcut map in
  `docs/windows-ui-features.md` / release notes). Status line in words (size, develop
  time, export progress + Cancel, Original ⌄ hold/menu, Fit, 100%). Alert card replaces
  task dialogs (About stays native). Empty window, dropping, Developing tag, can't
  develop on the canvas (`Model::develop_error`).
- **Gates:** cross-check clean (14 old warnings); PC build; 182 Windows tests pass.
  Startup ABBA 6 runs: A 86.4 / 181.1, B 81.9 / 176.4 (first window / inspector). Fixed
  on the way: `d2d::prepare` now starts at the top of `run()`, `d2d::warm` sets the first
  frame's type styles on that thread.
- **Decided (coordinator):** no duplicate View › Reset All Adjustments (Windows has one
  reset, Photo › Reset Edits Ctrl+Shift+R); the big shell commit `0836731` stays whole.
  Rotate/Crop/Fit/Actual in View as on the Mac; Crop shows Ctrl+K, C works too.
- **Not exercised on the PC:** runtime theme switch from the menu, the drop state, Try
  Again, holding Original, keyboard on the confirmations. 150 % not testable per run
  (PC is at 125 %, system-wide).
- **Carried over:** phase 3: inspector controls (still old painting in theme colours),
  Export foot, a stray glyph left of "No developed photograph" in the empty histogram.
  Phase 4: film → paper label (dropped from the status line), empty-window words centred
  on the print area, whether the status line hides while dragging (match macOS), the
  Developing tag's line is still on the software canvas. Phase 6: library items in Photo
  and the status line's library notes; folder wording in the empty window.
- Screenshots: `~/.negs/windows-parity/phase2/`.

## 2026-11-18: phase 6 crate merged (branch tip `7718029`)

- **What landed:** `crates/negs-library` (no dependencies; workspace member; `Cargo.lock`
  gains only its own entry). Exact JSON (JSONEncoder's `.prettyPrinted` `" : "`, sorted
  keys, Swift's `Double.description` incl. ties-to-even; JSONDecoder's quirks), sidecar
  read/patch/atomic write, edits as the sidecar's own JSON, folder listing in Finder order
  with RAW+JPEG/HEIF pairs, filter + wording, selection, delete plans + `Bin` trait,
  thumbnail looks/keys (CryptoKit goldens), xmp:Rating embedding (PNG/JPEG/TIFF), and a
  `Library` with actions returning an `Outcome` (the Mac's note text) and byte-exact undo.
  API and the Swift each part mirrors: `crates/negs-library/README.md`.
- **Gate (crate part):** `cargo test --release -p negs-library`: 48 pass; every fixture
  reads and rewrites to the Mac encoder's bytes (`tests/mac-encoded/`);
  `NEGS_LIBRARY_SEEDS=25` random-action test passes (1.3 s; 200 × 200 also passes).
  Re-run by the coordinator before merging.
- **Decided (coordinator):**
  - `docs/sidecar-fixtures/` stay as they are (hand-written `": "`); the crate checks
    that spacing is the only difference. Not worth touching shared fixtures.
  - Undoing a rating/flag after another app's write restores only the fields the action
    changed (keeps the other app's change). Safer than restoring the whole summary.
  - Case-insensitive sidecar names on Linux: decide in the Linux port, not now.
- **Left for the Windows glue (phase 6 proper):** Recycle Bin `Bin` (IFileOperation),
  folder watcher + refresh on activate, 1 s save debounce, one undo stack interleaving
  editor steps with `library.undo()`, brightness-match inputs, `EditDefaults` from the
  catalog, settings persistence (filter, copied groups), view restore after undo,
  batch-export naming, `xmp::embed_in_file` after exports, and making `change_edits`
  public for whole-roll actions. Then the Mac↔Windows sidecar round trip.

## 2026-11-18: phase 1 merged (`e142fba`, branch tip `fe662f4`)

- **What landed:** `windows/marks/` (crate `negs-marks`, no Win32: pencil, rings,
  `HandStar`, `RejectCross`, `PickBox`, Plain variants; 16 tests against the Swift's
  numbers, `cargo test --manifest-path windows/marks/Cargo.toml --target-dir target/marks`).
  `ui/d2d.rs`: software `ID2D1DCRenderTarget` into the GDI back buffer via
  `Canvas::d2d(scale, plain, |p| …)`, cached text formats/layouts/geometry, lost-target
  redraw; Newsreader from memory (`IDWriteFactory6` axes on 1803+, named instances on
  1703–1709, Segoe UI before). `ui/theme.rs`: `ThemeName` + token tables for all six
  themes, checked against `AppTheme.swift` by script. `ui/sampler.rs`:
  `NEGS_DRAW_SAMPLER=<theme>` draws every primitive. The visible app is unchanged.
- **Startup** (ABBA, 6 runs, A = main, B = branch): first window 90.4 / 88.4, inspector
  184.4 / 181.0, GPU ready 426 / 429, photograph 1191 / 1182. Within noise.
  D2D + DWrite + fonts cost ~2.5 ms, made on a `negs-drawing` thread right after
  `inspector_shown` (`drawing_ready` stamp).
- **Plain on Windows:** follows Windows' light/dark (or `NEGS_THEME`), Segoe UI Variable
  where present, else Segoe UI. Cheap (~20 lines), kept.
- **Decided (coordinator):** the Barlow Condensed OFL notice now ships with the Windows
  package too (it was already compiled in through `spektrafilm-io` but uncredited); a
  licence requirement, not a design choice.
- **Carried over:** the marks' halo is two translucent strokes (a real blur needs a
  device-context target: phase 7). Highlighter uses normal blending, not multiply. The
  1703–1709 font path is untested. CI doesn't run `negs-marks` tests (needs a
  workflow edit; do it in phase 9).
- Screenshots: `~/.negs/windows-parity/phase1/`.

## 2026-11-18: phase 0, baseline

- **Tooling** (`windows/tools/`, see its README): `pc_console.sh` runs PowerShell in
  Noah's console session through a scheduled task; `pc_shot.sh` + `pc/shot.ps1` launch,
  drive (SendKeys, clicks) and capture the Windows app's window; `pc/startup.ps1` gives
  ABBA-interleaved startup medians with windows closed automatically; `mac_shot.sh`
  captures an isolated copy of the macOS dev app (System Events clicks; CGEvents posted
  from here don't reach SwiftUI). `C:\Users\Noah\.negs\build-parity.cmd <branch>`
  fetches from the Mac and builds in the new `C:\Users\Noah\Projects\negs-parity`
  worktree (cold build 1 min 55 s).
- **Startup baseline** (`startup.ps1`, 6 runs + warm-up, `docs\card.jpg`, medians in
  ms; two builds of `83e8c79`, interleaved): first window composited 88.3 / 86.3,
  inspector shown 182.2 / 183.5, GPU ready 438.6 / 432.4, photograph on screen
  1211.4 / 1205.5. These are lower than `docs/windows-app.md`'s 106/257 because this
  harness and machine state differ; **compare only against runs of this harness**, A/B
  interleaved with a `main` build. Gate: first window and inspector within ~10 ms.
- **Reference photographs:** eight D750 NEFs (`_NHA9786` … `_NHA9810`), at
  `C:\Users\Noah\.negs\photos\ref` and `~/.negs/windows-parity/photos`.
- **Reference screenshots** in `~/.negs/windows-parity/ref/`: `windows-0.17.0/`
  (dark and light: empty, Prep, Lab, Output, film fold-out, crop, presets, File menu,
  Settings) and `macos-main/{darkroom,daylight,plain}/` (Prep, Lab, Output, film tray,
  crop, presets, grid, Image Only). macOS Settings is a separate window and not
  captured yet.
- **Mac Screen Recording** had to be granted to Rex (the host app) for `screencapture`;
  it took effect without a restart.

## 2026-11-18: survey and plan

- **Windows `main` worktree:** `negs-presets` was fast-forwarded to `83e8c79` from
  GitHub. It built cleanly there: helper, both exporters and `Negs.exe`, with LTO off
  and 16 codegen units, in about 2 min for the Windows app.
- **Mac cross-check:** the `x86_64-pc-windows-msvc` cross-check of `windows/` works
  (41 s cold).
- **PC to Mac:** `git fetch mac main` works from the PC.
- **Decisions:** Noah's five answers are in the plan's "Decided" section. The
  coordinator chose wgpu for the strip, grid and tray tiles (to be confirmed by
  measurement in phase 7).
- **Phase 0 still to do:** re-measure startup, write the scheduled-task screenshot
  script, and take reference screenshots of both apps on the same photos (empty window,
  photo with each tab, crop, presets, settings, light and dark).
