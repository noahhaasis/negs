# Apple and LibRaw: public evidence

Research date: 2026-10-19. This is a public-source comparison, not a new local
benchmark or an integration of LibRaw. Apple's path remains selected on macOS.
Highlight reconstruction remains enabled where supported, without a user option.

## Support

The published LibRaw release is [0.22.2](https://www.libraw.org/download).
Its [0.22 camera list](https://www.libraw.org/supported-cameras) explicitly
excludes HE/HE* for Nikon Z f, Z 6 III, Z 8 and Z 9. A proposed HE decoder is
still an [open pull request](https://github.com/LibRaw/LibRaw/pull/826), not
released support. Do not count forks or embedded JPEG previews as support in
the stock RAW developer.

[Apple's OS 26 list](https://support.apple.com/en-us/122870), published October
6 and described as current as of September 2026, lists these Nikon bodies.
It does not provide a comprehensive per-compression compatibility matrix.
Our separate local evidence establishes that the two Z f HE* files develop
through Apple's decoder version 8 on this Mac; it does not establish every
HE profile or older macOS release.

LibRaw documents legacy/specialist coverage absent from Apple's list, including
Sigma SD1/Merrill/Quattro models and many Sinar/Kodak cameras. Some entries,
such as Sigma SD9/SD10/SD14/SD15, explicitly provide raw decode only, so do not
equate the camera list with a complete RGB development guarantee. LibRaw's
list also assumes all features enabled; build configuration matters.

LibRaw's [0.22 release notes](https://www.libraw.org/download) document DNG 1.7,
including JPEG-XL compression, through optional Adobe DNG SDK 1.7 integration.
Apple promises basic DNG support, not universal coverage of DNG variants.
Its list also excludes certain high-resolution/multishot modes. Neither
published list establishes a strict superset of the other.

LibRaw can ship on Windows, Linux and macOS with a pinned library version;
Apple's framework and supported profiles depend on the installed Apple OS.
This makes LibRaw a plausible future portable backend, without resolving the
released HE-support gap.

## Performance

No public benchmark located provides a controlled comparison of the current
LibRaw developer against the Apple version-8 path used by Spektrafilm, on the
same files, hardware and output contract. No speed ratio is justified yet.

The [Phoronix LibRaw test definition](https://raw.githubusercontent.com/phoronix-test-suite/test-profiles/master/pts/libraw-1.0.0/test-definition.xml)
uses LibRaw 0.20. Its [runner](https://raw.githubusercontent.com/phoronix-test-suite/test-profiles/master/pts/libraw-1.0.0/install.sh)
runs 50 postprocessing repetitions of one NEF. LibRaw's
[benchmark implementation](https://github.com/LibRaw/LibRaw/blob/0.22.2/samples/postprocessing_benchmark.cpp)
times unpacking separately from repeated development. These results cannot
be compared directly with our full file-to-float-RGB timings.

[Apple's Core Image guidance](https://developer.apple.com/videos/play/wwdc2020/10008/)
describes Metal integration and context reuse. This supports a potential
acceleration advantage, not a measured advantage over LibRaw. Apple's newer
[RAW 9 presentation](https://developer.apple.com/videos/play/wwdc2026/305/)
describes additional neural processing and higher resource requirements;
version 9 requires opt-in. Those claims do not characterize our version-8
measurements.

The [LibRaw maintainer distinguishes](https://www.libraw.org/node/2831)
sensor decompression from RGB postprocessing and describes the supplied
postprocessor as a minimal example. Fast applications using LibRaw may use
their own RGB development, so their speed is not automatically rawpy's speed.

## Product implication and next measurement

Public evidence supports keeping Apple for the current macOS HE requirement
and considering LibRaw for broader portable support and upstream comparison.
It does not establish a performance reason to switch. Existing Apple/rawler
measurements are not Apple/LibRaw measurements.

A subsequent local comparison should measure the same full-resolution files
from opening through completed RGB output, including conversion/copies. Keep
first-call and warm timings separate, report memory and failures, and record
library/OS versions and build features. Match white balance, orientation,
linear output, disabled automatic brightening and optional look adjustments.
Report upstream-equivalent LibRaw Clip separately from a reconstruction mode:
Apple recovery and LibRaw reconstruction are different algorithms, with no
assumed equivalent setting. HE unsupported results are capability gaps, not
timing wins.
