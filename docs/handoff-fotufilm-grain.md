# Handoff: grain for the Fotufilm-ported films

Written 2 November 2026, after the B&W port (commit `9c617b1` on `fotufilm-port`, worktree
`~/Work/Negs-fotufilm`, local only: not merged, not pushed). Read `docs/handoff-fotufilm-bw.md`
first for the port itself and how the owner works.

## Status (4 November 2026)

- Step 1 done for the five ports and Tri-X 400: a marked Negs-only block at the end of
  `data/presets/grain.toml` (Delta 19, Tri-X 17, HP5 16, FP4 10, T-MAX 8, Acros 7). The B&W defaults merge in automatically. A test
  is in `crates/spektrafilm-core/src/grain.rs`, and `data/CHANGELOG.txt` has an entry.
- Upstream's fallback checks out: `_apply_grain_preset` (params_builder.py:237 at 28bf883) returns
  early for a stock with no section, as Negs does. It is not a parity bug.
- The anchor isn't net D 1.0. `coarsest_area_from_curves` sizes the grain so that the **peak**
  RMS over the curve equals the preset. The peak sits near Dmax, so σ at net D 1.0 reads 0.85 to
  0.93 × the preset for every B&W film. Upstream's Double-X reads 13.0 for a preset of 14 at
  6.5 min. The ratio is similar across films, so their order holds. The presets keep upstream's
  convention (datasheet figure = preset), not a per-film correction.
- Tri-X 400: the owner chose accuracy over today's look ("it should be accurate to the real
  film"), so `kodak_trix400_d76_8min` now has rms 17 (Fotufilm's and SpektraLab's figure; Kodak F-4017,
  rev. 10-21, p7 lists 17, measured in HC-110 (B) at 48 µm, net D 1.0; Kodak gives no D-76 figure). Before, on the schema default, it read like T-MAX 100 (flat-roof noise 1.61
  vs 1.58); at 17 it reads 2.77, beside HP5's 2.80. The owner likes the grain. Page:
  https://claude.ai/artifact/YSu9xhNJTEndT6LE7LNeR9.
- Step 3 (colour), 5 November 2026, not committed yet: a second Negs-only block in `grain.toml` for
  the 24 colour ports. The "per class" plan didn't hold. The overlap splits by where Fotufilm's
  figure comes from, not by class:
  - Fotufilm's datasheet figures match upstream 1:1: C-200, X-TRA 400 and Pro 400H all 4; Provia
    8, Ektachrome 8, Kodachrome 64 10. So Fujifilm still negatives (Superia 200, Reala 100,
    Pro 160NS), all slides and Aerocolor 2460 (Kodak AS-2652: 8.5 at C-41 3:15) are used × 1000,
    uniform over the channels as upstream's still films are.
  - Fotufilm's Kodak consumer figures read 1.9× upstream (Gold 10.5/5, UltraMax 11/6, Ektar 6/4,
    Portra 400 9/4.5). Kodak publishes only Print Grain Index for these now, so Fotufilm's numbers
    are not datasheet RMS. Kodacolor 100 and the approximate films (Lomography Color 100,
    CineStill 400D, LomoChrome, Phoenix) are scaled by 0.525 (least squares on those four).
  - Kodak cine: upstream reads 1.54× Fotufilm's per-layer figure (grainStrength × layer weight;
    least squares over 250D and 500T). The Fujifilm cine films (Eterna, Eterna Vivid, F-64D,
    Reala 500D) have no overlap and use that fit with their own layer weights. That is a
    judgement call: on Fujifilm's own still scale (× 1), Eterna 500 would read finer than Ektar.
  - CineStill 800T copies Vision3 500T's rms, uniformity and sublayers (same emulsion; Fotufilm
    gives both identical grain).
  - Measured on a flat 18 % grey frame at the half-size pitch (L* noise, sigma-25 high-pass;
    scratch `cgrain/flat.py`): Pro 160NS 0.48 ≈ Ektar 0.52 < Portra 400 0.59 < Gold 0.69 <
    Eterna 500 0.76 ≈ Portra 800 0.78 < Aerocolor 1.14 < Phoenix 1.31 < CineStill 800T 1.66 ≈
    Vision3. Before, every ported negative read 1.0–1.2, grainier than Portra 800. Slides read
    0.27–0.62 (Aerochrome 1.13). On the photo patches, scene texture and sensor noise swamp colour
    grain at these levels (grain-off renders read *more* fine noise), so measure on the flat frame.
  - Page: https://claude.ai/artifact/C1vn3SMYFN3RhnLYhJkoek.
