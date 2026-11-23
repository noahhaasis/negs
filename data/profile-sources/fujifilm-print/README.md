# Fujifilm Eterna-CP print films: source record (23–24 November 2026)

Five Fujifilm colour print films (ECP-2B), extracted from Fujifilm's own brochures: **3513DI**, **3521XD**, **3512**, **3514DI** and **3523XD**. Kodak **2383** and **2393** are rebuilt from spektrafilm's public raw data as calibration only; the shipped Kodak profiles stay as they are. Procedure: [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md) §D12. Tools: [tools/film_profiles](../../../tools/film_profiles) (`print_spectra.py`, `build_print.py`, `check_print.py`, `finalize_print.py`).

The brochures came from a pixls.us forum thread (topic 58032). They are © FUJIFILM Corporation and are not in the repository; each stock's `extraction.json` has the document, edition and SHA-256.

**Status: the five ship (24 November 2026) behind Settings › Show Negs-built films**, which now covers papers too. This is Option A: spektrafilm's public method plus the neutral pre-alignment, with no Negs refinement step.

## Choices, briefly

- **Reference film:** Kodak Vision3 250D, under TH-KG3 through Y50 M50, as Kodak's prints (#557).
- **Eterna negatives:** they keep `target_print` kodak_2383.
- **Dyes:** the base under the brochures' dye curves is taken off, using the base 3513DI's chart implies. The four brochures sharing one hand-drafted dye chart get their undrawn cyan (< 488 nm) and magenta (> 649 nm) from 3513DI.
- **Sensitivity tails:** beyond Fujifilm's drawn ends, the shape of Kodak 2383, joined at the end and only falling.

Each profile's `metadata.datasource` lists every choice and recorded exception. Each stock's `build_report.json` has the build's checks.

## Results (runtime 28bf883, against Kodak 2383)

| Print | Curve fit | Filters on Vision3 250D (M, Y) | Camera ramp drift, mean | Scene ramp C*ab, mean / max | ColorChecker ΔE00 vs 2383, mean |
|---|---|---|---|---|---|
| 3513DI | 0.015 D | 48.4, 50.6 | 0.113 | 5.3 / 8.4 | 4.7 |
| 3521XD | 0.013 D | 48.9, 51.3 | 0.109 | 2.3 / 6.0 | 4.3 |
| 3512 | 0.029 D (recorded) | 49.3, 51.2 | 0.148 | 6.8 / 11 | 3.9 |
| 3514DI | 0.024 D (recorded) | 49.1, 51.8 | 0.151 | 6.0 / 11 | 3.6 |
| 3523XD | 0.023 D (recorded) | 49.1, 51.4 | 0.149 | 5.7 / 11 | 3.6 |
| *Kodak 2383 (shipped)* | | 51.2, 52.0 | 0.070 | 2.9 / 6.3 | – |

The worst shipped cine pair, 2383 with Portra 400, drifts 0.225 on the camera ramp; every Fuji pair stays below that. For every negative, the neutral filters are within about 3 CC of 2383's.

**What they look like:**
- 3521XD has the most neutral grey scale of the five.
- 3513DI's shadows lean green-blue.
- 3512, 3514DI and 3523XD warm their deep shadows and highlights. The brochures draw their blue curve steeper than green between D 1 and 2; the trace was checked against the drawing.
- Through Vision3, 3513DI and 3521XD are less saturated than 2383.

## Files

- `recipe.json`: the stocks, their extraction specs and gate exceptions.
- `<stock>/`: the traced CSVs, `trace/`, `extraction.json`, `build_report.json`.
- `densitometer/status_A/`: copied from `fujifilm-reversal`.
- `calibration/`:
  - `kodak_2383_*`, `kodak_2393_*`: the step-0 rebuilds against the shipped profiles;
  - `eterna_cp_runtime_checks.json`: the table above, per negative.
