# Windows port: handoff for the next agent

You are continuing the Negs Windows port. The golden matrix matches the Mac
within calibrated gates on a real Windows GPU. Full-resolution testing has
now exposed **capacity/watchdog failures** beyond that matrix. Performance
baselines and a Vulkan pipeline cache are implemented; the **Win32 app** has
not been started.

Noah's priorities, in order: **correctness**, **performance**, **launch speed**
(a hard requirement for the UI). The **GPU path is what ~99 % of users run**.
Dark mode does not matter on Windows. Windows comes before Linux/Omarchy.

Read first: `docs/cross-platform-port.md` (plan, risks, decisions) and
`scripts/golden/README.md` (the correctness harness, its gates, and the Mac and
Windows results).

## Resume here: handoff requested October 23

Noah requested a handoff to another agent and asked that the existing
background run finish. **Do not restart the project or discard the dirty
worktree.** There are two distinct validation checkpoints below; the last
backend-reporting edits have not had full integration validation yet.

### Committed checkpoints

- `e3d0bb8`: measured full-resolution performance/capacity and added the
  persistent Vulkan pipeline cache. All 484 renders were exact against the
  previous same-machine manifests on **each** machine. Cache corruption and
  unwritable-directory recovery, three cache unit tests on both platforms,
  and 22 golden-harness tests passed. Nothing pushed or merged.
- `c7e762a`: the macOS-to-Windows feature inventory, added concurrently by
  another participant. Preserve `docs/windows-ui-features.md` and its link
  below; it is the reference for the eventual Windows UI.

### Uncommitted capacity guard: implemented and exercised

Files: `crates/spektrafilm-gpu/src/capacity.rs`, `src/lib.rs`,
`src/wgpu_backend.rs`, `crates/spektrafilm-core/src/pipeline.rs`,
`crates/spektrafilm-core/tests/gpu_capacity_fallback.rs`, and
`crates/spektrafilm-gpu/examples/windows_capacity.rs`.

- Admission checks apply to **Windows only**. Metal's existing path is kept.
  Check RGB binding/allocation limits, then a conservative working-set
  estimate: four RGB f32 images + 128 MiB reserve, plus the padded complex
  grids and kernel spectra for both diffusion filters. Query the largest
  Vulkan device-local heap on discrete GPUs and admit at most 80% of it.
  This is static headroom, **not** a live free-memory query or TDR recovery.
- `ComputeBackend::image_capacity` and `film_chain_capacity` default to
  acceptance. Windows wgpu overrides them. A rejected render goes through
  `CpuBackend` for the whole pipeline; returning `None` alone would instead
  dispatch individual GPU stages and fail again. Rescale occurs once.
- `SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB` is a diagnostic that can only lower the
  admission budget. Forced image rejection (0 MiB) and forced FFT rejection
  (160 MiB, small `diff_bpm` fixture) were compared on Windows: raw buffers
  and TIFFs are identical to explicit f32 CPU rendering.
- Actual hardware admission probe (`cargo run --release --locked -p
  spektrafilm-gpu --example windows_capacity`): 24.3 and 96 MP accepted;
  144 and 178.95 MP rejected against a **4772 MiB** admission budget;
  180 MP rejected against the 2,147,483,647-byte binding limit. The probe
  allocates no large image and does not establish runtime watchdog safety.
- The original **180 MP** input now completes via CPU fallback: pipeline
  **272.088 s**, wall **285.930 s**, whole-device GPU memory peak **635 MiB**
  (605 MiB baseline), exit 0, **no event 4101**. TIFF SHA-256:
  `6eb2fd41d9a5c29fbe7ca3853dd9c0332b09cd84f3a77e2ee3868cfae4f8f040`.
  This is a successful slow CPU export, not a GPU performance result.
- Three capacity unit tests and the pipeline fallback/rescale integration
  test passed on Mac and Windows. A complete Mac golden run passed 484/484
  exact. The Windows full run was allowed to finish for this handoff; see
  the completion record below.

These full matrices validate the guard **before** the reporting edits below.
Public WIP artifacts are `scripts/golden/manifests/*capacity-guard.json` and
`scripts/golden/reports/*capacity*`. Private dumps are under
`target/golden/dumps/macos-capacity-guard` and `windows-capacity-guard` on the
respective machines. Source manifests/reports and diagnostic logs are also
under `target/windows-perf/`.

