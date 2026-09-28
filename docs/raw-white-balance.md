# RAW input white balance

## Controls

Open a camera RAW, then use **RAW white balance** above Profiles:

| Mode | Behavior |
| --- | --- |
| **As shot** (default) | Uses the RAW's stored camera WB through rawler, exactly as before this feature. |
| **Daylight** | Develops with daylight coefficients derived from the camera's color matrix (6504 K reference). |
| **Tungsten (2850 K)** | Starts from Daylight and adapts a 2850 K scene white to the daylight reference. |
| **Custom** | Same adaptation using Temperature, then a green-channel multiplier in linear ACES2065-1 using Tint. |

Custom defaults to **5500 K, tint 1.0**. Temperature is **1667–25000 K**;
tint is **greater than 0 through 4** (the GUI slider starts at 0.01).
Tint 1 is neutral, higher values are greener, lower values more magenta.
Temperature/tint controls are disabled and ignored in the other modes.

Edits automatically re-read and re-develop the original RAW on the preview
worker after the usual short debounce. They replace the input image before
preview resizing and film simulation; they never stack a correction onto
the previously balanced image. EXIF orientation, manual rotation, zoom, and
film/scanner settings survive a WB edit. Opening another file retains the WB
selection (manual rotation resets, as before).

Stale preview jobs cannot overwrite a newer input/WB selection. Save is
unavailable until a new preview exists; Export waits for the new RAW WB to
finish. If development fails, the error is reported and the last successful
input/WB pair is restored. Export snapshots the selected settings when it
starts; subsequent edits affect the next export.

These are **input RAW** controls. **Scanner → White correction** still adjusts
the simulated scan and is independent of RAW white balance. TIFF/PNG/JPEG
inputs already contain developed RGB: the GUI disables RAW WB for them and
the shared loader leaves their pixels unchanged. Monochrome RAWs are not
tinted.

## CLI and settings

```bash
spektrafilm process input.NEF -o out.png --film kodak_gold_200 \
  --raw-wb custom --raw-temperature 3200 --raw-tint 0.85

spektrafilm-f64 process input.NEF -o out.tiff --film kodak_gold_200 \
  --raw-wb daylight
```

`--raw-wb` accepts `as_shot`, `daylight`, `tungsten`, or `custom`.
`--raw-temperature` / `--raw-tint` require `--raw-wb custom`. Omitting all
flags preserves the settings from `--params`, or As shot if none were saved.
Flags override only the specified fields. Non-RAW input pixels ignore WB.

The GUI export's existing RuntimeParams JSON now carries:

```json
{
  "io": {
    "raw_white_balance": {
      "mode": "custom",
      "temperature": 3200.0,
      "tint": 0.85
    }
  }
}
```

Old parameter files default to As shot. Unknown modes and invalid active
custom values return errors. Both preview and export call
`spektrafilm_io::load_image_with_wb` with this setting. The loader still
returns linear sRGB, so both paths disable pipeline input gamma decoding.
Rebuild both the GUI and its `spektrafilm-f64` companion when upgrading.
Building the GUI alone leaves the export binary untouched:

```bash
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
cargo build --release -p spektrafilm-gui
```

For RAW exports the GUI also passes explicit WB flags. This prevents an older
companion from silently ignoring the new JSON field and exporting As shot;
an incompatible companion reports its path and the rebuild command instead.

## Python implementation investigated

