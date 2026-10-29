# Plan: lens corrections on macOS, Windows, and Linux

Prepared 2026-10-29 against Negs `17e8a8c`, on branch `lens-corrections`
(worktree `~/Work/Negs-lens`).

**Status:** plan only. Nothing below is implemented. Claims marked
*(verify)* come from memory of other projects' code and must be checked
against current sources or sample files during the spike.

## Short answer

- **Are corrections embedded in RAWs?** Often, but not uniformly:
  - **DNG** carries them in a standard, documented form: the `OpcodeList1/2/3`
    tags (`WarpRectilinear` for distortion and lateral CA, `FixVignetteRadial`
    and `GainMap` for vignetting). iPhone ProRAW, Pixel, Samsung, Leica,
    Ricoh and Adobe-converted DNGs commonly have them.
  - **Sony ARW, Fujifilm RAF, OM/Olympus ORF and Panasonic RW2** embed
    the camera's in-body correction parameters in private or maker-note tags.
    The formats are undocumented but reverse-engineered; darktable's
    "embedded metadata" lens correction reads them *(verify the exact set)*.
    rawler already names the Sony (`0x7031–0x7037`) and Fujifilm
    (`0xf00b`, `0xf010`) tags but does not parse them.
  - **Nikon NEF and Canon CR2/CR3** have only partial or undecoded data
    (Canon stores vignetting parameters; Nikon Z maker notes contain
    distortion/vignette blocks of unknown layout) *(verify)*. Correcting these
    needs a lens profile database (lensfun) instead.
- **Is any of it used today?** No. The Apple decoder explicitly sets
  `lensCorrectionEnabled = NO` (`crates/spektrafilm-io/src/raw/apple_raw.m:46`),
  and rawler does not apply opcodes (it only copies them into DNGs it writes,
  `rawler/src/decoders/dng.rs:138`). Its `lens.rs` identifies lenses; it has
  no correction models.
- **How difficult?** Moderate. The image maths is small (a radial gain and a
  radial resample). The work is in parsing several metadata formats correctly,
  mapping coordinates through crop/orientation, and threading one new setting
  through three native UIs, sessions and exports. Rough sizes:

  | Phase | Scope | Estimate |
  | --- | --- | --- |
  | 0 | Spike: sample files, Apple geometry, coordinate mapping | 1–2 days |
  | 1 | DNG opcodes, shared engine stage, UI on all three apps | 1–1.5 weeks |
  | 2 | Sony, Fujifilm, OM/Olympus, Panasonic embedded data | ~1 week |
  | 3 | Optional: lensfun database for Nikon, Canon and the rest | 1–2 weeks |

  Phases 1–2 cover phones, Sony, Fuji, M43 and Leica without any lens database.

## Design decision: one Rust implementation everywhere

Two routes exist:

1. **Platform-native on macOS** — set Apple's `lensCorrectionEnabled = YES`
   and implement something else for rawler on Windows/Linux.
2. **Shared Rust stage** in `spektrafilm-io`, used after every decoder,
   with Apple's correction kept off.

**Recommendation: route 2.** Apple's correction is opaque (its coverage
depends on the macOS version and is not documented), it requires a full
re-decode to toggle, and it would make the same file correct differently on
macOS than on Windows/Linux. A shared stage gives one behaviour, one status
message ("Embedded Sony profile: distortion, vignetting, CA") and one test
suite, and it is pure Rust, so the web port in `~/Work/Negs-wasm` can reuse
it later.

Fallback if the Phase 0 spike shows Apple's output cannot be mapped to the
sensor coordinates the metadata uses: enable Apple's correction on macOS for
the Apple-decoded path only, and document the difference. Decide this at the
end of Phase 0, not before.

## Where the stage goes

Corrections must act on the image the lens projected, so they run:

```
decode (linear, sensor orientation, active area)
  → lens correction            ← new
  → default crop → orientation
  → user crop / straighten → Image Size → film simulation → export
```

- Before **orientation**: all embedded models are defined in sensor
  coordinates.
- Before the **default crop**: DNG opcode coordinates are relative to the
  active area, and `OpcodeList3` is defined on the linear reference image
  before the default crop. In `load_raw_rawler`
  (`crates/spektrafilm-io/src/lib.rs:216`), drop rawler's
  `ProcessingStep::CropDefault` from `RawDevelop` and apply the default crop
  ourselves after correction.
