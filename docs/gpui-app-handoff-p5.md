# Phase 5 Handoff — Cross-Platform GPUI Migration (Compare, Crop, Negative, Export, Batch, Settings & the rest)

Prepared 2026-11-25 at local `main` after `1d2b4abd` (not pushed to `origin/main`).
Read alongside [gpui-app-plan.md](gpui-app-plan.md), [gpui-app-coordinator.md](gpui-app-coordinator.md) and [gpui-app-progress.md](gpui-app-progress.md). The Phase 4 section of the progress log has the decisions and numbers this builds on.

---

## 1. Current state (Phases 0–4 done)

| Phase | Merge | What it gave `app/` |
|---|---|---|
| 0–1 | `5ef625e4`, `85281bf8` | Odin reverted; every non-UI module, `Model`, `inspector_view/` and themes in `crates/negs-app` behind `Host` + `platform/`. |
| 2 | `bd8c7662` | `gpui 0.2.2` shell; colour-managed exact-fit Lanczos canvas (`color_display.rs`: display ICC / Display P3 → BGRA8 `RenderImage`); Newsreader + 6 themes. |
| 3 | `87cb390d` | Widgets (`widgets/`: text input, ink slider, controls, scopes, tone graph, colour filters), Inspector, Presets sidebar, Film/Paper tray, `tile_cache.rs`. |
| 4 | `9c1020ed` | `thumbs.rs` (background, colour-managed, LRU, `drop_image`), `strip.rs`, `grid.rs` + `SheetFlight`, `library_menu.rs` (context + filter menus, Trash prompt), `marks_raster.rs` sprite cache, `motion.rs` (Reduce Motion), `bench.rs` (`NEGS_FRAME_BENCH`). |

**Decision (Noah, 2026-11-25): keep `SheetFlight` on both macOS and Windows for now.** Don't swap Windows to the Win32 app's 150 ms fade (`sheet_rules::FADE_MS`).

`app/` has 58 unit tests, all passing on Mac and PC, with 0 warnings in `negs-gpui`. The `spektrafilm-io` `finish_rgb` warning is engine code: leave it.

### What `app/` already does that Phase 5 plugs into
- `main.rs`:
  - `NegsApp` root, `actions!` list, key bindings, macOS menu bar;
  - `after_model_update` (drains `Host::post` work, syncs strip/grid, sets `first_render_shown`);
  - `refresh_display_profile`, `shutdown_cleanly`.
  - Keys that must not be swallowed by actions (grid arrows, ←/→, Delete) go through the root `on_key_down` with a `window.context_stack()` "TextInput" check. See Phase 4 decisions.
- `canvas.rs`:
  - fit/100%/zoom/pan, Original, Split before/after, `shown_trial()` stand-in;
  - `hide_photo` and `whole_image()`, used by the flight. Crop and Compare take the canvas's place or draw over it.
- `library_menu.rs`: `MenuState` popover + `Command` enum, and `request_delete` showing the platform alert via `window.prompt`. Reuse the popover for other menus; Alerts may outgrow `window.prompt` (see 5b).
- `inspector.rs`: already dispatches `apply_effects`, including the Output tab's export summary and an **Export button that needs the 5b dialog behind it**.
- `status.rs`: the status line. Batch progress hangs off it.

---

## 2. Phase 5 scope

Plan row 5: *Compare mode, Crop mode, Negative scan/flat-field UI, Export dialog, Batch Export progress, Open in Lightroom, Settings window, Viewer Sheet, and Alerts. Gate: full feature-parity checklist vs `docs/windows-ui-features.md` and `macos/` passes on both Mac and Windows PC; Mac↔Windows sidecar round-trip byte-identical.*

Split as the coordinator says, into two branches merged one after the other.

