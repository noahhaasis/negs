# Windows parity: progress log

Newest first. One entry per phase and per merge: what landed (sha), the gate results,
decisions made with their reasons, and what was carried over. See
[the plan](windows-parity-plan.md) and [the coordinator handoff](windows-parity-coordinator.md).

## Status

| # | Phase | State |
|---|---|---|
| 0 | Baseline | done |
| 1 | Drawing foundation | done, merged `e142fba` |
| 2 | Window shell | in progress: agent on branch `win-p2-shell` (owns the PC) |
| 3 | Inspector | not started |
| 4 | Canvas | not started |
| 5 | Tray | not started |
| 6 | Library (crate can start in parallel after phase 0) | crate in progress: agent on branch `win-p6-library-crate` (no PC); glue after phase 5 |
| 7 | Strip and grid | not started |
| 8 | Remaining features (Open in Lightroom last) | not started |
| 9 | Validation and release | not started |

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
