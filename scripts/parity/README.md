# Python ↔ Rust parity checks

Use `check_parity.py` for a live comparison against an **explicit Python
checkout**. The target is upstream **experimental `28bf883`** (ticket 16).
Upstream main `3bb2c2d` still works as a reference until the port is done.
`ablate_experimental.py` attributes a remaining difference to a ticket 16
item. The original scripts at the end were manual diagnostic probes, not
an automated byte-for-byte regression gate.

## Run

Use Python 3.13 with the chosen reference's runtime dependencies installed
(including numpy, scipy, colour-science, numba, and OpenImageIO), plus Cargo.
The existing sibling `spektrafilm-env` environment works on the audit machine.
The runner imports directly from `--python-source`; it does not install or
modify that checkout. Python caches go into the output directory unless
their environment variables are already set. Make a clean reference tree
with `git -C ../spektrafilm archive 28bf883 | tar -x -C /path/to/exp`.

From the Rust repository root:

```bash
# Choose a NEW output directory for every run.
# Acceptance run for ticket 16: every named route, both engines at their
# defaults except grain and glare, LUTs off and on.
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --case all --experimental-defaults \
  --output-dir /tmp/spektrafilm-parity-exp

# One route, one effect at a time (everything else spatial is off):
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --case vision3-2383 --dir-couplers \
  --output-dir /tmp/spektrafilm-parity-dir

# Any film/paper pair; --scan-film scans the film instead of printing:
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp \
  --film kodak_gold_200 --paper fujifilm_crystal_archive_typeii \
  --output-dir /tmp/spektrafilm-parity-gold

# Film Curves against experimental, whose film_render.chemistry is the morph:
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --case doublex-2302 --gamut-off \
  --film-curves-morph '{"gamma_factor": 1.4, "developer_exhaustion": 0.4}' \
  --output-dir /tmp/spektrafilm-parity-film-curves

# Any parameter in the Rust JSON names, merged over the case's overrides
# (here a tuned film and print base, ticket 16 item 7):
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --case all --experimental-defaults \
  --clip-reference-output \
  --override '{"film_render": {"base": {"scale": 1.1, "tilt": 0.3, "cyan": 1.2}},
              "print_render": {"base": {"scale": 1.2, "magenta": 0.8}}}' \
  --output-dir /tmp/spektrafilm-parity-bases

# Other colour spaces, by colour's names (both engines share them): input
# space and CCTF decoding, output space and CCTF encoding:
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --case all --experimental-defaults \
  --input-color-space "DaVinci Wide Gamut" --input-cctf-decoding \
  --output-color-space "Adobe RGB (1998)" --output-cctf-encoding \
  --output-dir /tmp/spektrafilm-parity-dwg-adobe

# The same against upstream main, with experimental's morph baked into its
# film profile:
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/main-checkout --morph-source /path/to/exp \
  --dir-couplers --film-curves-morph '{"gamma_factor": 1.4, "developer_exhaustion": 0.4}' \
  --output-dir /tmp/spektrafilm-parity-film-curves-main

# Which ticket 16 item explains what's left (Python only, plus the CLI):
../spektrafilm-env/bin/python -I scripts/parity/ablate_experimental.py \
  --python-source /path/to/exp --build-rust \
  --output-dir /tmp/spektrafilm-ablation

# Refreeze the cargo-test fixture for the film morph (never from Rust output):
../spektrafilm-env/bin/python scripts/parity/freeze_film_morph_reference.py \
  --experimental /path/to/exp \
  --output crates/spektrafilm-core/tests/fixtures/film_morph_python_28bf883.json

# Light sources by name (tests/illuminants.rs): spectra, reference whites and
# the cine pairs 500T -> 2383 and 200T -> 2393, from a `git archive` of
# experimental 28bf883. From a worktree, pass --git-repo explicitly:
../spektrafilm-env/bin/python -I scripts/parity/freeze_illuminants_reference.py \
  --python-source /path/to/28bf883-archive --git-repo /path/to/spektrafilm \
  --output crates/spektrafilm-core/tests/fixtures/illuminants_python_28bf883.json

# Spectral-upsampling tc LUTs (hanatos2025, arctic2026beta04, mallett2019)
# and the neutral print filter database with its fallback:
../spektrafilm-env/bin/python -I scripts/parity/freeze_spectral_upsampling_reference.py \
  --experimental /path/to/experimental-checkout \
  --output crates/spektrafilm-core/tests/fixtures/spectral_upsampling_python_28bf883.json

# Ticket 16: experimental 28bf883 on Negs' data (exp_data_reference.py).
# Print the inline test values:
EXPDATA_REFERENCE=/path/to/experimental-checkout \
  ../spektrafilm-env/bin/python -I scripts/parity/exp_data_unit_references.py

# Refreeze the DIR couplers fixture (couplers.toml presets per stock, Langmuir
# terms, curve inversion and spatial correction) from a `git archive` of 28bf883:
../spektrafilm-env/bin/python -I scripts/parity/freeze_dir_couplers_reference.py \
  --python-source /path/to/28bf883-archive --git-repo ../spektrafilm \
  --output crates/spektrafilm-core/tests/fixtures/dir_couplers_python_28bf883.json

# Tuned film/print bases and the print-balance solver (tests/print_balance.rs)
# from a `git archive` of 28bf883:
../spektrafilm-env/bin/python -I scripts/parity/freeze_print_balance_reference.py \
  --python-source /path/to/28bf883-archive --git-repo ../spektrafilm \
  --output crates/spektrafilm-core/tests/fixtures/print_balance_python_28bf883.json

# Colour spaces (tests/colorspaces.rs): colour's matrices, whites, CCTFs and
# CATs, the input projection, the scan's XYZ_to_RGB, the gamut compressor per
# output space, the output encoding and seven pipelines across spaces. Needs
# NUMBA_CACHE_DIR, MPLCONFIGDIR and XDG_CACHE_HOME pointing somewhere writable:
../spektrafilm-env/bin/python -I scripts/parity/freeze_colorspaces_reference.py \
  --python-source /path/to/28bf883-archive --spektrafilm-repo ../spektrafilm \
  --output crates/spektrafilm-core/tests/fixtures/colorspaces_python_28bf883.json

# The engine's colour-space table itself, from colour 0.4.7:
../spektrafilm-env/bin/python -I scripts/gen_colorspaces.py \
  > crates/spektrafilm-math/src/colorspace_data.rs

# Existing frozen Python reference tests and the full Rust suite:
cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64

# Validate the comparator itself:
PYTHONDONTWRITEBYTECODE=1 ../spektrafilm-env/bin/python -I \
  -m unittest discover -s scripts/parity -p 'test_*.py'
```

