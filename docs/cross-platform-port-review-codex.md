# Codex review of the cross-platform plan (gpt-6-astra, high, 2026-10-22)

I recommend **tight numerical equivalence for the Windows port, with separate treatment of grain and f32 paths**. I would not make a wholesale portable-math rewrite the prerequisite for porting. The current plan overstates both the completeness of that determinism fix and the confidence of its performance estimates.

I verified the manifest counts and inspected the retained CPU dumps. No files were modified, and I ran no GPU code.

1. **Determinism: preserve performance initially, but define the guarantee precisely.**

   Correctness does not require matching Apple’s last bits. Conversely, tolerance cannot satisfy a literal promise of byte-identical exports. Keep exact regression checks for a fixed build/platform, and use numerical checks across platforms. If identical files are a product requirement, implement a deliberately specified deterministic export path.

   The proposed portable implementation is incomplete:

   - **Scalar FFT planning alone is insufficient.** It fixes algorithm selection, but rustfft still constructs twiddles using system `sin`/`cos`. Both FFT callers need attention, including LUT construction. See [fft_conv.rs:63](crates/spektrafilm-math/src/fft_conv.rs:63), [input_gamut.rs:63](crates/spektrafilm-core/src/input_gamut.rs:63), and the installed [rustfft twiddle implementation:11](~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rustfft-6.4.1/src/twiddles.rs:11).
   - **Pure Rust does not establish determinism.** `libm` enables architecture-specific implementations by default. Audit the functions and enabled features rather than treating the dependency name as a guarantee. See [libm Cargo.toml.orig:18](~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/libm-0.2.16/Cargo.toml.orig:18).
   - **The f64 engine contains consequential f32 calculations.** Auto-exposure returns f32 and applies f32 `powf`; borders independently use f32 transcendentals. Replacing only f64 math misses these. See [filming.rs:95](crates/spektrafilm-core/src/stages/filming.rs:95), [filming.rs:297](crates/spektrafilm-core/src/stages/filming.rs:297), and [film35.rs:172](crates/spektrafilm-io/src/film35.rs:172).

   **The cost estimates are plausible hypotheses, not measured end-to-end results.** Applying the document’s own ratios mechanically gives roughly:

   `1 + 0.272 × (38.2/4.1 − 1) + 0.124 × (40.2/10.5 − 1) ≈ 3.61×`

   That is already above the quoted 2.5–3.5× before other changes. However, active-CPU sample shares are not wall-time fractions, so this is only a consistency check. The microbenchmarks need representative input distributions, compiler flags and executable sources before supporting a forecast. See [plan:143](docs/cross-platform-port.md:143).

   The claim that portable dgemm costs “at most a few percent” is incorrect reasoning: if a component occupying 3.6% becomes ten times slower, the total increases by approximately 32%, under the same simplified model. FFT’s negligible share in the default render says nothing about diffusion-heavy exports.

   **There are cheaper deterministic approaches worth measuring.** Start with specialized deterministic `exp10`, rather than general `pow(10,x)`: the supplied microbenchmark suggests approximately 22% additional total cost for that substitution alone, instead of approximately 226%. Optimize the actual narrow GEMM shapes, vectorizing across independent pixels/output entries while preserving each reduction’s order. Precompute invariant LUTs or coefficients where the input set really is fixed.

   A fast ARM-derived `pow` is a credible candidate, but its FMA and non-FMA branches are different algorithms with different error bounds. They are not automatically bit-identical. CORE-MATH is another candidate, but its correctly rounded result depends on preserving its arithmetic assumptions. Neither justifies a +15–40% promise without testing native Windows x86, including a baseline without hardware FMA. [ARM implementation](https://raw.githubusercontent.com/ARM-software/optimized-routines/master/math/pow.c), [CORE-MATH implementation](https://raw.githubusercontent.com/lntue/core-math/master/src/binary64/pow/pow.c).

   Specify fused versus unfused operations explicitly. A software implementation of correctly rounded FMA can preserve results on non-FMA hardware, but can be expensive. SIMD is compatible with determinism when it preserves the specified per-lane computation; architecture-selected polynomial approximations and reduction trees need separate verification. Rust’s explicit `mul_add` promises a single rounding, unlike separate multiplication and addition. [Rust documentation](https://doc.rust-lang.org/std/primitive.f64.html#method.mul_add).

   **Do not retain macOS-only `vvpow` on the strength of empirical agreement.** The current comment explicitly describes empirical verification against Apple scalar `pow`, not proof against a portable implementation ([vforce.rs:7](crates/spektrafilm-math/src/vforce.rs:7)). Restricting portable math to selected call sites is useful incremental work, but guarantees identical exports only after every remaining platform-sensitive path has been eliminated or bounded sufficiently to prove identical quantization. Simply rounding intermediate values more coarsely moves the disagreement boundaries.

2. **Tolerance: use strict stage checks, and never silently discard a percentage of outliers.**

   The assertion that relative `1e-9` “still catches any real porting bug” is too strong. Relative-only comparisons are also troublesome near zero. Use mixed absolute/relative bounds, plus distribution and spatial diagnostics.

   These are reasonable **initial gates to validate on Windows**, not established universal limits:

   | Observable | Initial policy |
   |---|---|
   | Smooth f64 stages and final f64 output | Every value satisfies `abs(a-b) ≤ 1e-12 + 1e-11 × max(abs(a),abs(b))`; also require RMS of `(a-b)/(1+max(abs(a),abs(b))) ≤ 1e-12`. |
   | f32 CPU smooth output | Separate gate, initially `2e-6 + 2e-6 × max(abs(a),abs(b))`, with normalized RMS ≤ `2e-7`; tighten from measurements. |
   | Identified f32 border calculations | Local f32-ULP comparison, initially ≤4 ulps, restricted to the affected operation/region. Preserve strict f64 checks elsewhere. |
   | Dimensions, channel layout, orientation | Exact. |
   | NaN/Inf | Explicit failure for finished images; never let equality or aggregate statistics hide them. |

   Record maximum error and its coordinates, RMS, percentiles, signed channel means and spatial clusters. Keep stage-specific limits: an FFT intermediate and a normalized encoded RGB image have different scales.

   For TIFF/PNG, compare decoded integer pixels and metadata separately from file hashes. Normally require exact pixels. Permit a one-code difference only where the approved raw difference crosses the actual rounding boundary. Independently verify that each file’s pixels equal quantization of its own raw buffer. The exporter clamps and uses ties-to-even at [save.rs:43](crates/spektrafilm-io/src/save.rs:43). A blanket “one code anywhere” rule could conceal systematic quantization bugs.

   JPEG needs a different check: compare its pre-JPEG u8 raster, and test the encoder on identical raster inputs. A one-code input change can affect several decoded JPEG pixels, so TIFF’s rule cannot simply be applied to decoded JPEG.

   **Grain can amplify a `1e-14` difference into much more than a rare local outlier.** The sampler consumes one shared stream across the whole Poisson array, then the whole binomial array. A changed rejection decision can shift subsequent draws, including the later binomial pass. See [grain.rs:46](crates/spektrafilm-model/src/grain.rs:46), [numpy_rng.rs:98](crates/spektrafilm-math/src/numpy_rng.rs:98), and [numpy_rng.rs:114](crates/spektrafilm-math/src/numpy_rng.rs:114).

   A concrete boundary example using the code’s seed-0 first uniform, `0.5488135039273248`:

   - Set `density_max=1`, uniformity `0.97`, particles/pixel approximately `0.3089985382430351`.
   - Densities `0.499999999999995` and `0.500000000000005` produce Poisson rates approximately `0.5999965965916172` and `0.5999965965916284`.
   - The first comparison against `exp(-lambda)` takes opposite branches: one returns zero; the other consumes another uniform.

   I checked that boundary arithmetic in memory. It follows directly from [grain.rs:66](crates/spektrafilm-model/src/grain.rs:66); the seed sequence is recorded at [numpy_rng.rs:331](crates/spektrafilm-math/src/numpy_rng.rs:331).

   Therefore, test grain separately: strict pre-grain density comparison; identical-input sampler tests; Poisson/binomial counts and draw-consumption diagnostics at the first divergence; deliberate probes around `lambda=10`, `p=0.5`, and `n*p=30`. An explained stream divergence may justify a separately reviewed statistical-equivalence test, but must not automatically pass under a “rare outliers” allowance. Per-pixel random streams would limit propagation, but change the existing NumPy-compatible model.

3. **The harness is useful, but currently unsafe as a release gate.**

   I reproduced three false passes using in-memory manifests:

   - Equal raw hashes but different TIFF hashes return success.
   - Different input hashes only print a warning and can return success.
   - Additional candidate cases are ignored.

   These follow from [run.py:196](scripts/golden/run.py:196). Compare both artifacts independently, reject incompatible input sets, compare case/engine sets symmetrically, and check dimensions even when hashes match.

   Other concrete issues:

   - **Render failures do not make `run` fail.** They are recorded and printed, but execution reaches successful completion. A filter producing zero cases also succeeds. See [run.py:145](scripts/golden/run.py:145).
   - **The provenance is insufficient.** Binary/input hashes are useful, but record source revision plus dirty state, lockfile/toolchain/features/target flags, effective parameters, and hashes of profiles/LUTs/border assets. The Rosetta manifest actually reports `machine: arm64`, because it records Python’s environment, not the renderer’s architecture ([Rosetta manifest:3](scripts/golden/manifests/macos-x86_64-rosetta.json:3)).
   - **Thread reporting can be wrong.** Without `--threads`, an inherited `RAYON_NUM_THREADS` survives, while metadata records `os.cpu_count()`. BLAS threading is separate. Inherited stage-dump variables can also make parallel subprocesses write the same diagnostic files. See [run.py:128](scripts/golden/run.py:128).
   - **Windows basics are mostly correct:** `.exe` handling and argument-list subprocess invocation are appropriate. But `Path.replace` can fail when the temporary directory and `--keep-dir` are on different volumes. Resolve executable paths, use explicit subprocess text encoding, add a timeout, and preserve complete failure logs. See [run.py:117](scripts/golden/run.py:117).
   - **Numeric diagnostics need strengthening.** `dump_stats` uses native-endian doubles, allocates a Python tuple for every differing value, and handles NaN/Inf poorly. It can consume enormous memory on full-resolution exports. Use streaming statistics and an explicit dump format. Python 3.9 itself appears sufficient; I found no inherent Python-version-dependent render math. See [run.py:184](scripts/golden/run.py:184).

   The recursive parameter merge is sensible, and omitted Rust fields are defaulted through serde. The problem is that effective parameters and preset contents are not captured, and the corpus/matrix is discovered dynamically. Adding a lexically earlier photo changes which image exercises every effect and layout ([run.py:82](scripts/golden/run.py:82)).

   **The generated TIFFs do not appear to contain volatile timestamps or output paths.** The synthetic generator supplies dimensions and pixels; exported metadata uses a fixed software version and source metadata. TIFF output and raw dumps are native-endian, which is fine for the listed little-endian targets but not a universal byte-portability guarantee. See [golden_inputs.rs:92](crates/spektrafilm-cli/src/bin/golden_inputs.rs:92), [metadata.rs:93](crates/spektrafilm-io/src/metadata.rs:93), and [save.rs:128](crates/spektrafilm-io/src/save.rs:128).

   The synthetic patterns have fixed arithmetic order, but “IEEE-exact” is inaccurate wording: division by 767 and constants such as 0.2 involve rounding. Fixed IEEE operations can still be reproducible. RAW-derived inputs are only as reproducible as decoding; they also intentionally narrow to f32 before box averaging. Duplicate RAW stems overwrite one another, and extreme downscale settings can produce a zero dimension. See [golden_inputs.rs:44](crates/spektrafilm-cli/src/bin/golden_inputs.rs:44) and [golden_inputs.rs:71](crates/spektrafilm-cli/src/bin/golden_inputs.rs:71).

   **Coverage is substantially narrower than “every effect/export path.”** The matrix exercises float TIFF, linear sRGB, seven stock pairs, selected presets and four layouts. Add actual RAW decoding/WB/orientation, PNG and integer TIFF inputs, JPEG/PNG output, metadata-bearing files, alternate input/output spaces, post adjustments, alternate upsamplers, LUT modes, quarter-turn rotations, border variants, and combined effects. Include odd/tiny dimensions and full-resolution cases: shrinking changes physical pixel size, grain rates and blur branches. The input loader has materially different PNG/RAW paths at [lib.rs:112](crates/spektrafilm-io/src/lib.rs:112).

   `--raw-out` is the right **final pre-quantization observable**, although the CLI writes it after saving the image. It already contains crop, rotation, border and resize; those are not downstream omissions. Add a separate pre-layout dump to isolate the engine. See [main.rs:330](crates/spektrafilm-cli/src/main.rs:330).

   A `gpu` engine entry does not establish GPU coverage: backend selection can silently fall back to CPU, and the harness never checks the actual backend. Require adapter/backend identity and confirmation that the resident path ran. Existing stage dumps are bypassed by that path. See [gpu/lib.rs:450](crates/spektrafilm-gpu/src/lib.rs:450) and [pipeline.rs:525](crates/spektrafilm-core/src/pipeline.rs:525).

4. **The cross-architecture result is real, but its explanation needs correction.**

   I independently confirmed: identical input hashes; 116/116 f64 raw hashes differ; 115/116 f64 TIFF hashes match; all 116 f32 TIFF hashes differ. The border dumps’ hashes match their manifest entries ([arm64 entry:151](scripts/golden/manifests/macos-arm64.json:151), [Rosetta entry:151](scripts/golden/manifests/macos-x86_64-rosetta.json:151)).

   **The “one f32 ulp on 55% of values” interpretation is wrong.** In the retained border dumps:

   | Measurement | Result |
   |---|---:|
   | Total channel values | 2,791,890 |
   | Any difference | 1,535,021 — 54.98% |
   | Difference greater than `1e-10` | 7,717 — 0.276% |
   | Maximum absolute difference | `1.1920928955078125e-7` |

   Most differences are tiny photograph differences. Almost all larger differences lie outside the centered photograph. The border independently evaluates f32 `powf` and `exp`, then composites in f32: this is a much stronger explanation than widespread amplification of incoming `1e-14` noise. See [film35.rs:172](crates/spektrafilm-io/src/film35.rs:172), [film35.rs:619](crates/spektrafilm-io/src/film35.rs:619), and [border.rs:278](crates/spektrafilm-io/src/border.rs:278).

   The retained grain-disabled stage dumps reproduce the README’s exposure result: 23.396% differ, maximum `4.44e-16`; final maximum is `9.10e-15`.

   **My first suspect is the precomputed TC LUT.** Film sensitivity already uses platform `10^x`, then the default windowed LUT is built using Accelerate dgemm, before `expose` runs. The per-pixel Hanatos path performs ordinary matrix arithmetic and bicubic lookup; `expose` finishes with `log10`. `vvpow` is not a direct operation in that default exposure path. See [pipeline.rs:239](crates/spektrafilm-core/src/pipeline.rs:239), [spectral_service.rs:288](crates/spektrafilm-core/src/spectral_service.rs:288), [spectral.rs:985](crates/spektrafilm-math/src/spectral.rs:985), and [filming.rs:400](crates/spektrafilm-core/src/stages/filming.rs:400).

   Cheapest investigation: one tiny fixed image; dump sensitivity, TC LUT, exposure scale and raw values immediately before `log10`. If pre-log values agree, replay those exact operands through each platform’s `log10`. If not, compare LUT construction inputs/output before investigating per-pixel arithmetic. For the border, render the same constant image on both architectures and compare border-only pixels. Neither investigation requires a full 232-render rerun.

5. **Windows: Win32 is a reasonable default; establish engine behavior before building the full UI.**

   Standard controls fit the stated priorities. Keep WinUI as a measured alternative, not an unresolved architectural dependency. The spike should measure first painted frame, first responsive interaction and first usable image separately, including helper startup, catalog loading and shader compilation. A blank-window time is insufficient.

   DPI scaling is not entirely “native for free.” Specify per-monitor DPI awareness, layout/font updates and image-surface resizing; test keyboard operation and accessible names. Microsoft explicitly documents the additional work required by Win32 applications. [DPI guidance](https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows).

   **Test DX12 and Vulkan explicitly on the available GPU; do not prefer Vulkan merely to avoid FXC.** DX12 with bundled DXC is a reasonable shipping candidate. The current code hardcodes `Backends::PRIMARY` and default backend options, so ordinary `WGPU_BACKEND`/compiler environment settings are not automatically a reliable selector. Add deliberate configuration and log what actually ran ([wgpu_backend.rs:305](crates/spektrafilm-gpu/src/wgpu_backend.rs:305)).

   The FXC statement is correct for the installed wgpu 24 code. Dynamic DXC requires **both** `dxcompiler.dll` and `dxil.dll`, and missing DLLs can fall back to FXC. Verify the packaged application outside the development environment. See [wgpu-types instance.rs:270](~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-types-24.0.0/src/instance.rs:270).

   **The D3D12 buffer-limit explanation is wrong for this dependency version.** wgpu-hal 24 reports approximately 2 GiB limits here; its source explicitly says DX12 does not expose a maximum buffer size. VRAM residency/budget is a separate constraint. See [DX12 adapter.rs:501](~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-hal-24.0.4/src/dx12/adapter.rs:501) and [adapter.rs:541](~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-hal-24.0.4/src/dx12/adapter.rs:541).

   The 100 MP test only asserts reported limits; it allocates nothing. It therefore neither proves a render fits nor constitutes an appropriate universal hardware requirement. Runtime admission needs per-binding checks and peak memory estimates, including ping-pong buffers, effect scratch, padded FFT storage and staging/readback. The code allocates additional whole-image scratch at [wgpu_backend.rs:1297](crates/spektrafilm-gpu/src/wgpu_backend.rs:1297); the limit-only test is at [wgpu_backend.rs:5127](crates/spektrafilm-gpu/src/wgpu_backend.rs:5127).

   Initially, use a conservative capacity check and explicit CPU fallback. Tiling requires careful boundary handling: FFT convolution, recursive blurs, global exposure statistics and sequential grain streams cannot all be split into independent tiles without changing results.

   TDR is a failure to complete **or preempt** work within the timeout, not simply “a render lasted two seconds.” Measure individual dispatches and submission behavior, retain cancellation/responsiveness, and recover from device loss; do not solve this by changing users’ TDR settings. [Microsoft TDR documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/timeout-detection-and-recovery).

   **There are two Windows scalar dgemms.** Besides the parallel pixel implementation, LUT construction has a serial triple loop in [spectral_service.rs:41](crates/spektrafilm-core/src/spectral_service.rs:41). Benchmark both. The pixel implementation ignores `ldb`; current tightly packed call sites are compatible, but it is not a general replacement for its BLAS interface ([cpu_backend.rs:72](crates/spektrafilm-gpu/src/cpu_backend.rs:72)).

   On the Windows machine, I would first record CPU features, GPU/driver/backend limits and the actual adapter visible in the remote session; build the locked MSVC binaries; run repaired CPU goldens; profile full-resolution CPU exports; then run explicit DX12/DXC and Vulkan correctness/capacity tests. Software adapters add valuable coverage but do not validate the physical GPU’s driver, memory behavior or performance. The reviewer with remote access can conduct these checks and the startup spike directly.

6. **Other important omissions, ranked by severity.**

   **High: the native-helper protocol is not the whole export architecture.** Its `Save` command saves the cached rendered image. The Mac’s reference export separately launches `spektrafilm-f64`. Porting only the JSON helper client would omit that behavior. See [native/main.rs:146](crates/spektrafilm-native/src/main.rs:146) and [Engine.swift:319](macos/Sources/NegsApp/Engine.swift:319).

   **High: define RAW equivalence and display color management.** Apple-versus-rawler differences are product-visible, not merely test noise. Tests on copied float TIFFs isolate the engine but do not validate the Windows import experience. Specify the cross-platform decoder policy, WB/orientation behavior, and monitor-color/HDR handling.

   **Medium: portable host math can affect GPU rendering too.** GPU paths share CPU-built LUTs and calibration. “GPU previews and GPU exports are not affected at all” is too absolute; their shader execution may remain unchanged while setup cost and uploaded values change ([pipeline.rs:194](crates/spektrafilm-core/src/pipeline.rs:194)).

   **Medium: persistent shader caching is proposed, not implemented.** Current pipeline creation passes `cache: None`; the existing cache is an in-process map. Include persistent-cache implementation and invalidation in the startup plan ([wgpu_backend.rs:497](crates/spektrafilm-gpu/src/wgpu_backend.rs:497)).

   **Medium: the estimated 5% file-flip probability is illustrative only.** It assumes roughly uniform distances to quantization boundaries and uses a maximum observed delta as though it represented every sample. Correlated errors, clipping, repeated values and discrete amplifiers invalidate that as a measured likelihood ([plan:178](docs/cross-platform-port.md:178)).

My ranked recommendations are:

1. **Repair the harness’s false passes and provenance before using it to approve a port.**
2. **Adopt strict stage-aware tolerances; investigate grain divergences explicitly.**
3. **Correct the border finding and isolate TC-LUT versus `log10` divergence with tiny probes.**
4. **Establish Windows CPU and explicit DX12/Vulkan baselines, capacity checks and recovery.**
5. **Proceed with standard-control Win32, including the separate reference-export workflow.**
6. **Benchmark specialized portable math before committing to a byte-identical export guarantee.**
