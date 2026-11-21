# Adding colour film stocks from manufacturer datasheets

Method document, 2026-11-14. It is written for an agent that will do the work without prior context, and for Noah, who decides. It follows Andrea Volpato's (arctic's) spektrafilm method as far as it can be known. The research behind it is in `~/.negs/research/film-profiling/notes/`:

- `upstream-python-method.md`: the pipeline reconstructed from the Python source;
- `negs-stock-integration.md`: how stocks live in Negs;
- `fuji-data-inventory.md`: what the Fuji folder holds;
- `forum-48209-part1…4.md` and `forum-57512-58032.md`: what the author said.

Read them for detail; this document does not repeat them. The earlier B&W-oriented survey is `docs/adding-film-stocks-research.md`.

---

## A. For Noah: summary and decisions

**What adding a film takes.** The datasheet curves must be copied out of the PDF as numbers: spectral sensitivity, characteristic curves, and the spectral densities of base and mid-grey. They are then processed the way spektrafilm processes them. That means guessing the dye colours where the sheet doesn't print them, separating the three layers, tuning the curves until a grey ramp prints neutral, and scaling the result to Negs's conventions. After that the stock needs print-filter values for every paper, a grain value and an edge print, and has to pass checks. Negs has no tool for the colour part yet, so one has to be built first (§F). After that, each stock is mostly routine work.

**Is the Fuji data enough?**

| Group | Stocks in the folder | Verdict |
|---|---|---|
| Camera negatives | Eterna 250, 250D, 400, 500, Vivid 160, Vivid 250D, Vivid 500, F-64D, Reala 500D | **Enough.** Every curve the method needs is present except per-dye colour spectra. No Fuji or Kodak still negative upstream has those either; spektrafilm fills them with a generic set. Spectra stop at 400–700 nm, the exposure axis is in relative stops, and sensitivity is defined at 0.4 above base. All three are handled without inventing data (§D3). Reala 500D's fourth layer cannot be modelled and is dropped. |
| Print films | Eterna-CP 3512, 3513DI, 3514DI, 3521XD, 3523XD | **Probably enough, but defer.** The data is better than for Crystal Archive (which has no curves at all): it includes per-dye spectra. Missing are the base spectrum (the method synthesises one) and absolute exposure labels. The print path, though, is the least documented part of the author's method, and each new print needs filter values for every negative. |
| Recording film | Eterna-RDI | **Do not attempt.** It is exposed by a laser recorder, its axis is in digital code values and it has no grain figure. It isn't a camera film. |
| Intermediate and old print | Eterna-CI, F-CP (manual only) | **Do not attempt.** Only characteristic curves are given, and Negs has no intermediate stage. |

**New versus already in Negs.** Five negatives are new: Eterna 250, Eterna 250D, Eterna 400, Eterna Vivid 160 and Eterna Vivid 250D. Eterna 250D exists only on the local Dyeworks branch, whose data may not be copied. Four are already shipped as Fotufilm ports: Eterna 500, Eterna Vivid 500, F-64D and Reala 500D. Those four were made by a different method (no layer separation, no masking couplers), so they make a good cross-check of the extraction but are not the same thing.

**What the result will and won't be.**

