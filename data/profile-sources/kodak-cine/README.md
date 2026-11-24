# Kodak motion-picture negative source record — 24 November 2026

**Kodak EXR 100T** (EASTMAN EXR 100T Color Negative Film 5248/7248), prepared for Negs from **KODAK Publication No. H-1-7248, March 1999** (Minor Revision 3-99; `5248.pdf`, SHA-256 `4735518c66d406e508928715f932a626b9f1e23647d45fd84f8c3cd4133291fc`). The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | PDF page | Light | Alignment chosen | Grey ramp through 2383 | Profile |
|---|---|---|---|---|---|
| EXR 100T | 3 | T | mid | 0.049 | `kodak_exr_100t` |

For comparison on the same ramp: Vision3 500T 0.060 and Vision3 250D 0.070 (`../fujifilm-cine/calibration/shipped_ramps_kodak_2383.json`); the datasheet Eterna 250 0.083 and Eterna 400 0.034 (`../fujifilm-cine/README.md`). The other variants: none 0.085, green 0.058. Neutral filters on 2383 M 67.8, Y 55.7 (shipped range M 43–94, Y 40–121); every colour paper and Eterna-CP print is in its range.

## What is here

- `recipe.json`: the build settings (`stocks`) and the extraction spec (`extraction`).
- `kodak_exr_100t/`: the curves as the public spektrafilm loader reads them, `trace/` with their PDF coordinates, `extraction.json` with each axis fit and residual, `build_report.json` and `reconstruction.png`.
- `calibration/`: the comparison with an independent trace (below).
- `donors/generic_a/` and `densitometer/status_M/`: as in `fujifilm-cine`.
- `sources/5248.pdf`: the data sheet, unmodified (SHA-256 above), so the extraction can be checked and repeated. Kodak no longer hosts it; the same file is mirrored at https://125px.com/docs/motionpicture/kodak/5248.pdf.

## How H-1-7248 is drawn

- As E-190: black 0.72 pt polylines, 0.432 pt frames and ticks; the characteristic and dye charts have tick marks only, the sensitivity chart gridlines broken by its labels. Every axis fits within the 0.2 % gate (worst 0.15 %, the sensitivity chart's wavelength axis).
- **Two exposure scales.** The characteristic chart carries log lux-seconds on its top edge (bar notation, 3̄.0 to 1.0, ticks drawn across the frame) and camera stops on its bottom edge. They disagree by 9 %: one drawn stop is 0.276 log10 H on the top scale, not 0.301. The curves are read on the top scale, the sensitometric one (green gamma about 0.59, a usual ECN-2 aim; the stop scale would give 0.54). `extract_pdf.py`'s `tick_edges` and `tick_straddle` take only the top and left ticks. The independent trace below found the same and made the same choice.
- Sensitivity at 0.40 above D-min, effective exposure 0.013 s, Status M; characteristic curves tungsten 1/50 s, ECN-2, Status M. The spectral-dye chart gives D-min and the midscale neutral only, so the dyes are `generic_a` as for every datasheet negative.

## Cross-check

An independent vector trace of the same 1999 sheet (Toni-Chan/spektrafilm, GPL-3.0, branch kodak-cine-negatives at edeadb5fd0, `src/spektrafilm_profile_builder/data/raw/kodak_motion_picture_data_sheets/negative/kodak_exr_100t_5248`; used only as a check, none of its data is here) agrees within 0.001 D on every characteristic curve (`calibration/kodak_exr_100t_vs_independent_trace_curves.json`), 0.002 D on both spectra and 0.02 on the sensitivity shapes (offset-free, worst on the steep flanks where its points are sparser).

## Recorded exceptions and choices

- **Window.** At the default prior weight (0.002) the adaptation window's c_uv came out at 389.1 nm, under the shipped range (391–456; D8 check 10). The recipe raises the weight to 0.003, the smallest that passes: [394.4, 21.5, 610.4, 59.2], corpus loss 0.0215 against 0.0211 unconstrained and 0.0276 without a window. `build.py` now reports `check10_pass`.
- **Grain.** The sheet gives "less than 5" (diffuse RMS, 48 µm, net D 1.0). The bound is taken as the figure through the Fujifilm cine rule: [5.0, 7.9, 10.0].
- **Light** `T` and **target print** 2383, as the tungsten Eternas (method §A decision 3). **Antihalation** strong (rem-jet).
- **Edge print** `5248` with no Keykode prefix (the sheet gives none), as Verita 200D.

## Rebuild

As `fujifilm-cine/README.md`, with `--recipe data/profile-sources/kodak-cine/recipe.json`, `--packet data/profile-sources/kodak-cine`, `--pdf-dir data/profile-sources/kodak-cine/sources` and `--created 2026-11-24`.

## Licence

The profile and this preparation are CC BY-SA 4.0; method and generic dye spectra from spektrafilm by Andrea Volpato; measurement data © Eastman Kodak Company. The data sheet in `sources/` is © Eastman Kodak Company and is included unmodified only so the work can be checked; it is not covered by the CC licence. No endorsement by Kodak is claimed.
