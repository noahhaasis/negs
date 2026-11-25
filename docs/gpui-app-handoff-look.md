# Look-Parity Handoff: make the GPUI app look like Negs

Prepared 2026-11-25 at local `main` `311e47d1` (not pushed). Read it with:
- [gpui-app-plan.md](gpui-app-plan.md)
- [gpui-app-coordinator.md](gpui-app-coordinator.md)
- [gpui-app-progress.md](gpui-app-progress.md): the Phase 6 section has the latest numbers and decisions.

This phase comes **between Phase 6's parity work and the Windows cutover**. The cutover (`docs/gpui-windows-packaging-plan.md`) stays unapplied until Noah signs off the look.

---

## 1. Why this phase exists

The GPUI app in `app/` works: it has feature parity, it is faster than the Win32 app, and its exports are byte-identical. But it looks very different from the Negs people use today. On Phase 6's Go/No-Go, Noah said the UI "still looks veeeeeeeeery different to before" and deferred the cutover.

The plan never had an explicit "match the existing look" step. The Phase 3 and 6 gates asked for side-by-side screenshots, but those screenshots were only used to check that features and colours worked. Each phase built its panels from the shared model and its own idea of the layout. Phase 6 noticed the gap and didn't flag it. Don't repeat that: **the look is the gate here, and a screenshot that differs from the reference is a finding, not a matter of taste.**

**Noah's rules for this phase (2026-11-25):**
- **Settle case by case.** For every visible difference, Noah picks which look wins: the Win32 app (`windows/`), the SwiftUI app (`macos/`), the current GPUI one, or a fix. Don't decide a whole area for him.
- **Fix the originals' visual bugs instead of copying them.** Where an original has odd spacing, something off-centre, misaligned baselines, clipped text and the like, the GPUI app does it right. List each one as a case with the fix as an option, so Noah can see it.

Fix those bugs in `app/` only. `windows/` and `macos/` will be retired, so leave them alone unless Noah asks.

---

## 2. The three apps

| | Where | How to run it for screenshots |
|---|---|---|
| **SwiftUI** (the original look; Mac) | `macos/Sources/NegsApp/` | `scripts/package_macos_app.sh --dev` builds `dist/negs-macos-dev/Negs.app`; never touch `dist/negs-macos`. Launch an isolated copy: change the bundle ID, re-sign it ad hoc, and set `HOME`/`CFFIXED_USER_HOME` to scratch. Capture with `screencapture -l <windowID>`, bringing the window to the front by PID first. The steps are in the `macos-app-test-tricks` memory. |
| **Win32** (a port of the SwiftUI look; PC) | `windows/src/ui/` | Main's `C:\Users\Noah\Projects\negs\windows\target\release\Negs.exe`, through `app/tools/pc-look/run.ps1`. |
| **GPUI** (what you change) | `app/src/` | Mac: `app/tools/run_mac.sh` (with `winid.swift`). PC: `app/tools/pc-look/`; start with its README. |

Where each area lives:

| Area | Win32 | SwiftUI | GPUI |
|---|---|---|---|
| Theme tokens, ink, type | `theme.rs`, `ink.rs` | `Theme.swift`, `AppTheme.swift` | `theme_gpui.rs` + `crates/negs-app` themes |
| Title bar, view words | `titlebar.rs`, `view_switch.rs` | `TitleBar.swift`, `MainMenu.swift` | `titlebar.rs`, `menu_bar.rs` |
| Inspector (tabs, rows, sliders, switches, headers, scopes) | `inspector.rs`, `panes.rs`, `ink.rs`, `tone_graph.rs`, `color_filter.rs` | `Inspector.swift`, `InkSlider.swift`, `InkControls.swift`, `PresetInspector.swift`, `ToneGraph.swift`, `ColorFilterPicker.swift`, `Histogram.swift` | `inspector.rs`, `widgets/*` |
| Presets sidebar | `presets.rs` | `Presets.swift` | `presets.rs` |
| Film strip | `strip.rs` | `PhotoBrowser.swift` | `strip.rs` |
| Status line | `status.rs` | `StatusLine.swift` | `status.rs` |
| Film/Paper tray | `tray.rs` | `Tray.swift` | `tray.rs` |
| Contact-sheet grid, flight | `grid.rs`, `sheet.rs` | `LibraryGrid.swift`, `LightTable.swift`, `SheetFlight.swift` | `grid.rs`, `sheet.rs` |
| Canvas, viewer sheet, print label | `canvas*`, `sheet.rs` | `PreviewCanvas.swift`, `ViewerSheet.swift`, `PrintLabel.swift` | `canvas.rs`, `print.rs`, `sheet.rs` |
| Compare, Crop | `compare.rs`, `canvas/print.rs` | `Compare*.swift`, `Crop.swift` | `compare_view.rs`, `crop_view.rs` |
| Settings | `settings.rs` | `Settings.swift`, `ThemePicker.swift` | `settings_window.rs` |
| Dialogs, alerts, export, batch | `dialogs.rs`, `dialog_look.rs`, `alert.rs`, `batch.rs` | `AlertSheet.swift`, `ImageSize.swift`, `Border.swift`, `BatchExport.swift` | `export_card.rs`, `batch_view.rs`, `win.rs` |
| Empty state, small states | `empty.rs` | `SmallStates.swift` | `canvas.rs`, `titlebar.rs` |

