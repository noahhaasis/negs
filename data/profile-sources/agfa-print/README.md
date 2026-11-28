# Agfa Print CP30 source record — 28 November 2026

One Agfa motion-picture colour release print film (ECP-2B/D, 120 µm GEVAR polyester base), extracted from Agfa's official datasheet **AGFA Motion Picture Division Technical Data: CP30 Colour print film** (`cp30_en.pdf`, dated `18/10/2003`, SHA-256 `efa02cf2648bf4906453c9a11741af3f325a2bf7142d63abab0f624cb00f60a4`): **Agfa Print CP30** (`agfa_print_cp30`). Procedure: [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md) §D12. Tools: [tools/film_profiles](../../../tools/film_profiles) (`extract_pdf.py`, `print_spectra.py`, `build_print.py`, `check_print.py`, `finalize_print.py`).

**Status: ships (28 November 2026) behind Settings › Show Negs-built films.** Option A: spektrafilm's public paper/print method plus the neutral pre-alignment and the #557 print normalisation (TH-KG3, Y50 M50, Vision3 250D), with no Negs refinement step.

## What is here

- `recipe.json`: build settings (`stocks`) and vector extraction spec (`extraction`) for page 5 of `cp30_en.pdf`.
- `sources/cp30_en.pdf`: the unmodified Agfa datasheet (SHA-256 above), included so extraction can be checked and repeated (Agfa no longer hosts it).
- `agfa_print_cp30/`: raw CSVs as spektrafilm's loader reads them, `trace/` with PDF coordinates, `extraction.json`, `build_report.json`, and `info.txt`.
- `densitometer/status_A/`: copied from `fujifilm-print`.
- `calibration/cp30_runtime_checks.json`: runtime checks on spektrafilm `28bf883` against Kodak 2383 and Eterna-CP 3514DI/3521XD across five negatives.

## How CP30's datasheet is drawn

