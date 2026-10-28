# Goose CPU grain, samplers and blur experiment

Status: proposed plan; no rendering code or toolchain changed.
Repository inspected: `38722c36f72ae3c6a0113c492b98a4c39032359e`.
Goose documentation consulted: 2026-10-27. No `goose` executable was found on PATH.

Execution entry point: [new-agent handoff](handoff-goose-grain.md). Prepared
capture settings and the fixture contract are in
[`experimental/goose-grain/`](../experimental/goose-grain/README.md).
Run `python3 -I scripts/goose/preflight.py` to inventory the current environment.

## Objective

Compare the complete Rust grain operation with a faithful Goose rewrite of that
operation, including its random samplers and Gaussian blurs. Benchmark the subpath
in isolation with all compatible grain options enabled. Application integration
and full-render benchmarks are deferred.

Build two native benchmark executables, one Rust and one Goose. Each loads the
same fixtures, initializes its runtime and worker pool, and then times repeated
native calls internally. No process is launched, no service-thread request is
made, and no Rust/Goose FFI call is needed inside the measured subpath. Each
executable is launched separately; its startup is outside its internal timer.

The result answers how fast this grain implementation runs in each language. It
does not measure complete exports or claim a universal language speed difference.

## Scope

Start with f64, matching the reference CPU exporter. Keep the existing Rust
implementation as the baseline. Production/GPU code and application defaults
remain unchanged; the standalone experiment requires no production backend switch.

| Existing source | Rewrite responsibility |
| --- | --- |
| `crates/spektrafilm-model/src/grain.rs` | CPU grain calibration, layer interpolation, particle model, accumulation, microstructure, pixel blur, mass-preserving multiplicative sharpening and density-floor removal |
| `crates/spektrafilm-math/src/numpy_rng.rs` | Uniform-double conversion, Poisson multiplication/PTRS, binomial inversion/BTPE and cached polar Box–Muller normal sampling |
| `rand_mt::Mt` dependency | MT19937 initialization, state, twist and tempering, implemented in Goose |
| `crates/spektrafilm-math/src/gaussian.rs` | Gaussian blur primitives used within grain: FIR/IIR, horizontal and vertical passes |

The three Rust source files contain approximately 1,100 code lines including
inline tests, excluding MT19937 and new benchmark/build code. Profile parsing,
preset resolution and GPU parameter construction remain outside the rewrite.
The primary timed operation includes grain's curve calibration and interpolation;
do not feed precomputed particle parameters only to Goose.

Shared blur elsewhere in the renderer is not replaced for this experiment.
Standalone sampler and blur timings are supporting diagnostics; the headline is
complete grain, including its own samplers and filtering.

## Relevant Goose findings

