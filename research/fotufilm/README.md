# Fotufilm research packet (2026-11-01)

Gathered for porting Fotufilm's film profiles into Negs. Nothing here is integrated yet.

## Pinned sources

| Item | Revision / hash | Licence |
|---|---|---|
| github.com/DhaliwalX/fotufilm-engine (`upstream/`, not committed) | `96623ba0831a52dbddcda35727a206d4123a8b5f` (2026-10-28) | Code Apache-2.0; 47 stock JSONs CC BY-SA 4.0 (MUAStudio Inc., attribute "Fotufilm", https://fotufilm.com); print-media digitised coordinates Apache-2.0; xenon SPD BSD-3 (colour-science) |
| github.com/DhaliwalX/gold200-dataset v1.0 zip (`gold200-dataset/`, not committed) | sha256 `07b2ab7e81cb2e4397cde501cff4ec18652a67210829a24fa149d40e5b773684` | CC BY-SA 4.0 |
| PIXLS thread 60539 (Fotufilm) | 42 posts, retrieved 2026-11-01 → `forum/fotufilm-thread-p1.md` | forum content |
| PIXLS thread 58032 (datasheet campaign) | 21 posts → `forum/datasheet-campaign-thread.md` | forum content |
| PIXLS 48209 post 548 (datasheet links) | → `forum/spektrafilm-post548-datasheets.md` | forum content |

Per-stock provenance from Fotufilm's SOURCE_ASSETS.json: `stock-provenance.md`.
First numeric comparison of shared stocks: `overlap-first-look.txt` (scripts in `scripts/`).

## Key findings

* Same wavelength grid (380–780 nm, 5 nm, 81 samples) and same log10-exposure axis anchored on mid-grey.
* Profile fields map onto spektrafilm's schema: `layerSensitivity` (linear, D65-normalised) → `log_sensitivity`;
  `imageDyeDensity` → `channel_density`; `minimumDensity` → `base_density`; sampled gross curves minus `dMin`
  → net `density_curves`. Missing: `density_curves_layers` / `norm_cdfs` model (spektrafilm grain sublayers),
  `midscale_neutral_density`, neutral print filters per film×paper.
* Shared stocks: sensitivities differ by a constant offset only (≤0.01 log10 for Gold 200, Ektar, UltraMax,
  Pro 400H, Vision3, Verita) and base density matches Negs to ~0.002 D, with identical measured-domain ends.
  That is closer than two independent WebPlotDigitizer traces normally get, so the spectral data likely
  shares a lineage with spektrafilm's profiles. Characteristic curves differ (independent retrace or different
  fitting). Attribute both Volpato and Fotufilm on anything derived; ask the author.
* Fotufilm-only model features without a Negs equivalent: per-stock DIR coupler matrices/geometry, per-stock
  grain (RMS granularity, density laws, crystal populations), per-stock halation/layered transport, donor 4th
  layer (Reala 500D), push/pull condition sets, reciprocity, reflection instant prints, Digital Reference /
  Auto Levels / Lab Scan output models.

## Converter status (2026-11-01, evening)

Tools in `tools/fotufilm_port/` (uv project): `convert.py` (Fotufilm → Negs JSON), `fit_window.py`
(hanatos2025 adaptation window), `add_filters.py` + `crates/spektrafilm-cli/src/bin/solve_neutral_filters.rs`
(neutral print filters), `compare_negs.py` / `hybrids.py` (chart renders and ingredient swaps), `make_charts.py`
(spectral charts). Test profiles live only in `research/fotufilm/testdata/` (symlinked copy of `data/`, ignored).

Verified:
* Sensitivity balance: Negs profiles give the LUT neutral equal raw in all channels (K ≈ 1.8886). Converted Gold 200
  sensitivities render 0.13 ΔE00 (mean) from Negs' — same data, so the rebalance is right.
* Neutral print filters: the Rust solver reproduces shipped entries to ≤0.015 units. New stocks must get entries, or
  they fall back to C0 M50 Y50 (a large cast).
* Window fit: reconstruction of Andrea's method (corpus Otsu/NIST skin/forest/Munsell, 50/20/20/10, per-channel log
  exposure). His stored windows are near-optimal under it. With a weak prior (0.002) toward the other stocks' median,
  leave-one-out renders over all 22 Negs colour stocks are within mean ΔE00 0.08–0.59, max ≤ 2.9 of the stored window.
* Negs renders film curves from `density_curves_model`, so the norm_cdfs fit is what renders (Gold 200 residual ≤ 0.011 D).

Name-keyed behaviour new stocks don't inherit: `data/presets/couplers.toml` (per-stock DIR gammas for Velvia, Provia,
Ektachrome, Kodachrome; renaming Velvia alone moves it 4.9 ΔE) and `data/presets/grain.toml` (per-stock grain).
New stocks get the class defaults unless entries are added.

Gold 200 result: converted (fitted window) vs Negs, Endura, daylight chart: mean ΔE00 1.91, max 5.9; tungsten 1.72.
Ingredient swaps (mean/max): sens 0.13/0.5, base 0.53/2.4, dyes 1.52/2.9, curves 2.77/8.8, no window 2.82/19.5.

