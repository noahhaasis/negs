# Windows parity: progress log

Newest first. One entry per phase and per merge: what landed (sha), the gate results,
decisions made with their reasons, and what was carried over. See
[the plan](windows-parity-plan.md) and [the coordinator handoff](windows-parity-coordinator.md).

## Status

| # | Phase | State |
|---|---|---|
| 0 | Baseline | partly done (see below) |
| 1 | Drawing foundation | not started |
| 2 | Window shell | not started |
| 3 | Inspector | not started |
| 4 | Canvas | not started |
| 5 | Tray | not started |
| 6 | Library (crate can start in parallel after phase 0) | not started |
| 7 | Strip and grid | not started |
| 8 | Remaining features (Open in Lightroom last) | not started |
| 9 | Validation and release | not started |

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