The runner builds the current `spektrafilm-f64` CLI (release profile) with
the locked Cargo dependencies and forces `SPEKTRAFILM_BACKEND=cpu`. It does
not trust a potentially stale executable unless `--rust-binary` names one.
It requires the Python topology API
(`SimulationPipeline.process(..., collect=...)`), present in the audited
0.3.4 revisions and in experimental; older Python APIs may require an adapter.

## Named routes

`--case` (repeatable, or `all`) replaces `--film`/`--paper`/`--scan-film`:

| Case | Film | Print |
|---|---|---|
| `portra-endura` | Kodak Portra 400 | Kodak Portra Endura |
| `gold-crystal-archive` | Kodak Gold 200 | Fujifilm Crystal Archive Type II |
| `provia-scan` | Fujifilm Provia 100F | none (film scan) |
| `doublex-2302` | Kodak Double-X | Kodak 2302 |
| `vision3-2383` | Kodak Vision3 500T | Kodak 2383 |

The film scan still names Portra Endura as its paper, because both CLIs
need one; it doesn't enter the scanned result. With `--case`, case
directories are `<case>/<fixture>-lut-<on|off>`. Without it the layout is
flat, as the fixture freezers expect.

## Parameters in both engines

The runner writes one override file in the **Rust** JSON names
(`params.json`) and translates it for the reference (`python-overrides.json`):