### Last edits: actual-backend reporting (finish and validate next)

Review the current diff; these edits are newer than the capacity matrices:

- `Pipeline::process_with_backend` returns `(ImageBuf, &str)` with the backend
  that actually rendered it. `process` remains a compatibility wrapper.
  `process_scan` uses a static stateless CPU backend for a stable returned
  name when capacity rejection causes fallback.
- `crates/spektrafilm-cli/src/main.rs`: print `Backend: ...` **after** the
  render, using that returned name. Previously a capacity-fallback CPU
  export still printed `Backend: wgpu` because it reported startup selection.
  Existing GUI code interprets that marker as proof of GPU execution.
- `crates/spektrafilm-native/src/main.rs`: the JSON render response similarly
  reports the actual backend. A successful borrowed GPU render keeps the
  selected backend name; the full fallback path uses `process_with_backend`.
- `scripts/check_native_macos.py`: replaced `select()` on stdout pipes with
  one reader thread and a queue, because Windows cannot select anonymous
  pipes. Added `--expected-backend cpu|wgpu` assertions for render replies.
  The script still has its historical filename but is intended to run on
  both platforms. **It has not yet been run after these edits.**
- The updated core integration test (now also asserting the returned CPU
  name) passes on Mac. Mac f32 CLI and native helper builds completed.
  **Mac f64 has not been rebuilt after the reporting edits. Windows has
  neither these reporting sources nor their rebuilt binaries yet.**

Next actions, without reopening previous decisions:

1. Review the reporting diff, build f32 CLI/native helper and separately f64
   on both machines. Do not enable `precision-f64` on a combined helper/f32
   build: Cargo feature unification would change their precision.
2. Run the native bridge test on Mac and Windows, both ordinarily and on
   Windows with `SPEKTRAFILM_BACKEND=wgpu` plus
   `SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB=0`, expecting `cpu`. It exercises helper
   render/save and separate Reference export, including saved-preview versus
   Fast-export bytes. Pass explicit `.exe` paths on Windows.
3. Recheck CLI fallback reporting and same-machine bytes; rerun the golden
   matrix for the final source and compare `--exact` before committing the
   guard/reporting changes. Keep the old validation checkpoint distinct.
4. Update the public manifests/reports and port plan, then commit on this
   branch only. Do not push or merge.
5. Device-loss recovery and long-running effects that fit in memory remain
   unresolved. Do not describe memory admission as complete watchdog safety.
   The startup spike result and UI-plan go-ahead remain outstanding too.

### Remote/source details for resuming

The Windows checkout is still based on `214ed09`, with the cache and first
capacity-guard changes transferred directly into its working tree. It is
**dirty intentionally**. Do not blindly reset or assume `win-sync.sh` can
fast-forward over it. Review/preserve the remote changes before reconciling
them with the local committed branch.

Private diagnostic helpers in `target/windows-perf/` on both machines:
`check_fallback.py` (forced image/FFT versus CPU),
`check_large_fallback.py` (180 MP run), and `transfer.tar` (last transfer,
not an authoritative complete source snapshot). The two check scripts still
expect the **old** early `Backend: wgpu` marker; update those assertions to
the final actual-backend behavior before reusing them. Do not rerun the
4.5-minute 180 MP export solely to test a log label.

After a Codex daemon restart, local execution-session IDs may disappear while
the remote Python/cargo process continues. Check the Windows process list and
manifest/report existence before relaunching anything. Redirect future long
jobs' stdout/stderr to a remote log so completion can be recovered without
the original SSH tool session. Keep all machine access details in the ignored
helpers, not this public document.

### Background-run completion record

The native-helper Mac build completed successfully. The Windows capacity
matrix finished at 10:08 on October 23: **484/484 identical** (184 GPU,
184 f64, 116 f32 CPU, no problems) against `windows-gtx1660s-vulkan`. Its
manifest and report are `scripts/golden/manifests/windows-gtx1660s-capacity-guard.json`
and `scripts/golden/reports/windows-capacity-guard-exact.json`. Like the Mac
run, it predates the actual-backend reporting edits above.

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

## Baseline before the October 23 continuation

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

