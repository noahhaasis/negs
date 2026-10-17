# Sequential scratch reuse

The resident GPU chain submits once and runs each dispatch in a separate compute
pass. A/B are the primary RGB buffers; S0/S1/S2 are shared full-image scratch.
The scratch pool belongs to one render, not the backend cache. Allocate only the
maximum number required by enabled effects.

| Pass, in execution order | Primary storage | Shared scratch and last use |
|---|---|---|
| Front spectral conversion | A → B | None; A becomes available |
| Highlight boost | B in place | Small reduction buffers only |
| Camera diffusion | B → mix → B | S0 upsampled, S1 mix; separate reduced-size buffers |
| Camera lens blur | B → A → destination → B | S0 destination |
| Halation | B exposure, A blur intermediate | S0 core/blur output, S1 tail/accumulator; final add consumes both |
| Log + density curve | B log exposure → A density | None |
| DIR | A density consumed by matmul; A then blur intermediate and weighted mix; B log exposure corrected, then → A | S0 correction/tail, S1 Gaussian; both consumed before grain |
| Grain | A in place; optional blur via B | None |
| Printing (skipped for film scans) | A → B log exposure | None |
| Enlarger diffusion (skipped for film scans) | B linearized → mix → B, then logged | S0 upsampled, S1 mix; separate reduced-size buffers |
| Print density + scan | B → A → B RGB (film scan: A → B) | None |
| Glare | B in place, A blur intermediate | S0 noise, overwritten by generation before use |
| Gamut | B in place | None |
| Scanner lens blur | B → A → destination → B | S0 destination |
| Unsharp | B original, A blur intermediate | S0 sharpened output, S1 blurred image; copy S0 → B |
| Post-scan + readback | B | No scratch; map B or copy to dedicated readback |

No dispatch binds the same buffer as both a read-only input and writable output.
Every scratch input is fully initialized by a preceding dispatch (including
explicit accumulator clearing). Separate compute passes preserve dependencies;
buffer handle drops do not establish GPU lifetimes. Bind groups and cloned
buffer handles reference the same allocations. No shader math or pass order
changes are needed.

Default full-image GPU allocations fall from ten to five (A/B + three scratch),
excluding input/output host images and the optional readback buffer. Camera and
enlarger diffusion share their full-resolution intermediates with the same pool;
their reduced-size buffers remain local to each effect.

The later DIR mix reuse removes S2: the default chain now allocates four
full-image GPU buffers (A/B + two scratch). Both DIR blurs finish using A as
their intermediate before the weighted mix writes A. The mix reads S0/S1 in
separate dispatches, and the density curve overwrites A after subtraction.

## Matched measurements

Leica Q3 DNG, 9520 × 6336, Kodak Gold 200 → Portra Endura, M1 Pro / 32 GB.
All figures below are from separate normal release runs, sampled by
`scripts/profile_memory.py`. Footprint is macOS process accounting (including
compressed memory), not Rust heap bytes or GPU device allocation totals.
The CPU comparison uses the committed prototype with parallel grain and
GUI-equivalent LUT export settings. The GPU comparison uses a freshly built
binary from the same source before scratch reuse, with default GPU params.

| Path | Peak before | Peak after | Wall time before | Wall time after |
|---|---:|---:|---:|---:|
| f64 CPU export | 11.82 GiB | 9.29 GiB | 34.23 s | 34.46 s |
| f32 GPU export | 8.76 GiB | 5.83 GiB | 6.04 s | 6.20 s |

The GPU's retained idle footprint was measured separately with the decoded
input, profile and backend kept alive across three renders while each output
was dropped. After a one-second settling period, the mean of 17 footprint
samples was 1.66 GiB before and 1.42 GiB after. This measures the whole
process, including decoded input and native driver caching; the exact idle
number varies with host state.

The full-resolution CPU TIFF is byte-identical to the committed prototype's
TIFF. The full-resolution GPU TIFF is byte-identical to the pre-change GPU
TIFF. `scripts/parity/check_gpu_scratch.py` also compared 22 combinations
across two image sizes, each with three consecutive renders, against the
pre-change f32 GPU. It covers default effects, individually enabled effects,
combined camera/enlarger/scanner optics and direct film scans. A second pass
forces a dedicated staging readback (`SPEKTRAFILM_WGPU_FORCE_STAGING=1`).
The CPU f64 parity harness matched the original binary in all four synthetic
cases under both serial and parallel grain.