Use ahead-of-time native compilation, not the TinyCC runner. Goose emits C; pin
its revision/runtime and record the C compiler and flags. Check data-stack usage
and reservation requirements with `--stacks` and `--stack-reserve`.
[Command-line documentation](https://aardappel.github.io/goose/docs/command_line.html)

Goose workers exchange copied flat values. That affects internal parallelism even
in a standalone executable. Preserve its supported memory model and include the
cost of worker messages, packing and assembly when a grain call needs them.
Exported functions must stay on their initializing thread, but exports are not
needed for this native-to-native benchmark.
[Language reference, sections 22 and 24](https://aardappel.github.io/goose/docs/reference.html)

The documented math module covers ordinary transcendental functions but does not
list `lgamma` or `log1p`. Investigate these explicitly: preserve the Rust
`libm::lgamma` computation or document an identical shared math primitive. Avoid
accidental fused multiply/add or fast-math changes.
[Standard-library documentation](https://aardappel.github.io/goose/docs/stdlib.html)

Goose's published benchmarks concern different workloads and do not predict this
result. [Upstream results](https://aardappel.github.io/goose/bench/results.html)

## 1. Freeze an identical subpath fixture

Capture a density-CMY image immediately before Rust `apply_grain`, along with:

- Width/height and the exact interleaved f64 values.
- Resolved `GrainSettings`, total/per-layer `GrainCurves`, and pixel size.
- Positive/negative and monochrome flags, and the established seed scheme.
- Input, profile/data and parameter hashes, source revision and local diff.

Use one 24 MP colour-negative fixture as the primary workload and a 60 MP fixture
for scaling/memory. The image may come from a full-effects render, but everything
before and after grain is excluded from the timed operation.

Enable multilayer sampling, dye-cloud blur, microstructure, grain pixel blur and
multiplicative sharpening with moderate nonzero settings. Check that microstructure
actually exceeds its physical-size activation threshold. Freeze values before
benchmarking; do not tune the recipe to favor either implementation.

Use a simple versioned fixture format: metadata/settings/curves plus a
little-endian f64 pixel file. Both drivers validate the same hashes and lengths.
All parsing, fixture loading and input restoration occur outside timing. The
original fixture remains immutable; every iteration starts from identical data.

## 2. Establish the standalone native harnesses

First build a tiny Goose native program that reads the fixture and produces a
known array result. Verify f64 round-tripping, scratch reuse, internal monotonic
timing and repeated execution at 24/60 MP. Confirm the pinned compiler handles
the required arrays and worker lifecycle before porting the complete model.

The Rust harness calls the existing grain implementation directly. The Goose
harness calls its Goose implementation directly. No language bridge, subprocess
exchange or dedicated Rust service thread participates in an iteration.

Compare one CPU execution thread on both sides first. Then compare matching total
CPU budgets using Rayon and Goose's supported workers. Start runtime/pools before
timing, but include all per-call dispatch and data movement needed by grain.
Record actual worker counts. If a persistent Goose worker pool is not supported
by the prototype, include its per-call startup cost and report it separately.

Keep channel/layer RNG streams sequential. Channel jobs can run independently;
naively splitting a stream into pixel chunks changes seeded output. Parallelize
deterministic interpolation/filtering separately where useful. Include Goose's
internal copied jobs and result assembly rather than timing only worker arithmetic.

## 3. Port and verify the random samplers

Implement MT19937, `rk_double`, cached normal sampling, Poisson and binomial.
Preserve u32 wrapping, shift widths, scalar seeds, exact uniform conversion,
normal-cache behaviour and every rejection/inversion threshold.

Differential checks compare outputs and the next RNG draws/state. Cover multiple
seeds, MT twist boundaries, Poisson below/at/above 10, binomial around p=0.5 and
n*min(p,1-p)=30, zero/extreme cases, rejections and normal-cache reuse. Check
numeric casts explicitly rather than assuming Goose and Rust conversions agree.

Require exact integer streams and sampled counts for the supported corpus.
Diagnose the first divergent math or branch decision. Record bit/ULP differences
for normal values and transcendental intermediates.

## 4. Port blur and the complete grain operation

Preserve blur's f32 sigma conversion even in f64 mode; FIR for sigma < 3 and IIR
for sigma >= 3; the identity case; FIR radius/normalization/tap order; repeated-edge
reflection; and IIR coefficients, endpoint state, pass order and traversal.

Check constants, impulses, ramps, noise, odd/tiny dimensions, 1×N/N×1 images,
kernels wider than the image, sigma around 3 and large radii. Use the existing
blur-edge fixtures as independent checks.

Then port calibration, sub-layer interpolation, particle draws, dye-cloud blur,
accumulation, microstructure, pixel blur, mass-preserving sharpening and floor
removal. Preserve seeds `channel + 10*layer` and `100 + channel`; draw the entire
plane's Poisson counts before its binomial counts. Preserve 16,384-value reduction
chunks and the ordered final sum used for mass conservation.

Cover colour, positive interpolation, monochrome broadcasting, one/three/four
layers, zero granularity, all subeffects and serial/parallel equivalence. Reuse
the Python-derived fixtures in `crates/spektrafilm-core/tests/grain_statistics.rs`
as an additional model check. Target exact same-machine output; expose every
remaining floating discrepancy. Freeze justified tolerances before performance
runs. RNG divergence fails the faithful-port comparison; a statistics-only pass
cannot hide it. Do not introduce thinning, approximate sampling or f32.

## 5. Benchmark only the rewritten subpath

For each executable:

1. Load/validate fixtures; initialize the runtime/pool before timing.
2. Perform a warmup, restore the original input, and start a monotonic timer.
3. Run the complete grain function, including curve calibration, per-call RNG
   seeding, required scratch allocation, filtering and worker result assembly.
4. Stop timing only when all work has joined and the output is ready.
5. Consume and checksum every result outside timing, retaining correctness data
   so neither compiler can discard the work. Restore the input before the next run.

Run at least seven measured iterations per variant, in multiple alternating
Rust/Goose executable batches. Process startup, fixture loading, input restoration,
checksumming and compilation are excluded from reported subpath milliseconds.
Keep validation dumps, detailed instrumentation and concurrent builds out of runs.
Record the input restoration/cache policy and use it consistently on both sides.

Use comparable release optimization policies, CPU features and strict floating
semantics. Record Rust LTO/codegen flags, Goose inlining and generated-C compiler
flags. Do not compare native Goose to debug Rust or TinyCC to optimized Rust.

Report median and spread, paired batch speedup, correctness, worker count,
physical peak/retained memory with the platform metric named, and virtual stack
reservations separately. Existing per-channel timings overlap; do not sum them
as elapsed time. If a preallocated-scratch experiment is useful, offer it on both
sides and label it separately from the complete-call result.

The primary result is complete grain, with all grain subeffects active. Supporting
sampler/blur timings explain where a difference arises; they do not replace it.
Start on macOS arm64, then validate/measure Windows x64 and Linux x64 independently.
No cross-machine speed ratio is a language comparison.

## Deliverables and later decisions

Suggested layout:

```text
experimental/goose-grain/     Goose sources, native driver and toolchain pin
scripts/goose/               fixture capture, builds, validation and benchmark runner
crates/spektrafilm-cli/examples/grain_bench.rs
                             Rust driver calling the existing implementation
target/goose/               generated C, binaries, private fixtures and raw runs
```

Write `docs/goose-cpu-results.md` with compact machine-readable results. Keep large
arrays, generated code and photographs under ignored `target/goose/`. Leave
existing untracked research files untouched.

After the faithful port, evaluate scratch reuse, blocked transposes, loop layout,
bounds-check elimination and worker batching individually, with comparable Rust
changes as controls. RNG/model reformulations are separate experiments.

A slower result is a completed experiment. Only if the isolated result warrants
it, design the optional application ABI/service-thread integration and then
measure complete exports including its real overhead. Neither integration nor
full-render benchmarking is required to finish this subpath experiment.

First milestone: **freeze the pre-grain fixture and establish native Rust/Goose
harnesses**, then port samplers, blur and complete grain, validate, and benchmark.
