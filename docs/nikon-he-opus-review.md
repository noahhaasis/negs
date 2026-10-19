# Independent review of the Nikon HE implementation plan

Date: 2026-10-19. Requested reviewer: Claude Opus 5.5.
The Claude Code response metadata records `claude-opus-5-5`; exit status was 0,
`is_error` was false, and no tool permission denials were reported.
The review used read-only tools. The text below is the reviewer’s unedited
response. Its numerical confidence estimates and technical inferences are the
reviewer’s judgments, not empirical probabilities or independently verified
codec results.

Plan reviewed: `nikon-he-implementation-plan.md` before the revisions listed
in that document's review disposition. This is a review of a plan and existing
experimental source, not approval of a finished implementation.

---

I've finished reading the code and checking the numbers. The review follows.

---

# Independent review: Nikon HE/HE★ plan

**Reviewer model:** Claude Opus 5.5 (`claude-opus-5-5`). No subagents were used, no files were changed, and no images or photo files were opened.

**Evidence labels.** **[O]** means I saw it in local source or in the plan. **[E]** means an external claim from the LibRaw PR 826 comments; I have not reproduced any of these. **[I]** means my own inference.

## 1. Verdict: **proceed with changes**

The plan's overall structure is sound: an oracle first, exact parity, a bounded claim, and no tuning for appearance. Six changes are needed before Phase 2:

1. Treat every constant in `mct.rs` as unverified, and remove the black-level floor.
2. Add a way to check the spatial reconstruction and chroma stages separately from the transfer curve.
3. Add negative controls so the owner can trust a "0 mismatches" result.
4. State the real tested profile, which is narrower than "Z f".
5. Compare metadata as well as arrays.
6. Validate the image size declared in the stream header against the file before allocating memory.

## 2. Findings, in priority order

### P0: these block a trustworthy Gate B

**F1. The fitted transfer curve has the wrong shape, not just slightly wrong constants.**
- [O] `mct.rs:164` clamps `t = (v+V0).max(0)`. Output can therefore never go below `C = 1050`, which is 42 codes above the black level of 1008. The measured minima of 1050 and 1054 match this floor exactly.
- [E] The published rule uses a *signed* square below `C` (`q = -d*d`). Output keeps falling below `B` toward 0, so sensor noise around black survives.
- [I] The floor clips shadow noise to one side, which adds a positive bias in every channel. White balance then scales that bias differently per channel, so it shows up as a shadow colour cast.
- Required change: the replacement must be the header-driven signed rule, not a refit.

**F2. The fitted curve may also have the wrong slope and white point. A cheap test can show this before the oracle arrives.**
- [I] Assume rawler's pre-transfer `v` is in the same domain as the published rule's `v`. Both span roughly 16 bits; rawler applies the `Fq` shift in `dwt.rs:108`. Under that assumption:
  - At `v=0`: rawler gives 2413 (asserted in `mct.rs:248`), the rule gives about 2734.
  - At `v=30000`: rawler gives 12176, the rule gives about 14706.
  - At the rule's clamp, `v=32767`: rawler gives about 13550, which is below the Z f white point of 15892 (`data/cameras/nikon/z_f.toml:19`).
- So rawler's output would be roughly 0.8× the rule's above black, and clipped highlights would never reach the white level. After white balance, that can leave clipped highlights tinted instead of white.
- No power-of-two change of domain reconciles the two curves. The implied gain ratio is about 1.10, and the offsets disagree.
- Either the fit is wrong, or rawler's `v` is about 10% larger than the rule assumes. That second case would mean the problem sits upstream of the curve, in dequantisation or the colour transform. Which one is true is unknown until measured.
- Cheap diagnostic, not acceptance: record min, max and quantiles of rawler's pre-transfer `v` on both files. Check whether saturated regions sit near `v ≈ 32767`.

**F3. The chroma gains are most likely leftovers from the joint fit and must not be kept by default.**
- [O] `mct.rs:27-30, 37-38`: `CB_GAIN=4339` and `CR_GAIN=4178` were fitted together with the curve, against lossless shots of one scene. Those shots are different captures, so they are not a pixel-exact reference. Nothing in the stream supplies these numbers.
- [I] When the curve's offset and slope are wrong, the colour differences between channels come out wrong as well. A per-plane gain can partly hide that. Switching to the exact curve while keeping the gains will probably *add* colour error.
- [E] The edit to LibRaw comment 3 says that next to `A` in the vendor header sits "another gain field". The vendor bytes not yet explained are `tail[0..2]`, the upper nibble of `tail[3]`, `tail[5..6]`, the upper bits of `tail[7]`, and `tail[12]`.
- Rule: a chroma gain is allowed only if it is exactly 1.0 or comes from a documented header field. It also has to pass on a file that was not used to derive it.

