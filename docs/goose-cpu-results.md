# Standalone Goose grain results

The faithful Goose port passes bit-exact validation, but offers no full-grain speedup in the measured configurations and uses more resident memory. Completed on macOS arm64. These timings cover the complete native f64 grain call, with every required grain effect enabled. They exclude process startup, decoding, fixture loading, input restoration, pool initialization, checksums and any application/language bridge. Required Goose worker copies and dispatch are included.

| Frozen photo | CPU budget | Rust median s (min–max) | Goose median s (min–max) | Rust / Goose | Paired batch ratios |
| --- | ---: | ---: | ---: | ---: | --- |
| 6048×4024 | 1 | 41.38 (41.04–52.08) | 44.74 (44.43–51.78) | 0.925× | 0.974×, 0.924× |
| 6048×4024 | 4 | 13.04 (12.96–23.05) | 15.61 (15.53–15.64) | 0.835× | 0.835×, 0.835× |
| 9520×6336 | 1 | 103.69 (103.55–106.32) | 109.64 (109.26–111.36) | 0.946× | 0.955×, 0.946× |
| 9520×6336 | 4 | 49.63 (33.85–124.91) | 62.99 (57.58–81.57) | 0.788× | 0.639×, 0.649× |

A ratio above 1 means Goose was faster. This is one faithful implementation comparison on this machine; it is not a whole-export or general language benchmark.

The host was not CPU-affinity-pinned or otherwise isolated. The 60 MP parallel measurements and IIR diagnostics have substantial spread. Pooled median ratios are descriptive; paired batch ratios show the effect of collecting variants at different times. The data support the qualitative result above, but not a precise universal slowdown percentage or a causal explanation for the variation.

## Correctness

All 478 native differential cases have zero bit differences; both native loaders reject all eight malformed-file checks. The sampler corpus contains 146,960 values, including integer/uniform streams, switch boundaries, rejection paths, normal-cache reuse and next RNG draws for five seeds. Full-size outputs are bit-identical across both executables and budgets 1 and 4. No floating tolerance was needed.

The synthetic suite covers colour, positive interpolation and monochrome, 1/3/4 layers, 1×1, 1×N, N×1 and odd dimensions, zero RMS, zero-density layers, duplicate curve positions and NaN curve entries. FIR/IIR tests include identity, tiny sigma, wide kernels and either side of sigma 3. Rust’s existing sampler, model, Python-derived grain statistics and blur-edge baseline tests all passed (3, 5, 8 and 6 tests respectively).

## Fixtures and active effects

- nikon24: 6048×4024, 5.7870370370370372 µm/px, 3 layers; microstructure sigma 0.1728 (>0.05), microstructure blur 3.456 px (>0.4), dye-cloud sigma 0.089601–0.192386 px. Pixel blur 0.89 uses FIR; sharpening sigma 3.5, amount 1.5 uses IIR. Dye blur runs for every sampled layer; the finest layers’ rounded FIR radii can be zero.
- leica60: 9520×6336, 3.6764705882352939 µm/px, 3 layers; microstructure sigma 0.272 (>0.05), microstructure blur 5.44 px (>0.4), dye-cloud sigma 0.141039–0.302831 px. Pixel blur 0.89 uses FIR; sharpening sigma 3.5, amount 1.5 uses IIR. Dye blur runs for every sampled layer; the finest layers’ rounded FIR radii can be zero.

Both fixtures use resolved `kodak_gold_200` curves/settings from the same newly built CPU pipeline that emitted pre-grain density, with `kodak_portra_endura` selected during capture. RAWs were decoded with Rawler. The chosen capture recipe was unchanged. Pixel arrays and private paths are excluded from version control; only hashes and compact measurements are published here. Sanitized fixture manifests are retained in `experimental/goose-grain/fixtures/`.

## Measurement policy

Each variant has fourteen measured calls in two native process batches (seven per process), AB then BA, with one warmup per process. Each input is restored through a fresh deep copy immediately before its internal monotonic timer. Outputs are consumed after every call; the final full output is SHA-256 checked after every batch. Full-size equality was established before benchmark runs. Benchmarks ran sequentially, after builds, captures and validation completed.

