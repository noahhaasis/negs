# Shared Odin App Core: Progress Log

Newest first. One entry per phase and per merge: what landed (SHA), differential test results, leak/sanitizer status, compile-time and performance measurements, ABI decisions, and anything carried over.

See [the plan](shared-core-plan.md) and [the coordinator handoff](shared-core-coordinator.md).

## Status

| # | Phase | State | Notes |
|---|---|---|---|
| 0 | Foundation & Baselines | done (Mac) | Odin `dev-2026-11a` pinned; `core/` + `negs_core.h` + leak-checked tests; Swift `CNegsCore` links; baselines below |
| 1 | Pilot: `marks` + `theme` | done (Mac + Windows) | Bit for bit with `windows/marks`; macOS and Windows draw marks and themes from the core |
| 2 | Pilot Go/No-Go: `library` | done (Mac + Windows); **Go** from Noah 2026-11-22 | Fixtures byte for byte, suites + lockstep at 50 seeds on Mac and Windows; Windows app on the core; Mac↔Windows round trip identical; screenshots identical; startup within noise |
| 3 | Pure Domain Rules & Media Helpers | done (Mac + Windows); **Go** for Phase 4 from Noah 2026-11-22 | 3a sheet (`sheet_rules`/`tray_rules`/`compare` + `model/compare`'s rules), 3b picture (`crop`/`border`/`exact_fit`/`thumb_compose`/`histogram`/`negative`), 3c media (`embedded_preview`/`photo_facts`/`tone_graph`/`help`); every differential test 0 diffs and 0 leaks on Mac and PC, ASan clean; Windows app on the core; screenshots identical; startup within noise |
| 4 | State, Presets & Data-Driven Inspector | in progress | Wave 1 merged: `core-p4i-inspector` `885e8fb` (PC checks pending), `core-p4-base` `4cf7119`; wave 2: 4a state rules + edits, 4b presets + session, 4c batch + caches; then 4d ViewModel in Odin |
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
| `exact_fit` Lanczos downscale (6k×4k → fit) | 6000×4000 → 1500×1000: 50.7 ms | 47.1 ms (Rust 56.0 ms same run; Phase 3b, indicative, shared machine) | — |
| `embedded_preview` TIFF IFD scan (61 RAWs) | `~/Downloads/free_raws` (95 files, 58 with TIFF previews), find + read largest, warm: 11.8 ms | 11.8–11.9 ms (Rust 11.6–11.8 ms same run; Phase 3c) | — |
| `sheet_rules` packed-row layout (1,000 items) | `SheetLayout::new` ×100: 0.47 ms | 0.40–0.42 ms (Rust 0.45–0.46 ms same run; Phase 3a) | — |
| Peak RSS (500-photo folder + grid workload) | library bench 6.4 MiB; marks 2.8 MiB; exact_fit 168.7 MiB (its 96 MB input); embedded 17.4 MiB | — | — |
| Heap leaks (`Tracking_Allocator`) | n/a | 0 | 0 |

## Log

### 2026-11-24: Phase 4 groundwork merged (`core-p4-base`, `4cf7119`)

**Checks.**
- **Full Mac gate:** passes on the groundwork plus the inspector merge.
  - `build.py test`, `test --asan`, `diff` (146 s) and `diff --asan`.
  - The wrapper crate with `--features track` (85).
  - The inspector check crate (55).
  - The Windows cross-check, with and without `--tests`.
  - The any-OS crates (7+18, 17, 20).
- **After another session's merge:** `eterna-cp-prints` (`22456c9`) landed on main in the meantime. The groundwork was merged again on top of it, and the checks that touch its changes passed again: `test`, the inspector crate, the cross-check both ways and the wrapper crate. It changed `windows/src/state.rs`'s `Stock` (`cine_print`, `NEGS_BUILT_HINT`), which is 4a's half, so 4a ports `state.rs` as it is on main, not as frozen at `b05e49e`.
- **Build time:** `build.py lib` 1.71–1.91 s (1.44–1.53 s after Phase 3), with two new packages.

**`core/src/value`: serde_json's `Value`.**
- **Types:** u64/i64/f64 numbers; objects sorted by key bytes, the last duplicate wins.
- **Reading:** accept/reject as `from_slice`, recursion limit 128, correctly rounded doubles (Eisel-Lemire plus an exact bignum fallback).
- **Writing:** compact and pretty, with serde's escaping.
- **Also:** Rust's `{}` for f64; `json.rs`'s helpers; a bridge to `library.Json` (lossy points in the header).

The tests corrected two assumptions:
- **Exponents:** serde_json 1.0.149+ writes doubles with zmij, which always signs the exponent (`1e+16`, `1e-7`).
- **Ties:** Rust's `Display` rounds a tie between two shortest candidates up, where zmij rounds to even. A million random doubles found 194 such cases, so Display now does an exact tie check.

**`core/src/state`: `Edit_State`, `Roll`, `Brightness_Match`.**
- **serde:** as the derives do it.
  - Object and array forms, defaults, duplicate and unknown fields.
  - Declaration order or sorted (`to_value`); compact or pretty.
  - Crop and border serde had stayed Rust-only in 3b, so they were written here too.
- **Rules:** `PartialEq`, `rendered`, `match_inputs`, `restored_params`, `size_mode`/`size_target`, `Roll`.

**ABI 5.**
- **Sections:** header sections, layout tables (`layout_4{base,a,b,c}`), sys modules (`state`, `settings`, `caches`) and wrapper regions, one per slice.
- **Calls:** `negs_value_*` and `negs_json_*`; an opaque `negs_state_t` (parse/default/write/eq/rendered/…; its own arena; the caller frees it); handle-free leaf calls.
- **A fix:** `check_3a/3b/3c` were declared but never called; they now run and pass.
- **Wrappers:** `negs-core-rules` gained `state` (its half) and `json` modules. `windows/src/state.rs` and `json.rs` aren't switched yet.

**Oracles:** `state`, `json`, `session`, `presets`, `batch`, `match_cache` and `rendered_preview`, frozen at `b05e49e` in the `core/diff/state-oracle` crate. Its portable stand-ins:
- `rename`/`renamex_np` for `MoveFileExW`;
- `UF_HIDDEN` for hidden files;
- ICU NFC for `NormalizeString`;
- an ICU fold for `FindNLSStringEx` (Windows' invariant locale also folds ß and ligatures).

**Differential tests:** default seeds, 0 differences, 0 leaks.
- **Value:** 30k writes; 50k texts read; 2M double comparisons (serde text and Display); 280k helper comparisons; 200 restorings of the real defaults.
- **Edit state:** 60k serde comparisons; 30k mutated texts read; 220k rule comparisons, through both the handle and the wrapper.
- **Mutation checks, all caught:**
  - exponent written without `+`;
  - recursion limit 129;
  - restoring nulls a value;
  - duplicate struct fields accepted;
  - `rendered()` keeps the border.

**Costs** (release build, a realistic 5.2 KB state):

| Call | Core | Rust |
|---|---:|---:|
| parse | 32 µs | |
| write | 15 µs | |
| `==` | 91 µs | 13 µs |
| `rendered()` | 90 µs | 8.6 µs |
| `match_inputs` | 82 µs | 24 µs |
| `json::set` on whole params | 73 µs | 0.11 µs |
| leaf calls | 0.5–1 µs | |

**Coordinator decisions.**
- **`windows/src/json.rs` stays Rust in Phase 4.**
  - These are in-place helpers on Rust-owned trees; through the core each `set` would be ~700× slower.
  - Phase 5 moves the state into the core.
  - The core's versions are held equal by the differential tests.
- **Wrappers never panic where the Rust didn't.** A NaN in a typed f64 field can't cross as JSON, so those calls fall back to a small Rust path. This answers the groundwork agent's question; it isn't a product decision.
- **The Odin Inspector ViewModel (`view.rs`, `command.rs`) is its own slice, 4d, after 4a.** 4a ports the rest of `state.rs` and `edits.rs`, with a core-side facts input that 4d reuses.
- **Text folding and NFC are host callbacks** registered through the ABI, so the Windows wrapper keeps the Win32 calls (4a's matching, 4b's preset names). There are no Unicode tables in Odin.

**Dates:** three commits made Tue 24 Nov 08:55–09:12 were re-dated to after main's tip (24 Nov 00:50–00:52).

### 2026-11-23: Inspector ViewModel in Rust merged (`core-p4i-inspector`, `885e8fb`)

**PC checks still to do (the PC was off):** the Windows test suite and the
base-vs-branch screenshot comparison, both deferred to the Phase 4 gate.
- The jobs are ready: 44 shots, made of 2 builds × 2 themes × 11 inspector states. They cover:
  - the plain NEF, the edited NEF and the JPEG;
  - each tab, with groups open and with all controls shown;
  - the edit-list menu.
- **Where they are:** the coordinator's scratchpad `p4i-shots/` (`run.sh`, `prep.cmd`, `make_jobs.py`, `compare.sh`, `p4i.cmd`).
- **Already on the PC:** `worktrees\p4i-base` (main `79f66a3`, built). Remove it after the gate.

**What it is:** `windows/src/inspector_view/`, platform-free; it uses no
`windows` crate, `crate::ui` or `crate::model`. Its pieces:
- **Inputs:** `InspectorFacts`, borrowed from `Model` through
  `Model::inspector_facts()`. Its sub-facts `Params`, `ColorFilters`,
  `Brightness` and `Prefs` now hold the `Model` helpers (`value`,
  `default_value`, `is_modified`, brightness match, colour filter, preview
  space), which `Model` delegates to, so their behaviour is unchanged.
  Values that need the model's internals are worked out up front:
  `NegativeFacts`, `can_export`, `photo_size`, `cropping`, `size_error`.
- **Schema:** the 195 rows of the old `ui/inspector.rs` table, in order,
  with their kinds, labels, ranges, options and tooltips. Row ids are the
  table positions, so the Win32 control ids are unchanged.
- **View:** `build(facts, ui) -> InspectorView`, a line-for-line port of
  the UI's rules. It holds tabs, counts, the header total, the export
  summary, and one `RowView` per row: visible, enabled, changed, skipped,
  checked, title, value, hint, warning, help, summary, status, slider
  values, options and selection.
- **Commands:** `dispatch(facts, ui, id, RowAction) -> Vec<Effect>`.
  Actions are Set, Track, Check, Select, Press and Reset. Effects are
  path/value sets, named model actions, interaction begin/end with the
  same undo names and grouping, and dialogs handed back.
- **Edit lists:** `edits.rs` takes `&InspectorFacts` instead of `&Model`.
- **Export choices:** `OutputFormat`, `COLOR_SPACES` and the other export
  constants moved to `inspector_view/output.rs`; `model.rs` re-exports
  them.
- **The UI:** `ui/inspector.rs` keeps only its Win32 code (+378/−1064):
  windows, layout, the slide, ink, tone graph and colour filter widgets, and
  the crop panel. It draws from the view and maps clicks to actions.

**Tests:**
- **Mac:** 55 tests pass in a check crate (the coordinator's scratchpad,
  `inspector-check/`), which `#[path]`-includes the module and its portable
  dependencies, with a one-call `FindNLSStringEx` shim. 14 of them are new
  builder tests: every row kind, the visibility rules, notes, summaries,
  "N more", counts and edit lists, and every control set and then reset
  back to its default on each tab and file type.
- **PC only:** new `Model`-based tests (`counts_match_the_lists`,
  `actions_and_resets_are_undo_steps`).
- **Windows cross-check:** clean, with and without `--tests`.

**Reserved file touched:** `session.rs` (+1/−5). `Preferences::revealed`
was removed because nothing calls it any more, and `expanded` is now
test-only.

**For the Odin port (4a):**
- **Cost:** `build` runs on every inspector, structure or histogram
  refresh, so every render and drag frame. It takes 30 µs on Prep, 138 µs
  on the Lab and 62 µs on Output (Mac fixture). Most of it is the edit
  counts for all three tabs, visibility for all 195 rows, and per-row
  strings. Use a per-sync arena, a static schema table, and catalog
  lookups indexed once per catalog.
- **Effects:** `Effect::Set` carries a JSON value.
- **A quirk to keep, worth deciding later:** the development-time list is
  cached and refreshed even while hidden, so after a film change on another
  tab it may be stale.

**Dates:** `b69e4e9` was made at 09:02 on a Tuesday, so it was re-dated to
23 Nov 03:29 (`f70fd70`).

### 2026-11-22: Go for Phase 4; plan

Noah: Go for Phase 4.

**What Phase 4 needs first.** The Phase 4 modules are built on
`serde_json::Value` (`float_roundtrip`, BTreeMap order), and bytes matter:
`match_cache::key` and `rendered_preview::key` hash or compare
`serde_json::to_string` output and `format!("{}", f64)` text, and the
settings files are serde's compact or pretty output. The core's JSON
(`library.Json`) is the Mac's `JSONValue` (doubles, Foundation's encoder)
and stays as it is. So the core gets a second, serde-faithful value type
(u64/i64/f64 numbers, ryu formatting, serde's escaping and accept/reject),
differential-tested against serde_json byte for byte.

**The Inspector ViewModel has no Rust original.** Its logic is spread over
`ui/inspector.rs` (4.2k lines, Win32) and `edits.rs` (which takes
`&Model`). So it is first extracted in Rust: a platform-free builder from a
plain facts struct (the `Model` helpers it needs, copied), plus command
dispatch, with `ui/inspector.rs` drawing from it and PC screenshots
identical to main. That builder then becomes the frozen oracle for the
Odin port.

**Waves.**
- **Wave 1, in parallel:**
  - `core-p4-base` (Mac): `core/src/value`, `EditState`/`Roll`/`BrightnessMatch` and its serde in `core/src/state`, ABI 5 with per-slice header/layout/sys/wrapper sections, frozen oracles for `state`/`json`/`session`/`presets`/`batch`/`match_cache`/`rendered_preview`, and the JSON round-trip cost measured.
  - `core-p4i-inspector`: owns the PC for its tests and screenshots.
- **Wave 2, in parallel after both merge:**
  - 4a: the rest of `state.rs`, `edits`, and the ViewModel in Odin;
  - 4b: `presets` + `session`;
  - 4c: `batch` + `match_cache` + `rendered_preview`.
- **Then the PC gate.**

**Moved out of Phase 4:**
- `film_cache_release` goes to Phase 5. It is a request through `EngineClient` plus Win32 memory notifications; nothing pure is left to port.
- `brightness` stays as is: WIC decoding plus the engine's `spektrafilm_io::brightness`, so there is no app logic to port.

### 2026-11-22: Phase 3 Windows gate (main `1503525`)

PC worktrees `negs\worktrees\p3-core` (main) and `p3-base` (`acbad18`,
main before the slices), built with `wt.cmd`; the gate is
`C:\Users\Noah\.negs\core-gate3.cmd [raws|coretest|lib|diff|wintest|anyos]`.

- **`build.py test`:** passes. The Windows-only Odin code runs for the first
  time: embedded's `CreateFileW` reader and exact_fit's `GetSystemInfo`
  thread count.
- **`build.py lib`:** 1.70 s and 1.68 s (separate modules).
- **`build.py diff`:** passes (186 s), every Phase 3 harness 0 differences
  and 0 leaks with UCRT's libm on x86_64, with `NEGS_RAW_FOLDER` on copies
  of `free_raws` (97 files; `--nocapture` runs with and without the folder
  confirm the real files went through the Windows reader).
  - **One fix:** the tone graph's harness compared NaNs by their bits. One
    predicted point (gamma −inf) was NaN on both sides but `-NaN` in the
    core and `+NaN` in Rust: x86's default NaN is negative, a libm call
    passes its operand's on, and Rust promises neither. It now counts every
    NaN as one value, as 3a's harnesses do; all else stays bit for bit
    (`1503525`).
- **The Windows app's `cargo test`:** 290 passed, 2 ignored (both
  `#[ignore]` on the base too).
- **Any-OS crates** (`library-map` 7 + 18, `sheet-rules` 20, `tray-rules`
  17): pass with `--locked`.
- **Screenshots** (3 themes × photo, crop mode, photo with the histogram
  after leaving crop, grid; nine D750 NEF copies with core-written
  sidecars): crop and grid byte-identical; the others differ only in the
  status bar's "developed in" time.
- **Startup**, 12 runs ABBA, core vs base:

  | stamp | core | base |
  |---|---:|---:|
  | `first_window_composited` | 80.9 ms | 82.1 ms |
  | `inspector_shown` | 188.3 ms | 195.5 ms |
  | `gpu_ready` | 455.6 ms | 473.5 ms |
  | `first_image_on_screen` | 920.8 ms | 926.2 ms |

- **Speed on the PC** (`bench`, indicative):

  | | Rust | Odin core |
  |---|---:|---:|
  | sheet layout, 1,000 items ×100 | 1.74 ms | 0.65 ms |
  | exact_fit 6000×4000 → 1500×1000 | 74.0 ms | 54.8 ms |
  | embedded, 96 RAWs warm | 38.3–39.1 ms | 39.6–43.9 ms |

  The embedded gap is within run-to-run noise after the first run; Rust's
  `seek_read` vs the core's `SetFilePointerEx` + `ReadFile` is the one
  difference to look at in Phase 7.
- **Open for Noah:** Go for Phase 4.

### 2026-11-22: Phase 3 Mac side, three slices merged (`core-p3a-sheet`, `core-p3c-media`, `core-p3b-picture`)

Three agents worked in parallel, one per slice, from `00b0d0d`; the
coordinator merged 3a (`9c77ee9`), then 3c, then 3b, and checked the
combined tree after each (`build.py test`, `test --asan`, `diff`,
`diff --asan`, the Windows cross-check with and without `--tests`, and the
any-OS Windows crates with `--locked`). All pass on the merged tree.

**Pattern (as in Phase 2).** `negs-core-rules` (`core/rust/rules`) keeps
each Windows module's public API; plain types stay Rust structs; the logic
runs in Odin. `windows/src/<module>.rs` is now `pub use
negs_core_rules::<module>::*` and keeps its own `#[cfg(test)]` tests; no
other `windows/src` file changed. The Rust originals are frozen as oracles
in `core/diff/oracle` (3b's and 3c's compile in their own crates,
`core/diff/picture` and `core/diff/media-oracle`, so their stand-in
`crate::` modules don't clash).

**Differential tests** (SplitMix inputs, `NEGS_RULES_SEEDS`/`NEGS_RULES_SEED`;
every output compared, floats bit for bit; 0 differences; 0 live core
allocations and 0 bad frees after each test; each one also under ASan):

| Slice | Module | Compared | Mutation check (caught) |
|---|---|---|---|
| 3a | `sheet_rules` | 909k values, 46 functions (+30k Rect) | layout clamp 10→9.5: 5,084 diffs |
| 3a | `tray_rules` | 700k values, 39 functions, all 128 Esc contexts | U+3000 out of White_Space: 3,752 |
| 3a | `compare` | 290k values, 21 functions | `>`→`>=` in arrangement: 931 |
| 3a | `model/compare` (`halved`, `over_frame` geometry) | 20k | `+2`→`+1` rounding: 7,612 |
| 3b | `crop` | 10k + 2.5k 12-step sessions + serde both ways (1.16M values) | inscribe 0.98→0.97: 10,836 |
| 3b | `border` | 10k + every film + serde (496k) | film_geometry 0.025→0.0251: 2,763 |
| 3b | `exact_fit` | 10k sizes, 200 resamples incl. 6000×4000→1500×1000 (98.4M) | encode constant: 101,682 |
| 3b | `histogram` | 10k + 24 MP in 8 orientations (190M) | turn-3 mapping: 4,570 |
| 3b | `negative` | 10k (230k) | tolerance `<=`→`<`: 308 |
| 3b | `thumb_compose` | 10k sizes, 2.5k full thumbnails with real borders (230M) | decode constant: 881 |
| 3b | `windows_units` | the Windows modules' own tests on the core; the app's `CropSession` vs the frozen one (2.5k) | wrapper drops `intended`: caught |
| 3c | `embedded_preview` | 96 real RAWs by path and bytes (98.4 MB of previews), 12k headers (cut, corrupted, synthetic), 403 synthetic files | orientation 1–8→1–7; `largest` tie-break |
| 3c | `photo_facts` | 5.57M camera names (every Unicode scalar), 60k numbers, `read` on 106 real + 300 synthetic files | ties-to-even→half-up |
| 3c | `tone_graph` | 10k curves, 30k predictions, 480k hits, 389k drags | flat run `>0`→`>=0` |
| 3c | `help` | all 91 paths, 15 groups, 20k lookups | `-` accepted in lanes |

3b also passed at seed 20261122 with 40,000 cases each. Two mutations that
survived were shown equivalent (sheet's row-fill and ln-tie comparisons;
exact_fit's `<=` at 0.0031308).

**Bugs the differential tests found** (in the port, fixed before merge):
histogram's `usize::MAX` sample budget became −1 as an Odin `int`, and a
far-off-picture sample (1e300 crop) saturated to −1 and indexed out of
range; the core now compares unsigned, as the Rust does.

**What stayed in Rust, and why.** Engine calls (`CropSettings::engine()`,
`PhotoFacts::read`'s EXIF via spektrafilm-io, `image` resizes and
rotations, PNG strip textures); JSON (the tone graph's `Controls`/`Curve`
parsing, negative's `FlatField::of`/`BaseStatus::of`, border's `options()`)
until the core has JSON (Phase 4); Win32 packing (`colorref`); `Lru` (a
generic container of host values); `CropSession` (holds `EditState`; it
delegates every method to the core's `SessionGeometry`); constants the app
matches on (tested equal to the core's). Engine *formulas* that 3b needed
(border margins, film geometry, the 35mm strip, `Crop::pixels`,
`straightened_size`) are ported to Odin and diff-tested bit for bit against
spektrafilm-io's own functions, so an engine change shows up as a failing
test.

**ABI** (version 4 throughout): ~109 `negs_sheet_*`/`negs_tray_*`/
`negs_compare_*`, ~100 Phase 3b functions, ~30 Phase 3c. No JSON. Text into
caller buffers (with an exact-size retry) or owned `negs_buf_t`; pixels,
bins and previews into caller buffers (`negs_embedded_read` writes straight
into the caller's buffer: 14.0 → 11.9 ms on the bench); static strings for
constants. Layout tables cover every header struct (C, Odin, Rust).

**Speed** (indicative; Docker used ~3 cores): sheet 0.40 ms vs Rust 0.45;
fit 47.1 ms vs 56.0 (byte-identical); embedded 11.85 ms vs 11.7. The first
ports were slower (sheet 0.65 ms, fit 136 ms): bounds checks in the inner
loops (now `#no_bounds_check` with lengths clamped up front) and
`math.round` not inlining across separate modules (now libm's `round`).
`build.py lib` 1.44–1.53 s with every slice in.

**Build tooling.** `diff --asan` passes `--target <host>` so RUSTFLAGS
don't put the ASan runtime into proc macros (rustc aborted with
"Interceptors are not working" once core/diff pulled in serde_derive).
core/diff now builds spektrafilm-io and rawler (for the oracles), so its
first build is slower. Because `negs-core-rules` now brings spektrafilm-io,
`windows/sheet-rules` and `windows/tray-rules` patch `rawler` to
`vendor/rawler` as `library-map` does; the merged tree's check caught it
(crates.io's rawler lacks the X-Trans API). Lock files were resolved offline with `cargo tree
--offline` (`cargo update -p` offline downgrades unrelated crates).

**Coordinator decisions** (the agents' questions; none is a product
trade-off):
- `Lru`'s `len`/`bytes`/`retain` stay `pub` in the wrapper (Windows' tests
  use them).
- `windows/library-map` and `windows/sheet-rules` now use the core's
  library under the `negs-library` name, as the app does (otherwise
  `compare::Item` is a different type).
- `windows/sheet-rules` and `windows/tray-rules` only re-run tests the
  wrapper crate runs too; retire them in Phase 5 with the rest of the
  non-UI Rust. CI still runs them (after `install_odin`).
- core/diff may depend on spektrafilm-io and rawler (read only) so
  `PhotoFacts::read` and the engine formulas are compared on real inputs.
- The tone graph's and negative's JSON stay in Rust until Phase 4.
- Porting engine formulas is accepted when a differential test holds them
  to the engine function bit for bit.
- `negs-core-rules` depends on spektrafilm-io unconditionally (3b needs
  it); 3c's `io` feature is kept as an empty name so `windows/Cargo.toml`
  needs no change. `image` (no default features) comes with it for
  `model_compare`; making it optional can wait.

**Risks to check at the Windows gate:** embedded's Windows reader
(`CreateFileW`/`ReadFile`) has never run (no `\\?\` prefix for long
paths; failures fall back to decoding, no crash); exact_fit's thread count
via `GetSystemInfo`; bit-exactness with UCRT's libm on x86_64 (`build.py
diff` on the PC); `pow` standing in for `exp10` (drags snap to 0.01, so
low risk); a panic in the `cubic_pieces` callback aborts (documented);
border's `strip_textures_load` runs only in `windows/`; photographs compare
byte for byte (a path with mixed `\` and `/` counts as different; the
library never produces one).

### 2026-11-22: Go for Phase 3; build tooling and Phase 3 groundwork (branch `core-p3-base`)

- **Noah's answers:** Go for Phase 3; adopt `-use-separate-modules`; add
  an ASan target. The Phase 2 commits were re-dated to 22 Nov evening at
  his request (merge now `c52e424`).
- **`-use-separate-modules`** for the library and tests: `build.py lib`
  1.5 s (6.8 s whole-program, kept as `--whole-program` for comparing).
  `build.py diff` passes, the marks bit for bit. On the PC (`-build-mode:obj`
  then writes one object per package; `lib.exe` archives them all):
  1.6 s against 7.5 s, and `build.py test` and `build.py diff` pass.
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
