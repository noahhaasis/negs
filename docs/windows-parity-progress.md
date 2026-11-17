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
| 3 | Inspector | 3a done (merged, see log); 3b presets tiles + Settings tabs in progress (`win-p3b-presets-settings`, owns the PC) |
| 4 | Canvas | not started |
| 5 | Tray | not started |
| 6 | Library (crate can start in parallel after phase 0) | crate done, merged (see log); Windows glue after phase 5 |
| 7 | Strip and grid | not started |
| 8 | Remaining features (Open in Lightroom last) | not started |
| 9 | Validation and release | not started |

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