| Rust | Experimental `28bf883` | Main `3bb2c2d` |
|---|---|---|
| `io.scan_film` | `workflow.route = "input > film > scan"` | `io.scan_film` |
| `film_render.development_time` | `film_render.chemistry.development_time` | — (error) |
| `film_render.density_curves_morph` | `film_render.chemistry` | curves baked from `--morph-source` |
| `print_render.development_time` | `print_render.chemistry.development_time` | — (error) |
| `print_render.density_curves_morph` | `print_render.chemistry` | `print_render.density_curves_morph` |
| `io.input_gamut_compress.algorithm = "off"` | `io.input_gamut_compress.active = false` | same as experimental |
| `io.*_gamut_compress.center` | dropped (Python hard-codes D65) | dropped |

`--override JSON` merges any further Rust-named parameters over the case's
own (leaves replace) before the translation; the report records it.

Frozen dataclasses (`chemistry`, the gamut specs) are replaced with
`dataclasses.replace`. A name the reference doesn't have is an error, so a
renamed upstream parameter can't be ignored silently. `python-params.json`
records every digested reference parameter (presets applied), so changed
defaults stay visible.

## What is compared

- Identical float32-representable linear sRGB input, promoted to f64 in both
  pipelines. Python consumes the array; Rust loads its lossless float TIFF.
  RAW decoder, orientation, and white-balance differences are excluded.
- Four deterministic fixtures (`--fixture` selects some): 1×1 midgray,
  4×4 colour gradient, 16×16 gray ramp, and `scene`, a 384×256 image with
  spatial structure: a hue × saturation field, a grey ramp, a 1-pixel
  checkerboard and an 8× highlight disc. At the default 35 mm format a
  pixel is 91 µm, so halation (σ 50–65 µm per bounce) and the DIR diffusion
  tail (200 µm) cross several pixels around the highlight.
- Film log exposure, film CMY density, print CMY density, final RGB.
- Both enlarger/scanner LUTs off and on, at resolution 17 (`--lut` selects
  a single mode if desired).
- Output is linear sRGB. Grain and film/print glare are always off: their
  random streams differ between the engines.

Effects, by default all off so that per-pixel parity is isolated:

- `--experimental-defaults` keeps every effect at **each engine's own
  defaults**: the reference's stock presets (halation, DIR couplers),
  auto-exposure, halation, DIR couplers and the scanner unsharp mask
  (0.7/0.7). Only grain and glare are off. Rust gets no other overrides, so
  this is the ticket 16 acceptance run: it fails until Negs' defaults match.
- `--dir-couplers`, `--halation` (scatter and back-reflection),
  `--auto-exposure` (deterministic: center-weighted metering) and
  `--scanner-usm` (0.7/0.7 on both) switch one effect on.
- `--lens-blur CAMERA_UM ENLARGER SCANNER` sets the three lens blurs
  (default 0); it combines with `--experimental-defaults`.
- `--clip-reference-output` clips Python's `rgb_out` to [0, 1] before
  comparing. Upstream (main and experimental) leaves the scan output, and
  so the unsharp mask's overshoot, unclipped. Rust does too since ticket
  16.10; older builds (main) clip, and against them this option shows
  what's left once the clip is accounted for.
- `--input-color-space`, `--input-cctf-decoding`, `--output-color-space` and
  `--output-cctf-encoding` set `io` in both engines (default: sRGB in and
  out, neither curve). Adobe RGB and DCI-P3 encode negatives to NaN
  in colour; the comparison requires NaN in the same places and compares the
  rest.
- `--no-arrays` deletes each case's images and arrays after comparing (the
  scene keeps about 20 MB per case).

Every float must be finite (bar the NaN above) and within absolute tolerance `1e-6`, with no
relative tolerance. This matches the existing Rust f64 reference tests.
`--atol 0` requires numerical equality; the separately reported
`byte_identical` field compares canonical little-endian f64 buffers (even
signed zeros can differ bytewise).

Exit status is **0 for pass, 1 for numerical drift, 2 for execution errors**.
Shape mismatches, NaNs, missing dumps, or Rust's simplified-pipeline fallback
are errors. The comparison never accepts a small mean error in place of
checking every component.

