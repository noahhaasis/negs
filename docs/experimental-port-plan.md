# Experimental port plan (ticket 16, phase 0 audit)

Written 2026-10-25. **Status: awaiting Noah's approval; nothing is ported yet.**

Goal: make the Negs engine behave like Python spektrafilm `experimental` at
`28bf883e1672e884307edc75852549376e13644e` (8 July 2026) instead of main `3bb2c2d`.
`git fetch` on 2026-10-25 confirmed that `origin/experimental` is still at `28bf883`, and
that it is the newest commit on any upstream branch.

The audit covers `git log 3bb2c2d..28bf883` (69 commits). The dev branch was merged
twice, so several commits appear under two hashes (for example `85717fb` = `2323a67`,
`ffbee22` = `6c6efdb`, `437c8e6` = `b7990c0`). Both forms are in range. This doc cites
the hash that the ticket uses.

## How this was measured

- Clean reference trees came from `git archive` of `28bf883` and `3bb2c2d`, and the
  Python environment ran with `-I`. The local `spektrafilm` checkout was not touched.
- The source of every engine file in `src/spektrafilm/` was diffed. Profiles, presets and
  LUT data were compared numerically against `data/` in Negs.
- **Ablation:** the experimental pipeline rendered a synthetic 288×192 linear-sRGB test
  image (hue × saturation field, grey ramp, one 8× highlight spot) with its defaults,
  except that grain and glare were off (determinism) and in/out were linear sRGB. Each
  row of the tables below reverts **one** item to what Negs does today and reports the
  change against the experimental render. ΔE00 is computed on the linear output. "8-bit"
  means the difference in sRGB-encoded 8-bit codes. Rows don't add up; the items
  interact.
- **Validation of the ablation:** reverting every item at once reproduces today's Rust
  f64 CLI (`target/release/spektrafilm-f64`, CPU backend, same params) to **0 8-bit
  steps (ΔE00 ≤ 0.01)** for Portra 400 → Portra Endura, Gold 200 → Crystal Archive II,
  Provia 100F scan and Double-X → 2302. Vision3 500T → 2383 reproduces only after one
  more revert, the illuminant fallback described in finding 1 (then ≤ 0.07 8-bit steps).
  So the inventory below accounts for every deterministic difference on these routes.
- The tools and outputs are in the session scratchpad and were not committed:
  `ablate.py`, `rust_vs_ablation.py`, `grain_cmp.py`, `cmp_profiles.py`. Phase 1 will turn
  the useful parts into `scripts/parity/` tooling.

## Findings that change the ticket's inventory

1. **Pre-existing Negs bug, not experimental: illuminants `T` and `K75P` are missing.**
   `select_illuminant` / `select_illuminant_f64` (`stages/filming.rs`,
   `stages/scanning.rs`, `pipeline.rs`, `color_reference.rs`) and
   `enlarger_filtered_illuminant_f64` only know D50/D55/D65/TH-KG3, and silently fall back
   to D55 (reference) or D50 (viewing). Python maps `T` to colour's "Incandescent" and
   `K75P` to "Kinoton 75P" (`model/illuminants.py:32`, the same on main). Affected:
   Vision3 500T and 200T (reference `T`), and every print on 2383/2393 (viewing `K75P`).
   On 500T → 2383 this alone costs **mean ΔE00 18.5 (p99 35, max 103 8-bit steps)**. That
   is far more than all the experimental changes together (mean 1.45). Upstream main has
   the same illuminants, so Negs doesn't match main on cine either. The parity audit
   didn't catch it because it only covered Portra/Gold and B&W.
2. **Experimental's default upsampler is `hanatos2025`, not `arctic2026beta04`.**
   `a1c0bb0` made beta04 the default, and `0f690da` (5 July) reverted the schema default
   to `hanatos2025` (`runtime/params_schema.py:369`). beta04 is first in the GUI's
   dropdown but isn't the default. Porting beta04 is therefore an option, not a default
   change. The hanatos2025 LUT itself is byte-identical (only renamed
   `hanatos2025_irradiance_xy_tc.npy`).
