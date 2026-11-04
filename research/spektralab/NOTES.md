# SpektraLab (JamesQiu2005/SpektraLab): research notes for Negs

Written 2026-11-04. Read-only investigation of a temporary clone of
https://github.com/JamesQiu2005/SpektraLab (untrusted, nothing run or built; clone not kept),
`origin/main` at `02d82b5` (2026-11-02, "chore(release): 1.3.5 (20)"), with upstream spektrafilm
added as remote `upstream`.

What it is: a macOS app (SwiftUI + C++20/Metal engine; Windows/Linux Vulkan work on branch
`Windows_Linux`) that ports spektrafilm to native code, like Negs does in Rust. Author Hanze Qiu
(363 of 547 commits), much of it written with Claude/Codex. It contains **no Python engine**: the
Python reference stays in a separate local checkout, used only by parity harnesses.

Tags used below: **[read]** = seen in the repo; **[inferred]** = my conclusion, not stated there.

---

## Ranked summary: what could be useful to Negs

1. **Published RMS granularity for four B&W films, measured the same way Negs defines
   `rms_granularity`.** SpektraLab's `FilmParams.ownGrain`
   (`modern_UI/Spektrafilm/Spektrafilm/Model/Params.swift:592-611`, commit `d610ac9`) uses targets
   Tri-X 400 = 17, T-Max 100 = 8, Acros II = 7, HP5 Plus = 16 (σ_D × 1000, 48 µm aperture, net D 1.0).
   These are the same numbers `docs/handoff-fotufilm-grain.md` takes from Fotufilm, so two
   independent ports agree. HP5's 16 comes from Ilford's *motion-picture* sheet, because the still
   sheet gives none. Their per-film multipliers (1.70 / 0.74 / 0.64 / 1.49) belong to their older
   grain model and **do not carry over to Negs**. The targets and the way they were measured do.
   *Why it matters:* it backs plan step 1 of the grain handoff (B&W `grain.toml` sections) and step 2
   (check the anchor). *Effort:* tiny (data only). The values are facts from datasheets, so there is
   no licence issue.

2. **A variable-contrast silver paper: Ilford Multigrade IV RC, grades 00 to 5.**
   `engine/resources_product/profiles/ilford_multigrade_iv_rc.json` and `paper_grades.json`
   (commit `4599179`). It models the paper as three emulsions in the three channels, all spectrally
   flat (neutral silver), so the total density is the sum of the three. The emulsions share a blue
   sensitivity and have green sensitivity at 0, -0.76 and -1.73 log10. The grade is just the
   enlarger's Y/M dichroic pack under TH-KG3: grade 00 = Y199, grade 2 = Y68, grade 5 = M151.7, each
   with its own `print_exposure`. The standard 3-channel spektrafilm print path handles it with **no
   engine change**. Negs has only `ilford_mgrc_deluxe_grade2`, a single emulsion at one fixed grade.
   *Why it matters:* a real grade control for B&W prints. *Effort:* medium to high. Negs' B&W profiles
   are one-channel, so we would need to check that a one-channel film can expose a three-channel
   neutral paper. We would also need a grade UI and a neutral-pack row per film. The data's licence
   is unclear (section 5), so the safest route is to **re-derive it ourselves from the HARMAN sheet
   using their method**, not to copy their JSON.

3. **A halation model for CineStill (no anti-halation layer).** `engine/src/core/halation_psf.hpp`
   and `engine/tests/halation_psf_fit.py` (commits `389ecc0`, `342c974`). It models light reflecting
   off the back of the film base: Fresnel reflection with total internal reflection past 42.5°, a 10 %
   return from the camera's pressure plate, and a ring at 1.83 × the base thickness d. Each colour
   layer gets its own blur: per-layer path absorption τ = 0 / 1.2 / 3.0, fitted as four Gaussians per
   layer, d = 120 µm, strengths R/G/B 0.30 / 0.03 / 0.003. A `halation_headroom_ev` setting (default
   4) restores the light that clipped highlights lost. Negs ships `cinestill_800t` and
   `cinestill_400d`. *Why it matters:* the stock Gaussian `no` preset does not look like CineStill
   (their own finding, from scans). *Effort:* medium (one new PSF path, plus headroom gating). The code
   is GPL-3.0-or-later, so it can go into GPL-3.0-only Negs with credit. The constants were tuned on
   the owner's scans and an "LLM study" ("GPT Astra").

