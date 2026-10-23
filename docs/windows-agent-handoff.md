# Windows port: handoff for the Win32 UI agent

You are building the **Win32 app** for Negs. You work directly on the Windows
test box, in Noah's logged-in desktop session, in the checkout at
`C:\Users\Noah\Projects\negs` (branch `worktree-cross-platform-port`). The
engine side of the port is done and validated on both machines. The app
itself has not been started.

Noah's priorities, in order: **correctness**, **performance**, **launch speed**
(a hard requirement for the UI). The **GPU path is what ~99 % of users run**.
Dark mode does not matter on Windows. Windows comes before Linux/Omarchy.

Read first, in this order:

1. `docs/windows-ui-features.md`: every macOS feature, its behaviour (the
   contract), what to do instead on Windows, and what not to port. **§20
   lists the decisions that are Noah's**; see "Before you build" below.
2. `docs/cross-platform-port.md`: plan, risks, measurements, decisions.
3. `docs/native-macos-ui.md`: how the Mac app reaches its startup times.
4. `scripts/golden/README.md`: the correctness harness and its gates.

## Before you build

**Blocking:** §20 item 1. Noah was to run a startup spike (blank window +
~40-slider inspector, cold and warm, Win32 vs WinUI 3) and give a go-ahead
on the UI plan. Ask him for the result. If he hasn't run it, offer to run it
yourself now that you are in the desktop session, and wait for his yes
before building the app.

The other §20 items can wait until you reach their feature, but ask before
building them. Proposed answers (Noah has **not** decided these):

| §20 | Decision | Proposal |
|---|---|---|
| 2 | Preview colour on wide-gamut monitors (§6) | Convert to the monitor's ICC profile (matrix/TRC profiles) in the presenting shader; plain sRGB when the profile can't be used. Windows 11 Advanced Color later. |
| 3 | Show Original for RAW (§7) | A helper command that renders the rawler decode with no film (same framing as the engine by construction). Not the embedded JPEG: its framing differs. |
| 4 | Export options (§11) | `IFileDialogCustomize` in the save dialog; a separate options dialog only if Size can't be expressed there. |
| 5 | Crop key, Show Original key (§2, §7) | `C` for crop (no modifier, not in text fields); keep `\` by `WM_CHAR` and add a modifier-free second key. |
| 6 | Updater and installer (§4, §15) | WinSparkle (same appcast and EdDSA scheme as Sparkle) with a per-user installer that registers file associations; winget later, pointing at the same installer. |

## State at handoff (October 23)

Commits on `worktree-cross-platform-port`, none pushed or merged:

| Commit | What |
|---|---|
| e3d0bb8 | Full-resolution performance/capacity measurements; persistent Vulkan pipeline cache |
| c7e762a | `docs/windows-ui-features.md` |
| f27722c | Windows GPU capacity admission with whole-pipeline CPU fallback; CLI and helper report the backend that actually rendered |
| (next) | Validation manifests/reports at f27722c, docs, this handoff |

Validation at f27722c (details: `docs/cross-platform-port.md`, "Reporting the
backend that actually rendered"): the golden matrix is **484/484
byte-identical** to the previous same-machine checkpoint on **both**
machines. The native bridge test passes on both, including Windows with
`SPEKTRAFILM_BACKEND=wgpu` and `SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB=0`, which
reports `CPU (rayon)`. Forced image and FFT fallbacks are byte-identical to
explicit CPU rendering.

What this means for the app:

- **Two export paths**, both to port (`macos/Sources/NegsApp/Engine.swift`):
  the `spektrafilm-native.exe` JSON-lines helper serves the preview and
  `Save` of the cached render, and the Reference export launches
  `spektrafilm-f64.exe` separately. Fast export saves the helper's preview at
  export size; the bridge test proves that equals `spektrafilm.exe` output.
- **Backend reporting.** The CLI's `Backend:` line is printed after the
  render; the helper's `render` reply has `backend`. Both name what actually
  rendered. On Windows a render that fails the capacity check says
  `CPU (rayon)` even though wgpu was requested: show it as a CPU fallback.
- **Capacity.** On the GTX 1660 SUPER the admission budget is 4772 MiB
  (80 % of the largest device-local heap): four RGB f32 images plus
  128 MiB, so up to about 100 MP renders on the GPU (96 MP measured) and
  larger images on the CPU; diffusion filters lower that. CPU fallback is
  slow: the 180 MP test took 272 s. Exports need progress and a working
  Cancel (Job Object, §16). The Mac never falls back.
- **Watchdog/device loss is not handled.** Memory admission is not
  watchdog safety. A driver reset (System event 4101) can still happen on
  long GPU work that fits in memory; the helper may then die, hang or return
  an image of unknown validity. Give every helper request a timeout, restart
  the helper on death (§16), and never present an export as good just
  because the process exited 0. Fixing this in the engine is a separate,
  open task: don't take it on without Noah.
- **Pipeline cache.** `%LOCALAPPDATA%\Negs\Cache\gpu`; isolation and
  disable switches are in `scripts/golden/README.md`. No warm-start gain was
  measured on this NVIDIA driver; reboot-cold startup is unmeasured.
- **Startup.** Measure three parts: first painted frame, first interaction,
  first usable image. Include helper start, catalog load and shader
  compilation. Earlier measurements here ran over SSH in the services
  session; measure the app in the desktop session.

Build the engine tools (three separate invocations: never enable
`precision-f64` in a build that also produces the f32 binaries, Cargo feature
unification would change their precision):

```bat
cargo build --release --locked -p spektrafilm-cli --bin spektrafilm --bin golden_compare --bin golden_probes --bin golden_inputs
cargo build --release --locked -p spektrafilm-native
cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
python -I scripts\check_native_macos.py --helper target\release\spektrafilm-native.exe ^
  --exporter target\release\spektrafilm-f64.exe --fast-exporter target\release\spektrafilm.exe ^
  --expected-backend wgpu
