# RAW decoding on macOS and other platforms

`spektrafilm-io::raw::RawDecoder` is the platform-neutral development boundary.
A backend receives a file and `RawDecodeOptions` (white balance and orientation)
and returns a full-resolution, top-row-first, linear-sRGB `ImageBuf`. It must
not return a camera JPEG preview. The film engine, preview sizing and export
writers do not depend on native framework types.

`RawDecoderPreference::Auto` selects Apple's implementation on macOS and rawler
elsewhere. An explicitly unsupported Apple profile may fall back to rawler;
I/O and failed rendering remain errors. The returned `DecodedRaw` records the
actual backend used. Explicit `Apple` or `Rawler` requests never fall back.
Additional platform/vendor implementations can implement `RawDecoder` and be
registered in this routing module without altering film processing.

The existing `load_image_with_wb`, `load_raw`, `load_raw_with_wb`, and
`load_raw_unoriented` APIs use Auto. The native helper, cross-platform GUI, CLI
and f64 exporter therefore share the same policy and white-balance processing.
There is no preview-only Apple path or external conversion subprocess.

## Apple development and quality contract

The macOS-only Objective-C bridge is statically linked into each Rust executable
and links system frameworks. No Swift runtime or separate decoder installation
is needed. The `apple-raw` Cargo feature is on by default but only builds native
code on macOS. `--no-default-features` builds a portable rawler-only IO crate.

Apple runs at native size with draft mode off. Baseline exposure, exposure,
shadow bias, global/local tone boosts, gamut mapping, lens correction, noise
reduction, sharpening, contrast, detail and moire adjustments are explicitly
disabled where supported. Apple's highlight colour recovery remains enabled:
it reconstructs clipped sensor channels and prevented a pink saturated sky in
the D750 comparison. It is part of the RAW development, not a tone boost.
Product policy confirmed on 2026-10-19: keep highlight reconstruction enabled
where supported, with no user-facing option for now. The rawler fallback has
no equivalent reconstruction stage; this decision does not add one.
This preserves a linear input for film simulation rather than applying Apple's
default photographic look. Noise is intentionally retained.

Core Image works and renders in extended linear sRGB at float32 RGBA precision;
the bridge drops alpha into float RGB without an 8/16-bit image roundtrip.
Rendering is checked through `CIRenderTask` completion. Rust rejects non-finite
pixels and clamps negative values to zero, matching the existing loader's
contract. There is no upper clamp in the bridge or Rust conversion. The f64
export widens these same decoded f32 values; it does not invent additional RAW
precision. Apple's internal camera processing remains Apple's implementation.

As Shot retains Apple's camera white balance. The other modes request a D65
neutral and share the established CAT02/ACES adaptation/tint implementation.
Apple's orientation is applied once (or explicitly disabled); its active-area
crop can differ from rawler. Do not infer a loss of resolution from a comparison
between stored sensor borders and the active photograph.

Apple and rawler use different demosaicing and camera calibration, so their RGB
outputs need not match. Numerical differences are not a quality ranking. Camera
JPEGs also contain a separate look. Quality assessment should cover orientation,
detail, shadow colour, high-contrast edges and highlight range at actual size,
with the exact input and rendering settings recorded. Camera/profile support
is determined at runtime and depends on macOS; this implementation does not
claim universal HE★ support on every supported macOS release.

The bridge bounds dimensions before allocating its output (maximum 200 MP).
Apple's internal allocations and decoding are controlled by the OS framework.

## Reproducing comparisons

```bash
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --release -p spektrafilm-io --example raw_decoder_probe
target/release/examples/raw_decoder_probe --decoder apple --iterations 6 /path/photo.NEF
target/release/examples/raw_decoder_probe --decoder rawler --iterations 6 /path/photo.NEF
python3 scripts/parity/compare_raw_decoders.py --output-dir /tmp/raw-study \
  --input 'he_star=/path/HE.NEF' --input 'lossless=/path/lossless.NEF' --artifacts
```

The probe measures the complete shared RAW loading path, excluding statistics,
artifact writing and film rendering. First-call latency is separate from the
median of later iterations. Fresh process does not mean a purged disk or shader
cache. The macOS script also measures process peak resident memory; that includes
probe statistics buffers and may omit GPU allocations. Failed HE★ decoding with
stock rawler is recorded as unsupported, not represented as a speed comparison.

`--output DIR` stores private float32 linear-RGB data, a display preview and a
JSON report for closer inspection. `--no-orient`, `--wb`, `--temperature` and
`--tint` exercise development options. Do not publish personal photographs or
their absolute paths when sharing reports.

Tests include fallback classification, malformed WB, existing generated DNG
preview/export parity, portable builds, and an explicitly opt-in real-file test:

```bash
SPEKTRAFILM_RAW_TEST_FILE=/path/photo.NEF MACOSX_DEPLOYMENT_TARGET=14.0 \
  cargo test -p spektrafilm-io apple_raw_orientation_white_balance_and_highlights -- --ignored
```

The real-file test verifies upright/unoriented consistency, exact Daylight /
Custom 6504 K identity, changed custom WB and finite output. Missing input fails
that requested test. Normal CI intentionally marks it ignored rather than
pretending that proprietary camera fixtures were tested.

Primary references:
- [CIRAWFilter](https://developer.apple.com/documentation/coreimage/cirawfilter)
- [Baseline exposure and linear response](https://developer.apple.com/documentation/coreimage/cirawfilter/baselineexposure)
- [CIRenderDestination](https://developer.apple.com/documentation/coreimage/cirenderdestination)

Public support/performance research comparing Apple with LibRaw is recorded in
[the LibRaw comparison](libraw-apple-public-comparison-2026-10.md).
