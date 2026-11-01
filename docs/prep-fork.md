# Negs Prep fork

Based on `noahhaasis/negs` at `d3f06c7`. This fork adds preparation and scopes to
the native macOS app. The shared Rust engine also accepts the Prep parameters;
the web and legacy egui inspectors have not been changed.

- **Prep / Lab / Post:** Prep sits immediately before Lab. Tone has Highlights,
  Shadows, Contrast, Lift and Gain; Color has Color Boost and Saturation; Detail
  has Midtone Detail. All eight default to zero. Highlights spans −180…100,
  retaining Sawmill's extended recovery range; the others span −100…100.
- **Starting brightness:** Prep also has an independent Exposure control and
  **Match camera brightness**. Fresh RAWs can automatically place their graded
  brightness against the embedded camera preview while limiting highlight
  crowding. See [exposure placement](exposure-placement.md).
- **Double-click reset:** double-click a slider's track, thumb or title to return
  to its default. Film-dependent sliders return to that stock's default. Reset
  participates in Undo; dragging still updates continuously as one undo step.
- **New photograph:** selecting a photograph starts fresh Prep, Lab and Post
  adjustments, white balance, crop, rotation and border, then optionally places
  the new RAW's exposure. Film/paper selection,
  Image Size and output colour preferences remain. Relaunching the current saved
  session restores its edits, including Prep.
- **Scopes:** a 190-point RGB histogram replaces the old 88-point plot. The
  dropdown switches to RGB waveform and remembers the choice. Both use the
  developed preview, honour crop/straightening, exclude borders, and update off
  the main thread. The waveform follows quarter-turn rotation and uses bounded
  sampling even for very wide or tall images.
- **RAW:** test each file against Apple RAW 9, then Apple RAW 8, then rawler.
  Selection uses the filter's supported versions and completed render; no
  camera-model allowlist. See [camera results](raw-compatibility-2026-11-01.md).

## Processing and persistence

Prep ports Sawmill's recovered Resolve CameraRawAdjustFilter control math and
guided filter. Input decoding and white balance precede auto/manual exposure,
then Prep exposure and controls, then the film simulation. Prep exposure is
independent of Lab print compensation. The recovered controls operate in a Rec.709 encoded
domain and return signed scene-linear pixels to the input colour space. HDR
and negative values are retained. Neutral settings bypass every conversion.
This is a port of those controls, not a claim of complete Resolve RAW-decoder
or rendering parity.

Preview, Fast export, and Reference export share the same Prep stage. Active
Prep invalidates source-only GPU upload keys; successive edits cannot reuse
stale prepared pixels. Reference builds retain the recovered f32 control math
inside the otherwise f64 pipeline. Whole-frame spatial processing runs on CPU
before the GPU film stage. Interactive previews apply it at preview resolution.

Prep values are saved with the photograph's session and undo history. Film/Lab
presets preserve the photograph's existing Prep, just as they preserve Post.
Film-look LUT export excludes Prep because it is image-specific and spatial.

## Build this fork

```sh
bash scripts/package_prep_macos.sh
open 'dist/negs-prep/Negs Prep.app'
```

This builds for the current Mac, bundles all three engines and the data, and
ad-hoc signs the app. It uses a separate bundle identifier and has no upstream
update feed. The installed Negs app's session and preferences remain separate.
Corresponding source, including vendored rawler, is beside the app.
Set `NEGS_DIST_DIR` to package into a different directory when keeping a running
build available during validation.

The build machine's Command Line Tools macOS 27 SDK lacks the SwiftUI macro
plugin. Its installed 26.5 SDK builds the app successfully:

```sh
NEGS_SWIFT_SDK=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk bash scripts/package_prep_macos.sh
```

## Verification

The implementation includes numerical golden tests, full-frame guided-filter
references, signed/HDR and colour-space checks, default/old-params compatibility,
CPU/GPU output checks, GPU cache transitions, RAW fallback tests, scope geometry
tests and slider/session lifecycle tests. See the companion validation report
for the checks actually run on this machine:
[validation results](prep-validation-2026-11-01.md).
