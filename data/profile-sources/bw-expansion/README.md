# B&W expansion source record — 18 October 2026

Two new preparations: **400TX 35 mm in D-76, 8 minutes at 20°C**, and **fifth-generation Ilford Multigrade RC Deluxe, filter 2, Multigrade developer 1+9, 1 minute at 20°C**. These are new source-based preparations, not renamed donor stocks. See [recipe.json](recipe.json) for document SHA-256 hashes, PDF coordinates, axis transforms, conditions, assumptions and authorship; [the visual review](../../../docs/bw-expansion-review.md) separates scientific evidence from appearance.

| Source | Extraction | Measured/plotted range used |
|---|---|---|
| [Kodak F-4017, October 2021](https://kodakprofessional.com/sites/default/files/wysiwyg/film/f4017_trix_320400.pdf), PDF p9, top-left characteristic panel | Vector path 22; 137 points in `trix400-d76-8min.csv`, including original PDF coordinates | log exposure −3.4500 to +0.3500 lux-seconds; total diffuse visual density 0.2880–2.0880 |
| Same document, PDF p8, spectral sensitivity | Solid path 73 labelled D=1.0 above gross fog; 323 vector samples in `trix400-sensitivity.csv` | About 300–668 nm; engine uses 380–665 nm and relative shape only |
| [HARMAN/Ilford MULTIGRADE RC PAPERS, October 2020](https://www.ilfordphoto.com/wp/wp-content/uploads/2021/01/MULTIGRADE-RC-Papers-J20.pdf), PDF p3, NEW Deluxe/Portfolio panel | Original embedded 646×417 raster, 35 manual centreline points; pixel coordinates in recipe and `ilford-mgrc-grade2.csv` | Relative log exposure 0–4.0775; reflection density 0.0524–2.0974 |

The Kodak characteristic caption specifies a large tank with agitation at one-minute intervals. It does not explicitly state dilution: interpreting unqualified D-76 as stock solution is an assumption. The sensitivity panel specifies effective exposure 0.5 seconds and diffuse visual density; it does not independently identify processing. Its solid/dashed curves are used as labelled, without deriving absolute speed from their relative placement. The Ilford p4 processing summary specifies intermittent agitation. Its curve identifies neither a surface-specific spectrum nor paper texture. These historical document revisions are not verification of a subsequently changed emulsion.

**Preparation.** `prepare.py` reads checked-in CSVs. PCHIP interpolation provides a fitting prior. Film tails continue with matched endpoint derivatives toward assumed base density 0.28 and assumed maximum total density 3.5; the source does **not** measure that shoulder. Paper tails approach the plotted plateaus. Positive normal-CDF mixtures (four film components, three paper components) give monotone bounded responses. Components are mathematical, not measured emulsion layers. The authoritative fitted model, 256 samples and sublayer fields are generated together. The exact optimization bounds and weights are in the recipe/script.

Film exposure is shifted so engine log exposure zero gives net density 0.75, matching the existing Double-X rendering anchor. Its spectral weights are scaled so reconstructed 18.4% reference gray gives unit exposure; this uses the bundled Hanatos LUT and colour-science D55/CAT16 calculation. Neither convention implements absolute ISO metering. Spectral sensitivity beyond 665 nm is `null`, explicitly an unmeasured-tail truncation. The loader subtracts the minimum sampled net film density, about 0.000786 D; the source-agreement regression includes that offset. Base density is added once.

Paper exposure is shifted to 18.4% reflectance. Published **reflection** density is used once: `R = 10^(-D_total)`, without a double-pass multiplier. Its plotted white and black imply reflectances about 0.8863 and 0.007991 before scanner endpoint correction. Flat neutral base/image spectra are assumptions, not measured paper tint.

**Why the paper is experimental but useful.** The manufacturer spectral plot has no calibrated vertical scale. The profile therefore uses unit weights over 380–550 nm, missing elsewhere, explicitly not a digitisation of that plot. For a spectrally neutral negative, exposure is `10^(-D_negative-B_negative) × sum(light × sensitivity)`. With normalized print exposure, dividing by the reference-gray exposure cancels the spectral factor. A regression substitutes blue, green and all-visible weights and checks invariance. This supports fixed-grade neutral-negative printing with print normalization enabled and no preflash. It does not support calibrated colour-negative printing, absolute paper speed, spectral filtration, changing contrast grades, tint or paper texture. The catalogue labels this limitation “experimental”.

**Agreement.** Maximum source-point errors are 0.004702 D for film and 0.016645 D for paper, within 0.01/0.03 D gates. The Kodak stroke is about 0.0156 D thick; the paper's 2–3-pixel stroke/trace ambiguity is about 0.015–0.022 D, with additional interpolation uncertainty. These are graph-fitting tolerances, not empirical accuracy claims. Source overlays and [preparation-report.json](../../../docs/bw-expansion-review/preparation-report.json) retain the evidence. Repeat generation produced identical profile bytes; software parity is a separate check.

Run from the repository root with Python 3.13 and `scripts/bw_expansion/requirements.txt` installed in a dedicated environment:

```sh
# Normal regeneration uses checked-in points and source excerpts, not PDFs.
python scripts/bw_expansion/prepare.py --output-dir target/bw-profiles
cmp target/bw-profiles/kodak_trix400_d76_8min.json data/profiles/kodak_trix400_d76_8min.json
cmp target/bw-profiles/ilford_mgrc_deluxe_grade2.json data/profiles/ilford_mgrc_deluxe_grade2.json

# Optional re-extraction: first download the two exact documents linked above.
# Hash mismatches are rejected. The manual paper trace is retained in the recipe.
python scripts/bw_expansion/extract_sources.py \
  --kodak-pdf /path/to/f4017_trix_320400.pdf \
  --ilford-pdf /path/to/MULTIGRADE-RC-Papers-J20.pdf
```

The scripts also regenerate the review plots/report. Profiles stay outside the catalogue unless `--output-dir data/profiles` is explicitly supplied. Cross-platform optimizer/library differences may affect last digits; the pinned environment and comparison reports identify the verified run.

New extraction, preparation and profiles: spektrafilm-rs contributors, Codex-assisted, 2026; new profile/preparation data under CC BY-SA 4.0. Spektrafilm schema and pipeline: Andrea Volpato and contributors, with existing repository attribution preserved. Manufacturer graph excerpts retain Eastman Kodak/HARMAN rights; this record does not grant a new licence to their documents. Excerpts are retained for source verification, not as independently licensed artwork. No manufacturer's endorsement or laboratory comparison is claimed.
