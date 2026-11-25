# Handoff: Coordinating the Odin Revert & Cross-Platform GPUI App (`crates/negs-app` + `app/`)

For the new coordinator agent taking over the Odin unwind and GPUI migration, with access to both **macOS** (local) and **Windows** (`ssh pc`). Linux verification comes later in Phase 7. Written 2026-11-25 against `main` `8cff4c21` (`v0.20.0`).

**Read first:** [gpui-app-plan.md](gpui-app-plan.md) (decision rationale, spike data, exact Odin-revert manifest, architecture, color/sharpness requirements, Phases 0–7). Create and maintain `docs/gpui-app-progress.md` as your live source of truth after every phase and merge.

## The mission

1. **Phase 0:** Cleanly forward-revert the Odin shared-core experiment on `main` (deleting `core/` and Odin CI/build wiring) while **preserving** every release commit (`v0.18.0`–`v0.20.0`), all new film stocks, the `.github/workflows/release.yml` `deploy-feed` fix (`8154a6ac`), and the pure-Rust `windows/src/inspector_view/` (`885e8fb4`).
2. **Phase 1:** Extract the ~24–26k LOC pure-Rust shared app core from `windows/src/` + `crates/negs-library` + `windows/marks` into `crates/negs-app` (plus `crates/negs-marks`), behind a minimal `Platform` trait (~800–1,200 LOC across macOS/Windows/Linux).
3. **Phases 2–6:** Build **one cross-platform GPUI desktop app** (`app/`) on top of `crates/negs-app` — developed daily on macOS (1–2.5 s rebuilds) and verified on Windows PC at every gate — with **Windows shipping first** to replace the lagging Win32/WARP/multi-swap-chain UI, followed by Linux (Phase 7a) and macOS (Phase 7b).

## Hard rules (never violate)

- **Never touch the film engine:** Do not edit `crates/spektrafilm-*`, `crates/negs-libraw`, `vendor/`, or `web/`. Exports and all 496 golden renders are sacred.
- **Never `git reset` `main`:** `origin/main` already has `v0.18.0`–`v0.20.0` and ~30 new film stocks merged. Revert Odin via a clean forward commit.
- **Commit on local `main`, never push** unless Noah explicitly asks. Merge phase branches with `--no-ff` (`Merge gpui-pN-<name>: what it does`).
- **Commit dates:** Each commit landing on `main` is dated `main`'s tip + 1 min (`GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE`). Nothing on `main` may be dated 08:00–16:00 Europe/Berlin.
- **Don't touch:**
  - Noah's installed Negs (`dist/negs-macos` on Mac, `%LOCALAPPDATA%\Programs\Negs` on PC) or his real profiles.
  - The main Windows checkout (`C:\Users\Noah\Projects\negs`) — always use a dedicated worktree under `C:\Users\Noah\Projects\negs\worktrees\<name>` via `C:\Users\Noah\.negs\wt.cmd` (or scratch dir for spikes).
  - Noah's real photo folders — copy test RAWs (`C:\Users\Noah\Downloads\102ND750`, `C:\Users\Noah\Downloads\raws`, `docs/card.jpg`) into scratch folders before running sidecar/library tests.
- **Color & sharpness non-negotiable:** Never display raw un-downscaled photos through GPUI's bilinear `img()` atlas (use exact-fit Lanczos to 1:1 physical screen pixels), and never send untagged sRGB pixels straight to macOS GPUI's untagged `CAMetalLayer` (convert Rec.2020/sRGB → active monitor ICC / macOS `NSScreen.colorSpace`, or use color-tagged `IOSurface` via GPUI `surface()`).

## Machines & environment

