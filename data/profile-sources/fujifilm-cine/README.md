# Fujifilm Eterna source record — 14 November 2026

Five Fujifilm motion-picture camera negatives, prepared for Negs from Fujifilm's own datasheets: **Eterna 250D** (8563), **Eterna Vivid 250D** (8546), **Eterna 250** (8553), **Eterna 400** (8583) and **Eterna Vivid 160** (8543). The procedure, its pass criteria and every choice are in [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | Document | Light | Alignment chosen | Grey ramp through 2383 | Profile |
|---|---|---|---|---|---|
| Eterna 250D | KB-0507ER, © 2005 | D55 | mid | 0.102 | `fujifilm_eterna_250d` |
| Eterna Vivid 250D | KB-1009E, © 2010 | D55 | green | 0.111 | `fujifilm_eterna_vivid_250d` |
| Eterna 250 | KB-0506ER, © 2005 | T | mid | 0.083 | `fujifilm_eterna_250` |
| Eterna 400 | KB-0508E, © 2005 | T | none | 0.034 | `fujifilm_eterna_400` |
| Eterna Vivid 160 | KB-0701E, © 2007 | T | green | 0.101 | `fujifilm_eterna_vivid_160` |

For comparison on the same ramp: Vision3 500T 0.060, Vision3 250D 0.070, the Eterna 500 port 0.103, Eterna Vivid 500 0.143 (`calibration/shipped_ramps_kodak_2383.json`).

## What is here

- `recipe.json`: per stock, the build settings (`stocks`) and the extraction spec (`extraction`): the document, edition, file name and SHA-256, page, panel frames, the labelled gridlines of every axis, the curve colours, and the two recorded axis exceptions.
- `<stock>/`: the curves as the public spektrafilm loader reads them (`log_sensitivity_{r,g,b}.csv`, `density_curve_{r,g,b}.csv` in log10 H, `dye_density_{min,mid}.csv`), `trace/` with the same points and their PDF coordinates, `extraction.json` with each axis fit and its residual, `build_report.json` with every gate of every alignment variant, and `reconstruction.png`, the dye-reconstruction control plot.
- `donors/generic_a/`: spektrafilm's generic dye set (Giorgianni, Madden & Kriss 2009, p. 112), from spektrafilm 9428440.
- `densitometer/status_M/`: the status-M densitometer, from the same snapshot.
- `calibration/`: the two calibration builds that do not ship (Fuji C200 from spektrafilm's public raw data, Eterna 500 from KB-501E) and their comparison with the shipped profiles. `fujifilm_eterna_500/` and `fujifilm_eterna_500_manual/` are the Eterna 500 traces (brochure, and the Motion Picture Film Manual KB-0707E as a second trace).

The Fujifilm PDFs are not in the repository. The curves were read from their vector paths, so they carry no tracing error beyond the axis calibration.

## Recorded exceptions (approved by Noah, 2026-11-14)

- **Axis calibration on the Vivid brochures.** Their charts are hand-drafted: the wavelength gridlines are unevenly spaced, so no straight axis reproduces them within the method's 0.2 % (worst 0.39 %, about 1.2 nm). The affected axes carry `gate` 0.4 % and a `gate_reason`. The Vivid 250D dye chart is the same curve as Eterna 250D's precisely drawn one, and lands within 0.4 nm of it.
- **Dye reconstruction.** Four stocks reproduce the measured midscale neutral within 0.046–0.057 D instead of 0.03 D. The error sits at the chart's blue edge, 405–410 nm, which the Eterna 500 chart does not reach. The method's fallback (flat extension past 700 nm) made it worse. Weighted by the print film's sensitivity the error is under 0.01 D per channel, as for Eterna 500. Each profile's `datasource` records its figures.
- **Alignment pick.** The least-neutral-ramp rule would have picked the unaligned variant for Eterna 250D and Vivid 160. That variant renders grey 0.16–0.27 D less red than the datasheet's own midscale neutral and needs a weaker magenta print filter than any shipped film. The rule now picks among the variants whose filters fall in the shipped range.

## Rebuild

From the repository root, with the creator environment of `tools/film_profiles` and the spektrafilm 28bf883 environment:

```sh
python -I tools/film_profiles/extract_pdf.py --recipe data/profile-sources/fujifilm-cine/recipe.json \
  --pdf-dir <folder with the Fujifilm PDFs> --out data/profile-sources/fujifilm-cine --stock <stock>
python -I tools/film_profiles/build.py --recipe data/profile-sources/fujifilm-cine/recipe.json --stock <stock> \
  --packet data/profile-sources/fujifilm-cine --python-source <spektrafilm 28bf883 tree> \
  --refine-python <its python> --corpus <reflectance corpus> --out <build dir>
python -I tools/film_profiles/finalize.py --recipe data/profile-sources/fujifilm-cine/recipe.json --stock <stock> \
  --build <build dir> --created 2026-11-14
```

Extraction refuses a PDF whose hash differs. A second build of all five on the same machine reproduced every gate figure and variant pick.

## Licence

The new profiles and this preparation are under CC BY-SA 4.0. The method and the generic dye spectra are from spektrafilm by Andrea Volpato (CC BY-SA 4.0 profiles; see `data/SPEKTRAFILM_LICENSE.txt`). The measurement data are © FUJIFILM Corporation. This record redistributes extracted curve points for verification, not Fujifilm's documents. No endorsement by Fujifilm is claimed.
