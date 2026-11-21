# Handoff: coordinating the shared Odin app core (`core/`)

For the coordinator agent running the shared app core port from start to finish, mostly without supervision. Written 2026-11-21 against `main` `a9d64f6`.

**Read first:** [shared-core-plan.md](shared-core-plan.md) (architecture, C ABI rules, phases, differential gates, Phase 7 benchmark matrix). Then [shared-core-progress.md](shared-core-progress.md).
The progress log is the source of truth for what is done. Update it after every phase and every merge so you can resume after compaction or a new session from the log alone.

## The mission

Extract and port Negs' shared non-UI application core (~25k–29k LOC currently duplicated across `windows/src/`, `macos/Sources/`, and `linux/src/`) into **Odin** (`core/`), exposed via a clean C99 ABI (`core/include/negs_core.h`).

- **Leave the Rust film engine untouched** (`crates/spektrafilm-*`, `crates/negs-libraw`, `web/`). Correctness of the engine is sacred.
- **Prove correctness via differential testing** against the existing Rust crates (`crates/negs-library`, `windows/marks`, `windows/src/*`) and Swift tests before deleting any superseded code.
- **Do all Mac-side work first; ping Noah only when the Windows PC is needed** (see Windows Checkpoints below).
- **Measure & optimize at the end (Phase 7):** compare Odin vs. Rust on compile time, startup latency, runtime speed, and memory usage (peak RSS & allocations).

## How you work

You are the coordinator. Run Phases 0 → 7 in order, either yourself or by spawning one subagent per phase (or per independent slice of a phase). Between phases:

1. Write the phase agent's brief (template below).
2. Inspect its diff (`git diff --no-ext-diff`, because `git diff` is difftastic here).
3. Verify its differential tests, leak checks (`mem.Tracking_Allocator`), and platform tests.
4. Merge it to local `main` with `--no-ff`, then update `docs/shared-core-progress.md`.

Small phases or glue fixes are cheaper to do yourself than to brief.

### Parallelism (only where it pays)

