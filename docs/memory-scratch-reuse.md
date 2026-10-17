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
| DIR | A density consumed by matmul; A then blur intermediate; B log exposure corrected, then → A | S0 correction/tail, S1 Gaussian, S2 weighted mix; all consumed before grain |
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

## Next investigations

These are source-based opportunities, not implemented or measured savings.
Keep the full-resolution preview and current precision, calculations and
default export speed. Use a fresh pre-change binary for each comparison.

1. **GPU DIR mix lifetime.** In `build_dir_state`, `buf_a` is the blur
   intermediate and appears free after the second vertical blur. Try using
   it as the weighted-mix destination instead of `scratch[2]`. The final
   density interpolation overwrites `buf_a` later. If all dispatch bindings
   remain distinct, this could remove one 12-byte-per-pixel buffer (about
   0.67 GiB at Q3 size). Recheck both mappable and staging readback paths,
   all 22 GPU cases, the full Q3 TIFF and process footprint.
2. **CPU grain storage.** `layer_particle_model` retains a `Vec<u64>` of
   Poisson seeds while allocating a separate output plane for the binomial
   pass. Investigate safely reusing the seed allocation for the f64 output
   plane, or another exact representation that keeps whole-image Poisson
   draws before binomial draws. Three channels run concurrently by default;
   a single 60 MP seed plane is about 0.45 GiB. Preserve every RNG draw and
   bit-exact output in both grain modes. Benchmark default runtime because
   grain took about 15 of 33 pipeline seconds in the current Q3 run.
3. **Stage-local channel layout and scratch.** `gaussian_blur` extracts three
   planes, allocates pass buffers and interleaves a new image; DIR's
   Gaussian/exponential path also repeats channel extraction. Reusable
   channel scratch or an interleaved blur can reduce transient storage.
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