4. **Checking grain by its distribution, not pixel by pixel.** `engine/tests/parity_grain.py`
   renders flat patches at 9 input levels (0.01 to 1.5) and compares mean, std and **skewness**
   (tolerances 4e-3, 8 %, 0.25). See also the AGENTS.md traps "8. Approximating a distribution can
   preserve RMS and still change the look" and "9. Grain draws are i.i.d.". *Why it matters:* this is
   a cheap way to check Negs' GPU grain against CPU when the random streams differ. *Effort:* small.
   Their sampler draws a single `Poisson(n/sat · p)` (`engine/src/shaders/spk_common.h:184`). That
   has the same distribution as Negs' Poisson-then-Binomial pair, at one draw per sub-layer. It is
   only an option for GPU paths that don't need numpy bit parity.

5. **More lens colour filters, with a filter-factor normalisation.**
   `engine/src/core/camera_filters.hpp` (commit `6d46e08`; API-SPEC §15) has Wratten 8, 11, 12,
   15, 21, 25, 47 and 58 at 380-780/5 nm. The filter multiplies the film's sensitivity, and the
   result is divided by the green layer's ratio under the film's reference illuminant, so 18 % grey
   keeps its exposure. Checked against Tri-X's published factors: 6 of 7 within 0.21 stop, No. 58 off
   by 0.67. Negs already has 5 Hoya filters (`data/filters/colored/hoya`, `color_filters.rs`), so the
   new parts are blue 47, green 58 and the normalisation. *Effort:* small. The data's origin is weak:
   Kodak B-3 tables "as transcribed by P. Repacholi (Usenet, 1992)".

6. **Smaller ideas.**
   - The "crop is the frame" grain scale (commit `13ecce8`, `Session.swift:3330-3410`): an optional
     setting where a crop rescales `film_format_mm`, so grain and halation follow the crop.
   - Effect strengths as **area** multipliers (commit `082f7ac`, "RFC-034"): halation and glare
     sigmas scale by √amount.
   - Per-paper calibration for HDR (EDR) display (commit `d04eccf`): a `data.edr_tone_map` added to
     the 8 upstream papers.
   - A runbook for "adding a stock" (`PRD/PRD-v1.4.0.md` §4.2), plus the finding that each
     (film, paper) pair needs its own neutral filter row.

Not useful: no new colour papers or print films, no changes to couplers, and nothing on grain from
upstream after 2026-05-30. On all of that Negs is **ahead** of SpektraLab (sections 1 and 3).

---

## 1. Relationship to upstream spektrafilm

- **Base:** upstream `main` as of 2026-05-30. The fork's last upstream commit is `4fb6559`
  ("Merge branch 'dev'", andreavolpato, 2026-05-30 15:40 +0200). It matches upstream `0a44600` by
  subject, date and author. `0a44600` is an ancestor of the `experimental` commit `28bf883` that
  Negs is pinned to: 77 upstream commits lie in `0a44600..28bf883`. [read]
- **History was rewritten.** `git merge-base origin/main upstream/main` (and `upstream/experimental`)
  returns only the root `a09c162` ("Initial commit", 2025-02-07). In the fork's copy, upstream
  commits keep their messages, but at `4fb6559` the tree holds only `.gitignore`, `CITATION.cff`,
  `LICENSE` and `SPEKTRAFILM_LICENSE.txt`. The fork's early Python commits (e.g. `a2fcb15`,
  `d31817b`) now touch only `.gitignore` and `AGENTS.md`. The rewrite most likely happened at
  `00e0179` (2026-09-11, "chore(repo): stand the desktop product up as its own repository"). [read;
  the cause is inferred]
