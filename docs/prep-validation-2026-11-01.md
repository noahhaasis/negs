# Negs Prep validation — 1 November 2026

*Results from yosoyelchris's fork (`d2a144d`), before the port into Negs; kept as
that build's record. The port's own checks are in ticket 26.*

Validated on this Apple silicon Mac using actual Metal/Core Image rendering.

| Check | Result |
| --- | --- |
| Core library and Prep integration, including exposure placement | 102 passed |
| Precision-f64 core, GPU capacity, LUT export and Prep checks | 105 passed |
| Full image I/O library suite | 107 passed, 3 existing opt-in tests ignored |
| Portable RAW unit suite without Apple features | 6 passed |
| Added generated-DNG Auto → rawler integration | 1 passed; identical oriented pixels to explicit rawler |
| Seven real camera files, three decoder choices per file | 21 probes; all seven Auto imports succeeded |
| Native protocol, GPU preview, placement, cache resets, save, Fast and Reference exports | Passed, 40 requests; Metal backend used |
| Exact-source scope and slider assertion harnesses | 9 scope methods and 2 slider methods passed |
| Production-source editor lifecycle harness with release helper | Passed new-image reset, undo/redo, saved-session restore and pre-Prep session migration |
| Production-source camera-placement lifecycle harness | Passed automatic first render, one-shot behavior, saved exposure restoration, edit/race rejection, save availability and embedded-reference checks |
| Native Swift release app | Built with macOS 26.5 SDK |
| Packaged app signature and plist | Verified |
| Camera-preview brightness placement on three OM-family photographs | +0.75…+0.91 EV; residual below 0.004 EV; before/after renders reviewed |

Core tests exercise recovered numerical goldens, guided-filter references,
bit-exact neutral bypass, signed/HDR values, D50/D65 colour-space conversions,
exposure placement, and real GPU resident/submitted rendering. Repeated upload
keys were tested through neutral → active → changed → neutral Prep states.

The native protocol check verifies that changing Prep changes the rendered
image, repeated settings do not apply Prep twice, resetting restores the exact
neutral frame, and an invalid control value is rejected. Exposure placement
tests cover absolute/noncompounding results, stale source and RAW-setting
rejection, unusable references, and preservation of the current save state.
The final export checks retain nonzero Prep exposure in both engines and verify
byte-identical Fast export from the retained preview, plus a changed Reference
export compared with zero placement.

Live packaged-app checks opened an OM-3 ORF at 5184 × 3888, displayed the Prep
tab and RGB waveform, changed Highlights, double-clicked its slider to reset,
and selected another image to confirm fresh controls. Visual review caught and
corrected low-contrast waveform labels in light appearance and a stale native
slider thumb following a reset. A hosted slider regression executes the full
first-click/second-click sequence, waits 700 ms, and verifies both model and
native thumb stay reset; dragging afterward also remains accurate and stable.

See [exposure placement](exposure-placement.md) for the investigation and
photographic measurements behind the brightness correction. The graded view
remains active in Prep.

## Reproduce

```sh
cargo test -p spektrafilm-core --lib
cargo test -p spektrafilm-core --test prep --test gpu_capacity_fallback --test lut_export
cargo test -p spektrafilm-core --features precision-f64 --lib prep
cargo test -p spektrafilm-core --features precision-f64 --test prep
cargo test -p spektrafilm-io --lib
cargo test -p spektrafilm-io --no-default-features --lib raw::
python3 scripts/check_native_macos.py
swift test --package-path macos
```

The installed Command Line Tools do not contain XCTest. Consequently the full
Swift XCTest suite could not run here. The scope/slider test methods and editor
lifecycle scenarios, including real-RAW automatic exposure placement, were executed in temporary assertion harnesses against
production source instead; the committed XCTest cases remain for a full Xcode
environment. No claim is made that the complete Swift suite passed.

The default macOS 27 SDK also lacks its SwiftUI macro plugin in this toolchain;
the installed macOS 26.5 SDK successfully builds the unchanged application
structure. See the fork's build instructions (not part of Negs). The local package is for the
current Mac architecture, ad-hoc signed, and not notarized. Other operating
systems and the web UI were not exercised.

Camera-specific results and their per-file limits are in the
[RAW compatibility report](raw-compatibility-2026-11-01.md). Original photographs
and private fixture paths are excluded from this repository.
