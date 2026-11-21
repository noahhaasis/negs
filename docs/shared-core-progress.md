# Shared Odin App Core: Progress Log

Newest first. One entry per phase and per merge: what landed (SHA), differential test results, leak/sanitizer status, compile-time and performance measurements, ABI decisions, and anything carried over.

See [the plan](shared-core-plan.md) and [the coordinator handoff](shared-core-coordinator.md).

## Status

| # | Phase | State | Notes |
|---|---|---|---|
| 0 | Foundation & Baselines | done (Mac) | Odin `dev-2026-11a` pinned; `core/` + `negs_core.h` + leak-checked tests; Swift `CNegsCore` links; baselines below |
| 1 | Pilot: `marks` + `theme` | not started | Mac-side + Swift `CNegsCore` wiring |
| 2 | Pilot Go/No-Go: `library` | not started | Mac differential tests first → **ping Noah for Windows PC** → Go/No-Go review |
| 3 | Pure Domain Rules & Media Helpers | not started | 3a (`sheet`/`tray`/`compare`), 3b (`crop`/`border`/`exact_fit`/`thumb_compose`), 3c (`embedded_preview`/`photo_facts`/`tone_graph`/`histogram`/`negative`/`help`) |
| 4 | State, Presets & Data-Driven Inspector | not started | Includes Inspector ViewModel schema |
| 5 | `EditorModel` & `EngineClient` | not started | Frame pipeline + delete ~28k LOC non-UI Rust from `windows/src/` |
| 6 | Linux & macOS Adoption | not started | 6a `linux/`, 6b `macos/` |
| 7 | Performance, Memory & Compile-Time Evaluation + Optimization | not started | Odin vs. Rust comparison report + optimization pass |

## Toolchain

- **Base `main` SHA:** `a9d64f6` (2026-11-21)
- **Odin version (Mac):** `dev-2026-11a` (`odin version`: `dev-2026-11-nightly:9a69916`), the official macOS arm64 release in `~/.negs/odin/dev-2026-11a` (`core/install_odin.py`). Pinned in `core/ODIN_VERSION`; `build.py` refuses any other build. (Nix had `dev-2025-01` on PATH; Homebrew offers `2026-11a` but floats with upgrades, so neither is used.)
- **Odin version (Windows PC):** *TBD at Phase 2 Windows checkpoint*

## Baseline vs. Final Comparison (fill in Phase 0 and Phase 7)

| Metric | Rust/Swift Baseline (Phase 0) | Odin Core (Phase 7, pre-opt) | Odin Core (Phase 7, post-opt) |
|---|---:|---:|---:|
| Core cold build | Rust `negs-library` 4.7 s (`cargo build --release`, cold target dir) | — | — |
| Core 1-line incremental build | Rust `negs-library` 1.6 s (function-body edit in `sidecar.rs`); its tests 1.3 s more; `negs-marks` 0.4 s | — | — |
| Windows 1-line UI edit rebuild (`windows/src/ui/inspector.rs`) | `cargo check` (x86_64-pc-windows-msvc, on the Mac) 1.4 s, same for `sheet_rules.rs`; full PC build at the Phase 2 gate | — | — |
| macOS 1-line edit rebuild (`--dev`) | Swift release 56 s (117 s cold); debug test build 9.5 s (string edit in `Sidecar.swift`) | — | — |
| Windows startup: `first_window_composited` (median) | ~82–90 ms *(re-measure in Phase 0/2)* | — | — |
| Windows startup: `inspector_shown` (median) | ~176–181 ms *(re-measure in Phase 0/2)* | — | — |
| Library: 500-photo folder open + sidecar parse/write + look hash | open + parse + summaries + looks 28.6 ms (175,861 allocs, 28.4 MiB); rate 500 + undo (1,000 writes) 216 ms (330,562 allocs); filter + sort 5×500 0.38 ms | — | — |
| Marks: 1,000 names × (5 stars + cross + grid box) | 23.6 ms (203,585 allocs, 163.9 MiB) | — | — |
| `exact_fit` Lanczos downscale (6k×4k → fit) | 6000×4000 → 1500×1000: 50.7 ms | — | — |
| `embedded_preview` TIFF IFD scan (61 RAWs) | `~/Downloads/free_raws` (95 files, 58 with TIFF previews), find + read largest, warm: 11.8 ms | — | — |
| `sheet_rules` packed-row layout (1,000 items) | `SheetLayout::new` ×100: 0.47 ms | — | — |
| Peak RSS (500-photo folder + grid workload) | library bench 6.4 MiB; marks 2.8 MiB; exact_fit 168.7 MiB (its 96 MB input); embedded 17.4 MiB | — | — |
| Heap leaks (`Tracking_Allocator`) | n/a | 0 | 0 |