`report.json` records commits, dirty status, source/data/binary hashes,
the reference API, dependency versions, maximum/mean differences, exact
equality, the first stage outside tolerance, and a per-route summary.
`summary.md` (also printed) has one row per route and LUT mode: the first
stage outside tolerance in any fixture and each stage's largest error.
Per-case files retain Python arrays, Rust f64 dumps, the input TIFF,
parameter JSON, and Rust logs. The 8-bit metric
quantizes **linear RGB** identically on both sides; it is not a comparison
of encoded PNG file bytes or the historical sRGB-encoded PNG experiment.

B&W exposure and density taps are single-channel in Python and three
identical lanes in Rust: the runner explicitly repeats the Python channel
and checks all three Rust lanes. Final RGB is always compared as three
channels.

`--film-development-time` and `--print-development-time` select B&W family
members. `--gamut-off` explicitly disables both input/output gamut
compressors in both implementations; this isolates stock parity from the
gamut algorithms/defaults. The report records these options, so such a
pass is not confused with default-settings parity.

Both implementations default to experimental `28bf883`'s gamut settings,
so a run against `28bf883` compares them without flags (the former
`--experimental-gamut` flag and preset were removed in ticket 16.8). Against
main, use `--gamut-off`. Add `--gamut-stress` to compare saturated,
imaginary, and above-white inputs as well as the ordinary three fixtures.
See the [gamut handoff](../../docs/handoff-experimental-gamut.md) for the
passing B&W routes and Python fixture regeneration.

`--auto-exposure` keeps auto exposure on in both implementations and adds a
300×200 frame, large enough for the metering downsample. `--color-filter
hoya_r1` (or another key) sets the camera colour filter in both.

`--adversarial-input` adds 1024 seeded HDR/imaginary/near-black pixels.
`--rust-binary /path/to/spektrafilm-f64 --rust-data /path/to/data` checks an
existing release and its own data instead of building a development binary.
See the [adversarial review](../../docs/review-bw-gamut.md) for the near-black
bug this exposed and the hardened fixture-provenance checks.

```bash
../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --film kodak_doublex --paper kodak_2302 \
  --film-development-time 4 --print-development-time 2 --gamut-off \
  --output-dir /tmp/spektrafilm-bw-print-check

../spektrafilm-env/bin/python -I scripts/parity/check_parity.py \
  --python-source /path/to/exp --film kodak_trix --scan-film --gamut-off \
  --output-dir /tmp/spektrafilm-bw-slide-check
```

These tests do not establish parity for every profile, GPU execution,
RAW decoding, or stochastic grain. See the
[B&W stock port handoff](../../docs/handoff-bw-stocks.md) for the tested B&W
routes and frozen Python fixtures.

## Attributing a difference: `ablate_experimental.py`

The ablation renders the `scene` fixture (`--fixture` picks another) with
experimental at its defaults, grain and glare off and linear sRGB in and
out: the same settings as `--experimental-defaults`. Then it reverts one
ticket 16 item at a time to what Negs did before the port and reports how
far each revert moves the output (mean/p99/max ΔE00 on linear output,
sRGB-encoded 8-bit steps, linear max). The items:

| Variant | Reverts to |
|---|---|
| `1_film_stored_curves` | the film profile's stored curves instead of the fitted model |
| `9_profiles_negs` | Negs' profiles (`--negs-data`, default `data/`) |
| `2_couplers_negs_gammas` | Negs' hard-coded DIR gammas instead of `couplers.toml` |
| `2_couplers_linear` | no Langmuir saturation |
| `2_couplers_tail_006` | DIR diffusion tail weight 0.06 |
| `4_halation_no_preset` | halation schema defaults, no `use`/`antihalation` preset |
| `5_wb_main` | window normalisation on the bare illuminant |
| `5_npf_negs` | Negs' neutral print filter database, missing entries C0 M65 Y55 |
| `6_gamut_main` | main's gamut defaults (locus, CAM16-UCS, centre on the reference illuminant) |
| `8_autoexposure_rgb` | metering on input RGB, no midgray scale |
| `0_illuminant_fallback` | `T`/`K75P` falling back to D55/D50 (the pre-16.2 bug) |
| `all_negs`, `all_negs_illuminant_fallback` | every item above, without/with the fallback |