The Win32 app already encodes the look in Rust: sizes, paddings, row metrics, ink roles and fonts. That makes it the quickest spec to read. Still check each number against SwiftUI.

---

## 3. Differences already seen (Phase 6, Win32 vs GPUI, every theme)

These are the 12 baseline shots in `~/.negs/gpui-look/baseline-phase6/<theme>-{gpui,win32}.png`, all at 1600×1050 on the PC with `DSC_0262.NEF` open. This list is a **starting point**, not the inventory: SwiftUI hasn't been compared yet.

1. **Title bar.**
   - GPUI has a row of boxed buttons: Photo, Grid, Compare, Open, Save, Original, Split, Fit, a theme dropdown and Inspector.
   - Win32 shows only the words *Grid · Photo · Compare*, in italic Newsreader. `app/` already embeds Newsreader italic.
2. **Inspector tabs.** GPUI uses small sans-serif "Prep Lab Output". Win32 uses large serif words with an ink underline under the active one.
3. **Inspector rows.**
   - GPUI puts each slider full-width under its label, so rows are about twice as tall.
   - Win32 keeps the label, a short slider and the value on one line.
4. **Section headers.** GPUI's are small and dim. Win32's are bold, spaced capitals.
5. **Switches.** GPUI has a pill toggle; Win32 has a tick box.
6. **Film/Paper boxes** sit at the top of GPUI's inspector. Win32 doesn't have them there.
7. **Film strip.** Win32 centres the current photo and has a fold chevron.
8. **Status line contents.**
   - Win32 ends with "Original ⌄ · Fit · 100%".
   - GPUI ends with the display's colour profile.

---

## 4. Method

### 4.1 Baseline
Take all three apps through the same states, with the same photo, window size and theme:
- Photo, Grid, Compare and Crop;
- each inspector tab;
- the tray open;
- a menu open;
- Settings (each tab);
- the export dialog;
- an error;
- the empty window.

Do this in all six themes: Darkroom, Contact Sheet, Light Table, Daylight, Plain, Plain dark.

Compare SwiftUI with GPUI on the Mac, and Win32 with GPUI on the PC. Keep the shots under `~/.negs/gpui-look/<date>-<state>/`, not in the session scratchpad: a reboot clears that.

### 4.2 Inventory
Write `docs/gpui-look-cases.md`, one numbered case per difference (L1, L2, …). Each case gives:
- the area and the state it shows in;
- what Win32, SwiftUI and GPUI each do, with numbers where they exist (pt/px, colours by token name);
- whether either original has a visual bug there;
- the options, and **one recommended option** with its reason.

Platform chrome is its own case. That covers the traffic lights and native menu bar on the Mac, and the caption buttons and in-window menu bar on Windows. Decide early whether both platforms share one layout apart from that chrome (recommended).

### 4.3 Decisions, in batches by area
Put each area's cases to Noah as a **private artifact page**. Show each case's crops side by side (Win32 | SwiftUI | GPUI, plus a mock of the fix where one is proposed), with the options and your recommendation. Noah answers per case. Record each answer in `docs/gpui-look-cases.md`, with the date, and in the progress log.

- Don't implement a case before Noah has decided it.
- You may work on a decided batch while Noah looks at the next one.
- Pinging Noah per batch is expected for this phase. It overrides the coordinator's "ping only at checkpoints" rule.

### 4.4 Build it
Work area by area, in this order: the main window first, since that's what Noah sees every day.
1. Theme tokens and type scale, if any case changes them.
2. Title bar.
3. Inspector: tabs, rows, sliders, switches, headers, scopes and tone graph.
4. Film strip and status line.
5. Film/Paper tray.
6. Presets sidebar.
7. Grid, and the flight's end states.
8. Compare and Crop.
9. Settings, the export and batch dialogs, alerts, menus and tooltips.
10. Empty state and small states.

Each area is one branch (`gpui-look-<area>`), committed in small steps.

An area is done when:
- shots of it in all six themes, on the Mac and the PC, match the decided cases. Where a case says "match X", measure positions against X; within 1 physical px is the bar. Fixed cases match the agreed mock.
- the `app/` tests pass on both machines, with new tests for any layout maths pulled out of the GPUI code.
- Noah has seen the finished shots, sent as an artifact update.

