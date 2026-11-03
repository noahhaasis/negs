# Per-file Apple RAW compatibility verification

*Results from yosoyelchris's fork (`d2a144d`), before the port into Negs; kept as
that build's record. The port's own checks are in ticket 26.*

Verified on macOS 27.2 (26B5086k), 1 November 2026, using one local camera RAW
per model. Original photographs, filenames and paths are not included here.

The shared loader checks the file's advertised decoder versions and attempts a
full-resolution render in this order: Apple RAW 9, Apple RAW 8, then rawler.
The probe's `apple9` and `apple8` preferences force each generation separately;
`auto` exercises production routing. Each successful result below contains zero
non-finite RGB values. Dimensions include the original EXIF orientation.

| Camera | Apple 9 | Apple 8 | Auto selected | Dimensions |
| --- | --- | --- | --- | --- |
| OM SYSTEM OM-3 | Not advertised; versions 7 and 8 available | Pass | 8 | 5184 × 3888 |
| Canon EOS RP | Pass | Pass | 9 | 6240 × 4160 |
| Sony ILCE-7RM5 | Pass | Pass | 9 | 9504 × 6336 |
| Nikon D850 | Pass | Pass | 9 | 8256 × 5504 |
| Fujifilm X-T5 | Pass | Pass | 9 | 7728 × 5152 |
| Panasonic DC-S1RM2 | Not advertised; versions 7 and 8 available | Pass | 8 | 8144 × 5424 |
| Leica SL2-S (DNG) | Pass (`9.dng`) | Pass (`8.dng`) | `9.dng` | 4000 × 6000 |

These are 21 independent probes: 19 completed renders and two expected
unsupported-version responses. They establish compatibility for these files on
this OS, not all files from a model or all macOS versions. The rawler fallback
and early termination after success/fatal failure are covered by the ordered
backend unit test; the existing generated-DNG tests exercise portable decoding.
An additional generated tiny-DNG integration check confirmed that Auto selects
rawler after Apple's decoders decline, preserving orientation and exactly
matching explicit rawler pixels.

Reproduce with a local fixture:

```sh
cargo build -p spektrafilm-io --example raw_decoder_probe
target/debug/examples/raw_decoder_probe --decoder apple9 /path/photo.ORF
target/debug/examples/raw_decoder_probe --decoder apple8 /path/photo.ORF
target/debug/examples/raw_decoder_probe --decoder auto /path/photo.ORF
cargo test -p spektrafilm-io --lib raw::
```

Core Image must have normal macOS GPU/framework and cache access. Restricted
filesystem sandbox runs denied its compiler cache and caused failed renders;
those diagnostic runs were discarded and are not evidence for the table.

The raw unit suite passed six tests, with the two pre-existing opt-in camera
orientation/white-balance and lens-correction tests left ignored in that run.
The per-file probes above separately verify actual completed camera renders.
