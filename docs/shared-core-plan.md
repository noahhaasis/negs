> **Superseded (2026-11-25):** The Odin shared-core experiment was cleanly reverted in favour of the pure-Rust shared core + cross-platform GPUI app. See [gpui-app-plan.md](gpui-app-plan.md).

# Shared App Core in Odin: Architecture and Migration Plan

Prepared 2026-11-21 against `main` at `a9d64f6`.

## Goals

1. **Faster compile times** for day-to-day app and UI iteration (target: sub-second to ~2 s rebuilds of the shared app core, and substantially smaller/faster incremental builds in `windows/`, `linux/`, and `macos/`).
2. **One shared app core for faster porting and new features** across Windows, Linux, and macOS—eliminating the 2×–3× duplication of ~25k–29k lines of non-UI application logic.
3. **Uncompromised correctness** via differential testing against the existing Rust and Swift implementations before retiring any Rust/Swift code.
4. **Performance and memory parity (or better)** against the Rust implementation—measured cleanly at the end (startup medians, runtime latency, peak RSS, allocation counts) and optimized once correctness is locked in.

## Non-goals (hard boundary)

- **Do not touch the film engine:** `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, and `web/` stay in Rust as-is. Exports and the 496 golden renders must stay byte-identical. The shared core talks to `spektrafilm-native`, `spektrafilm`, and `spektrafilm-f64` over the existing helper/CLI protocol.
- **Do not rewrite the native UI toolkits:** macOS stays SwiftUI + AppKit + CoreAnimation; Windows stays Win32 + Direct2D/DirectWrite + wgpu; Linux stays GTK4 + Cairo/GSK + Wayland.

## Where things stand (`a9d64f6`)

| Target | Total LOC | Non-UI / shared logic inside |
|---|---:|---|
| `crates/negs-library/` | 5,385 | Pure Rust, **0 external deps** (currently used only by `windows/`) |
| `windows/marks/` (`negs-marks`) | 1,155 | Pure Rust, **0 Win32 deps** (pencil stroke, rings, stars, cross, pick box) |
| `windows/src/` (non-UI) | 27,888 | Full redesigned app core (`model.rs` 5.3k, `model/` 7.6k, `state.rs`, `sheet_rules.rs`, `tray_rules.rs`, `crop.rs`, `compare.rs`, `presets.rs`, `embedded_preview.rs`, `exact_fit.rs`, etc.) |
| `windows/src/ui/` | 30,031 | Win32 + Direct2D + DirectWrite + DirectComposition + wgpu UI |
| `macos/Sources/NegsApp/` | 27,918 | SwiftUI + AppKit UI + Swift copy of the app core (`EditorModel.swift` 6.1k, `Library.swift`, `Sidecar.swift`, etc.) |
| `linux/src/` | 21,546 | GTK4 + libadwaita UI + pre-T2 copy of non-UI logic (missing library, grid, flight, tray, compare, marks, themes) |

21 of the non-UI files in `windows/src/` already have **zero** Win32/UI imports; the remaining ~10 files have only 1–11 platform touchpoints each.

## Architecture of `core/`

```
core/
  include/
    negs_core.h          # Single versioned C99 ABI header (imported by Swift, Rust, C)
  src/
    abi/                 # C-ABI exports, opaque handles, buffer/arena helpers, platform vtable
    marks/               # Pencil stroke, rings, HandStar, RejectCross, PickBox
    theme/               # 5-theme token tables + WCAG contrast helpers
    library/             # JSON (Mac byte-exact), SHA-256, sidecars, edits, folder, filter, selection, delete, XMP, undo
    rules/               # sheet_rules, tray_rules, crop, compare, tone_graph, border, negative, histogram, help, exact_fit, thumb_compose, photo_facts, embedded_preview
    state/               # EditState, defaults, presets, session, batch, brightness, match_cache, rendered_preview, film_cache_release, inspector ViewModel
    model/               # EngineClient protocol + EditorModel frame-pipeline state machine
  tests/
    runner.odin          # Unit tests with mem.Tracking_Allocator (0 leaks enforced)
  diff/                  # Differential test drivers comparing Odin vs Rust crates/modules
  build.py               # Cross-platform build helper (darwin_arm64, darwin_amd64 + lipo, windows_amd64, linux_amd64/arm64)
