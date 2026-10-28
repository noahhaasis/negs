# Colour management

How Negs reads a photograph's colours, what it computes in, and how the
preview, Save Preview and Export get their colours. The same engine runs on
macOS, Windows and Linux; only the last step (tagging the display proxy) is
platform code.

```
file ──read──▶ linear sRGB (D65, unclamped) ──film model──▶ scan in the output space
                                                             │
                                  ┌──────────────────────────┼─────────────────────┐
                                  ▼                          ▼                     ▼
                         Export (re-render)        Save Preview (render,     Preview proxy
                                                   converted if needed)      (sRGB or Rec. 2020)
```

## 1. Input

`spektrafilm_io::load_image_colour(path, formats, white balance, interpretation)`
returns the image as **linear light with sRGB primaries and D65 white,
unclamped** (negative and >1 values survive), plus an `InputColour` that says
what was read and how (`colour/mod.rs`).

### Precedence (automatic)

| Order | Source | Notes |
|---|---|---|
| 1 | PNG `cICP` | sRGB, Display P3, BT.2020 primaries with sRGB, BT.709/2020 or linear transfer. PQ and HLG are refused (HDR, not supported). |
| 2 | Embedded ICC (PNG `iCCP`, JPEG APP2, TIFF tag 34675) | Evaluated exactly, see below. |
| 3 | PNG `sRGB` chunk | sRGB. |
| 4 | PNG `gAMA` + `cHRM` | Primaries/white from `cHRM` (sRGB primaries without it), pure gamma. |
| 5 | EXIF `ColorSpace` = 1, or interop index `R03` | sRGB, or Adobe RGB (1998) for R03 with ColorSpace = uncalibrated. |
| 6 | Untagged | 8/16-bit integer files: sRGB. Float TIFFs: linear sRGB primaries. |

RAW files are decoded by Apple's decoder (macOS) or rawler to scene-linear
sRGB primaries, now without the per-channel floors that clipped colours outside
sRGB, and rawler's highlight step no longer clips (`lib.rs`, `apple.rs`).

### ICC profiles

`colour/icc.rs` parses and evaluates profiles itself, with no colorant
approximations:

- matrix/TRC RGB (curv gamma/table, para types 0–4), gray TRC;
- LUT profiles: `mft1` (lut8), `mft2` (lut16), `mAB` (A curves, CLUT, M
  curves, matrix+offset, B curves); tag choice A2B1 → A2B0 → A2B2, as
  LittleCMS's relative colorimetric intent picks them; XYZ and Lab PCS (legacy
  lut16 0xFF00 Lab and v4 Lab); tetrahedral CLUT interpolation.

A LUT profile is never replaced by its colorant tags: if it can't be read the
file fails to open with the reason, and the Assume/Manual overrides remain.
CMYK files (JPEG Adobe APP14, TIFF photometric 5, CMYK profiles) are refused.
Non-finite samples become 0 with a warning. Third-party CMS crates were tried
and removed: moxcms (0.7) lacked the A2B1 → A2B0 fallback and resampled LUTs
to a 33³ grid, up to 11% off LittleCMS in the shadows.

PCS XYZ (D50) is adapted to D65 with Bradford, as ICC viewers do.

Reading a profile interprets the stored values correctly; it does not undo
what made them. A JPEG or TIFF from a camera or editor already carries that
software's tone curve, white balance and gamut mapping, and the scene's
spectrum is gone: Negs films the image as rendered, not the original scene.
RAW files are the input the film model is built for.

### Overrides (`io.input_interpretation`)

| `mode` | Meaning |
|---|---|
| `auto` (default) | The table above. |
| `assume` | Ignore tags; read as `color_space` (sRGB, Display P3, Adobe RGB (1998), ProPhoto RGB, ITU-R BT.2020), with its curve or `linear`. |
| `manual` | Pass the stored values through untouched; the pipeline's own `io.input_color_space` / `io.input_cctf_decoding` interpret them, exactly as Negs 0.8 read every file. |

All three apps show these for non-RAW files (Input Colour); the CLI takes
`--input-colour auto|manual|SPACE` and `--input-linear`.

**Compatibility.** Params JSON without `input_interpretation` that sets
`io.input_color_space` or `io.input_cctf_decoding` is read as `manual`
(`params::infer_input_interpretation`): CLI `--params` files, scripts, the
golden and parity harnesses keep their meaning. App sessions always carried the
full defaults, so they don't carry that intent: their JPEG/TIFF/PNG edits are
now read automatically and look different (correctly). Nothing rewrites saved
files.

## 2. Processing representation

