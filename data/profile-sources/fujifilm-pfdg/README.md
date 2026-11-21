# Fujifilm Professional Film Data Guide source record — 20 and 21 November 2026

Five Fujifilm still colour negatives, prepared for Negs from the **FUJIFILM Professional Film Data Guide (US), AF3-207U, © 2005 Fuji Photo Film Co., Ltd.** (`ProfessionalFilmDataGuide.pdf`, SHA-256 `64c6455651b9f13f5cd190219e4928d1fdf13ecf33859c4691d830bced8d3d36`): **Pro 160S**, **Pro 160C**, **Pro 800Z**, **Superia 1600** and, from 21 November, **True Definition 400**. The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md) (section E.1 for what differs from the Eterna brochures); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | PDF page | Alignment chosen | Grey ramp through Crystal Archive Type II | Profile |
|---|---|---|---|---|
| Pro 160S [PRO 160S] | 29 | mid | 0.117 | `fujifilm_pro_160s` |
| Pro 160C [PRO 160C] | 29 | mid | 0.127 | `fujifilm_pro_160c` |
| Pro 800Z [PRO 800Z] | 31 | green | 0.180 | `fujifilm_pro_800z` |
| Superia 1600 [CU] | 34 | mid | 0.071 | `fujifilm_superia_1600` |
| True Definition 400 [CH] | 33 | mid | 0.202 | `fujifilm_true_definition_400` |

For comparison on the same ramp (`calibration/ramps_crystal_archive/`): Pro 400H 0.071, Portra 400 0.070, Gold 200 0.080, X-TRA 400 0.115, Reala 100 0.250, C200 0.281, Superia 200 0.314, Pro 160NS 0.746.

## Not shipped, and why

| Stock | Status | Reason |
|---|---|---|
| Pro 400H, X-TRA 400 | calibration | Shipped from spektrafilm (other datasheets). X-TRA 400 from this guide matches spektrafilm's independent trace of AF3-151E within 0.006 D (characteristic curves) and builds to a profile within 0.013 D (status-M curves) of the one built from spektrafilm's raw data (`calibration/xtra_upraw_vs_pfdg.json`). Pro 400H's variants land just below the shipped filter range on Crystal Archive (yellow 33–45 against 50.0–116.5; the shipped 400H sits at 50.0), so its build stops at D8 check 8. That is a calibration finding, not a shipped film. |
| Superia Reala, Superia 200 | calibration | Negs ships the Fotufilm ports; they stay. |
| NPL 160 (tungsten) | stopped | Dye reconstruction: print-weighted residual 0.013 D, over the approved 0.01 D bound (`fujifilm_npl_160/build_report.json`). |
| Superia 100 | stopped | Dye reconstruction: print-weighted residual 0.011 D, over the 0.01 D bound. |
| X-TRA 800 | stopped | No alignment variant's Crystal Archive filters fall in the shipped range (yellow 36–48). The extraction is faithful: the guide's own dye chart prints D-min about 1.0 at 500 nm, twice X-TRA 400's. |

Each stopped stock keeps its extracted curves and build report, so it can ship after a decision without re-extraction. Noah will review the three later (20 November 2026).

## What is here

- `recipe.json`: per stock the build settings (`stocks`, with the guide's heading, product code and RMS granularity) and the extraction spec (`extraction`: page, panel frames, labelled gridlines, curve selection by line style, and the recorded axis exceptions).
- `<stock>/`: the curves as the public spektrafilm loader reads them, `trace/` with their PDF coordinates, `extraction.json` with each axis fit and residual, and for the shipped and stopped stocks `build_report.json` and `reconstruction.png`.
- `calibration/`: the calibration build reports and comparisons above, and `ramps_crystal_archive/`, the shipped films' grey ramps on the target print.
- `donors/generic_a/` and `densitometer/status_M/`: as in `fujifilm-cine`.

The PDF is not in the repository. The curves were read from its vector paths.

## Recorded exceptions (approved by Noah, 20 November 2026)

- **Axis calibration.** The guide is hand-drafted and reuses one uneven grid template across spreads. Axes are fitted to the labelled gridlines only (the hand-drawn frame edges made fits worse). Per-axis gates are 0.3/0.4/0.5 % with a `gate_reason`; the worst residual on a shipped stock is 0.41 % (1.4 nm, 0.02 log H, 0.016 D), above the 0.4 % approved for the Vivid brochures; approved for this guide up to 0.41 %. The X-TRA 400 cross-check bounds the effect. True Definition 400's worst is 0.28 %.
- **Relative sensitivity axes.** The sensitivity charts have no tick values, only a 1.0 scale bar between two gridlines. The offset is arbitrary and D5 rescales every channel, so it does not reach the profile.
- **Fourth layer.** The guide's dashed "Cyan Sensitive Layer" is not modelled (three-layer model), as for the Fotufilm Reala ports.
- **Pro 800Z** sensitivity is measured to status A density (the others status M); used as printed.
- **Edge prints** follow the shipped Pro and Superia patterns with the guide's product codes; not checked against scans (approved). True Definition 400 prints FUJI TD-400 CH by the same analogy.
- **True Definition 400** shares X-TRA 400's code [CH] but not its curves (up to 0.59 D, `calibration/true_definition_400_vs_xtra_400_curves.json`); shipped as a film of its own (approved).

## Rebuild

As `fujifilm-cine/README.md`, with `--recipe data/profile-sources/fujifilm-pfdg/recipe.json`, `--packet data/profile-sources/fujifilm-pfdg` and `--created 2026-11-20`.

## Licence

As `fujifilm-cine`: the profiles and this preparation are CC BY-SA 4.0; method and generic dye spectra from spektrafilm by Andrea Volpato; measurement data © FUJIFILM Corporation. Extracted points are redistributed for verification, not the guide. No endorsement by Fujifilm is claimed.
