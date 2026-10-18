# Resuming the B&W expansion — 18 October 2026

Note for Codex. Your previous run hit its usage limit during the last smoke test, just after the results note was written. The test had passed. Claude Code picked the work up. Read [the results note](handoff-bw-expansion-results.md) first; this note only covers what changed afterwards.

**Where the work lives now**

- Branch `bw-expansion`, worktree `.claude/worktrees/bw-expansion`, commit `9579d36` on top of `main` `3f3782d`. It holds all files from your run, byte-identical to what you left in the main checkout.
- The main checkout was cleaned: your six modified files were reverted, and your new files were removed there. The owner's own untracked files (`TODO.txt`, `problem.txt`, the performance-study docs, `.claude/`, earlier `dist/` builds) were left alone. Do not edit the main checkout from this task.
- The commit also includes the three brief files that were untracked before your run: `docs/adding-film-stocks-research.md`, `docs/film-stock-research-sources.json` and `docs/handoff-bw-expansion.md`.

**The base moved under your run**

You validated against `567a431`. Before your work was committed, `main` gained the real-scan 35mm border (`9a84b41`) and crop (`19d1d2b`, merged as `3f3782d`). Your `border.rs` test hunk still applies. That commit deletes the per-stock `kodak_trix.png` strips and adds `borders/35mm/material.png`. So the feature package bundles 120 data files rather than your candidate's 121: it has the 9 B&W files plus `material.png`, but not the two `kodak_trix.png` strips.

**Re-verified on the new base (commit `9579d36`)**

| Check | Outcome |
|---|---|
| `cargo test --workspace --locked` | 157 passed, 0 failed, 1 ignored (`camera_raw_smoke`) |
| Same with `--features spektrafilm-cli/precision-f64` | 159 passed, 0 failed, 1 ignored |
| `rustfmt --check --edition 2024` on changed Rust files | Only two pre-existing complaints in `border.rs` (lines ~126 and ~374, from the border commit, not yours); left untouched |
| `git diff --check` | Clean |
| New package `dist/spektrafilm-bw-expansion-9579d36.zip` (in this worktree) | `verify_macos_package.py` passed: 840 source files, 120 data files |
| `scripts/bw_expansion/check_candidate.py` on that package | Passed; f64 CPU, f32 CPU and Metal results match your run (Metal max 8.539e-6). Native helper uses `wgpu (GPU)`, shows 33 catalogue entries, and preview/export differ by at most one 8-bit level |
| ZIP SHA-256 | `e069c3d64460dba8e0ed5eb9e8cc310559667189f7c2c25bdd4fd02dea64f5de` |

A matching build of plain `main` is in the main checkout at `dist/spektrafilm-main-3f3782d.zip`, and its audit passed. Its verifier predates your `SpektrafilmApp` fix, so that audit did not cover the native helper.

**Not redone on the new base**

- The live Python parity run (`scripts/parity/check_parity.py`) against `/private/tmp/spektrafilm-bw-reference`. The frozen-fixture regressions in `bw_expansion.rs` and the packaged-app check above did pass. `/private/tmp` may have been cleared; rebuild the reference from Python commit `28bf883` with `git archive` if needed.
- The review renders in `docs/bw-expansion-review/`. They were made with the old base. Rendering code did not change, and no `scripts/bw_expansion` script uses a border, so the 35mm-border change does not affect them.
- Manual SwiftUI clicking, CUDA, Intel and notarisation, as before.

**Practical traps**

- The owner's git uses an external diff tool. Use `git diff --no-ext-diff` whenever you need a patch.
- `scripts/package_macos_app.sh` deletes `DIST_DIR`. Always use a new directory name.
- `SOURCE.tar.gz` packs the whole `docs/` tree, including untracked files. In this worktree that is only branch content.

**Suggested next steps, in order**

1. If wanted, rerun live parity on the new base for the three pairings in the results note, and record the result in `docs/bw-expansion-review/validation/`.
2. Merging `bw-expansion` into `main` is the owner's decision. Don't merge it unasked.
3. The product work is unchanged from the results note. Get a calibrated grade-2 step-wedge print and paper reflectance spectra, then measured Tri-X granularity and sharpness, before adding another film or development time.
