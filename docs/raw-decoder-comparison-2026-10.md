# Apple RAW decoder: quality and performance

Measured 2026-10-19 on Apple M1 Pro, macOS 26.5.2. Implementation details and
reproduction commands: [decoder architecture](apple-raw-decoder.md).
Machine-readable results: [comparison JSON](raw-decoder-comparison-2026-10.json).

## Performance after integration

These measurements call the actual shared loader from the release Rust probe,
including native rendering, RGBA-to-RGB conversion and white balance. They
exclude film simulation, statistics and artifact writing. Each backend ran in
a fresh process for six iterations, sequentially; the warm figure is the median
of iterations 2–6. Disk and OS shader caches were not purged. This is a local
comparison on a shared computer, not an isolated benchmark or a universal speed
claim. An earlier exploratory run had more variation; the complete final-run
ranges are included in the JSON.

| Input | Active image | Apple warm median | rawler warm median | Apple latency change |
| --- | --- | ---: | ---: | ---: |
| Z f HE★ portrait | 2656 × 3984 | 208 ms | Unsupported | Newly supported |
| Z f HE★ landscape | 3984 × 2656 | 208 ms | Unsupported | Newly supported |
| Z f lossless | 6048 × 4032 | 298 ms | 388 ms | 23% lower |
| Z6 lossless | 6048 × 4024 | 296 ms | 362 ms | 18% lower |
| D750 lossless | 4016 × 6016 | 314 ms | 385 ms | 18% lower |
| 60 MP DNG | 9520 × 6336 | 1348 ms | 1018 ms | 32% higher |

First-call latency was higher: Apple took 0.49–1.67 s in this final run, versus
0.44–1.10 s for successful rawler inputs. Earlier first encounters with some
camera profiles took approximately 5–6 s, consistent with OS/framework startup
and shader preparation. Warm timings should not be presented as initial-open
latency. A later fresh-process full export measured 3.35 s for RAW loading.

Peak process resident memory in the probe (including its own statistics/copy
buffers, excluding unaccounted GPU allocations):

| Input | Apple | rawler |
| --- | ---: | ---: |
| Z f HE★, each | about 538 MiB | No successful decode |
| Z f lossless | 901 MiB | 953 MiB |
| Z6 lossless | 895 MiB | 946 MiB |
| D750 lossless | 891 MiB | 1211 MiB |
| 60 MP DNG | 2267 MiB | 2197 MiB |

The Apple path improves format support and several measured cases; the large
DNG remains a regression in both latency and resident memory. The routing API
retains an explicit rawler choice for callers, and the portable backend is not
removed. Auto uses Apple on macOS rather than guessing the fastest backend per
photograph, keeping colour processing consistent.

## Quality observations and checks

Both previously rejected HE★ files render correctly through the production
loader, including portrait orientation. Comparison views included skin, hair,
rock texture, sky and high-contrast edges, with 100% Apple/rawler crops on the
lossless examples. Both implementations retain fine detail; their colour and
exposure calibrations differ. This small visual sample does not establish a
universal quality ranking or numerical equivalence of their demosaicing.

The bridge preserves full active resolution and float32 linear RGB, with no
JPEG/8-bit/16-bit intermediate. All six Apple outputs were finite. Values above
1 survive to the film engine: the HE★ landscape has 136,786 RGB components above
1, and its red maximum exceeds 2.3. Display previews clamp for viewing; the
linear arrays do not. Negative values are floored at zero, preserving the
existing input contract. f64 export widens the decoded f32 values before film
processing, as the previous rawler loader did.

An important observed improvement during development was saturated-highlight
colour. Disabling Apple's highlight recovery made a blown sky pink in the D750
sample. Enabling it restored a neutral-looking white sky. The shipped policy
keeps Apple's highlight colour reconstruction enabled while disabling tone
boosts, noise reduction, sharpening and other photographic look adjustments.
This intentionally retains sensor noise instead of smoothing it away.

Camera JPEG appearance is not the target: JPEGs include a separate tone/colour
look. The goal is high-precision linear input for Spektrafilm. Compression already
present in the source HE★ is not reversed by switching decoders.

Personal previews, linear arrays and comparison crops stay under the private
temporary study directory; they are not included in the source repository or
application bundle.

## Functional validation

- IO tests: 46 passed, with real-file tests explicitly marked ignored in normal
  CI. The rawler-only configuration also passed 46 tests.
- The real Apple orientation/WB test was explicitly run on each HE★ file. It
  verifies upright/unoriented consistency, exact Daylight/Custom 6504 K identity,
  changed custom WB and finite output.
- The f64 IO suite and existing CLI/native-helper tests passed.
- The release native helper imported both HE★ files, rendered them through the
  GPU film pipeline and saved a preview TIFF. Full active source dimensions
  were preserved before the explicitly requested quarter-size test preview.
- A packaged CPU f64 export produced a full-resolution 3984 × 2656 TIFF from
  the HE★ landscape, retaining Nikon Z f metadata and Orientation=1. With grain
  disabled, that one run took 3.35 s to load, 5.50 s for film processing and
  8.98 s in total. This is a functional timing sample, not a paired film-speed
  benchmark. The output writer quantizes the final TIFF to its usual 16 bits;
  the decoder/film calculations are not performed at that precision.

The macOS application is packaged separately at
`dist/spektrafilm-apple-raw-20261019/Spektrafilm.app`, with its f32 helper and
f64 exporter linked against the same bridge. Bundled executable signatures and
the application bundle were verified by the packaging script. The GUI displayed
the HE★ portrait at 2656 × 3984 with the normal film simulation active. Windows/Linux
were not executed on this Mac; their cfg-independent rawler code path was
checked with the native Apple feature disabled.