Then merge it `--no-ff` into local `main`.

### 4.5 Speed check, then cutover
After the inspector and again at the end, re-run the Phase 6 ABBA (`app/tools/pc-look/job-abba.ps1`) and the `NEGS_FRAME_BENCH` runs on both machines. The new layout must not lose ground on any row of Phase 6's table:
- grid and flight at 60 fps, p99 ≤ 17 ms;
- tab switch ≤ 33 ms;
- slider-drag UI ≥ 51 fps;
- peak RAM ≤ ~345 MB.

Compact inspector rows mean more rows on screen, so watch tab switching and slider drag. The scenario coordinates in `scen-*.txt` follow the old layout: re-measure them. Then report to Noah for the cutover Go/No-Go.

**Phase 6 leftovers (only after the look, and only if cheap):**
- tooltips on the small controls (Phase 6's Deferred list);
- slider-drag UI p99 of 34 ms, probably the picture texture upload;
- startup at 474 ms against Win32's 96 ms.

---

## 5. Hard constraints (unchanged)

1. **Never touch the film engine.**
   - Off limits: `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, `web/`, and the root `Cargo.toml`/`Cargo.lock`.
   - Change `crates/negs-app`/`negs-library` only where the UI needs a missing model call. Keep those changes small, tested, and listed in the progress log. `windows/` must still build.
   - Theme token *values* live in `crates/negs-app`. If a case changes a shared token, check that `windows/` still looks right: it reads the same tokens until the cutover.
2. **Exports stay byte-identical.** Recheck after any change near `color_display`, `print.rs` or export options.
3. **Photographic quality.**
   - Photos and thumbnails go only through the ICC/P3 BGRA8 `RenderImage` path (`color_display`, `thumbs`, `tile_cache`).
   - Always `window.drop_image` a replaced texture.
   - Keep `SheetFlight` on both platforms.
4. **Commit dates.** Every commit on `main` is dated main's tip + 1 min (`GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE`). Nothing on `main` may be dated 08:00–16:00 Europe/Berlin except on Fridays. Main is at `2026-11-25 22:38 +0100` before this handoff's commit. In zsh, re-date with a bash script file: zsh doesn't word-split.
5. **Git.**
   - Never push, and never reset `main`.
   - Use `git diff --no-ext-diff`.
   - Never use bare `git stash`.
   - Check that the main checkout is on `main` before merging there; otherwise merge from your own worktree.
   - Agent worktrees may start from `origin/main`: reset them to local `main` first.
6. **Mac.**
   - Rust is in `/Users/noahhaasis/.nix-profile/bin`.
   - Build with `cargo test|build --release --manifest-path app/Cargo.toml --target-dir target/<yours>` inside your worktree. Never point it at the main checkout's `target/`.
   - For a warm start, `cp -c -R` Phase 6's `/Users/noahhaasis/Projects/negs/.claude/worktrees/gpui-p6/target/gpui-p6`. Then remove that worktree and its branch `gpui-p6-windows-cutover`: it is merged.
   - Use scratch copies of photos and scratch profiles (`NEGS_PROFILE_DIR`, `NEGS_ALLOW_MULTIPLE=1`).
   - Noah's own Negs may be running: target processes by PID, never by name.
7. **PC (`ssh pc`).**
   - One agent at a time. Use worktrees via `wt.cmd`, and scratch under `C:\Users\Noah\.negs\scratch\gpui-look`.
   - Never touch Noah's installed Negs, his profiles or photo folders, or the main checkout `C:\Users\Noah\Projects\negs`.
   - Other sessions' worktrees (`idle`, `p4-core`, `p4i`, `p4i-base`) and their processes aren't yours.
   - GUI runs go through the `schtasks /it` task.
   - When done, delete the task, the worktree and the scratch dir.
   - If `ssh pc` times out, ping Noah.
8. **GPUI 0.2.2 gotchas**, as in Phase 5/6:
   - `window.is_action_available` panics on the first frame.
   - Draw arrows and symbols in `theme.ui_family`: the Newsreader subset lacks them.
   - Wrapping text needs `.w_full().min_w_0()`.
   - Cache paths drawn often, as `marks_raster.rs` does.
   - Opacity on a picture comes from the parent div.
9. **Privacy.** Never put Noah's email or other personal details into external requests.

---

## 6. Report

At the end, update the progress log:
- a Look-parity section: cases and decisions, what landed per area, the shots and where they are, the speed table against Phase 6;
- a status row.

Clean up the PC. Then stop and ask Noah for the Windows cutover Go/No-Go, with:
- the six-theme side-by-side shots of the main states;
- the case list with each outcome;
- the numbers.