- Not started: step 4. SpektraLab (`research/spektralab/NOTES.md`) has no colour granularity
  data.

## The problem, as the owner sees it

Every film Negs ported from Fotufilm has the same grain. That is the 24 colour films and the 5 B&W
films, plus Negs' own datasheet Tri-X 400. Delta 3200 looks as fine-grained as Acros 100, and Ektar
as grainy as Superia 400. The owner chose to keep Negs' grain *model* for the B&W port and asked for
this follow-up: "investigate Fotufilm grain".

## Why it happens

`crates/spektrafilm-core/src/grain.rs` `stock_preset` reads `data/presets/grain.toml`, upstream
spektrafilm's per-stock table (25 sections). A stock missing from it gets the schema defaults
(`SCHEMA_RMS_GRANULARITY` = [6, 8, 10], uniformity 0.97, density_min 0.03, sublayers
[1, 0.5, 0.25]). None of the ported films, and not `kodak_trix400_d76_8min`, is in the table.

A B&W stock that is missing does **not** get `[defaults.bw.negative]` (rms 14, uniformity 0.6,
density_min 0.01) either. Those defaults are only merged into stocks that have a section of their
own. Check whether upstream Python's `_apply_grain_preset` does the same before treating this as a
bug.

## What Fotufilm has

Each Fotufilm stock (`research/fotufilm/upstream/Sources/FotufilmCore/Stocks/*.json`) carries:
- `grainStrength`: the datasheet's diffuse RMS granularity / 1000, i.e. σ_D through the 48 µm
  aperture at net density 1.0 (gross 1.0 for reversal). See `FilmStock.swift` around line 262.
- `grainSizeMM` (clump radius: 0.005 for slow films, 0.008 for fast ones), `grainLayerWeights`
  (colour: about [0.7, 1, 1.35] R/G/B), `grainLumaCorrelation` (colour 0.6),
  `crystalGrainPopulation`, `grainDensityProfile` (the same on every stock), and optional mottle.
- Development conditions may override grain fields. None of the B&W conditions does.
- Model: `docs/film-grain.md` upstream. It is a crystal-by-crystal "film" grain model, anchored to
  the sheet's RMS granularity, plus a "standard" model. Porting it would be a large engine feature
  (tile builder, per-pitch tone tables, new kernel stage on CPU, wgpu and the web port).

Negs' `rms_granularity` (`crates/spektrafilm-model/src/grain.rs` line 40, `params.rs` line 196) is
**the same quantity**: σ_D × 1000 at 48 µm, net D 1.0, per channel.

| Film | Fotufilm `grainStrength` | Negs today | Note |
|---|---|---|---|
| Delta 3200 | 0.019 | 6 (schema) | |
| Tri-X 400 | 0.017 | 6 (schema) | Kodak F-4017 lists 17 (HC-110 B) |
| HP5 Plus | 0.016 | 6 (schema) | |
| Double-X 5222 | 0.014 | **14** (grain.toml) | B&W cross-check: the two agree 1:1 |
| FP4 Plus | 0.010 | 6 (schema) | |
| T-MAX 100 | 0.008 | 6 (schema) | Kodak F-4016 lists 8 |
| Acros 100 II | 0.007 | 6 (schema) | Fujifilm AF3-0259E lists 7 (p3) |