3. **The default output gamut compressor is OkLCh, not ACES RGC.** `85717fb` made
   `aces_rgc` the default; `5535ac5`/`67b47b4` (the same day) switched it to `oklch`; and
   `4260c7b` set knee and lightness to `(0.95, 1, 1.6)` and `hull_detail` 11 → 5. Those are
   exactly the values in Negs' opt-in `data/presets/experimental-gamut.json`. Item 6 is
   therefore a default flip, and Negs already has the algorithms (including `aces_rgc`)
   on CPU and GPU.
4. **Halation presets (item 4) are on main too.** The `(use, antihalation)` table is
   identical on main and experimental. Negs never applied it, so Negs doesn't match main
   with halation on either. The parity audit ran with halation off.
5. **Negs grain doesn't match main's default either.** Main defaulted to the layered path
   (`sublayers_active=True`). Negs ports only the single-layer `apply_grain_to_density`.
6. **B&W profiles already match `28bf883`.** `kodak_2302`, `kodak_doublex` and
   `kodak_trix` in Negs are identical to experimental. All 28 colour profiles in Negs are
   identical to **main**. The Negs-only stocks (`kodak_trix400_d76_8min`,
   `ilford_mgrc_deluxe_grade2`) have Gaussian `norm_cdfs` models, so they work with
   model-derived curves. No shipped experimental profile uses the skewed
   `sept_norm_cdfs` model or `alphas`.
7. **No default output change:** film/print base tuning (identity at defaults), the
   camera colour filter (`"none"`), `print_balance.py` (a GUI action), workflows, the
   `exposure_factor` move (`1d7e2e4`, a refactor), the halation in-place rewrite, the
   n-channel LUT generalisations, and the DWG/V-Log entries (GUI enum only).

## Impact summary

Change against experimental when only that item is reverted to Negs' behaviour. Values
are mean / p99 ΔE00 and max 8-bit steps.

| Item | Portra 400 → Endura | Gold 200 → CA II | 500T → 2383 ¹ | Provia 100F scan | Double-X → 2302 |
|---|---|---|---|---|---|
| 1 film curves: stored instead of model | 0.24 / 0.65 / 7 | 0.22 / 0.63 / 18 | 0.24 / 0.59 / 4 | 0.13 / 0.36 / 3 | 0 |
| 9 profile data: main instead of exp | 1.52 / 2.85 / 21 | 2.66 / 5.84 / 39 | 1.59 / 3.88 / 37 | 0.02 / 0.06 / 0.3 | 0 |
| 2 DIR gammas: main instead of presets | 0.90 / 3.02 / 32 | 0.71 / 2.06 / 31 | 0.93 / 2.64 / 35 | 2.66 / 9.25 / 50 | 0.00 / 0.02 / 2 |
| 2 DIR Langmuir off (linear) | 1.90 / 5.59 / 42 | 2.60 / 7.42 / 47 | 1.60 / 3.48 / 42 | 4.70 / 13.95 / 86 | 0.00 / 0.03 / 2 |
| 2 DIR tail weight 0.06 instead of 0.03 | 0.01 / 0.22 / 11 | 0.01 / 0.19 / 6 | 0.01 / 0.15 / 6 | 0.01 / 0.11 / 3 | 0.00 / 0.06 / 3 |
| 4 halation: no preset | 0.00 / 0.09 / 17 ² | 0.00 / 0.05 / 62 ² | 0.01 / 0.13 / 54 ² | 0.01 / 0.10 / 53 ² | 0.01 / 0.07 / 23 ² |
| 5 white balance: main normalisation | 2.12 / 3.68 / 28 | 1.16 / 2.53 / 27 | 1.77 / 3.56 / 23 | 0.54 / 0.92 / 11 | 0 |
| 5 neutral print filters: main database | 0.97 / 1.96 / 10 | 2.64 / 5.53 / 14 | 1.88 / 3.61 / 15 | n/a | 0 |
| 6 gamut: main defaults | 0.88 / 2.99 / 49 | 0.85 / 2.66 / 48 | 1.03 / 4.67 / 42 | 0.80 / 3.34 / 37 | 0.02 / 0.13 / 21 |
| 8 auto-exposure: RGB metering | 0.19 / 0.26 / 4 | 0.15 / 0.21 / 6 | 0.33 / 0.43 / 3 | 0.58 / 0.76 / 5 | 0.12 / 0.14 / 1 |
| **All of the above (= Rust today)** | **1.88 / 7.36 / 47** | **1.98 / 5.26 / 62** | **1.45 / 4.31 / 64** | **2.90 / 10.21 / 47** | **0.14 / 0.27 / 33** |
| 0 illuminants `T`/`K75P` (pre-existing) | — | — | **18.5 / 35 / 103** | — | — |