Linear, unclamped, sRGB primaries, D65: the engine's existing representation
(the film model starts from XYZ, so the primaries only fix the matrix). The
review proposed linear Rec. 2020; extended-range linear sRGB holds the same
colours (negative components are allowed end to end), keeps every existing
matrix, GPU kernel and golden input valid, and costs nothing at f32/f64.

**Spectral boundary** (`settings.spectral_boundary`, default `guard`): the
film's spectral lookup takes chromaticity (x, y) and brightness b = X+Y+Z of
the CAT-adapted input. The unclamped input can hold values outside the xy
triangle: X+Y+Z ≤ 0, or a negative X, Y or Z (from wide camera gamuts, camera
matrices or noise). Upstream's arithmetic folded x > 1 back inside, a
different hue. `guard` gives no exposure for b ≤ 0 and moves an
out-of-triangle chromaticity toward the white onto the triangle at the same
X+Y+Z (so Y changes). It is an arithmetic safeguard at the triangle, not a
gamut map to the spectral locus: the triangle is larger than the locus, and
chromaticities between the two (non-physical but non-negative) pass through
the guard and are compressed by the input gamut compression (`xy`, inscribed
hull, on by default), as before. Pixels with no negative X, Y or Z are
unchanged (bit-identical); this includes every physical colour, but a real
colour that a camera matrix or adaptation has already pushed to a negative
component is changed. On wgpu the guard is its own pass
(`spectral_guard.wgsl`) that rewrites only out-of-triangle input in place
before the unchanged front pass, so GPU renders of everything else stay
byte-identical too; CUDA (source only, untested) guards inside its kernel.
`upstream` restores the old arithmetic for reference comparisons.

## 3. Output

**Output adaptation** (`settings.output_adaptation`, default `via_d65`): the
scan is adapted from the viewing illuminant with CAT02 to D65, then Bradford to
the output space's white, the inverse of what an ICC viewer does on display.
Outputs with a D65 white (sRGB, Display P3, Adobe RGB, BT.2020) are
bit-identical to before. ProPhoto RGB (D50) and DCI-P3 (DCI white) change by up
to ~1.4 ΔE00 and now look like the sRGB export within gamut. `direct` is
upstream's CAT02 straight to the output white.

Output gamut compression targets the output space, as before. Exports embed
matrix/TRC ICC profiles (`spektrafilm-io/src/icc.rs`).

**Finishing** (Post, crop, border, 35mm strip, resizing) works in the output
space (`spektrafilm_io::encoding::Encoding`): Post decodes with the space's own
curve; resizing works in its linear light; border and strip colours (defined in
sRGB) are converted to the output space. sRGB keeps its exact arithmetic.

## 4. Preview, Save Preview, Export

- **Preview space = the export space.** Every app develops the preview in the
  colour space Save and Export last used (macOS and Windows: the save dialog's
  choice; Linux: the same preference), selectable in Scan & Color → Output.
  The preview therefore shows the export's gamut and its gamut compression,
  not an sRGB stand-in. After a save in another space the preview follows it.
- **Export** develops the full image in the chosen space (CLI/f64 or GPU).
  **Fast** export reuses the preview's float render when it is at export size
  *and in the same space* (previously sRGB only); otherwise it develops again.
