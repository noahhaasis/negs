# Cross-Platform GPUI App & Pure-Rust Shared Core: Architecture and Migration Plan

Prepared 2026-11-25 against `main` at `8cff4c21` (`v0.20.0`). Supersedes [shared-core-plan.md](shared-core-plan.md) (Odin shared core).

## Why this pivot (decision & spike evidence)

1. **The Odin shared-core port solved the wrong bottleneck:** By mid-Phase 4, `core/` had grown to 79k LOC (38.6k Odin + 2.1k C99 header + 13.6k Rust FFI wrappers + 24.7k diff harnesses), still leaving ~78k LOC of UI duplicated across 3 native toolkits (`windows/src/ui/` 29.1k Win32+D2D, `macos/Sources/` ~18k SwiftUI, `linux/src/` 21.6k pre-T2 GTK4) and adding a 5-layer FFI tax to every feature change.
2. **Windows runtime performance regressions:** Switching `windows/` to the macOS darkroom look + library stacked four rendering pipelines inside one window: Vulkan (`wgpu` canvas), DX11/D2D/DirectComposition (separate swap chains for strip, grid, tray), CPU-software Direct2D (`WARP` + CPU readback to GDI `DIBSection` + `BitBlt` for every inspector/titlebar/control HWND), and the engine's Vulkan helper. That caused 15–50 ms swap-chain `Present` stalls, 18–105 ms `RedrawWindow` cascades, 40–50 ms WARP stalls under CPU load, and forced removing the grid flight animation (`e1a0ad67`).
3. **GPUI vs. Slint spike results (2026-11-24/25, 7-run medians on Negs-shaped prototype with `docs/card.jpg`):**

| Metric | Current macOS *(SwiftUI)* | Current Windows *(Win32+D2D)* | **GPUI `0.2.2` macOS** *(Metal)* | **GPUI `0.2.2` Windows** *(DX11)* | Slint `1.18.1` macOS *(FemtoVG GL / wgpu)* |
|---|---:|---:|---:|---:|---:|
| First frame presented (bare UI) | ~265–508 ms | 81–87 ms *(window)* / 188–195 ms *(inspector)* | **182.0 ms** | **480–652 ms** *(window 474–609 ms)* | 358.4 / 401.2 ms |
| `docs/card.jpg` decoded + presented | ~265 ms *(cached)* | ~918 ms *(full dev)* | **277.0 ms** *(+11 ms upload)* | **557.9 ms** *(+10 ms upload)* | 554.6 / 414.9 ms |
| Peak RAM (bare / with photo) | ~374 MiB *(full app)* | — | **66.8 / 115.7 MiB** | **46.6 / 81.0 MB** | 98.3 / 179.0 MiB |
| Release binary size (unstripped) | — | ~21.8 MB | **8.28 MB** | **11.77 MB** | 12.37 / 17.43 MB |
| 1-line UI edit (release, no LTO) | 56 s | 4.5–4.6 s | **2.46 s** *(1.17 s debug)* | **14.6 s** *(MSVC link)* | 5.51 s *(`.slint`)* |

- **Slint** was ruled out: ~2× slower startup on macOS, slower photo upload, slower `.slint` incremental rebuilds, and awkward for custom canvas/marks/crop/flight UI.
- **GPUI** beats current macOS SwiftUI startup (182 ms vs ~265–500 ms), cuts Mac UI rebuilds by ~20× (1.2–2.5 s vs 56 s), eliminates the Windows multi-swap-chain + software-WARP runtime bottlenecks with a single DX11/DirectComposition swap chain, and collapses ~188k LOC across 4 languages/3 UI toolkits down to **~38–45k LOC of pure Rust** (1 shared core + 1 UI for macOS, Windows, and Linux).
- **Accepted trade-off:** Windows cold first-frame moves from ~85 ms (Win32) / ~188 ms (inspector) to ~480–600 ms (where DX11 + DirectWrite initialize synchronously before frame 1, matching current Windows `gpu_ready` at ~456–474 ms).

## Goals & Non-goals