- **Phase 1 (`marks` + `theme`) and Phase 2 (`library`)** touch disjoint packages inside `core/` and disjoint tests—run them in parallel after Phase 0 is merged.
- **Phase 3** splits into three independent slices (run up to 3 in parallel after Phase 2's Go/No-Go):
  - **3a:** `sheet_rules`, `tray_rules`, `compare` + `model/compare`
  - **3b:** `crop`, `border`, `exact_fit`, `thumb_compose`
  - **3c:** `embedded_preview`, `photo_facts`, `tone_graph`, `histogram`, `negative`, `help`
- **Phase 4 (`state` + `presets` + inspector schema)** and **Phase 5 (`engine` + `model`)** run sequentially (Phase 5 builds on Phase 4).
- **One PC rule:** Only one agent builds or runs GUI tests on `ssh pc` at a time (16 GB RAM). Hand out the PC explicitly in briefs: "you own the PC until you report".

### Subagent worktrees

`isolation: "worktree"` may branch from `origin/main`, which can be behind local `main`. Every brief starts by telling the agent to run `git reset --hard <current local main sha>`. Worktree sessions refuse Bash with `$VAR`, loops, or `bash script-that-runs-git`: put multi-step shell logic in scratch script files and invoke them with literal paths.

## When to ping Noah

Do **not** ask Noah about routine engineering decisions—decide, log the decision and reason in `docs/shared-core-progress.md`, and keep moving. **Ping Noah only at these points:**

1. **If Odin cannot be installed on this Mac** (e.g., Homebrew/network unavailable in Phase 0—stop and ask him to install Odin and pin the version).
2. **End of Phase 2 — First Windows PC ping + Go/No-Go checkpoint:**
   - Before pinging, finish **all Mac-side work** for Phases 0, 1, and 2: Odin `marks`, `theme`, and `library` built, tested with 0 leaks, `marks`/`theme` wired into macOS Swift (`CNegsCore`) and passing `swift test`, and all `library` differential tests (`docs/sidecar-fixtures/` + `NEGS_LIBRARY_SEEDS=50` vs. Rust `negs-library`) passing on the Mac.
   - Then ping Noah: ask him to connect/wake the Windows PC (`ssh pc`) and confirm Odin can be installed there so you can run the Windows Phases 0–2 integration gate, startup measurement, and Mac↔Windows sidecar round-trip.
   - Present the **Phase 2 Go/No-Go report** (real Odin compile times, Swift + Windows FFI ergonomics, startup impact, any agent friction) and get his go-ahead before starting Phase 3.
3. **End of Phases 3, 4, 5, and 7 (Windows PC integration / benchmark gates):** Batch all Mac-side slices of a phase and pass every Mac differential test *first*, then ping Noah if `ssh pc` is unreachable or booted into Linux.
4. **Real product/ABI trade-offs** that change user-visible behaviour or platform scope (recommend one option rather than dumping an open-ended list).

## Rules

- **Commit on local `main`, never push.** Merge phase branches with `--no-ff` (`Merge core-pN-<name>: what it does`), or commit small things directly.
- **Commit dates:** Each commit landing on `main` is dated `main`'s tip + 1 min (`GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE`). Nothing on `main` may be dated 08:00–16:00 Europe/Berlin. Branch commits made in those hours move to tip + 1 min onward; keep real dates outside them.
- **Never change engine behaviour:** Do not edit `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, or `web/`. If you think an engine change is needed, stop and flag it—almost certainly the adaptation belongs in `core/`.
- **Keep the Rust oracle until the differential gate passes:** Never delete `crates/negs-library`, `windows/marks`, or non-UI files in `windows/src/` until the Odin replacement is wired in and passes both differential tests and the platform gate.
- **Don't touch:**
  - Noah's installed Negs (`dist/negs-macos` on Mac, `%LOCALAPPDATA%\Programs\Negs` on PC) or his real profiles.
  - The main Windows checkout (`C:\Users\Noah\Projects\negs`).
  - Noah's real photo folders. Sidecar and library tests run only on copies in scratch directories.

## The machines

### Mac (here): `~/Projects/negs`
- **Odin:** Check `odin version` (install via `brew install odin` in Phase 0 if missing; record the exact release tag in `docs/shared-core-progress.md`).
- **Rust:** Ensure `~/.cargo/bin` is on PATH. Use a separate `--target-dir` per worktree; don't share the main checkout's `target/`.
- **Swift tests:** `swift test --package-path macos --scratch-path target/swift-native` (after `--dev` or building `libnegs_core.a`).
- **macOS dev build:** `scripts/package_macos_app.sh --dev` (Apple silicon, `dist/negs-macos-dev/Negs.app`). Never overwrite `dist/negs-macos`.
- **Windows cross-check (Rust side):**
  ```
  cargo check --release --locked --target x86_64-pc-windows-msvc \
    --manifest-path windows/Cargo.toml --target-dir target/wincheck
  ```
  *(Note: once `windows/` links `negs_core.lib`, `cargo check` validates Rust types/FFI signatures without needing the Windows linker; full linking runs on `ssh pc`.)*

### Windows PC: `ssh pc` (only when needed at gates)
Same setup documented in [docs/windows-parity-coordinator.md](windows-parity-coordinator.md#L89-L127):
- Windows 10 22H2, Ryzen 5 3600, 16 GB RAM, `cmd.exe` over SSH (`cd /d`, `&&`, no `tail`/`grep`).
- Engine binaries live in `C:\Users\Noah\Projects\negs-presets` (`SPEKTRAFILM_ROOT=C:\Users\Noah\Projects\negs-presets`).
- Use a dedicated worktree on the PC (e.g., `C:\Users\Noah\Projects\negs-core`) fetching from remote `mac`.
- Install the **same pinned Odin version** on the PC (e.g., `C:\Users\Noah\.negs\odin` added in build `.cmd` scripts alongside `C:\Users\Noah\.cargo\bin`).
- Always run GUI tests with `NEGS_PROFILE_DIR` pointing to a scratch dir and `NEGS_ALLOW_MULTIPLE=1`.
- Screenshots: `C:\Users\Noah\.negs\shot.ps1` via scheduled task (`schtasks`). Startup: `windows/measure_startup.ps1`.
- If `ssh pc` times out, the PC may be asleep or booted into Linux—ping Noah.

## Gates

| After | Gate |
|---|---|
| **Phase 0** | Odin version pinned; `core/` builds static lib on Mac; leak-checked test runner passes (`Tracking_Allocator` fails intentionally on a synthetic leak test, then passes clean); `CNegsCore` smoke test passes in `swift test`; **baseline metrics table** recorded in `docs/shared-core-progress.md` (compile times, Rust `negs-library` & hot-loop timings, peak RSS, Windows startup baseline). |
| **Phase 1** | 10,000-case differential test (`core/marks` vs. Rust `windows/marks`) exact match; 5-theme tokens & contrast ratios match `Theme.swift`; `macos/` wired to `CNegsCore` for marks/theme and `swift test` passes; 0 leaks. |
| **Phase 2** | All `docs/sidecar-fixtures/` match `crates/negs-library/tests/mac-encoded/` **byte-for-byte**; 50-seed differential random state test vs. Rust `negs-library` passes with 0 diffs and 0 leaks; **Windows PC gate** (`windows/` links `negs_core.lib` for Phases 1+2, `cargo test` passes, Mac↔Win sidecar round-trip passes, screenshots match, `measure_startup.ps1` within noise); **Go/No-Go review with Noah**. |
| **Phase 3** | Every pure module passes its Rust differential harness on Mac (0 leaks); `windows/` switched to call `core/` for Phase 3 modules; PC build + tests + screenshots + startup pass. |
| **Phase 4** | Presets, session, edit state, and data-driven Inspector ViewModel pass differential tests vs. Rust; `windows/` wired and verified on PC. |
| **Phase 5** | Frame-pipeline differential tests (`NEGS_PIPELINE_SEEDS=25`) and `lifecycle_tests.rs` scenarios match Rust `model.rs` against real `spektrafilm-native`; superseded ~28k LOC of non-UI Rust deleted from `windows/src/`; full PC test suite + screenshots + ABBA startup pass. |
| **Phase 6** | **6a:** `linux/` links `libnegs_core.a` and replaces its duplicated non-UI modules cleanly. **6b:** `macos/` adopts `CNegsCore` for `negs-library` and pure rules with `swift test`, `LibraryStateTests`, and `FramePipelineTests` green. |
| **Phase 7** | Full Rust-vs-Odin comparison report logged in `docs/shared-core-progress.md` (compile times, startup medians, runtime benchmarks, peak RSS, allocation counts, binary sizes) + targeted optimization pass on any bottleneck where Odin trails Rust. |

## Odin coding & ABI checklist (give to every worker)

- Pin exact Odin flags in `core/build.py`: debug tests with `-debug` + `mem.Tracking_Allocator`; release static lib with `-build-mode:static -o:speed -no-bounds-check` only *after* bounds-checked release is benchmarked (keep bounds checks on by default until Phase 7 proves where disabling them matters!).
- Keep `core/include/negs_core.h` strictly valid C99 (`clang -std=c99 -Wall -Wextra -Werror -fsyntax-only core/include/negs_core.h` runs in every test!).
- Verify struct layout compatibility with a tiny C test program (`sizeof` / `offsetof` assertions matching Odin's `size_of` / `offset_of`).
- Use explicit `base:runtime` / `context.temp_allocator` discipline: reset temp allocator at FFI entry boundaries or per frame, never return temp-allocated pointers past the next call without documenting it in `negs_core.h`.
- Keep modules small and clean—don't recreate a single 5,000-line file like `model.rs`; split `model/` by concern (`pipeline.odin`, `undo.odin`, `crop_flow.odin`, `compare_flow.odin`, `tray_flow.odin`, `batch_flow.odin`).

## Brief template for a phase agent

> You are implementing Phase N (<name>) of Negs' shared Odin app core (`core/`).
> First run `git reset --hard <current local main sha>`, then read `docs/shared-core-plan.md`, `docs/shared-core-coordinator.md` (rules, ABI checklist, gates), and `docs/shared-core-progress.md`.
> Scope: <exact modules & C-ABI surface>.
> Reference oracle: <exact Rust/Swift files and tests>.
> Done means: <exact differential tests, leak check, C99 header check, and platform tests>.
> Machine access: Mac only / You own the PC (`ssh pc`) until you report.
> Commit on branch `core-pN-<name>` in small steps. Do not merge or push.
> Report: what changed, differential test counts & seeds, leak/sanitizer output, compile time of `core/`, any ABI additions or trade-offs, and questions for Noah (if any).

## Done means

1. `core/` in Odin owns the shared non-UI app core (`marks`, `theme`, `library`, domain rules, state/inspector schema, `EditorModel` + `EngineClient`) behind `core/include/negs_core.h`, with zero engine changes.
2. Differential tests and end-to-end platform suites prove correctness on macOS, Windows, and Linux with zero leaks.
3. Phase 7's comparison table documents measured compile times, startup latency, runtime speed, and memory usage (peak RSS & allocations) against the Rust baseline, with optimizations landed where they pay off.
4. Everything is merged cleanly on local `main`, nothing is pushed, and Noah gets the final report.