- All three required charts on PDF page 5 (Sensitometric curves, Spectral sensitivity curves, Spectral Dye Density Curves) are pure vector polylines (0 raster images) on clean rectangular frames with uniform interior grids.
- Every axis fits well inside the 0.2 % gate with zero exceptions: characteristic x 0.133 %, y 0.115 %; spectral dye density x 0.042 %, y 0.086 %; sensitivity x 0.087 %, y 0.174 %.
- **Characteristic curves:** Relative Log Exposure 0.0–2.5 vs Density 0–450 (Agfa's density × 100 convention, mapped directly to 0.00–4.50 D; status A per p. 3; all three channels overlaid with no plot shift).
- **Spectral dye densities:** plotted for a neutral subject at **D = 3.00** over the full **380–780 nm** range (cyan and magenta paths are drawn twice identically in the PDF; one copy is selected). Across 380–780 nm, `C + M + Y = Black` within 0.012 D — pure net dye densities without base, so no base subtraction or donor fill is needed. `build_print.py` divides them by `dye_chart_density: 3.0` to enter status-A unmixing at the standard D = 1.00 scale (avoiding broadband status-A Beer-Lambert bias and the 0.25 CDF lower bound).
- **Spectral sensitivity:** drawn continuously over 376–770 nm (R), 376–600 nm (G), and 376–548 nm (B), reaching 2.2–2.6 decades below peak. Kodak 2383 tail shape only fills the far-out-of-band ends (0.06 % R, 0.70 % G, 0.15 % B of raw exposure).

## Choices and recorded exceptions

- **Reference film:** Kodak Vision3 250D under TH-KG3 through Y50 M50, as Kodak 2383 and Fujifilm Eterna-CP (#557).
- **Curve fit (recorded exception, gate 0.035 D):** Agfa's 37–41-point piecewise-linear sensitometric polylines have a small kink around Relative Log Exposure 1.45–1.55 (D ≈ 2.0–2.6); three `norm_cdfs` miss blue by up to 0.0326 D and green by 0.0229 D there (red 0.0168 D). Status-A remix matches the sheet within 0.027–0.034 D up to D 2.0. (Above D 2, the diagnostic status-A remix reports 0.34–0.49 D because CP30's dyes remain finite to 780 nm where the status-A CSV clamps a ~0.0005 floor; in visual CIE rendering CP30 reaches L* 3.27 blacks matching Kodak 2383's 3.05.)
- **Far-red sensitivity:** CP30's red sensitivity genuinely peaks at 701 nm (35 % of red exposure above 700 nm, matching Agfa's p. 3 warning on far-red absorption in optical printers). On Vision3 negatives (base density to 780 nm) filters match 2383 within 0.5–2.2 CC; on negatives whose published base stops at 700 nm (`null` = opaque above 700 nm), neutral printing requires higher M/Y compensation (all 49 solved films remain inside 2383's range: M 43.69–94.74, Y 48.80–120.43, C = 0).
- **Edge print:** `CP30 AGFA` (`EdgePrint::Print("CP30 AGFA")` in `crates/spektrafilm-io/src/film35.rs`).

## Results (runtime 28bf883, against Kodak 2383)

| Negative | Filters (M, Y) | Camera ramp drift, mean | Scene ramp C*ab, mean / max | Scene L* (-3..+3 EV) | ColorChecker ΔE00 vs 2383, mean / max |
|---|---|---|---|---|---|
| Vision3 250D | 50.6, 50.5 | 0.069 | 1.66 / 3.22 | 3.27..84.49 | 2.87 / 5.05 |
| Vision3 500T | 45.4, 52.5 | 0.065 | 1.76 / 3.28 | — | 2.71 / 5.01 |
| Agfa Portrait XPS 160 | 61.5, 67.5 | 0.066 | 1.40 / 2.70 | — | 2.77 / 5.05 |
| Eterna 250D | 61.2, 69.4 | 0.247 | 2.96 / 5.45 | — | 2.95 / 4.96 |
| Portra 400 | 73.0, 71.8 | 0.440 | 2.72 / 4.86 | — | 2.99 / 4.82 |
| *Kodak 2383 (Vision3 250D)* | *51.2, 52.0* | *0.070* | *2.94 / 6.29* | *3.05..88.26* | *–* |

**What it looks like:** deep neutral blacks matching Kodak 2383 (L* 3.27 vs 3.05, deeper than Eterna-CP 3514DI's 4.24 and 3521XD's 4.88), the most neutral grey scale of any cine print film in Negs (scene ramp C*ab mean 1.66, max 3.22 on Vision3 250D), a gentler highlight roll-off (L* 84.49 vs 88.26 at +3 EV), and moderate saturation (ColorChecker mean C*ab 29.94 vs 2383's 34.94 and 3521XD's 27.37).

## Rebuild

```sh
python -I tools/film_profiles/extract_pdf.py --recipe data/profile-sources/agfa-print/recipe.json \
  --pdf-dir data/profile-sources/agfa-print/sources --out data/profile-sources/agfa-print
python -I tools/film_profiles/print_spectra.py --python-source <spektrafilm 28bf883 tree> \
  --negs-data data --film kodak_vision3_250d --out spectra.npz
python -I tools/film_profiles/build_print.py --recipe data/profile-sources/agfa-print/recipe.json \
  --stock agfa_print_cp30 --packet data/profile-sources/agfa-print --spectra spectra.npz --out build
python -I tools/film_profiles/finalize_print.py --recipe data/profile-sources/agfa-print/recipe.json \
  --stock agfa_print_cp30 --build build --created 2026-11-28
```

## Licence

The profile and this preparation are CC BY-SA 4.0; method from spektrafilm by Andrea Volpato; measurement data © Agfa-Gevaert N.V. / AG. The data sheet in `sources/` is © Agfa-Gevaert N.V. / AG and is included unmodified only so the work can be checked; it is not covered by the CC licence. No endorsement by Agfa is claimed.