## October 23 continuation

- Full-resolution Z 6 input decoded independently on both platforms with
  identical bytes. Portra 400 → Endura, grain on: GPU pipeline 0.724–0.757 s
  on M1 Pro, 0.891–0.935 s on GTX 1660 SUPER; f64 15.961–16.395 s vs
  35.111–35.479 s. The full-size f64 TIFF is identical across platforms;
  raw buffers pass the unchanged f64 gate.
- New standard-library scripts: `scripts/golden/benchmark.py` and
  `capacity_input.py`. The benchmark checks backend/resident execution,
  repeated output hashes, sampled NVIDIA memory and Windows display resets.
- 96 MP completed without a matching Windows display-reset event. 144 MP
  and 178.95 MP returned exit 0 **while the NVIDIA driver reset** (System
  event 4101). 180 MP panicked at the 2 GiB storage-binding limit, with no
  CPU fallback. Do not describe these as successful capacity tests.
- Vulkan pipeline cache: `crates/spektrafilm-gpu/src/pipeline_cache.rs`.
  Per-user storage, adapter/driver validation, CRC envelope, atomic writes,
  persistence after resident renders and on Drop. Metal does not use it.
  Corrupt and unwritable cache recovery passed on Windows, with identical
  output. Unit tests pass on both platforms.
- No demonstrated NVIDIA warm-start benefit: first pipeline ~112 ms with
  populated application cache vs 105–113 ms disabled. Driver/OS caches were
  already warm; reboot-cold launch remains unmeasured.
- No gate, shader math, f32 CPU grain implementation or macOS numerical
  implementation changed. No push or merge. The Win32 startup-test result
  and UI go-ahead are still outstanding.
- All 484 outputs (184 GPU, 184 f64, 116 f32 CPU) are byte-identical to the
  previous same-machine manifests on **both** machines. Full combined
  manifests are `macos-arm64-pipeline-cache.json` and
  `windows-gtx1660s-vulkan-pipeline-cache.json` under
  `scripts/golden/manifests/`. The 22 harness tests also pass.
- A 178.95 MP `--profile-only` diagnostic also reset the driver, then stalled
  until explicitly terminated after 125 s. Per-stage waits alone are not
  sufficient at that size. Use a bounded `--timeout` for further diagnostics.

Detailed measurements and limitations: `docs/cross-platform-port.md`.
Public report: `scripts/golden/reports/windows-performance-2026-10-23.json`.
Private TIFFs, raw dumps, logs and WPR trace: `target/windows-perf/` on each
machine. The Windows CPU trace is `target/windows-perf/f64-cpu.etl`.
Cache-change golden subsets/dumps use `macos-pipeline-cache`,
`macos-cache-cpu`, `windows-pipeline-cache`, and `windows-cache-cpu`.

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

### 1. Capacity and watchdog recovery (start here)

Use the recorded failures above to add preflight capacity checks and CPU
fallback before oversize allocations/bindings, and to investigate shorter
Windows submissions and device-loss recovery. An exit-0 TIFF during a driver
reset is not proof of correct output. Keep shader math unchanged and prove
exact output on both machines. Do not increase Windows TDR timeouts.

Use `--profile-only` for diagnostics without first repeating the known-unsafe
normal submission. The per-stage waits change scheduling, so a profile run
without a reset does not establish that the normal render is safe. Test wide
blurs/FFT effects as well as defaults before claiming a supported MP ceiling.

For f64 speed work, inspect the captured WPR stacks before changing the
Windows triple loops. Stage timings put most time in printing/scanning, but
do not isolate BLAS from pow/exp and other work. Preserve same-machine bytes
and the existing cross-platform f64 gate.

### 2. Startup: Vulkan pipeline cache

Implemented and measured as above. Cache isolation and disable switches are
documented in `scripts/golden/README.md`. Do not claim a speedup on the tested
NVIDIA driver; keep process-cold, application-cache-cold and reboot-cold
measurements distinct. Re-run `run.py compare --exact --engines gpu` after any
further cache/compilation changes.

### 3. Win32 app

The app follows Windows conventions rather than copying the Mac layout.
`docs/windows-ui-features.md` lists every macOS feature and its behaviour,
what to do instead on Windows, and the decisions still open.

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