- Before **film simulation**: halation, grain and diffusion must form on the
  corrected geometry, and vignetting is an exposure change the film should
  respond to (it is a linear gain on scene-referred values, so apply it in
  linear and keep the no-upper-clamp contract).
- Not part of **LUT export** (no photograph) and not part of **presets**:
  corrections are per-file, like orientation, not a look.

## Implementation

### Shared model (`spektrafilm-io/src/lens/`)

```rust
pub struct LensProfile {
    pub source: LensProfileSource,          // Dng, Sony, Fujifilm, Olympus, Panasonic, Lensfun
    pub distortion: Option<RadialWarp>,     // per-plane radial polynomial (+ tangential for DNG)
    pub lateral_ca: Option<[RadialWarp; 3]>,// or folded into per-plane distortion
    pub vignetting: Option<Vignette>,       // radial polynomial or gain map
    pub centre: (f64, f64),                 // normalised, in sensor coordinates
}
```

- Every vendor parser converts into this model; the renderer knows nothing
  about vendors. Vendor curves sampled at fixed radii (Sony, Fuji) are
  stored as splines.
- `fn read_lens_profile(path) -> Option<LensProfile>` reads metadata only, so
  the UI can show availability without decoding.
- `fn apply(image, profile, settings, geometry)` runs in place on the
  decoded `ImageBuf`. Use rayon like the rest of the IO crate.

### Rendering

- **Vignetting:** multiply by the gain at each pixel's radius (or bilinearly
  sampled `GainMap`). Cheap and exact.
- **Distortion + lateral CA:** inverse mapping. For each output pixel,
  compute the source position per channel and sample with a separable
  Lanczos-3 (or bicubic) in linear float. One pass handles both. Pixels that
  land outside the source: scale to fill by default (compute the largest
  scale that keeps the corners inside, as cameras do), so no black corners
  reach the film stage.
- Precision: compute positions in f64, sample in f32. The f64 exporter
  widens the corrected f32 image, as it does for decoded pixels today.
- Cost at 60 MP is one extra resample (a few hundred ms on CPU). That runs
  once per decode, not per slider move. A GPU path is not needed for v1.

### Coordinates

- rawler path: we own the crop steps, so active-area coordinates are direct.
- Apple path: `CIRAWFilter` output is already default-cropped. Read
  `DefaultCropOrigin/Size` and the active area through rawler's metadata and
  offset the profile centre and normalisation radius accordingly. **Phase 0
  must confirm** the Apple output extent equals the default crop for each
  sample (it may differ, `docs/apple-raw-decoder.md` already notes
  active-area crop differences).
- DNG `WarpRectilinear` normalises radius to the distance from the optical
  centre to the farthest corner of the image area; vendor models use
  their own normalisation (half-diagonal, half-width, etc.). Encode this per
  parser, with a unit test against a reference value per format.

### Settings and data flow

- Add `io.lens_correction` next to `io.raw_white_balance`
  (`crates/spektrafilm-core/src/params.rs:650`):
  `{ enabled: bool, distortion: bool, vignetting: bool, chromatic_aberration: bool }`.
  Pass it through `RawDecodeOptions`, so the native helper
  (`crates/spektrafilm-native/src/main.rs:262,392`), CLI and GUI follow the
  same path the white balance already uses (a change re-runs the decode).
- Do **not** add it to `presets.rs` `SECTIONS`; Python spektrafilm has no
  such field.
- Defaults: **on** for newly opened photos that have a profile. Sessions
  saved before this change load with `enabled: false`, because distortion
  correction moves pixels and would shift existing user crops. Serde default
  on the missing key handles this.
- Non-RAW inputs (JPEG/TIFF/PNG): no correction (cameras already applied it
  to JPEGs). Hide the controls.

### UI on all three apps

In the existing RAW/camera section of the inspector, next to white balance:

- **Lens Corrections** toggle.
- A status line: "Embedded DNG profile", "Embedded Sony profile", or
  "No correction data in this file" (toggle disabled).
- Three sub-toggles (Distortion, Vignetting, Chromatic aberration), each
  disabled when the profile lacks it.
- Undo, reset (Reset Adjustments restores the default for the file), and
  session restore on each platform.

