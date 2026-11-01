# Next-session handoff: IMG_0316 web vs native RAW difference

## Goal

Make the **web** render of the Canon RAW `IMG_0316.CR2` match **native
macOS**. The user treats native Reference CPU (and web CPU) as the accuracy
target. After the GPU preflash fix (commit `5aea2d1`, see
[handoff-img0316-cooked-debug.md](handoff-img0316-cooked-debug.md) → Resolution),
the remaining visible web/native gap on this photo is **not** in the film
engine. It comes before the engine, in RAW decode/development.

Start by localizing it. Don't change the decoder policy or a default until the
cause is demonstrated and the user has approved the tradeoff (the earlier
accuracy handoff makes the same rule).

## Findings (2026-10-31): steps 1–3 done, step 4 awaits the user

**Cause:** Apple's built-in camera colour profile for the Canon EOS 6D.
rawler isn't wrong here. rawler, LibRaw, Canon's own metadata, and Apple itself
reading Adobe's DNG matrices all agree with each other. Apple's built-in CR2
profile is the one that differs. The web/native gap is entirely the decoder.
No code or defaults were changed.

Outputs are in `target/img0316-raw-decode-20261031/` (private pixels,
about 14 GB, mostly the sweep's float arrays). Scripts are in its `scripts/`.

### 1. Decode-only difference (daylight WB, linear sRGB, unclamped float)

- Mean linear RGB on IMG_0316: Apple `[0.0457, 0.0150, 0.0101]`, rawler
  `[0.0501, 0.0151, 0.0086]`. rawler is 10% redder and 15% less blue,
  which is the sign of the web/native gap. On DSC_2498 they agree within 0.1%.
- Least-squares fit over 8 px blocks, unclipped, so `apple ≈ M · rawler`:

  ```text
  IMG_0316  [[0.924 -0.053  0.029]     DSC_2498  [[1.000  0.000 -0.001]
             [0.016  0.971 -0.050]                [0.000  0.999  0.001]
             [0.008  0.050  1.047]]               [0.000  0.000  0.999]]
  ```
  The relative residual is 1–2% (0.4% for the DNG). A diagonal-only (WB) fit
  doubles the residual. An affine offset term is about 2e-4, so black level
  isn't the cause. Alignment and orientation are identical: same size, and a
  LibRaw crop offset of (12, 12) was confirmed by correlation.
- Shape of M: the neutral isn't preserved. What rawler develops as D65 white,
  Apple renders at **7563 K**. Apple's own "D65" white is about 5740 K in
  Adobe-matrix terms. With the neutral restored, what remains is a small hue
  rotation, about 1.6°. So the difference is mostly white point, plus a minor
  matrix term.

### 2. The cause, tied to metadata

| Interpretation of IMG_0316's camera multipliers | What it says |
|---|---|
| rawler: `vendor/rawler/data/cameras/canon/6d.toml` D65 matrix | = Adobe ColorMatrix2 = LibRaw `rgb_xyz_matrix` |
| LibRaw develop vs rawler (3×3 fit) | identity within about 1% |
| Canon ColorData (ver 10) as-shot WB, RGGB 2036/1024/1024/1697 | **5189 K** (Canon) |
| Adobe DNG matrices (DNG-SDK solve, validated against Apple's own report) | **5156 K**, Duv +0.0024 |
| Apple, IMG_0316 converted by Adobe DNG Converter (decoder `8.dng`) | 5155 K, tint +7.5 |
| Apple, original CR2 (decoder `8`, built-in profile) | **5908 K**, tint +13.1 |

Apple follows a DNG's embedded matrices only when it has no built-in profile
for the camera (`decoderVersion` `8.dng`). For a known camera (`8`), it swaps
in its own profile, even for a DNG.

Decode-level ΔE00, exposure-normalized 8 px blocks: rawler CR2 vs Apple CR2
**2.22**; Apple CR2 vs Apple-on-Adobe-DNG 2.01; rawler CR2 vs Apple-on-Adobe-DNG
0.54, which is the remaining forward-matrix and hue/sat difference.

### 3. Proven through the engine (native f64 CPU, frozen run's params)

Each decoder's linear pixels go in as an untagged float TIFF. That path is
bit-identical to the direct RAW path: ΔE 0.000. Scores are mean ΔE00 after
16×16 pooling, which removes grain. Per-pixel results are in the logs.

| case | rawler vs Apple | rawler·M vs Apple | post-fix web vs rawler-on-native |
|---|---|---|---|
| IMG_0316-01 | 2.59 | 0.21 | **0.10** |
| IMG_0316-06 | 3.35 | 0.41 | (frozen web predates the preflash fix) |
| IMG_0316-08 B&W | 0.14 | 0.11 | — |
| IMG_0316-09 | 1.34 | 0.12 | **0.05** |
| DSC_2498-01 (control) | 0.18 | 0.18 | 0.17 |
| DSC_2498-06 (control) | 0.50 | 0.50 | — |

Per pixel, IMG_0316-01 rawler-on-native vs Apple is 2.702, the same as the
measured web vs native CPU (2.702). Mean output RGB `[127.0, 34.4, 49.2]` is
web's exactly.

Side notes:

- Per-pixel ΔE has a large grain floor. Any input difference, demosaic
  included, reshuffles the grain realization. Example: the DNG, whose colour
  is identical, still scores 1.39 per pixel on 01 and 3.8 on 06. Use pooled
  metrics for decoder questions.
- Correction: native CPU is deterministic for every preset, TRIX included
  (repeat renders are byte-identical). The 0.89 "direct vs TIFF" on 08 came
  from WB: the TRIX preset uses **custom 5500 K**, not daylight (the other
  eight use daylight), and these TIFFs were decoded at daylight. So the 08
  rows above compare daylight decodes, not the preset's WB.

### Wider sweep: the difference isn't Canon-specific

Grid-pooled decode-level ΔE00. LibRaw is the arbiter. It has no matrix for
the Leica DNGs, so those were checked against the file's own tags.

| file | Apple vs rawler | LibRaw vs rawler | LibRaw vs Apple |
|---|---|---|---|
| IMG_0316.CR2 (Canon 6D) | 3.02 | 0.53 | 3.18 |
| IMG_4416.CR2 (Canon) | 9.81 | 0.68 | 9.83 |
| 3908653826.rw2 | 4.99 | 0.76 | 5.11 |
| 0955097116.raf | 4.32 | 0.19 | 4.33 |
| Z 6/DSC_5048.NEF | 2.18 | 0.73 | 2.42 |
| DSC_2498.dng | 0.07 | 0.40 | 0.43 |
| Leica Q3 DNG ×3 | 8.4–10.8 | — | — |

Leica Q3: rawler uses the DNG's ColorMatrix2 (D65), as the DNG spec says. The
file's matrices put the as-shot neutral at 7719 K / 7848 K. Apple (decoder
`8`, built-in profile) reports 4801 K, tint −42 / 5611 K. So on the macOS
app, most proprietary RAWs, and DNGs from Apple-known cameras, develop to
different colours from the web. IMG_0316 is a mild case.

### Browser vs Python, and what else differs (2026-10-31, later)

- **Engine vs Python: exact.** Re-running Python experimental's deterministic
  track with its runtime LUTs off (`use_enlarger_lut`, `use_scanner_lut`,
  `use_convert_lut` = false; fast stats left on) matches the frozen native CPU
  float scans on **all 18 cases**, with max |Δ| < 1e-4. The 0.26–0.43 RGB8
  ΔE that the frozen review shows for Python vs CPU is Python's own
  approximations: its 17³/33³ LUTs (0.02–0.12 ΔE on float scans) and its RGB8
  writer truncating where Rust rounds (the rest). Neither is a Negs bug.
  Outputs: `python-nolut/`.
- **Browser vs Python application.** The frozen review couldn't compare these
  (LibRaw's frame is 3670×5496). Here the post-fix browser is stood in for by
  native CPU on rawler pixels, which matches web within 0.10 pooled ΔE. On a
  48×72 grid, browser vs Python is **0.50–0.70 mean on all 18 cases**. Apple
  native vs Python is 1.5–3.8 on IMG_0316, and 0.5 on DSC_2498, where the
  decoders agree. That 0.5 floor is LibRaw vs rawler demosaic plus Python's
  truncation: Python's mean RGB is 0.5–1 code lower everywhere. The p99 of
  2–5 comes from edge cells, where the frames differ by about 0.4%.
- **Grain** strength matches. High-pass RMS on IMG_0316-01 and -09 is the same
  for web and native CPU on the same pixels, to 0.01 code per channel. The
  per-pixel residual is a different grain realization with the same
  statistics.
- Still unverified: the post-preflash-fix browser has only been re-captured
  for 3 of the 18 cases (`web-run/`). The other 15 frozen web captures predate
  the fix. Presets 02–07 and 09 all have preflash, so the frozen review's web
  numbers for those are stale.

### 4. Decision for the user (not yet made)

Ground truth for saturated colours would need a colour chart, which we don't
have. The neutral axis is checkable, though, and Canon's metadata sides with
the Adobe/rawler calibration.

1. **Recommended: make rawler the colour reference on all platforms.** macOS
   Auto would prefer rawler, with Apple as an explicit option and fallback.
   Web and native would then match by construction (about 0.1 ΔE pooled), and
   colours would follow the file's or Adobe's calibration. rawler was also
   faster here (CR2 324 ms vs Apple 2217 ms). Costs: a visible colour change
   for existing macOS users and presets tuned on Apple colour; loss of Apple's
   demosaic and highlight reconstruction (Apple's max is 1.61 vs rawler's
   1.87/1.23 on clipped pixels); Apple's broader format coverage only as a
   fallback. Image quality (demosaic, highlights) needs its own check before
   switching.
2. Keep Apple's demosaic and correct its colour toward the calibration. Not
   recommended: Apple's profile is opaque, per camera and partly nonlinear.
   A fitted 3×3 leaves 0.2–0.4 ΔE on this photo, and nothing generalizes.
3. Make the web match Apple. Not feasible: it would need Apple's proprietary
   per-camera profiles.
4. Accept and document it: native = Apple's rendering, web = calibrated.
   Accuracy harnesses must then compare engines on identical decoded pixels
   (as above), and not RAW-to-RAW across platforms.

Step 5 (fix, regression, the 18-case re-run) waits on this choice.

## What is established

**The engine agrees.** With identical float pixels as input (the deterministic
track, grain and glare off), web WebGPU and native CPU now match within
2e-6 max scan error on IMG_0316 (`KODACHROME` and `COOKED` were checked). So any web/native
difference in a full application export comes from what each side feeds
the engine.

**The application gap is specific to the Canon RAW and shows up in chroma.** From
the frozen run, application track, web vs installed **Fast GPU** (both GPU,
so the old preflash bug cancels between them), mean ΔE00:

| Preset | DSC_2498 (Nikon DNG) | IMG_0316 (Canon CR2) |
|---|---|---|
| 01 KODACHROME | 0.40 | **2.64** |
| 02 P800 push2 | 0.64 | 1.82 |
| 03 P800 vintage | 0.60 | 1.94 |
| 04 GOLD200 | 0.43 | 1.34 |
| 05 P400 natural | 0.24 | 1.89 |
| 06 VERITA | 0.71 | **3.38** |
| 07 C200 modern | 0.30 | 1.78 |
| 08 TRIX (B&W) | 0.15 | 0.24 |
| 09 COOKED | 0.22 | 1.88 |

Source: `target/preset-accuracy-post-raw-fix-20260724-01/review-full/metrics.csv`,
rows `application,<case>,web,installed-gpu`. Grain is on in this track, so
roughly 0.2–0.5 ΔE is noise.

Re-measured after the fix, web vs native **CPU**: IMG_0316-09 is 1.41 and
IMG_0316-01 is 2.70. In IMG_0316-01 the web mean RGB is about `[127.0, 34.4, 49.2]`
and the CPU's is `[123.3, 36.6, 55.5]`: web is redder and less blue. These outputs are in
`target/img0316-cooked-gpu-debug-20261031/{web-run,matrix}/application/`.

**Reading of the evidence (a hypothesis, not yet shown):** the B&W preset
nearly agrees while every colour preset differs, and center-weighted auto
exposure normalizes overall level. Together that points to a **colour**
difference between the decoders: camera matrix, white-balance multipliers,
daylight WB interpretation, or the camera → linear sRGB transform. Tone
or exposure differences look less likely. The Nikon DNG is close, so
the cause may be specific to CR2 or to Canon metadata.

**Decoders and dimensions:**

- Web: `Camera RAW, developed by rawler`, labelled `Linear sRGB
  (scene-referred)`, output 3648×5472.
- Native (installed and current source on macOS): `Camera RAW, developed by
  apple` (see `export.log`, "Input colour"), output 3648×5472.
- Python experimental: rawpy/LibRaw, output 3670×5496. Use it as an
  independent third opinion, but it needs explicit alignment first. Don't
  silently crop or resize it.
- The preset WB for every case is `daylight`, 5500 K, tint 1 (`config.json` `wb`).

## Inputs

```text
RAW:    ~/Downloads/tag @signatureeditsco - free raws from signatureedits.com -IMG_0316.CR2
        sha256 0980eb3ab6dea41b335d3b44af305ae2020dc716a0a48f1ed22c69213ce31cfd
Nikon:  ~/Downloads/tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng  (control)
Presets: ~/Downloads/Presets/01 KODACHROME.json (no preflash; simplest), 06, 09
Frozen run: target/preset-accuracy-post-raw-fix-20260724-01   (read-only)
```

Always quote these paths (they contain spaces and `@`). Don't commit private
pixels or presets.

## Existing tools (check them before writing new ones)

- **`crates/spektrafilm-io/examples/raw_decoder_probe.rs`** decodes with the
  shared loader, using `--decoder auto|apple|rawler` and `--wb daylight|...`. It
  can save the linear array.
  Build it with `cargo build --release -p spektrafilm-io --example raw_decoder_probe`.
- **`scripts/parity/compare_raw_decoders.py`** compares the Apple and rawler
  backends on local files (`--artifacts` saves private linear arrays and
  previews to the output dir). This is the natural first experiment:
  ```bash
  python3 scripts/parity/compare_raw_decoders.py \
    --output-dir target/img0316-raw-decode-<timestamp> --artifacts --iterations 2 \
    --input "img0316=$HOME/Downloads/tag @signatureeditsco - free raws from signatureedits.com -IMG_0316.CR2" \
    --input "dsc2498=$HOME/Downloads/tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng"
  ```
- `crates/spektrafilm-cli/src/bin/decode_raw_gui.rs` writes the GUI loader's
  linear-sRGB float TIFF.
- Decoder selection lives in `crates/spektrafilm-io/src/raw/mod.rs`
  (`RawDecoderPreference`; Auto prefers Apple on macOS). The rawler path is
  `load_raw_rawler`. Web WB plumbing is in `web/raw-wb.js` and `web/decoder.js`.
- Engine-only comparisons: `target/img0316-cooked-gpu-debug-20261031/scripts/`
  has `run.sh` (CPU vs wgpu scans on one fixture), `matrix.py` (all 18
  deterministic cases), and `application.py` (full-res native exports vs CPU).
- Web capture: `scripts/accuracy/browser.mjs` driven by `web/cdp.mjs` against
  the dev server on port 8137. It needs a run dir with `config.json` and
  `provenance.json` whose `wasm_assets` hashes match the served pkg. See
  `target/img0316-cooked-gpu-debug-20261031/web-run/` for a focused 3-case
  run built that way, without touching the frozen run.

## Suggested work order

1. **Decode-only comparison.** Decode IMG_0316 with Apple and rawler at
   daylight WB into linear sRGB and compare them directly, before any film
   simulation. Look at:
   - per-channel means and medians on a neutral region,
   - a per-pixel 3×3 least-squares fit (rawler → apple). A near-diagonal fit
     means WB multipliers; an off-diagonal one means a matrix or primaries
     difference; a nonlinear residual means tone or highlights,
   - highlight clipping and black level,
   - orientation and crop alignment (both are 3648×5472).

   Run the Nikon DNG as the control: it should fit close to identity.
2. **Explain the fitted difference from metadata.** Compare the camera
   matrices each decoder uses (rawler's camera table vs the Adobe/DNG
   ColorMatrix vs Apple's), the daylight-WB multipliers each derives for
   5500 K, and any Canon-specific handling (black/white level, `WB_RGGB`
   levels). LibRaw/rawpy can serve as a third data point.
3. **Prove it through the engine.** Feed each decoder's linear output to
   native CPU with the same preset. The difference in the final images
   should reproduce the web/native application gap (about 2.6 ΔE on 01). Also
   check that swapping in the fitted correction closes it.
4. **Decide the reference with the user.** "Native" here means Apple's RAW
   decoder, which isn't available on the web. Present the measured cause, which
   decoder agrees with LibRaw, and the options before changing anything. The
   options might be fixing a rawler bug, matching Apple's matrix/WB on the web, or
   accepting the difference.
5. **Fix, regress, verify.** Write a minimal decode regression using public
   or synthetic data only. Then re-run the IMG_0316 + DSC_2498 application
   cases on web and native, and confirm the DNG doesn't regress.

## Preservation and traps

- Don't overwrite the frozen run, the preserved app bundles, or
  `target/img0316-cooked-gpu-debug-20261031/`. Write to new directories.
- The working tree has uncommitted RAW-related changes that aren't yours
  (`crates/spektrafilm-io/src/raw/mod.rs`, `raw/dng.rs`,
  `crates/spektrafilm-wasm/src/lib.rs`, web sample files). The DNG thumbnail
  routing fix there is finished, so don't redo it. Don't stash or reset them.
- Rebuilding the web pkg: `PATH="$HOME/.cargo/bin:$PATH" ./web/build.sh --threads`
  (it needs wasm-bindgen 0.2.121 from `~/.cargo/bin`). `web/serve.mjs` on port
  8137 serves `web/` live. Use fresh CDP profiles to avoid a stale service
  worker or WASM.
- Python: `~/Projects/spektrafilm-env/bin/python` (the system python3 has no numpy).
- Compare unclipped float data where you can. RGB8 ΔE also includes
  quantization and grain noise.
- No deployment or publishing is authorized. Leave `public/` alone.

## Expected hand-back

- The decode-only difference, quantified for both photos, and the form of
  the fitted transform.
- Its cause, tied to specific metadata or code (matrix, WB, levels, …).
- A recommendation and the options for which decoder output should be the
  reference, for the user to decide.
- Only after approval: the fix, a regression test, and before/after application
  metrics for all 18 cases.
