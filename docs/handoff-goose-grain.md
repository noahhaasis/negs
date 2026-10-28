# Handoff: standalone Goose grain experiment

## Start here

Implement the experiment in [the agreed plan](goose-cpu-rewrite-plan.md).
The user chose **grain + random samplers + blur**, in
[aardappel's Goose](https://aardappel.github.io/goose/), and then explicitly
narrowed measurement to **just that subpath**. Build separate optimized native
Rust and Goose executables with internal timers. No process startup, renderer,
Rust/Goose bridge or service-thread round trip belongs inside timing.

All grain subeffects must run: multilayer sampling, dye-cloud blur, microstructure,
pixel blur and mass-preserving multiplicative sharpening. Start in f64. Keep the
model, RNG sequence, seeds and precision faithful. A faster different grain
algorithm is a separate experiment. Do not restart the discussion by asking which
language, which subpath, whether to include blur, or whether to benchmark exports.

The first deliverable is a small native Goose feasibility program plus a frozen
pre-grain fixture and a working Rust baseline. Continue through the rewrite,
validation and isolated comparison; preparing another plan is not the task.

## What has actually been done

- Source inspection and Goose documentation research; the plan is written.
- Capture settings and source fingerprints are prepared under
  `experimental/goose-grain/`.
- `scripts/goose/preflight.py` records tools, private input availability and source
  fingerprint changes without compiling, installing or rendering.
- No Goose compiler has been installed, no fixture has been captured, no harness
  or Goose numerical implementation exists, and no new performance result exists.
- No production source, Cargo manifest or lockfile was changed for this handoff.

Preparation checks completed: preflight executed successfully, all source
fingerprints matched, local documentation links/fences were checked, and the
recipe's microstructure activation and FIR/IIR sigma choices were checked at
6048- and 9520-pixel long sides. These are preparation checks, not model tests
or verified render outputs.

Preparation was against `38722c36f72ae3c6a0113c492b98a4c39032359e` on
macOS 26.5.2 arm64, Rust/Cargo 1.93.1 and Apple clang 21.0.0. Recheck actual state.
There are unrelated untracked files, including `.claude/`, `tickets/`, personal
notes and previous performance research. Preserve them; do not clean/stash them
or treat them as changes made for this experiment. No commit or push is requested.

## First commands

From `/Users/noahhaasis/Projects/negs`:

```sh
git status --short
python3 -I scripts/goose/preflight.py
```

Inspect `target/goose/preflight.json`. The report is local: it includes absolute
private input paths and is ignored by git. Original source hashes are in
`experimental/goose-grain/source-baseline.json`; they are provenance, not a rule
that forbids necessary experiment scaffolding changes.

Install/download a pinned Goose release into `target/goose/toolchain/`, preserving
the standard library and runtime assets. Use the official repository/releases,
record the release/commit, artifact hash and runtime version, and read the docs
that match that pin. Set `GOOSE_HOME` to an experiment-local cache when invoking
the compiler. Follow environment approval requirements if network or filesystem
permissions require escalation; no separate product approval is needed to begin
the experiment. Avoid a global install or an unrecorded moving compiler revision.

First focused baseline checks, before changing numerical code:

```sh
cargo test --release --locked -p spektrafilm-math --features precision-f64 numpy_rng::tests
cargo test --release --locked -p spektrafilm-model --features precision-f64 grain::tests
cargo test --release --locked -p spektrafilm-core --features precision-f64 --test grain_statistics -- --skip gpu
cargo test --release --locked -p spektrafilm-core --features precision-f64 --test blur_edges
```

These commands have not been run as part of preparation. Do not claim they pass
until executed. Finish builds/tests before performance runs; do not benchmark
while another task compiles or runs a performance test.

## Source map and important traps

Line numbers below are from preparation; use symbol search if they move.

| Source | Read first |
| --- | --- |
| `crates/spektrafilm-model/src/grain.rs:459` | `apply_grain`: the complete timed boundary |
| Same file, lines 50–325 | `GrainSettings`, `GrainCurves`, calibration, `LayerInterp`, `layer_density` |
| Same file, lines 326–558 | Blur adapter, particle model, microstructure, deterministic sum and sharpening |
| `crates/spektrafilm-math/src/numpy_rng.rs` | Exact distribution algorithms; `lgamma` uses Rust's `libm` crate |
| `crates/spektrafilm-math/src/gaussian.rs` | FIR/IIR and reflection semantics; port the implementation rather than stale comments about sigma=3 |
| `crates/spektrafilm-core/src/stages/filming.rs:583` | Existing `SPEKTRAFILM_DUMP_FILM_PRE_GRAIN` hook, immediately before `apply_grain` |
| `crates/spektrafilm-core/src/grain.rs:124` | `grain_settings`: resolve schema + stock preset + overrides |
| Same file, line 166 | `grain_curves`: resolved normalized total/per-layer curves |
| `crates/spektrafilm-core/src/stages/filming.rs:44` | `grain_pixel_size_um`: exact f64 physical pixel size |
| `crates/spektrafilm-cli/src/bin/golden_probes.rs` | Reusable sampler cases including an extra RNG draw to detect consumption changes |
| `crates/spektrafilm-core/tests/grain_statistics.rs` | Independent Python-derived deterministic/statistical checks |
| `crates/spektrafilm-core/tests/blur_edges.rs` | Independent blur edge tests |

`ImageBuf` is row-major H×W×3 interleaved; `Scalar` is f64 only with
`precision-f64`. The primary benchmark includes layer calibration/interpolation,
RNG initialization, scratch, all subeffects and complete worker assembly.
Input copying/restoration happens outside timing on both sides. Do not precompute
particle parameters only for Goose or allocate its scratch outside the timer
while timing Rust's allocation.

The CLI crate currently lacks a direct `spektrafilm-model` dependency. If adding
`examples/grain_bench.rs` there, add an appropriate example/dev dependency rather
than assuming transitive crates are directly importable. Use the real Rust model,
not a copied, simplified baseline. Existing features propagate f64 through core.

`rand_mt` is pinned to 5.0.0 and `libm` to 0.2.16 in `Cargo.lock`. Their sources
were present under `~/.cargo/registry/src/`; preflight locates them. Preserve
source provenance/notices for any ported dependency routines.

## Freeze the fixture before timing

Read [the fixture contract and capture recipe](../experimental/goose-grain/README.md).
The prepared `capture-params.json` is the chosen recipe; validate the resolved
effects and then freeze it with the fixture. It deliberately enables microstructure
at realistic full-resolution pixel sizes, where the normal defaults skip it.

Private local inputs confirmed present during preparation:

- Primary candidate: `~/Downloads/Z 6/DSC_4990.NEF` (approximately 24 MP; check decoded dimensions).
- Secondary: `target/memory-study/leica-q3.dng` (approximately 60 MP).

These are availability hints, not portable dependencies. If absent, use another
local photo or a documented synthetic density fixture to make progress. Never
commit/upload RAWs or large pixel dumps. Existing exports and old binaries have
unverified provenance and are not today's baseline.

Rebuild f64 before capture:

```sh
cargo build --release --locked -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
```

The existing environment dump can capture the f64 pre-grain pixels during a
one-off CPU render. That run is fixture generation only; its full-render time
does not count. It still needs a new helper to export the exact resolved settings,
curves, dimensions and pixel size from the same pipeline state. Do not pair its
pixels with raw JSON profile curves that bypass profile normalization/morphing.
Use `Pipeline.film`/`Pipeline.params` and the core resolution functions above.

Build tiny fixtures first for rapid differential checks, then the frozen 24 MP
and 60 MP cases. Primary stock: `kodak_gold_200`; paper used during capture:
`kodak_portra_endura`. Paper processing is outside the timed operation.

## Implementation order and acceptance

1. Pin Goose and verify a small ahead-of-time native program, file IO, exact f64
   round-trip, monotonic timing, arrays and worker lifetimes. Inspect generated C.
2. Implement fixture capture/validation and the native Rust baseline. Prove all
   grain branches execute. Save reference outputs and hashes outside timing.
3. Port MT19937, uniform, normal, Poisson and binomial. Compare counts and RNG
   consumption/state, including switch boundaries and rejection paths.
4. Port blur and complete grain. Preserve f32 sigma rounding, FIR <3/IIR >=3,
   boundary states, whole-plane Poisson-before-binomial order, established seeds,
   and the 16,384-element ordered reduction for mass conservation.
5. Validate tiny, colour/positive/monochrome, one/three/four layers, edge cases,
   all subeffects and parallel equivalence against the existing Rust source.
6. Run one-thread comparison, then matching CPU budgets. Warm up once and collect
   at least seven timed calls in each native process, alternating executable
   batches. Startup, decoding, fixture loading/restoration and checksum are out;
   required internal worker copies and dispatch are in. Do not use external shell
   wall time as the subpath result.
7. Report correctness, median/spread, native compilation settings, actual worker
   budget and peak/retained memory. Write `docs/goose-cpu-results.md` and compact
   machine-readable results, including a slower or limited result if that occurs.

Goose's thread copies remain relevant even with process overhead excluded.
Do not bypass its memory model by sending pointer tokens to workers. Compare
equivalent single-thread work before attributing differences to parallelism.
RNG divergence is a failure of the faithful port; statistical resemblance alone
does not resolve it. Surface any justified floating tolerance before timing.

If threading requires an unexpected design, make progress with the serial port
and record the limitation. Do not expand into application ABI/service-thread
integration, rewrite shared renderer blurs, switch to GPU, change the grain
distribution or benchmark the whole app to work around it.

## References and evidence

Read Goose's [command line](https://aardappel.github.io/goose/docs/command_line.html),
[language reference](https://aardappel.github.io/goose/docs/reference.html),
[specification](https://aardappel.github.io/goose/docs/goose_spec.html),
[standard library](https://aardappel.github.io/goose/docs/stdlib.html), and
[official releases](https://github.com/aardappel/goose/releases). Use documentation
from the selected compiler revision where the live site differs.

Historical local research is in `docs/performance-options-2026-10.md`,
`docs/performance-study-results.json` and `docs/memory-study-q3-results.json`.
It explains likely hotspots and prior algorithm changes, but predates today's
grain and is not a measurement of this experiment. Other handoffs describe
different tasks; their old platform status, commit conventions and GPU gates
are not instructions for this experiment.
