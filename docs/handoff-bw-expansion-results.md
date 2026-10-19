# B&W expansion results — 18 October 2026

**Sharing follow-up, 19 October.** The PM requested a commit and merge into `main`, retaining this review for later feedback. Separate commit `aa20096` adds the generated `KODAK 400TX` stock label to both 35mm preview formats and the export renderer; 13 border/rendering checks passed. The latest sharing artifacts are [the app](../dist/spektrafilm-bw-sharing-20261019/Spektrafilm.app), [ZIP](../dist/spektrafilm-bw-sharing-20261019.zip), [package audit](../dist/spektrafilm-bw-sharing-20261019-package-report.json) and [validation files](../dist/spektrafilm-bw-sharing-20261019-validation/). Earlier statements below about unmerged status, generic 400TX borders and candidate paths are historical.

**Current delivery, updated 19 October.** Work lives on branch `bw-expansion` in `.claude/worktrees/bw-expansion`. Application commit `9579d36` includes the newer border/crop base; `65410b5` adds the [resume handoff](handoff-bw-expansion-resume.md). The current [app](../dist/spektrafilm-bw-expansion-9579d36/Spektrafilm.app) and [ZIP](../dist/spektrafilm-bw-expansion-9579d36.zip) supersede the earlier candidate referenced in the historical record below. The main checkout was left untouched during this resume, and no merge was performed.

The resume closed the outstanding live-parity check: **30/30 cases pass** using the extracted current package and pinned Python `28bf883`. All 292 Python source/data files were checked against Git; only the two new profiles are added. Regenerating the frozen fixture from the fresh Python outputs produced identical bytes. Maximum final RGB errors are 2.675e-13 (new pair), 2.342e-13 (Double-X/new paper), and 3.199e-12 (new film/2302), all below 1e-6. Native Metal preview/export passed again, with at most one encoded 8-bit level difference. All nine controlled review photographs were rerendered and match the existing images pixel-for-pixel; creative grain variants were not rerendered.

The current ZIP was audited again before these documentation updates: **four executables, 120 data files, 840 source files**, signature and dependency checks passed. SHA-256: `e069c3d64460dba8e0ed5eb9e8cc310559667189f7c2c25bdd4fd02dea64f5de`. Its contents were not modified. Workspace results on the new base are **157 f32 / 159 f64 passed, zero failures, one ignored**, as recorded by Claude Code in the resume handoff; those full suites were not redundantly rerun during this documentation/live-parity follow-up. The new base also has pre-existing formatting findings in `border.rs`.

Persistent fresh reports, raw arrays, exact commands and reproduction notes: [validation/resume-20261019](bw-expansion-review/validation/resume-20261019/README.md). The package source snapshot corresponds to the build revision; these later review/report additions remain in the worktree. For repeat source-snapshot verification, use the clean build revision because the verifier compares archived documents against the current checkout. No material calibration or runtime code changed during this resume.

**Original implementation record follows.** Its earlier test counts, package paths, workspace location and preservation statements describe the initial run, not the subsequent branch transfer or current delivery above.

Implemented one conventional camera film and one experimental fixed-grade photographic paper, with source extraction, reproducible preparation, independent Python parity, physical checks, photographic comparisons and a fresh macOS candidate. Start with [the visual review](bw-expansion-review.md). No second film or additional measured development time was attempted.

The run started at 15:26:19 UTC with an initial 17:26:19 UTC target. The conversation was interrupted and resumed around 19:03 UTC. This delivery therefore does not claim 120 minutes of uninterrupted work. Final effort focused on completing the first pair and verification.

**Implemented materials and scope**

| ID | Identity and processing | Integration |
|---|---|---|
| `kodak_trix400_d76_8min` | Kodak 400TX 35 mm, D-76, 8 minutes, 20°C, large tank, agitation at one-minute intervals; stock dilution inferred from unqualified D-76 caption | B&W camera-negative catalogue entry; default `target_print` is the new Ilford paper |
| `ilford_mgrc_deluxe_grade2` | Fifth-generation Ilford Multigrade RC Deluxe/Portfolio curve, filter 2, Multigrade 1+9, one minute, 20°C | B&W paper entry, explicitly named “experimental neutral”; normalized neutral-negative route only |

Existing `kodak_doublex`, `kodak_trix` (7266 reversal), `kodak_2302` and all colour profiles are unchanged. There are now 33 catalogue entries: three B&W filming choices and two B&W printing choices. No rendering-engine algorithm was changed. The paper uses the existing scanner with reflection density counted once; analytical tests cover this convention.

**Files and artifacts**

