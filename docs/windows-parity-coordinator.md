# Handoff: coordinating Windows parity with the macOS app

For the agent that runs the Windows parity project from start to finish, mostly without
supervision. Written 2026-11-18 against `main` `83e8c79` (0.17.0).

**Read first:** [windows-parity-plan.md](windows-parity-plan.md) (inventory, approach,
phases, Noah's decisions). Then [windows-parity-progress.md](windows-parity-progress.md).
The progress log is the source of truth for what's done. Update it after every phase
and every merge, so you can resume after compaction or a new session from the log alone.

## The mission

Bring the Win32 app (`windows/`) to the redesigned macOS app's look and features.
Diverge only where macOS leans on macOS. In those places, lean on Windows instead:
title-bar menus, Ctrl shortcuts, the Recycle Bin, Energy saver, and native file dialogs.
Keep the app fast. Launch time is a hard requirement. Test strategically, not exhaustively.

## How you work

You are the coordinator. Run phases 0→9 in order, either yourself or by spawning one
agent per phase (or per independent slice of a phase). Your job between phases:

1. Write the phase agent's brief (template below).
2. When it reports, read its diff (`git diff --no-ext-diff`, because `git diff` is
   difftastic here) and look at its screenshots next to the macOS ones.
3. Check its startup numbers.
4. Merge it to `main`, then update the progress log.

Small phases or fix-ups are cheaper to do yourself than to brief.

**Parallelism, only where it pays:**

- **Phase 6's library crate** (`crates/negs-library`) is pure Rust and needs neither
  the UI nor the PC. Run it in parallel with phases 1–5, starting right after phase 0.
  Its glue into the Windows app waits until phase 5 is merged.
- **Phases 1–5 touch the same files** (`ui/draw.rs`, `ui/theme.rs`, `ui/inspector.rs`,
  `ui/mod.rs`), so they run one after another.
- **Phase 8 slices** can each go to an agent once phase 7 is in, but `inspector.rs` and
  `model.rs` are shared. Run at most two at a time, and merge between them.
- **One PC.** Only one agent builds or runs GUI on the Windows PC at a time (16 GB
  RAM). Hand out the PC explicitly in briefs: "you own the PC until you report".

**Subagent worktrees:** `isolation: "worktree"` may branch from `origin/main`, which can
be behind local `main`. Every brief starts by telling the agent to run
`git reset --hard <current local main sha>`. Worktree sessions refuse Bash with
`$VAR`, loops or `bash script-that-runs-git`: put logic in scratch script files and call
them with literal paths.

**Ask Noah only for real design choices** (how a macOS-only idea should look on
Windows), and batch them. Otherwise decide, log the decision with its reason in the
progress log, and move on. His standing answers are in the plan's "Decided" section.
He prefers recommendations to option lists.

## Rules

- **Commit on local `main`, never push.** Merge phase branches with `--no-ff`
  ("Merge win-pN-…: what it does"), or commit small things directly.
- **Commit dates:** each commit landing on main is dated main's tip + 1 min (author
  and committer, `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE`). Nothing on main may be
  dated 08:00–16:00 Europe/Berlin. Branch commits made in those hours move to tip + 1
  min onward; keep real dates outside them. Commit messages follow the repo's style:
  "Windows: what the user now sees".
- **Other sessions may be working on macOS on the same `main`.** Touch only `windows/`,
  `crates/negs-library/`, workspace `Cargo.toml`/`Cargo.lock` where needed, and
  `docs/windows-parity-*`. Rebase or merge carefully; don't reformat shared files.
- **Shared crates and exports:** don't change engine behaviour. Exports must stay
  byte-identical. If a phase truly needs a helper change, it goes in its own commit,
  and the macOS tests that cover it are run.
- **Don't touch:**
  - Noah's installed Negs (`%LOCALAPPDATA%\Programs\Negs`) or his Windows profile.
  - The main Windows checkout (`C:\Users\Noah\Projects\negs`, on `crop-reuse-key`,
    with uncommitted notes).
  - His photo folders. Library work writes `.negs` sidecars, so test it only on
    copies in a scratch folder.

## The machines

**The Mac** (here): `~/Projects/negs`. Cross-check the Windows app:

```
cargo check --release --locked --target x86_64-pc-windows-msvc \
  --manifest-path windows/Cargo.toml --target-dir target/wincheck
```

That takes 41 s cold and seconds after. Do this on every edit; it catches almost
everything before the PC is involved. `crates/negs-library` tests run here natively.
Use a separate `--target-dir` per worktree; don't share the main checkout's.

**The PC:** `ssh pc` (Windows 10 22H2 build 19045, Ryzen 5 3600, 16 GB, one GPU,
Vulkan). It runs cmd.exe: use `cd /d`, `&&`, and no `tail` or `grep` (pipe through
`findstr`, or filter on the Mac). Cargo is not on the ssh PATH; use a `.cmd` that sets
`PATH=C:\Users\Noah\.cargo\bin;%PATH%`. `C:\Users\Noah\.negs\build-main.cmd` is the
template, with LTO off, 16 codegen units and incremental on. Copy scripts in with `scp`.

- **Engine binaries:** `C:\Users\Noah\Projects\negs-presets` is the worktree with
  `main` checked out. It is at `83e8c79`, and its `target\release` has
  `spektrafilm-native`, `spektrafilm` and `spektrafilm-f64`. Set
  `SPEKTRAFILM_ROOT=C:\Users\Noah\Projects\negs-presets` for app runs. Rebuild the
  engine there only when `crates/` changes; update it with
  `git fetch mac main && git merge --ff-only mac/main`.
