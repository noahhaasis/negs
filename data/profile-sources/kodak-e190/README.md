# Kodak E-190 source record — 24 November 2026

Four Kodak still colour negatives, the first-generation Portra NC and VC films, prepared for Negs from **KODAK PROFESSIONAL PORTRA Films, technical data E-190, November 1998** (Minor Revision 11-98; `e190.pdf`, SHA-256 `fa3e715bc7057e70a755a25b5535abf47a50ce868fe86e8b7c7732d51113cc23`): **Portra 160NC**, **160VC**, **400NC** and **400VC**; the two 400s ship (see below). The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | PDF page | Alignment chosen | Grey ramp through Portra Endura | Profile |
|---|---|---|---|---|
| Portra 160NC | 8 | mid | 0.044 | not shipped (below) |
| Portra 160VC | 9 | green | 0.053 | not shipped (below) |
| Portra 400NC | 10 | green | 0.046 | `kodak_portra_400nc` |
| Portra 400VC | 11 | green | 0.055 | `kodak_portra_400vc` |

For comparison on the same ramp (`calibration/ramps_portra_endura/`): Gold 200 0.035, Portra 400 0.048, Portra 800 0.081, Ultramax 400 0.098, Ektar 100 0.100, Portra 160 0.194.

These are the 1998 emulsions, not the shipped Portra 160 and 400 (spektrafilm, from the 2010s data sheets). The NC and VC sheets of each speed print the same spectral-sensitivity chart (the traces agree to 0.0001 log units), so each pair shares its sensitivity and adaptation window; their characteristic curves and dye spectra differ.

## Why only the 400s ship (24 November 2026)

All four pass every check, but in Negs each 160 renders almost exactly like the 400 of its line: ColorChecker through Portra Endura, neutral filters solved per film, mean ΔE00 0.4 (NC) and 0.5 (VC), worst 2.0, at −1, 0 and +1 EV. Kodak matched the speeds for printing, and E-190 prints one sensitivity chart per speed for both lines. Shipping both speeds would add two tiles that look like their neighbours; the remaining difference is grain (PGI 30/33 against 41/43), which the grain control covers. For scale, NC against VC is 1.7 and the shipped Portra 160 against Portra 400 is 1.6. The 160s' curves, traces and build reports stay here; finalizing them is all it takes to ship them.

## What is here

- `recipe.json`: per stock the build settings (`stocks`, with the sheet's heading and Print Grain Index) and the extraction spec (`extraction`: page, panel frames, tick marks and gridlines, curve selection by line width and position).
- `<stock>/`: the curves as the public spektrafilm loader reads them, `trace/` with their PDF coordinates, `extraction.json` with each axis fit and residual, `build_report.json` and `reconstruction.png`.
- `calibration/`: `ramps_portra_endura/`, the shipped Kodak negatives' grey ramps on the target print, and the comparison with an independent trace (below).
- `donors/generic_a/` and `densitometer/status_M/`: as in `fujifilm-cine`.

The data sheet is in `sources/e190.pdf`, unmodified (SHA-256 above), so the extraction can be checked and repeated. Kodak no longer hosts it; this copy was downloaded on 24 November 2026 from the 125px.com mirror (https://125px.com/docs/unsorted/kodak/e190.pdf). The curves were read from its vector paths.

## How E-190 is drawn

- Curves are black polylines (0.72 pt); grids and ticks are 0.43 pt. The characteristic and dye charts have tick marks only, no gridlines (`extract_pdf.py` panel options `ticks`, `grid_width`; curve options `polyline`, `width`). Portra 160VC's characteristic frame is drawn in one path with two label marks (`frame_any_re`).
- The characteristic charts' log-exposure labels are in bar notation (4̄.0 = −4.0 … 1.0); sensitivity is log sensitivity in ergs/cm² at 0.2 above D-min, effective exposure 1/25 s (160) or 1/50 s (400), status M.
- Every axis fits within the 0.2 % gate (worst 0.15 %, the sensitivity chart's wavelength axis); no exceptions.

## Cross-check

An independent vector trace of E-190's May 2003 edition (Toni-Chan/spektrafilm, GPL-3.0, commit 69fef0ce59; used only as a check, none of its data is here) agrees with this packet within 0.001 D on every characteristic curve, 0.001 D on the dye spectra and 0.002 on the sensitivity shapes (`calibration/independent_2003_trace_all_panels.json`, `calibration/portra_*_vs_independent_2003_trace_curves.json`). The 2003 edition reprints the 1998 charts.

## Recorded exceptions and choices

- **Grain.** E-190 gives no RMS granularity, only a Print Grain Index (4x6 from 135: 160NC 30, 160VC 33, 400NC 41, 400VC 43), which Kodak says cannot be converted. The grain presets copy the shipped Portra 400 (4.5); NC and VC differ by less than one just-noticeable step (4 PGI).
- **Dye reconstruction.** 400NC and 400VC reproduce D-mid within 0.038 and 0.034 D (worst at 405 nm, the chart's blue edge), over 0.03 D; weighted per Portra Endura channel within 0.005 and 0.006 D, under the approved 0.01 D. The 160s pass outright (0.023, 0.022 D).
- **Antihalation** strong and **target print** Portra Endura, as the shipped Portras.
- **Edge prints** KODAK PORTRA 400NC and 400VC, by analogy with the shipped Portras; not checked against scans.

## Rebuild

As `fujifilm-cine/README.md`, with `--recipe data/profile-sources/kodak-e190/recipe.json`, `--packet data/profile-sources/kodak-e190`, `--pdf-dir data/profile-sources/kodak-e190/sources` and `--created 2026-11-24`.

## Licence

The profiles and this preparation are CC BY-SA 4.0; method and generic dye spectra from spektrafilm by Andrea Volpato; measurement data © Eastman Kodak Company. The data sheet in `sources/` is © Eastman Kodak Company and is included unmodified only so the work can be checked; it is not covered by the CC licence. No endorsement by Kodak is claimed.
