# Agfa Technical Data PF source record — 24 November 2026

One Agfa still colour portrait negative prepared for Negs from **Agfa Technical Data PF, Range of Professional Films**, 1st edition, 09/1998 (`agfa_films.pdf`, SHA-256 `982d920110a598a910e7f31f7fa2d7446f4238d84408ae83bfa32e92d1188d79`): **Agfa Portrait XPS 160** (`agfa_portrait_xps_160`). The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | PDF page | Alignment chosen | Grey ramp through Portra Endura | Profile |
|---|---|---|---|---|
| Agfa Portrait XPS 160 | 8 | mid | 0.061 | `agfa_portrait_xps_160` |

For comparison on the same ramp (`kodak-e190/calibration/ramps_portra_endura/`): Gold 200 0.035, Portra 400 0.048, Portra 800 0.081, Ultramax 400 0.098, Ektar 100 0.100, Portra 160 0.194.

## What is here

- `recipe.json`: build settings (`stocks`, with the sheet's heading and diffuse RMS granularity 3.5) and vector extraction spec (`extraction`: page, panel frames, gridlines, curve selection).
- `agfa_portrait_xps_160/`: the curves as the public spektrafilm loader reads them, `trace/` with their PDF coordinates, `extraction.json` with each axis fit and residual, `build_report.json` and `reconstruction.png`.
- `donors/generic_a/` and `densitometer/status_M/`: as in `fujifilm-cine`.

The data sheet is in `sources/agfa_films.pdf`, unmodified (SHA-256 above), so the extraction can be checked and repeated. Agfa no longer hosts it; this copy was downloaded on 24 November 2026 from the 125px.com mirror (`https://125px.com/docs/film/agfa/agfa_films.pdf`). The curves were read from its vector paths.

## How Agfa Technical Data PF is drawn

- All three charts on PDF page 8 (spectral sensitivity, spectral dye density D-min/D-mid, and characteristic curves) are pure vector strokes in PageMaker/PDF coordinates with uniform interior grids.
- Every axis fits well inside the 0.2 % gate with no exceptions: sensitivity x 0.011 %, y 0.013 %; dye density x 0.012 %, y 0.022 %; characteristic x 0.063 %, y 0.016 %.
- On the characteristic panel, the y-axis fits the 7 interior gridlines (0.5–3.5 D) and top frame edge (4.0 D); the bottom box border sits ~1.03 pt below the uniform 0.0 D gridline spacing across all six charts in the brochure (the printed `0` label aligns with the gridline spacing, not the box border).
- Curves are plotted over 390–690 nm (sensitivity, ~1.9 decades below peak), 390–720 nm (D-min and D-mid), and −3.46 to +1.48 log10 H (characteristic, daylight 5400 K, Process AP 70 / C-41, status M).

## D8 verification summary

- **Curve fit:** 0.0038 D (≤ 0.02 D). **Normalisation:** 1.000000.
- **Dye reconstruction:** 0.0344 D worst at 405 nm (chart's blue edge), over 0.03 D; print-weighted RGB on `kodak_portra_endura`: `+0.0005 / +0.0058 / +0.0000` D, within the approved 0.01 D bound (same exception as Kodak Portra 400NC/400VC).
- **Grey ramp (Portra Endura, EV −2…+6):** EV 0 = 0.0000, mean 0.0607 (`mid`; `none` 0.0685, `green` 0.0738); small refit (`scale [1.0154, 0.9706, 1.0129]`, `shift [0.0656, 0, 0.0640]`).
- **Neutral print filters:** C 0, M 52.24, Y 44.61 on `kodak_portra_endura`; all 12 colour papers solved with `C = 0` inside shipped ranges.
- **DIR couplers:** fold check `[0.47, 0.32, 0.43]`, uses class default couplers (`defaults.color.negative`).
- **Adaptation window:** `[421.7, 26.5, 593.1, 36.0]`, loss `0.0591 → 0.0381`.
- **Preflash (`0.05`):** black `a* = -0.43, b* = -0.26`.
- **ColorChecker vs 7 portrait peers:** mean ΔE00 2.08–4.62 (max 5.28–12.25), grey `C*ab` 1.38, +4 EV grey neutral (`a* = -0.2, b* = -0.9`).

## Recorded exceptions and choices

- **Target print:** `kodak_portra_endura` (Negs has no Agfacolor RA-4 print paper; refined against Portra Endura as the portrait RA-4 reference paper).
- **Sensitivity tails (§D3):** left empty (`null`) outside the published wavelength range. Controlled truncation tests on shipped `kodak_portra_160` and `fujifilm_pro_400h` (truncating tails to −1.7 and −1.2 decades) shifted ColorChecker chroma `C*ab` by only +0.35 to +0.75 (mean ΔE00 0.2–0.5), confirming that synthetic tail extension is neither necessary nor warranted.
- **Grain:** diffuse RMS granularity `3.5` on every layer (`[3.5, 3.5, 3.5]`), directly from Technical Data PF p. 8 (48 µm aperture, net D 1.0).
- **Antihalation:** `weak` (`use: still`), matching the shipped still portrait negatives.
- **Edge print:** `AGFA` / `PORTRAIT XPS 160` in `crates/spektrafilm-io/src/film35.rs`; not checked against scans.

## Rebuild

As `fujifilm-cine/README.md`, with `--recipe data/profile-sources/agfa-pf/recipe.json`, `--packet data/profile-sources/agfa-pf`, `--pdf-dir data/profile-sources/agfa-pf/sources` and `--created 2026-11-24`.

## Licence

The profile and this preparation are CC BY-SA 4.0; method and generic dye spectra from spektrafilm by Andrea Volpato; measurement data © Agfa-Gevaert AG. The data sheet in `sources/` is © Agfa-Gevaert AG and is included unmodified only so the work can be checked; it is not covered by the CC licence. No endorsement by Agfa is claimed.
