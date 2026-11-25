# Cross-Platform GPUI App & Pure-Rust Shared Core: Progress Log

Newest first. Live source of truth for every completed phase, merge SHA, verification result, architectural decision, and performance measurement.

See [gpui-app-plan.md](gpui-app-plan.md) and [gpui-app-coordinator.md](gpui-app-coordinator.md).

## Phase Status

| # | Phase | State | Merge SHA | Summary |
|---|---|---|---|---|
| 0 | Clean Odin Revert | **Done** | `5ef625e4` | Forward-reverted `core/` & Odin wiring on `main`; preserved `v0.18`–`v0.20`, all film stocks, `8154a6ac`, and `windows/src/inspector_view/`. Mac + PC green. |
| 1 | Extract Pure-Rust `crates/negs-app` | **Done** | `85281bf8` | Promoted `windows/marks` → `crates/negs-marks`; extracted all 32 non-UI modules + 6 themes + `Model` into standalone `crates/negs-app` behind `Host` & `platform`; `windows/` thin shell green on PC. |
| 2 | GPUI Shell + Color-Managed Canvas | **Done** | `bd8c7662` | Standalone `app/` workspace on `gpui 0.2.2`; embedded Newsreader + 6 themes; fused linear-light exact-fit Lanczos-3 + per-display ICC/P3 pipeline; vector `negs-marks`; Mac & PC live verified. |
| 3 | Inspector, Presets & Stock Tray | **Done** | `87cb390d` | Custom darkroom widgets (`InkSlider`, `TextInput`, `Scopes`, `ToneGraph`, `ColorFilterDiscs`), full 26-kind `InspectorView` Prep/Lab/Output UI, Presets sidebar, and Film/Paper Stock Preview Tray (`TileImageCache`). Mac & PC live verified. |
| 4 | Library, Filmstrip, Grid & Flight | **Done** (Smoothness Proof reviewed 2026-11-25) | `9c1020ed` | Colour-managed background thumbnails, virtualized filmstrip + contact-sheet grid with hand-drawn marks (cached sprites), library menus, `SheetFlight` Photo↔Grid flight, `NEGS_FRAME_BENCH`. PC 60.0–60.8 fps, 0 dropped frames. |
| 5 | Compare, Crop, Negative, Export, Batch & Settings | **Done** (5a + 5b) | 5a `200def15`, 5b `7720b5ca` | Print on its sheet, crop, negative pick, Compare; export card → native dialog, batch, Settings window, full menus (in-window menu bar + caption buttons off the Mac), Lightroom. Exports byte-identical to `windows/` and SwiftUI. |
| 6 | Windows Cutover & Packaging | **Parity merged; cutover deferred** (look first) | `5bc98c59` | ABBA vs Win32 `Negs.exe` (GPUI ahead on grid, strip, tabs, RAM; behind on startup and size), Windows parity (menu keys/ticks, TaskDialog errors, taskbar, WinSparkle, end-session, single instance, tooltips), packaging plan prepared, not applied. |
| 6L | Look parity (before the cutover) | **Next** | — | Make `app/` look like Negs, case by case (Win32 / SwiftUI / GPUI / fix each original's visual bugs), area by area with six-theme shots; see [gpui-app-handoff-look.md](gpui-app-handoff-look.md). |
| 7 | Linux Adoption & macOS Polish | Pending | — | 7a Linux (T2 parity, retire GTK4), 7b macOS (HIG polish, Go/No-Go before retiring SwiftUI). |

---

## 2026-11-25 — Phase 6: Windows cutover gate (`5bc98c59`; cutover not applied)

### What landed
- Branch `gpui-p6-windows-cutover` (`fedfe600` … `f610edd7`, 12 commits) merged `--no-ff` into local `main`; `app/` tests 98/98 on the merged tree on both machines.
- Under `app/` and `docs/` only (no `crates/`, `vendor/`, `web/`, `windows/`, root `Cargo.toml`/`Cargo.lock`; `app/Cargo.lock` unchanged). Not pushed.
- **Speed:** the canvas read the display's ICC profile and rebuilt its transform on every paint: 82% of the main thread in a slider drag (macOS `sample`). Now it does so only when the window changes display or becomes active. PC slider UI went 30 → 51 fps; Mac strip fling 73 → 91–120 fps. The grid's first open now flies too (it had no size before its first frame). `NEGS_TRACE_UI=<file>` times drag steps and renders.
- **Menu bar from the keyboard** (`menu_bar.rs`, `menu_state.rs`):
  - Alt alone or F10 focuses it with access keys underlined. Alt+letter opens a menu. ← → ↑ ↓, Return/Space, a letter, Esc work as in Windows menus.
  - Items are ticked, disabled and titled as `windows/`'s `update_menu` does ("Undo Change Adjustment", "Done Cropping", "Move 3 Photographs to Recycle Bin…").
  - Photo › Filter ▸, View › Show All Controls, Help › Check for Updates….
  - The access keys follow `windows/`; the Mac's menus drop them.
  - Rows are built only for the open menu: asking what's enabled needs the last frame's dispatch tree, and on the first frame GPUI panics (`key_dispatch.rs:383`).
- **Errors** in a TaskDialog titled "Negs", with the exporter's log under "See details" (`win.rs`). Windows only: elsewhere the status bar's banner stays. **Notices** (presets imported) show on every platform; they showed nowhere before.
- **Taskbar progress**: indeterminate for one export, counted for a batch. The button flashes when an export ends while Negs is in the background.
- **WinSparkle** (`updates.rs`), ported from `windows/` and inert unless packaging configures it. Settings › Upkeep then shows Automatically check / Last checked / Check for Updates….
- **WM_QUERYENDSESSION / WM_ENDSESSION** save the edits, the session and the window's place without asking.
- **One Negs**: a second launch hands its photograph to the running window (`Local\Negs.SingleInstance`, `WM_COPYDATA`; the main window carries a `Negs.MainWindow` property). `Local\Negs.AppLifetime` is held for the installer, and AppUserModelID is `dev.negs.app`. These were found missing while reading `installer.iss`.
- **Window placement** is restored if still on a display.
- **Icon, version resource and manifest** (`app/build.rs`, `app/negs.manifest`). GPUI's `windows-manifest` feature is off: its manifest has no `longPathAware`.
- **Tooltips** (`tooltip.rs`, themed, quiet during a slider drag) with `windows/`'s help:
  - inspector rows (`Kind::help`, Match brightness's), and a slider's ↺ ("Reset to 0");
  - title bar Photo/Grid/Compare/Open/Save;
  - pane words, Export, the crop panel, and the presets' Import.

### Noah's answers (2026-11-25)
- Updates: **WinSparkle**.
- Accessibility (no UIA in GPUI 0.2.2): **Go anyway**; say so in the release notes, follow up on GPUI accessibility later.
- Cutover (packaging plan): **deferred**. The GPUI UI still looks very different from `windows/`/SwiftUI; that comes first.

### Decisions
- **Updates: WinSparkle** (already `windows/`'s, §20.6), not MSIX: it keeps the Inno installer, the per-user install without admin, and the existing appcast/EdDSA scheme shared with Sparkle on the Mac. MSIX would need a signing certificate in the Store/trusted chain, a package identity rework, and new file-association plumbing. *Asked of Noah in the Go/No-Go.*
- Errors become dialogs only on Windows (as `windows/`); the Mac and Linux keep the status bar's banner until 7a/7b decide.
- A forwarded photograph opens on the app's executor (channel), never inside the window procedure.
- `runtime_shaders` stays: it only affects the Mac (Windows release builds precompile HLSL with fxc).

### Deferred (and why)
- **Accessibility:** GPUI 0.2.2 has no UI Automation/MSAA tree. A screen reader finds nothing in the window, where `windows/` has an `IAccessible` tray and named controls. It can't be added from `app/` without a GPUI fork. *Put to Noah as a trade-off.*
- Tooltips on preset tiles, colour-filter discs, tab counts, the scope grip, Lightroom foot buttons, Compare's Edit and the tray's Done: small controls whose names show already; next pass.
- "See details" with an exporter log, and the taskbar flash, were not seen live. Errors with a log come only from a failing export. The flash needs Negs in the background as an export ends. The code paths are the same as the counted progress and the TaskDialog that were seen.
- Startup: GPUI's first window comes at ~475 ms against Win32's ~95 ms. GPUI creates the DX11 device and composition swap chain before its first frame, where `windows/` paints a GDI window first. This is within the plan's 480–600 ms budget; no cheap win found (shaders are precompiled already).
- Slider-drag UI is 51 fps with p99 34 ms (Win32: 48–51 fps, p99 84 ms). About a third of the 25 canvas updates a second costs a UI frame: likely uploading the new picture texture. Next pass.

### Verification Gate
- **ABBA on the PC** (`ssh pc`, Console Session 1 via `schtasks /it`):
  - Conditions:
    - 3440×1440 display, window 1584×1042 at 40,40;
    - scratch copies of 67 NEFs, open `DSC_0262.NEF`, Darkroom;
    - Creative Cloud helpers at 5–10% CPU, 8.7 GB free;
    - both builds without LTO (`build-main.cmd`'s settings).
  - Method: GPUI vs shipping `windows\target\release\Negs.exe`. Frames are measured from outside with DXGI Desktop Duplication (probe rows hashed per desktop frame, input by `SendInput`, QPC). 8 alternating runs (GPUI, Win32, Win32, GPUI, …) on `a9d15131`, then 2+2 on the final build (`8d39797b`).

  | | GPUI | Win32 `Negs.exe` |
  |---|---|---|
  | Strip fling (wheel −40 / 8 ms) | 55.7 fps, p99 41.8 ms, 1 frame > 25 ms | 45.5–50.1 fps, p99 42–60 ms, 7–13 frames > 25 ms |
  | Grid open (final build) | SheetFlight 60.0 fps, p99 17.0 ms, 0 > 25 ms; settled 520–535 ms (480 ms flight) | fade 60.0 fps, p99 16.8 ms; settled 166–186 ms |
  | Grid open (`a9d15131`, 8 runs) | 36–57 fps, one 63–217 ms first frame per run | as above |
  | Grid scroll | 60.0 fps, p99 16.9–17.1 ms, 0 > 25 ms | 37.5–49.1 fps, p99 33–49 ms |
  | Grid close | 48–60 fps, p99 16.9–33 ms | 26–60 fps, p99 17–66 ms |
  | Tab switch Lab / Output / Prep | 33 / 27 / 23 ms | 84 / 34–50 / 34 ms |
  | Slider drag, UI | 50.7–51.9 fps, p99 34–50 ms | 48–51 fps, p99 83–85 ms |
  | Slider drag, canvas updates | 24.7–25.6 /s | 22.3–23.3 /s |
  | Idle | 0 frames | 0 frames |
  | Peak working set, app | 331–342 MB (private 382–387 MB) | 478–484 MB (private 513–518 MB) |
  | Engine helper | 652 MB WS, 734 MB private | 652 MB WS, 992 MB private |
  | Startup, first window / first image (median of 8) | 474 / 1208 ms | 96 / 953 ms |
  | `Negs.exe` size, no LTO | 30,800,896 B | 21,305,856 B |
  | `Negs.exe` size, shipping profile (thin LTO, 1 CGU) | 23,927,296 B | 19,001,344 B |

- **`NEGS_FRAME_BENCH` on the PC** (final build, Phase 4 continuity):
  - strip 60.0 fps, p99 17.1 ms;
  - grid 60.0 fps, p99 17.1 ms;
  - flight open 60.6 fps, p99 17.1 ms;
  - flight fold 60.5 fps, p99 17.2 ms;
  - 0 frames over 25 ms.
- **Mac `NEGS_FRAME_BENCH`** (AC power, Docker using 3 cores, load 12):
  - flights 120 fps (p99 10–12 ms);
  - grid 120 fps (p99 10.7 ms);
  - strip 91 fps (p99 15.9 ms);
  - 0 frames over 25 ms.
- **Unit tests (`app/`): 98 passed, 0 failed on macOS and on the PC.** 0 warnings in `negs-gpui` (the one warning is `spektrafilm-io`'s `finish_rgb`, as on main).
- **`windows/` still builds** (shipping profile, `--locked`, on the PC).
- **Export byte-identity** (`_NHA9786.NEF`, Reference, PNG 8-bit sRGB, full size, grain and glare off):
  - PC: 28,667,269 B, SHA-256 `F47D7676…1F763` as before.
  - Mac, with Phase 5's photo and PC sidecar: `3f1d1062…760e8a` as before. A fresh Mac sidecar gives `11d1fe3c…` from both this branch and `main` (5ecf1892), against both engine sets: the Mac's own brightness match, not the branch.
- **Seen on the PC:**
  - Alt cues; Alt+P, → View, F10 ↓ File;
  - ticks, disabled rows, shortcut hints;
  - "Undo Change Adjustment"; Photo › Filter ▸;
  - tooltips;
  - TaskDialog "Negs" ("“folder” has no photographs Negs can open.");
  - taskbar progress during an export;
  - icon on the taskbar;
  - `WM_QUERYENDSESSION` → 1, and `WM_ENDSESSION` writing `session.json` and the window's place;
  - a second launch (exit 0) opening its photograph in the first window.
- **Screenshots, every theme, both apps** (12, `<theme>-{gpui,win32}.png`), taken on the PC and kept in the session's Mac scratchpad (`…/scratchpad/pc/themes/`); the PC's scratch copies were removed with the worktree.

---

## 2026-11-25 — Phase 5b: Export, Batch, Settings, Menus (`7720b5ca`)

### What landed
- Branch `gpui-p5b-export-settings` (`f72db1a1` … `0dcbf717`, 14 commits) merged `--no-ff` into local `main`. Changes are under `app/` and this log only (no `crates/`, `vendor/`, `web/`, `windows/`, or any `Cargo.toml`/`Cargo.lock`). Not pushed.
- `app/src/export_card.rs`: **Save Preview…, Export Photograph… and Export N Photographs…** go through a darkroom options card (Format, Engine, Size + px, Bit depth, Colour space, JPEG quality; for a batch: File name, If a file exists), with the message that follows the choices. Its button opens the plain system save or folder dialog with the name and extension already set. A typed extension picks the format. The choices are remembered in the preferences (`exportEngine`, `outputFormat`, `exportEdge`, …, `lastSaveFolder`, `batchNaming`). **Export LUT…** saves `<film>_<paper>.cube`. Esc closes the card and Return confirms; other keys wait.
- `app/src/batch_view.rs`: the batch popover over the status line (progress rule, the photograph being exported, failures listed then "and N more", skipped, Cancel / Retry Failed / Show in Finder·Explorer·Files). The status line's words show a running batch, Lightroom's words, the outcome, or "Exporting name · N s" + Cancel. They tick each second. **A batch the last session didn't finish is offered at launch** (Resume / Discard).
- Quitting during an export or batch asks ("Continue Exporting" / "Quit"), and while a preview is being written it says so and stays open (QuitApp and the window's close).
- `app/src/settings_window.rs`: **Settings** (⌘, / Ctrl+,) is its own window with `windows/`'s four pages:
  - Photographs: new photographs' film and paper, Match to camera brightness, Fotufilm films, Negs-built films, RAW + JPEG, sidecars.
  - Speed: previews, 12 MP limit, low-memory export.
  - Upkeep: updates not configured; Lightroom sent copies and Delete Sent Copies, with Classic's question first.
  - Look: theme swatches and Viewer sheet.
  - The page is remembered (`settingsTab`).
- **Menus** in `windows/`' order: File (Open, Open Recent ▸, Save Preview, Export Photograph, Export LUT, Open in Lightroom [Classic] for each one installed, New Preset, Import Presets, Show Presets Folder; Settings and Exit off the Mac), Edit, Photo (Show Original, Rating ▸, Flag ▸, Copy/Paste Edits, Paste from Previous, Duplicate, Reset Edits, Move to Trash/Recycle Bin, Delete Rejected, Advance After Rating, Clear Filter), View (panes, Image Only, Full Screen, Grid, Compare, Split, zoom, rotate, flip, crop, Theme ▸, Viewer Sheet ▸), Help › About (Negs › About/Settings/Quit on the Mac). New keys: ⌘/Ctrl+Shift+C/V, ⌘/Ctrl+D, ⌘/Ctrl+Shift+R, `[` `]`, ⌘/Ctrl+Shift+O, F11 (Ctrl+Cmd+F on the Mac). The library's right-click menu gains Export… and Open in Lightroom.
- `app/src/menu_bar.rs`: **an in-window menu bar off the Mac.** GPUI 0.2.2 keeps menus on Windows and Linux but draws none. Their words sit after "Negs", each opens its menu under it (submenus beside, shortcuts as Windows writes them), and moving onto another word while one is open switches.
- **Caption buttons and dragging off the Mac** (`titlebar::caption_buttons`). GPUI's own title bar is hidden, so the Windows window had no Minimise, Maximise or Close and could not be dragged. They are now `WindowControlArea`s (Segoe MDL2 glyphs, red Close hover). "Negs", the file name and the space between the title bar's groups drag the window. Settings has a drag strip and Close.
- **Tiles off → stock menu.** With Settings › Speed's previews off, the print's film/paper/read-as words and the inspector's Film and Paper open the stocks as a menu, as `windows/` does: favourites starred, groups under headings, the chosen one ticked, Your negative last. The library menu scrolls when long.
- Lightroom is detected 2 s after the first picture, and again when the window becomes active (with `library_refresh`, as `WM_ACTIVATEAPP` does).
- Bench tours (`NEGS_BENCH_TOUR`): `export` (cards + a real Reference export into `NEGS_BENCH_EXPORT_DIR`), `sidecar` (`NEGS_BENCH_SIDE=mac1|pc|mac2`), `settings` (each page, two themes, the stock menu; off the Mac the menu bar), `hold` (idle 20 s for outside checks). `settings-*` shots capture the front window (`winid … front`, `GetForegroundWindow`).

### Decisions
- **Options card, then the native dialog** (Noah, 2026-11-25). GPUI's save dialog takes no filters or controls, so the format, engine and size are chosen in-window first. The batch's folder chooser comes the same way.
- **Questions use GPUI's native prompt** (NSAlert / TaskDialog): quit, resume batch, Delete Sent Copies, Delete Rejected. Errors keep the status line's error. A darkroom-styled alert card would only restyle those dialogs.
- **Off the Mac the title bar leaves out the stock badge.** The menus need the room, and the caption under the print and the status line name the film and paper already. With it, at 1584 px the file name was cut to "_NI" and only 16 px were left to drag by.
- **Drag only where nothing is clickable.** GPUI's Windows hit test returns the first control area under the pointer before any click handler, so a Drag area behind the pills would swallow their clicks.
- **Menus are rebuilt** when the installed Lightroom kinds or the recent files change (`cx.set_menus`). Open Recent uses ten fixed actions (`Recent0…9`), as `actions!` carries no data.
- **Settings opens after the update that asks for it** (`cx.defer`). `open_window` draws at once, and Settings reads the app entity, which would still be leased (it panicked on the first try).
- **Settings' tick test only reads.** The setters save preferences, and `negs-app`'s test-profile guard is `cfg(test)` of its own crate, so an `app/` test calling them would write the real profile.

### Deferred (and why)
- **Updates** (Settings › Upkeep, Help › Check for Updates): the updater is Phase 6's choice (WinSparkle vs. MSIX); the page says "not configured".
- **Menu state:** GPUI 0.2.2 menus have no checked or disabled items, and their titles are fixed. So there is no tick on Film Strip, Adjustments or Advance After Rating, no "Undo Change Film", and impossible items are no-ops rather than grey. This needs GPUI menus or our own (Phase 6).
- **The menu bar from the keyboard** (Alt, F10, Alt+letter, arrows): with the Phase 6 accessibility pass.
- **Photo › Filter ▸** (the grid sentence's menu has it; Photo has Clear Filter); **View › Show All Controls**; **Edit › Cut/Copy/Paste/Select All** items (text fields take the keys themselves).
- **Taskbar progress and flashing** (`ITaskbarList3`) and the exporter log as an expandable error detail: Windows-only work for Phase 6.
- **Lightroom buttons in the inspector's Export foot** (it's in File and the library menu), and the grid foot's batch words.

### Verification Gate
- **Unit tests (`app/`): 93 passed, 0 failed on macOS and on the PC** (15 new: export card choices/destination/messages, batch popover and resume title, Settings pages/ticks/sizes, menu shortcut words, stock menu and Read as). 0 warnings in `negs-gpui`.
- **Export byte-identity:** `_NHA9786.NEF`, Reference, PNG 8-bit sRGB, full size, grain and glare off via the sidecar.
  - PC: GPUI app and shipping `windows/` `Negs.exe` both write 28,667,269 bytes, SHA-256 `F47D7676…1F763`. The same again from the final build (35.8 s; 67.5 s once while another session compiled on the PC).
  - Mac: the GPUI app and the shipping SwiftUI app (an isolated copy of `dist/negs-macos-dev`, same engine binaries) both write `3f1d1062…760e8a`.
  - Mac and PC differ from each other (ARM vs x86 f64), as before.
- **Sidecar round trip** (8 NEFs; scratch copies on both):
  - Mac edits `_NHA9786` (+0.7 EV, square crop), rates `_NHA9787` 3★ and picks `_NHA9788`: 3 sidecars.
  - On the PC, the GPUI app reads all of them back exactly (ev 0.7, crop 0.166,0,0.668×1; 3★; Pick), then rates `_NHA9789` 5★ and gives `_NHA9790` −0.3 EV + Reject.
  - Back on the Mac, all five read as written.
  - The Mac's three sidecars stayed **byte-identical** through the PC session, and all five through the second Mac session. Untouched photos got no sidecar.
- **Title-bar hit test on the PC** (real cursor moves, `WM_NCHITTEST`): "Negs", the file name and the space before the pills → `HTCAPTION`; the menu words and pills → `HTCLIENT`; then `HTMINBUTTON`, `HTMAXBUTTON`, `HTCLOSE`.
- **Phase 4 frame bench re-run**, A/B against pre-5b `main` (`200def15`) built the same way and run alternately (base, 5b, base, 5b):

| Phase | Machine | pre-5b fps (2 runs) | 5b fps (2 runs) | 5b interval p99 (ms) | 5b render p99 / max (ms) |
|---|---|---|---|---|---|
| Strip fling 4 s | Mac | 73.7 / 74.4 | 73.9 / 69.3 | 22.4 / 31.7 | 0.6 / 0.7 |
| Flight open ×5 | Mac | 77.2 / 61.9 | 77.9 / 75.7 | 13.7 / 14.2 | 3.8 / 4.4 |
| Flight fold ×5 | Mac | 76.1 / 57.9 | 76.7 / 75.2 | 15.1 / 15.3 | 3.6 / 3.8 |
| Grid fling 6 s | Mac | 119.6 / 119.6 | 119.3 / 119.8 | 10.7 / 10.2 | 1.7 / 2.4 |
| Strip fling 4 s | PC | 55.2 / 47.2 | 53.8 / 47.2 | 34.9 / 39.5 | 2.5 / 2.7 |
| Flight open ×5 | PC | 51.6 / 53.1 | 50.3 / 52.4 | 44.8 / 31.0 | 9.0 / 27.0 |
| Flight fold ×5 | PC | 51.1 / 50.9 | 49.7 / 49.6 | 46.9 / 36.4 | 10.6 / 17.7 |
| Grid fling 6 s | PC | 60.0 / 60.0 | 60.0 / 60.0 | 20.0 / 20.6 | 1.5 / 1.9 |

  - **5b and pre-5b are the same within noise on both machines.** Today's machines are busier than at Phase 4: the Mac was on battery with Docker at ~300% CPU; the PC had Adobe Creative Cloud helpers, Defender, another session's `rustc` and another session's `Negs.exe` (`.negs\idle\fix`). The PC's strip and flights (≈50 fps, against 60 at Phase 4) are as low in the pre-5b build.
  - **The Mac grid fling spike from Phase 4 (24 ms render) did not recur.** Grid render max was 2.4–5.8 ms in five runs, at ~119 fps. A ~70 ms strip hitch appears once per run in both builds; it is older than 5b.
- **Screenshots (real windows):**
  - Mac: `/private/tmp/claude-501/-Users-noahhaasis-Projects-negs/3d47e6be-b5c8-4129-bdd3-5e7c7e85cdc3/scratchpad/mac-export/shots/export-card*.png` (Darkroom, Light Table, Plain dark, batch), `/private/tmp/claude-501/-Users-noahhaasis-Projects-negs/3d47e6be-b5c8-4129-bdd3-5e7c7e85cdc3/scratchpad/mac-settings/shots/*.png` (each Settings page, Look in Light Table and Plain dark, the tiles-off stock menu).
  - PC: `/private/tmp/claude-501/-Users-noahhaasis-Projects-negs/3d47e6be-b5c8-4129-bdd3-5e7c7e85cdc3/scratchpad/pc/shots5b/*.png` (the same, plus `menu-file.png` and `menu-photo-rating.png`, the in-window menu bar with caption buttons).
- Seen once on the PC, not reproduced: a run restored the canvas at 100% (`fitImage: false` in the scratch session). The same tour sequence from a clean session keeps Fit.
- **PC cleanup:** `schtasks negs-gpui-p5`, worktree `negs\worktrees\gpui-p5` and `C:\Users\Noah\.negs\scratch\gpui-p5` deleted after the gate. Noah's main checkout, installed Negs, profiles and photo folders were never touched.

---

## 2026-11-25 — Phase 5a: Print on its Sheet, Crop, Negative pick, Compare, Image Only (`200def15`)

### What landed
- Branch `gpui-p5a-compare-crop-negative` (`b8dfecf3` … `505a9c19`, 7 commits) merged `--no-ff` into local `main`. Changes are under `app/` only (no `crates/`, `vendor/`, `web/`, `windows/`, or any `Cargo.toml`/`Cargo.lock`). Not pushed.
- `app/src/sheet.rs`: the eight viewer sheets (Theme's own, Darkroom, Black, Charcoal, 22% Grey, Middle Grey, Light Grey, Paper), their inks, and the Light Table edge and glow (`light_table`), ported from `windows/src/ui/sheet.rs`. **View › Viewer Sheet** and a "Viewer sheet" section in the title bar's theme popover set `prefs.viewer_sheet`.
- `app/src/print.rs`: the print's geometry and words (caption, film/paper label with carets and the paper lock, trim marks, the pencil or highlighter "open" mark), shaped through one `Painter`.
- `app/src/canvas.rs`: the photograph is drawn as a print on its sheet (centred print band, Light Table shadow, the black-border edge on dark sheets), with the badge above. Clicking the film or paper name opens the stock tray, the lock keeps the paper, a trim mark dragged inwards starts cropping, and a click on the photograph picks the film base while the negative pick is armed. Dropped files and folders open (2 px accent outline while dragging). The empty state shows "Open a photograph", the drop hint and up to five recent files.
- `app/src/crop_view.rs`: crop and straighten. The whole photograph shows unrotated and fitted. Outside the crop is shaded; the overlay has an outline, corner brackets, a thirds grid while dragging and a 9×9 grid while straightening. You can drag the handles, move the crop one axis at a time, and use the straighten dial (7 px per degree, snaps to 0 below 0.4°, double-click resets) or the arrow keys. The crop panel replaces the inspector: aspect list with Custom fields, Orientation, Straighten slider + reset, Flip, Size, Reset Crop, Cancel/Done. Return/Esc apply or cancel unless a field has focus. The turned photograph is resampled bilinearly off the UI thread and cached; replaced textures are dropped.
- `app/src/compare_view.rs`: Compare with another photograph or the original, side by side (or stacked in a tall window) and as a slider. Numbered rings, captions that name what differs, ← → stepping the right-hand photo, S toggles layout, Swap, Fit/100% linked on both sides, Esc leaves (and folds back to the grid if Compare started there). Each side's picture is resampled off-thread at the frame's physical size (and up to 3,072 px for 100%) through the ICC/P3 path. Library menu → "Compare …" (C) and the title bar's Compare pill.
- Image Only (F), the photograph upside down while T is held, View › Actual Size (⌘/Ctrl-1), Photo › Crop (⌘/Ctrl-K), Rotate (⌘/Ctrl-R), Flip. **T no longer cycles themes**: themes moved to View › Theme, because hold-T is the Mac/Windows binding.
- Quitting writes the library's edits (`library_save_now`) before saving the session, and puts back a stock-tray trial.
- `app/src/bench.rs`: `NEGS_BENCH_TOUR=print` walks the print view (sheets, black border on black, crop, straighten, Image Only, upside down, Compare in each layout, 100%) and captures each step. `pc_bench.ps1 -Tour print` runs it on the PC. The JSON's `window` field is now `grid_viewport_and_strip_width`.

### Decisions
- **Ported, not shared:** the Windows app's pure layout code (sheets, print geometry, crop hit-testing, compare frames) is copied into `app/`, not moved into `negs-app`. `windows/src/ui` is retired in Phase 6, so moving it now would mean editing `windows/` for nothing. Until then the code exists in both places.
- **Compare pictures are made per side, off the UI thread**, at the frame's physical size, instead of letting the GPU scale one large texture (that aliases fine detail such as grain). While a new size is being made, the previous picture is stretched in its place. The Windows app's "made ahead" step (preparing the likely next comparison) is left out until stepping is measurably slow.
- **The crop's rotation preview is bilinear** (it's a working view, like the Mac's); the export cuts its pixels in `spektrafilm-io::crop` as before.
- **Zoom is disabled while cropping** (fit only, as on the Mac). In Compare, Z/Fit/100% apply to both photographs.

### Deferred (and why)
- **Tooltips** (crop panel, empty state): GPUI 0.2.2 has no tooltip element. To be added with the Phase 6 accessibility pass.
- **The drop highlight** is an accent outline; the photo doesn't step aside and there's no "drop to open" line. That's cosmetic; Phase 7 macOS polish.
- **Stock menu when tray tiles are off**: needs the Settings toggle (5b).

### Verification Gate
- **Unit tests (`app/`): 78 passed, 0 failed on macOS and on the PC** (20 new: crop handles/dial/ratio parsing/size words, compare frames/actual-size/knob, viewer sheets and inks, print geometry, canvas zoom). 0 warnings in `negs-gpui`. `windows/` still builds on the PC (`wt.cmd … build`).
- **Print tour** (`NEGS_BENCH_TOUR=print`, real windows, scratch profiles; Mac 64 cloned Z 6 NEFs, PC 67 NEFs copied from `Downloads\102ND750` + `Downloads\raws`). 16 shots on each machine: the print in Darkroom, Light Table and Plain dark, black border on the black sheet, crop and straighten (all three themes), Image Only, upside down, Compare side by side (all three themes), slider, slider at 100%, and against the original. Mac: `/private/tmp/claude-501/-Users-noahhaasis-Projects-negs/3d47e6be-b5c8-4129-bdd3-5e7c7e85cdc3/scratchpad/mac/shots/*.png`; PC: `/private/tmp/claude-501/-Users-noahhaasis-Projects-negs/3d47e6be-b5c8-4129-bdd3-5e7c7e85cdc3/scratchpad/pc/shots/*.png`.
- Checked in the shots: colour-managed pictures on both (PC: Dell P3424WEB ICC); the crop shade turns white to grey with detail kept; Compare underlines only what differs ("differ in shutter and aperture"); the Light Table glow and the black-border edge on black.
- **Known, same as the references:** while straightening in a short window, the turned photo's corner can cover the caption. `crop_place` fits the unturned photo exactly as `windows/` and the Mac's `CropEditor.place` do.
- Sidecar round-trip, export byte-identity and the Phase 4 bench re-run are part of the 5b gate (after Export lands).

---

## 2026-11-25 — Phase 4: Library, Filmstrip, Contact-Sheet Grid & SheetFlight (`9c1020ed`)

### What landed
- Branch `gpui-p4-library-grid` (`f82004ad` … `51e93a3a`, 8 commits) merged `--no-ff` into local `main` at `9c1020ed`; 4,808 insertions across 12 files, all under `app/src/`. No changes to `crates/`, `vendor/`, `web/`, root or `app/` `Cargo.toml`/`Cargo.lock`. Not pushed.
- `app/src/thumbs.rs`: background reader threads behind `negs_app::{sheet_rules, thumbnail, thumb_compose, embedded_preview, rendered_preview}`. Developed thumbnails (on-disk store, keyed by the engine stamp) win over the camera's embedded preview; both are converted to the display's ICC/P3 BGRA8 `RenderImage` off the UI thread. Two bounded LRUs (developed, embedded). Evicted and replaced images queue for `window.drop_image` and are flushed every frame; `Host::thumbnail_filed` → `thumbs::filed_from_host` swaps in a freshly developed thumbnail. Display changes bump an epoch so late results for the old display are dropped.
- `app/src/marks_gpui.rs` + `app/src/marks_raster.rs`: `negs-marks` stars, reject cross, pick/selection boxes and pencil underline. GPUI 0.2.2's `paint_path` re-tessellates with lyon on the CPU every frame (~7 µs per polygon), so marks are rasterised once (non-zero fill, 4 sub-scanlines, box-blurred halo) into an LRU sprite cache (96 MB / 1,500 sprites, 3 ms per-frame making budget). Paths stay as the fallback and for boxes that fade during a flight.
- `app/src/strip.rs`: virtualized bottom filmstrip driven by `sheet_rules::StripLayout`: frames dimmed to `STRIP_DIM` except the open one, rejects dimmed, `+J` pairs, hover/press, eased glide to the open frame, wheel scrolling, ⌘/Ctrl-click toggle and Shift range, right-click library menu, `F6` toggle. Before the library loads it lists the folder in natural order.
- `app/src/grid.rs`: virtualized contact sheet driven by `sheet_rules::SheetLayout`:
  - interactive filter sentence ("Showing *all* ⌄ · 72 photographs", × to clear) and the foot (counts + Size slider, keeping the anchor frame in place);
  - rubber-band selection on top of the existing selection, arrows/Home/End/PageUp/PageDown with Shift ranges, ⌘/Ctrl-A, double-click/Enter/E to open, Esc to fold;
  - ratings, flags, Trash with undo through the shared model;
  - an auto-hiding scroll bar and wheel glide.
- `SheetFlight` (Photo ↔ Grid), on both platforms, with Swift's timing: 480 ms, cubic-bezier(.22,.8,.24,1), 16 ms stagger up to 12 steps, boxes from 0.35, a 180 ms fold fade, and a mask reaching 25% past the top. Frames in view fly from the strip to their cells and back (`fadeIntoFrame` keeps flyer + frame at the frame's opacity). Beyond the Mac, the open photo itself flies from the canvas to its cell (`canvas.hide_photo` while it does). System Reduce Motion is honoured (`motion.rs`: macOS `reduceMotion`, Windows `SPI_GETCLIENTAREAANIMATION`, `NEGS_REDUCE_MOTION` override): the grid then shows at once.
- `app/src/library_menu.rs`: the Photo menu's library commands as a darkroom popover: ratings, Pick/Reject/Unflagged, Copy/Paste/Paste from Previous, Duplicate, Reset, Trash / Delete Rejected (with the platform alert via `window.prompt`), Advance After Rating. The grid sentence uses the same popover for its filter menu.
- `main.rs` / `titlebar.rs`: `G` (`NegsApp && !TextInput`), `Esc`, and the title bar's Photo / Grid pills switch views. View menu items for Grid and Filmstrip (`F6`). ⌘/Ctrl ± resize the grid while it covers the view. The canvas and inspector are swapped for placeholders while the grid covers them.
- `app/src/bench.rs`: `NEGS_FRAME_BENCH=<out.json>` drives the app itself after loading:
  - steps: strip fling 4 s, five open/fold flights, grid fling 6 s, filter, themes, and frozen mid-flight frames;
  - output: per-phase frame-interval and render-time percentiles;
  - `NEGS_BENCH_SHOTS=<dir>` writes `<name>.ready` and waits for `<name>.done`, so an outside script can capture the real window.

### Decisions
- **Grid and arrow keys go through the root `on_key_down`**, with a `window.context_stack()` "TextInput" check, not through actions. GPUI runs matched action bindings first, and an action that stops propagation hides the key from `on_key_down`. Single-key bindings stay `NegsApp && !TextInput`.
- **The GPUI shell sets `model.first_render_shown`** once a developed image is on screen, as `windows/` does after its first scene. Launch-gated work (thumbnail pump, prefetch) waited for it forever otherwise.
- **A keyed thumbnail request looks for its developed thumbnail on disk once, even when an embedded preview already covers that size.** Without this, a read that happened before the store knew the engine (no key yet) kept the camera preview for good. The Z 6 test roll's previews are monochrome, so the strip stayed grey. Regression test: `a_key_learnt_after_the_embedded_preview_still_finds_the_developed_one`.
- **Flights carry only frames in view in both the strip and the grid**, as on the Mac, so the bench re-centres the strip on the open photo after its fling.
- **Flight on both platforms (Noah, 2026-11-25, after the Smoothness Proof):** keep `SheetFlight` on macOS and Windows for now. On Win32 the Mac flight "felt slow" at 59 Hz (2026-11-20), so `windows/` fades instead (`sheet_rules::FADE_MS`, 150 ms). The GPUI app holds 60 fps through the flight on the PC; revisit only if it feels slow there.
- **Bench scripts kept:** `app/tools/run_mac.sh`, `winid.swift` and `pc_bench.ps1` drive `NEGS_FRAME_BENCH` + `NEGS_BENCH_SHOTS` on either machine (scratch profile, capture by PID / Console Session 1).

### Verification Gate
- **Unit tests (`app/`): 58 passed, 0 failed on macOS and on the PC.** 23 are new: thumbnails, sprites, strip hit-testing and glide, flight mask, stagger and fold, arrows/Home/End, Shift ranges, rubber band, strip clicks, bench flings. There are 0 warnings in `negs-gpui`; the one in `spektrafilm-io` is upstream engine code, left alone.
- **Frame bench:**
  - Scratch copies: Mac 72 Nikon Z 6 NEFs (APFS clones); PC 67 NEFs copied from `Downloads\102ND750` + `Downloads\raws`.
  - Profiles: scratch `NEGS_PROFILE_DIR` on both.
  - "Interval" is the time between presented frames; "render" is the CPU time of our `render()` + paint.

| Phase | Machine | Frames | fps | Interval median / p95 / p99 / max (ms) | Render p99 / max (ms) | > 25 ms |
|---|---|---|---|---|---|---|
| Strip fling 4 s | PC (60 Hz, 125%) | 240 | **60.0** | 16.69 / 17.21 / 17.37 / 18.31 | 0.89 / 0.90 | **0** |
| Flight open ×5 | PC | 177 | **60.8** | 16.56 / 17.52 / 17.80 / 19.26 | 6.04 / 6.65 | **0** |
| Flight fold ×5 | PC | 150 | **60.5** | 16.41 / 18.61 / 18.94 / 19.86 | 5.68 / 5.70 | **0** |
| Grid fling 6 s | PC | 359 | **60.0** | 16.66 / 16.92 / 17.01 / 17.37 | 0.37 / 0.40 | **0** |
| Strip fling 4 s | Mac (ProMotion, 2×) | 299 | 74.9 | 12.85 / 16.58 / 17.95 / 22.74 | 0.42 / 0.44 | 0 |
| Flight open ×5 | Mac | 230 | 75.7 | 12.99 / 16.66 / 16.78 / 24.41 | 4.16 / 4.22 | 0 |
| Flight fold ×5 | Mac | 188 | 75.6 | 12.88 / 16.58 / 16.76 / 18.61 | 3.83 / 9.66 | 0 |
| Grid fling 6 s | Mac | 585 | 97.7 | 10.46 / 17.52 / 22.65 / 41.77 | 4.80 / 24.21 | 3 |

  - **PC reading of the "p99 ≤ 16.7 ms" target:** the display runs at 60 Hz, so the present interval is one refresh (16.67 ms) plus timestamp jitter. Its p99 is 17.0–18.9 ms. No frame was dropped: the longest is 19.9 ms, against 33 ms for a skipped refresh. Our CPU render p99 is ≤ 6.0 ms. So the literal p99 ≤ 16.7 ms is met by render time, not by the present interval; the interval shows a steady 60 fps.
  - **Mac:** the Mac shows a mixed 8.3 / 16.7 ms cadence on its 120 Hz panel. During the run Docker was using ~300% CPU. An earlier run with no shots gave a grid p99 of 14.2 ms and 1 frame > 25 ms; this run's 3 slow grid frames include a 24 ms render spike, probably sprite or thumbnail uploads as new rows enter. That needs a look in Phase 5 with a quiet machine.
  - **PC cold start:** 66 thumbnails were developed by the engine on first open (80 s until settled). The Mac profile already had them on disk (1 s).
  - The JSON's `window` field is `[grid viewport height, strip width]` in DIPs: Mac `[724, 940]`, PC `[716.8, 925.6]`.
- **Screenshots (real windows; Mac `screencapture -l`, PC `CopyFromScreen` in Console Session 1):** strip, grid, filtered grid, Contact Sheet and Light Table themes, and frozen fold and open frames mid-flight. Both platforms show:
  - developed, colour-managed thumbnails (PC: Dell P3424WEB ICC);
  - stars, crosses, pick boxes, yellow selection boxes and pencil marks;
  - the sentence and foot;
  - flyers staggering out of the strip while the canvas photo shrinks into its cell.
- **PC cleanup:** `schtasks negs-gpui-p4` was deleted after the run, and so were worktree `negs\worktrees\gpui-p4` and `C:\Users\Noah\.negs\scratch\gpui-p4`. Noah's main checkout, installed Negs, profiles and photo folders were never touched.

---

## 2026-11-25 — Phase 3: Inspector, Presets & Stock Preview Tray (`87cb390d`)

### What landed
- Branch `gpui-p3` (`cf361fdc`, `dce8ac8c`, `f308a8a5`, `19263a31`), built across parallel Phase 3a (`gpui-p3a-inspector`) and Phase 3b (`gpui-p3b-presets-tray`) subagents and merged `--no-ff` into local `main` at `87cb390d` (8,609 insertions across 14 files under `app/src/`).
- **Zero engine/core drift:** `crates/`, `vendor/`, `web/`, root `Cargo.toml`/`Cargo.lock`, and `app/Cargo.toml` remain untouched (zero new crates added).
- **Phase 3a — Custom Darkroom Widgets & `InspectorView` UI:**
  - `app/src/widgets/text_input.rs`: Reusable GPUI single-line text editor (`.key_context("TextInput")` suppressing single-key photo shortcuts while typing, UTF-8 cursor/selection, clipboard copy/cut/paste, locale `.`/`,` numeric parsing).
  - `app/src/widgets/slider.rs`: Editorial hairline `InkSlider` with default tick mark, modified dot + `↺` reset button, click-to-type inline numeric edit, double-click reset, and window-level smooth drag (`Track::Drag` → `Track::End` single undo step).
  - `app/src/widgets/controls.rs`: `SectionHeader`, `GroupHeader` (chevron, Newsreader title, collapsed summary, enable switch, strikethrough when skipped), `Toggle`, `Choice` dropdown popover, `MoreRow`, `NoteRow`/`FooterRow`, `Actions`, `BalancePrint`, `FlatField`, `BorderStyles`, `Color` well + preset/hex popover, `LongestSide`, and `Photograph` readout.
  - `app/src/widgets/scopes.rs`: Live `Histogram` (overlapping translucent RGB `PathBuilder` fills + luma outline) and `Waveform` (premultiplied RGBA→BGRA8 `RenderImage` cached with `window.drop_image`), plus draggable height grip clamped to `SCOPE_HEIGHTS` (double-click resets).
  - `app/src/widgets/tone_graph.rs`: Interactive Print Curves graph driven by `negs_app::tone_graph` (`Curve::predicted` live curve during drag, film histogram backdrop, RGB/R/G/B channel pills, horizontal exposure + vertical gamma drag, double-click reset).
  - `app/src/widgets/color_filter.rs`: Tactile colored glass swatch discs (`None` + `model.color_filters`), stops hint, and color-film warning.
  - `app/src/inspector.rs`: Collapsible 340 px right column driven directly by `negs_app::inspector_view::InspectorView` (all 26 `schema::Kind` variants), Pane Header (`Adjustments` · `Presets`, total edit count + `Reset`, `New` / `Import…`), pinned Film & Paper stock strip, `Prep` · `Lab` · `Output` tabs with per-tab edit count badges & clickable Tab Edits reset popover, Output export summary + primary Export button, and full `apply_effects` dispatch (including native light-frame file picker and border color popover).
- **Phase 3b — Presets Sidebar, Film/Paper Stock Preview Tray & Tile Cache:**
  - `app/src/tile_cache.rs`: Color-managed `previews::Bitmap` (BGRA8 already transformed via display ICC/P3) → GPUI `RenderImage` cache holding strong `Arc<Bitmap>` references with frame-epoch eviction and immediate `window.drop_image` VRAM cleanup.
  - `app/src/presets.rs`: Two-column Presets sidebar with live thumbnail previews, Newsreader names & film/paper detail lines, grease-pencil accent ring on applied preset, empty state, pointer drag-to-reorder (`model.move_presets`), external `.json` drop import (`model.import_presets`), inline rename validated live against `presets::name::problem` (auto-started on `New`), right-click context menu (`Apply`, `Rename`, `Export…`, `Show Presets Folder`, `Delete`), and visible-range reporting (`model.set_visible_presets`).
  - `app/src/tray.rs`: 186 px bottom Film & Paper Stock Preview Tray driven by `negs_app::tray_rules` and `Model::tray_*` (`212×118` tiles, `Scan` / `Your negative` card, `Favourites` section + vector star toggle, `"before"` indicator, sideways wheel scroll, 250 ms eased group-index glide, live film search box, paper lock `keep_paper` switch, `Done` button, keyboard `←`/`→`/`Return`/`Esc` single-undo visit semantics, and VRAM cleanup on close).
  - `app/src/canvas.rs`: Wired `model.shown_trial()` stand-in rendering through `color_display::prepare_exact_fit` for instant canvas feedback when trying a stock in the tray.
- **Cross-platform polish caught via Windows screenshot inspection:**
  - Rendered `← →` arrows in the tray foot hint using `theme.ui_family` (the subsetted `Newsreader-Negs-Italic.ttf` lacks arrow glyphs, causing DirectWrite fallback to render wrong glyphs on Windows).
  - Added `.w_full().min_w_0()` to the `Your negative` tray tile card so its italic caption wraps cleanly inside the 212×118 frame, and `.flex_shrink_0()` on note rows.

### Verification Gate
- **Unit tests (`app/`):**
  - **macOS:** **35 passed, 0 failed** (0.07 s, 0 warnings in `negs-gpui`); incremental release rebuild: **5.37 s** (debug test rebuild: **2.17 s**).
  - **Windows PC (`ssh pc`, worktree `gpui-p3`, cleaned up after gate):** **35 passed, 0 failed** (0.11 s, 0 warnings in `negs-gpui`); incremental release rebuild: **9.08 s** (debug test rebuild: **7.13 s**).
- **Visual & Interactive Verification (`docs/card.jpg` scratch copy, live `spektrafilm-native`):**
  - **macOS:** Verified Display P3 color-managed canvas + Inspector + Histogram across `Darkroom`, `Plain`, and `Plain dark` themes.
  - **Windows PC (Console Session 1, Dell P3424WEB ICC active):** Verified live Exposure (EV) slider drag (`0` → `1.6`, modified dot + `↺` reset + `Prep (1)` / `1 edit` badge + live photo & histogram update), Film & Paper Stock Preview Trays (live color-managed tiles, gold favourite stars, crisp `← →` hint, `{RIGHT}` step to `Kodak Portra 400` with `"before"` badge + live re-develop, `{ESC}` single-visit revert to `Kodak Gold 200`), `Waveform` scope, `Lab` foldout groups, `Output` tab (Image Size + Border + export summary), `{F8}` Presets sidebar, and `Darkroom` / `Plain` / `Plain dark` themes.

---

## 2026-11-25 — Phase 2: GPUI Shell + Color-Managed Photo Canvas (`bd8c7662`)

### What landed
- Branch `gpui-p2-shell-canvas` (`aa66b592`, `0fb5424f`, `0d76f195`, `5104c083`), merged to local `main` with `--no-ff` at `bd8c7662`.
- **Zero engine drift:** `Cargo.toml`, `Cargo.lock`, `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, and `web/` remain 100% untouched.
- **Per-display ICC in `crates/negs-app`:** Added ICC v2 `desc` / v4 `mluc` profile name parsing (`icc::profile_name`), `icc::for_display_id`, `icc::describe_display`, and macOS `CGDisplayCopyColorSpace` + `CGColorSpaceCopyICCData` per-display lookup.
- **Standalone `app/` workspace (`negs-gpui` / `Negs`):**
  - Locked `gpui = { version = "0.2.2", features = ["runtime_shaders"] }` (`runtime_shaders` is macOS-only in `gpui 0.2.2` and avoids requiring Xcode's optional MetalToolchain CLI while leaving Windows DX11 untouched).
  - `theme_gpui.rs`: Embeds `Newsreader-Negs.ttf` & `Newsreader-Negs-Italic.ttf`, maps all 6 `ThemeName` palettes (`Darkroom`, `ContactSheet`, `LightTable`, `Daylight`, `Plain`, `PlainDark`), honors `NEGS_THEME`.
  - `host_gpui.rs`: Bridges `negs_app::host::Host` (`post`, `after`, atomic `canvas_scale`, `owner_window_handle`) to GPUI's foreground thread.
  - `color_display.rs`: Fused linear-light exact-fit Lanczos-3 resampling (`exact_fit::coefficients`, multi-threaded via `std::thread::scope`) + unclipped `REC2020_TO_SRGB` + monitor ICC 3×3 matrix & 4096-entry LUT -> BGRA8 `RenderImage`, native-resolution cropped tile path at zoom ≥ 100%, crop/flip/rotate/border support, `window.drop_image` VRAM cleanup, and dynamic display-change refresh (`observe_window_bounds`).
  - `marks_gpui.rs`: Hand-drawn `negs-marks` stars, pick box, and reject cross via GPUI `PathBuilder` + `window.paint_path`.
  - `main.rs`, `titlebar.rs`, `canvas.rs`, `status.rs`: Darkroom titlebar, status bar with live ICC label, smooth zoom/pan, Before/After toggle (`\`) & draggable split divider (`Shift+\`), native Open/Save dialogs, drag-and-drop, keyboard shortcuts (`Cmd/Ctrl+O/S/Z/Shift+Z/+/-/0/Q`, `\`, `Shift+\`, `Z`, `0–5`, `P/X/U`, `T`), and `NEGS_STARTUP_REPORT` instrumentation compatible with `shot.ps1` and `startup.ps1`.

### Verification Gate & Baselines
- **Unit tests:**
  - macOS: `app/` **8 passed, 0 failed** (BGRA order, Display P3 red desaturation, Rec. 2020 wide-gamut on P3, Lanczos 3px stripe sharpness + flat grey, crop/orientation, 6 themes, env override, canvas scale); `crates/negs-app` ICC tests **4 passed, 0 failed**.
  - Windows PC: `app/` **8 passed, 0 failed**; `crates/negs-app` ICC tests **4 passed, 0 failed**.
- **Build & incremental edit speed:**
  - Mac release binary (`no LTO`): **18 MB** unstripped; 1-line incremental UI release rebuild: **2.11 s** (target `<= 5 s`).
  - Windows release binary (`no LTO`): **25.2 MB** unstripped (0 errors, 0 warnings).
- **Live Color Management & Startup Baselines (`docs/card.jpg`, live `spektrafilm-native`):**
  - **macOS (Retina Display P3):** Detected `display_key=1 label="Display" active_transform=true`; warm `gpu_ready` **227.5 ms**, `first_window_composited` **254.8 ms**, `first_image_on_screen` **1238.9 ms** (fresh develop; `0.25 s` engine render). Note: precompiled Metal shaders (dropping `runtime_shaders` once MetalToolchain is installed) can reduce Mac first-frame further toward the ~182 ms spike baseline.
  - **Windows PC (`startup.ps1 -Runs 5`, Dell P3424WEB wide-gamut monitor):** Detected `ICC: DELL P3424WEB Color Profile, D6500` (active matrix+LUT transform); **5-run medians:** `gpu_ready` **447.1 ms**, `inspector_shown` **447.2 ms**, `first_window_composited` **466.2 ms** (beats `<= 550 ms` target by 84 ms!), `first_image_on_screen` **1189.9 ms** (fresh develop; `0.18–0.19 s` engine render).
  - Visually verified side-by-side screenshots on Mac (`Darkroom` P3) and Windows PC (`Darkroom`, `marks`, `Contact sheet`, `Plain`): matched color saturation across Mac P3 & Windows wide-gamut Dell ICC, razor-sharp 1:1 Lanczos film grain, crisp Newsreader typography, and vector `negs-marks` overlay.

---

## 2026-11-25 — Phase 1: Extract Pure-Rust `crates/negs-app` & `crates/negs-marks` (`85281bf8`)

### What landed
- Branch `gpui-p1-negs-app` (`05ca56be`, `59bce147`, `444ef415`, `976953c3`), merged to local `main` with `--no-ff` at `85281bf8`.
- **Standalone workspaces (zero engine lockfile drift):** Root `Cargo.toml`, root `Cargo.lock`, `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, and `web/` are 100% untouched.
- **`crates/negs-marks/`**: Promoted from `windows/marks/` (zero external deps).
- **`crates/negs-app/`**: Extracted all 32 non-UI modules (`batch`, `border`, `brightness`, `compare`, `crop`, `edited_store`, `edits`, `embedded_preview`, `engine`, `exact_fit`, `film_cache_release`, `help`, `histogram`, `icc`, `inspector_view/`, `json`, `lightroom`, `match_cache`, `model/`, `negative`, `photo_facts`, `picture`, `presets`, `previews`, `rendered_preview`, `session`, `sheet_rules`, `state`, `theme`, `thumb_compose`, `thumbnail`, `tone_graph`, `trace`, `tray_rules`) behind two thin boundaries:
  - `host.rs` (`Host` trait: `post`, `after`, `thumbnail_filed`, `batch_refresh`, `export_finished`, `note_recent_document`, `canvas_scale`, `decimal_separator`, `background_enter`/`leave`, plus built-in headless `TestHost`).
  - `platform/{mod,windows,unix}.rs` (atomic file replace, `move_no_replace`, hidden flags, NFC/diacritic folding via Win32 NLS or `icu_normalizer`/`icu_properties`, settings/cache dirs, monotonic ticks, job/process lifecycle, exclusive lock, `RecycleBin`, monitor ICC via `GetICMProfileW` / `CGDisplayCopyColorSpace`+`CGColorSpaceCopyICCData`, `FolderWatch`).
  - `picture.rs`: Decoupled from `wgpu::GpuImage` via type-erased `gpu_slot` + `register_uploader`, keeping CPU pixels available for GPUI while keeping zero-copy GPU upload on Win32.
- **Thin `windows/` shell:** `windows/Cargo.toml` now depends on `negs-app` + `negs-marks`, registers `Win32Host` + `wgpu` uploader at startup, and drops `windows/library-map/windows-stub`.

### Decisions Locked for Phase 2
- **GPUI dependency:** Locked to **`gpui = "0.2.2"`** (crates.io; proven on `rustc 1.93.1` across macOS Metal + Windows DX11, already cached locally; zero toolchain bump).
- **Windows linker:** `lld-link.exe` is installed on the Windows PC (`Swift\Toolchains\6.4.2+Asserts\usr\bin\lld-link.exe` on PATH) — will benchmark incremental Windows GPUI rebuilds with `lld-link` without hardcoding a mandatory `.cargo/config.toml` that could break CI runners lacking `lld`.

### Verification Gate
- **macOS (local):**
  - `crates/negs-marks`: **16 passed, 0 failed**.
  - `crates/negs-app` (`NEGS_REQUIRE_HELPER=1` against live `spektrafilm-native`): **267 passed, 0 failed, 2 ignored** (including all 54 `lifecycle_tests` and all `inspector_view` tests).
  - `windows/` MSVC cross-check (`--release` & `--tests`): **0 errors**; standalone `crates/negs-app` MSVC cross-check (`--tests`): **0 errors**.
  - `windows/library-map` (25), `sheet-rules` (20), `tray-rules` (19): **64 passed, 0 failed**.
- **Windows PC (`ssh pc`, worktree `gpui-p1`, removed after verification):**
  - `crates/negs-marks`: **16 passed, 0 failed**.
  - `crates/negs-app`: **268 passed, 0 failed, 2 ignored**.
  - `windows/` thin shell: **37 passed, 0 failed**.
  - `windows/library-map` (25), `sheet-rules` (20), `tray-rules` (19): **64 passed, 0 failed**.
  - **Total Windows PC:** **385 passed, 0 failed, 2 ignored**.

---

## 2026-11-25 — Phase 0: Clean Odin Revert (`5ef625e4`)

### What landed
- Branch `gpui-p0-revert-odin` (`775eb3a6`), merged to local `main` with `--no-ff` at `5ef625e4`.
- Deleted `core/` (~92.7k lines removed across Odin sources, C99 header, Rust FFI wrappers, and differential harnesses).
- Restored pure-Rust `windows/` crates and modules from `release-0.20` (`021ce54b`): `Cargo.toml`, `Cargo.lock`, `library-map/`, `sheet-rules/`, `tray-rules/`, `ui/theme.rs`, `json.rs`, `platform.rs`, and all 19 Odin-wrapped non-UI modules (`batch`, `border`, `compare`, `crop`, `embedded_preview`, `exact_fit`, `help`, `histogram`, `match_cache`, `negative`, `photo_facts`, `presets`, `rendered_preview`, `session`, `sheet_rules`, `state`, `thumb_compose`, `tone_graph`, `tray_rules`, `model/compare`).
- Preserved **`885e8fb4`** (`windows/src/inspector_view/`, `windows/src/edits.rs` taking `&InspectorFacts`, `windows/src/model.rs`, `windows/src/ui/inspector.rs`, `windows/src/ui/tone_graph.rs`, `windows/src/session.rs` `revealed`/`expanded` cleanup) plus `v0.19`/`v0.20` `cine_print` & `listed_papers` updates.
- Restored pure-Swift `macos/` from pre-Odin commits (`434461c5^1` for `macos/Package.swift`, `009bc641^1` for `AppTheme.swift`, `HandStar.swift`, `PickBox.swift`, `RejectCross.swift`, `Theme.swift`), deleted `CoreMarks.swift`, `CNegsCore/`, and `CoreSmokeTests.swift`, and removed Odin build lines from `scripts/package_macos_app.sh`.
- Cleaned Odin steps from `.github/workflows/release.yml` while preserving **`8154a6ac`** (`deploy-feed` + `feed_run_id` fix).
- Removed Odin notes from `CLAUDE.md` and `docs/windows-app.md`; added superseded banners to `docs/shared-core-{plan,progress,coordinator}.md`.

### Verification Gate
- **macOS (local):**
  - `swift test --package-path macos --scratch-path target/swift-native`: **487 passed, 0 failed** (20 skipped — local RAW tests).
  - `cargo test --release --workspace --exclude spektrafilm-wasm`: **524 passed, 0 failed** (14 ignored).
  - `cargo check --release --locked --target x86_64-pc-windows-msvc --manifest-path windows/Cargo.toml --target-dir target/wincheck`: **clean (0 errors)**.
  - Host rules crates (`negs-marks` 16, `library-map` 25, `sheet-rules` 20, `tray-rules` 19): **80 passed, 0 failed**.
- **Windows PC (`ssh pc`, worktree `gpui-p0`, removed after verification):**
  - `cargo test --locked --manifest-path windows\Cargo.toml`: **304 passed, 0 failed** (2 ignored), including all 55 `inspector_view` tests and `lifecycle_tests`.
  - `negs-marks` (16), `library-map` (25), `sheet-rules` (20), `tray-rules` (19): **80 passed, 0 failed**.
  - **Total Windows PC:** **384 passed, 0 failed** (2 ignored).