| Location | Contents |
|---|---|
| `data/profiles/{kodak_trix400_d76_8min,ilford_mgrc_deluxe_grade2}.json` | Integrated coherent fitted models, sampled densities, sublayers, sensitivity, baseline, metadata |
| `data/profile-sources/bw-expansion/` | Source CSVs, raw graph/pixel coordinates, recipe, source-document hashes and detailed README |
| `scripts/bw_expansion/` | PDF extraction, profile preparation, review rendering, diagnostic checks, independent fixture freezing, packaged binary checks; pinned Python dependencies |
| `crates/spektrafilm-core/tests/bw_expansion.rs` | Four substantive regressions: independent Python cases, single reflection-density conversion, normalized neutral-negative spectral-weight invariance, loaded models against manufacturer points |
| `crates/spektrafilm-core/tests/fixtures/bw_expansion_python.json` | Three independent Python cases, full inputs/expected RGB and pinned-tree provenance |
| `crates/spektrafilm-native/src/main.rs`, `crates/spektrafilm-gui/src/main.rs` | Catalogue test updates for distinct negative/reversal/paper identity and new counts |
| `data/borders/35mm/{full,half}/kodak_trix400_d76_8min.png`, `data/borders/README.md` | Copies of generic 35 mm borders to satisfy supported-film coverage; no fabricated manufacturer edge text |
| `scripts/package_macos_app.sh` | Package README explains the new pair and scope |
| `scripts/verify_macos_package.py` | Includes native helper when Swift executable is named `SpektrafilmApp`; previously only the alternate name triggered its audit |
| `docs/bw-expansion-review/` | Actual photos/credits, 15 full-size renders, comparison sheets, three source overlays, exposure bracket, diagnostic curves, exact recipes, hashes, raw parity evidence and test logs |

Baseline Rust was `567a43132f64538ec6d877c8d015e319139b80b3`. The clean Python reference is `28bf883e1672e884307edc75852549376e13644e`, extracted into `/private/tmp/spektrafilm-bw-reference` using `git archive`. Only the two new profiles were added to that temporary reference. The freezer checked all 292 pinned `src/spektrafilm` files against Git objects, required the exact two added profiles, then checked all comparison arrays before freezing Python output. The user's sibling checkout was not changed.

**Evidence and assumptions**

The authoritative source record is [data/profile-sources/bw-expansion/README.md](../data/profile-sources/bw-expansion/README.md) and its `recipe.json`. Primary documents are Kodak F-4017 October 2021 (PDF p8/p9) and HARMAN/Ilford Multigrade RC Papers October 2020 (p3/p4). URLs, hashes and processing conditions are recorded there. The current public profile creator was not located; a small documented preparation path was implemented instead.

Film data cover total density 0.288–2.088. Base density 0.28, high-density continuation toward 3.5, the rendering exposure anchor and sensitivity truncation above 665 nm are explicit assumptions. Paper reflection density 0.0524–2.0974 comes from the actual grade-2 trace. There is no calibrated paper spectral sensitivity or reflectance spectrum in this source. An arbitrary integration weight cancels for neutral negatives with normalized print exposure; it does not substitute for measured filtration/speed/colour-negative behaviour. Paper remains experimental. Generic grain is a separate creative recipe, not measured stock granularity.

Maximum fitted source error: film 0.0047017 D (gate 0.01), paper 0.0166441 D (gate 0.03). Tolerances reflect graph line thickness and tracing uncertainty. Both prepared profiles regenerated identically; see [reproducibility.json](bw-expansion-review/validation/reproducibility.json). Profile SHA-256 values:

```text
7fee413efc7efd99a92daa38df641484c5c82d1809f0c04c3bdd835394171dd9  kodak_trix400_d76_8min.json
cd87207e6c58a30b4785296429058ece6f3db35feb862a1d6ea58b1b0a24adcf  ilford_mgrc_deluxe_grade2.json
```

**Validation outcomes**

| Check | Actual outcome and persistent evidence |
|---|---|
| Baseline workspace f32 | 144 passed, zero failures, one ignored; `validation/workspace-baseline-f32.log` |
| Final workspace f32 | 149 passed, zero failures, one ignored; `validation/workspace-f32.log` |
| Final workspace with CLI f64 feature | 151 passed, zero failures, one ignored; `validation/workspace-f64.log` |
| Python parity comparator tests | Seven passed; `validation/parity-unittests.log` |
| Live Python parity | Ten cases each for new pair, Double-X/new paper, new film/2302; all stages at absolute tolerance 1e-6, relative tolerance zero; `validation/parity-{fit,doublex,2302}/report.json` and raw arrays |
| Independent physical/edge cases | 256-point neutral ramp −14 to +10 EV, coloured patches, exact black, near-black, HDR to 1e6; all stages finite, neutral output monotone, LUT on/off tested; `diagnostics/report.json` |
| Reflection check | Maximum Y error 3.8081e-5 against independent `10^(-D_total)` calculation, gate 1e-3; base and image density counted once |
| Candidate packaged binaries | f64 CPU, f32 CPU, f32 Metal checked against independent Python; maximum Metal RGB error 8.5391e-6, gate 0.003; `validation/candidate/report.json` |
| Packaged native helper | Actual backend `wgpu (GPU)` on Apple M1 Pro; correct catalogue, 512×512 portrait render/save; f64 export without explicit `--paper` exercises target pairing; maximum difference one encoded 8-bit level |
| Formatting | Changed Rust files pass `rustfmt --check --edition 2024`; `git diff --check` passes. Full-workspace fmt reports pre-existing changes in unchanged CPU/CUDA backend files |

