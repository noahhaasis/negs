# Cross-Platform GPUI App & Pure-Rust Shared Core: Progress Log

Newest first. Live source of truth for every completed phase, merge SHA, verification result, architectural decision, and performance measurement.

See [gpui-app-plan.md](gpui-app-plan.md) and [gpui-app-coordinator.md](gpui-app-coordinator.md).

## Phase Status

| # | Phase | State | Merge SHA | Summary |
|---|---|---|---|---|
| 0 | Clean Odin Revert | **Done** | `5ef625e4` | Forward-reverted `core/` & Odin wiring on `main`; preserved `v0.18`–`v0.20`, all film stocks, `8154a6ac`, and `windows/src/inspector_view/`. Mac + PC green. |
| 1 | Extract Pure-Rust `crates/negs-app` | **Done** | `85281bf8` | Promoted `windows/marks` → `crates/negs-marks`; extracted all 32 non-UI modules + 6 themes + `Model` into standalone `crates/negs-app` behind `Host` & `platform`; `windows/` thin shell green on PC. |
| 2 | GPUI Shell + Color-Managed Canvas | **In progress** | — | Locking `gpui = "0.2.2"`; creating `app/` workspace with Newsreader fonts, 6 themes, `EngineClient`/`Model`, exact-fit Lanczos + monitor ICC / macOS Display P3 canvas. |
| 3 | Inspector, Presets & Stock Tray | Pending | — | Custom widgets, `inspector_view`-driven Prep/Lab/Output inspector, Presets sidebar, Film/Paper Stock Preview Tray. |
| 4 | Library, Filmstrip, Grid & Flight | Pending | — | `negs-library` + folder watcher, filmstrip with `negs-marks`, contact-sheet grid, 60 fps `SheetFlight` transition. |
| 5 | Compare, Crop, Negative, Export, Batch & Settings | Pending | — | Full feature parity with `windows/` and `macos/`. |
| 6 | Windows Cutover & Packaging | Pending | — | ABBA perf/screenshot report vs Win32 `Negs.exe`, Go/No-Go gate, installer + CI cutover. |
| 7 | Linux Adoption & macOS Polish | Pending | — | 7a Linux (T2 parity, retire GTK4), 7b macOS (HIG polish, Go/No-Go before retiring SwiftUI). |

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