**F4. The spatial reconstruction rests on several guesses, and the plan has no way to test them apart from the curve.**
- [O] The Star-Tetrix parameters are hard-coded because the CTS/CRG markers are missing (`mct.rs:7-11`):
  - `Cf=3`
  - `e1=e2=1` (`mct.rs:109-114`; the spec default would be 0, which halves one term)
  - CFA type 0
  - the `0312` component permutation (`mct.rs:195-197`)
  - the rule that vertical taps stay within one row pair (`mct.rs:57-60, 68`)
  - edge reflection (`mct.rs:65-66`)
- Phase 2 says to "validate as separate stages" (plan:100-101), but Adobe only gives the final values. Synthetic planes only test our code against our own model of the transform.
- Missing method [I]: the exact rule is monotonic. Each Adobe raw14 value can therefore be mapped back to the *interval* of pre-transfer `v` values that produce it, and rawler's post-transform `v` checked against those intervals. That separates spatial and chroma errors from transfer errors. Use mid-tones and highlights, because near black one raw code covers a wide band of `v`.
- The leftover error then points to a cause:
  - error that correlates with the Cb or Cr plane means a gain problem
  - error at quad-row edges means the reflection rule
  - error in the bottom two rows means the vertical boundary
  - error repeating every 64 Bayer rows means slice handling
- Favourable static reading:
  - [E] The bottom-row bug in LibRaw comment 1 has the form of the 5/3 vertical inverse wavelet recomputing a boundary value from clamped rows. The stated fix is `x[2N] := x[2N-2]`.
  - [O] `dwt.rs:306-313` already uses that form: `last = hf + even`.
  - [O] Raw-GCLI packets bypass significance coding (`entropy.rs:193-205`), which matches [E] comment 4's statement that "the dnglab core" decodes the raw-GCLI ramp the same as Adobe. That claim may refer to a different commit, so pin it.

**F5. The plan has no negative controls, so a non-specialist cannot tell whether the harness actually checks anything.**
- A harness can pass by comparing the wrong data: an array against itself, the wrong IFD, a pre-cropped DNG, or a black-subtracted DNG.
- Gate A must show all three of these:
  - (a) oracle against itself gives 0 mismatches
  - (b) one injected ±1-code change at a known pixel gives exactly 1 mismatch at that location
  - (c) the current fitted decoder gives **non-zero** mismatches. F1 and F2 predict widespread mismatches with large maximum errors.
- If (c) unexpectedly shows near-zero error, the oracle's value domain is wrong.

**F6. The existing upstream reference test checks the code against itself, not against an independent decoder.**
- [O] `mct.rs:314-327` compares against a *patched* SVT-JPEG-XS decoder driven by `NIKON_*` settings. That decoder uses the same guessed output stage.
- Even when it runs, it shows the two copies agree. It does not show the result is correct. Plan:40-41 should say so, and those tests must not be counted as reference evidence.

### P1: scope and claim accuracy

**F7. The tested profile is narrower than "Z f HE★".**
- [O] Both mosaics are 4000×2672. The Z6III sample header shows a full-frame grid of 6064×4040 (`header.rs:384`).
- [I] So both test files are almost certainly the **DX image area**, not full frame.
- [O] Both files carry byte-for-byte the same vendor tail. That tail is also identical to the Z6III sample (`header.rs:364-365`).
- Effect: Gate B tests one point. It covers one body, the DX geometry, HE★ only, and one set of header parameters. Full-frame Z f, plain HE, M/S RAW sizes and raw-GCLI streams are all untested. Gate B's claim must say exactly that.

**F8. Images should be accepted by exact profile, not by compression tag.**
- [O] `nef.rs:283` sends any file with tag 13 or 14 to the HE decoder.
- The allowlist should key on all of these:
  - model
  - mosaic size
  - `Bw/Fq/Br`, `Ng/Ss`, `NLx/NLy`
  - the CDT bytes
  - `Qpih/Fs/Rm/Cpih/Ppoc`
  - component count and bit depth
  - the full 13-byte vendor tail
- Unknown vendor bits, which header.rs currently skips at `header.rs:253-254`, must be rejected rather than ignored.
- Add a consistency check: `B` from the stream header must equal MakerNote 0x003d for all four CFA positions (`nef.rs:379-384`).

**F9. Gate B only compares arrays, but Spektrafilm's output also depends on metadata.**
- [O] `nef.rs:302` leaves the white level as `None`, so the camera file's value of 15892 is used.
- [O] The crop comes from the MakerNote (`nef.rs:310-313`). The CFA pattern comes from the camera file, while the decoder always assembles RGGB (`mct.rs:209-227`).
- Gate B should compare rawler's black level, white level, crop or active area, CFA phase and as-shot white balance against the DNG tags. Any difference must be explained.

**F10. Memory limits are checked too late.**
- [O] Plane buffers are allocated from the image size in the stream header: `dwt.rs:230` uses `vec![0; w*h]` per component.
- The IFD size check happens only after decoding finishes (`nef.rs:591`).
- A corrupt header can therefore request about 17 GB. That is an out-of-memory abort, which `catch_unwind` cannot catch (`spektrafilm-io/src/lib.rs:201`).
- Fix: pass the expected size into `jpegxs::decode` and reject a mismatch before allocating. This is a Phase 2 item, not Phase 3.