- **Getting code to the PC:** the Windows repo has a remote `mac` (this Mac, over
  ssh, through Git for Windows' ssh), and it works. Commit (WIP commits are fine) on
  a branch here, then on the PC run `git fetch mac <branch>` and
  `git checkout --detach FETCH_HEAD`. Do this in a parity worktree you create once:
  `git worktree add --detach C:\Users\Noah\Projects\negs-parity` from `negs-presets`.
  Its `windows\target` builds incrementally (about 2 min cold).
- **App runs:** set `NEGS_PROFILE_DIR` to a scratch folder and
  `NEGS_ALLOW_MULTIPLE=1`; `NEGS_THEME=light|dark` forces the system theme.
- **Test photos on the PC:** `C:\Users\Noah\Downloads\102ND750` (34 D750 NEFs) and
  `C:\Users\Noah\Downloads\raws` (33 mixed). Copy them to a scratch folder before
  anything can write sidecars. `docs\card.jpg` is the startup photo.
- **Screenshots:** ssh is session 0 and has no desktop. Noah's console session (1) is
  logged on, so run a GUI via a scheduled task:
  `schtasks /create /it /ru Noah /sc once /st 00:00 /tn negs-shot /tr <cmd> /f`, then
  `schtasks /run /tn negs-shot`. Capture the window rectangle only, with a PowerShell
  `CopyFromScreen` on the Negs window's bounds after bringing it to the front.
  `PrintWindow` misses the Vulkan canvas. Windows will appear on Noah's monitor, so
  keep runs short and close the app afterwards. Phase 0 builds this as a reusable
  script, `C:\Users\Noah\.negs\shot.ps1` (launch, wait, capture to PNG, close), and
  the PNGs come back with `scp`.
- **Startup:** `windows/measure_startup.ps1` reads the `NEGS_STARTUP_REPORT` stamps.
  The 0.17.0 medians on this PC are `first_window_composited` ≈ 106 ms and
  `inspector_shown` ≈ 257 ms (see `docs/windows-app.md`); phase 0 re-measures them.
  Close other heavy work on the PC while measuring.
- **AC sleep is off** on the PC. If ssh times out, it is probably booted into Linux
  (same IP, same key, different host key). Then stop and ask Noah to reboot it into
  Windows.

**macOS reference screenshots:** see the memory note "macOS app test tricks". Run an
isolated copy of the app with its own bundle id, `HOME` and `CFFIXED_USER_HOME`
pointing at scratch folders, then `screencapture -l <windowID>`. Build it with
`scripts/package_macos_app.sh --dev`. Noah may be running `dist/negs-macos`, so target
processes by PID, never by name.

## References

| What | Where |
|---|---|
| The look, every token, component and behaviour | `design/proposals/IMPLEMENTATION.md`; why each was chosen: `FEEDBACK.md` |
| Themes (5 palettes, the Look tab) | `design/proposals/IMPLEMENTATION-THEMES.md`, `macos/Sources/NegsApp/AppTheme.swift`, `Theme.swift` |
| Mockups (open in a browser) | `design/proposals/final.html` and the `rN-*.html` rounds (r27 sheets, r28 themes, r29 compare) |
| Font | `macos/Fonts/Newsreader-Negs.ttf`, `-Italic.ttf`, `OFL-Newsreader.txt` (credit it in the Windows notices/installer) |
| Exact shapes to port | `HandStar.swift`, `RejectCross.swift`, `PickBox.swift`, the pencil stroke in IMPLEMENTATION §2.1 |
| Library behaviour | `docs/library.md`, `docs/sidecar-format.md`, `docs/sidecar-fixtures/`, `tickets/30-library*.md`, `Sidecar.swift`, `Library.swift`, `Selection.swift`, `TabEdits.swift` |
| Thumbnails | `tickets/35-*`, `36-*`, `EditedThumbnails.swift`, `Previews.swift`, `LibraryGrid.swift` |
| Features | tickets 33 (Your negative: `Negative.swift`), 38 (Windows fit view: the ticket is written for Windows), 39, 41, 46 (`Compare*.swift`), 47, 48 (`Lightroom.swift`); issue #21 (`ColorFilterPicker.swift`); paper lock and auto (`Tray.swift`, `EditorModel.swift`) |
| How the macOS frame pipeline behaves | `macos/Specs/README.md`, `FramePipeline.tla`; Windows already mirrors the held and quick frames (`docs/windows-app.md`) |
| Windows app internals | `docs/windows-app.md`, `docs/windows-ui-features.md`, `docs/handoff-windows-ui-polish.md` |
| Release notes | `release/notes/unreleased.md` (add a Windows section per phase that changes what users see) |

The Swift is the behavioural reference. Port behaviour and wording; don't copy
SwiftUI structure. Where the Swift and IMPLEMENTATION.md disagree, the Swift on `main`
wins, because later rounds changed things.

## Gates

| After | Gate |
|---|---|
| Every edit | The Mac cross-check is clean (no new warnings in touched code) |
| Every phase | A PC build, the app runs, one screenshot set compared with macOS on the same photos, `measure_startup.ps1` (one run of 6) within noise of the baseline, a progress-log entry |
| Phases 4 and 8 (the frame pipeline) | The Windows lifecycle suite: `cargo test --locked --manifest-path windows/Cargo.toml`. It's slow, so run it once per phase, not per edit |
| Phase 6 | The `negs-library` tests on the Mac, every fixture in `docs/sidecar-fixtures/` read and rewritten byte-identically. A random-action test (ratings, flags, edits, paste, duplicate, reset, delete, undo, outside writes, reopen) with 25 seeds. A Mac-written folder opens correctly on Windows and a Windows-written one on the Mac |
| Phase 9 | All of the above once more, `windows/package.ps1` builds the installer, `verify_package.py` passes, and side-by-side screenshots of every view for Noah |

Don't run the macOS Swift suite unless you changed shared code it covers. Don't run
the golden harness unless the engine changed.

## Brief template for a phase agent

> You're implementing phase N (name) of Windows parity for Negs. First run
> `git reset --hard <sha>` (local main), then read `docs/windows-parity-plan.md`,
> `docs/windows-parity-coordinator.md` (rules, machines, gates) and the progress log.
> Scope: … (the phase's row, plus anything the log carried over). Reference: … (the
> Swift files and IMPLEMENTATION sections). Done means: … (the gate). You own the PC
> until you report / you don't use the PC. Commit on branch `win-pN-<name>` in small
> steps, with Windows-style messages. Don't merge or push. Report: what changed
> (user-visible), the screenshot paths, the startup medians against the baseline,
> anything skipped or left undecided, and questions for Noah.

## Done means

Every row of the plan's feature table works on Windows. The Windows app reads like the
macOS one in each theme, except for the Windows-leaning points in the plan. Startup is
no slower than 0.17.0, and sidecars round-trip between the two. Then tell Noah, with
the screenshots, what's left (if anything) and that nothing is pushed.
