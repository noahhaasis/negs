# Notes from vkdt `filmsim` — possible GPU speedups

Read on 18 October 2026 against a local vkdt checkout at `d327ddf4`
(`src/pipe/modules/filmsim`). Nothing here is implemented yet.

vkdt's `filmsim` is a Vulkan reimplementation of the same model (agx-emulsion /
spektrafilm) by hanatos. It is fast mainly because it approximates more than we
do, so **do not port its model**. Two engineering ideas are worth taking.

## Where vkdt is less accurate (don't copy)

| | vkdt `filmsim` | spektrafilm-rs |
|---|---|---|
| Wavelengths | 40, 10 nm steps (`#define SN 40`) | 81, 5 nm steps, as upstream |
| Density curves | Fitted sigmoids | Measured curves, interpolated |
| Enlarger filters | `smoothstep` shapes (`thorlabs_filters`) | Measured filter data |
| Grain | Hashed uniform noise, blurred in a 4×4 window, 3 sub-layers, Gaussian | Poisson/binomial particle model |
| Halation, couplers | One separable blur each, f16 buffers | Multi-component blurs, f32 / f64 |
| Auto white balance | Precomputed table per film × paper (`wb.h`) | Computed |

## Idea 1: pyramid blurs instead of wide FIR kernels

`filmsim` itself blurs halation and couplers with a plain separable FIR
(`dt_api_blur_sep`, shaders `shared/blurh.comp` / `blurv.comp`), like ours. It
is cheaper mainly because it does one blur per effect on 4-channel f16 textures,
and computes the Gaussian weights incrementally instead of reading a kernel
buffer.

vkdt's general-purpose blur (`dt_api_blur` in `src/pipe/modules/api.h`) is the
interesting part: it builds large Gaussians from a chain of 5×5 blurs on
progressively subsampled images, then finishes with a small separable blur.
The cost barely grows with radius.

**Status (19 October 2026):** halation, scatter and DIR now use `RefBlur`
(`crates/spektrafilm-gpu/src/wgpu_backend.rs`), which mirrors the CPU: FIR
below σ = 3, Young–van Vliet IIR above. IIR cost doesn't grow with σ, so this
already removed the wide-kernel cost for those effects. A pyramid blur would
break parity with the CPU reference, so it is no longer the plan for them.

Still on the old 3σ FIR (`gaussian_blur_gpu`, capped at 256 taps): unsharp
mask, lens blurs, glare blur, grain blur and the diffusion filter. These
measured within ΔE00 0.13 of the CPU, but could move to `RefBlur` for exact
parity.

Where `RefBlur` could still get faster:
- The IIR row pass runs one thread per row, so neighbouring threads read
  addresses a row apart. A transpose (tiled, through workgroup memory) before
  and after would let both passes read coalesced columns. The CPU already
  does this for its vertical pass.
- FIR channels at σ < 3 run one dispatch pair per channel; they could share one.
- Measure first: per-pass GPU timestamps, not end-to-end wall time.

## Idea 2: fewer, fused passes

vkdt runs the whole film → print → scan chain as 3 compute kernels plus the
blurs (`part0`, `part1`/`part1h`, `part2h`), with 16-bit intermediates. The
spectral products that every pixel needs are computed once per workgroup into
shared memory (`init_*_shared` in `filmsim.glsl`), and the spectral loops run
4 wavelengths at a time as `vec4`.

Our resident chain runs many separate passes, each reading and writing the full
image. Fusing per-pixel stages (for example, density interpolation → DIR matrix
→ print spectral) would cut memory traffic. Keep f32 intermediates; f16 is part
of why vkdt is less accurate.

## Licence

vkdt is BSD-2-Clause, with some GPL-3 files marked as such. We are GPL-3, so
borrowing ideas or code is fine. Keep the attribution.