- **Divergence:** 410 commits on `origin/main` after `4fb6559`. The diff from the base is 725 files,
  +1,138,670 lines. Nothing from upstream has been merged since 2026-05-30. [read]
- **What it therefore lacks from upstream** (all present in Negs' pin `28bf883`): grain presets
  (`a9299ce`/`8cbfb01`, 2026-06-06), the mass-conserving grain unsharp mask (`ffbee22`), "reduce
  grain ringing" (`b3e65de`), Langmuir/cine DIR-coupler presets (`2ea9724`, `11e8b90`) and the arctic
  spectral upsampling (`f4ffe5c` and later). Its grain is upstream main's older model with global
  defaults (`engine/src/core/params.hpp:32-50`: `particle_area_um2 = 0.2`,
  `particle_scale = {1.6,1.6,3.2}`, `particle_scale_layers = {2.0,1.0,0.5}`,
  `density_min = 0.03`, `uniformity = {0.97,0.99,0.97}`, `blur = 0.65`,
  `blur_dye_clouds_um = 1.0`, `micro_structure = {0.2, 30}`). [read]
- **Its 28 spektrafilm profiles** (`engine/resources/profiles/`) are byte-identical to upstream main
  `0a44600`, except the 8 papers, which gained `data.edr_tone_map` (commit `d04eccf`). None is
  identical to the `28bf883` copies or to Negs' `data/profiles` (checked by git blob hash). [read]
- The README calls it "a native C++/Metal port of that engine"; `CITATION.cff` asks people to cite
  spektrafilm for the science. [read]
- Branches: `main` is the macOS product, `Windows_Linux` holds the Vulkan, Qt and Tauri work, plus
  about 18 codex/assist/ci branches. Every non-gh-pages branch has the same 34-profile set (28
  upstream, 5 product, 1 alias), or an earlier subset. The branch `rfc-022-solved-neutral-profiles`
  (the neutral-pack solver, `96d083bb`) is cited in the PRD but **not pushed**. [read]

## 2. Papers and print media

**Colour papers and print films:** the same 8 as upstream main: kodak_portra_endura,
kodak_endura_premier, kodak_supra_endura, kodak_ultra_endura, kodak_ektacolor_edge,
fujifilm_crystal_archive_typeii, kodak_2383, kodak_2393. **No new colour paper**, and no kodak_2302
(Negs has it). The only change is `data.edr_tone_map` on all 8 (`d04eccf`, 2026-09-17): a 256-sample
log2-Y map fitted on Portra 400, used for HDR display. The baking tool and the evidence were left on
an unpublished `experiment/edr` branch (`26eab11`). [read]

**B&W paper: `ilford_multigrade_iv_rc`, "Ilford Multigrade IV RC"** (commit `4599179`, 2026-10-07,
version "feasibility-1"):
- Source: HARMAN "MULTIGRADE RC PAPERS Technical Information", June 2019
  (MULTIGRADE-IV-RC-Papers-060619.pdf). It uses the characteristic curves for filters 00-5 (raster
  charts, p3), the ISO R/P tables (p2) and the spectral sensitivity chart (p1, no ordinate scale).
  Negs' grade-2 profile used the October 2020 J20 edition instead.
- Construction (`metadata.provenance`):
  - `density_curves_model`: FITTED. Three emulsions, one per channel, each a sum of three normal
    CDFs (`model_type: "cdfs"`, centres, amplitudes and sigmas in the file).
  - `log_sensitivity`: the sheet's single curve, read as linear relative sensitivity (ASSUMED), and
    split into a shared blue part plus a green part scaled per emulsion. The green speeds are FITTED:
    `green_speed_log10 = [0, -0.761, -1.728]`, `blue_edge_nm = 465.4`, `blue_edge_width_nm = 6.64`.
  - `channel_density`: ASSUMED flat 1.0 in all three channels (neutral silver).
  - `base_density`: ASSUMED flat 0.06.
  - Total `dmax_over_base` = 2.22.
  - `log_exposure` origin: chosen so that a Tri-X mid-grey negative printed at filter 2 lands on
    Y = 0.184.
  - info: `channel_model: "bw"`, `reference_illuminant: "TH-KG3"`,
    `log_sensitivity_density_over_min: 0.6`, `antihalation: "strong"`.
- **Variable contrast** is handled with the existing colour-enlarger filters, with no engine code:
  `engine/resources_product/paper_grades.json`. The engine never reads this file; the app sends the
  pack.

  | Grade | C | M | Y | print_exposure |
  |---|---|---|---|---|
  | 00 | 0 | 0 | 199.0 | 0.5323 |
  | 0 | 0 | 0 | 137.9 | 0.6642 |
  | 1 | 0 | 0 | 99.8 | 0.8257 |
  | 2 | 0 | 0 | 68.0 | 1.0 |
  | 3 | 0 | 0 | 34.1 | 1.159 |
  | 4 | 0 | 39.1 | 0 | 1.334 |
  | 5 | 0 | 151.7 | 0 | 1.3507 |

  There are no half grades ("not on the sheet"). The pack was fitted on Tri-X, with print exposure
  set so mid-grey holds. `bw_checks.py` shows the same table prints T-Max, Acros II and HP5 mid-grey
  equally. `engine/resources_product/neutral_print_filters.json` has the grade-2 row for each of the
  four films. [read]
- How it fits the engine: `channel_model` is parsed (`engine/src/core/profile.cpp:60`) but nothing
  uses it. B&W runs through the ordinary 3-channel path, with films writing one emulsion into all
  three channels (each channel_density 1/3). [read]
- The app (since 1.3.3, 2026-10-09) always sends grade 2 (`FilmParams.multigradeFilter2 = (0,0,68)`,
  `Params.swift:590`). The other grades are in the file for a Grade control that has not been
  built. [read]
- Validation: `engine/tests/bw_checks.py`. Checks: the print is neutral (channel spread < 2e-3);
  filter 2 prints 18 % grey at displayed density 0.55-0.95; ramp contrast span 00 < 2 < 5 with
  5 > 1.3 × 00; lens-filter behaviour; the edge print. [read]
- Multiple grades, yes; fibre-based or other paper brands, no. [read]

**B&W films** (commit `4599179` for Tri-X; `c0f8c7e` for the rest; all `channel_model: "bw"`,
`target_print: ilford_multigrade_iv_rc`, `antihalation: "weak"`):

| Id | Source | Development |
|---|---|---|
| kodak_tri_x_400 | Kodak F-4017 (Feb 2016), PDF vector paths p7/p8 | D-76, CI 0.56 (interpolated between 6 and 8 min) |
| kodak_tmax_100 | Kodak F-4016 (June 2018), vector p8 | D-76 small tank, 6.2 min, CI 0.56 |
| fujifilm_neopan_acros_100_ii | FUJIFILM AF3-0258E, raster traced at 600 dpi, 5400 K black body divided out | D-76 7¼ min, CI 0.57 |
| ilford_hp5_plus_400 | HARMAN HP5 PLUS (Nov 2018), raster traced at 600 dpi; the 2850 K spectrogram used "as drawn" (the reading that reproduces Ilford's filter factors) | ILFOTEC HC 1+31, 6½ min, CI 0.60 |

Each profile tags every field SHEET, ASSUMED or MEASURED in `metadata.tags`. Common assumptions: a
flat toe, a straight line into a gross-D 3.0 ceiling, a log-linear red tail and flat base density.
Negs already has all four (`kodak_trix400_d76_8min` / `kodak_trix`, `kodak_tmax_100`,
`ilford_hp5_plus`, `fujifilm_acros_100_ii`), from the same datasheets. I compared gradients on
channel 0 (G from D 0.1 to 1.3 net) [read, my own quick script]:
- Tri-X: 0.58 vs Negs `kodak_trix400_d76_8min` 0.58.
- T-Max: 0.56 vs 0.55.
- HP5: 0.60 vs 0.60.
- Acros II: 0.60 vs 0.52 (Negs uses a different development time).

So the films add nothing new.

**Alias profile:** `cinestill_800t.json` is just `{"derived_from": "kodak_vision3_500t", info: {name,
use: still, antihalation: no}}`, merged over the base when loaded (commit `089e335`). Negs has its own
CineStill profiles. [read]

The digitising and fitting tools, and the write-ups (`research/black-and-white-tri-x.md`,
`research/black-and-white-three-more-films.md`, `research/colour-film-granularity.md`,
`research/bw-films/grain/`), are **git-ignored and not published** (AGENTS.md:345-348). The profiles
cannot be rebuilt from the repo. [read]

## 3. Grain

- **Model:** upstream main's pre-preset Poisson/binomial sub-layer model, ported to Metal
  (`engine/src/shaders/grain.metal`, `spk_common.h:184 layer_draw`). One exact draw,
  `Poisson((n/sat)·p) · od·sat`, with Philox keyed on (pixel, stream, seed), so it is the same
  whether the frame is tiled or not. Kernels: `spk_grain_layers` (fused, 3 channels × 3 sub-layers),
  `spk_grain_simple`, `spk_grain_layer_one` (used when the dye-cloud blur sigma exceeds 0.4 px, e.g.
  at small `film_format_mm`). Vulkan versions are in `engine/src/shaders/vulkan/spk_grain_*.comp`.
  [read]
- **No per-stock presets and no `rms_granularity`.** The only per-stock grain is the app-side
  multiplier `grain_amount`. It is applied in density: `in + amount·(grained − in)`
  (`params.hpp:44`, "RFC-025", commit `2872b3c`), with wire range 0-2. [read]
- **Per-film B&W amounts** (`Params.swift:604`, commit `d610ac9`, 2026-10-09): Tri-X 1.70,
  HP5 Plus 1.49, T-Max 0.74, Acros II 0.64; colour films 1. B&W films always render grain as one
  layer (`grain_sublayers_active` forced false; `Params.swift:761`). How they were measured
  (doc-comment): engine renders at 6 µm pixels, one layer, net density 1.0, 48 µm aperture. At
  amount 1, all four render RMS 10-11. The targets are 17 / 8 / 7 (Acros "in Microfine: a floor") /
  16. Before this, Tri-X rendered RMS ~7 against the sheet's 17 (AGENTS.md:345). The measurement
  scripts (`research/bw-films/grain/`) are not published. [read]
- Comparison with Negs and upstream `28bf883:src/spektrafilm/data/presets/grain.toml` (103 lines;
  Negs' `data/presets/grain.toml` is that file plus a header and a Negs block):
  - Upstream's `[defaults.bw.negative]` is rms 14 (Double-X), uniformity 0.6, density_min 0.01,
    sublayers [1, 0.4, 0.25].
  - SpektraLab's published targets for Tri-X (17), T-Max (8), Acros II (7) and HP5 (16) match the
    Fotufilm `grainStrength` × 1000 values listed in `docs/handoff-fotufilm-grain.md` exactly.
    [read; the match is my comparison]
  - SpektraLab has **no colour granularity data**. The PRD says "grain has no per-film data, and the
    repo's rule is to inherit honestly or measure" (`PRD/PRD-v1.4.0.md` §4.2). A colour-granularity
    write-up is mentioned (commit `4599179`) but is git-ignored.
- **Lessons recorded in AGENTS.md:**
  - Trap 8: `fast_stats` keeps RMS within 0.14 % but sets skewness to zero. Skewness is about 1/√μ:
    +0.165 in the shadows, +0.022 in the highlights. Matching the second moment is not enough.
  - Trap 9: the draws are i.i.d., so chunking them makes no seam; only the blurs need about 4 px
    halos.
  - Trap 1: grain is deterministic, seeded [0, 1, 2]; glare is the only unseeded stage in the
    reference.
- **Grain checking:** `engine/tests/parity_grain.py` (moments per density level, described in the
  summary). The early commit `a2fcb15` describes a "stochastic" comparison mode (moment match +
  radially averaged PSD). Its `compare.py` was lost in the history rewrite. [read]
- **Physical scale:** the grain scale follows `film_format_mm`, clamped to 4-200 mm. An optional
  setting, "recalculate effects after crop", maps a crop to a bigger virtual format (`13ecce8`).
  Off by default, "the physically true answer". [read]
- Conclusion [inferred]: on the grain *model*, Negs (with upstream's June 2026 presets) is ahead.
  What is worth taking is the targets, the measuring method (48 µm, net D 1.0, one layer) and the
  distribution checks, not their numbers.

## 4. Other engine, model and tooling changes

- **CineStill back-reflection halation** (detail in the summary, item 3). Files:
  `engine/src/core/halation_psf.hpp` (fitted tables), `engine/tests/halation_psf_fit.py` (the
  integration and constrained fit: non-negative weights, sum = 1, Σ|w| ≤ 2, sigmas at least 1.35×
  apart, so the IIR blur stays stable), `engine/tests/cinestill_checks.py` (8 checks, including a
  red/green half-height reach of 1.49-2.65 measured on 5 lights in scans). API-SPEC §16. It applies
  only to profiles tagged `antihalation: no`; `antihalation_removed` keeps the Gaussian preset
  0.30 / 0.10 / 0.015. A Metal kernel `spk_highlight_headroom` and node `node_headroom` run before
  overscan. Commits `389ecc0`, `342c974`, `bdef67d`. [read]
- **An anti-halation layer switch and a highlight boost**, `8c0f555`: `HalationParams.boost_ev`,
  `boost_range`, `protect_ev`. [read; not studied in detail]
- **Lens colour filters**, `camera_filter`: summary item 5. [read]
- **Effect strengths:** "RFC-025" `grain_amount`, glare and scatter strengths; "RFC-034" makes them
  area multipliers (`082f7ac`). [read]
- **"Virtual contrast mask"**, "RFC-024" (API-SPEC §11): an unsharp-mask gain field on the print
  exposure, `P·[2^δ·A + F]`, a darkroom contrast mask. [read]
- **Scene latitude**, "RFC-023" (API-SPEC §12): a highlight/shadow roll-off before the film, plus a
  latitude probe that renders a neutral ramp through film and paper and reads it with ISO 6846
  criteria. [read]
- **Preflash** (`printing.cpp:187`) and filter shifts: the preflash is upstream's. `filter_shift_scale`
  (API-SPEC §14) records that upstream's ±1 Y/M shift is only ±1 CC (barely visible); the app sends
  ×40. [read] Worth checking whether Negs' enlarger shift sliders have the same problem
  [inferred].
- **Digital intermediate**, "RFC-028" (`engine/src/core/digital_intermediate.cpp`): a 3×3 matrix
  fitted only on spectra inside Pointer's gamut, plus an optional blue/violet hue fix. PRD §5 records
  that Hanatos upsampling puts 44 % of a ColorChecker blue's energy below 430 nm (11 % in the real
  paint). Not directly relevant [inferred]: Negs' pin uses upstream's later upsampling.
- **Film edge and date back** (`engine/src/pipeline/overscan.cpp`, `kEdgeLooks[]`): per-stock edge
  prints measured on real strips, including Ilford's own 135 layout and HP5's DX code 1753. Negs has
  its own (`spektrafilm-io/src/border.rs`). [read]
- **Validation tooling:** the parity harnesses `engine/tests/parity_{render,setup,schema,lut,grain,
  exposure,session}.py` check against the Python reference (27 render cases). There are also
  `bw_checks.py`, `cinestill_checks.py` and `overscan_checks.py`. They run the engine through
  ctypes, so we cannot use them directly; their check lists are good templates. [read]
- No changes to couplers, the scanner model or spectral upsampling beyond the port. [read, from git
  log subjects]

## 5. Licences

- **Code:** GPL-3.0-or-later (README "Credits and licence"; `LICENSE.txt` at the root added
  `c1447c6` 2026-10-29; `license/LICENSE`; `CITATION.cff`). Negs is GPL-3.0-only (`Cargo.toml:19`),
  so porting their code (halation PSF fit, camera-filter normalisation, test logic) is allowed,
  with credit to Hanze Qiu / SpektraLab. [read; compatibility inferred: "or-later" code can be
  used under GPL-3.0]
- **Upstream profiles and LUTs:** CC BY-SA 4.0 (`license/SPEKTRAFILM_LICENSE.txt`, preamble v1.1
  2026-05-28). Their bundled `Profiles-and-LUTs-CHANGELOG.txt` says the profiles are "UNMODIFIED",
  yet `d04eccf` added `edr_tone_map` to the 8 papers. Their changelog is stale. That is their
  compliance problem, not ours. [read; the discrepancy is my reading]
- **Their own data (`engine/resources_product/`: 4 B&W films, Multigrade IV RC, `paper_grades.json`,
  neutral rows): no explicit licence.**
  - Each profile's metadata says "SpektraLab product data. Not a spektrafilm profile", plus "Built
    from <datasheet>. See NOTICE.md". NOTICE.md says "These files are SpektraLab's, not spektrafilm's"
    and names no licence.
  - The README's licence table lists "film and paper profiles, print LUTs: CC BY-SA 4.0" right
    after crediting "4 new measured B&W profiles ... by Hanze Qiu". That could be read as covering
    them.
  - The bundled `Resources/Licenses/README.txt` limits CC BY-SA to "The 28 ... profiles ... by
    Andrea Volpato".
  - Without a statement, the repository's GPL-3.0-or-later is the most likely default.
  - [read; the conclusion is inferred and **ambiguous**]
- Consequence [inferred]: Negs keeps its profile data under CC BY-SA 4.0. Copying the Multigrade
  JSON or `paper_grades.json` into `data/` would need the author to confirm CC BY-SA 4.0. GPL-only
  data does not fit a CC BY-SA data folder. The datasheet *facts* (RMS figures, grade ranges, the
  datasheet editions) are free to use. Re-fitting the paper ourselves from the HARMAN PDF, using
  their published method (summary item 2), avoids the question entirely.
- The Wratten tables are third-hand (Kodak B-3, transcribed by Repacholi in 1992), and their status
  is unclear. Better to take them from a primary source if we want them.
- Trademark: "SpektraLab" name and icon (Guangzhou Songzi Intelligent Technology Services Co.,
  Ltd.). This does not affect reuse of code or data. [read]

## Open questions

1. Will Hanze Qiu confirm a licence for `engine/resources_product/` (CC BY-SA 4.0 like the README
   table suggests, or GPL)? Would they publish `research/` (digitising, grain measurements,
   colour-granularity write-up)?
2. Can Negs' one-channel B&W films expose a three-channel neutral paper (and a one-channel
   paper)? This needs checking before planning a variable-contrast paper. [not checked]
3. Does `ilford_mgrc_deluxe_grade2`'s curve sit near SpektraLab's grade 2 (Y68)? They were fitted
   from different sheet editions (2019 vs 2020) and different curve models. A quick render
   comparison would show it.
4. HP5's 16 comes from Ilford's motion-picture sheet (via SpektraLab and Fotufilm). Is there a
   still-film figure, and is Acros II's 7 a "floor" ("in Microfine")? Both matter for `grain.toml`.
5. The grain handoff suggests B&W might want sub-layers off, as SpektraLab forces. Does upstream
   `28bf883` treat B&W sub-layers the same way, and what does the owner prefer?
6. Neutral filter solver "RFC-022" (branch `rfc-022-solved-neutral-profiles`, `96d083bb`): not
   pushed. Its reported results are 71/128 pairs solved; Ultra Endura, 2383 and 2393 solve 0/16.
   Is it worth asking for, given Negs' 24 new colour films need neutral packs too?
7. Is the CineStill halation model's fit to scans good enough to adopt, or should Negs fit its own
   constants to the owner's CineStill scans?