## Log

### 2026-11-21: Phase 0, foundation and baselines (branch `core-p0-foundation`)

- **Odin** pinned at `dev-2026-11a` (commit `9a69916`, `core/ODIN_VERSION`).
  `core/install_odin.py` installs it into `~/.negs/odin/<tag>` from the
  official release (macOS arm64, Windows amd64, Linux); `core/build.py`
  finds it (`$NEGS_ODIN`, then `~/.negs/odin/<tag>`, then PATH) and refuses
  any other commit.
- **Scaffold:** `core/include/negs_core.h` (C99, `NEGS_CORE_ABI_VERSION 1`,
  `negs_buf_t` owned blocks, `negs_core_alloc_stats`, `negs_core_layout`);
  `core/src/abi` (package `negs_core`, the static library's root; every
  export runs in `core_context()`); `core/tests` (`odin test`, each test
  under `mem.Tracking_Allocator`, `ODIN_TEST_FAIL_ON_BAD_MEMORY=true`);
  `core/tests/leakcheck` (leaks on purpose: `build.py test` fails unless
  it's caught); `core/tests/c/layout_check.c` (every header struct's
  `sizeof`/`offsetof` against the core's, plus a leak check across the ABI
  in a `--track` build).
- **Builds:** `build.py lib` (host) 0.4 s; `--universal` (arm64 + x86_64,
  lipo) 0.5 s. The library is built with `-no-entry-point` (without it Odin
  exports `_main`, which would clash with the apps') and exports only
  `negs_*` symbols. Windows `.lib` can't be cross-linked from the Mac (Odin
  needs MSVC's linker); it builds on the PC.
- **Decision: allocation tracking across the ABI.** `build.py lib --track`
  wraps the core's heap in a mutex-guarded `mem.Tracking_Allocator`;
  `negs_core_alloc_stats` reports live bytes/count, totals and bad frees, so
  the Rust and Swift differential tests assert 0 leaks after each run.
- **Decision: libm for inexact maths.** The marks' `sin`/`cos`/`atan2`/
  `hypot` go through `core:c/libc` (the platform's libm, as Rust's `f64`
  methods and Swift's Darwin functions do), so geometry can match bit for bit.
- **Swift:** `macos/Package.swift` gains the `CNegsCore` system-library
  target (`macos/Sources/CNegsCore/module.modulemap` → `core/include`),
  linked from `target/negs-core` (`Context.packageDirectory`).
  `CoreSmokeTests` (2 tests) pass in `swift test`.
  `scripts/package_macos_app.sh` builds the core first (universal, host-only
  for `--dev`). CI (`release.yml`, macOS job): installs the pinned Odin, runs
  `build.py test`, and `core/**` triggers the macOS build. *(Not run: nothing
  is pushed.)*
- **Benchmarks:** `core/diff` (its own Cargo workspace, `target/core-diff`)
  holds the Rust side of every Phase 7 workload (`bench library|marks|
  sheet|fit|embedded`), with a counting global allocator; it compiles
  `windows/src/{exact_fit,embedded_preview,sheet_rules}.rs` as they are.
- **Baselines** are in the table above. M1 Pro, 32 GB, macOS 26.5.2, rustc
  1.93.1, Swift 6.3, Claude build profile (no LTO, 16 codegen units,
  incremental). Caveat: **taken on battery** (Low Power Mode off) with load
  average ~8 from other sessions; Phase 7 re-measures both sides ABBA under
  the same conditions before comparing. Windows startup is re-measured at the
  Phase 2 PC gate.

### 2026-11-21: Plan and coordinator handoff created (`a9d64f6`)

- Decided with Noah: build the shared non-UI app core in **Odin** (`core/`), leaving the Rust film engine (`crates/spektrafilm-*`, `crates/negs-libraw`, `web/`) untouched.
- Work starts on the Mac (Phases 0, 1, and Mac-side Phase 2). Coordinator pings Noah once Windows PC access is needed (first ping at the end of Phase 2 for Windows Odin setup, Phases 0–2 integration, and the Go/No-Go checkpoint).
- Final Phase 7 compares compile times, runtime performance, and memory usage against the Rust baseline and runs an optimization pass once correctness is locked in.