Files: `macos/Sources/NegsApp/Inspector.swift` and `EditorModel.swift`;
`windows/src/ui/inspector.rs`, `windows/src/state.rs`, `windows/src/model.rs`;
`linux/src/inspector.rs`, `linux/src/parameters.rs`, `linux/src/runtime.rs`.
Follow the white-balance control on each platform as the template.
The old egui app (`crates/spektrafilm-gui`) is not maintained; skip it.

## Phases

### Phase 0 — spike (1–2 days)

1. Collect samples with known in-camera corrections: iPhone ProRAW DNG, Pixel
   DNG, a Leica or Ricoh DNG, Sony ARW, Fujifilm RAF, OM/Olympus ORF,
   Panasonic RW2, a Nikon Z NEF and a Canon CR3. raw.pixls.us and
   signatureedits are sources; record each file's origin.
2. Dump their opcodes / private tags (`exiftool -v3`, rawler's
   `devtools`), and confirm which actually carry coefficients.
3. On macOS, compare Apple output extents with rawler's active area and
   default crop for each sample.
4. Check darktable's current implementation of embedded corrections
   (`src/common/exif.cc`, `src/iop/lens.cc`) for the vendor formats and
   normalisations. darktable is GPL-3.0-or-later, so porting its parsers into
   GPL-3.0-only Negs is licence-compatible; credit it in the source.
5. Write the result into this document: confirmed formats, the Apple
   decision, and any revision of the estimates.

### Phase 1 — DNG opcodes and the full pipeline (1–1.5 weeks)

1. `lens` module: model, `WarpRectilinear`, `FixVignetteRadial`, `GainMap`
   parsers (big-endian opcode blobs, per the DNG 1.6 spec), renderer.
2. Rawler path: remove `CropDefault`, apply correction, crop, orient.
3. Apple path: apply correction to Apple's output with the Phase 0 mapping.
4. Settings, native helper, CLI flag (`--lens-correction on|off`).
5. UI on macOS, Windows, Linux.
6. Tests (below).

Ship after Phase 1: it is already useful for phone DNGs, and it puts every
cross-cutting piece in place.

### Phase 2 — vendor embedded data (~1 week)

One parser per vendor, each converting to `LensProfile`, each a day or two
with tests: Sony, Fujifilm, OM/Olympus, Panasonic. Add more only where
Phase 0 found decodable data. Nikon HE★ files decode only through Apple; their
profile parsing is independent of the decoder and still applies.

### Phase 3 — optional lensfun database (1–2 weeks)

For Nikon, Canon and anything without embedded data:

- Parse lensfun's XML database in Rust (no C dependency on Windows); bundle a
  snapshot. Its data is CC BY-SA 3.0; check the bundling and attribution
  requirements before shipping.
- Match camera and lens using rawler's `LensResolver` and EXIF lens model;
  interpolate by focal length (and aperture/distance for vignetting).
- Show "Lensfun profile: <lens name>" in the status line and let the user
  pick the lens when matching fails.
- Decide on update policy (ship a snapshot per release).

Only do this if users ask for Nikon/Canon coverage after Phase 2.

## Testing

- Unit: each parser against hand-checked coefficients from real tag dumps;
  the renderer with synthetic profiles (identity warp is bit-exact; a known
  barrel profile maps a grid of points to computed positions; vignette
  gain at the corner equals the profile value).
- Integration (`#[ignore]`, `SPEKTRAFILM_RAW_TEST_FILE` pattern already in
  `raw/mod.rs`): straight lines in a sample photo of architecture or a
  chart stay straight (measure line curvature), and the Apple and rawler
  paths agree geometrically on the same file within a pixel.
- Session compatibility: a session JSON without the key loads with
  corrections off and an unchanged crop.
- Preview equals export (existing guarantee) with corrections on.
- Native UI checks on each OS for the toggle, status line, undo and reset.
  Report any platform that could not be validated natively.

## Risks

- **Reverse-engineered formats drift** with new camera bodies. Parse
  defensively; an unrecognised layout yields "no profile", never an error
  that blocks opening the file.
- **Apple geometry mismatch** (Phase 0) could force the fallback route.
- **Crop shift** when toggling distortion on an edited photo. Acceptable
  if documented; optionally rescale the stored crop in a later pass.
- **Memory:** correction can run in place except for the resample, which needs
  one extra full-size buffer briefly (720 MB at 60 MP, f32 RGB). Check against
  `docs/handoff-memory.md` limits; tile the resample if needed.
