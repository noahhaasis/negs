# Windows port: handoff for the next agent

You are continuing the Negs Windows port. Correctness is done: the engine
matches the Mac within calibrated gates on a real Windows GPU. What is left is
**performance**, **startup**, and the **Win32 app**.

Noah's priorities, in order: **correctness**, **performance**, **launch speed**
(a hard requirement for the UI). The **GPU path is what ~99 % of users run**.
Dark mode does not matter on Windows. Windows comes before Linux/Omarchy.

Read first: `docs/cross-platform-port.md` (plan, risks, decisions) and
`scripts/golden/README.md` (the correctness harness, its gates, and the Mac and
Windows results).

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
  rendering straight into the HWND for the image. Noah ran or is running a
  Win32 vs WinUI 3 startup spike: ask for its result before building the app.
- The egui GUI (`crates/spektrafilm-gui`) and `scripts/package_windows_app.ps1`
  (CUDA build) are legacy. Do not build the Windows app on them.

**Deferred** (don't act without Noah): f32 CPU grain diverges across platforms
(one shared MT19937 stream; 82 FAIL-GRAIN in `compare`, expected).

## Rules

- **Never loosen a gate to make a run pass.** Find the first divergent stage
  (`probe`, `compare-probe`) and bring evidence.
- **Any change to macOS output needs Noah's approval.** Speed must not come from
  approximations: Noah declined that before. Exports stay accurate.
- The repo is **public**: no dumps, RAWs, machine access details or credentials
  in git or releases.
- Commit on branch `worktree-cross-platform-port` only. Don't push or merge
  without Noah's go-ahead. Commits that land on main are dated one minute after
  main's tip (author and committer).

## State of the branch

| Commit | What |
|---|---|
| 018c226 | Cross-platform golden test and the port plan |
| f596979 | Auto-exposure identical across platforms; `.gitattributes` LF; harness hardening |
| e1ca2a2 | Gates calibrated on Windows; Windows manifest and results |
| 316824c | Decisions recorded |
| 312d24f | Vulkan or Metal only |
| 214ed09 | wgpu `dx12` feature dropped (Windows `spektrafilm.exe` 16.6 → 16.0 MB) |

Verified at 214ed09: all 184 GPU cases byte-identical to the committed
manifests on both machines.

## The machines

**Mac (where this was driven from):** M1 Pro, worktree
`.claude/worktrees/cross-platform-port`. The Mac reference dumps are in
`target/golden/dumps/macos-arm64/`, the inputs in `target/golden/inputs/`, and
the source RAWs in `~/Downloads/"Z 6"/` (Nikon Z 6 NEFs, 24.3 MP) and
`~/Downloads/7858115920.dng`.

**Windows test box:** Windows 10 22H2 (19045), Ryzen 5 3600 (6 cores / 12
threads), GTX 1660 SUPER (6 GB, driver 610.47, Vulkan), MSVC, Rust 1.93.1,
Python 3.9 (standard library only). Sleep on AC is off. The repo is at
`C:\Users\Noah\Projects\negs` and has no git remote: code arrives as a git
bundle. It already holds the Mac reference dumps, its own dumps
(`target/golden/dumps/windows-gtx1660s-vulkan`) and the golden inputs.

**Access:** SSH with a key from the Mac. The host, user and key are in Claude's
project memory (`linux-omarchy-port-plan`), not here. Never ask for or accept a
password in chat. Helpers, git-ignored, in this worktree:

- `target/windows-remote/win-ssh.sh '<cmd.exe command>'` runs a command there.
- `target/windows-remote/win-sync.sh [--no-build]` fast-forwards the Windows
  checkout to this branch with a bundle, then rebuilds the golden binaries.
- `target/windows-remote/win-get.sh <repo-relative path> <local dir>` copies
  files back to the Mac.

Gotchas:

- **cmd.exe, not a shell.** Use `&` to always continue and `&&` to continue on
  success. Cargo isn't on the SSH PATH: prefix
  `set "PATH=%PATH%;C:\Users\Noah\.cargo\bin" &&`.
- **`findstr` is fragile.** It reads arguments like `/184` as options, and
  `/b` misses cargo's indented `Finished` line. Redirect output to a file
  instead, or filter it on the Mac.
- **Tar archives from the Mac** need `COPYFILE_DISABLE=1`, or they unpack `._*`
  files.
- **Line endings.** Python on Windows writes CRLF, and `.gitattributes`
  normalises it.
- **Run length.** A full `run` takes ~40 min there, f64 mostly; the GPU cases
  alone take a few minutes. Run long jobs in the background.
- **Worktree bash guard.** In a worktree session, put loops, variables and
  multi-step logic in script files.

## Next steps

### 1. Performance (start here)

Measure on both machines with the same full-resolution input. Decode one Z 6
NEF to a full-size float TIFF with
`golden_inputs --long-edge 100000 <out_dir> <file.NEF>` on the Mac, check it
is byte-identical on Windows, and copy it over.

- **GPU export:** wall time per stage with `SPEKTRAFILM_GPU_PROFILE=1` (it
  inflates small renders, so use full resolution). The Mac does ~33 ms/MP;
  halation blurs dominate.
- **f64 Reference export:** the Mac takes 22.0 s for 24.3 MP with grain. On
  Windows there is no BLAS: `cpu_backend.rs` and `spectral_service.rs` use plain
  triple loops. Measure, then profile (e.g. `cargo flamegraph` or Windows
  Performance Recorder) before changing anything. Keep results bit-identical
  on each machine, and within the f64 gate across machines.
- **VRAM and limits:** peak VRAM at 24 MP and at the largest size the GPU
  accepts (`nvidia-smi --query-gpu=memory.used`). The 100 MP test only checks
  the reported limits. On 6 GB, find where it fails and whether it fails
  cleanly (CPU fallback, not a crash).
- **Watchdog (TDR):** Windows resets the GPU when one submission runs ~2 s. Look
  for device-lost errors on long blur/FFT passes at high megapixels. No
  device-lost recovery exists yet.

Write the numbers into `docs/cross-platform-port.md`, Mac and Windows side by
side.

### 2. Startup: Vulkan pipeline cache

wgpu supports a pipeline cache on Vulkan only. The engine passes `cache: None`
(`crates/spektrafilm-gpu/src/wgpu_backend.rs`, in the compute pipeline
descriptor). Measure cold and warm time from start to first GPU render on
Windows, add a disk-backed `wgpu::PipelineCache`, and measure again. This must
not change output: re-run the GPU golden cases with
`run.py compare --exact --engines gpu`.

### 3. Win32 app

Port **both** halves of the macOS export architecture:

- the `spektrafilm-native` JSON-lines helper, which serves the preview and
  `Save` of the cached render;
- the separate Reference export, which launches `spektrafilm-f64`
  (`macos/Sources/NegsApp/Engine.swift`).

Measure startup in three parts: first painted frame, first interaction, and
first usable image. Include helper start, catalog load and shader compilation.
Get the spike's result and Noah's go-ahead on the UI plan first.

### Later and open

- The GPU gates come from Apple and NVIDIA only. AMD, Intel and software Vulkan
  may need more room: measure, then propose.
- Linux: if the only Vulkan device is llvmpipe/lavapipe, wgpu picks it over the
  CPU engine. Decide whether to reject `DeviceType::Cpu` adapters.
- After any engine change, re-run the golden matrix on both machines (see
  `scripts/golden/README.md` → Run).

## Reporting back

Commit manifests, reports and docs (not dumps) to the branch. Summarise for
Noah: measured numbers, what changed, every non-PASS status with its
explanation, and open decisions with a recommendation.
