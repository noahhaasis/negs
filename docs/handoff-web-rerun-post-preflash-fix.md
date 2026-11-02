# Next-session handoff: re-run the browser on all 18 preset cases after the preflash fix

## Goal

Confirm that the browser (Chrome, WebGPU) now matches native Reference CPU on
all 18 preset cases (IMG_0316 + DSC_2498 × 9 presets). Produce an up-to-date
review site. This is a **verification run**. Nothing is expected to need fixing,
and no production code should change.

## Why it's still open

The GPU preflash fix (commit `09a66ad`, "Scale GPU preflash by the folded print
exposure"; older docs call it `5aea2d1` from before a rebase) is verified like
this:

- **Native wgpu vs CPU, deterministic track:** all 18 cases, max scan |Δ| ≤ 1e-5
  (`target/img0316-cooked-gpu-debug-20261031/matrix/deterministic`).
- **Browser vs native CPU:** only 3 cases (DSC_2498-09, IMG_0316-01,
  IMG_0316-09), in `target/img0316-cooked-gpu-debug-20261031/web-run/`.
  Deterministic COOKED max scan |Δ| is 1.8e-6 / 1.5e-6.

The other 15 browser captures in the frozen review
(`target/preset-accuracy-post-raw-fix-20260724-01`) predate the fix. Seven of the
nine presets use preflash (02–07 and 09). There the old bug cost about 1 ΔE00
(02–07) and 6–10 (09, COOKED). So the frozen review's web columns are stale
for those cases.

Background: [handoff-img0316-cooked-debug.md](handoff-img0316-cooked-debug.md)
(Resolution) and [handoff-img0316-raw-decode-diff.md](handoff-img0316-raw-decode-diff.md)
(Findings).

## What the results should show

1. **deterministic** (grain and glare off, identical float fixture). Web vs
   installed-cpu and source-cpu: max scan |Δ| about 1e-5 or less, and RGB8 at
   most 1 code apart, on **all 18**. Before the fix: up to 0.247 scan, 9.86 ΔE00.
   This is the pass/fail track.
2. **same-input** (same fixture, grain on). Web vs CPU should drop to grain
   noise. In the frozen run that's the 0.15–0.25 mean ΔE00 seen on 01 and 08
   (no effective preflash), versus about 1.0 for 02–07 and 6.2–9.9 for 09.
3. **application** (full RAW). Web vs native CPU **won't** be small on
   IMG_0316, because the decoders differ (web rawler, native Apple; deferred in
   `../negs/tickets/19-raw-colour-reference.md`). Check the browser against
   **native CPU on rawler's own pixels** instead. Those references already exist
   for all 18 cases at
   `target/img0316-raw-decode-20261031/engine/<case>/rawler/output.png`. With
   16×16 pooling (to remove grain), the browser should be within about 0.1–0.2
   ΔE00 of them. Reference values: 0.10 on IMG_0316-01 and 0.05 on IMG_0316-09.
   - Skip this check for case 08. The TRIX preset uses custom 5500 K white
     balance, but that reference TIFF was decoded at daylight. Use the
     deterministic and same-input tracks for 08.
   - The cross-check is
     `target/img0316-raw-decode-20261031/scripts/pooled.py`. It hard-codes the
     web-run and frozen paths, so point it at the new run's
     `renders/application/<case>/web/output.ppm`.
   - The review's own per-pixel metric includes grain noise. For DSC_2498 the
     decoders agree, so expect web vs CPU there to fall to the DSC-01 level
     (about 1.4 per pixel), and to about 3.8 on 06 (VERITA amplifies grain).

## How to run it

Read `scripts/accuracy/README.md` first. Short version:

1. **Rebuild the web package from the current tree** and note the commit:
   `PATH="$HOME/.cargo/bin:$PATH" ./web/build.sh --threads`. This needs
   wasm-bindgen 0.2.121 from `~/.cargo/bin`. `web/pkg` was last rebuilt
   2026-10-31 16:35, but rebuild anyway so `provenance.json` records exactly
   what was served. `prepare` hashes `web/pkg` and `web/pkg-threads`, and
   `browser.mjs` refuses to run if the served assets differ.
2. **Server:** a `node web/serve.mjs` was already listening on 8137 at handoff
   time. Check it serves this checkout (`lsof -iTCP:8137 -sTCP:LISTEN`) and
   don't start a second one. `web/cdp.mjs` makes a fresh Chrome profile per
   run, so stale service workers aren't a problem.
3. **New run directory, engines chosen deliberately:**
   ```bash
   PY="$HOME/Projects/spektrafilm-env/bin/python"
   RUN="$PWD/target/preset-accuracy-post-preflash-fix-$(date +%Y%m%dT%H%M%S)"
   "$PY" -I scripts/accuracy/run.py prepare --output-dir "$RUN" \
     --python-source "$HOME/Projects/spektrafilm" --preset-dir "$HOME/Downloads/Presets" \
     --engines web python installed-cpu source-cpu
   "$PY" -I scripts/accuracy/run.py render --output-dir "$RUN" --tracks application
   "$PY" -I scripts/accuracy/run.py render --output-dir "$RUN" --tracks same-input deterministic
   "$PY" -I scripts/accuracy/audit_params.py --run "$RUN"
   "$PY" -I scripts/accuracy/compare.py --run "$RUN" --output-dir "$RUN/review-01"
   "$PY" -I scripts/accuracy/freeze.py --run "$RUN" --review "$RUN/review-01"
   python3 -m http.server 8140 --bind 127.0.0.1 --directory "$RUN/review-01"
   ```
   - **Leave out the native GPU engines** (`installed-gpu`, `source-gpu`). The
     harness's native binaries are the preserved Negs.app 0.12.0 and a
     `cd3bc2b` source build, both from before the fix. They would still show
     the old bug and confuse the review. The preserved CPU binaries are still
     valid references, because the fix didn't touch CPU output. Native post-fix
     GPU is already verified on all 18 cases. If you want it in the site anyway,
     you'd need a new preserved snapshot (`--native-dir` with a `manifest.json`).
     `run.py` hard-codes the label "Preserved source build cd3bc2b", so change
     the label first.
   - **Python** is needed: its RAW loader creates the same-input and
     deterministic fixtures, and it fills the Python panels. Its full-resolution
     application renders are the slow part, taking minutes per case. Python
     results aren't expected to change.
   - Before the full run, try a smoke run:
     `compare.py --tracks application --case DSC_2498-01` on a separate output
     dir. It checks that compare and site cope with the reduced engine set.
     The README says missing engines are reported as failures, not passes;
     confirm that unselected engines aren't flagged that way.
4. Optionally, check the browser against rawler-on-native as described in
   item 3 of the previous section.

## Preservation and traps

- Write only to new directories. **Don't** modify the frozen run
  `target/preset-accuracy-post-raw-fix-20260724-01`,
  `target/img0316-cooked-gpu-debug-20261031/`, or
  `target/img0316-raw-decode-20261031/`. Don't modify the preserved app or
  binaries under `target/preset-accuracy/` either.
- The harness has no export cache. Rendering into an existing engine/case
  directory fails by design. If you need a redo, make a new run.
- **Disk:** about 49 GB free at handoff. A full review needs about 10–20 GB.
  `target/img0316-raw-decode-20261031/sweep/` holds about 12 GB of regenerable
  float arrays that could be deleted, but ask the user first.
- The working tree has uncommitted RAW and web-sample changes that aren't
  yours. Don't stash, reset or commit them. `prepare` records them in
  `dirty.patch`.
- Don't commit private pixels or presets. Don't deploy or publish anything,
  and leave `public/` alone.
- Python: `~/Projects/spektrafilm-env/bin/python`. Quote the RAW paths, since
  they contain spaces and `@`.

## Expected hand-back

- A table for all 18 cases. Deterministic: web vs CPU max scan |Δ| and RGB8 max
  code. Same-input: mean ΔE00. Application: mean ΔE00 per pixel and pooled,
  plus pooled ΔE00 against the rawler-on-native references.
- A clear statement of whether the browser matches CPU on all 18 deterministic
  cases. If any case doesn't, give it with its numbers and **stop**: that would
  be a real new bug, so localize it as in the cooked-debug handoff and don't
  fix it unasked.
- The new run and review paths, the commit and the `web/pkg` hashes used, and a
  note that the native GPU columns were left out on purpose.