For colour the scales **don't** map 1:1. Negs' (spektrafilm's) per-layer values are about half of
Fotufilm's composite figure on negatives, 1:1 on Provia, and different again on cine:

| Film | Fotufilm | Negs grain.toml |
|---|---|---|
| Portra 400 | 0.009, weights [0.7, 1, 1.35] | [4.5, 4.5, 4.5] |
| Gold 200 | 0.0105 | [5, 5, 5] |
| Ektar 100 | 0.006 | [4, 4, 4] |
| UltraMax 400 | 0.011 | [6, 6, 6] |
| Provia 100F | 0.008 | [8, 8, 8] |
| Vision3 250D | 0.0063, weights [0.85, 1, 2.15] | [10, 11, 18] |

The colour scale therefore needs a calibration (per class: negative, slide, cine) fitted on the
films both tables share. It isn't a straight copy.

## Suggested plan

1. **B&W presets, data only (recommended first).** Add `grain.toml` sections for the five B&W ports
   and `kodak_trix400_d76_8min` with `rms_granularity` = Fotufilm × 1000, or the datasheet figure
   where checked (T-MAX 8, Acros 7). Decide whether to also set the B&W defaults
   (uniformity 0.6, density_min 0.01) by naming them, or to fix the fallback in `stock_preset`.
   That fallback is an engine change: it changes Tri-X 400's current look, so get the owner's OK.
   `grain.toml` is upstream's file "unmodified apart from this header". Either keep Negs additions
   in a separate section with a comment, or record them in `data/CHANGELOG.txt` like the filter
   entries.
2. **Verify the anchor.** Render a flat patch at net D 1.0 for each film with grain on, measure σ_D
   through a 48 µm aperture (pixel pitch from the film format), and check that it reads the preset.
   The Rust model claims this. Check it at least for one slow and one fast film, B&W and colour.
   Then show the owner crops at 100 % on his NEFs (DSC_5784/5809/6298/6448, via
   `tools/fotufilm_port/xphotos_bw.py` with grain on): Delta 3200 vs HP5 vs Acros, and the same for
   colour.
3. **Colour presets.** Fit Fotufilm-to-Negs factors per class on the overlapping stocks (Portra
   160/400/800, Gold, Ektar, UltraMax, C200, Pro 400H, Superia X-TRA, Provia, Velvia 100,
   Ektachrome, Kodachrome 64, Vision3), then apply them to the 24 colour ports. Fotufilm's
   `grainLayerWeights` can set the per-channel split. Superia 200 and the approximate films stay
   approximate.
4. **Fotufilm's film-grain model: only if 1 to 3 don't look right.** Compare Fotufilm with
   `--grain 1` against Negs with the new presets on the same crops first. A real port is a large
   engine feature and needs the owner's explicit OK.

## How the owner works

Product owner: technical, not a film expert. Lead with what he'll see ("Delta 3200 is now visibly
grainier than HP5"), keep film science in docs. He OKs every commit. Commit to the
`fotufilm-port` branch only when asked; never push or merge to `main` unasked. Visual comparison
pages are welcome. Use only his own NEFs, never third-party sample raws.

Practicalities:
- Toolchain: `source /home/noah/Work/Negs/linux/core-env.sh` (the worktree's own `core-env.sh`
  finds no cargo).
- For the CLI and the catalog:
  `LD_LIBRARY_PATH=/home/noah/Work/Negs/target/linux-toolchain/sysroot/usr/lib`.
- Tests:
  `cargo test --release -p spektrafilm-core -p spektrafilm-io -p spektrafilm-native`
  (280 passed at `9c617b1`).
- App: `bash linux/build.sh`, then `bash linux/run.sh [image]`, both after sourcing the main
  checkout's `core-env.sh`.
- Never run `cargo fmt` (no rustfmt config; it rewrites unrelated files). Keep Rust lines ≤ 100
  characters.
- Fotufilm reference renders: `ssh noahhaasis@192.168.178.88`, `~/fotufilm-ref`,
  `engine/.build/release/fotufilm ... --grain 1`.
- `compare_negs.render` swaps the output path's suffix, so keep dots out of output stems
  (`6.5min` becomes `6.png`).