## Batch port status (2026-11-01, end of session)

- `tools/fotufilm_port/port_stocks.py` ported 25 new colour films into `testdata/` (duplicates of Negs films excluded; listed in `DUPLICATES` for comparison pages only). All pass the automatic checks.
- Fixed: slide (reversal) curves are now fitted in their viewed, falling form (Fotufilm `developedDensity = dMin + dMax - formed`), so the plateau is pinned to the record's dMax. The curve fit uses multiple starts, weights tones by visibility (T^(1/3)) and penalises the worst error. Kept at 3 sub-layers because Negs grain assumes 3.
- Resolved: the large k3-vs-k6 differences (Infrared Color 40, Velvia 50 22) were not the curve fit. Negs' DIR-coupler
  pre-correction (`compute_curves_before_dir`) resamples each curve on `x - couplers(x)`; on steep curves the
  shift grows faster than x, the axis folds and the result is arbitrary. `fold_check.py` measures it: every shipped
  Negs film stays positive (Velvia 100 only via its per-stock preset), while 11 ports and the Provia/Ektachrome
  references folded. `coupler_presets.py` scales the class-default gammas uniformly to keep a 0.1 margin; entries are
  in `data/presets/couplers.toml` (new IDs only; a `TEMP-K6` block holds test-only twins/references, remove before
  shipping). After the fix, k3-vs-k6 max ΔE00 is <=1.8 everywhere except one Agfachrome 50 patch (9.3 at 0 EV).
- Next: cross-engine check. The charts exist as Rec.2020 EXR (`testimages/synthetic/chart_*.exr`, `write_exr.py`) and are copied to the Mac's `~/fotufilm-ref/renders`. Fotufilm must write `.tif` (a `.exr` name still produces an 8-bit PNG); use `--grain 0 --halation 0 --local-tone 0`. Baselines: Gold 200 for negatives, Provia 100F for slides.

## Cross-engine check (2026-11-01)

- `tools/fotufilm_port/xeng.py` renders `chart_D55.exr` in Fotufilm on the Mac (`--grain 0 --halation 0 --local-tone 0 --flare 0`, 16-bit TIFF) and in Negs (Ektacolor Edge for negatives, `--scan-film` for slides, print exposure compensation off). `xeng_relative.py` gives film-vs-reference differences. Page: https://claude.ai/artifact/CQyc9oBMmkGuhV5gxyrp2s (built by `xeng_page_assets.py`).
- Bug fixed: Negs' pipeline output (and `--raw-out`) is sRGB-encoded (`output_cctf_encoding` defaults on). `compare_negs.py` had treated it as linear; it now decodes it (`lab_encoded`). **Every Negs-vs-Negs ΔE measured before this fix, including the Gold 200 difference page, used the wrong scale.** Rankings hold, magnitudes don't; recompute before quoting.
- Bug fixed: slides. Fotufilm leaves a slide's mid-grey near 0.9 above base and brightens it in its screen view (`SpectralModel.reversalBalance`). Negs' slides put mid-grey at 0.5 above base. `convert.py` now shifts reversal records along log E (one shift, from green) to `REVERSAL_MIDGREY_NET = 0.5`. Converted Provia vs Negs' own Provia went from 11.7 to 4.1 mean ΔE00.
- Results at 0 EV (mean ΔE00 vs Fotufilm): reference Gold 200 5.7, negatives 5.0–10.1; reference Provia 7.6, slides 7.2–10.7. "Check" (more than 2.5 over the reference): LomoChrome Purple, Phoenix 200, Velvia 50, Agfachrome 200, Kodachrome 200. Infrared films cannot be compared (RGB input has no IR).
- Remaining: recompute the Gold 200 page; real-photo comparisons; one Agfachrome 50 patch unstable under the k6 test; remove the `TEMP-K6` block in couplers.toml before shipping.

## Ship status (end of session 2026-11-01, uncommitted in the worktree)

Done: slide mid-grey now solved neutral at Y=0.18 (`slide_neutral.py`; converted E100 within 1.0 and Provia within 2.2 of Negs' own); photo cross-engine run (`decode_photos.py`, `xphotos.py`); page updated with photos; Gold 200 page republished as https://claude.ai/artifact/SR7V4dFWrhyRy95NkNdZn1 with corrected numbers; `ship.py` has copied the 25 profiles and 128 filter entries into data/; the TEMP-K6 block is gone from couplers.toml.
Left before commit: add data/FOTUFILM_LICENSE.txt (upstream licenses/FILM-PROFILES.txt + CC-BY-SA-4.0.txt), a CHANGELOG.txt section and a README credits line; regenerate linux/catalog.json (build spektrafilm-native, run linux/generate_catalog.py, add Fotufilm to its attribution string); run the core tests; gitignore research/fotufilm/forum and spectral-datasets before committing research/; get the user's OK on LomoChrome Purple, Superia 200 and Velvia 50.