The ignored test is `camera_raw_smoke`, which requires external files in `SPEKTRAFILM_TEST_RAWS`; no camera-RAW coverage is claimed. Early new-profile runs exposed the stale 31-stock catalogue count and the need for coherent fitted models in Python. Those were corrected; existing numerical golden values were not changed. No live CUDA run, Intel build, notarization or manual SwiftUI clicking was performed. Native selector data and the actual packaged preview/export backend were exercised through their protocol.

**Reproduction commands**

Run from the Rust repository root. Preparation uses the pinned dependencies in `scripts/bw_expansion/requirements.txt`; parity additionally uses the upstream Python runtime dependencies described in `scripts/parity/README.md`. This run used the existing `../spektrafilm-env/bin/python` environment, with PyMuPDF supplied from `/private/tmp/spektrafilm-bw-tools` for extraction only.

```sh
python scripts/bw_expansion/prepare.py --output-dir target/bw-profiles
python scripts/bw_expansion/render_review.py --binary target/release/spektrafilm-f64
python scripts/bw_expansion/diagnostics.py --binary target/release/spektrafilm-f64

cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64
PYTHONDONTWRITEBYTECODE=1 ../spektrafilm-env/bin/python \
  -m unittest discover -s scripts/parity -p 'test_*.py'

# After extracting the pinned Python commit and copying ONLY the two new profiles
# into its src/spektrafilm/data/profiles directory, use a fresh output directory:
../spektrafilm-env/bin/python scripts/parity/check_parity.py \
  --python-source /private/tmp/spektrafilm-bw-reference \
  --film kodak_trix400_d76_8min --paper ilford_mgrc_deluxe_grade2 \
  --gamut-off --gamut-stress --adversarial-input \
  --output-dir /tmp/bw-parity-new-run

# Repeat with Double-X/Ilford and 400TX/2302 to isolate the two materials.
../spektrafilm-env/bin/python scripts/bw_expansion/freeze_reference.py \
  --python-source /private/tmp/spektrafilm-bw-reference --git-source ../spektrafilm \
  --parity-dir /tmp/bw-parity-new-run
```

The photograph settings are encoded sRGB input/output, auto exposure off, zero exposure adjustment, no gamut compression, identical settings across images. Controlled comparisons disable texture and optical effects. Balanced/punchy recipes are described in the review, saved separately, and do not silently modify the stock calibration. All actual commands and hashes are in `render-manifest.json`.

**Candidate package**

Built in the previously unused directory `dist/spektrafilm-bw-candidate-20261018-1903`, using:

```sh
DIST_DIR=dist/spektrafilm-bw-candidate-20261018-1903 bash scripts/package_macos_app.sh
```

Do not rerun that command against this existing directory: the packager removes its output directory. Choose a new unique directory for another build. This candidate includes the SwiftUI app, native preview helper, f32 CLI, f64 CLI and exact bundled profiles. It is a local Apple Silicon candidate, ad-hoc signed. Build log: `docs/bw-expansion-review/validation/package-build.log`.

The final `SOURCE.tar.gz` is refreshed from tracked source paths plus only this run's new files, excluding unrelated pre-existing untracked work and caches. The ZIP is then regenerated and extracted for audit. The final audit command is:

```sh
../spektrafilm-env/bin/python scripts/verify_macos_package.py \
  --zip dist/spektrafilm-bw-candidate-20261018-1903.zip \
  --output-dir /private/tmp/bw-expansion-package-verified-final
```

The extracted candidate passed the package audit: all four executables, 121 bundled data files and 832 source files were checked. The final machine-readable `dist/spektrafilm-bw-candidate-20261018-1903-package-report.json` records signature status, minimum macOS, architectures, system-library dependencies, every data-file hash, source-file matching and ZIP hash. It is outside the archive to avoid embedding the archive's own hash in its contents. Packaged CPU/Metal/native behaviour can be repeated with `scripts/bw_expansion/check_candidate.py --app /path/to/Spektrafilm.app --output-dir /new/output/directory`.

Unrelated initial untracked files were preserved: `.claude/`, `TODO.txt`, `problem.txt`, earlier `dist/` contents, the research/brief files and performance-study artifacts. No reset, cleanup, commit or publication was performed. No unrelated untracked files are included in the final source snapshot.

**Next concrete step**

Keep the balanced recipe as the recommended preview look and keep the paper experimental. Obtain a calibrated grade-2 step-wedge print and spectral reflectance for paper white, intermediate gray and maximum black under the documented process; then compare matched real negatives/prints with these renders. That addresses tint and complete-route fidelity. Measured Tri-X granularity/sharpness is the next texture improvement. Another film or development time should follow those checks, with its own documented curve, rather than expanding names first.