```

The script keeps its historical name; it runs on both platforms.

## Decisions already made: do not reopen

- **Cross-platform correctness = tight numeric delta**, gated per engine in
  `scripts/golden/gates.json`; exact hashes within one machine and build. No
  portable-math rewrite.
- **Auto-exposure is computed in f64 and rounded once** (f596979). Noah accepted
  that about 0.6 % of auto-exposed CPU exports on macOS move by one ulp of
  exposure.
- **GPU grain differs slightly between GPU vendors** (the samplers use the GPU's
  `log`/`exp`). Accepted, gated statistically by `gpu_grain`.
- **Vulkan only on Windows and Linux, Metal on macOS; no DX12** (312d24f).
  wgpu's `dx12` cargo feature is off (214ed09). Without Vulkan the CPU engine
  renders.
- **Windows UI: Win32 with the standard common controls**, light mode, wgpu
  rendering straight into the HWND for the image, pending the spike result
  above.
- **Capacity: admission with whole-pipeline CPU fallback**, Windows only;
  Metal unchanged.
- The egui GUI (`crates/spektrafilm-gui`) and `scripts/package_windows_app.ps1`
  (CUDA build) are legacy. Do not build the Windows app on them.

**Deferred** (don't act without Noah): f32 CPU grain diverges across platforms
(one shared MT19937 stream; 82 FAIL-GRAIN in `compare`, expected). This now
also covers Windows exports that fall back to the CPU.

## Rules

- **Never loosen a gate to make a run pass.** Find the first divergent stage
  (`probe`, `compare-probe`) and bring evidence.
- **Any change to macOS output needs Noah's approval.** Speed must not come from
  approximations: Noah declined that before. Exports stay accurate.
- After any change to shared engine code (anything outside the new Windows
  app), re-run the golden matrix on **both** machines and compare `--exact`
  against the latest same-machine manifest (`scripts/golden/README.md` →
  Run). Changes that only add the Windows app still need `cargo build` and
  `cargo test` to pass on the Mac, so macOS keeps building.
- The repo is **public**: no dumps, RAWs, machine access details or credentials
  in git or releases.
- Commit on a branch of your own (for example `windows-ui`, from
  `worktree-cross-platform-port`). Don't push to GitHub or merge without
  Noah's go-ahead. Commits that land on main are dated one minute after
  main's tip (author and committer).

## The machines

**Windows (you are here):** Windows 10 22H2 (19045), Ryzen 5 3600 (6 cores /
12 threads), GTX 1660 SUPER (6 GB, driver 610.47, Vulkan), MSVC, Rust 1.93.1
(`C:\Users\Noah\.cargo\bin`), Python 3.9 (standard library only), Git for
Windows 2.41. Sleep on AC is off. The checkout holds the golden inputs, its
own dumps (`target\golden\dumps\windows-*`) and the original Mac reference
dumps (`target\golden\dumps\macos-arm64`). Private diagnostics and logs are
in `target\windows-perf\` (including a backup of the checkout's pre-commit
working tree, `windows-dirty-214ed09.patch` and `.tar`, now superseded by
f27722c).

**Mac:** M1 Pro, the reference platform. The Mac app is the behaviour
reference; the Mac golden manifests and dumps are the output reference.

**Git:** the checkout's `origin` is the Mac's repository over SSH, no more
bundles. `git fetch origin` brings Mac commits; push your own **new**
branch to it. The Mac has `main` and `worktree-cross-platform-port` checked
out (git refuses pushes to those), and Noah or other sessions may be
working there.

**Reaching the Mac to verify macOS output:** SSH with a dedicated key that
works only from this machine. Host, user, key, the helper
`%USERPROFILE%\.negs\mac-ssh.cmd`, the Mac's PATH quirk and the exact
verification recipe are in `%USERPROFILE%\.negs\mac-access.md`, outside the
repo. In short: build and run in **a Mac worktree of your own**, never in the
main checkout or another session's worktree; run the golden matrix there with
the Mac inputs and compare `--exact` against the latest Mac manifest and its
dumps. Things that need the Mac app on screen need Noah. Keep all access
details in that private file, not in this document or any commit.

The reverse direction still exists: a Mac session can reach this box with
the helpers in the Mac worktree's ignored `target/windows-remote/`. From now
on this checkout is yours; Mac sessions should not change it.

Gotchas:

- **Line endings.** Python on Windows writes CRLF, and `.gitattributes`
  normalises it. Git for Windows would otherwise check data files out with
  CRLF and change their hashes.
- **System32 OpenSSH under git hangs.** The checkout's `core.sshCommand`
  uses Git for Windows' bundled `ssh.exe`; keep it.
- **cmd.exe** for scripts run over SSH: `&` always continues, `&&` only on
  success; `findstr` reads `/184` as an option. Redirect output to a file.
- **Run length.** A full golden `run` takes about 11 minutes here
  (f64 mostly) and about 2.5 minutes on the Mac. Redirect long jobs' output
  to a log so completion can be checked later.
- A 178.95 MP diagnostic once stalled after a driver reset until killed:
  give GPU experiments a bounded `--timeout`. Do not increase Windows TDR
  timeouts.

## Open beyond the UI (not yours unless Noah asks)

- Watchdog and device-loss recovery, including long-running effects that fit
  in memory; shorter submissions with unchanged shader math and exact-output
  checks.
- f64 speed on Windows: inspect the WPR stacks (`target\windows-perf\f64-cpu.etl`)
  before changing the triple-loop dgemm.
- The GPU gates come from Apple and NVIDIA only; AMD, Intel and software
  Vulkan may need more room: measure, then propose.
- Linux: if the only Vulkan device is llvmpipe/lavapipe, wgpu picks it over
  the CPU engine. Decide whether to reject `DeviceType::Cpu` adapters.

History and measurements: `docs/cross-platform-port.md`. Public reports:
`scripts/golden/reports/` (performance: `windows-performance-2026-10-23.json`;
capacity: `windows-capacity-guard*.json`; final source:
`*-backend-report*.json`).

## Reporting back

Commit manifests, reports and docs (not dumps) to your branch. Summarise for
Noah: measured numbers, what changed, every non-PASS status with its
explanation, and open decisions with a recommendation.