¹ 500T with correct illuminants; the last row is the extra error of today's fallback.
² Halation only moves pixels around the highlight spot (max ΔE00 4.8–8.3). On real
photographs with large highlights it shows more.

Grain is off in all of the above. Measured separately on flat patches at 6 µm pixels, the
48 µm-aperture RMS at net density 1.0 is:

| Stock | Experimental (R, G, B) | Negs today (R, G, B) |
|---|---|---|
| Portra 400 | 2.9, 2.7, 2.9 | 11.2, 10.1, 16.9 |
| Provia 100F | 4.9, 5.2, 5.6 | 13.8, 13.7, 16.6 |
| Vision3 500T | 6.4, 8.0, 16.9 | 9.6, 9.1, 15.1 |

So grain changes the most visibly: Negs' grain is 3–4× stronger on most still stocks,
with a different spectrum (dye-cloud blur 2 µm, multiplicative USM).

## Inventory

Python references are to `src/spektrafilm/` at `28bf883`. Rust references are to this
repo. The GPU column is the wgpu backend (`crates/spektrafilm-gpu/src/wgpu_backend.rs`,
`crates/spektrafilm-shaders/wgsl/`); the CUDA backend mirrors it.

### 0. Illuminants `T` and `K75P` (pre-existing gap)

- **Upstream:** `model/illuminants.py:32`, on main and experimental. Experimental also
  accepts any `colour.SDS_ILLUMINANTS` / `SDS_LIGHT_SOURCES` name and `BB<temp>`, and raises
  on unknown names. That is needed later for `convert.scan_illuminant` (item 11).
- **Rust:** add incandescent and Kinoton 75P spectra (tabulated from colour-science
  0.4.7, CC BY-SA data noted) to `spektrafilm-math/src/spectral.rs`. Replace the four
  `select_illuminant*` copies and the enlarger lookup with one resolver that errors on
  unknown names. GPU: none; spectra are baked on the CPU.
- **Default output:** changes every render with a Vision3 200T/500T film or a 2383/2393
  print, by mean ΔE00 ~18.
- **UI:** none.
- **Risk:** low. This is a bug fix and could ship on its own against main.

### 1. Density curves from the fitted model, always

- **Commits:** `4f3b186`/`3f5975c` (model instead of curves), `6cd00c8` (wider gamma
  range), `1986503` (refits).
- **Upstream:** `runtime/pipeline.py:52-83` samples `density_curves_model` onto each
  profile's `log_exposure` (`model/density_curves.py:142`), collapses B&W families
  (`:165`), then bakes `film_render.chemistry` with
  `apply_print_curves_morph_with_layers` (`utils/morph_curves.py:390`), which also
  produces `density_curves_layers`. `density_curve_gamma` is gone for film and print. Both
  chemistry dataclasses default to `active=True`, and the morph at identity reproduces the
  fitted curves. Profiles without a model fall back to stored curves in printing only
  (`stages/printing.py:66`).
- **Rust:** `pipeline.rs` (`apply_film_curves_morph`, `film_stock_density_curves`),
  `print_morph.rs` (already evaluates the model; needs the per-layer output),
  `stages/filming.rs` / `stages/printing.rs` / `enlarger.rs` / `color_reference.rs` (remove
  `density_curve_gamma`), `params.rs`. GPU: none; curves are uploaded as tables.
- **Default output:** film mean ΔE00 0.13–0.24, max 18 8-bit steps (with experimental's
  profiles). The print already uses the model.
- **UI:** none new. Film Curves already exists; the "inactive keeps stored curves"
  exception goes away as planned.