### Mac (local): `~/Projects/negs`
- **Rust:** `/Users/noahhaasis/.nix-profile/bin/{rustc,cargo}` (`1.93.1`; ensure it's on PATH). Remember `.claude/settings.json` sets `CARGO_PROFILE_RELEASE_LTO=false`, `CODEGEN_UNITS=16`, `INCREMENTAL=true`, `MACOSX_DEPLOYMENT_TARGET=14.0` for fast local release builds.
- **Swift tests (Phase 0 & until Phase 7b):** `swift test --package-path macos --scratch-path target/swift-native` (after building engine helpers in `target/release`).
- **macOS dev build:** `scripts/package_macos_app.sh --dev` (into `dist/negs-macos-dev/Negs.app`; never touch `dist/negs-macos`).
- **Windows cross-check from Mac:**
  ```
  cargo check --release --locked --target x86_64-pc-windows-msvc \
    --manifest-path windows/Cargo.toml --target-dir target/wincheck
  ```
- **Worktrees:** Always pass a separate `--target-dir` per worktree; never share `target/` across parallel sessions.
- **Reference spike code (from 2026-11-24/25):**
  - Prototype + benchmark harness live in `/Users/noahhaasis/.gemini/antigravity-cli/brain/acd93f4d-4a60-4c35-bdcb-83d2f494df65/scratch/ui-spike/` (and on PC in `C:\Users\Noah\.negs\scratch\gpui-spike\`).

### Windows PC: `ssh pc`
- **Specs:** Windows 10 22H2 (`DESKTOP-CAGU879`), Ryzen 5 3600, 16 GB RAM, Vulkan + DX11 GPU.
- **One PC rule:** Only one agent builds or runs GUI tests on `ssh pc` at a time (16 GB RAM). Hand out PC ownership explicitly in subagent briefs.
- **Shell:** `cmd.exe` over SSH (`cd /d`, `&&`, no `grep`/`tail`; use `findstr` or filter on Mac). Cargo is at `C:\Users\Noah\.cargo\bin` (not on default SSH PATH — prepend `set PATH=C:\Users\Noah\.cargo\bin;%PATH%`).
- **Engine helpers on PC:** `C:\Users\Noah\Projects\negs-presets` (set `SPEKTRAFILM_ROOT=C:\Users\Noah\Projects\negs-presets` or build in worktree).
- **Interactive GUI runs (CRITICAL):** SSH runs in Session 0 (headless — no desktop / DWM). To capture screenshots or run real startup/FPS benchmarks, launch in Noah's interactive Console Session 1 via a temporary scheduled task:
  ```cmd
  schtasks /create /it /ru Noah /sc once /st 00:00 /tn negs-gpui-test /tr "<cmd>" /f
  schtasks /run /tn negs-gpui-test
  ```
  Always delete the scheduled task afterwards (`schtasks /delete /tn negs-gpui-test /f`), pass `NEGS_PROFILE_DIR=<scratch>` + `NEGS_ALLOW_MULTIPLE=1`, keep window appearances brief, and close the app cleanly at the end. See `C:\Users\Noah\.negs\shot.ps1` and `C:\Users\Noah\.negs\startup.ps1` for working patterns.
- **If `ssh pc` times out:** The PC may be asleep or booted into Linux — ping Noah rather than retrying endlessly.

## How to run the phases

1. **Start with Phase 0 (Odin revert) yourself** — it's surgical and fast, and establishes the clean pure-Rust baseline before branching any subagents.
   - Verify Mac (`swift test`, workspace `cargo test`, Windows `cargo check`) AND Windows PC (`windows/` `cargo test`, `library-map`, `sheet-rules`, `tray-rules`).
   - Merge to local `main` and initialize `docs/gpui-app-progress.md`.
2. **Phase 1 (`crates/negs-app` extraction):**
   - Audit every Win32 import in non-UI `windows/src/*.rs` (most files already have 0 Win32 imports; the remaining ~10 have only 1–11 touchpoints).
   - Keep `windows/` compiling against `crates/negs-app` at the end of Phase 1 so the shipping Windows app isn't broken while `app/` is built.
   - Run full `crates/negs-app` tests (including `lifecycle_tests` + `inspector_view` 55 tests) natively on **both** macOS and Windows PC.
3. **Phase 2 (GPUI shell + color-managed canvas):**
   - Lock the GPUI dependency (`gpui 0.2.2` vs pinned upstream Zed commit — prefer `0.2.2` unless a concrete blocker hits; check `lld-link` on Windows for faster local rebuilds).
   - Build the color-managed exact-fit Lanczos photo canvas FIRST and visually verify macOS Display P3 and Windows monitor ICC color accuracy + 1:1 sharpness before building all the panels.
4. **Phases 3–5 (UI feature parity):**
   - Split independent UI areas across parallel subagents once Phase 2's shell, theme tokens, and shared widgets (`InkSlider`, `ToneGraphCanvas`, `MarksOverlay`) are merged:
     - Phase 3: (3a) Inspector (`inspector_view` renderer) + (3b) Presets sidebar & Film/Paper Stock Tray.
     - Phase 4: Filmstrip + Contact-Sheet Grid + `SheetFlight` animation (benchmark 60 fps on Windows PC!).
     - Phase 5: (5a) Compare + Crop + Negative UI + (5b) Export, Batch Export, Settings, Lightroom, Alerts.
   - Inspect every diff (`git diff --no-ext-diff`, because `git diff` uses difftastic here) and run Mac + Windows screenshot & FPS checks at each phase gate.
5. **Phase 6 (Windows cutover):**
   - Run full ABBA comparison on Windows PC vs old Win32 `Negs.exe` (startup, grid scroll/flight fps, tab-switch latency, slider drag fps, peak RAM, binary size, side-by-side screenshots across all themes).
   - **Ping Noah for Go/No-Go** before retiring `windows/src/ui/` and updating `windows/installer.iss` + `.github/workflows/release.yml`.
6. **Phase 7 (Linux & macOS):**
   - **7a Linux:** Verify on Linux once Noah gives Linux access; retire `linux/src/` GTK4.
   - **7b macOS:** Polish macOS native menus, Finder open-with, trackpad gestures, Sparkle updates, universal DMG; **ping Noah for Go/No-Go** before retiring `macos/` SwiftUI.

## When to ping Noah

Do **not** ask Noah about routine engineering decisions — decide, document the choice and reason in `docs/gpui-app-progress.md`, and keep moving. **Ping Noah only at these checkpoints:**
1. **If `ssh pc` is unreachable** when a Windows gate is ready.
2. **End of Phase 2 (Canvas & Color Checkpoint):** Show side-by-side Mac P3 + Windows ICC screenshots and real startup numbers so Noah confirms visual quality and feel before building all remaining panels.
3. **End of Phase 4 (Windows Smoothness Proof):** Share measured Windows grid scroll / `SheetFlight` / inspector frame-time numbers confirming the user-reported regressions are gone.
4. **End of Phase 6 (Windows Cutover Go/No-Go):** Full report + request approval to replace `windows/` and ship GPUI on Windows (and to request Linux access for Phase 7a).
5. **End of Phase 7b (macOS Cutover Go/No-Go):** Request approval before retiring SwiftUI `macos/`.
6. **Real product/UX trade-offs** (recommend one concrete option).

## Subagent brief template

> You are implementing Phase N (<name>) of Negs' pure-Rust shared core & cross-platform GPUI app (`crates/negs-app`, `app/`).
> First run `git reset --hard <current local main sha>`, then read `docs/gpui-app-plan.md`, `docs/gpui-app-coordinator.md`, and `docs/gpui-app-progress.md`.
> Scope: <exact modules, widgets, and behaviours>.
> Reference implementation: <exact `windows/src/` and `macos/Sources/NegsApp/` files>.
> Done means: <exact unit tests, Mac + Windows builds, screenshots, and frame-time / startup checks>.
> Machine access: Mac only / You own the PC (`ssh pc`) until you report.
> Commit on branch `gpui-pN-<name>` in small steps. Do not merge or push.
> Report: what changed, test counts, build times, measured frame/startup timings (if applicable), screenshots, and any trade-offs.