### Goals
1. **Cleanly revert the Odin port** on `main` without losing any release commits (`v0.18.0`–`v0.20.0`), film stocks, CI fixes, or the pure-Rust `windows/src/inspector_view/`.
2. **Extract one pure-Rust shared app core (`crates/negs-app` + `crates/negs-library` + `crates/negs-marks`)** (~24–26k LOC, ~95% already written in `windows/src/`) that compiles cleanly on macOS, Windows, and Linux with zero Win32/AppKit/GTK imports.
3. **Build ONE cross-platform GPUI desktop app (`app/`)** on top of the shared core, developed daily on macOS and verified on Windows PC at every phase gate (Linux adoption follows once Windows ships).
4. **Ship order:** **Windows first** (immediately replaces the lagging Win32/WARP/multi-swap-chain UI), **Linux second** (instant T2 parity: library, grid, tray, compare, marks, themes), **macOS third** (once GPUI matches or beats SwiftUI polish).
5. **Uncompromised photographic quality:** Zero export changes; razor-sharp 1:1 Lanczos display scaling; accurate wide-gamut / monitor ICC / macOS Display P3 color management.

### Non-goals (hard boundary)
- **Do not touch the film engine:** `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, and `web/` stay untouched. All 496 golden renders and exports stay byte-identical. The GPUI app talks to `spektrafilm-native`, `spektrafilm`, and `spektrafilm-f64` over the existing helper/CLI protocol.
- **Do not rewrite working pure-Rust logic:** Reuse `crates/negs-library`, `windows/marks`, `windows/src/inspector_view`, and `windows/src/{model,engine,state,edits,presets,session,batch,sheet_rules,tray_rules,crop,border,compare,exact_fit,thumb_compose,histogram,negative,embedded_preview,photo_facts,tone_graph,help,match_cache,rendered_preview}.rs` as-is (decoupling only their tiny Win32 touchpoints).

---

## Phase 0 Audit: Interleaved Commits on `main` vs. `release-0.20`

`main` (`8cff4c21`) has both Odin commits and real product commits interleaved since `a9d64f6`. **Do not `git reset`.** Make a clean forward revert commit on `main`.

### Non-Odin work to PRESERVE (all already on `main`)
1. **Already on `release-0.20` (`021ce54b`, tag `v0.20.0`) AND `main`:**
   - Windows fixes: `211a3e25` (`win-film-fringe`), `7e443cff` (`win-plain-strike`), `6a9b59f2` (`win-layout-shifts`).
   - Film stocks & releases: `ba8aee1a` (`fuji-slides`), `7c4b0a1b` (`package-static`), `v0.18.0` (`61072f04`), `v0.19.0` (`08721fa1`/`a1e19546` — Eterna-CP prints, Reala Ace, Portra 400NC/400VC, Linux Show Negs-built films, web i18n test), `v0.20.0` (`12cbed9c`/`021ce54b` — Kodak EXR 100T 5248, Agfa Portrait XPS 160, Konica Minolta Professional 160, macOS `TrayTests` fix).
2. **On `main` ONLY (NOT on `release-0.20`) — MUST KEEP:**
   - **`8154a6ac`** — `.github/workflows/release.yml`: `deploy-feed` fix when a tag reuses `main`'s artifacts.
   - **`885e8fb4`** (`core-p4i-inspector`) — `windows/src/inspector_view/` (2,671 LOC pure-Rust platform-free Inspector ViewModel + 55 tests) + the corresponding `windows/src/edits.rs`, `windows/src/model.rs`, `windows/src/session.rs`, `windows/src/ui/inspector.rs`, and `windows/src/ui/tone_graph.rs` changes.

### Exact Odin Revert Checklist (Phase 0)
Starting from `main` (`8cff4c21`):
1. **Restore pure-Rust `windows/` from `release-0.20` (`021ce54b`):**
   - `windows/Cargo.toml`, `windows/Cargo.lock`, `windows/library-map/`, `windows/sheet-rules/`, `windows/tray-rules/`
   - `windows/src/ui/theme.rs`
   - The 18 Odin-wrapped modules: `batch.rs`, `border.rs`, `compare.rs`, `crop.rs`, `embedded_preview.rs`, `exact_fit.rs`, `help.rs`, `histogram.rs`, `match_cache.rs`, `negative.rs`, `photo_facts.rs`, `presets.rs`, `rendered_preview.rs`, `session.rs` *(re-apply `885e8fb4`'s 5-line `Preferences::revealed` cleanup if needed)*, `sheet_rules.rs`, `state.rs`, `thumb_compose.rs`, `tone_graph.rs`, `tray_rules.rs`, `model/compare.rs`.
   - **Restore `windows/src/edits.rs` from `885e8fb4`** (NOT `release-0.20` — `885e8fb4` has the pure-Rust `&InspectorFacts` version that `inspector_view` uses, before Phase 4a `daac1331` moved it to Odin).
   - **Keep** `windows/src/inspector_view/`, `windows/src/ui/inspector.rs`, `windows/src/ui/tone_graph.rs`, `windows/src/model.rs`, `windows/src/main.rs` from `main` (cleaning up any Phase 4a `negs_core_rules` imports in `model.rs` / `inspector_view/facts.rs` / `platform.rs` / `json.rs`).
2. **Restore pure-Swift `macos/` from `009bc641^1` (pre-Phase 1; verified zero post-Odin edits to these 6 Swift files):**
   - `macos/Sources/NegsApp/{AppTheme,HandStar,PickBox,RejectCross,Theme}.swift`
   - Delete `macos/Sources/NegsApp/CoreMarks.swift`, `macos/Sources/CNegsCore/`, `macos/Tests/NegsTests/CoreSmokeTests.swift`
   - Restore `macos/Package.swift` (drop `CNegsCore` target) and remove the `core/build.py lib` lines from `scripts/package_macos_app.sh` (keeping any `v0.18.0`+ changes!).
3. **Remove `core/` & Odin CI/docs wiring:**
   - Delete `core/` (~79k LOC).
   - Remove Odin install / `core/build.py` steps from `.github/workflows/release.yml` while **keeping `8154a6ac`**.
   - Remove the Odin paragraph from `CLAUDE.md` and `docs/windows-app.md` (add a 1-line superseded banner to `docs/shared-core-*.md`, or archive them).
4. **Verify Phase 0 gate:**
   - Mac: `swift test --package-path macos --scratch-path target/swift-native` green; Windows cross-check `cargo check --release --locked --target x86_64-pc-windows-msvc --manifest-path windows/Cargo.toml --target-dir target/wincheck` green; `cargo test --release --workspace --exclude spektrafilm-wasm` green.
   - Windows PC: `cargo test --locked --manifest-path windows/Cargo.toml` + any-OS crates (`library-map`, `sheet-rules`, `tray-rules`) green.

---

## Target Architecture

```
crates/
  spektrafilm-*          # UNTOUCHED Rust film engine (~68.6k LOC)
  negs-libraw            # UNTOUCHED
  negs-library/          # Existing pure-Rust library/sidecars/undo (5.4k LOC)
  negs-marks/            # Moved/shared from windows/marks/ (1.2k LOC)
  negs-app/              # NEW: Pure-Rust shared app core (~18–20k LOC, 0 UI toolkit deps)
    src/
      platform/          # Tiny OS adapter (~800–1,200 LOC behind Platform trait / #[cfg]):
                         #   - trash_item / restore_from_trash (NSFileManager / SHFileOperation / freedesktop)
                         #   - atomic_replace (POSIX rename / Win32 MoveFileExW)
                         #   - monitor_icc_profile (CGDisplayCopyColorSpace / GetICMProfileW / colord)
                         #   - folder_watch (FSEvents / FindFirstChangeNotificationW / inotify via notify)
                         #   - normalize_nfc & case_fold_contains (OS or unicode-normalization)
                         #   - open_in_lightroom (macOS / Windows)
                         #   - profile_dir / cache_dir / executable_dir
      theme.rs           # 6 themes (Darkroom, Plain, Plain dark, etc.) + WCAG contrast helpers
      rules/             # sheet_rules, tray_rules, compare, crop, border, exact_fit, thumb_compose,
                         # histogram, negative, embedded_preview, photo_facts, tone_graph, help
      state/             # state, edits, json, presets, session, batch, match_cache, rendered_preview
      inspector_view/    # Platform-free 195-row Inspector ViewModel (from windows/src/inspector_view/)
      engine.rs          # EngineClient protocol (spawns/talks to spektrafilm-native, spektrafilm, spektrafilm-f64)
      model/             # EditorModel frame pipeline + compare/library/negative/tray/batch/lightroom
