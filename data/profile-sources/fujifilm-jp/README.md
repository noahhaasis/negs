# Fujifilm Japan data sheets source record — 24 November 2026

One Fujifilm still colour negative, prepared for Negs from Fujifilm Japan's data sheets (FUJIFILM DATA SHEET, Japanese; Fujifilm's index of discontinued-product sheets, fujifilm.com/jp/ja/consumer/support/films/negative-and-reversal/datasheet): **Reala Ace**. Two more sheets were extracted and kept: Superia Venus 800 (stopped) and Natura 1600 (the second trace). The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md) (section E.1 applies: the sheets share the Data Guide's drafting); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | Sheet | PDF page | Alignment chosen | Grey ramp through Crystal Archive Type II | Profile |
|---|---|---|---|---|---|
| Reala Ace | `datasheet_realaace_01.pdf`, 013Ai158A (96.3), SHA-256 `04bb3116…` (full hash in `recipe.json`) | 4 | green | 0.076 | `fujifilm_reala_ace` |

For comparison on the same ramp (`../fujifilm-pfdg/calibration/ramps_crystal_archive/`): Pro 400H 0.071, Portra 400 0.070, Gold 200 0.080, X-TRA 400 0.115, Reala 100 0.250, C200 0.281.

## Not shipped, and why

| Stock | Status | Reason |
|---|---|---|
| Natura 1600 | second trace | The same emulsion as the shipped `fujifilm_superia_1600`: its curves match the Data Guide's Superia 1600 within 0.009 D, its spectra within 0.021 D, its sensitivity shapes within 0.034 (`calibration/`). It is the physical check that bounds this packet's axis exceptions. |
| Superia Venus 800 | stopped | Dye reconstruction: print-weighted residual on Crystal Archive R/G/B +0.004/−0.006/+0.016 D, over the approved 0.01 D bound (worst at 415 nm, the chart's blue edge; `fujifilm_superia_venus_800/build_report.json`). Like NPL 160 and Superia 100 in the Data Guide, kept for a decision. |

Venus 800 is often taken for the Japanese name of X-TRA 800, but this sheet does not match the Data Guide's X-TRA 800 (curves up to 0.28 D, D-min at 500 nm 0.70 against 0.99, D-mid 1.51 against 2.08); it is closer to the guide's X-TRA 400 (D-min 0.68). The guide's X-TRA 800, stopped at D8 check 8 for its doubled D-min, looks like the odd one. It is kept under its own name, with no claim that it is X-TRA 800.

Reala Ace is not the Superia Reala that Negs ships (`fujifilm_reala_100`, a Fotufilm port of AF3-967E): against the guide's Superia Reala trace its curves differ by up to 0.29 D and its green D-min by +0.21 D. It ships as a film of its own.

## What is here

- `recipe.json`: per stock the build settings and the extraction spec (page, frames, gridlines, curves by colour on Venus 800 and by position on the black-line sheets, the recorded axis exceptions).
- `<stock>/`: the curves as the public spektrafilm loader reads them, `trace/` with their PDF coordinates, `extraction.json` with each axis fit and residual, and for Reala Ace and Venus 800 `build_report.json` and `reconstruction.png`.
- `calibration/`: the comparisons with the Data Guide traces (`vs_pfdg_all_panels.json`, and the curve comparisons from `check_second_trace.py`).
- `donors/generic_a/` and `densitometer/status_M/`: as in `fujifilm-cine`.

The PDFs are not in the repository. The curves were read from their vector paths.

## Recorded exceptions

- **Axis calibration** (to confirm). The sheets reuse the Data Guide's hand-drafted grid: Venus 800 and Natura 1600 share the dye chart's residuals to 0.001 %. Axes over 0.2 % get a 0.3 % gate with a `gate_reason`: Reala Ace's characteristic y (0.23 %), and on the unshipped sheets up to 0.25 %. That is under the 0.41 % approved for the guide; Natura 1600 against the guide's Superia 1600 (above) bounds the effect.
- **Relative sensitivity axes.** As in the guide: 比感度（対数）, a 1.0 scale bar and no tick values. D5 removes the offset.
- **Fourth layer.** The dashed fourth-layer curve is not modelled (three-layer model), as for the guide's stocks.
- **Dye reconstruction.** Reala Ace reproduces D-mid within 0.043 D (worst at 585 nm), over 0.03 D; weighted per Crystal Archive channel it is +0.002/+0.004/+0.009 D, within the approved 0.01 D.
- **Edge print** FUJI REALA ACE by analogy with the shipped Reala; not checked against scans.

## Rebuild

As `fujifilm-cine/README.md`, with `--recipe data/profile-sources/fujifilm-jp/recipe.json`, `--packet data/profile-sources/fujifilm-jp`, the sheets' folder as `--pdf-dir` and `--created 2026-11-24`.

## Licence

As `fujifilm-cine`: the profiles and this preparation are CC BY-SA 4.0; method and generic dye spectra from spektrafilm by Andrea Volpato; measurement data © FUJIFILM Corporation. Extracted points are redistributed for verification, not the sheets. No endorsement by Fujifilm is claimed.