### 5a — `gpui-p5a-compare-crop-negative`
| Feature | Shared model, already built (don't rewrite) | Windows reference | macOS reference |
|---|---|---|---|
| Compare (ticket 46): side by side, numbered rings, split slider, Esc exits | `negs_app::compare`, `model/compare.rs` | `windows/src/ui/compare.rs` (1.4k) | `Compare.swift`, `CompareCanvas.swift` |
| Crop & straighten: handles, aspect presets, straightening dial in the label's place, dimmed outside, Return/Esc | `negs_app::crop`; `Model::{cropping, begin_crop, crop_changed, crop_draft_size}` | `windows/src/ui/canvas/print.rs`, `canvas/software.rs` (crop shade blend) | `Crop.swift`, `SmallStates.swift` |
| Negative scan / flat field (ticket 33): picking the film base, flat-field frame | `negs_app::negative`, `model/negative.rs` | `windows/src/ui/inspector.rs` negative rows | `Negative.swift` |
| Viewer Sheet: the sheet the print lies on + inks (`sheetInk`), View › Viewer Sheet | `negs_app::theme` sheet tokens | `windows/src/ui/sheet.rs` | `ViewerSheet.swift`, `LightTable.swift` |
| Image Only (F) and hold-T views | — | `windows/src/ui/view_modes.rs` | `MainMenu.swift` |
| Drop a photo or folder on the canvas; the empty state ("Open a photograph") | `Model::open`, `open_folder` | `windows/src/ui/drop.rs`, `empty.rs` | `SmallStates.swift` |

### 5b — `gpui-p5b-export-settings`
| Feature | Shared model | Windows reference | macOS reference |
|---|---|---|---|
| Save Preview / Export: format, colour space, quality, bit depth, size, border; options inside the save dialog (§20.4, decided) | `negs_app::{border, previews, exact_fit}`, `Model` export calls | `windows/src/ui/dialogs.rs`, `dialog_look.rs` | `ImageSize.swift`, `Border.swift`, `EditorModel.swift` |
| Batch Export + progress popover over the status line / grid foot | `negs_app::batch`, `model/batch.rs` | `windows/src/ui/batch.rs` | `BatchExport.swift` |
| Open in Lightroom (never click Add in Lightroom; cold-start LR drops files) | `negs_app::lightroom`, `model/lightroom.rs` | `windows/src/ui/library.rs` | `Lightroom.swift` |
| Settings window: tabs, Viewer, Library (sidecars, camera JPEGs), New photographs, Fotufilm films, updates/About | `negs_app::session::Preferences` | `windows/src/ui/settings.rs` (1.2k) | `Settings.swift`, `ThemePicker.swift`, `Updates.swift` |
| Alert card for errors, warnings and questions not about the photo on screen | `Model.error`, `delete_question` | `windows/src/ui/alert.rs` | `AlertSheet.swift` |
| Menu-bar parity: Photo menu (Copy/Paste Edits, Paste from Previous, Duplicate, Reset, Trash, Filter submenu), View (Viewer Sheet, Image Only), Help | `library_menu::library_entries` / `filter_entries` already list them | `windows/src/ui/mod.rs` menus | `MainMenu.swift` |

The feature-by-feature checklist is `docs/windows-ui-features.md` (§2 menus/shortcuts, §9 crop, §10 borders, §11 Save Preview/Export, §13 status/progress/errors, §15 settings/updates/About, §18 accessibility; §19 is Mac-only: don't port; §20 lists open decisions). Tick each item off in the progress log and say which are deferred to Phase 6/7 and why.

### Also carried over from Phase 4
- The Mac grid fling had a 24 ms render spike (3 frames > 25 ms) while Docker was using ~300% CPU. Re-measure on a quiet Mac (Low Power Mode off, on AC, no other sessions; see the benchmark-hygiene memory). If it's still there, find out whether sprite making or thumbnail upload causes it.
- The bench JSON's `window` field is really `[grid viewport height, strip width]`. Rename it if you touch `bench.rs`.
- Check that the GPUI menu bar shows on Windows at all. If `gpui 0.2.2` has no native Windows menus, the title bar needs a menu entry point. That's a 5b item, so decide and document it.

---

## 3. Hard constraints (unchanged from Phase 4)

1. **Never touch the film engine:** `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, `web/`, root `Cargo.toml` / `Cargo.lock`. `crates/negs-app` / `negs-library` changes are allowed only where a UI truly needs a missing model call; keep them small, tested, and listed in the progress log. `windows/` must still build against them.
2. **Exports stay byte-identical** (accuracy over approximation): the Export UI only collects options and calls the shared model.
3. **Commit dates:** every commit on `main` is dated main's tip + 1 min (`GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE`). Nothing on `main` is dated 08:00–16:00 Europe/Berlin except on Fridays. `main` is at `2026-11-25 18:39 +0100`, so continue from 18:40.
4. **Git:**
   - Branches `gpui-p5a-…` / `gpui-p5b-…`; merge `--no-ff` into local `main` only after the Mac + PC gates pass.
   - **Never push**, never `git reset` `main`.
   - Use `git diff --no-ext-diff` (difftastic is the default).
   - The stash is shared: never use bare `git stash`.
   - Agent worktrees may start from `origin/main`: reset to local `main` first.
   - The main checkout may be on another session's branch: check before merging there.
5. **Photographic quality:** photos and thumbnails only through the ICC/P3 BGRA8 `RenderImage` path (`color_display`, `thumbs`, `tile_cache`). Always `window.drop_image` replaced or evicted textures (`thumbs::release` queues them).
6. **GPUI 0.2.2 gotchas:**
   - Single-key shortcuts are scoped `Some("NegsApp && !TextInput")`.
   - Draw arrows and symbols in `theme.ui_family`; the subsetted Newsreader lacks them.
   - Wrapping text in a flex container needs `.w_full().min_w_0()`.
   - `paint_path` re-tessellates every frame, so cache anything drawn often (see `marks_raster.rs`).
   - `paint_image` opacity comes only from a parent div's `.opacity()`.
   - On Windows use `HasWindowHandle::window_handle(window)` via UFCS.
   - `app/` can't be cross-checked for Windows from the Mac: build it on the PC.
7. **Mac:**
   - Rust is in `/Users/noahhaasis/.nix-profile/bin`. Env: `CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_INCREMENTAL=true MACOSX_DEPLOYMENT_TARGET=14.0`.
   - Build with `cargo test|build --release --manifest-path app/Cargo.toml --target-dir target/gpui-p5`. To skip a cold build, APFS-clone a warm one: `cp -c -R <main checkout>/.claude/worktrees/gpui-p4/target/gpui-p4 target/gpui-p5`, if it's still there.
   - Engine helper: `/Users/noahhaasis/Projects/negs/target/release/spektrafilm-native` with `SPEKTRAFILM_ROOT=/Users/noahhaasis/Projects/negs`.
   - Never touch `dist/negs-macos`. Noah's `/Applications/Negs.app` may be running: never target processes by name, only by PID.
8. **Windows PC (`ssh pc`):**
   - Only one agent at a time. If it times out, ping Noah.
   - Never touch Noah's installed Negs, his profiles (always `NEGS_PROFILE_DIR=<scratch>` + `NEGS_ALLOW_MULTIPLE=1`), the main checkout `C:\Users\Noah\Projects\negs`, or his photo folders. Copy test RAWs from `C:\Users\Noah\Downloads\102ND750` and `C:\Users\Noah\Downloads\raws` (67 NEFs together) into scratch.
   - Worktree: `C:\Users\Noah\.negs\wt.cmd gpui-p5 <mac-branch>` fetches the Mac's branch over ssh into `negs\worktrees\gpui-p5`; run it again to update. Scratch: `C:\Users\Noah\.negs\scratch\gpui-p5`. Remove both when done (`git -C C:\Users\Noah\Projects\negs worktree remove --force worktrees\gpui-p5`, `rmdir /s /q …`).
   - The shell is cmd.exe. Prepend `set "PATH=C:\Users\Noah\.cargo\bin;%PATH%"&&`, with no space before `&&` after `set`.
   - The display is a Dell P3424WEB at 125%, so client coordinates are physical pixels.
   - Engine helper: `SPEKTRAFILM_ROOT=C:\Users\Noah\Projects\negs` (that's where `target\release\spektrafilm-native.exe` is; `negs-presets` has none).
   - GUI runs go to Console Session 1: `schtasks /create /it /ru Noah /sc once /st 00:00 /tn negs-gpui-p5 /f /tr "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File <script> <args>"` then `schtasks /run /tn negs-gpui-p5`. Wait for the sentinel file, then **always** `schtasks /delete /tn negs-gpui-p5 /f`.
   - Interactive screenshots (clicks, keys, drags, front windows such as Settings): `C:\Users\Noah\.negs\shot.ps1 -JobFile <job.json>`; its header documents the job format.
9. **Privacy:** never put Noah's email or other personal details into external requests.

---

## 4. Tools from Phase 4 (in `app/tools/`)
- `NEGS_FRAME_BENCH=<out.json>` (+ `NEGS_BENCH_SHOTS=<dir>`) makes the app drive itself through strip/grid/flight phases and write p50/p95/p99 frame intervals and render times. `<name>.ready` → the outside script captures the window → `<name>.done`. Extend `bench.rs` steps for Phase 5 views (Compare open, crop drag, slider drags with Settings open) instead of writing a new harness.
- `app/tools/run_mac.sh <Negs> <photos> <scratch> <bench.json|none> <shots|none> <timeout>` launches with a scratch profile, captures by PID (`winid.swift`), and answers the handshake.
- `app/tools/pc_bench.ps1 -Root <scratch> -Exe <Negs.exe>` does the same on the PC. `scp` it to the PC scratch dir and run it through `schtasks`; it writes `finished.txt` last.
- Mac test roll: APFS-clone 60+ NEFs into the scratch dir (`cp -c`); Phase 4 used 72 from `~/Downloads/Z 6`. Never use the originals.

---

## 5. Gate and checkpoint
1. **Tests:**
   - New unit tests for each view's pure logic: compare layout/split, crop handle maths and aspect locking, negative sampling hit-tests, export option → model mapping, settings ↔ `Preferences` round-trip. Pull logic out of GPUI handlers as Phase 4 did (`grid::moved`, `strip::click_selection`).
   - All tests pass on Mac and PC with 0 warnings in `negs-gpui`.
   - `cargo test --release -p negs-app -p negs-library` stays green on both if you touched them.
2. **Parity checklist:** go through `docs/windows-ui-features.md` item by item on both machines; record ✓ / deferred (why) in the progress log.
3. **Sidecar round-trip:** edit, rate and crop on the Mac, copy the folder to the PC, open it, edit there, copy back. The sidecars must stay byte-identical where nothing changed, and every edit must be visible on the other side.
4. **Exports:** export the same photo with the same settings from GPUI `app/` and the shipping `windows/` / `macos/` app; the files must be byte-identical (glare and grain off for pixel compares).
5. **Screenshots** on Mac and PC of each new view in Darkroom, Light Table and Plain dark. Re-run the Phase 4 bench to show no regression.
6. Merge 5a and 5b `--no-ff` into local `main`, update `docs/gpui-app-progress.md` (status table, a section per sub-phase, decisions, numbers), clean the PC, and **stop and report to Noah before Phase 6** (Windows cutover), with the parity checklist, test counts, timings and screenshot paths.