app/                     # NEW: Cross-platform GPUI desktop app (~12–18k LOC, its own workspace)
  Cargo.toml             # Depends on gpui, negs-app, negs-library, negs-marks, spektrafilm-io
  src/
    main.rs              # Window bootstrap, menus, CLI/open-file args, startup instrumentation
    color_display.rs     # Exact-fit Lanczos + Rec.2020/sRGB -> monitor ICC / macOS Display P3 -> BGRA8 RenderImage
    theme_gpui.rs        # Maps negs_app::theme::Tokens -> GPUI Hsla + Newsreader/UI font setup
    widgets/             # InkSlider, Toggle, Choice, SectionHeader, ToneGraphCanvas, ColorFilterDiscs, MarksOverlay
    titlebar.rs          # Custom title bar + filename + view/theme/before-after controls
    canvas.rs            # Photo canvas: zoom/pan, exact-fit sharp view, before/after split, crop handles & angle dial
    inspector.rs         # Renders negs_app::inspector_view::InspectorView directly + dispatches Effects
    presets.rs           # Presets sidebar (drag-reorder, inline rename, drop import, context menu)
    tray.rs              # Film & paper stock preview tray (grouped, favourites, search, paper lock)
    strip.rs             # Bottom filmstrip with marks, ratings, selection, folder-watch updates
    grid.rs              # Contact-sheet grid + SheetFlight zoom/fade transition + band selection + filter sentence
    compare.rs           # Side-by-side & split-slider compare view
    dialogs.rs           # Settings, Export (format/space/quality/bit-depth), Batch Export progress, Alerts, Viewer Sheet