Rows don't add up; the items interact. With `--rust-binary` (or
`--build-rust`) it also renders the f64 CLI on the same input and compares
it with every variant, marking with `*` each variant that is closer to Rust
than experimental is. On the experimental-port base (before any engine
branch), `all_negs` reproduces Rust within 0.02 8-bit steps (mean ΔE00
≤ 2e-4) on four routes, and `all_negs_illuminant_fallback` does on
Vision3 → 2383. The remaining linear difference (≤ 0.14) was the output
clip that Rust had then.
So during integration, a `*` on one item names what's left to port, and a
row where Rust is far from every variant is a new bug.

After the integration (16.10) no variant is marked on any route: the
integrated f64 CLI matches experimental to 0 8-bit steps (mean and p99
ΔE00 0.00); the remaining linear difference (≤ 0.12, ΔE00 max 0.15–3.75 on
the highlight spot) was the output clip, which Rust no longer applies. Since `data/` now holds 28bf883's
profiles and filter database, the `9_profiles_negs` and `5_npf_negs`
reverts load upstream's own data and change nothing; the pre-port data is
in Git history.

The reverts patch private 28bf883 functions (`compute_hanatos2025_tc_lut`,
the pipeline's curve refresh, `FilmingStage._auto_exposure`), so another
upstream commit may need changes in the script. `--item` restricts the
variants, `--case` the routes, and `--save-arrays` keeps every output.

## Starting failure table against 28bf883

Measured on 2026-10-25 with the f64 CLI built from `exp-harness` (engine
identical to `experimental-port` `13ac0d0`) against a clean `28bf883` tree.
Every value is the largest absolute error over the four fixtures; the
first failing stage is the earliest stage outside 1e-6 in any fixture.
"8-bit" is the largest linear-RGB 8-bit difference.

**Acceptance run** (`--case all --experimental-defaults`):

| Case | LUTs | First failing stage | log_e_film | cmy_film | cmy_print | rgb_out | 8-bit |
|---|---|---|---|---|---|---|---|
| portra-endura | off | log_e_film | 2.9e-01 | 1.3e-01 | 3.1e-01 | 1.5e-01 | 37 |
| portra-endura | on | log_e_film | 2.9e-01 | 1.3e-01 | 3.1e-01 | 1.5e-01 | 37 |
| gold-crystal-archive | off | log_e_film | 1.4e-01 | 1.0e-01 | 1.8e-01 | 1.4e-01 | 37 |
| gold-crystal-archive | on | log_e_film | 1.4e-01 | 1.0e-01 | 1.8e-01 | 1.4e-01 | 37 |
| provia-scan | off | log_e_film | 3.0e-01 | 8.1e-01 | — | 1.4e-01 | 36 |
| provia-scan | on | log_e_film | 3.0e-01 | 8.1e-01 | — | 1.4e-01 | 36 |
| doublex-2302 | off | log_e_film | 1.6e-01 | 5.6e-02 | 1.2e-01 | 2.0e-01 | 46 |
| doublex-2302 | on | log_e_film | 1.6e-01 | 5.6e-02 | 1.2e-01 | 2.0e-01 | 46 |
| vision3-2383 | off | log_e_film | 5.3e-01 | 4.5e-01 | 1.4e+00 | 3.7e-01 | 95 |
| vision3-2383 | on | log_e_film | 5.3e-01 | 4.5e-01 | 1.4e+00 | 3.7e-01 | 96 |

**Per-pixel run** (`--case all`; auto-exposure, halation, DIR couplers,
unsharp mask and lens blur off):

| Case | LUTs | First failing stage | log_e_film | cmy_film | cmy_print | rgb_out | 8-bit |
|---|---|---|---|---|---|---|---|
| portra-endura | off | log_e_film | 1.0e-01 | 5.3e-02 | 1.2e-01 | 1.1e-01 | 28 |
| portra-endura | on | log_e_film | 1.0e-01 | 5.3e-02 | 1.2e-01 | 1.1e-01 | 28 |
| gold-crystal-archive | off | log_e_film | 7.2e-02 | 4.1e-02 | 1.4e-01 | 1.2e-01 | 30 |
| gold-crystal-archive | on | log_e_film | 7.2e-02 | 4.1e-02 | 1.4e-01 | 1.2e-01 | 30 |
| provia-scan | off | log_e_film | 9.8e-02 | 1.5e-01 | — | 1.1e-01 | 28 |
| provia-scan | on | log_e_film | 9.8e-02 | 1.5e-01 | — | 1.1e-01 | 28 |
| doublex-2302 | off | log_e_film | 2.9e-03 | 2.0e-03 | 2.9e-03 | 1.6e-01 | 41 |
| doublex-2302 | on | log_e_film | 2.9e-03 | 2.0e-03 | 2.9e-03 | 1.6e-01 | 41 |
| vision3-2383 | off | log_e_film | 5.2e-01 | 2.6e-01 | 1.1e+00 | 2.8e-01 | 72 |
| vision3-2383 | on | log_e_film | 5.2e-01 | 2.6e-01 | 1.1e+00 | 2.8e-01 | 73 |

Reading the tables:

- Every route fails at film log exposure, so the earliest items (16.2
  illuminants on Vision3, 16.3/16.4 profiles and white balance, 16.8 input
  gamut defaults) have to land before later stages can be judged. Double-X
  passes per pixel with `--gamut-off` (all stages ≤ 3e-15), so its
  per-pixel error is the input gamut default alone.
- The `scene` fixture sets most maxima: saturated colours (gamut) and, in
  the acceptance run, the highlight (halation presets, auto-exposure).
- `--clip-reference-output` changes only `rgb_out` of the acceptance run
  (Double-X 2.0e-01 → 1.8e-01, Portra 1.5e-01 → 1.4e-01).

### Spatial gaps that predate ticket 16

Against upstream main `3bb2c2d` with matched parameters, Portra 400 →
Endura per pixel passes on all four fixtures (≤ 8e-15 before the scan,
4e-9 at `rgb_out`), and auto-exposure passes (1.5e-7). The spatial
effects had never been compared on a spatial input; on `scene` they show
the differences below. Upstream's unsharp mask and Gaussian blur
functions and its unclipped scan output are unchanged on experimental, so
ticket 16 has to resolve or document these as well:

| Effect (Portra unless noted) | First failing stage | Max error |
|---|---|---|
| scanner unsharp mask 0.7/0.7 | rgb_out | 0.19 (Rust clips at 0); 3.7e-5 with `--clip-reference-output` |
| scanner lens blur 1 / 2 | rgb_out | 3.4e-4 / 2.1e-3 (clipped reference) |
| camera lens blur 100 µm | log_e_film | 3.1e-2, at the image border (edge handling) |
| DIR coupler diffusion (Portra / Provia scan) | cmy_film | 7.7e-6 / 2.8e-5 |
| enlarger lens blur 1 / 2, camera lens blur 5 µm | — | pass |
| halation | log_e_film | presets missing in Negs (main applies them too) |

All of these are resolved by ticket 16: the halation presets came with
16.7, the blurs' borders (lens blur, DIR diffusion, unsharp mask) reflect
like Python's since `exp-edges`, and the scan output is unclipped like
Python's since 16.10; see the result below.

## Result after integration (ticket 16.10)

Measured on 2026-10-26 with the f64 CLI built from `experimental-port`
after every wave-1 branch and `exp-edges` were merged, against a clean
`28bf883` tree. Values are the largest absolute error over the five
fixtures (`frame300` is added with `--experimental-defaults`), over the
whole image: the border band is no longer an exception. Like Python, the
scan output is unclipped (it reaches −0.09 and 1.06 on `scene`); displays
and 8/16-bit files clip it, as Python's `save_image_oiio` does.

**Acceptance run** (`--case all --experimental-defaults`, no clip on
either side): **50/50 pass.**

| Case | LUTs | Passed | log_e_film | cmy_film | cmy_print | rgb_out | 8-bit |
|---|---|---|---|---|---|---|---|
| portra-endura | off | 5/5 | 1.2e-08 | 5.1e-09 | 7.6e-09 | 8.7e-09 | 0 |
| portra-endura | on | 5/5 | 1.2e-08 | 5.1e-09 | 7.6e-09 | 8.7e-09 | 0 |
| gold-crystal-archive | off | 5/5 | 4.4e-08 | 8.9e-09 | 3.3e-08 | 9.9e-09 | 0 |
| gold-crystal-archive | on | 5/5 | 4.4e-08 | 8.9e-09 | 3.3e-08 | 1.0e-08 | 0 |
| provia-scan | off | 5/5 | 1.3e-08 | 2.5e-08 | — | 8.0e-09 | 0 |
| provia-scan | on | 5/5 | 1.3e-08 | 2.5e-08 | — | 8.0e-09 | 0 |
| doublex-2302 | off | 5/5 | 3.4e-09 | 9.9e-10 | 1.7e-09 | 1.0e-08 | 1 |
| doublex-2302 | on | 5/5 | 3.4e-09 | 9.9e-10 | 1.7e-09 | 1.0e-08 | 1 |
| vision3-2383 | off | 5/5 | 7.8e-09 | 5.1e-09 | 2.0e-08 | 7.3e-09 | 0 |
| vision3-2383 | on | 5/5 | 7.8e-09 | 5.1e-09 | 2.0e-08 | 7.3e-09 | 0 |

The Double-X 8-bit step is one value on `scene` that sits on a rounding
tie: Rust 225.4999998/255, Python 225.5000007/255, 3e-9 apart. The
clipping build before 16.10 rounds it the same way. **Per-pixel run**
(`--case all`, every effect off): **40/40 pass**, every stage ≤ 1.3e-08.

**Film and print bases** (ticket 16 item 7, `exp-bases`): the same run
with `--override` tuning both bases (film scale 0.9, tilt -0.4, C/M/Y 0.8,
1.15, 0.95; print scale 0.8, C/M/Y 0.9, 1.2, 1.3) and scanner white and
black correction on passes 50/50, every stage ≤ 8.1e-08 and `rgb_out`
≤ 3.9e-08. Scale-only bases (1.3 film, 1.4 print) pass 50/50, ≤ 4.4e-08.

Exceptions that remain:

- **Diffusion filter** (off by default): Rust keeps f64 where Python's FFT
  runs in float32 (decided 2026-10-25).
- **Grain** is gated statistically (`tests/grain_statistics.rs`, CPU and
  GPU), not per pixel.

## Catalog and controls: `sweep_experimental.py`

`check_parity.py` proves the f64 engine stage by stage on five routes. The
sweep covers the whole catalog and the inspector, and compares what the app
shows. Each case renders one 384 × 766 input (two golden photographs above
the `scene` fixture) with Python, the f64 CLI (float for float) and the app's
own engine, `spektrafilm-native` on the GPU, driven over its JSON-lines
protocol as the app drives it (the 8-bit sRGB preview buffer it hands the
app). Both engines keep their defaults; grain and glare are off.

The plan (556 cases) is fixed in `build_plan`:

- every film in both catalogs on the paper the app pairs it with, every
  colour negative on Supra Endura (the Python app's default paper), and every
  paper with Portra 400 (Double-X for B&W papers);
- on Portra 400 → Endura, Vision3 500T → 2383, Provia 100F scanned and
  Double-X → 2302: every Lab slider once below and once above its default,
  and every toggle and picker once. `response` records how far each setting
  moves the picture from the route's defaults (mean and max ΔE00) in Python
  and in the app.

Python runs in warm worker processes, once per case and to `rgb_out` only;
the f64 renders and metrics run in parallel. A full run takes about 4
minutes on an M1 Pro. `--reuse-python` checks another build, such as a
released app, against an earlier run's references in about 20 s. Inputs are
the golden harness's TIFFs (`NEGS_GOLDEN_INPUTS`, default
`target/golden/inputs`).

```bash
NEGS_GOLDEN_INPUTS=target/golden/inputs ../spektrafilm-env/bin/python -I \
  scripts/parity/sweep_experimental.py --python-source /path/to/exp \
  --output-dir /tmp/negs-sweep

# A released app against the same references:
../spektrafilm-env/bin/python -I scripts/parity/sweep_experimental.py \
  --python-source /path/to/exp --output-dir /tmp/negs-sweep-release \
  --reuse-python /tmp/negs-sweep --skip-f64 \
  --native-binary /Applications/Negs.app/Contents/MacOS/spektrafilm-native \
  --rust-data /Applications/Negs.app/Contents/Resources/data --group films --group papers
```

Result on 2026-10-26 (main `84129ee`): 556/556 within one 8-bit step of
Python (at most 0.04% of pixels touched, ΔE00 ≤ 1.23); f64 ≤ 2.8e-7,
except the diffusion filter (7.8e-6, Python's float32 FFT). Every setting
moves the picture by the same amount in both (largest gap 0.02 ΔE00).
Release 0.7.0 (`a5c360f`, before ticket 16) was far off: Vision3 500T and
200T mean ΔE00 about 16 (much too blue: `T` fell back to D55), daylight films
around 2. Not compared: Negs-only stocks and settings (`settings.use_cat16`,
RAW white balance, Image Size, Post), glare, and grain, which
`grain_compare.py` covers.

## Grain statistics: `grain_compare.py`

Flat grey tiles at four exposures, auto exposure and glare off, the stock's
grain preset on. In the middle of each tile it measures the sRGB output's RMS
in 8-bit steps and the grain's autocorrelation half width, plus the radially
averaged power spectrum, for Python, the GPU CLI and the f64 CLI. Sizes:
640 px (the Python app's preview), 2016 px, and 1008 px enlarged 2× before
the simulation (Python `io.upscale_factor`, Negs `io.photo_long_side_px`).
`--release APP` adds a released app; `--view` compares Negs' full-size render
shown at window size with Python's 640 px preview enlarged to it.

```bash
../spektrafilm-env/bin/python -I scripts/parity/grain_compare.py \
  --python-source /path/to/exp --output-dir /tmp/negs-grain --release /Applications/Negs.app
```

Result on 2026-10-26 (main `84129ee`), Portra 400, Vision3 500T, Provia 100F
and Gold 200: RMS and grain size match Python at every size and with
enlargement (RMS within 0.01, size 0.86–0.87 px for both). Release 0.7.0
had 3–4× the RMS on Portra, Gold and Provia, about 20% less on 500T, and
finer grain (0.70 px). At a 1512 px window, Python's 640 px preview shows
4.4× less grain than a 6048 px Negs render. That is the preview size, not the
engine: Python's full-size scan matches Negs.

## Original methodology and surviving probes

The initial release (`b97a682`) records Python v0.3.2 measurements: decode
RAW once to a shared float TIFF, render both implementations, dump
intermediate f64 arrays, and compare stage errors and final 8-bit pixels.
It reports float differences around `1e-15`, 99.9962% identical pixels,
and a maximum one-level pixel difference. Those are numerical parity
measurements, **not strict byte identity**. Grain textures were explicitly
excluded from pixelwise parity because rejection sampling amplifies small
upstream numerical differences.

The later `81518fb` commit adds frozen 0.3.4 reference assertions, including
full-chain 1×1/4×4 cases and a stage-by-stage gray-card case. These run under
`cargo test`: f64 tolerance `1e-6`, f32 tolerance `3e-3`. Some older
`debug_compare.rs` functions only print reference values and have no
numerical assertions. The release workflow builds packages but does not
run the test suite or live Python comparisons.

The following files remain as historical probes, superseded by the runner:

- `spektra_compare.py`: generates Python output and prints a Rust command;
  it does not run Rust, compare results, or enforce a threshold. The printed
  command omits matching colour-space/effect settings and explicit f64/CPU
  selection.
- `py_bisect.py`: hard-coded `/tmp/cmp`, profile pair, and private stage API.
- `rs_bisect.rs`: not a Cargo target, has obsolete signatures and absolute
  author paths, and downcasts stage values to f32 before writing f64.
- `py_stages.py`: prints hand-computed diagnostics; references an external
  Rust probe that was never checked in and an import removed on experimental.

The former README's May 23 print-stage drift was an intermediate finding,
superseded by the initial release's later measurements. The exact original
RAW fixture, environment lock, and comparison command were not retained,
so those historical figures cannot be reproduced verbatim from this repo.

See [the October 2026 audit and porting baseline](../../docs/upstream-sync.md)
for fresh results and the distinction between upstream `main` and
`experimental`.