Budget 1 executes directly in Goose and uses one Rayon worker in Rust. Budget 4 uses three persistent Goose channel workers plus the coordinator, versus four Rayon workers. The coordinator packs channel inputs and sends copied flat jobs; packing, queue copies, receive and HWC assembly are timed. Goose blur is serial within each channel, while Rust can parallelize interpolation/filter passes. The one-thread Goose path also packs channel inputs so it can use the same planar channel routine; that packing is timed. The additional throughput and memory costs are part of this implementation result. No pointer tokens or shared-memory workaround was used.

Goose v2026.3 is pinned to commit `8511ae34bdc9aabf77baaab80b4d659ea87835e2`, runtime `4d3da949e563`. Goose -O2 controls inlining; standalone generated C uses Apple clang 21.0.0, `-O3 -ffp-contract=off`. Rust 1.93.1 uses release opt-level 3, thin LTO and one codegen unit. Both use the default arm64 target CPU and strict floating evaluation. Dependency versions match the workspace lock; the separate experiment lock adds SHA-256 support.

Goose calls the identical pinned `libm` 0.2.16 `lgamma` scalar primitive directly, since its standard math module lacks it. This shared native math routine is timed on both sides. Grain, MT19937, distribution algorithms, blur and worker processing are implemented in Goose; no language bridge or cross-language runtime dispatcher is in either timer. Native support for fixture hashing/loading and RSS runs outside timing. See [provenance and notices](../experimental/goose-grain/NOTICE.md).

## Supporting diagnostics

| Operation (one thread) | Rust median ms (min–max) | Goose median ms (min–max) | Rust / Goose |
| --- | ---: | ---: | ---: |
| 24 MP single-plane blur, sigma 0.89 | 179.57 (175.82–367.00) | 420.60 (415.26–499.51) | 0.427× |
| 24 MP single-plane blur, sigma 3.5 | 1379.27 (661.10–1889.87) | 1012.38 (797.91–1855.48) | 1.362× |
| Sampler corpus (146,960 values) | 15.53 (14.00–19.40) | 15.02 (12.31–25.00) | 1.034× |

Sampler diagnostics include corpus output allocation/construction and RNG initialization. Blur diagnostics include scratch/output allocations; channel extraction and sigma parsing are outside both timers. These diagnostics do not replace complete grain timings.

## Memory

| Frozen photo | Budget | Rust peak / retained MiB | Goose peak / retained MiB |
| --- | ---: | ---: | ---: |
| nikon24 | 1 | 2514 / 2514 | 3344 / 3344 |
| nikon24 | 4 | 4185 / 4185 | 6316 / 6316 |
| leica60 | 1 | 6220 / 5990 | 8286 / 8286 |
| leica60 | 4 | 9442 / 9442 | 15649 / 15649 |

Peak is macOS `time -l` maximum resident set size for the entire process, including fixture loading and validation. Retained is the maximum Mach task resident size sampled immediately after a timed call, with immutable input and completed output still alive. These are resident-memory measurements, not allocation totals or isolated scratch peaks. Goose reserves 2 GiB per data stack: 27 main/global regions (54 GiB virtual) and 11 additional regions per worker. Budget 4 has 60 regions (120 GiB virtual), plus 1 MiB guard per region. These compiler-reported virtual reservations are separate from physical RSS; see `target/goose/goose-build.log`.

## Reproduction and limits

See [the experiment README](../experimental/goose-grain/README.md) for build, capture, validation and batch commands. Compact raw per-call durations, checksums, hashes, IQR, per-batch ratios, compiler settings and memory measurements are in [goose-cpu-results.json](goose-cpu-results.json). Generated private artifacts, toolchains and logs go under ignored `target/goose/` and can be removed after recording the results.

Measurements used source revision `38722c36f72ae3c6a0113c492b98a4c39032359e` (0.8.0). Publishing onto current main updated only the experiment lock’s five local workspace package versions to 0.9.0; measured numerical sources and registry dependency versions are unchanged. Sanitized fixture manifests preserve the original metadata hashes and numerical inputs’ payload hashes. Private pixel dumps and generated caches are disposable and are not committed.

This implementation and report cover macOS arm64. Windows/Linux were not measured; current hashing/RSS support is macOS-specific. Pools are persistent and initialized before timing. No application ABI or service-thread integration was added. Unrelated tracked/untracked work was preserved; experiment code is confined to `experimental/goose-grain/`, `scripts/goose/` and these result documents.

Official compiler evidence: [release v2026.3](https://github.com/aardappel/goose/releases/tag/v2026.3), [pinned language and runtime sources](https://github.com/aardappel/goose/tree/8511ae34bdc9aabf77baaab80b4d659ea87835e2).