On the CPU, filming consumes the decoded image and mutates exposure in place.
DIR consumes the log exposure and density buffers, then reuses them for the
corrected exposure and matrix correction. Grain writes its finished channels
into the consumed density buffer. Printing's LUT interpolation reuses the film
density buffer; scanning's LUT interpolation reuses the print/film density
buffer. Unsharp writes back into its input. Camera RAW conversion fills one
preallocated RGB buffer by pixel index. These changes keep the same f64
calculations, summation order and per-channel RNG streams.

## DIR mix follow-up

At 9520 × 6336, reusing A for DIR's weighted mix removes one 12-byte-per-pixel
GPU buffer (723,824,640 bytes, 0.674 GiB). A matched Q3 export from an
explicitly rebuilt pre-change binary and the candidate measured 5.75 →
5.08 GiB peak process footprint, a 0.67 GiB reduction. Wall time was
6.12 → 5.95 s in these single runs; that difference is not a speed claim.
The TIFFs were byte-identical (SHA-256 `48deca419cf613af2e4a3ba1f1695e67d393fa0f801ab2c785415e252d3ed4cd`).

All 22 GPU parity cases matched the exact pre-change binary across two image
sizes and three consecutive renders, with both direct mapping and forced
staging readback. The memory runs and parity reports are under the ignored
`target/memory-study/dir-mix-exact-*` paths. The earlier binary copied from
`target/release` gave an anomalously high footprint; it was excluded from
this comparison after rebuilding both source versions explicitly.

## CPU grain storage follow-up

In f64 mode, `layer_particle_model` now stores each Poisson `u64` in the bits
of the f64 output plane. After the whole-image Poisson pass, the binomial pass
recovers each integer with `to_bits` and replaces that slot with its final
grain value. This preserves the RNG sequence and removes one 8-byte-per-pixel
allocation per active channel. The f32 path still uses separate seed and
output planes because its output elements are half the size of `u64`.

On the same Q3 input, matched normal CPU exports measured 9.14 → 7.96 GiB
peak process footprint (1.18 GiB less) and 34.75 → 32.59 s wall time. These
are single runs; the timing difference is not a speed claim. The TIFFs were
byte-identical (SHA-256 `333eedff0b3a0394e6623d175fb8d43950673fd51a88fc98ccb4a6a06e961ca7`).
The four synthetic f64 parity cases also matched the pre-change binary under
both parallel and serial grain. Artifacts are under the ignored
`target/memory-study/grain-*` paths.

## Owned CPU blur follow-up

The CPU Gaussian blur now accepts an owned image at grain and scanner blur
call sites, extracts and blurs the same channel planes, then writes them back
into the input RGB allocation. This avoids allocating one more interleaved
full-image result at those call sites. Backends that do not override the owned
method retain the borrowed blur behavior.

The full Q3 f64 export remained byte-identical to the preceding grain-storage
version. Peak process footprint was 7.958 → 7.955 GiB and wall time was
32.59 → 33.02 s in single runs. The overall peak did not materially change;
the allocation saving is local to blur. All four synthetic f64 parity cases
matched in both grain scheduling modes. Artifacts are under the ignored
`target/memory-study/channel-*` paths. The final f32 and f64 workspace test
suites passed.

## Next investigations

These are source-based opportunities, not implemented or measured savings.
Keep the full-resolution preview and current precision, calculations and
default export speed. Use a fresh pre-change binary for each comparison.

1. **Stage-local channel layout and scratch.** `gaussian_blur` still extracts
   three planes and allocates pass buffers; DIR's Gaussian/exponential path
   also repeats channel extraction. Reusable channel scratch or an interleaved
   blur can reduce transient storage further.
   Compare peak live bytes, footprint and output bits before changing the
   shared `ImageBuf` layout. Per-pixel spectral, LUT and colour operations
   already use its contiguous RGB layout, so a global planar conversion
   may add staging copies instead of saving memory.

SIMD is a separate, profiling-led speed experiment. The CPU spectral path
already tiles its 81-wavelength workspace and uses BLAS plus vForce; many
per-pixel operations use Rayon. The default grain sampler advances a seeded
RNG in a fixed order, so vectorizing or parallelizing its draws can change
the image. Packed half-precision or normalized-integer images would change
the established f32 GPU / f64 CPU calculations and need separate approval.
