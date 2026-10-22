# Windows port: handoff to the agent on the Windows machine

You are continuing the Negs Windows port on the target machine. Noah's
priorities, in order: **correctness**, **performance**, **launch speed** (a hard
requirement for the UI). Dark mode does not matter on Windows. The **GPU path is
what ~99 % of users run**, so verify it first and most carefully.

Read first: `docs/cross-platform-port.md` (plan and decisions),
`scripts/golden/README.md` (the correctness harness, its gates and the macOS
results), `docs/cross-platform-port-review-codex.md` (an independent review).

## Decisions already made — do not reopen

- **Correctness across platforms = tight numeric delta**, not byte-identical
  output. No portable-math rewrite (it would cost ~22 %–3.6× on f64 exports).
  Within one machine and build, output must be bit-identical.
- Gates live in `scripts/golden/gates.json`. **Never loosen a gate to make a run
  pass.** If something fails, find the first divergent stage (`probe`,
  `compare-probe`), explain it, and bring the evidence to Noah. Relaxed
  statuses (PASS-BORDER, PASS-GRAIN) must stay visible in reports.
- Windows UI: Win32 with the standard common controls (light mode), wgpu
  straight into the HWND for the image, custom drawing only where needed.
  Noah is running a Win32 vs WinUI 3 startup spike; follow its result.
- Windows before Linux.

## What you receive

1. **Code:** branch `worktree-cross-platform-port` (in `negs-cross-platform.bundle`
   if it is not on GitHub: `git fetch path\to\negs-cross-platform.bundle
   worktree-cross-platform-port:cross-platform-port`).
2. **Handoff archive** (transferred directly, not via GitHub — too large, and the
   repo is public). Unpack into the repo root so the paths line up:
   - `target/golden/inputs/` — the 10 fixed float-TIFF inputs (64 MB)
   - `target/golden/dumps/macos-arm64/` — the Mac reference dumps (5.9 GB)
   - `target/golden/probe/macos-arm64/` — the Mac probe outputs (0.96 GB)
   - `SHA256SUMS` — verify before use
   The Mac manifests are in git: `scripts/golden/manifests/macos-arm64.json`
   (reference) and `macos-x86_64-rosetta.json`.

## Setup

- Rust **1.93.1**, MSVC toolchain (`rustup toolchain install 1.93.1`,
  `rustup default 1.93.1`), Visual Studio Build Tools with the C++ workload.
- Python 3.9+ (standard library only for the harness).
- From the Mac: `cargo check --target x86_64-pc-windows-msvc` passes for the
  CLI, the f64 CLI and `spektrafilm-native`, with two harmless warnings
  (`unused mut` in `wgpu_backend.rs:417`, dead `finish_rgb`). Linking and
  running have never been tried on Windows.
- Windows has no BLAS: `cpu_backend.rs` and `spectral_service.rs` use plain
  triple loops there. Expect the f64 engine to be slower than on macOS;
  measure, don't guess.

Record before anything else: CPU model and features (AVX2? FMA?), GPU, driver
version, which adapter and backend wgpu picks (the `run` manifest records
adapter and backend for every GPU render), Windows build.

## Step 1 — build

```powershell
cargo build --release --locked -p spektrafilm-cli --bin spektrafilm --bin golden_compare --bin golden_probes --bin golden_inputs
cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
python -I -m unittest discover -s scripts/golden -p "test_*.py"
```

## Step 2 — correctness

```powershell
python -I scripts/golden/run.py run --label windows-<gpu> --out scripts/golden/manifests/windows-<gpu>.json --dump-dir target/golden/dumps/windows-<gpu>
python -I scripts/golden/run.py quality scripts/golden/manifests/windows-<gpu>.json --dumps target/golden/dumps/windows-<gpu>
python -I scripts/golden/run.py compare scripts/golden/manifests/macos-arm64.json scripts/golden/manifests/windows-<gpu>.json --dumps-a target/golden/dumps/macos-arm64 --dumps-b target/golden/dumps/windows-<gpu> --report target/golden/compare-windows.json
python -I scripts/golden/run.py probe --label windows-<gpu> --out-dir target/golden/probe/windows-<gpu>
python -I scripts/golden/run.py compare-probe target/golden/probe/macos-arm64 target/golden/probe/windows-<gpu>
```

What to expect and watch:

- **GPU renders must use the resident GPU path.** `run` fails a GPU render that
  fell back to the CPU or skipped the resident chain. If wgpu picks DX12 with
  FXC, shader compiles may be slow; note it. Choosing DX12 vs Vulkan needs an
  explicit engine setting first (`WgpuBackend::new` hardcodes
  `Backends::PRIMARY`; `run.py` strips `WGPU_*`). Test both backends once that
  exists, and record which one ran.
- **`gpu` and `gpu_grain` gates** were calibrated on the GTX 1660 SUPER
  (2026-10-22, `scripts/golden/README.md`). On another GPU, report the measured
  ΔE statistics; propose a change with evidence, don't just widen them.
- **`quality` (GPU vs f64 on Windows)** must pass the same gate as on the Mac
  (worst ΔE00 there: 0.0054).
- **f64 `dir_wide`** used half the f64 gate between arm64 and x86_64; Windows
  adds rustfft's AVX path, MSVC's math library and the naive dgemm.
- **Auto-exposure** used f32 `log2`/`powf`, and MSVC's differ from Apple's:
  fixed in f596979 (f64, rounded once). Any other f32 libm call on a whole-image
  path fails the f64 gate the same way.
- **f32 CPU with grain** diverges (FAIL-GRAIN): one shared MT19937 stream, so
  the first flipped draw shifts all later ones. Known; open decision.
- Run every comparison twice on Windows too (`compare --exact` between two
  Windows runs) to confirm same-machine determinism, including the GPU.

## Step 3 — performance

Profile a full-resolution Reference (f64) export and a GPU export (the Mac:
22.0 s f64 for 24.3 MP with grain, M1 Pro; GPU ~33 ms/MP). Check VRAM use at
full resolution and the largest image the GPU accepts; the 100 MP test only
checks reported limits. Watch for TDR (device lost) on long dispatches.

## Step 4 — app

The Windows app must port **both** halves of the macOS export architecture:
the `spektrafilm-native` JSON-lines helper (preview, `Save` of the cached
render) **and** the separate Reference export that launches `spektrafilm-f64`
(`macos/Sources/NegsApp/Engine.swift`). Startup: measure first painted frame,
first interaction and first usable image separately, including helper start,
catalog load and shader compilation; the wgpu pipeline cache is not
implemented yet (`cache: None`).

## Reporting back

Commit manifests and reports (not dumps) to the branch, update
`scripts/golden/README.md` with a Windows results section in the same format as
the macOS one, and summarise for Noah: what passed, every non-PASS status with
its explanation, measured numbers, and open decisions. Don't push or merge
without Noah's go-ahead; main's commits use his dating rule (one minute after
main's tip).
