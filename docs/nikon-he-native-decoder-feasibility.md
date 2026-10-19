# Nikon HE★ decoding through Apple's RAW implementation

Date: 2026-10-19. This records the initial feasibility probe. Integration was
subsequently implemented and tested; see [the decoder architecture](apple-raw-decoder.md)
and [the final quality/performance report](raw-decoder-comparison-2026-10.md).

The preferred next step for Spektrafilm on macOS is to reuse Apple's existing
RAW decoder rather than develop the experimental rawler codec first. The latter
remains a separate option for portable decoding or upstream contributions.

A standalone Swift probe used `CIRAWFilter(imageURL:)`, disabled draft mode,
set boost, shadow boost, exposure and local tone mapping to zero, and requested
full-size half-float RGB rendering in extended linear sRGB through `CIContext`.
This uses the RAW-processing API, not extraction of an embedded JPEG preview.

Observed on macOS 26.5.2 (25F84), RAW decoder version 8:

| Input | Apple native active size | Rendered output | Result |
| --- | --- | --- | --- |
| DSC_2533.NEF | 3984 × 2656 | 2656 × 3984 | Success, 16-bit floating-point components |
| DSC_1180.NEF | 3984 × 2656 | 3984 × 2656 | Success, 16-bit floating-point components |

The first input's output is portrait-oriented. Apple's active area and
orientation handling differ from the stored 4000 × 2672 un-oriented mosaic
measured in the rawler experiment. Both center patches contained nonzero finite
linear RGB values. Single-run elapsed times were 6.33 s and 0.235 s respectively;
these are cold/warm observations, not a benchmark.

Probe source: `/private/tmp/spektrafilm-apple-raw-probe.swift`.
Probe log: `/private/tmp/spektrafilm-apple-raw-probe.log`.
These temporary files should be preserved in an isolated implementation checkout
if implementation proceeds. Personal images remain in their original locations.

The successful test establishes that this OS can decode these two inputs into
high-precision RGB. It does not prove broad camera/profile support, numerical
equivalence to rawler, or a finished production colour configuration. Apple
performs demosaicing and development; its RGB output is not an independent
integer-Bayer oracle for validating a separate rawler codec.

Integration work should use a shared macOS decoder bridge for both preview and
export, preserve linear-sRGB/high-precision data, explicitly configure rendering
adjustments, map As Shot/custom white balance, and handle orientation/cropping
once. Inspect default baseline exposure, noise reduction, sharpening, lens
correction and highlight processing rather than assuming a zero boost disables
everything. Test capability on the actual file and report unsupported OS/profile
combinations clearly. Existing rawler-supported inputs can keep their decoder.

Verify the packaged application, RAW white-balance edits and f64 export on both
files; ensure the same decoder/configuration is used by all execution paths.
Validate preserved metadata and the linear colour contract with independent
checks. These integration checks replace the need to finish a custom HE codec
for the macOS product goal; they do not require matching Apple's demosaicing
pixel-for-pixel with rawler's different algorithms.

Apple's framework is a macOS-specific dependency. Windows/Linux support needs
a separate backend or a conversion workflow. Existing alternatives to assess
include Nikon's NEF/NRW Image SDK (free within its agreement, specific HE support
and redistribution terms still to be checked) and Adobe DNG conversion before
loading through rawler. Neither was tested in this experiment.

Primary documentation:
- [Apple CIRAWFilter](https://developer.apple.com/documentation/coreimage/cirawfilter)
- [Nikon Image SDK information and FAQ](https://sdk.nikonimaging.com/information/en/)
- [Adobe DNG Converter](https://helpx.adobe.com/camera-raw/desktop/dng-and-file-formats/adobe-dng-converter.html)
