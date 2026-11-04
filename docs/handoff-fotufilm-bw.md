# Handoff: porting Fotufilm's black-and-white films

Written 2 November 2026 at the end of the Fotufilm colour port (branch
`fotufilm-port`, worktree `~/Work/Negs-fotufilm`). That branch is committed
locally only: not merged to `main`, not pushed. Start this work from that
branch, or from `main` once the owner has merged it.

## Status, 2 November 2026: ported, not committed

The five films are in the working tree on `fotufilm-port`, uncommitted, waiting for the owner's OK:
`ilford_hp5_plus`, `ilford_fp4_plus`, `ilford_delta_3200`, `kodak_tmax_100`, `fujifilm_acros_100_ii`.
The 35mm borders, halation fixture, Linux catalog, README, `FOTUFILM_LICENSE.txt` and `CHANGELOG.txt`
are done. `cargo test --release -p spektrafilm-core -p spektrafilm-io -p spektrafilm-native`: 280
passed, 0 failed, 3 ignored. `spektrafilm-gui` builds.

Owner decisions (answers to the open questions at the end):
- **Push/pull:** one entry per film with a development-time picker holding every Fotufilm time,
  estimated ones included (HP5 9.5/14, FP4 6/9 min). The engine gained an optional
  `data.default_development_time` (`profile.rs`, `ProfileData::development_index`; validated to be
  one of the times), because Negs otherwise opens on the middle time, which for HP5 is a push. The
  egui GUI pre-selects it; the Linux, macOS and Windows apps send "Profile default" (null), which
  the core resolves. Python upstream doesn't know the field.
- **Grain:** keep Negs' grain. **Follow-up: investigate Fotufilm's per-film grain.** Today every
  film missing from `data/presets/grain.toml` gets the schema default (rms 6, uniformity 0.97).
  That covers all the new B&W films, Tri-X 400 and the colour ports, so Delta 3200 is as fine as
  Acros.
- **Paper:** grade 2 for every film (only B&W paper).

What was done, and what it found:
- **Datasheet check** (`research/fotufilm/bw-datasheet-check.md`): every reference curve overlaid
  on its manufacturer graph matches (HP5 0.016 D, FP4 0.009 D, T-MAX 0.02 D, Acros within line
  width, Delta all four times). This also fixed the reference times Fotufilm leaves implicit: HP5
  6.5, FP4 8, Delta 9, Acros 7, T-MAX 6 min. None is marked approximate. The estimated times are
  listed in `metadata.estimated_development_times`.
