# Fujifilm reversal (slide) source record — 20 November 2026

Nine Fujifilm colour reversal films extracted from Fujifilm's own datasheets. Seven are new to Negs: **Velvia 100F** (RVP100F), **Provia 400F** (RHPIII), **Provia 400X** (RXP), **64T Type II** (RTPII), **Sensia 100** (RA), **Sensia 200** (RM) and **Sensia 400** (RH). Two are calibration stocks that Negs already ships from spektrafilm and that stay as they are: **Velvia 100** (RVP100) and **Provia 100F** (RDPIII). The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md) §D11, and the tools are in [tools/film_profiles](../../../tools/film_profiles) (`build_positive.py`).

**Status: extracted and built, not shipped.** Under the public method, Velvia 100F, Provia 400X, Sensia 100 and Sensia 200 have grey ramps less neutral than the least neutral shipped spektrafilm slide. The method's own remedies don't close that gap. Whether to ship them that way or to add a neutralisation step is Noah's decision (§D11, calibration result).

| Stock | Document | Light | RMS | Grey ramp, EV −1…+2 (trust 1 / 0.1) | Fold at defaults |
|---|---|---|---|---|---|
| Velvia 100F | AF3-207U, PDF page 24 left | D55 | 8 | 0.039 / 0.032 | folds (G −0.30, B −0.33) |
| Provia 400F | AF3-207U, PDF page 25 right | D55 | 13 | 0.014 / 0.013 | ok |
| Provia 400X | AF3-0213E, PDF page 6 | D55 | 11 | 0.032 / 0.027 | folds (G −0.05, B −0.08) |
| 64T Type II | AF3-207U, PDF page 26 left | BB3100 | 10 | 0.019 / 0.016 | ok |
| Sensia 100 | AF3-207U, PDF page 26 right | D55 | 8 | 0.026 / 0.024 | folds slightly (B −0.03) |
| Sensia 200 | AF3-207U, PDF page 27 left | D55 | 13 | 0.028 / 0.025 | ok |
| Sensia 400 | AF3-207U, PDF page 27 right | D55 | 13 | 0.020 / 0.019 | ok |
| *Velvia 100 (calibration)* | AF3-207U, PDF page 23 right | D55 | 8 | 0.044 / 0.041 | B −0.06 with its shipped coupler table |
| *Provia 100F (calibration)* | AF3-207U, PDF page 24 right | D55 | 8 | 0.016 / 0.013 | G −0.05, B −0.22 with its shipped table |

The same ramp for the shipped slides (`calibration/shipped_slide_ramps.json`): Velvia 100 0.024, Kodachrome 64 0.020, Ektachrome 100 0.011, Provia 100F 0.006. The figure is the mean over EV −1, 0, +1, +2 of max(|R−G|, |B−G|)/G. The film is scanned directly under D50 on spektrafilm 28bf883. Every build is neutral at EV 0 to within 0.0001 and has its level within 1.2 % of 0.184 (trust 1). PDF pages are zero-based indices, the left or right half of a two-page spread.

## What is here

- `recipe.json`: per stock, the build settings (`stocks`) and the extraction spec (`extraction`). The spec holds the document, edition, file name and SHA-256, page, panel frames, the labelled gridlines of every axis, the curve selectors (dash pattern, order along the axis, joined parts) and every recorded axis exception.
- `<stock>/`: the curves as the public spektrafilm loader reads them for a positive: `log_sensitivity_{r,g,b}.csv`, `density_curve_{r,g,b}.csv` (status A, log10 lux·s) and `dye_density_{c,m,y}.csv`, Fujifilm's own dye spectra, peak-normalised as printed. Also `trace/` with the same points and their PDF coordinates, `extraction.json` with each axis fit and its residual, and `build_report.json` with every gate at data trustability 1 and 0.1.
- `densitometer/status_A/`: the status-A densitometer (Giorgianni, Madden & Kriss), from spektrafilm 9428440, identical to b2c7fe6's.
- `calibration/`: Velvia 100 and Provia 100F rebuilt both from the author's raw data and from this extraction, compared with the shipped profiles. It also holds the shipped slides' grey ramps and the `stretch_curves` diagnosis.

The Fujifilm PDFs are not in the repository. The curves were read from their vector paths.

## Choices and exceptions to review

- **Axis calibration.** The guide's charts are hand-drafted at about 95 × 85 pt. Several axes miss the 0.2 % gridline criterion by up to 0.48 % of span, about 1.6 nm, 0.016 log H or 0.02 D. Each such axis carries `gate` (the residual rounded up to the next 0.1 %) and a `gate_reason`. The parent session's colour negatives from the same guide use the same rule (worst 0.48 %).
- **Dye-chart zero.** The dye charts draw only the 1.0 and 0.5 gridlines. The printed 0.0 sits at the plot's bottom edge, which is used as the zero. Provia 100F and 400F miss the gridline spacing by 1.1 % (2.6 pt) and 64T Type II by 0.7 %. With the spacing-only zero, their yellow tails would go negative (−0.024 D and −0.013 D). The dye scale is renormalised later, so only the zero matters.
- **Stray gridlines.** Velvia 100 and 100F have a second 400 nm line drawn as its own path, 1.0 and 1.7 nm from the grid's own. It is ignored, with a reason recorded (`grid_ignore`).
- **Shared artwork.** Fujifilm reuses drawings. Velvia 100 and 100F share their dye chart, Provia 100F and 400F share theirs, and Provia 400F and Sensia 400 share their sensitivity chart. No two stocks share all three, and their curves differ (Provia 400F vs Sensia 400: dyes 0.09 D, curves 0.19 D). Fujifilm's text does not say any two are the same emulsion, so all are kept.
- **Sensia 400's yellow dye** is drawn only to 659 nm. Beyond that it is NaN, which passes no light at runtime (§D3).
- **64T Type II** is exposed at "Tungsten (3100 K)", so its reference illuminant is a 3100 K blackbody (`BB3100`).
- **Provia 400X** comes from a separate bulletin, measured with Fuji's FAD-30S (status A). Its RMS granularity is 11, sample density 1.0.
- `log_sensitivity_density_over_min` is 1.0 as Fujifilm prints it. The shipped spektrafilm slides say 0.2. Nothing reads the field.

## Rebuild

From the repository root, with the creator environment of `tools/film_profiles` and the spektrafilm 28bf883 environment:

```sh
python -I tools/film_profiles/extract_pdf.py --recipe data/profile-sources/fujifilm-reversal/recipe.json \
  --pdf-dir <folder with the PDFs> --out data/profile-sources/fujifilm-reversal --stock <stock>
python -I tools/film_profiles/build_positive.py --recipe data/profile-sources/fujifilm-reversal/recipe.json \
  --stock <stock> --packet data/profile-sources/fujifilm-reversal --python-source <spektrafilm 28bf883 tree> \
  --refine-python <its python> --corpus <reflectance corpus> --out <build dir>
python -I tools/film_profiles/check_slide_port.py --b2c7fe6 <spektrafilm b2c7fe6 tree> \
  --densitometer data/profile-sources/fujifilm-reversal/densitometer --stock fujifilm_velvia_100
```

## Licence

The extracted points are measurement data © FUJIFILM Corporation, redistributed for verification. Fujifilm's documents themselves are not included. No endorsement by Fujifilm is claimed. The method is spektrafilm's by Andrea Volpato.