```

### Photographic Quality & GPUI Rendering Design (non-negotiable!)

Every photography nerd will judge Negs on **color accuracy** and **sharpness**. Stock GPUI has two traps that `app/src/color_display.rs` MUST solve from Phase 2:

1. **Downscaling sharpness (anti-moiré / crisp film grain):**
   - Stock GPUI `img()` samples its sprite atlas with **plain bilinear filtering and no mipmaps**. Feeding a 4000×3000 photo directly into GPUI causes shimmering film grain and aliasing when zoomed out.
   - **Solution:** Reuse `negs_app::rules::exact_fit` (Lanczos) to resample the displayed viewport/fit region to **exact 1:1 physical screen pixels** (`viewport_dips * scale_factor`) before uploading to GPUI's `RenderImage`. At zoom ≥ 100%, upload the visible crop region (or full image at 1:1 / crisp pixel sampling). GPUI then draws 1:1 with zero bilinear degradation.
2. **Color management (wide gamut & calibrated monitors):**
   - Today's engine outputs an 8-bit **sRGB** or **Rec. 2020 (sRGB transfer)** preview proxy.
   - **Windows & Linux:** GPUI's DX11 / Blade / Vulkan swapchain is 8-bit `BGRA8Unorm` — identical to Negs' current Windows `wgpu` swapchain. Apply the existing monitor ICC transform (`windows/src/icc.rs` 3×3 matrix + 4096-entry LUT, with `Rec.2020 -> linear sRGB` unclipped first for wide-gamut proxies) during the exact-fit / BGRA conversion pass (CPU SIMD or tiny helper pass: ~1–2 ms on a 1500×1000 viewport).
   - **macOS (CRITICAL):** Stock `gpui 0.2.2` (`metal_renderer.rs:144`) creates `CAMetalLayer` with `BGRA8Unorm` **without calling `setColorspace`**, so macOS passes raw pixels directly to the display (Display P3 on MacBooks / Studio Display). **Never send raw sRGB pixels to an untagged P3 Metal layer** (they will look oversaturated!). Either:
     - **Option A (recommended — unified across all 3 OSes):** Query the window's active `NSScreen.colorSpace` ICC data on macOS (same `monitor_icc_profile` hook as Windows/Linux) and run the exact same display color transform into `RenderImage` (re-running when the window moves between displays!).
     - **Option B (bonus check in Phase 2):** Use GPUI's built-in `surface()` element on macOS with a color-tagged `CVPixelBuffer`/`IOSurface` (`kCGColorSpaceITUR_2020_sRGBGamma` / `kCGColorSpaceSRGB`), or patch `CAMetalLayer.setColorspace` if using a pinned GPUI fork.
3. **Future 10-bit / HDR path:** Keep `color_display.rs` behind a clean trait/module boundary so if Negs later adds 10-bit (`RGB10A2Unorm`) or HDR/EDR (`RGBA16Float`), only that module (plus a GPUI swapchain patch or child native surface) changes.

### GPUI Dependency Decision (lock in Phase 1)
Evaluate and pin **one** GPUI source before writing Phase 2 UI code:
- **Candidate A:** `gpui = "0.2.2"` on crates.io (proven in our spike on `rustc 1.93.1`, works out-of-the-box on both macOS Metal and Windows DX11; zero toolchain upgrade needed). Check Linux Wayland/X11 and `gpui-component` compatibility if desired.
- **Candidate B:** Pin a specific commit of `zed-industries/zed` (`gpui` + `gpui_platform` with `gpui_wgpu` on Linux) or `gpui-ce` (note `gpui-ce 0.3.0` requires `rustc 1.95`).
- **Rule:** Prefer Candidate A (`gpui 0.2.2`) unless a concrete blocker (e.g., missing IME, text input, file-dialog, or Linux Wayland bug) requires pinning upstream Zed. Never chase floating `main` of Zed.
- **Fonts:** Embed `macos/Fonts/Newsreader-Negs.ttf` and `Newsreader-Negs-Italic.ttf` via `cx.text_system().add_fonts(...)` at startup so Negs' signature typography is identical on macOS, Windows, and Linux.
- **Windows link speed tip:** Configure `.cargo/config.toml` in `app/` to test `lld-link` on Windows if installed (can cut Windows 1-line incremental rebuilds from ~14.6 s down toward ~4–6 s).

---

## Staged Migration Phases

| # | Phase | Scope | Verification Gate |
|---|---|---|---|
| **0** | **Clean Odin Revert** | Forward-revert Odin on `main` per the exact checklist above; preserve `v0.18`–`v0.20`, all film stocks, `8154a6ac` CI fix, and `windows/src/inspector_view/`. | Mac: `swift test`, Rust workspace tests, Windows `cargo check`. PC: `windows/` `cargo test` + any-OS crates green. |
| **1** | **Extract Pure-Rust `crates/negs-app`** | Move/promote `windows/marks` → `crates/negs-marks` and non-UI modules of `windows/src/` (`rules`, `state`, `inspector_view`, `engine`, `model`) into `crates/negs-app` behind a tiny `Platform` trait (`macos`/`windows`/`linux` impls). Keep `windows/` compiling against `crates/negs-app` as a temporary thin shell so nothing breaks during the port. | `cargo test --release -p negs-app -p negs-library -p negs-marks` passes natively on **both** macOS and Windows PC (including `lifecycle_tests` and `inspector_view` 55 tests!). `windows/` still builds and passes all tests on PC. |
| **2** | **GPUI Shell + Color-Managed Photo Canvas** | Create `app/` workspace with GPUI; embed Newsreader fonts + all 6 themes; wire `EngineClient` + `EditorModel`; implement titlebar, status line, keyboard shortcuts, native open/save dialogs, drag-drop, and `color_display.rs` (exact-fit Lanczos + monitor ICC / macOS Display P3 transform + zoom/pan + before/after toggle & split + vector marks overlay). | Opens RAW + JPEG via real `spektrafilm-native` on **Mac and Windows PC**. Verify color accuracy (Mac P3 screenshot vs SwiftUI Negs; Windows wide-gamut ICC vs current Win32 Negs) and 1:1 sharpness at fit & 100% zoom. Record Mac & Windows startup baselines. |
| **3** | **Inspector, Presets & Stock Tray** | Build custom widgets (`InkSlider` with click-to-type numeric edit + double-click reset, toggles, choices, foldout groups, `ToneGraphCanvas`, `ColorFilterDiscs`, histogram/waveform scope); drive the entire Prep/Lab/Output inspector directly from `negs_app::inspector_view::InspectorView`; build Presets sidebar (drag-reorder, inline rename, drop import) and Film/Paper Stock Preview Tray (grouped, favourites, search, paper lock). | Every control in `inspector_view` interactive on Mac + PC; slider drag ≥ 60 fps with zero UI thread stalls while engine develops; tab switches instant (<16 ms); side-by-side screenshots vs current Windows & macOS apps across themes (Darkroom, Plain, Plain dark). |
| **4** | **Library, Filmstrip, Contact-Sheet Grid & Flight** | Wire `negs-library` + folder watcher + thumbnail pipeline; build bottom filmstrip (hand-drawn `negs-marks` stars/cross/pick-box, ratings, selection) and full-window contact-sheet grid (packed rows from `sheet_rules`, filter sentence, band selection, size slider, **smooth 60 fps `SheetFlight` transition** on both Mac and Windows!). | Open 60+ RAW folder on Mac & Windows PC: smooth 60 fps grid scroll and flight animation (measure p50/p95/p99 frame times — confirm Windows regression is completely gone!), ratings/flags/delete/undo match `LibraryStateTests` invariants. |
| **5** | **Compare, Crop, Negative, Export, Batch & Settings** | Implement Compare mode (side-by-side + split slider + Esc exit), Crop mode (handles, aspect ratios, straightening dial, dimmed outside), Negative scan/flat-field UI, Export dialog, Batch Export progress, Open in Lightroom, Settings window, Viewer Sheet, and Alerts. | Full feature-parity checklist vs `docs/windows-ui-features.md` and `macos/` passes on both Mac and Windows PC; Mac↔Windows sidecar round-trip byte-identical. |
| **6** | **Windows Cutover & Packaging** | Benchmark GPUI `Negs.exe` vs old Win32 `Negs.exe` on Windows PC (startup ABBA, grid scroll & flight fps, tab-switch latency, slider-drag fps, peak RAM, binary size); wire Windows installer (`installer.iss`, manifest, icon, WinSparkle/updates, CI release job); retire `windows/src/ui/` once Noah approves the gate! | Noah's Go/No-Go gate on Windows PC (real feel test + side-by-side screenshots + perf report). |
| **7** | **Linux Adoption & macOS Polish** | **7a Linux:** Build & verify `app/` on Linux (Wayland + X11, colord/ICC, freedesktop Trash, Flatpak/AppImage/Arch packaging) — instant T2 parity, retiring `linux/src/` GTK4. **7b macOS:** Polish macOS HIG details (native menu bar, Finder open-with, trackpad pinch/momentum zoom, Sparkle updates, universal binary + DMG); compare against SwiftUI `Negs.app` and retire `macos/` only when Noah agrees it matches/beats SwiftUI. | Linux & macOS package + real-device verification gates. |

---

## Performance & Quality Targets (measured at Phases 2, 4, and 6)

| Metric | Target |
|---|---|
| **macOS startup (first frame / photo visible)** | ≤ 250 ms first frame / ≤ 350 ms cached photo *(current SwiftUI: ~265–500 ms)* |
| **Windows startup (first frame / photo visible)** | ≤ 550 ms first frame *(spike baseline: ~480–600 ms)* — investigate shader pre-compilation / async font warm if > 600 ms |
| **Windows runtime smoothness** *(fixing user regressions)* | **60 fps sustained** (p99 frame ≤ 16.7 ms) on grid scroll, `SheetFlight`, inspector scroll, tab switch, and window resize — even while thumbnails/engine develop |
| **Slider drag responsiveness** | Main UI loop p99 busy ≤ 8 ms during 4 s continuous drag on both Mac and Windows |
| **Incremental UI rebuild (1-line edit)** | Mac: ≤ 3 s debug / ≤ 5 s release; Windows: ≤ 8 s with `lld-link` |
| **Color & sharpness fidelity** | Byte-identical exports (496 golden renders); verified Display P3 (Mac) and monitor ICC (Windows) preview match; 1:1 Lanczos sharp display with zero bilinear moiré |

Execution handoff: [gpui-app-coordinator.md](gpui-app-coordinator.md).
