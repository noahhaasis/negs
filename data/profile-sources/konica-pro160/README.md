# Konica Minolta Professional 160 source record — 24 November 2026

One Konica Minolta still colour portrait negative prepared for Negs from **Konica Minolta PROFESSIONAL 160 for Color Print brochure (`AB3 0407T02M6`), April 2004** (`professional_160.pdf`, SHA-256 `76a229cf99f7b75d813afa1c907e9335c8544391de4a5da6c4890f94026da4e6`): **Konica Minolta Professional 160** (`konica_professional_160`). The procedure is [docs/adding-film-stocks.md](../../../docs/adding-film-stocks.md); the tools are in [tools/film_profiles](../../../tools/film_profiles).

| Stock | PDF page | Alignment chosen | Grey ramp through Portra Endura | Profile |
|---|---|---|---|---|
| Konica Minolta Professional 160 | 4 | none | 0.083 | `konica_professional_160` |

For comparison on the same ramp (`kodak-e190/calibration/ramps_portra_endura/`): Gold 200 0.035, Portra 400 0.048, Portra 800 0.081, Ultramax 400 0.098, Ektar 100 0.100, Portra 160 0.194.

## What is here

- `recipe.json`: build settings (`stocks`, with the sheet's heading and diffuse RMS granularity 4) and vector extraction spec (`extraction`: page, panel frames, gridlines, curve selection).
- `konica_professional_160/`: the curves as the public spektrafilm loader reads them, `trace/` with their PDF coordinates, `extraction.json` with each axis fit and residual, `build_report.json` and `reconstruction.png`.
- `donors/generic_a/` and `densitometer/status_M/`: as in `fujifilm-cine`.

The brochure is in `sources/professional_160.pdf`, unmodified (SHA-256 above), so the extraction can be checked and repeated. Konica Minolta no longer hosts it; this copy was downloaded on 24 November 2026 from the 125px.com mirror (`https://125px.com/docs/film/konica/professional_160.pdf`). The curves were read from its vector paths.

## How the Konica Minolta brochure is drawn

- All three charts on PDF page 4 (characteristic curves, spectral sensitivity, and spectral dye density D-min/D-mid) are vector curves on filled plot frames (`frame_kind: "fill"`).
- Every axis fits well inside the 0.2 % gate with no exceptions: characteristic x 0.044 %, y 0.017 %; sensitivity x 0.053 %, y relative (1.0 scale bar between the two horizontal gridlines with no tick values; `relative_reason` set, D5 rescales every channel); dye density x 0.052 %, y 0.042 %.
- Curves are plotted over 395–663 nm (sensitivity, ~1.2 decades below peak), 400–730 nm (D-min and D-mid), and −3.18 to +0.65 log10 H (characteristic, daylight 1/125 s, Process CNK-4 / C-41, status M).

## D8 verification summary

- **Curve fit:** 0.0000017 D (≤ 0.02 D). **Normalisation:** 1.000000.
- **Dye reconstruction:** `0.0227 D` outright (worst at 560 nm, well inside the 0.03 D gate; print-weighted RGB on `kodak_portra_endura`: `+0.0044 / -0.0031 / -0.0048` D).
- **Grey ramp (Portra Endura, EV −2…+6):** EV 0 = 0.0000, mean 0.0831 (`none`; `mid` 0.0878, `green` 0.0892); refit `scale [1.0615, 0.9502, 0.9761]`, `shift [0.1239, 0, 0.0306]`.
- **Neutral print filters:** C 0, M 39.81, Y 37.02 on `kodak_portra_endura`; all 12 colour papers solved with `C = 0` inside shipped ranges.
- **DIR couplers:** fold check `[0.52, 0.38, 0.48]`, uses class default couplers (`defaults.color.negative`).
- **Adaptation window:** `[445.7, 22.7, 595.5, 49.5]`, loss `0.0500 → 0.0340`.
- **Preflash (`0.05`):** black `a* = -0.14, b* = -1.14`.
- **ColorChecker vs 7 portrait peers:** mean ΔE00 1.81–3.72 (max 4.84–7.76; 3.28 vs `agfa_portrait_xps_160`), grey `C*ab` 1.63.

## Recorded exceptions and choices

- **Target print:** `kodak_portra_endura` (Negs has no Konica QA RA-4 print paper; refined against Portra Endura as the portrait RA-4 reference paper).
- **Sensitivity tails (§D3):** Konica's brochure plots sensitivity down to ~1.2 decades below peak. Controlled truncation tests on shipped `kodak_portra_160` and `fujifilm_pro_400h` (truncating tails to −1.7 and −1.2 decades) shifted ColorChecker chroma `C*ab` by only +0.35 to +0.75 (mean ΔE00 0.2–0.5), proving tail truncation is not responsible for saturation differences against Portra 160; therefore spectra remain `null` outside the published wavelength range.
- **Grain:** the 4-page 2004 brochure prints no RMS granularity; diffuse RMS granularity `4.0` on every layer (`[4.0, 4.0, 4.0]`, 48 µm aperture, D 1.0) is cross-verified from fresh-era datasheet compilations (`cacreeks.com/films.htm` and `foto-35mm.narod.ru`, matching Konica Centuria 100/200/400 = 4).
- **Antihalation:** `weak` (`use: still`), matching the shipped still portrait negatives.
- **Edge print:** `KONICA PRO 160` in `crates/spektrafilm-io/src/film35.rs`; not checked against scans.

## Rebuild

As `fujifilm-cine/README.md`, with `--recipe data/profile-sources/konica-pro160/recipe.json`, `--packet data/profile-sources/konica-pro160`, `--pdf-dir data/profile-sources/konica-pro160/sources` and `--created 2026-11-24`.

## Licence

The profile and this preparation are CC BY-SA 4.0; method and generic dye spectra from spektrafilm by Andrea Volpato; measurement data © Konica Minolta Photo Imaging, Inc. The brochure in `sources/` is © Konica Minolta Photo Imaging, Inc. and is included unmodified only so the work can be checked; it is not covered by the CC licence. No endorsement by Konica Minolta is claimed.
