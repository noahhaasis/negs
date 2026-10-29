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
    "embedded metadata" lens correction reads exactly these four plus DNG
    (confirmed in its source, see Prior art below).
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
  | 0 | Mac check of Apple's correction; sample files; coordinate mapping | 1–2 days |
  | 1a | macOS: Apple's built-in correction behind a toggle | ~1 day |
  | 1b | DNG opcodes, shared Rust stage, UI on Windows and Linux | 1–1.5 weeks |
  | 2 | Sony, Fujifilm, OM/Olympus, Panasonic (port from darktable) | 3–5 days |
  | 3 | Optional: lensfun database for Nikon, Canon and the rest | 1–2 weeks |

  Phases 1–2 cover phones, Sony, Fuji, M43 and Leica without any lens database.

## Apple's built-in correction

`CIRAWFilter` (the decoder Negs already uses on macOS) exposes exactly two
things: `isLensCorrectionSupported` (does this file have correction data Apple
can apply) and `lensCorrectionEnabled` (on/off). The older dictionary key was
`kCIInputEnableVendorLensCorrectionKey`: it applies the correction data the
camera vendor embedded in the RAW. Negs currently sets it off together with
every other Apple processing step (`apple_raw.m:46`).

What Apple does not document:

- which cameras and lenses are covered (it changes with macOS releases; an
  [Apple forum thread](https://developer.apple.com/forums/thread/28477) asks
  for exactly this and reports seeing no effect on one file);
- which of distortion, lateral CA and vignetting it corrects; there are no
  separate switches;
- whether turning it off also suppresses a DNG's own `OpcodeList` warps
  (relevant for iPhone ProRAW, Leica, Pixel);
- whether the output extent changes when it is on.

| | Apple built-in | Shared Rust stage |
| --- | --- | --- |
| Effort on macOS | ~1 day incl. UI | Phases 1b–2 |
| Windows, Linux, rawler fallback on macOS | no | yes |
| Coverage | unknown, set by Apple | known, documented, testable |
| Separate distortion / vignetting / CA | no | yes |
| Status ("Embedded Sony profile") | only "supported" | yes |
| Toggle cost | re-decode (as white balance already does) | same |

Earlier drafts rejected Apple's route for cross-platform consistency. That
argument is weak: Apple and rawler already demosaic and calibrate differently,
so the same file does not match pixel-for-pixel across platforms today.

## Design decision: Apple on macOS now, shared Rust stage everywhere else

1. **macOS (Phase 1a):** turn on `lensCorrectionEnabled` when the user's
   setting is on and `isLensCorrectionSupported` is true. Show the toggle only
   when supported. Ship it independently of the Rust work.
2. **Windows, Linux and the rawler fallback on macOS (Phases 1b–2):** a pure-Rust
   stage in `spektrafilm-io`, used after rawler. Pure Rust also lets the web
   port in `~/Work/Negs-wasm` reuse it later.
3. **Later:** once the Rust stage exists, compare both on the same Mac files.
   If ours matches or beats Apple and covers more, switch macOS to it too
   (this requires the Apple-geometry mapping described under Coordinates).

Both use the same `io.lens_correction` setting, so sessions, undo and the UI
stay the same on every platform.

## Prior art and what we can reuse

### spektrafilm OFX ([spektrafilm.114c.de](https://spektrafilm.114c.de/))

Not reusable for this. It is a DaVinci Resolve / OpenFX plugin for footage
(log and Cineon/DPX input, no RAW decoding). Its "Lens, Gate, and Movement"
group (distortion, vignetting, chromatic aberration, bokeh, coma, film gate)
*adds* lens character as a creative effect; it does not remove a real lens's
defects using camera data. The public GPL-3.0 repository
([chaert-s/spektrafilm-ofx](https://github.com/chaert-s/spektrafilm-ofx),
last pushed 2026-07-06) contains none of the lens code: its parameters and
Metal shader have no distortion, vignetting or CA terms. Official binaries
ship under a separate non-GPL notice. Could be interesting later as an
"add lens character" feature; it is the opposite of this one.

### darktable (lens correction module)

The best reference, and licence-compatible. darktable 5.x has two methods in
`src/iop/lens.cc`:

1. **Lensfun** profiles (C library) for any camera/lens in the database.
2. **Embedded metadata**, `dt_image_correction_type_t` in
   `src/common/image.h`: `SONY`, `FUJI`, `DNG`, `OLYMPUS`, `PANASONIC`.
   - Parsing: `_check_lens_correction_data` in `src/common/exif.cc`
     (~400 lines, from line ~1376; uses exiv2 tag names such as
     `Exif.SubImage1.DistortionCorrParams`, `Exif.Fujifilm.VignettingParams`).
   - DNG opcodes: `src/common/dng_opcode.c` (~290 lines;
     `WarpRectilinear`, `FixVignetteRadial`, `GainMap`).
   - Turning coefficients into radial knots, scale-to-fill and rendering:
     `_commit_params_md`, `_process_md`, `_init_vignette_spline` and
     neighbours in `lens.cc` (~500 lines).

So the vendor knowledge is about 1,200 lines of C/C++. It is not reusable
as a library: it is tied to darktable's pixelpipe, exiv2 and GLib. It is
reusable as source to port to Rust. We would read the same tags through
rawler's TIFF/maker-note parsers instead of exiv2 (Phase 0 must check that
rawler exposes the Sony `SubImage1` and Fujifilm tags; darktable copies some
of them around itself, e.g. `exif.cc:3236` onward).

**Licence:** darktable is GPL-3.0-or-later; Negs is GPL-3.0-only. Porting
darktable code into Negs is allowed (we take it under v3). Keep darktable's
copyright notices in the ported files and credit it in the README.

### Lensfun

- **Library:** LGPL-3.0, C/C++ (`/usr/include/lensfun` is installed here,
  version 0.3.4). Linking it would add a native dependency to the Windows
  installer and the macOS bundle. Not worth it: the models (`poly3`, `poly5`,
  `ptlens` distortion; `pa` vignetting; `linear`/`poly3` TCA) are short
  formulas. Implement them in Rust.
- **Database:** XML, about 56 files (`/usr/share/lensfun/version_1`),
  CC BY-SA 3.0. darktable and RawTherapee ship it alongside GPL code. Ship it
  as separate data files with its licence and attribution, not compiled into
  the binary, and confirm this reading before release.
- The hard part is not the maths but matching camera and lens names from EXIF
  to database entries and interpolating by focal length/aperture/distance.
  That is why Phase 3 stays optional.

### Effort impact

Porting from darktable removes most of the reverse-engineering risk in
Phase 2: the formats are already decoded and tested by darktable users. That
lowers Phase 2 from ~1 week to 3–5 days. Phase 1b is unchanged because the
pipeline, settings and UI work dominate it.

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
- Apple path (only needed if macOS later moves off Apple's own correction):
  `CIRAWFilter` output is already default-cropped. Read
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

0. **On a Mac (Noah):** decode a Sony, Fujifilm, OM/Olympus or Panasonic RAW,
   an iPhone ProRAW DNG and a Nikon NEF with `lensCorrectionEnabled` off and
   on. For each, record `isLensCorrectionSupported`, the output size, and
   whether corners brighten (vignetting) and straight lines move
   (distortion). A small change to `raw_decoder_probe` plus a temporary
   environment switch in `apple_raw.m` is enough. This decides Phase 1a.
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

### Phase 1a — macOS via Apple (~1 day)

1. `io.lens_correction` setting (one `enabled` flag is enough for Apple),
   passed through `RawDecodeOptions` and the bridge.
2. `apple_raw.m`: set `lensCorrectionEnabled` from the option when supported;
   return `isLensCorrectionSupported` in `SFAppleRaw`.
3. macOS inspector toggle, shown only when supported; undo, reset, sessions.
4. Update `docs/apple-raw-decoder.md` (lens correction is no longer always off).

### Phase 1b — DNG opcodes and the shared pipeline (1–1.5 weeks)

1. `lens` module: model, `WarpRectilinear`, `FixVignetteRadial`, `GainMap`
   parsers (big-endian opcode blobs, per the DNG 1.6 spec; port from
   darktable's `dng_opcode.c`), renderer.
2. Rawler path: remove `CropDefault`, apply correction, crop, orient.
3. Settings sub-flags, native helper, CLI flag (`--lens-correction on|off`).
4. UI on Windows and Linux; on macOS the same toggle drives the Rust stage
   when the file fell back to rawler.
5. Tests (below).

Ship after Phase 1b: it is already useful for phone DNGs on Windows/Linux, and
it puts every cross-cutting piece in place.

### Phase 2 — vendor embedded data (3–5 days)

Port darktable's Sony, Fujifilm, OM/Olympus and Panasonic parsers and their
coefficient-to-knot conversion, each converting to `LensProfile`, with tests
against darktable's results on the same files (darktable 5.6.2 is installed
here, so its output can serve as the reference).

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
- **Apple's coverage is unknown** and may change between macOS releases;
  the toggle follows `isLensCorrectionSupported` per file, so nothing breaks,
  but coverage can differ from Windows/Linux.
- **Apple geometry mismatch** only matters if macOS later switches to the
  Rust stage for Apple-decoded files.
- **Crop shift** when toggling distortion on an edited photo. Acceptable
  if documented; optionally rescale the stored crop in a later pass.
- **Memory:** correction can run in place except for the resample, which needs
  one extra full-size buffer briefly (720 MB at 60 MP, f32 RGB). Check against
  `docs/handoff-memory.md` limits; tile the resample if needed.