**F11. Header-field perturbation is a cheap source of extra oracle cases, and the plan doesn't use it.**
- [E] The LibRaw author edited `B`, `A` and `C` in copies of NEFs and saw Adobe's output change as the rule predicts.
- Doing the same with local copies tests our header parsing against Adobe without needing new photos. This partly answers F7's lack of variety.

### P2

**F12. The order of checks in `nef.rs` is fragile.** [O] `nef.rs:247` and `nef.rs:280` use size guesses to detect uncompressed and sNEF files, and both run before the HE branch. A misroute is unlikely given HE★ file sizes, but the HE check should come first once its profile is validated.

**F13. Licensing.**
- [O] The PR files say `LGPL-2.1, Copyright 2026` with no copyright holder named (`mct.rs:1-2`). Record the provenance.
- [O] The app is GPL-3.0 (root `Cargo.toml:17`). LGPL-2.1 code can be used in a GPL-3.0 program, so the code licence is not a blocker.
- Implement the published formula from its description; don't copy LibRaw code.
- The codec patent question (JPEG XS, intoPIX) stays open, as the plan already says.
- Put HE decoding behind a Cargo feature that is off by default, so stock builds don't ship the codec before the owner makes a distribution decision.

**F14. The oracle needs a version pin, and DNG reading needs care.**
- Pin the DNG Converter version and build. 17.5.1 (2319) makes results comparable with [E].
- The checked TIFF reader must handle tiled uncompressed DNGs and `BlackLevelRepeatDim`.
- Installing a newer converter changes the owner's system. Ask the owner first.

## 3. Minimum revised sequence and gates

| Step | Work | Measurable gate |
|---|---|---|
| A0 | Isolated checkout, pinned sources, private manifest. Acquire and pin the Adobe DNG Converter (owner OK to install). | Version and build recorded. Uncompressed CFA DNG confirmed: `PhotometricInterpretation=32803`, `Compression=1`, no `LinearizationTable`, black level not subtracted. |
| A1 | Independent TIFF/DNG reader and comparison tool. | Negative controls (a), (b), (c) from F5 pass, with the output saved in the report. |
| A2 | Baseline: current fitted decoder vs oracle. Record `v` statistics (F2). | Full error report per CFA channel: histogram, edge rows and columns, last two rows, error by row modulo 64. Expected result: non-zero. |
| B1 | Swap **only** the transfer curve for the header-driven signed rule, with B/A/C parsed from the stream header and the 1050 clamp removed. Keep the gains for now. | Error report again. |
| B2 | Use oracle values mapped back to `v` intervals to isolate spatial and chroma errors (F4). Set gains to 1.0 unless a header field justifies them. | Leftover error near zero, or explained by location. No constant without stated provenance. |
| B3 | Header-field perturbation tests through Adobe (F11). Profile allowlist (F8). Size check before allocation (F10). | Predicted and observed oracle changes match. Unknown profiles return an error. |
| **Gate B** | Both files, full mosaic. | **0 mismatches out of all samples**, both files, no per-file settings. Metadata parity (F9) passes. Claim limited to "Z f, DX area, HE★, vendor tail `50887083…07`, parity with Adobe DNG Converter vX". |
| C | Bounded robustness: a mutated-stream run in a separate process with time and memory limits. Public samples if available. | No hangs or aborts. Reference jobs fail when data is missing. |
| D | Spektrafilm integration behind a feature flag; existing NEF regression tests. | Plan Gate D as written, plus a test that the export pixels came from the decoded RAW and not the embedded JPEG. |

**What the owner's report should contain.** It should be produced by a script, with no hand-written numbers:
- input and oracle hashes, and the oracle version
- results of the three negative controls
- mismatch count and total samples for each file
- metadata parity results
- the supported profile written out literally
- reference cases run versus skipped, where skipped must be 0
- export success

With those items, a result of "0 mismatches, controls passed, 0 skipped" can be read without specialist judgement. Without the negative controls it cannot.

## 4. Calibrated confidence

These are subjective engineering judgements, not measured probabilities.

- **Bounded Z f DX HE★, exact parity with Adobe: about 75%** if a current DNG Converter can be obtained, and about 65% overall.
  - In favour: [E] claims exact parity on a Zf file and on 10 others using public methods. [O] rawler's entropy decoder and vertical boundary already look like the corrected forms.
  - Main risks: spatial or chroma differences in `mct.rs` (F3, F4), and the source of F2's roughly 10% slope gap.
- **Broad HE and HE★ across Nikon bodies, near term: about 40%.** Reasons:
  - The other vendor bits are not understood.
  - Plain HE, full-frame/DX/M/S geometries and raw-GCLI streams are all untested.
  - An oracle has to be obtained for each profile.
  - [E] reports identical header parameters across 11 files, which helps but is a small sample.
- The plan's "high" confidence for a "useful" bounded version is fair if "useful" means it opens and looks plausible. For *exact* output it is too optimistic until step B2 passes. A plausible-looking picture can still be about 20% off and have a shadow colour cast (F1, F2).