- **Risk:** low.

### 2. DIR couplers

- **Commits:** `11e8b90` (presets with cine branches), `2ea9724` (Langmuir), `67186e0`
  (a22 refit; tail weight 0.03).
- **Upstream:** `data/presets/couplers.toml` (77 lines: defaults per
  `color|bw × positive|negative`, `cine.daylight|tungsten` for colour negatives, and
  overrides for Velvia, Ektachrome, Kodachrome and Provia), applied by
  `runtime/params_builder.py:263`. Langmuir donor saturation for negatives and
  receiver-side saturation for positives are in `model/couplers.py:11-139` and `:231`;
  the defaults `langmuir_*_k_rgb = (1, 1, 1)` are active. The old
  `high_exposure_couplers_shift` term is removed. The matrix is n_ch-aware
  (`:143`), which Negs already emulates for B&W.
- **Rust:** `pipeline.rs::apply_film_specific_params` (replace ticket 14's block and the
  type defaults with a TOML loader; new `data/presets/couplers.toml` with attribution);
  `spektrafilm-model/src/couplers.rs` (`compute_curves_before_dir`,
  `compute_exposure_correction`); `params.rs` (two new `[f64; 3]` fields, tail 0.03).
  GPU: `dir_matmul.wgsl` needs the donor nonlinearity before the matmul, and the receiver
  term needs a per-pixel pass after the diffusion blur. The inversion table stays on the
  CPU.
- **Default output:** the largest colour change. Langmuir alone is mean ΔE00 1.6–2.6 on
  negatives and 4.7 on Provia; the presets add 0.7–2.7.
- **UI:** experimental exposes `langmuir_donor_k_rgb` and `langmuir_receiver_k_rgb`. I
  suggest a follow-up ticket rather than the inspector now.
- **Risk:** medium. The cine branch keys off `reference_illuminant` starting with "T",
  which needs item 0's names. A GPU perf check is needed for the extra pass.

### 3. Grain

- **Commits:** `437c8e6` (RMS granularity), `a9299ce`/`8cbfb01` (presets), `ffbee22`
  (multiplicative USM), `b3e65de` (less ringing: blur 0.89, USM 0.7/1.5), `6d50ccd`
  (`use_fast_stats=True`).
- **Upstream:** `model/grain.py:44-338`. The model is always multilayer, one
  Poisson-binomial per sub-layer from `density_curves_layers` (3 sub-layers × 3
  channels). Particle area comes from the RMS target via the realised peak over the
  curves (`_coarsest_area_from_curves`, `:171`). It then applies dye-cloud blur 2 µm per
  sub-layer, lognormal micro-structure, pixel blur 0.89, and the mass-conserving
  multiplicative unsharp mask (`model/diffusion.py:24`). Presets are in
  `data/presets/grain.toml`, applied by `params_builder.py:237`. Work is in float32.
- **Rust:** `spektrafilm-model/src/grain.rs` (rewrite), `stages/filming.rs::develop`,
  `pipeline.rs` (GPU grain params, `npp` computation), `params.rs` (`GrainParams` fields
  renamed). GPU: `grain.wgsl` plus new sub-layer interpolation, USM and per-channel sum
  reduction (`max_reduce.wgsl` pattern) passes. CPU memory: one channel slab at a time,
  as upstream does.
- **Default output:** large, as in the grain table above. It can't be compared pixel by
  pixel: with `use_fast_stats=True` Python draws from numba's per-thread RNG, so the
  stream can't be reproduced. Even with it off, the RNG order changes.
- **UI:** the inspector's grain section changes (particle area/scale and sub-layer toggle
  become RMS granularity, sub-layer scale, USM sigma/amount). macOS `Inspector.swift` and
  Windows `state.rs` use the old fields.
- **Risk:** high. Parity can only be statistical: per-channel RMS at several densities,
  Wiener-spectrum shape, and mean preservation against Python-generated statistics in a
  frozen fixture. The GPU grain cost rises roughly 3× (9 draws per pixel instead of 3) plus
  a reduction. The mass renormalisation is image-global, so preview and export agree only
  if both see the whole frame. They do today, but a tiled preview would break it.