- **Save Preview** saves the float render. In the preview's space the file
  holds the render's values; in another space it is converted (relative
  colorimetric, Bradford, clipped to the target gamut), which the save dialog
  says ("Converts the sRGB preview to Display P3, clipping colours outside the
  smaller space"). A **conversion** (re-encoding finished pixels) is therefore
  distinguished from a **rendering** (developing the film into the space,
  including its gamut compression).

### Display proxy

The helper writes an 8-bit RGBA proxy (`preview::display_rgba_with_post`) and
names its space in the render response (`display_color_space`):

| Render space | Proxy | Bytes |
|---|---|---|
| sRGB | sRGB | Unchanged from 0.8. |
| any other | `Rec. 2020 (sRGB transfer)` | Clipped in the render space (as the export is), converted to BT.2020 primaries, sRGB curve, deterministic triangular dither (±1 code, hash of x, y, channel; black and white stay exact). |

BT.2020 holds Display P3, Adobe RGB and DCI-P3 whole; ProPhoto colours beyond
it are clipped, as no display shows them. The dither is there because 8-bit
steps in BT.2020 are coarser: adjacent-code ΔE00 at the 99th percentile is 0.82
for sRGB, 0.96 Display P3, 0.98 Adobe RGB, 1.18 BT.2020 and 1.54 ProPhoto
(10-bit ≈ 0.2–0.4, 16-bit ≈ 0.005). A 16-bit proxy would avoid dithering but
double the buffer on every edit; it is a possible later step.

| Platform | How the proxy is tagged |
|---|---|
| macOS | IOSurface `kIOSurfaceColorSpace` = sRGB or `itur_2020_sRGBGamma`; Core Animation converts for each display. The rendered-preview cache records the space. |
| Windows | The canvas samples the sRGB texture as linear, multiplies a Rec. 2020 proxy by the BT.2020 → sRGB matrix (unclipped), then applies the monitor's ICC matrix/TRC transform when one is assigned (`windows/src/icc.rs`; only the colorant and TRC tags are read). Without a usable monitor profile it shows sRGB. The software fallback does the same. |
| Linux | `GdkMemoryTexture` with colour state sRGB or CICP 9/13/0 (full range); GTK converts for the output. |

Grain inspection, the histogram and the press-and-hold original use the same
proxy bytes; the histogram counts proxy values (sRGB or Rec. 2020 encoded), not
export-space values.

## 5. Caches, sessions, presets

- Preview caches (all apps) key on the rendered state, which now includes the
  preview space; their records store the proxy's space (older records are sRGB).
- Sessions keep the edit; the preview space is an app preference, not part of
  the edit. Opening another photograph without a session resets the input
  override to Automatic.
- Presets and LUT export are unchanged. The 3D LUT export stays sRGB in/out.

## 6. Tests

| What | Where | Reference / tolerance |
|---|---|---|
| ICC evaluation (matrix, gray, lut8, lut16 Lab, mAB Lab/XYZ) | `spektrafilm-io/src/colour/tests.rs`, fixtures from `scripts/gen_colour_input_fixtures.py` | LittleCMS 2.19 `transicc`: 3e-5 XYZ for LUTs (16-bit fixture quantisation); 2.5e-4 for matrix profiles (IEC sRGB white Z 1.0890 vs xy D65 1.08883); colour-science ProPhoto 5e-5 (ICC D50 vs xy white). |
| Tag precedence, CMYK/PQ refusal, overrides, non-finite input | same | exact |
| Spectral guard | `spektrafilm-math` spectral tests, `spektrafilm-core/tests/bw_expansion.rs`, wgpu `front_pass_boundary_matches_cpu` | pixels with non-negative XYZ bit-identical; GPU vs CPU as existing gates |
| Output adaptation | `scanning.rs` tests | D65 outputs bit-identical |
| Finishing in the output space | `encoding.rs`, `border.rs`, `resize.rs`, `post.rs` tests | XYZ of border colours 1e-5; flat fields 1e-5 |
| Wide proxy | `preview.rs` `wide_proxies_hold_the_renders_colours` | mean of a dithered flat field within 0.1 code of the exact conversion |
| Helper render/save in Display P3 | `spektrafilm-native` `wide_preview_space_renders_and_saves_without_reconverting` | 16-bit files within ½ code of the render / its conversion |
| macOS tagging, cache, save texts | `macos/Tests` | exact |
| Windows matrix = engine matrix, WGSL constant | `windows/src/gpu.rs` test | 1e-6 |

The parity runner (`scripts/parity/check_parity.py`) gives Rust
`spectral_boundary: upstream` and `output_adaptation: direct` when comparing
with Python; `--negs-colour-defaults` measures Negs' own defaults.

## 7. Known limitations

- The preview proxy is 8-bit (dithered outside sRGB); the histogram counts
  proxy values, not export-space values.
- macOS's press-and-hold Original view is still decoded by Core Image into
  Display P3 and ignores the Input Colour override; the engine and helper
  paths honour it.
- Windows reads only the matrix/TRC part of the monitor profile: a LUT-only
  profile can't be read, so the preview is shown as sRGB as if no profile
  were assigned, and a profile with both LUT and matrix tags is applied
  through its matrix, ignoring the LUTs. This is unlike input profiles, whose
  LUTs are evaluated. Monitor profiles have not been tested on Windows
  hardware. Its inspector does not show the detected input description.
- ProPhoto colours beyond BT.2020 are clipped in the preview proxy (never in
  the export).
- The 3D LUT export stays sRGB in and out.
- HDR inputs (PQ, HLG) and CMYK files are refused rather than converted.
- The CUDA backend's guard is untested (no CUDA hardware).
- The output gamut compressor (`oklch`/`oklrab`, inherited from upstream)
  converts ProPhoto RGB and DCI-P3 to Oklab relative to their own white,
  though Oklab assumes D65. Highlights compress toward D65 rather than the
  space's white; measured on neutrals the tint is at most C\*ab 0.4 (ProPhoto
  white), and saturated colours near the gamut edge are compressed in a
  slightly skewed direction. D65 outputs (sRGB, Display P3, Adobe RGB,
  BT.2020) are exact.