Source pinned to upstream commit
[`3bb2c2d2801ff68b92019cf1dbcbb133d60832bc`](https://github.com/andreavolpato/spektrafilm/tree/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc):

- [`utils/raw_file_processor.py`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc/src/spektrafilm/utils/raw_file_processor.py):
  `_postprocess_params` selects camera WB for `as_shot`; the other modes use
  LibRaw's default daylight development. RAW output is linear, 16-bit ACES,
  no auto-brightening, divided by 65535 into float32. Tungsten is 2850 K;
  Custom also accepts a `(temperature, tint)` tuple in the Python API.
  `_whitepoint_xyz_from_temperature` uses the CIE D-series locus at/above
  4000 K and Kang 2002 below it, then `xy_to_XYZ` with Y=1.
  `_apply_white_balance_adaptation` uses `colour.chromatic_adaptation`,
  method `Von Kries` (default **CAT02**), from the selected scene white to
  the 6504 K reference. `_apply_tint_adjustment` multiplies the **ACES green
  channel** after adaptation. Finally the requested output color space and
  transfer function are applied. Optional lens correction precedes WB
  adaptation; it is outside this port's scope.
- [`options.py`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc/src/spektrafilm_gui/options.py#L21)
  defines the four modes;
  [`state.py`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc/src/spektrafilm_gui/state.py#L323)
  defaults to As shot, 5500 K, tint 1. WB lives in GUI-only `load_raw` state,
  rather than the film-simulation runtime parameters.
- [`widget_sections.py`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc/src/spektrafilm_gui/widget_sections.py#L362)
  wires Select file and **reprocess raw** to `load_requested(path)`. Merely
  editing a WB field does **not** re-develop an already loaded file.
  [`app.py`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc/src/spektrafilm_gui/app.py#L227)
  connects that signal to `GuiController.load_raw_image`.
- [`controller.py`](https://github.com/andreavolpato/spektrafilm/blob/3bb2c2d2801ff68b92019cf1dbcbb133d60832bc/src/spektrafilm_gui/controller.py#L186)
  collects those WB settings, calls the RAW loader, replaces the full input
  and resized preview caches, and requests auto-preview. `run_preview` uses
  the preview cache; `run_scan` uses the full input. `save_output_layer`
  writes the simulated output. Thus WB reaches both paths through the
  **developed input pixels**, not an independent WB operation during saving.

### Earlier coefficient-based implementation

The initial RAW importer in
[`74fb3f2`](https://github.com/andreavolpato/spektrafilm/blob/74fb3f2199bb216db321d365bcc1eb67b3ac1273/src/spektrafilm/utils/raw_file_processor.py)
used `raw.daylight_whitebalance` for Daylight. For Tungsten/Custom it computed
Kang 2002 xy → XYZ, multiplied `raw.rgb_xyz_matrix @ XYZ`, inverted each
response, and normalized all multipliers by the primary green multiplier.
It supplied a missing/nonpositive fourth response from green, scaled both
green multipliers by tint, and passed the four coefficients as `user_wb`.
As shot selected `use_camera_wb=True`.

Upstream replaced that scheme in
[`6a891f0` (fix: raw processor white balance)](https://github.com/andreavolpato/spektrafilm/commit/6a891f0e75e6595ae0f90d57948fbc1689a937f8).
This port follows the **corrected adaptation/tint semantics**, rather than
reintroducing the original temperature-to-camera coefficient calculation.

## Deliberate differences and decoder limits

- **Daylight coefficients:** rawler 0.8 exposes stored `wb_coeffs` and camera
  XYZ-to-camera color matrices, but not LibRaw's `daylight_whitebalance` or
  its default daylight processing. We choose the same calibration matrix
  as rawler's `RawDevelop` (D65 first; the same supported alternatives and
  Bradford matrix adaptation otherwise). For reference white W, each
  response is `r[i] = matrix[i] · W`; multipliers are `r[green] / r[i]`.
  Three-channel matrices duplicate green into the fourth WB slot; genuine
  four-channel matrices use their fourth response. Missing, malformed, or
  nonpositive matrix responses return an error for non-As-shot color RAWs.
  As shot retains rawler's existing behavior, including its missing-WB
  fallback. No fabricated daylight fallback is substituted silently.
- **Color processing:** non-As-shot temperature/tint follows upstream's
  ACES/XYZ operations, expressed as a matrix acting on rawler's linear sRGB
  output (CAT02 D65↔D60 conversion around the ACES operation). The identity
  Custom setting, 6504 K / tint 1, is exactly Daylight. Rawler's demosaic,
  camera matrices, negative/highlight handling, and floating output differ
  from LibRaw's 16-bit ACES development. Exact upstream pixel parity is
  therefore not claimed, particularly in highlights or saturated colors.
  The existing final negative clamp remains. RAW decode and developed
  pixels are f32 in both builds, even for an f64 film-pipeline export.
- **Reprocessing:** Rust automatically re-develops after edits rather than
  requiring Python's reprocess button. WB must be serialized into runtime
  I/O settings here because Rust's final export reopens the source in a
  separate CLI process.
- **Validation:** Python's widget permits temperature down to 1000 K and
  tint down to zero, with no explicit upper bounds. Rust limits temperature
  to the valid shared approximation domain (1667–25000 K) and tint to a
  finite positive value ≤4 instead of extrapolating or zeroing green.
- Existing rawler limitations (e.g. unsupported lossy DNG and some unusual
  sensor/calibration layouts) still apply. Monochrome RAW WB is a no-op.

## Verification

Focused tests cover mode selection/validation; matrix projection and green
normalization; independent Python `colour` reference whitepoints and
temperature/tint output; non-RAW/monochrome behavior; missing calibration;
and a generated, oriented DNG decoded through preview and export paths
across all four modes and a switch back to As shot. The default is compared
against the old unmodified `RawDevelop::default()` path (minus sRGB gamma).
GUI tests cover stale-worker rejection, successful input/settings updates,
JSON export serialization, and reload failure rollback. CLI tests cover
old JSON defaults, flag precedence, and invalid combinations.

```bash
cargo test -p spektrafilm-io
cargo test -p spektrafilm-io --features precision-f64
cargo test -p spektrafilm-cli --bin spektrafilm -p spektrafilm-gui --bin spektrafilm-gui
cargo check -p spektrafilm-io -p spektrafilm-cli -p spektrafilm-gui
cargo check -p spektrafilm-cli --features precision-f64

# Optional: actual camera files, supplied locally (OS path-list separator).
SPEKTRAFILM_TEST_RAWS="/path/color.CR2:/path/monochrome.DNG" \
  cargo test -p spektrafilm-io camera_raw_smoke -- --ignored --nocapture
```