### 4. Halation presets

- **Upstream:** `runtime/params_builder.py:196-231` (unchanged from main). Sets
  `halation_first_sigma_um` (65 still / 50 cine) and `halation_strength` from
  `(info.use, info.antihalation)`.
- **Rust:** `pipeline.rs::apply_film_specific_params`; profiles already carry `use` and
  `antihalation` (`profile.rs:41`). GPU: none.
- **Default output:** only near highlights. Max ΔE00 5–8 at the spot. `still/strong` is
  3× weaker than today's default, while `still/weak` (Gold, Ultramax, C200, X-Tra,
  Kodachrome) is 1.6× stronger.
- **UI:** none.
- **Risk:** low. This is a quick win.

### 5. Spectral upsampling and white balance

- **Commits:** `e301791` (per-channel normalisation), `4599f63` (B&W), `06e778c` (LUT
  midgray), `1986503` (stock neutralisation: new `neutral_print_filters.json`), `a1c0bb0`
  and `0f690da` (beta04 options), `f4ffe5c`…`d8933a9` (arctic versions), `02a1613` (otsu,
  jakob).
- **Upstream:** the hanatos window normalisation now references the LUT's reconstructed
  white instead of the bare illuminant (`utils/spectral_upsampling.py:373`). Reflectance
  methods normalise per channel against the method's own neutral (`:533`). The
  `rgb_to_raw` dispatcher is at `:626`. The neutral filter database changed for every
  stock, and missing combinations fall back to C0 M50 Y50 instead of the schema's 0/65/55
  (`params_builder.py:39`).
- **Rust:** `spectral_service.rs` (tc LUT build, window normalisation), `mallett.rs` (per
  channel), `pipeline.rs` (arctic/reflectance dispatch), `neutral_filters.rs`,
  `data/filters/neutral_print_filters.json`. The Negs-only stocks keep their entries,
  re-derived with the 28bf883 code. GPU: none; the LUT is built on the CPU.
- **Default output:** mean ΔE00 0.5–2.1 from white balance and 1–2.6 from the filter
  database.
- **UI:** none for defaults. `arctic2026beta04` (and later jakob/otsu) can replace the
  opt-in `arctic2026alpha02`, which upstream deleted.
- **Risk:** low to medium. The LUT data needs attribution, and each extra method is a
  6 MB `.npy`.

### 6. Gamut compression defaults

- **Commits:** `85717fb`, `5535ac5`, `8e54f3b` (inscribed hull, D65 centre), `4260c7b`
  (knees).
- **Upstream:** `utils/gamut_compression.py:41-250`. The input defaults are `xy`,
  `inscribed_hull`, detail 5, knee `(0.815, 1, 1.2)`, centre D65 (hard-coded at
  `spectral_upsampling.py:373`). The output defaults are `oklch`, knee
  `(0.95, 1, 1.6)`, lightness `(0.95, 1, 1.6)`.
- **Rust:** `params.rs` defaults only. Algorithms exist on CPU and GPU
  (`gamut_compression.rs`, `input_gamut.rs`, `gamut_compress.wgsl`).
  `experimental-gamut.json` becomes redundant; keep it or add a `main-gamut.json`
  preset for the old look.
- **Default output:** mean ΔE00 ~0.9, but p99 33 8-bit steps on saturated colours.
- **UI:** the inspector's gamut choices default to the new values.
- **Risk:** low.

### 7. Film and print bases, print balance

- **Commits:** `86020fe` (print base), `df3cda6` (film base).
- **Upstream:** `FilmBaseParams` and `PrintBaseParams` (`params_schema.py:159,172`);
  `model/develop.py:22-90`. Applied in the print exposure, the film scan and the print
  scan. `runtime/print_balance.py:19` re-solves the M/Y filter shifts so midgray prints
  neutral (a GUI button).
- **Rust:** `stages/printing.rs`, `stages/scanning.rs`, `enlarger.rs`,
  `color_reference.rs`, and the GPU spectral shaders `print_spectral.wgsl` /
  `scan_spectral.wgsl` (the base is a spectral vector, so it can stay CPU-baked).
