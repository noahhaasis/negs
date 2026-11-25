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
| 4 | Library, Filmstrip, Grid & Flight | **Ready** | — | `negs-library` + folder watcher, filmstrip with `negs-marks`, contact-sheet grid, 60 fps `SheetFlight` transition. |
| 5 | Compare, Crop, Negative, Export, Batch & Settings | Pending | — | Full feature parity with `windows/` and `macos/`. |
| 6 | Windows Cutover & Packaging | Pending | — | ABBA perf/screenshot report vs Win32 `Negs.exe`, Go/No-Go gate, installer + CI cutover. |
| 7 | Linux Adoption & macOS Polish | Pending | — | 7a Linux (T2 parity, retire GTK4), 7b macOS (HIG polish, Go/No-Go before retiring SwiftUI). |

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