- **Same family as upstream's Fuji stocks.** The new stocks will be built like C200, Pro 400H and X-Tra 400: datasheet curves, generic dyes, and a grey-ramp correction that makes neutrals print neutral.
- **Colours away from grey are approximate.** Saturated colours are approximate because the dyes are generic and the DIR-coupler strength is a shared default. The author says the latter is "eyeballed" (#500) and equal for all negatives (#734).
- **Fuji data needs correction.** The author warns that Fuji datasheets are "not self-consistent" and need more correction than Kodak's (48209 #1; README @ 9428440). The result will be usable, but less trustworthy than a Kodak stock.
- **Tungsten is the weaker path upstream.** The author gave tungsten "a back seat" in validation (#747) and had not checked it with the newer upsampler (57512 #50). Four of the five new stocks are tungsten.
- **The author's current steps cannot be copied.** His creator has been private since March 2026. The last public version runs. On C200, its output differs from the profile Negs ships by up to about 0.26 density in the red mid-tones once rescaled (§B.2). A new stock will therefore sit close to the shipped ones, not be identical in method.

**Risks.**

1. The tool is real work, roughly 1,500–2,000 lines (§F).
2. Mis-calibrated axes. The author once lost a week to a 50 nm axis stretch (#549). The checks in §D8 are there to catch this.
3. Tungsten stocks may need hand adjustment.
4. A careless change to a shared file would alter existing exports. The rules in §D7 prevent that.

**Decisions that are yours** (the recommendation comes first in each):

1. **Build the tool and port the method** (§F), rather than running the old public creator and rescaling its output. *Recommend: yes.* The old creator's final correction step is tuned through a 2026-02 renderer that Negs no longer matches (§B.2).
2. **Scope.** *Recommend:* first re-derive C200 and Eterna 500 as calibration cases that don't ship. Then ship the five new negatives. Defer the print films. Skip RDI, CI and F-CP.
3. **Tungsten reference light for the new stocks.** *Recommend: `T`*, the light upstream uses for Vision3 200T/500T. It selects the tungsten coupler defaults and is upstream-tested. The alternative is `BB3200`, which matches the brochures' 3200 K and the existing Eterna 500 port, but gets the daylight coupler defaults (§B.2, check 3).
4. **Existing Eterna 500 and Eterna Vivid 500.** Both use `BB3200`, so they get daylight coupler gammas. They also sit about 0.2 EV under the exposure convention (§B.2). *Recommend: leave them as they are for now.* Fixing either changes existing exports. If you want them fixed, do it as a separate, announced change.
5. **Replace the four Fotufilm Fuji cine ports with re-derived versions?** *Recommend: no, not now.* Keep the re-derivations as calibration only, and revisit after comparing them on your photos.
6. **Licence and credit.** *Recommend:* CC BY-SA 4.0 for the new profiles. Credit Volpato for the method and the generic dyes, and Fujifilm for the data. Note the work in `data/CHANGELOG.txt`. Ask the author to confirm that the generic dye set, which came from the GPLv3-era repository, may go into CC BY-SA profiles (§G). Don't commit the Fuji PDFs, only the extracted points and their hashes.
7. **The author.** *Recommend:* post the questions in §G, but don't wait for answers. He hasn't posted since June 2026.

---

## B. Provenance and confidence

Every step in §D carries one of these marks:

- **[V]** The author's method as verified in public code (commit and path given) or by a run done for this document.
- **[F]** Known only from the author's forum statements (post number given).
- **[U]** Unknown: private steps since 2026-03-17. They are named, but their content can't be reproduced.
- **[N]** Negs must choose. The choice and the reason are given.

### B.1 What is known, from where

| Topic | Level | Source |
|---|---|---|
| Raw CSV layout, resampling, D-min, log-E zero, dye reconstruction with masking couplers, linear unmixing, 3-CDF curve fit, sensitivity balance, midscale alignment, grey-ramp correction, paper processing | V | `src/profiles_factory/*` @ 9428440 (2026-03-16); the same code is at `agx_emulsion/profiles/*` @ b2c7fe6 (it differs only in imports and plotting) |
| Densitometer normalisation (dyes ÷ diag M, curves × diag M) | V code, F/U for its use | defined at `factory.py:446 @ 9428440` but not called by the public driver; called in the private order (`examples/…gray_ramp.py @ 5d18512`) |
| Digitising with WebPlotDigitizer; vector PDFs are easier; check axes | F | 48209 #143, #549; 58032 #18 |
| Hand extension of curves to 380–780 nm, donors, "custom calibrations" per sheet | F | 58032 #18 |
| Normalisation: midgray input gives log exposure 0 in all channels | F, and V numerically | 48209 #557; check 2 below |
| Per-stock hanatos2025 window, fitted on reflectance corpora | F (method), U (exact optimiser) | 48209 #559, #634, #672, #937; 57512 #6–#38 |
| Print media balanced against a target film's D-mid plus Y50 M50 | F | 48209 #557 |
| Non-linear unmixing; sensitivity unmixing; "preliminary neutral shift"; the reworked "refinement"; "patches by external code" | U | commits c2ccdcf, bc95c01, 5d18512, dfb0219 (messages only) |
| How `data_trustability` and `reference_channel` are chosen | U | tuned by eye (a4612dc: "lowered until positive effects were noticed") |
| DIR coupler amounts | F | one shared default per class, tuned on BabelColor (#734) |

### B.2 Facts checked for this document

Read-only on Negs `main` @ 01ee6f9. The scripts and outputs are in `~/.negs/research/film-profiling/work/`.

**Check 1: the default upsampling is `hanatos2025`, so a window is needed.**

- The default is set in `crates/spektrafilm-core/src/params.rs:936` and in `data/sidecar-defaults/v1.json:299` and `v2.json:301`. `apply_hanatos2025_adaptation_window` defaults to true (`params.rs:937`). The upstream experimental default is arctic2026, but Negs ships `hanatos2025`.
- When `hanatos2025_adaptation_window_params` is missing or shorter than 4 values, the engine silently uses the unwindowed LUT (`pipeline.rs:573-590`, `spectral_service::compute_tc_lut`). Nothing fails. The Fotufilm work measured that "no window" is the single largest error (max 19.5 ΔE00; `research/fotufilm/README.md:57`).
- Every shipped colour film has a window. **A new colour film must have a fitted window.** It is not needed for papers or for `arctic2026beta04`, which normalises per channel by itself (`spectral_service.rs:229-273`).

**Check 2: the sensitivity normalisation convention.**

- The two notes describe the same convention. With the hanatos2025 LUT sampled bilinearly at the reference illuminant's chromaticity, every shipped upstream colour film gives Σ LUT·10^logS = y_ref / 0.184 per channel. That is the raw exposure an 18.4 % grey gets after the runtime's brightness factor 0.184 / y_ref, which equals 1.000 (±0.0004).
- The constant is therefore illuminant-dependent:

  | Reference illuminant | Target Σ LUT·S per channel |
  |---|---|
  | D55 | 1.8886 |
  | `T` (Vision3 200T/500T) | 2.218 |
  | `BB3200` | 2.168 |

- `tools/fotufilm_port/convert.py:33-35,305-307` used 1.8886 for every stock. So the Fotufilm `BB3200` ports (Eterna 500, Eterna Vivid 500, CineStill 800T) give raw 0.871 at midgray, about −0.20 EV. That is a small inconsistency; the decision is in §A.
- The forum's absolute-scale change of around 2026-04-24 (#556/#557) is the move to this convention.
- **The public-era creator does not produce it.** Its C200 is offset +1.77 log10 from the shipped profile, and its per-channel balance differs by up to 0.06 log10, because it balances against the bare illuminant instead of the LUT neutral. A new profile must be rescaled per channel to the target above. Script: `work/balance_check.py`.

**Check 3: the tungsten coupler branch. Confirmed, and faithful to upstream.**

- `couplers_presets.rs:72-79` picks `cine.tungsten` only if `reference_illuminant` starts with "T". Upstream does the same (`runtime/params_builder.py:280-283 @ 28bf883`). So `BB3200` stocks get `cine.daylight`.
- That is not a Negs porting bug, but it is probably unintended for the Fotufilm tungsten ports. Do not change it (§A decision 4).
- **A new tungsten stock should use `T`.**

**Check 4: how the existing Fuji cine stocks handled the brochure gaps.**

- **Fotufilm ports (Eterna 500, Eterna Vivid 500, F-64D, Reala 500D):**
  - `datasource` cites the same brochures (KB-501E, KB-0901E, KB-912ER, KB-111E3).
  - Curves are status-M gross minus D-min, **not unmixed** (`research/fotufilm/README.md:22`).
  - Dyes have no negative mask lobes (minimum 0.0), and their origin is not stated.
  - `base_density` is extended flat from 700 to 780 nm, and the blue sensitivity runs down to 380 nm.
  - There is no `midscale_neutral_density`.
  - `log_sensitivity_density_over_min` is mislabelled 0.2; the brochure says 0.4.
  - Reala's fourth layer is dropped. Fotufilm's "donor 4th layer" has no Negs equivalent.
  - Windows were fitted by Negs.
  - Grain is R = brochure RMS, G ≈ 1.57×, B = 2×. Eterna 500 has 3.5 → [3.5, 5.5, 7.0] and F-64D 2.5 → [2.5, 4.0, 5.0] (`data/presets/grain.toml:187-197`; `data/CHANGELOG.txt:92-99`).
- **Dyeworks Eterna 250D (local branch `dyeworks-stocks`, precedent only):**
  - Characteristic curves "scaffolded" on Vision3 250D sub-layers.
  - D-min and D-mid traced from the brochure.
  - Sensitivities extended to the full 380–780 nm.
  - CMY dyes **and** the hanatos window inherited from Vision3 250D.
  - Grain derived from an ISO power law. Its claim that the sheet publishes no grain figure is wrong: KB-0507ER gives 3.5.
- **Neither precedent follows Volpato's method.** Both are useful for checking digitisation.

**Check 5: the public-era creator runs.**

- An isolated venv at `work/venv` (Python 3.13.5) takes the pinned versions: numpy 2.1.3, scipy 1.14.1, colour-science 0.4.7, lmfit 1.3.4, opt-einsum 3.4, numba 0.61.2, scikit-image 0.25, matplotlib 3.10, OpenImageIO 3.0.3.1, plus dotmap for b2c7fe6. The GUI dependencies (napari, PyQt5, pyfftw) are not needed.
- **At 9428440 the snapshot is half-restructured:**
  - The shipped JSONs lost their `_auc` suffix, but the runtime still loads `kodak_portra_400_auc` (`runtime/process.py:24`).
  - With that aliased, the steps up to `_au` run.
  - The grey-ramp correction then renders the EV 0 patch at 0.028 instead of 0.184 for every stock, including the shipped Portra 400. It fits nonsense (C200: blue shift −1.63 log E; curve refit stuck on its bounds).
  - The creator code equals b2c7fe6's, so the fault is in the snapshot's runtime. That fits the author's "what was available in the repo was stale" (58032 #18).
- **At b2c7fe6 (2026-02-27) the full chain runs** (`work/run_b.py`). For C200 and Pro 400H it reproduces its own shipped `_au` and `_auc` profiles: sensitivities, dyes and base are identical, curves within 0.012 D. It runs in seconds. The driver call is in `work/run_b.py`:

  ```python
  p = create_profile(stock='fujifilm_c200', name='Fujifilm C200', type='negative', densitometer='status_M',
                     dye_density_cmy_donor='generic_a', reference_illuminant='D55', log_sensitivity_density_over_min=1.0)
  p = process_negative_profile(p)
  p = align_midscale_neutral_exposures(p, reference_channel='green')      # Pro 400H: 'mid', X-Tra: none
  p = correct_negative_curves_with_gray_ramp(p, target_paper='kodak_portra_endura_uc', data_trustability=0.3)
  p = replace_fitted_density_curves(p); p = adjust_log_exposure(p)
  ```

- **Compared with the C200 that Negs ships (28bf883):**
  - Sensitivity shape: identical.
  - Base: identical.
  - Absolute sensitivity: +1.77 log10.
  - Dyes: differ by up to 0.36, because of peak versus densitometer normalisation and a changed reconstruction.
  - Curves in status-M units differ by 0.10–0.26 D in the red channel from log E −1 to +1, and by ≤ 0.07 D in green and blue. Pro 400H is similar.
  - Scripts: `work/compare_b.py` and `work/compare_statusm.py`.

**What check 5 means for the route.** Running the old creator and rescaling would carry over a correction fitted through the 2026-02 renderer: mallett2019 upsampling, the old enlarger, and cyan held at 0.35. The author says the correction "depends on the pipeline" (#162). **So port steps 1–7 as they are, and redo the steps that depend on normalisation and the renderer against Negs's engine.**

---

## C. Data requirements

### C.1 Curves per stock type

| Curve (public-era file) | Colour negative | Slide | Print film / paper |
|---|---|---|---|
| Spectral sensitivity B, G, R (`log_sensitivity_{b,g,r}`) | required | required | required |
| Characteristic curves B, G, R (`density_curve_*`) | required, status M, including base | required, status A | required, status A |
| Per-dye spectral density C, M, Y (`dye_density_{c,m,y}`) | optional: donor `generic_a` [V] | required [V] | required (donor possible) [V] |
| Minimum-density spectrum (`dye_density_min`) | required [V] | not used (synthesised) | not used (synthesised) [V] |
| Mid-scale neutral spectrum (`dye_density_mid`) | required [V]; it is the dye-reconstruction target | not used | not used in public; private print balancing needs a **target film's** D-mid [F #557] |
| RMS granularity (a single number is enough) | needed for the grain preset | needed | not needed (grain is negative-only, #916) |
| Exposure conditions (illuminant, time, filter) | needed for `reference_illuminant` | needed | needed (print light) |

**Accepted substitutes.**

- Generic dyes for a negative (all upstream still negatives and every Fuji negative use `generic_a`) [V].
- Another stock's curves when a sheet has none (Crystal Archive borrowed Supra Endura's, at trustability 0.1) [V].
- Sister-stock data when the sheets share it (Supra and Portra Endura, #141) [F].

**Disqualifying cases.**

- No spectral sensitivity at all.
- No characteristic curves and no credible donor.
- A colour negative without both a D-min and a D-mid spectrum; this is what made Eterna-CI unusable.
- An exposure axis that isn't light exposure (RDI's code values).
- Raster-only scans too coarse to place curves within 0.02 D or 2 nm.

### C.2 Checklist for any new datasheet

1. Document code, edition and year. Prefer the **oldest** edition with original plots (#34). Record the SHA-256.
2. Plot form: vector (exact extraction) or raster (WebPlotDigitizer). Encryption.
3. Sensitivity: wavelength range, the density criterion ("x above minimum density"), the densitometry, and whether it is log or linear.
4. Characteristic curves: the axis unit (log H lux·s, camera stops, unlabelled), any per-channel offsets stated in the caption, the densitometry (status M or A), the exposure light and time, the processing.
5. Spectral densities: per-dye curves? D-min? D-mid? The range in nm.
6. RMS granularity: the value, aperture and density level.
7. Special structures (a 4th layer, no orange mask) that the 3-layer model can't hold.
8. The class it maps to (`type`, `use`, `stage`) and the intended print (`target_print`).
9. Overlap with stocks Negs already ships (`docs/features-vs-upstream.md`, `data/profiles/`).
10. Verdict: go, go with donors (list them), or no-go (give the reason).

---

## D. Procedure: colour negative

Work in a worktree branched from **local** `main`; agent worktrees start from a stale `origin/main`. Never push. Follow the commit-dating rules in the project memory. Don't run `cargo fmt`.

### D1. Extract the curves

- **Vector PDFs (all the Fuji brochures):** use PyMuPDF (`pymupdf==1.28.2`, as in `scripts/bw_expansion/requirements.txt`) with `page.get_drawings()`.
  - **Encryption.** The Fuji files open without a password (`needs_pass` 0, checked). If a file reports `needs_pass`, call `doc.authenticate("")`. To inspect content streams, decrypt a copy with `gs -sDEVICE=pdfwrite -dCompressPages=false`.
  - **Rotation.** Pages can be rotated: `eterna500.pdf` page 0 has `rotation` 90. `get_drawings()` returns unrotated coordinates, so calibrate the axes in that same coordinate space and expect x and y to be swapped relative to the printed page.
  - **What a curve looks like.** In the brochures checked (Eterna 500, Eterna-CP 3512), curves are **stroked cubic Béziers** (`type 's'`, items `c`), 0.75–0.85 pt wide, coloured by channel: blue ≈ (0, 0.44, 0.74), green ≈ (0, 0.67, 0.31), red ≈ (0.93, 0.11, 0.14). Dye curves use cyan, magenta and yellow strokes, grey for neutral. Dashed strokes (`dashes` not empty) are the second neutral or the D-min curve. Gridlines and ticks are thin `l` strokes; the frame is a `re` path.
  - **Text is outlined glyphs.** The large filled paths are glyphs, so don't pick paths by item count; pick them by plot bounding box and colour.
  - **Sampling.** Sample each Bézier densely: at least 50 points per segment, de Casteljau on the four control points.
  - **Filled outlines.** If a curve exists only as a filled closed polygon (the inventory suspects this in places in the manual and 3512), take the centreline. Split the polygon at its two x-extreme vertices into upper and lower chains, resample both on a common x grid, and average them.
- **Axis calibration, per panel.**
  - Fit an affine map from page to data using at least 3 labelled gridlines or ticks per axis, all taken from the drawing.
  - **Gate:** every labelled tick must reproduce to ≤ 0.2 % of the axis span, and the frame corners must land on the labelled extremes.
  - Then check physically: each sensitivity lobe's peak and ends must fall where the printed plot shows them, and the 400 and 700 nm gridlines must map to 400 and 700. This is the check that would have caught the author's 50 nm stretch (#549).
  - Log the residuals.
- **Raster plots:** WebPlotDigitizer, as the author does [F #143, 58032 #18]. Keep the `.tar` project file. Record at least 30 points per curve and denser in the toe and shoulder.
- Keep the PDF coordinates of every point (the bw-expansion precedent, `scripts/bw_expansion/extract_sources.py`).

### D2. Raw data package

Place it at `data/profile-sources/fujifilm-cine/`, modelled on the public-era layout (`data/film/negative/<stock>/` @ 9428440) and on `data/profile-sources/bw-expansion/`:

```
data/profile-sources/fujifilm-cine/
  README.md                      what, how, licence, how to regenerate (copy bw-expansion/README.md structure)
  recipe.json                    per stock: see keys below
  donors/generic_a/dye_density_{c,m,y}.csv + info.txt   (copied from the 9428440 snapshot, with source note)
  <stock_id>/
    log_sensitivity_{r,g,b}.csv  x = nm, y = log10 sensitivity as plotted
    density_curve_{r,g,b}.csv    x = log10 H (converted, §D3), y = status-M density incl. base
    dye_density_min.csv          x = nm, y = spectral density (minimum)
    dye_density_mid.csv          x = nm, y = spectral density (mid-scale neutral)
    info.txt                     one line, e.g. "Fujifilm Eterna 250D KB-0507ER 2005"
    trace/<curve>.csv            the same points with pdf_x, pdf_y
```

- **CSV format:** two comma-separated columns, no header, full precision, so the public loader (`utils/io.py:108,134 @ 9428440`) reads them unchanged.
- **`recipe.json` per stock:**
  - `stock`, `name`, `type`, `support`, `stage`, `use`, `antihalation`, `channel_model`;
  - `densitometer`, `log_sensitivity_density_over_min`, `reference_illuminant`, `viewing_illuminant`;
  - the four `*_donor` fields, `dye_density_reconstruct_model`, `reference_channel`, `target_print`, `data_trustability`. These keys mirror the private `profile.yaml` (`profile_summary.csv @ 03832c8`).
  - Also record:
    - the document code, edition, file name and SHA-256 (short prefixes are in `fuji-data-inventory.md` §2), page and panel bounding box;
    - the affine and tick residuals;
    - the axis conversion;
    - the exposure conditions and RMS figure as printed;
    - every assumption, with its §B mark.

### D3. Axis conversions

| Gap | Rule | Mark |
|---|---|---|
| "Camera stop" axis (Eterna family) | log10 H = 0.30103 × stop; leave the offset arbitrary. The creator centres on the green curve and then sets zero itself (step 2). | V (the offset is irrelevant: `utils/io.py:151`, `adjust_log_exposure`) |
| Bar-notation log H (F-64D, Reala 500D, CI) | n̄.f means −n + 0.f (e.g. 3̄.5 = −2.5) | V (arithmetic) |
| Print curves without numeric log H | Use the 0.5 log H scale bar to scale. Undo the caption's "G and B shifted 1.0 log H". The absolute offset is arbitrary. | N |
| Sensitivity at 0.40 above minimum (negatives), 1.0 (RDI, prints) | Use the curves as plotted and record the level in `log_sensitivity_density_over_min`. Only the shape matters: the absolute level is renormalised (step 5), and the only consumer of this field, `unmix_sensitivity`, is disabled (b2c7fe6) and never changed the output (`factory.py:175`). Upstream used 1.0 for Fuji still and 0.2 for Kodak. | V |
| Spectra end at 400 / 700 nm | **Leave them as NaN outside the plotted range**, as the public loader does (Akima, `extrapolate=False`) and as the shipped C200 does: its base and D-mid are null below 400 and above about 715 nm. At runtime a NaN density passes **no** light (`enlarger.rs:426,602`) and a NaN sensitivity is zero. The author says he sometimes extends curves "by hand… really inventing the continuations" (58032 #18). Do that only if a §D8 check fails because of the 700–780 nm band: extend D-min and D-mid flat from their 695–700 nm mean, and state it in `datasource`. | V default, F/N fallback |
| Reala 500D fourth (cyan-sensitive) layer | Use B, G and R only and state the omission. | N |

### D4. Processing, in order

Use the public creator functions (ported, §F) with these per-stock choices.

**1. Load and resample [V].**

- 380–780 nm in 5 nm steps (81 samples). log E −3…4 with 256 points.
- Akima interpolation, NaN outside the data.
- Density curves are centred on the green curve's x mid-range (`load_agx_emulsion_data`, `utils/io.py:151 @ 9428440`).
- Densitometer status M from `data/densitometer/status_M/` (Giorgianni et al.), with linear interpolation and each channel summing to 1.

**2. `remove_density_min`, then `adjust_log_exposure` [V]** (`factory.py:310,338`).

- A toe fit on the lowest fifth of each curve removes D-min.
- Zero log E is placed where green reaches 0.2 over D-min, plus 3 stops (0.903).
- Fixed parameters: `speed_point_density` 0.2, `stops_over_speed_point` 3.

**3. `reconstruct_dye_density(model='dmid_dmin')` [V]** (`reconstruct.py:258`).

- Start from the `generic_a` dyes (donor for every Fuji negative, [V]).
- Fit shift (±30 nm), stretch (0.9–1.1) and scale (0.5–1.5) per dye, plus five subtractive Gaussian masking-coupler bands, so that Σ cmy = D-mid − D-min.
- D-min is modelled at the same time.
- Bias towards zero status-M crosstalk, with `biases=(1,2,0)`.
- lmfit `least_squares` with `ftol=xtol=gtol=5e-5` and `max_nfev=500`.
- Keep the control plot.
- **Fuji risk:** the magenta coupler band at 650–720 nm (`cpl_max3`) has data only up to 700 nm. Inspect that band in the plot.

**4. `unmix_density` [V]** (`factory.py:429`).

- Build the status-M crosstalk matrix of the reconstructed dyes, apply its inverse to the curves, clip at 0.
- The private creator replaced this with a "non-linear" unmixing for high densities (#552) [U]. Keep the linear version.

**5. `replace_fitted_density_curves` [V]** (`factory.py:461`).

- Sum of 3 normal CDFs. Starting centres at mean −0.5 / 0 / +0.5, amplitudes 0.2–1.35, σ 0.05–2.

**6. `balance_sensitivity` [V], then refit [V].**

- Balance against the bare reference illuminant, shifting the curves by the same log correction (`balance.py:10`).
- Step D5 replaces its absolute and per-channel result; keep it for parity with the public output.

**7. Choices for the alignment step.**

- `align_midscale_neutral_exposures(reference_channel)` [V] (`correct.py:22`). Upstream Fuji used `'green'` (C200), `'mid'` (Pro 400H) or none (X-Tra 400). No rule is known [U].
- **Rule [N]:** run all three variants through step 10 and keep the one with the lowest grey-ramp neutrality error among those whose neutral filters on the target print fall in the shipped films' range (check 8). Record all three errors. (Added 2026-11-14: for Eterna 250D and Vivid 160 the unaligned variant was most neutral on the ramp but rendered grey 0.16–0.27 D less red than the datasheet's own midscale neutral and needed a magenta filter below every shipped film's.)

**8. `densitometer_normalization` [V code, F/U use]** (`factory.py:446`).

- Dyes ÷ diag(M), curves × diag(M). The rendered spectral density is unchanged, but the dye-amount scale matches the shipped profiles (their status-M diagonal is about 1.0–1.1).
- The DIR gammas and grain presets were tuned on that scale.

**9. Normalise to the Negs convention.** See §D5.

**10. Grey-ramp refinement [V for the public form; N for running it on the 28bf883 engine].**

- **Public form** (`correct.py:35-127 @ b2c7fe6`):
  - First solve the neutral print filters for (film, `target_print`).
  - Then render an 18.4 % grey at camera exposure EV −2, −1, …, +6 through the target print, with spatial and stochastic effects off, glare off and linear output.
  - Least-squares fit of five parameters: a density scale per channel and log-E shifts for R and B. The residual is printed RGB − 0.184, plus a regulariser 2·(scale − 1)·`data_trustability`.
- **Negs choice:** run it on the upstream 28bf883 Python runtime, which matches Negs by parity (`scripts/parity/`), using the import approach of `scripts/derive_neutral_print_filters.py`. Apply the correction directly to `density_curves_model`: amplitudes × scale and centres + shift, which is exact for norm_cdfs. Then re-zero log E (step 2 rule) and re-check step 9.
- **Parameters [V for upstream Fuji and Vision3]:**
  - `target_print = kodak_2383` (Vision3 precedent; the author corrected cine stocks against print film, #213);
  - `data_trustability = 0.3`;
  - the upsampler barely matters on a neutral ramp because the window keeps neutrals fixed. Use `hanatos2025`.
- The private era reworked this step ("refinement", f1e7946: "only global log_exposure correction") [U].
- **Acceptance (author, #523):** midtones neutral; shadows and highlights may drift, but in opposite directions.

**Per-stock choices for Fuji cine negatives.**

| Field | Value | Mark |
|---|---|---|
| `densitometer` | `status_M` | V |
| `log_sensitivity_density_over_min` | 0.4 (RDI 1.0) | V (recorded as printed) |
| `reference_illuminant` | `D55` for 5400 K stocks; `T` for 3200 K stocks (§A decision 3) | V (D55: Kodak and Fuji upstream, #747; T: Vision3 tungsten) |
| `viewing_illuminant` | `D50` | V |
| dye donor | `generic_a` | V |
| `target_print` | `kodak_2383` | V |
| `data_trustability` | 0.3 | V |
| `reference_channel` | by the rule in step 7 | N |
| `use`, `antihalation` | `cine`, `strong` (as Vision3 and the Eterna ports) | V for the tags; N for the choice |

### D5. Scaling to the Negs convention [F #557 + V numerically]

1. Take the hanatos2025 LUT `data/luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy`, sampled bilinearly at the tc coordinates of the reference illuminant's CIE 1931 xy (for the SPD formulas see `work/balance_check.py`; `T` is colour-science `SDS_LIGHT_SOURCES['Incandescent']`).
2. Per channel, scale S so that Σ LUT·S = y_ref / 0.184 (D55 1.8886; T 2.218; BB3200 2.168).
3. Shift each channel's curve model centres by the same log10 factor, as `balance_sensitivity` does.
4. Re-zero log E (step 2 rule) and run step 10 after this.
5. Re-run step 5 last if step 10 moved anything.
6. **Gate:** after the window is applied, the raw exposure at midgray is within 0.0005 of 1 per channel. Check in `work/balance_check.py`, and use the engine's dump `SPEKTRAFILM_DUMP_FILM_RAW` as a second check.

### D6. Window, filters, grain, couplers, halation

**Window [F method, N implementation].**

- Run `tools/fotufilm_port/fit_window.py`: erf4 window, corpus Otsu/NIST skin/forest/Munsell at 50/20/20/10, per-channel log-exposure loss under the reference illuminant, neutral held fixed. This is Negs's reconstruction of #559/#634. It left the stored upstream windows near-optimal (`research/fotufilm/README.md:46-50`).
- It needs the corpus files. The URLs are in every upstream profile's `metadata.datasource`. Fetch them into the tool's data folder; they are not on this Mac. Patch its hard-coded Linux paths.
- **Gate:** the loss improves on no window, and the parameters fall within the shipped range: c_uv 391–456, σ_uv 9–50, c_ir 567–670, σ_ir 22–70.

**Neutral print filters [V].**

- Every colour paper needs an entry (8: crystal_archive_typeii, 2383, 2393, ektacolor_edge, endura_premier, portra_endura, supra_endura, ultra_endura):

  ```
  cargo build --release -p spektrafilm-cli --features precision-f64 --bin solve_neutral_filters
  target/release/solve_neutral_filters data <paper> <film>
  ```

- Add new keys only, following the `ship.py` and `add_filters.py` pattern.
- **Gate:** finite values, C = 0, and M and Y within the range existing films have on that paper (2383: M 43–94, Y 40–121). Negative values signal miscalibration (#135).

**Grain [N, following the existing Fuji cine rule].**

- `grain.toml` gets `[<id>] rms_granularity = [3.5, 5.5, 7.0] × (brochure RMS / 3.5)`. That reproduces the shipped Eterna 500, F-64D and Reala presets within 0.3.
- Optional validation is the author's procedure: render a ramp at a 48 µm aperture and compare std × 1000 (#230).
- Add the tables in the Negs-only block at the end of the file.

**Couplers [V].**

- The class defaults apply automatically (`cine.daylight` or `cine.tungsten`).
- Run `tools/fotufilm_port/fold_check.py`. Only if the curve folds, add a per-stock table with `coupler_presets.py` (uniform scale, 0.1 margin; ticket 22).

**Halation [V].** From the `use` and `antihalation` tags; nothing per stock.

### D7. Negs integration checklist

The full checklist is in `negs-stock-integration.md` §3; the touch points are:

1. `data/profiles/<id>.json`:
   - `info.stock` equals the file stem.
   - `metadata.version` must not start with `fotufilm-port`.
   - `manufacturer_data` true.
   - `density_curves_model.model_type` is `norm_cdfs`, `alphas` null, 3 sub-layers.
   - Floats rounded to 6 decimals, NaN written as `null`.
   - Include `midscale_neutral_density` (raw D-mid) for upstream compatibility.
2. `data/filters/neutral_print_filters.json`: new keys only.
3. `data/presets/grain.toml`, and `couplers.toml` if needed: Negs-only blocks, existing tables untouched. Both files are compiled in, so rebuild.
4. `crates/spektrafilm-io/src/film35.rs` `STOCKS`: `EdgePrint::Cine { product: "<35 mm type no.>", keykode: None }`. The type numbers are in `fuji-data-inventory.md` §3: 8553, 8563, 8583, 8543, 8546. Then regenerate the strips with `cargo run --release -p spektrafilm-io --example film35_borders` and revert the spurious `fujifilm_velvia_100.png` change.
5. Refreeze `crates/spektrafilm-core/tests/fixtures/halation_presets_python_28bf883.json` (`scripts/parity/freeze_halation_presets.py`).
6. Counts:
   - `linux/src/catalog.rs:217-219` and `windows/src/model/lifecycle_tests.rs:195-231` (52 films and 10 papers today);
   - `linux/catalog.json` via `python3 linux/generate_catalog.py target/release/spektrafilm-native`;
   - `web/catalog.json` via `web/build.sh`.
7. README film table and counts, `web/credits.html`, `docs/features-vs-upstream.md`, release notes. The site (`~/Projects/negs-site`) only when Noah says.
8. **Existing exports stay byte-identical:**
   - `scripts/golden/run.py run --hashes-only`, then `compare --exact` against `scripts/golden/manifests/macos-arm64.json`;
   - `cargo test --release -p spektrafilm-core -p spektrafilm-io -p spektrafilm-native`.
   - If any golden image moves, a shared table changed. Revert it.

### D8. Validation: pass criteria and what to do on a failure

| # | Check | Pass | If it fails |
|---|---|---|---|
| 1 | Axis calibration | tick residual ≤ 0.2 % of span; frame corners exact | re-pick the ticks; check the rotation and the axis swap. A hand-drafted chart whose own gridlines are uneven may get a recorded per-axis exception (`gate` and `gate_reason` in the recipe) when a physical check bounds the error (Noah, 2026-11-14; the Eterna Vivid brochures, 0.4 %) |
| 2 | Extraction against a precedent (calibration stock Eterna 500) | offset-free log S shape within 0.05 of the Fotufilm port over the common range; D-min within 0.02 D (400–700 nm); gross curves minus D-min within 0.03 D after aligning green at 0.2 over D-min | find which side is wrong by overlaying on the PDF render; the port is a trace, not ground truth |
| 3 | Port of steps 1–7 (calibration stock C200, from `9428440/src/spectral_film_lab/data/film/negative/fujifilm_c200`) | log S and dyes equal to `work/creator-b2c7fe6/.../fujifilm_c200_REGEN_au.json` (≤ 1e-6 and ≤ 1e-4); curves ≤ 0.015 D | diff step by step against the b2c7fe6 run |
| 4 | Curve-model fit | ≤ 0.02 D against the unmixed data in the digitised range | add starting points; never widen the bounds silently |
| 5 | Normalisation | raw midgray 1 ± 0.0005 per channel | redo §D5 |
| 6 | Reconstruction | Σcmy reproduces D-mid − D-min within 0.03 D (400–700 nm); the crosstalk matrix is near-diagonal | inspect the coupler band near 700 nm; try the §D3 fallback extension. If it still fails, it may still ship (Noah, 2026-11-14) when the residual weighted per print channel (target print sensitivity × 3200 K lamp through the D-mid, `build.py` `check6_print_weighted_rgb`) is ≤ 0.01 D; record the figure and the worst wavelength in `datasource` |
| 7 | Grey ramp through `kodak_2383`, EV −2…+6 | neutral midtones (EV 0 \|R−G\|, \|B−G\| ≤ 0.005 in print RGB); mean neutrality error **not worse than the worst** of Vision3 250D/500T and the shipped Fuji upstream negatives on the same ramp; opposite drift in shadows and highlights | change `reference_channel`; lower trustability only to 0.1 (Crystal Archive precedent) and record it |
| 8 | Neutral filters | per §D6 | as in #523: if refitting the filters fixes a cast, the stored filter was wrong |
| 9 | Fold check | 0 backwards steps per channel at defaults | per-stock coupler table |
| 10 | Window | per §D6 | refit with the prior towards the median (`fit_window.py`) |
| 11 | Preflash | no strong blue cast versus siblings (users report one for Fuji, 57512 #156) | note it; check the base density and filters |
| 12 | Visual | renders of Noah's NEFs (DSC_5784/5809/6298/6448) and `tools/fotufilm_port/make_charts.py` charts, grain and glare off, next to Vision3 250D/500T and the Eterna 500 port; no stock-defining oddity (yellower greens may be right, #734) | report to Noah; don't hand-tune colour against scans (#747) |
| 13 | Regression | goldens exact; all cargo tests pass | §D7 item 8 |

**Stop conditions.**

- Stop and report if check 3 fails after a real attempt: the port is wrong, and nothing built on it is meaningful.
- Stop and report if check 7 fails at trustability 0.1 under all three alignment variants. That is a "challenging film stock" (#523), and the choice goes to Noah.
- Stop and report on any golden image change you can't trace.

### D9. Documentation, licence and credits

- Each `metadata.datasource` follows the template in `docs/adding-film-stocks-research.md:289-302`:
  - the document code and edition;
  - the Negs method name, e.g. "spektrafilm public-era creator (b2c7fe6) steps 1–8, Negs normalisation and refinement";
  - the donors, including `generic_a` (Giorgianni, Madden & Kriss 2009, p. 112, via spektrafilm);
  - every [N] choice and every extension.
- `license`: CC BY-SA 4.0 (§A decision 6). `copyright`: Negs authorship for the new preparation, with Volpato credited for the method and the generic dyes.
- `data/CHANGELOG.txt` gets a dated entry listing every file changed. CC BY-SA requires modification notes, and `SPEKTRAFILM_LICENSE.txt` asks for a changelog.
- The README credits are extended if needed. Fujifilm data stays © FUJIFILM; don't commit the PDFs.

### D10. Differences for print films (and slides)

**Print films [V public, F private].**

- Process with `process_paper_profile`: `remove_density_min`, then `adjust_log_exposure` (zero where green is 0.735 over D-min), then `balance_metameric_neutral` (a D-mid seen under `viewing_illuminant` is neutral at 0.184), then `unmix_density` (status A), then the fit (`factory.py:532`).
- The base spectrum is synthesised from the channel D-mins at 445/530/610 nm with σ 20 nm. That fills the brochures' missing base.
- Use the per-dye spectra given. They have no masking couplers.
- Set `reference_illuminant` to `TH-KG3-L` and `viewing_illuminant` to `K75P` (as 2383). Use `support: film`, `stage: printing`, `densitometer: status_A`, `log_sensitivity_density_over_min: 1.0`.
- **Private era [F #557]:** normalise the print sensitivities to TH-KG3-L attenuated by a target film's D-mid and by Y50 M50 CC filters (100 CC = 1 OD). For Fuji the target is an Eterna negative with a published D-mid (Kodak prints use Vision3 250D) [N].
- **Integration:**
  - The name must contain "Print Film", or the UIs file it as a colour paper (`isMotionPicturePrint`, `Parameters.swift:226`, `web/app.js:803`, `windows/src/state.rs:240`, `linux/src/catalog.rs:51`).
  - Every colour negative needs a neutral-filter entry for the new print (38+ entries).
  - It needs a `film35.rs` `EdgePrint::Print(..)` entry.
  - No grain preset.
- **Validation:** a grey ramp of Vision3 250D and of the new Eterna stocks through it; check the 3521XD/3513DI comparison plot. Kodak's 2393 had odd data (#234), so expect trouble.

**Slides** (not in the Fuji folder):

- Own dyes are required.
- No D-min or D-mid files.
- `correct_positive_curves_with_gray_ramp` (EV −1…2, hanatos2025).
- The private "neutralisation" of slides is [U] (#552).
- Negs expects mid-grey to view at Y 0.18 (`tools/fotufilm_port/slide_neutral.py`).
- Steep slides fold, so run the fold check.

---

## E. Fuji plan

Inputs are in `~/Downloads/fuji spectral/`. Page contents are in `fuji-data-inventory.md` §2–4.

| Order | Stock | Inputs | Gaps and the substitute chosen | Ships? |
|---|---|---|---|---|
| 0a | Fuji C200 (calibration) | public raw CSVs @ 9428440 | none | no; gate D8 check 3 |
| 0b | Eterna 500 (calibration) | `eterna500.pdf` KB-501E p1: S, curves (stops), D-min, D-mid, RMS 3.5 | generic dyes; NaN outside 400–700; `T` | no (decision 5); gate D8 check 2, and a full-pipeline comparison with the port |
| 1 | Eterna 250D (8563) | `eterna250d.pdf` KB-0507ER | as Eterna 500; `D55` | yes, after checks; the first daylight stock, so the best-validated path |
| 2 | Eterna Vivid 250D (8546) | `eterna_vivid250d.pdf` KB-1009E | as above; the caption's "5400K tungsten" is a typo, use D55 | yes |
| 3 | Eterna 250 (8553) | `eterna250.pdf` KB-0506ER | `T` | yes |
| 4 | Eterna 400 (8583) | `eterna400.pdf` KB-0508E; RMS 3.7 | `T` | yes |
| 5 | Eterna Vivid 160 (8543) | `eterna_vivid160.pdf` KB-0701E | `T` | yes |
| 6 | Eterna-CP 3513DI, then 3521XD, 3512, 3514DI, 3523XD | print brochures: S, curves (unlabelled), per-dye, Gray | base synthesised; G and B −1.0 log H; target-film D-mid from #1 | deferred (decision 2) |
| – | F-64D, Reala 500D, Eterna Vivid 500 | brochures | already shipped (Fotufilm) | re-derive only if decision 5 changes; Reala drops its 4th layer |
| – | Eterna-RDI, Eterna-CI, F-CP | | not camera film, or curves only | not attempted |

For every brochure, the manual (`fujifilm_motion_picture_film_manual.pdf`) repeats the characteristic curves. Use it as a second trace (they should agree within 0.02 D), and for the edge-marking pages (p31–34).

### E.1 Still negatives from the Professional Film Data Guide (2026-11-20)

The FUJIFILM Professional Film Data Guide (AF3-207U, 2005) has each still negative's sensitivity, characteristic and dye charts, two stocks per spread. Record: `data/profile-sources/fujifilm-pfdg/README.md`. What differs from the Eterna brochures:

- **Black curves.** All three curves of a chart share one colour and often one PDF path. `extract_pdf.py` splits paths into subpaths and selects curves by line style and position (`style_match`, `select_subpath`).
- **Filled, skewed frames.** Plot areas are grey fills or four-line quads skewed by about 0.25 pt (`quad_rect`, `frame_kind: "fill"`).
- **Hand-drafted grids.** One uneven template is reused across spreads. Fit axes to the labelled gridlines only; frame edges make it worse. Expect per-axis gates of 0.3–0.5 % with a `gate_reason`, and bound them with a cross-check against an independent trace (X-TRA 400 against spektrafilm's AF3-151E: 0.006 D).
- **Relative sensitivity axes.** No tick values, only a 1.0 scale bar between two gridlines: `fit_axis` accepts two positions when `relative_reason` is set. D5 removes the offset.
- **Target print** `fujifilm_crystal_archive_typeii` and `use: still`, as the shipped Fujifilm still negatives. D8 check 8's range is that paper's shipped films (yellow 50.0–116.5).
- **Fourth layer.** Fujifilm's cyan-sensitive fourth layer (dashed) is not modelled; recorded in each profile's notes.
- **Product codes are not identities.** True Definition 400 carries X-TRA 400's [CH] but different curves (up to 0.59 D); compare traces before treating two stocks as one. Noah decided (2026-11-20) to ship it as a film of its own.

Shipped: Pro 160S, Pro 160C, Pro 800Z, Superia 1600, and True Definition 400 (2026-11-21). Stopped at D8 and kept for a decision: NPL 160 and Superia 100 (print-weighted dye residual 0.013/0.011 D, bound 0.01), X-TRA 800 (check 8). Calibration only: Pro 400H, X-TRA 400, Reala, Superia 200.

---

## F. Tooling gap

Negs has no colour profile factory. `scripts/bw_expansion/` is B&W-only, and `tools/fotufilm_port/` converts finished profiles. **Recommended approach:** a new uv/venv Python project, `tools/film_profiles/`. uv is not installed on this Mac; the research venv shows the pinned set installs on Python 3.13.

| Module | Content | Reuse |
|---|---|---|
| `extract_pdf.py` | PyMuPDF path selection, Bézier sampling, centreline for outlines, affine calibration and gates, `trace/*.csv`, hashes | the pattern of `scripts/bw_expansion/extract_sources.py` |
| `creator/` | steps 1–8 ported **verbatim** from b2c7fe6 (`factory.py`, `reconstruct.py`, `balance.py`, `correct.py` minus the runtime parts; loader `utils/io.py`, `utils/measure.py`, `model/density_curves.py`), with a GPLv3 attribution header. The Negs licence is GPL-3, so this is compatible. | the b2c7fe6 snapshot (1,444 lines in all; about 800 needed) |
| `normalise.py` | §D5 | `tools/fotufilm_port/convert.py` balance (with a per-illuminant target), `negsmodel.py` |
| `refine.py` | step 10 on the 28bf883 runtime | `scripts/derive_neutral_print_filters.py` imports, `scripts/parity/` tree hash |
| `write_profile.py` | Negs JSON, metadata, rounding | `convert.py` `to_list` and its curve-model writer |
| `checks.py` | the §D8 gates and a report JSON | `fold_check.py`, `fit_window.py`, `solve_neutral_filters`, `compare_negs.py`, `make_charts.py` |
| `ship.py` | copy into `data/`, new filter keys only, refuse overwrites | `tools/fotufilm_port/ship.py` |

- **Size:** about 1,500–2,000 lines of Python plus a Rust test along the lines of `crates/spektrafilm-core/tests/bw_expansion.rs`. That is two to four focused agent sessions, the first spent mostly on D8 check 3.
- **Regeneration must be byte-identical** (the bw-expansion standard).
- Keep the Fotufilm tools' hard-coded Linux paths out of it: pass paths as arguments.

---

## G. Open questions for the author

Noah could post these in the 58032 thread. The author's Eterna-CP poll option and his promise that the creator "will come soon" (#18) are the natural hook.

> Hi Andrea, thanks for spektrafilm. I'd like to add Fujifilm Eterna negatives (250, 250D, 400, Vivid 160/250D) to Negs from the official brochures cinnyboy shared here, following your method as closely as I can, and credit you. A few questions where the public code (last at 9428440) and your posts leave me guessing:
>
> 1. Is the profile creator still planned for release? If so, I'd rather wait for your current steps (non-linear unmixing, sensitivity unmixing, "preliminary neutral shift", refinement) than approximate them.
> 2. The Fuji brochures give sensitivity at 0.4 above minimum density, spectra only from 400 to 700 nm, and the exposure axis in camera stops. Do you extend D-min/D-mid beyond 700 nm, and how?
> 3. What exactly made Fuji datasheets "not self-consistent" for you? Which curve disagreed with which?
> 4. How do you choose `reference_channel` ('green' for C200, 'mid' for Pro 400H) and `data_trustability`?
> 5. For tungsten cine stocks, would you use 'T' or 'BB3200'? The coupler defaults switch on the name starting with T.
> 6. For a Fuji print film such as Eterna-CP 3513DI, which target film's mid density would you balance it against?
> 7. May the `generic_a` dye spectra from the old GPLv3 repository be used in CC BY-SA 4.0 profiles, with credit to you and Giorgianni & Madden?
> 8. Would you want the digitised Fuji curves (CSV in your old layout, with datasheet codes and hashes) for your own campaign?

Two things to check in the meantime, without the author:

- whether D55 versus the brochure's 5400 K changes the correction needed (#63: the original Fuji illuminant was a guess);
- whether the private-era profiles show any "patch by external code" (dfb0219) on Fuji stocks. Diff `fujifilm_c200.json` across main, dev and experimental.