- **Default output:** none; identity at defaults.
- **UI:** new Film base and Print base groups, plus a "Balance print" action.
- **Risk:** low. Lower priority; it's mainly useful with film-convert.

### 8. Camera and front end

- **Commits:** `820de14` (Hoya filters; auto-exposure metering), `1d7e2e4`
  (`exposure_factor` to develop), `ffbee22` (resize stashes the full-frame preview),
  `8d4ff0f` (DWG/V-Log), diffusion float32 FFT (in the dev merge).
- **Upstream:** auto-exposure now meters **film raw** (mean of channels, target 1) on
  the full frame **before crop** (`stages/filming.py:79`, `utils/autoexposure.py:117`,
  `services/resize.py:42`). The print-balance reference applies the same scale
  (`filming.py:163`). `camera.color_filter` multiplies sensitivity by Hoya transmittance
  (`model/color_filters.py:184`, data in `data/filters/colored/hoya/`) and replaces
  `filter_uv`/`filter_ir`, which Negs already ignores. The enlarger/camera diffusion
  filter FFT now runs in float32 (`model/diffusion.py:664`).
- **Rust:** `stages/filming.rs::expose` / `measure_autoexposure_ev`, `pipeline.rs`
  (meter before crop/resize), `params.rs`, new filter data. GPU: the auto-exposure scale
  is a scalar computed on the CPU, so none.
- **Default output:** auto-exposure mean ΔE00 0.12–0.58. Metering before the crop also
  stops the exposure from jumping when cropping, which suits Negs' preview-matches-export
  promise.
- **UI:** a camera filter picker. DWG and V-Log input spaces are optional.
- **Risk:** low, except the diffusion filter. Python's float32 FFT can't be matched at
  1e-6 by the f64 build. I suggest keeping f64 (accuracy over approximation) and
  documenting the exception; the filter is off by default.

### 9. Profiles

- **Commits:** `1986503` (refit and neutralisation of every stock; floats rounded to 6
  decimals), `6d7e799` (autogenerated enums), `b245e51`/`250108c` (loader). New: `alphas`,
  `development_time` keys, `norm_cdfs` naming (Negs already accepts both names).
- **Rust:** replace the 28 colour profiles in `data/profiles/` with 28bf883's, keeping
  CC BY-SA attribution and noting the version. `profile.rs` accepts `alphas: null`. Add
  `sept_norm_cdfs` only if a shipped profile needs it (none does now).
- **Default output:** mean ΔE00 1.5–2.7 on colour negatives; prints (2383 curves up to
  0.42 density) dominate. Slides are almost unchanged.
- **UI:** none.
- **Risk:** low. Has to land together with item 5's filter database, which was
  re-derived for these profiles.

### 10. LUTs: single bake with params

- **Commits:** `28bf883`, `06e778c`.
- **Upstream:** `spektrafilm_lut_creator/` (bake, single, bundles, OCIO). In
  `digest_params(lut_mode)` (`params_builder.py:103-138`) the per-image trims (filter
  shifts, preflash), crop/upscale and the internal enlarger/scanner LUTs are forced to
  neutral/exact.
- **Rust:** `lut_export.rs::per_pixel_params` should adopt the same neutralisations
  (filter shifts and preflash are kept today). The rest of the LUT creator (bundles, OCIO,
  QA) is a separate feature.
- **Default output:** only the exported `.cube`.
- **Risk:** low.

### 11. Workflows and film-convert

- **Commits:** `df3cda6`, `ad90d5d`.
- **Upstream:** `workflow.route` (6 routes; `params_schema.py:275`), `ConvertingStage`
  (`runtime/stages/converting.py:10`, `model/convert.py`), `ConvertFilmParams`
  (scan illuminant, calibration matrix), scan-minus-base, and the "Detect base" and
  "Blind calibration" GUI actions. A new feature (inverting scanned negatives), not a
  behaviour change.
- **Scope:** **your decision.** See the questions below.

## Proposed sub-tickets and order

Pipeline order, with the things everything else depends on first. Each step follows
the ticket's per-step gates (f64 parity at 1e-6 or a documented exception, a frozen
Python fixture, GPU `gpu_vs_ref`, before/after renders, both precisions, macOS
build+tests, Windows check).

