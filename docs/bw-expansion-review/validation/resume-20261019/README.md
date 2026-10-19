# Verification after the branch transfer — 19 October 2026

The [resume handoff](../../../handoff-bw-expansion-resume.md) moved this task to `.claude/worktrees/bw-expansion`. Application code is commit `9579d36c052467cb417fe32ce2504f5c6f3e4ed0`; worktree HEAD at resume was `65410b5`, adding only that handoff. These checks use the existing, unmodified `dist/spektrafilm-bw-expansion-9579d36.zip`, freshly extracted into `/private/tmp/bw-expansion-resume-package-20261019`. They do not use a stale main-checkout exporter.

| Evidence | Result |
|---|---|
| [summary.json](summary.json) | Aggregated results and hashes |
| [package-report.json](package-report.json) | Signature, architecture, deployment target, system dependencies, 120 data files, 840 source files; all pass |
| [parity-pair/report.json](parity-pair/report.json) | 400TX/new Ilford paper, ten live Python cases; max final RGB error 2.675e-13 |
| [parity-doublex/report.json](parity-doublex/report.json) | Double-X/new Ilford paper, ten cases; max final RGB error 2.342e-13 |
| [parity-2302/report.json](parity-2302/report.json) | 400TX/Kodak 2302, ten cases; max final RGB error 3.199e-12 |
| [candidate/report.json](candidate/report.json) | f64 CPU, f32 CPU, f32 Metal and actual native GPU preview/export pass; 33 catalogue entries, target pairing exercised, preview/export differ by at most one 8-bit level |
| [controlled-photos.json](controlled-photos.json) | All nine controlled review renders regenerated with current packaged f64 CPU exporter; every decoded pixel matches the original renders |

Each parity route covers midgray, gradient, neutral ramp, gamut stress and 1024 adversarial pixels, with both LUT modes, checking four stages at absolute tolerance 1e-6 and relative tolerance zero. Input TIFFs, Python arrays, Rust dumps, parameters and execution logs are retained in the route folders. Numba/matplotlib caches are omitted. This is software consistency evidence, not a new physical measurement.

The temporary Python reference survived. `freeze_reference.py` verified all 292 pinned `src/spektrafilm` files against commit `28bf883e1672e884307edc75852549376e13644e`, required only the two identical added profiles, and checked fresh parity arrays. The newly frozen fixture was byte-identical to `crates/spektrafilm-core/tests/fixtures/bw_expansion_python.json` (SHA-256 `3f7d69404a2e06ab621e0171c8329f1ed8aab0f099ad0187a49c57e39962e996`). The fixture was not edited.

The ZIP hash remains `e069c3d64460dba8e0ed5eb9e8cc310559667189f7c2c25bdd4fd02dea64f5de`. Package/source verification ran **before** updating the review documents, against matching build contents. The existing package was preserved; new reports and corrected documentation are separate worktree changes. To repeat the source-snapshot part, use a clean checkout of application commit `9579d36`, since that verifier intentionally compares archived files with the current working tree.

Actual parity invocation, from the B&W worktree (choose a fresh output directory for another run):

```sh
PYTHONDONTWRITEBYTECODE=1 /Users/noahhaasis/Projects/spektrafilm-env/bin/python \
  scripts/parity/check_parity.py \
  --python-source /private/tmp/spektrafilm-bw-reference \
  --rust-binary /private/tmp/bw-expansion-resume-package-20261019/spektrafilm-bw-expansion-9579d36/Spektrafilm.app/Contents/MacOS/spektrafilm-f64 \
  --rust-data /private/tmp/bw-expansion-resume-package-20261019/spektrafilm-bw-expansion-9579d36/Spektrafilm.app/Contents/Resources/data \
  --film kodak_trix400_d76_8min --paper ilford_mgrc_deluxe_grade2 \
  --gamut-off --gamut-stress --adversarial-input \
  --output-dir /private/tmp/bw-expansion-resume-parity-pair-20261019
```

The other two invocations substitute film `kodak_doublex` or paper `kodak_2302`, with output suffixes `doublex-20261019` and `2302-20261019`. Package/native commands:

```sh
PYTHONDONTWRITEBYTECODE=1 /Users/noahhaasis/Projects/spektrafilm-env/bin/python \
  scripts/verify_macos_package.py \
  --zip dist/spektrafilm-bw-expansion-9579d36.zip \
  --output-dir /private/tmp/bw-expansion-resume-package-20261019

PYTHONDONTWRITEBYTECODE=1 /Users/noahhaasis/Projects/spektrafilm-env/bin/python \
  scripts/bw_expansion/check_candidate.py \
  --app /private/tmp/bw-expansion-resume-package-20261019/spektrafilm-bw-expansion-9579d36/Spektrafilm.app \
  --output-dir /private/tmp/bw-expansion-resume-native-20261019
```

No runtime/profile changes were needed. New-base workspace results (157 f32 / 159 f64 passed, zero failures, one external-RAW test ignored) are from the preceding handoff, not a rerun in this follow-up. Manual UI clicking, CUDA, Intel, notarization and physical print measurement remain outside the completed checks. Creative grain variants were not regenerated. Main and the sibling Python checkout were not modified; the branch remains unmerged.