```

### C ABI design rules

1. **Flat C99 header (`core/include/negs_core.h`)** with explicit integer widths (`uint32_t`, `int64_t`, `double`), `NEGS_CORE_ABI_VERSION`, and opaque handles (`NegsLibrary*`, `NegsApp*`).
2. **Explicit ownership:** Strings and variable-length arrays either write into caller-supplied buffers or return a borrowed view valid until the next mutating call on that handle (backed by the handle's scratch arena), or an explicit `negs_buf_t` freed via `negs_buf_free()`. Never leak Odin slices or `context.temp_allocator` pointers across FFI.
3. **Tiny platform vtable (`NegsPlatformVTable`):** OS-specific behaviour stays in the host shell and is injected via ~10 C function pointers:
   - `trash_item` / `restore_from_trash` (Recycle Bin / NSFileManager Trash / freedesktop Trash)
   - `atomic_replace_file` (handles Windows `MoveFileExW` / sharing violations vs POSIX `rename`)
   - `normalize_nfc` / `localized_compare` (when OS collation differs)
   - `spawn_helper` / `pipe_read` / `pipe_write` / `kill_helper` (or host-owned helper I/O feeding byte frames into `core`)
   - `now_monotonic_ns`, `wake_ui`, `log_event`
4. **Zero third-party C dependencies in `core/`:** Pure Odin + `core:*` only, so building static libraries (`libnegs_core.a` / `negs_core.lib`) is fast, reproducible, and has no external sysroot pain.
5. **Memory safety discipline:**
   - Every Odin test binary wraps `context.allocator` in `mem.Tracking_Allocator` and fails on any leak or bad free.
   - Run AddressSanitizer / UBSan builds (`-sanitize:address` where supported) at phase gates.
   - Prefer arena allocators per subsystem (document/folder arena, per-frame scratch arena) to keep allocation counts low and lifetimes obvious.

### Linking into each platform

- **macOS (`macos/`):** Add a SwiftPM C target (`CNegsCore`) pointing at `core/include/` with a `module.modulemap` and link `libnegs_core.a`. In `scripts/package_macos_app.sh`: `--dev` builds host arch (`darwin_arm64`); release builds `darwin_arm64` + `darwin_amd64` and `lipo`s them into a universal static lib.
- **Windows (`windows/`):** `windows/build.rs` emits `cargo:rustc-link-search` + `cargo:rustc-link-lib=static=negs_core` (plus any MSVC system libs Odin's static lib needs). A small `windows/src/core_ffi.rs` wraps the C ABI safely.
- **Linux (`linux/`):** Same pattern in `linux/build.rs` linking `libnegs_core.a`.

## Phases

The plan is staged so **Phases 0, 1, and 2 run on the Mac first**, culminating in a **Windows integration + Go/No-Go checkpoint** before committing to Phases 3–6.

| # | Phase | Contents | Reference (LOC) | Needs Windows PC? |
|---|---|---|---|---|
| **0** | **Foundation & Baselines** | Install/pin Odin on Mac; scaffold `core/`, `negs_core.h`, `Tracking_Allocator` test harness; smoke-link from macOS Swift (`CNegsCore`); record **Rust/Swift baseline metrics** (compile times, Rust `negs-library` & `windows/` benchmarks, RSS/memory). | New | No (Windows Odin install happens at the Phase 2 Windows checkpoint) |
| **1** | **Pilot: `marks` + `theme`** | Port `negs-marks` (pencil stroke, rings, `HandStar`, `RejectCross`, `PickBox`) and 5-theme token tables + WCAG contrast checks. Wire into `macos/` Swift as proof of C-ABI import. | `windows/marks/` (1,155 LOC), `Theme.swift` / `AppTheme.swift` | No (deferred to Phase 2 batch) |
| **2** | **Pilot Go/No-Go: `library`** | Port `crates/negs-library` (Mac-exact JSON, SHA-256, sidecars, edits, folder/pairing, filter, selection, delete plan, thumbnail look keys, XMP rating, undo). Run differential suite against Rust `negs-library` (`docs/sidecar-fixtures/` + 50 seeded random runs). **Then ping Noah for Windows PC** to install Odin on PC, link Phases 1+2 into `windows/`, run Mac↔Win sidecar round-trip, screenshots, and `measure_startup.ps1`. | `crates/negs-library/` (5,385 LOC, 0 deps) | **Yes — first ping to Noah** (at end of Phase 2, covering Phases 0–2 on Windows) |
| **3** | **Pure Domain Rules & Media Helpers** | Port all zero-Win32 modules in 3 parallel slices: **(3a)** `sheet_rules`, `tray_rules`, `compare` + `model/compare`; **(3b)** `crop`, `border`, `exact_fit` (Lanczos), `thumb_compose`; **(3c)** `embedded_preview` (TIFF IFD RAW JPEG locator), `photo_facts`, `tone_graph`, `histogram`, `negative`, `help`. Differential-test each against Rust before switching `windows/`. | `windows/src/*.rs` (~6,300 LOC, 0 Win32) | End of phase (batch all 3 slices into one PC gate) |
| **4** | **State, Presets & Data-Driven Inspector** | Port `state`, `edits`, `brightness`, `match_cache`, `rendered_preview`, `film_cache_release`, `presets`, `session`, `batch`. Add the **data-driven Inspector ViewModel** (tabs, sections, row kinds, labels, formatted values, defaults, min/max, changed/skipped flags, `"N more"`, edit counts, command dispatch). | `windows/src/*.rs` (~4,000 LOC) | End of phase |
| **5** | **`EditorModel` & `EngineClient`** | Port `engine` (helper JSON/binary protocol) and `model` + `model/*` (frame pipeline: quick/settled/held frames, brightness match, crop/border/negative/compare/batch coordination, unified undo, dirty bitmask). Differential-test against Rust `model.rs` using `FramePipelineTests` sequences (`NEGS_PIPELINE_SEEDS=25`) and `lifecycle_tests.rs`. Wire into `windows/` and **delete the superseded ~28k LOC of non-UI Rust from `windows/src/`**. | `windows/src/model.rs` + `model/*` + `engine.rs` (~8,500 LOC) | End of phase |
| **6** | **Linux & macOS Adoption** | **6a Linux:** Wire `libnegs_core.a` into `linux/`, replacing duplicated non-UI modules (`model.rs`, `presets.rs`, `border.rs`, `crop_geometry.rs`, `tone_graph.rs`, `brightness.rs`, `help.rs`, etc.) so the upcoming Linux T2 UI port only writes thin GTK4/Cairo/GSK views. **6b macOS:** Incrementally replace Swift `Library.swift`/`Sidecar.swift` and pure rules with `CNegsCore`, running `swift test`, `LibraryStateTests`, and `FramePipelineTests` at each step. | `linux/src/`, `macos/Sources/` | Only if ABI changes |
| **7** | **Performance, Memory & Compile-Time Evaluation + Optimization** | Benchmark **Odin core vs. Rust baseline** on identical workloads (see table below), profile hotspots and memory allocations, optimize where Odin trails Rust (or where clear wins exist), and write the final comparison report for Noah. | Full repo | **Yes — final Windows benchmark run** |

## Differential Testing Methodology

Every ported module keeps its Rust (or Swift) counterpart as an **oracle** until its differential gate passes:

1. **Marks (`Phase 1`):** Driver feeds 10,000 deterministic `(filename, rect, kind)` inputs to both Rust `negs-marks` and Odin `core/marks` via C ABI; asserts identical segment types, coordinates (≤ `1e-12`), and bounding boxes.
2. **Library (`Phase 2`):**
   - Fixture gate: every file in `docs/sidecar-fixtures/` reads and rewrites **byte-for-byte** identically to `crates/negs-library/tests/mac-encoded/`.
   - State-machine oracle: extend `crates/negs-library/tests/state.rs`'s seeded random action generator (`NEGS_LIBRARY_SEEDS=50`) to drive **both** Rust `Library` and Odin `NegsLibrary` in parallel temp directories, asserting after every step: identical `.negs` bytes on disk, identical `shown()` list, `summary()`, `look()` SHA-256 keys, `undo_name()`, and `Outcome` fields.
3. **Domain Rules (`Phase 3`):**
   - `sheet_rules` / `tray_rules` / `crop` / `compare` / `border`: property & seeded-input diff against Rust structs/rects.
   - `exact_fit` (Lanczos): pixel-exact (or ≤ 1 LSB) diff on test images at multiple scales.
   - `embedded_preview`: diff extracted JPEG byte offsets/lengths/orientations on test RAWs against Rust `embedded_preview.rs`.
   - `tone_graph` / `histogram` / `negative`: diff output arrays against Rust.
4. **State, Presets & Model (`Phases 4–5`):**
   - Preset corpus (including `data/presets/spektrafilm-gui-default-28bf883.json`) round-tripped identically.
   - Frame-pipeline oracle: replay seeded command streams (`NEGS_PIPELINE_SEEDS=25`) and `windows/src/model/lifecycle_tests.rs` scenarios against both Rust `Model` and Odin `NegsApp` (against the real `spektrafilm-native` helper), asserting identical helper requests, dirty masks, undo stacks, and settled states.
5. **End-to-End Platform Gates:**
   - macOS: `swift test --package-path macos --scratch-path target/swift-native` (`LibraryStateTests`, `FramePipelineTests`, `SidecarTests`, etc.) + isolated screenshots.
   - Windows: `cargo test --locked --manifest-path windows/Cargo.toml` + `shot.ps1` side-by-side screenshots + `measure_startup.ps1` ABBA medians.
   - Linux: existing `linux/check_*.py` + unit tests.

## Final Evaluation & Optimization Matrix (Phase 7)

Captured at **Phase 0 (Rust baseline)** and repeated at **Phase 7 (Odin core, before & after optimization)** on the exact same inputs and machines:

| Metric | How it is measured | Target |
|---|---|---|
| **Compile time — Core** | Cold and 1-line incremental build of `core/` (`odin build -o:speed` and debug) vs. Rust `negs-library` + `windows/` non-UI rebuild | Sub-second incremental; ~1–2 s cold |
| **Compile time — Windows app** | 1-line edit in `windows/src/ui/inspector.rs` (after removing ~28k LOC of non-UI Rust from `windows/src/`) vs. baseline | Measurably faster incremental link/build |
| **Compile time — macOS / Linux** | 1-line core edit + relink vs. Swift/Rust baseline | Faster iteration on shared logic |
| **Startup latency (Windows)** | `windows/measure_startup.ps1` ABBA (6 runs, `docs/card.jpg`): `first_window_composited`, `inspector_shown`, `first_image` | No regression vs. ~82–90 ms / ~176–181 ms baseline |
| **Startup latency (macOS)** | Existing macOS launch timing check | No regression |
| **Throughput — Library & Sidecars** | Open 500-photo folder, parse/write 500 `.negs` sidecars, compute 500 thumbnail SHA-256 looks, filter & sort | Within parity or faster than Rust |
| **Throughput — Hot loops** | `exact_fit` Lanczos downscale (6k×4k → fit), `embedded_preview` TIFF IFD scan (61 RAWs), `sheet_rules` packed-row layout (1,000 items), `marks` path generation | Within parity or faster than Rust |
| **Memory usage** | Peak RSS (`/usr/bin/time -l` on Mac, WorkingSet/PeakWorkingSet on Windows) + total heap bytes/allocations (`Tracking_Allocator`) on the 500-photo folder + 61-RAW open + grid flight workload | Equal or lower peak RSS vs. Rust, 0 leaks |
| **Binary size** | `libnegs_core.a` / `negs_core.lib` and final `Negs.exe` / `Negs.app` delta | Equal or smaller |

## Decisions (2026-11-21)

1. **Language:** Odin for the shared app core (`core/`).
2. **Engine stays in Rust:** `crates/spektrafilm-*`, `crates/negs-libraw`, and `web/` are untouched.
3. **Windows PC access:** Do Mac-side work first and **ping Noah once Windows is needed** (first ping at the end of Phase 2 for Windows Odin setup + Phases 0–2 integration, then once per Windows integration gate).
4. **Correctness first, optimize at the end:** Lock in differential parity and leak-free memory safety per phase; run the full Rust-vs-Odin performance and memory benchmark + optimization pass in Phase 7.

Execution handoff: [shared-core-coordinator.md](shared-core-coordinator.md).
Progress log: [shared-core-progress.md](shared-core-progress.md).