| # | Sub-ticket | Items | Default change | Size |
|---|---|---|---|---|
| 16.1 | Retarget the parity harness to 28bf883 (phase 1); add cine and slide cases and halation on; record the failure table | — | none | M |
| 16.2 | Illuminants `T`, `K75P` and a strict resolver | 0 | cine only, large | S |
| 16.3 | Experimental profiles and model-derived curves (with `density_curves_layers`); drop `density_curve_gamma` | 9, 1 | yes | M |
| 16.4 | White balance normalisation and the 28bf883 neutral print filter database | 5 | yes | M |
| 16.5 | Auto-exposure on film raw, full frame before crop | 8 (part) | small | S |
| 16.6 | DIR presets TOML, Langmuir donor/receiver (CPU and GPU), tail 0.03 | 2 | yes, largest colour | L |
| 16.7 | Halation presets | 4 | highlights | S |
| 16.8 | Gamut defaults to experimental | 6 | yes | S |
| 16.9 | Grain: RMS model, presets, layers, USM (CPU and GPU); statistical fixtures | 3 | yes, texture | XL |
| 16.10 | Docs (`upstream-sync.md`, `features-vs-upstream.md`) and golden manifests regenerated once on every machine | — | — | S |
| later | Camera colour filters; bases and print balance; arctic beta04/jakob/otsu options; LUT-mode neutralisations; Langmuir/RMS UI; film-convert | 7, 8, 5, 10, 11 | none | — |

Notes on the order:

- 16.2 is independent and fixes a real bug on today's baseline. It could go first, even
  before 16.1.
- 16.3 before 16.4: the new filter database was derived with the new profiles. Landing
  them separately shows each effect, but the intermediate state matches neither upstream
  commit. If you'd rather have fewer, more meaningful before/after reviews, 16.3 and 16.4
  can merge into one.
- 16.6 needs 16.2 (the cine tungsten branch keys off `T`) and 16.3 (Langmuir uses d_max
  of the model curves).
- 16.9 needs 16.3's `density_curves_layers` and is last because it can only be checked
  statistically.
- Each step changes macOS output, so each one needs your before/after approval. Golden
  manifests are regenerated once, in 16.10.

## Phase 1 harness changes (for 16.1)

`scripts/parity/check_parity.py` assumes main's API:

- `film_render.density_curve_gamma` / `print_render.density_curves_morph` →
  `chemistry` (frozen dataclasses; set with `dataclasses.replace`).
- `io.scan_film` → `workflow.route = "input > film > scan"` (the harness already
  branches on `hasattr(params, "workflow")`).
- Grain param names changed, but the harness only switches grain off.
- Add: halation on, DIR on (`--dir-couplers`), auto-exposure on (deterministic), and the
  pairs Vision3 500T → 2383, Provia scan and Double-X → 2302. Keep `--python-source` main
  runs working until 16.10.

## Questions for you

1. **Item 11 (workflows/film-convert):** in scope for ticket 16, a separate ticket, or
   out? My suggestion is a separate ticket. It doesn't change default output, and it adds
   the route model, the converting stage, calibration and two GUI actions.
2. **Illuminant fix (16.2):** OK to land it first, as a bug fix also on the current main
   baseline? It changes every Vision3 200T/500T and 2383/2393 render by a lot.
3. **Upsampling options:** Experimental's default stays `hanatos2025`. Should
   `arctic2026beta04` replace the opt-in `arctic2026alpha02` (upstream deleted alpha02),
   or should both be kept?
4. **Diffusion filter float32 FFT:** keep f64 and document a parity exception when the
   filter is on (my suggestion), or emulate float32?
5. **Grain acceptance:** statistical gates only (RMS per channel and density, spectrum,
   mean), since exact parity is impossible at experimental's defaults. OK?
6. **Review granularity:** one before/after review per sub-ticket as listed, or combine
   16.3 + 16.4 (data) and 16.7 + 16.8 (defaults)?
7. **Old look:** keep a `main-3bb2c2d` preset (old gamut defaults, etc.) for users who
   liked it, or drop main compatibility after 16.10?
