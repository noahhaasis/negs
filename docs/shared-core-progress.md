# Shared Odin App Core: Progress Log

Newest first. One entry per phase and per merge: what landed (SHA), differential test results, leak/sanitizer status, compile-time and performance measurements, ABI decisions, and anything carried over.

See [the plan](shared-core-plan.md) and [the coordinator handoff](shared-core-coordinator.md).

## Status

| # | Phase | State | Notes |
|---|---|---|---|
| 0 | Foundation & Baselines | done (Mac) | Odin `dev-2026-11a` pinned; `core/` + `negs_core.h` + leak-checked tests; Swift `CNegsCore` links; baselines below |
| 1 | Pilot: `marks` + `theme` | done (Mac + Windows) | Bit for bit with `windows/marks`; macOS and Windows draw marks and themes from the core |
| 2 | Pilot Go/No-Go: `library` | done (Mac + Windows); **Go** from Noah 2026-11-22 | Fixtures byte for byte, suites + lockstep at 50 seeds on Mac and Windows; Windows app on the core; Mac↔Windows round trip identical; screenshots identical; startup within noise |
| 3 | Pure Domain Rules & Media Helpers | in progress (3 slices in parallel) | 3a sheet (`sheet_rules`/`tray_rules`/`compare` + `model/compare`'s rules), 3b picture (`crop`/`border`/`exact_fit`/`thumb_compose`/`histogram`/`negative`), 3c media (`embedded_preview`/`photo_facts`/`tone_graph`/`help`) |
| 4 | State, Presets & Data-Driven Inspector | not started | Includes Inspector ViewModel schema |
| 5 | `EditorModel` & `EngineClient` | not started | Frame pipeline + delete ~28k LOC non-UI Rust from `windows/src/` |
| 6 | Linux & macOS Adoption | not started | 6a `linux/`, 6b `macos/` |
| 7 | Performance, Memory & Compile-Time Evaluation + Optimization | not started | Odin vs. Rust comparison report + optimization pass |

## Toolchain

- **Base `main` SHA:** `a9d64f6` (2026-11-21)
- **Odin version (Mac):** `dev-2026-11a` (`odin version`: `dev-2026-11-nightly:9a69916`), the official macOS arm64 release in `~/.negs/odin/dev-2026-11a` (`core/install_odin.py`). Pinned in `core/ODIN_VERSION`; `build.py` refuses any other build. (Nix had `dev-2025-01` on PATH; Homebrew offers `2026-11a` but floats with upgrades, so neither is used.)
- **Odin version (Windows PC):** `dev-2026-11a` (`9a69916`), the official windows_amd64 release in `C:\Users\Noah\.negs\odin\dev-2026-11a` (`core/install_odin.py`), with MSVC's `lib.exe` (found through vswhere).

## Baseline vs. Final Comparison (fill in Phase 0 and Phase 7)

| Metric | Rust/Swift Baseline (Phase 0) | Odin Core (Phase 7, pre-opt) | Odin Core (Phase 7, post-opt) |
|---|---:|---:|---:|
| Core cold build | Rust `negs-library` 4.7 s (`cargo build --release`, cold target dir) | — | — |
| Core 1-line incremental build | Rust `negs-library` 1.6 s (function-body edit in `sidecar.rs`); its tests 1.3 s more; `negs-marks` 0.4 s | — | — |
| Windows 1-line UI edit rebuild (`windows/src/ui/inspector.rs`) | `cargo check` (x86_64-pc-windows-msvc, on the Mac) 1.4 s, same for `sheet_rules.rs`; PC release build (LTO off, 16 units, incremental) 4.5 s; with the core linked 4.6 s | — | — |
| macOS 1-line edit rebuild (`--dev`) | Swift release 56 s (117 s cold); debug test build 9.5 s (string edit in `Sidecar.swift`) | — | — |
| Windows startup: `first_window_composited` (median) | 87.4 ms (Phase 2 gate, 12 runs ABBA, `8154a6a`, release LTO off); with the core 83.6 ms | — | — |
| Windows startup: `inspector_shown` (median) | 192.5 ms (same runs); with the core 188.3 ms | — | — |
| Library: 500-photo folder open + sidecar parse/write + look hash | open + parse + summaries + looks 28.6 ms (175,861 allocs, 28.4 MiB); rate 500 + undo (1,000 writes) 216 ms (330,562 allocs); filter + sort 5×500 0.38 ms | — | — |
| Marks: 1,000 names × (5 stars + cross + grid box) | 23.6 ms (203,585 allocs, 163.9 MiB) | — | — |
| `exact_fit` Lanczos downscale (6k×4k → fit) | 6000×4000 → 1500×1000: 50.7 ms | — | — |
| `embedded_preview` TIFF IFD scan (61 RAWs) | `~/Downloads/free_raws` (95 files, 58 with TIFF previews), find + read largest, warm: 11.8 ms | — | — |
| `sheet_rules` packed-row layout (1,000 items) | `SheetLayout::new` ×100: 0.47 ms | — | — |
| Peak RSS (500-photo folder + grid workload) | library bench 6.4 MiB; marks 2.8 MiB; exact_fit 168.7 MiB (its 96 MB input); embedded 17.4 MiB | — | — |
| Heap leaks (`Tracking_Allocator`) | n/a | 0 | 0 |

## Log

### 2026-11-22: Go for Phase 3; build tooling and Phase 3 groundwork (branch `core-p3-base`)

- **Noah's answers:** Go for Phase 3; adopt `-use-separate-modules`; add
  an ASan target. The Phase 2 commits were re-dated to 22 Nov evening at
  his request (merge now `c52e424`).
- **`-use-separate-modules`** for the library and tests: `build.py lib`
  1.5 s (6.8 s whole-program, kept as `--whole-program` for comparing).
  `build.py diff` passes, the marks bit for bit. Windows (`-build-mode:obj`
  then writes one object per package; `lib.exe` archives them all) is
  checked at the Phase 3 PC gate.
- **Trap found:** with separate modules, Odin writing to an archive that
  already exists keeps the old members' code: a rebuilt library still said
  ABI 3. Every build now goes to a fresh folder and is moved into place.
- **ASan:** `build.py test --asan` (unit tests; fails unless it catches a
  deliberate use after free in `tests/asancheck`) and `build.py diff
  --asan` (every Rust and differential test against an instrumented,
  tracked core). Both clean. On macOS it links Homebrew LLVM's runtime.
  What it adds: Odin's heap is libc's malloc, so use after free and stray
  raw pointers are caught; small overruns past a slice's end often aren't
  (the heap allocator pads for alignment), and bounds checks cover those.
- **Phase 3 slices, regrouped:** `histogram` and `negative` build on
  `crop`'s types, so they move from 3c to 3b; the three slices then share
  no Odin code. `model/compare` is mostly the frame pipeline (Phase 5):
  3a ports only its pure rules.
- **Groundwork so the slices merge cleanly:** ABI 4 (bumped once for the
  phase); a header section, layout table and `sys` module per slice; the
  `negs-core-rules` wrapper crate (`windows/` and `core/diff` depend on
  it); every Odin test package under `core/tests` runs; the benches' Rust
  side frozen in `core/diff/oracle`.

### 2026-11-22: Phase 2 Windows gate (branch `core-win-gate`)

- **Odin on the PC:** `dev-2026-11a`, installed by `core/install_odin.py`.
  `build.py test` passes there, including the library's kernel32 file
  system (Phase 2 had only type-checked it).
- **Linking on Windows.** None of Odin's static-library modes works for a
  Rust host:
  - Odin's default linker (radlink) and lld leave `memset` and
    `_tls_index` unresolved.
  - `-linker:msvc` archives every library the packages import, the static
    CRT among them: `LNK2005` against Rust's DLL CRT, and a 44.7 MB `.lib`.

  `build.py` now has Odin write an object (`-build-mode:obj`) and archives
  only that with MSVC's `lib.exe`. The result is 1.06 MB. The CRT and system
  libraries come from the host's link; `negs-core-sys` adds `bcrypt`. Test
  executables get `.exe`. `Negs.exe` grows by 0.5 MB (21.31 → 21.81 MB,
  LTO off).
- **Fixed:** an object has no entry point either, so `core_context()` now
  runs the runtime's startup for `.Object` builds as well as `.Static`. The
  Windows `negs-core-library` suite caught it (Finder order of `Ω`).
- **The Windows app on the core:** `windows/Cargo.toml` renames
  `negs-core-marks` and `negs-core-library` to `negs-marks` and
  `negs-library`, so no call site changed.
  - `ui/theme.rs` takes the six theme tables, names, hints, `parse`,
    `luminance`, `contrast` and `mix` from the core. The scalar constants
    stay in Rust and a test checks them against `negs_theme_constants`.
  - The original `theme.rs` is frozen as the theme oracle
    (`core/diff/tests/oracle/windows_theme.rs.txt`); `windows/marks` stays
    as the marks oracle.
  - CI: the Windows jobs install the pinned Odin and run `build.py test`;
    `core/**` changes trigger them.
- **Gate, all passing on the PC:**
  - `build.py test`.
  - `build.py diff --seeds 50`: fixtures byte for byte, `negs-library`'s
    suites, lockstep at 50 seeds.
  - The Windows app's `cargo test`: 292 passed, plus `negs-marks` 16.
  - To make the diff crate run on Windows: peak memory via
    `K32GetProcessMemoryInfo`, and `core/diff/tests/mac-encoded` as checked
    copies instead of a symlink, which Windows checkouts write as a text
    file.
  - **Flake, not the core's:** `state.rs` seed 44 failed once on Windows
    for the Rust oracle and once for the core, and passed on reruns. An
    "another app" write that changes neither size nor write time within a
    timer tick goes unnoticed (likely NTFS timestamp granularity). The Mac
    never shows it. Left as is; worth a look when the library's change
    detection is next touched.
- **Mac ↔ Windows round trip** (`core/diff/src/bin/roundtrip.rs`):
  - `make` builds a folder with every sidecar fixture, RAW+JPEG pairs, and
    non-ASCII and NFC names. `step` applies seeded core edits: ratings,
    flags, open/edit/save, copy and paste, duplicate, reset, undo.
  - `check` opens copies with the core and the Rust oracle and requires:
    the same view, no bytes changed by opening, and every sidecar
    rewriting byte for byte in both.
  - Results:
    - Mac → PC: identical digests.
    - The same seeded steps on each machine: byte-identical folders.
    - PC → Mac: identical once the transfer's NFD names (macOS `tar`) are
      normalised back. The NFD folder itself also checks OK.
    - Redone after the wire change below: the same digests.
- **Screenshots** (3 themes × photo and grid, base and core, nine D750
  NEFs with core-written sidecars): pixel-identical except the status bar's
  render time and the menu underlines that the harness's Alt tap shows.
- **Startup.**
  - The first run was +31 ms at `inspector_shown`, and above every base
    run.
  - Bisected with builds that put modules back on Rust: themes only +5 ms;
    themes + library +22 ms.
  - A call trace (`NEGS_CORE_TRACE`, a diagnostic branch, not merged)
    showed ~10 ms of library calls on the UI thread:
    - `unedited_state` 16–20 times, ~0.2 ms each.
    - `edit` and `set_defaults` 0.5–0.6 ms each.

    Most of it was Rust formatting numbers with the oracle's Swift-style
    `swift_double_description`. The later stages, the inspector's painting
    above all, were then bimodal (12 or 40 ms), most likely because they
    landed on the engine and GPU start-up.
  - Fixed in the wrapper:
    - `unedited_state` is cached until the library changes.
    - Values cross as compact JSON with Rust's shortest exact numbers
      (`codec::wire`). A new test sends 20,000 doubles (random bits,
      subnormals, extremes, ties) and awkward strings, and requires the core
      to read back the same bits and text.
  - After: 12 runs ABBA, core vs base:

    | stamp | core | base |
    |---|---:|---:|
    | `first_window_composited` | 83.6 ms | 87.4 ms |
    | `inspector_shown` | 188.3 ms | 192.5 ms |
    | `first_image_on_screen` | 918 ms | 926 ms |

    Every stage matches and the paint times are no longer bimodal.
  - The open sequence alone (`roundtrip open docs`) is still 2.7 ms on the
    core against Rust's 1.4 ms: JSON in and out of the ABI. A candidate for
    Phase 4's state design and Phase 7.
- **Speed on the PC** (`bench library`, 500 photos):

  | | Rust | Odin core |
  |---|---:|---:|
  | open | 58.1 ms | 49.2 ms |
  | 1,000 writes | 706 ms | 598 ms |
  | filter + sort 5×500 | 0.59 ms | 2.76 ms |
  | marks (1,000 names) | 48.7 ms | 45.0 ms |
- **Compile times on the PC:**
  - `build.py lib` (whole program): 7.5 s, against 6.5–6.9 s on the Mac.
  - A one-line `inspector.rs` edit: 4.6 s with the core, 4.5 s without.
    The core isn't rebuilt for UI edits.
- **Open for Noah:**
  - Go/No-Go for Phase 3.
  - `-use-separate-modules` (1.6 s instead of ~7 s builds; marks still bit
    for bit).
  - An ASan target in `build.py`.

### 2026-11-21: Phase 2 Mac side, `library` (branch `core-p2-library`)

- **Port:** `core/src/library` (23 files, ~7,400 lines of Odin, against
  `negs-library`'s 5,385 lines of Rust): its own JSON reader/writer,
  SHA-256, Finder order, folder listing and RAW+JPEG pairing, sidecars,
  edits, XMP, thumbnails' looks and keys, filter, selection, delete plans,
  the Library state machine with undo, and the file system (POSIX; a
  kernel32 version that type-checks for Windows but has not run there).
  ABI (`core/src/abi/library*.odin`, ~1,700 lines): an opaque
  `NegsLibrary*` used from one thread at a time, values crossing as JSON
  (shapes in the header), the bin as a host vtable (`negs_bin_t`), a
  per-call arena. `NEGS_CORE_ABI_VERSION` is 3.
- **Rust wrapper** `core/rust/library` (`negs-core-library`) has
  `negs-library`'s API: `Library` and `Selection` run in Odin; plain data
  types and pure helpers (Json, sidecar read/write, edits encode/decode,
  filter words, `DeletePlan::new`, `FolderBin`) are re-exported from Rust.
- **Gates, all passing** (`build.py test`; `build.py diff --seeds 50`):
  - `docs/sidecar-fixtures/`: every fixture reads and rewrites byte for
    byte, in Odin's own tests, through the C ABI, and through the wrapper.
  - `negs-library`'s own suites, unchanged (`fixtures.rs`, `library.rs`,
    `state.rs` at `NEGS_LIBRARY_SEEDS=50`), run against the core.
  - **Lockstep** (`core/diff/tests/lockstep.rs`): Rust and the core each get
    the same folder (fixture sidecars, RAW+JPEG pairs, non-ASCII names) and
    the same ~30 kinds of random action; after every step, every file's
    bytes, the items, names, summaries, looks, thumbnail states, export
    edits, the open photograph, copied edits, defaults, Undo/Redo and each
    outcome must match. **0 diffs** at 50 seeds (3,000 steps) and at
    300 seeds × 120 steps (36,000 steps); different id seeds and a planted
    bug both fail it. **0 leaks, 0 bad frees** (21M core allocations).
  - ASan (with Homebrew LLVM's runtime: Apple clang's lacks the check Odin's
    LLVM 20 emits): Odin tests and the 50-seed suites clean.
  - Full macOS `swift test` against this core: 490 tests, 0 failures.
- **Fixed for every module:** a static library has no entry point, so
  Odin's runtime startup never ran and globals initialised by code stayed
  zero: `core:unicode`'s tables, so `is_letter`/`is_upper` were false past
  Latin-1 (Ω sorted after an emoji). `core_context()` now runs
  `runtime._startup_runtime()` once in static builds. Tests missed it
  because they run as executables; 40,000 seeded Finder-order pairs and the
  lockstep folder's non-ASCII names now catch it.
- **Speed** (500 photos, median of 7; on battery, indicative):

  | | Rust | Odin core |
  |---|---:|---:|
  | open + summaries + looks + names | 28.3 ms | 15.0–15.5 ms |
  | rate 500 + undo (1,000 writes) | 206–210 ms | 177 ms |
  | filter + sort 5×500 | 0.34 ms | 1.2 ms (500 items as JSON per call) |
  | peak RSS | 6.4 MiB | 11.8–12.1 MiB |

  Rust-side allocations fall from 175,861 / 330,562 (open / rate) to
  19,831 / 2,037; the core makes 3,294 / 4,608 of its own.
- **Compile time:** `build.py lib` is a whole-program build, cold or after
  a one-line edit: **6.5–6.9 s** with the library (0.6 s before it), 92% of
  it LLVM codegen of one `-o:speed` module. With `-use-separate-modules`:
  **1.6 s**; the marks stay bit for bit, theme and lockstep pass, the
  library bench is within noise. Not adopted yet: Noah's call.
- **Open:** path equality is string equality; non-UTF-8 file names are read
  lossily; `is_letter`/`is_upper` follow Odin's Unicode tables (equal to
  Rust's on all 40,000 sampled pairs). Windows: type-checks only.

### 2026-11-21: Phase 1, `marks` + `theme` (branch `core-p1-marks-theme`)

- **marks** (`core/src/marks`, ~1,000 lines): hash, mulberry32, ribbon,
  pencil, ring (CSS radii), star, cross, pick box (grid, strip, selection),
  path bounds; `negs_marks_*` returns owned `negs_strokes_t` blocks or
  writes `negs_seg_t` paths into the caller's array. The Rust wrapper
  `core/rust/marks` (`negs-core-marks`) mirrors `negs-marks`' API, so the
  Windows app can switch with a Cargo rename.
- **Differential gate** (`core/diff/tests/marks.rs`, 10,000 seeded names,
  sizes and edge sizes, every coordinate, count, kind, tilt and opacity):
  **0 differences on arm64**, bit for bit; nothing leaks (tracked build).
  Getting there took two fixes worth knowing for later phases:
  1. Odin folds untyped constant expressions exactly (`1.0 - k`, `4.0/2.6`),
     where Rust and Swift round each step in f64: 1.9% of values differed by
     up to 1.2e-10 until those constants became typed `f64` variables.
  2. LLVM fuses `sin(a)` and `cos(a)` of the same angle into one
     `__sincos_stret` (Darwin) or `sincos` (glibc) call, whose results differ
     in the last bit from separate calls (3,927 of 1e6 angles). The core calls
     the same fused routine (`marks.sin_cos`); separate calls on MSVC.
  Under Rosetta (x86_64) 3,062 of 200M values differ by ≤ 2.8e-14, inside
  the plan's 1e-12 gate: Rust folds some sines of constant angles at compile
  time there. The test demands exactness on aarch64-macos only.
- **theme** (`core/src/theme`): the six themes' 14 tokens, film inks, flags,
  keys, titles, hints per platform (macOS's and Windows' differ: their
  defaults do), parse (Windows' rules), WCAG luminance and contrast, mix,
  and the shared constants. `core/diff/tests/theme.rs` reads Windows'
  tables from `windows/src/ui/theme.rs` (it links the Windows API, so it
  can't build on the Mac) and holds copies of its maths to the source:
  tables, names and hints equal, luminance/contrast/mix bit for bit over
  200,000 colour pairs, parse equal over 50,000 fuzzed names.
- **macOS wiring:** `AppTheme` titles, hints and palettes, `Theme.contrast`
  /`luminance` and the constants, and `HandStar`, `RejectCross`, `PickBox`,
  `PencilStroke` and `PencilRing` now call `CNegsCore` (`CoreMarks.swift`
  turns blocks into `Path`s); −250 lines of Swift. **No visual change:**
  before switching, a throwaway test drew 3,000 random names and sizes both
  ways: all 23.7M coordinates equal once both go through SwiftUI's `Path`
  (which stores Float32, so raw doubles differ by ~1e-6 either way); Plain
  rings (`CGPath(roundedRect:)` before) are pixel-identical at 4×; the old
  Swift tables and hints equal the core's. Full `swift test`: 490 tests,
  0 failures (20 skipped, as on main).
- **Tooling:** `build.py diff` runs `negs-core-sys`, `negs-core-marks` and
  `core/diff` with allocations tracked (~30 s warm). `NEGS_CORE_ABI_VERSION`
  is 2.
- **Numbers** (on battery at 17%, load ~22, so indicative only; Phase 7
  re-measures): the whole core (1,769 lines of Odin) builds in **0.57 s**
  (`build.py lib`, 83 KB `.a`); there is no incremental build, it's always
  the whole library. Marks, 1,000 names × (5 stars + cross + box): Rust
  23.5 ms, core through the Rust wrapper 24.8 ms (+6%: the wrapper copies
  each block into `Vec`s).

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