- **Converter:** `convert.py` `convert_bw` (one channel, same `BALANCE_K` as every Negs profile
  incl. Tri-X, flat silver, per-time base from each curve's dMin, four norm_cdfs components because
  three miss the 0.01 D gate, antihalation "strong" as Negs' Tri-X). `port_stocks.py` has the `BW`
  manifest and `port_bw`; `ship.py <dir> --bw`. Fotufilm's exposure axis is kept, as in the colour
  port. It places mid-grey differently per film (Delta 3200 thin at EI 3200).
- **Cross-engine** (`xeng_bw.py`; `xphotos_bw.py` for the owner's NEFs): Fotufilm `--paper screen`
  vs Negs on grade 2, L* only. Negs times every print to the same mid-grey; Fotufilm keeps the
  film's speed in the print, so raw gaps are 5 to 19 L*, the Tri-X baseline included. With the
  print exposure matched to Fotufilm's mid-grey once per film, normal development gives Tri-X
  baseline 3.0, HP5 2.5, FP4 2.5, Acros 1.6, T-MAX 2.6, Delta 4.3. All pass. Delta at +2 EV is
  5.4 against the baseline's 2.0, just outside. With the timing matched per development time,
  pushes and pulls are within 3 to 6. HP5 at 14 min is the exception (8.7). Fotufilm's B&W
  screen view is a digital read that stays soft; Negs prints the contrasty negative the way grade
  2 paper would.
- **Owner page:** "Five new black-and-white films",
  https://claude.ai/artifact/HbuvVXVtmqFMws8gEdXa9L (private to the owner's account), uses only
  the owner's NEFs.
- **Edge prints:** Fotufilm's texts; Acros II has none in Fotufilm, so it prints "ACROS II".

## What the owner wants

- The owner is the product owner: technical, but not a film expert. Report
  what the user will see, such as "HP5 now looks like this next to
  Tri-X", not curve maths.
- Port Fotufilm's **profiles only**, onto Negs' existing engine. Port a
  per-stock engine feature only if a film can't look right without it.
- **Where Negs already has a film, keep the Negs version.** For black and
  white that means Tri-X 400 (`kodak_trix400_d76_8min`) and Double-X 5222
  (`kodak_doublex`) stay as they are, and Fotufilm's `trix400` and
  `doublex5222` are used only as comparison references.
- Mark approximate films as approximate.
- Credit Fotufilm as the colour port does. Do not contact its author.
- The owner OKs every commit. Commit to `main` and push only when asked;
  this repo doesn't use PRs.

## Candidates

Fotufilm @ `96623ba` (`research/fotufilm/upstream/Sources/FotufilmCore/Stocks/`)
has eight monochrome stocks:

| Fotufilm id | Film | Development conditions | Data basis | Plan |
|---|---|---|---|---|
| `hp5plus400` | Ilford HP5 Plus | 9.5 min (EI 800, push 1), 14 min | curves from Ilford TI (Nov 2018); dev times *estimated* | port |
| `fp4plus125` | Ilford FP4 Plus | 6, 9 min | dev times *estimated* | port |
| `delta3200` | Ilford Delta 3200 | 7, 12, 16 min | Ilford TI (2002), DD-X, *measured* | port |
| `acros100` | Fujifilm Acros 100 II | 4 (pull 1), 10 min | *measured* | port |
| `tmax100` | Kodak T-MAX 100 | 7.5 min | *measured* | port |
| `trix400` | Kodak Tri-X 400 | 10, 12 min | Kodak F-4017 | reference only (Negs has it) |
| `doublex5222` | Kodak Double-X 5222 | 4, 5, 9, 12 min | | reference only (Negs has it) |
| `example-monochrome-100` | synthetic test stock | | | skip |

Provenance text for each stock is in `research/fotufilm/stock-provenance.md`;
the full licence notice is in `data/FOTUFILM_LICENSE.txt`.

Each stock has:
- three identical `curves` (one per layer), each with `sampled` points plus
  analytic `dMin`/`gamma`/`shoulder`;
- `spectral.layerSensitivity` (three curves);
- `development.conditions`, each with its own curves, time and
  push/pull stops;
- B&W-specific engine settings (`paperCurve`, `adjacencyStrength`,
  `reciprocityFailure`, a grain population, `halationStrength` around 0.015
  to 0.03 on all three channels).

Before porting a film, check its density data against the manufacturer
datasheet named in its provenance; only Delta 3200 and HP5 cite one in the
curve text.

## What Negs already has for black and white

Read these before writing code:

- `docs/handoff-bw-expansion-results.md`: how `kodak_trix400_d76_8min` and
  the Ilford Multigrade RC paper were built from datasheets, including the
  checks used (a source-curve fit gate of 0.01 D, a neutral ramp, a
  reflection-density check and Python parity).
- `docs/handoff-bw-stocks.md`: the `bw` channel model, NaN handling for
  missing sensitivities, and development-time selection.
- Profiles: `data/profiles/kodak_trix400_d76_8min.json` is the closest template.
  - `info.channel_model` is `"bw"`, `densitometer` is `"diffuse_visual"`,
    `antihalation` is `"strong"`, and `target_print` is
    `ilford_mgrc_deluxe_grade2`.
  - `data` has `development_time`, `density_curves_model` and
    `density_curves_layers`.
  - Its 35mm border prints `KODAK 400TX`.
- B&W papers: Negs has only Multigrade RC grade 2 (experimental) and print
  film 2302. Fotufilm's `paperCurve` is a generic analytic paper, not a
  stock, so don't port it.

## How to port

The colour pipeline in `tools/fotufilm_port/` gets you most of the way:

1. **`convert.py`** stops on `isMonochrome` (line ~174, "B&W stocks need
   the Negs bw channel model"). Add a monochrome branch that:
   - writes one curve family (the three Fotufilm layers are identical; check
     this) in the `bw` layout Tri-X uses;
   - takes the sensitivity from `layerSensitivity` and rebalances it the way
     Tri-X's is normalised (see the bw-expansion recipe), not with the
     colour `BALANCE_K`;
   - writes the default development condition as the profile, and each other
     condition as a separate profile only if the owner wants push/pull
     variants (Negs ships Portra 800 +1/+2 as separate stocks, so there is a
     precedent);
   - sets `antihalation` from `halationStrength` with
     `antihalation_class()` (≥ 0.02 gives `"no"`, so HP5's 0.03 would get
     "no"). That is probably wrong for a film with an anti-halation layer, so
     check what Fotufilm's B&W halation numbers mean before relying on it.
2. **`port_stocks.py`**: add a `BW` manifest next to `NEG` and `POS`.
   - Target print: `ilford_mgrc_deluxe_grade2`.
   - B&W films get no `neutral_print_filters.json` entries; colour papers
     don't apply.
3. **`ship.py`**: copies new profiles and refuses to overwrite.
4. **`fold_check.py` / `coupler_presets.py`**: B&W has no colour couplers,
   so confirm the B&W path skips DIR couplers (it does for Tri-X) and skip
   this step.
5. **Edge prints**: add each film to `STOCKS` in
   `crates/spektrafilm-io/src/film35.rs` as `Kind::BlackAndWhite` (the
   `every_film_profile_has_a_35mm_edge_print` test enforces this). Then run
   `cargo run --release -p spektrafilm-io --example film35_borders`, which
   rewrites `fujifilm_velvia_100.png` with a 1-level difference, so revert
   that file. Edge texts from Fotufilm's `edgePrinting` field are better
   than invented ones; the colour port invented its texts.
6. **Halation fixture**:
   `crates/spektrafilm-core/tests/fixtures/halation_presets_python_28bf883.json`
   needs an entry per film. Use the table lookup by (use, antihalation), as
   in the colour port.
7. **Catalog / credits**:
   - run `linux/generate_catalog.py` with
     `LD_LIBRARY_PATH=/home/noah/Work/Negs/target/linux-toolchain/sysroot/usr/lib`;
   - add the films to the README "Your film shelf" row;
   - add them to `data/FOTUFILM_LICENSE.txt` (profile list and count) and
     `data/CHANGELOG.txt`.

Formatting: the repo has **no rustfmt config and isn't rustfmt-clean**, so
don't run `cargo fmt`; it reformats unrelated files. Keep lines ≤ 100
characters by hand.

## How to check the result

Use the cross-engine tools from the colour port:
- `xeng.py`: charts, rendered on the Mac `noahhaasis@192.168.178.88`,
  directory `fotufilm-ref`;
- `xphotos.py`: the owner's own NEFs DSC_5784/5809/6298/6448, decoded by
  `decode_photos.py`.

They will need a B&W mode:
- Compare luminance only.
- Use Fotufilm's B&W paper or screen path.
- Turn off Negs' print-exposure compensation, as the colour runs did.

Use Fotufilm's `trix400` against Negs' Tri-X as the baseline, the way Gold 200
and Provia were baselines for colour. A ported film is fine when its gap is
within about 2.5 of that baseline.

Things worth showing the owner side by side:
- HP5 vs Tri-X;
- Delta 3200 vs HP5 (grain and contrast);
- Acros and T-MAX 100 vs FP4.

Published comparison pages may only use the owner's NEFs, not third-party
sample raws.

Before asking for a commit, run:

```sh
source /home/noah/Work/Negs/linux/core-env.sh
cargo test --release -p spektrafilm-core -p spektrafilm-io -p spektrafilm-native
```

At the end of the colour port this was 253 passed, 0 failed.

## Open questions for the owner (answered 2 November, see Status)

- Push/pull variants: ship only the default time, or separate profiles per
  development condition?
- Should Fotufilm's B&W grain look be ported? It would be an engine
  feature, so it needs the owner's OK. Negs' current B&W grain is a
  generic creative recipe.
- Only one B&W paper exists today. Is grade 2 the right default for every
  film?
