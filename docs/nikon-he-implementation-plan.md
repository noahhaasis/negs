# Nikon HE/HE★ decoder implementation and acceptance plan

**Direction update, 2026-10-19:** A subsequent test successfully decoded both
local HE★ inputs with Apple's built-in `CIRAWFilter` on this Mac. The preferred
macOS product path is now to integrate that existing decoder. See
[the native-decoder feasibility findings](nikon-he-native-decoder-feasibility.md).
The custom rawler codec plan below is retained for portable-decoder work; it is
no longer the recommended first step for opening these files in the macOS app.

Date: 2026-10-19. Status: revised after independent Claude Opus 5.5 review;
ready to begin the reference-validation milestone. Implementation is not yet
accepted or enabled. See [the independent review](nikon-he-opus-review.md).

## Product commitment and confidence

The proposed deliverable is dependable opening, editing, and export of a
documented subset of Nikon HE/HE★ NEFs in Spektrafilm, with exact decoded
sensor values established against an independent decoder. Start with the two
existing 4000 × 2672 Z f HE★ inputs, apparently DX image area, carrying vendor
tail `50887083f01523d149cd3f7f07`. Confirm the image-area interpretation from
metadata. This is one tested profile, not general Z f support. Expand only as
additional profiles pass validation. General support for every Nikon
model/profile is outside the initial commitment.

My confidence in producing a working prototype is high: it already decodes both
user files. Confidence in exact decoding of the initial profile is moderate,
and conditional on acquiring a usable oracle and resolving colour/spatial
reconstruction. Broad support is substantially less certain. The reviewer
estimates approximately 75% for exact initial-profile decoding if a current
oracle is available, 65% overall, and 40% for broad near-term support. These
are subjective judgments, not measured probabilities. I accept the reviewer's
correction that the initial description was too confident about exactness.
A recognizable photograph establishes feasibility, not numerical correctness.

I will own format research, test data selection, numerical diagnosis, and app
validation. The product owner need not supply image-processing expertise or
judge shadow colour by eye. Decisions needing owner input would be material
scope changes, paid third-party contracts, or a public distribution decision.

## Evidence and provenance

- Spektrafilm currently uses rawler 0.8.0 through `spektrafilm-io`.
- An isolated copy of rawler 0.8.0 with nine source files overlaid from upstream
  [PR #835](https://github.com/dnglab/dnglab/pull/835), commit
  `0f044c2c30d78c4ed5fcede6ab4e8db893d566f6`, decoded DSC_2533.NEF and DSC_1180.NEF.
- Both decoded mosaics are 4000 × 2672, one integer sample per sensor pixel.
  Single-run decode times were 287 ms and 257 ms. These are not benchmarks.
- Both inputs have MakerNote NEF compression 14 (HE★), black levels 1008,
  and identical 13-byte vendor picture-header data:
  `50887083f01523d149cd3f7f07`.
- Experimental `mct.rs` uses fitted chroma gains and a fitted nonlinear transfer
  function. The latter has a lower bound of 1050; measured output minima were
  1050 and 1054. This is an accuracy concern, not a measured reference error.
  Black level is not itself a requirement that a particular photograph contain
  samples below or equal to that value.
- The upstream test command reports 35 passes; four tests return early without
  external reference data. Even with those data present, agreement with a
  patched SVT-JPEG-XS implementation carrying the same guessed output stage
  does not independently validate the reconstruction. There is no established
  reference parity yet.
- [Published reconstruction findings](https://github.com/LibRaw/LibRaw/pull/826#issuecomment-5639699213)
  describe header fields and integer rounding, with reported same-input Adobe
  comparisons including a Z f. Those claims have not been reproduced locally.
  Our two inputs yield B=1008, A=84429, C=16255 under that field extraction.
- Installed Adobe DNG Converter 14.5 predates Z f support and is not a usable
  oracle for these inputs. Acquiring a current oracle is the first uncertainty
  to resolve. LibRaw PR #826 is a separate experimental implementation; its
  discussion and fixes must not be mistaken for a verified released decoder.

Experimental source, probe and logs currently live under
`/private/tmp/spektrafilm-he-research`. Copy the required source and sanitized
evidence into a durable isolated checkout before implementation. Personal
photographs remain local and are not published as fixtures.

## Phase 1: establish an independent acceptance harness

Create an isolated worktree/fork so ongoing Spektrafilm work is undisturbed.
Pin original rawler 0.8.0 and experimental source revisions and record patches.
Preserve notices and license provenance. Do not upgrade unrelated dependencies.

Obtain a current trusted decoder: preferably Adobe DNG Converter producing
uncompressed, non-linearized mosaic DNGs from the exact input NEFs. Stage a
standalone tool in an isolated location where practical rather than replacing
the installed converter. If another reference is used, establish its origin,
version, supported HE★ profiles and independence from the candidate code.

Record input hashes, model/firmware when present, compression tag, dimensions,
bit depth, CFA pattern, active/default crop, black/white levels, header profile,
oracle version, command arguments, output hashes, and conversion settings.
Keep local photo paths in a private manifest, not public documentation.

Check the DNG's actual storage, including CFA photometric interpretation
(32803), uncompressed sample storage (Compression=1), and absence of an applied
linearization table. Handle both strips and tiles correctly. The extractor must
have tiny independently constructed fixtures with known sample positions,
endianness, tile edges and black-level repeat dimensions.

Export candidate integer Bayer arrays before black subtraction, white balance,
demosaicing, orientation, colour transforms, or gamma. Extract reference stage-1
sensor arrays independently; for uncompressed DNG prefer a small checked TIFF
reader over the candidate rawler implementation. Confirm no lossy recompression,
linearization, rescaling, black subtraction, or opcode processing has silently
changed the domain. Inspect reference metadata before comparing arrays.

Compare all comparable sensor samples exactly, accounting explicitly for any
documented cropping, padding and CFA phase. Report any excluded pixels and why;
the entire declared supported output region must pass. Include dimensions,
mismatch count, maximum absolute error, signed-error histogram, per-CFA-channel
statistics and locations, plus border-row/column results. A one-code-value
error is a failure under the exactness claim. Reference-domain uncertainty must
be resolved rather than hidden with a tolerance or arbitrary crop.

Gate A: obtain interpretable independent reference arrays for both local files
and reproduce a baseline comparison. Prove the comparison harness with negative
controls: identical arrays give zero mismatches; changing exactly one known
sample by one code gives exactly one mismatch at the expected coordinate and
CFA position; a deliberately shifted/swapped-CFA fixture is detected. Record
the current fitted decoder's baseline, expected to disagree; investigate any
unexpected agreement rather than automatically declaring the oracle wrong.
If references are unavailable, report that limitation; do not label the feature
verified or enable it in the normal app build.

## Phase 2: implement exact reconstruction on a bounded profile

Start with PR #835's parsing, entropy decoder and inverse wavelet pipeline.
Read and document the applicable vendor picture-header fields rather than
discarding the vendor extension. Check lengths, endianness and profile support.
Before allocating coefficient or plane buffers, validate codestream dimensions
against expected TIFF dimensions and resource bounds. Pass expected dimensions
into the decompressor; an equality check after reconstruction is too late.

Replace the guessed output reconstruction incrementally. The newer transfer
rule is a hypothesis to reproduce against the oracle, not a drop-in proof that
the rest of `mct.rs` is correct. Validate chroma scaling, component permutation,
inverse spatial/Star-Tetrix reconstruction, edge handling and nonlinear transfer
as separate stages. Removing fitted constants requires a supported derivation
and evidence. Do not tune constants per scene or optimize merely for appearance.
This includes the hard-coded Star-Tetrix parameters, component permutation,
spatial-neighbour rules and two chroma gains; none becomes correct merely by
replacing the nonlinear function.

Record pre-transfer integer-domain statistics. First change only the transfer
rule and measure the error delta, retaining that result as a diagnostic rather
than a finished decoder. Then isolate chroma/spatial effects: enumerate the
bounded transfer input domain to map each reference sample to the interval of
pre-transfer values producing it, and check candidate values against those
intervals. Account explicitly for saturation plateaus and many-to-one mapping;
interval containment alone does not prove individual intermediate stages.
Use mid-tones and highlights, per-channel residuals and spatial error patterns
to distinguish gain, row-boundary, slice and wavelet errors. Unity chroma gain
is a diagnostic candidate, not an assumption of correctness. Any retained
non-unity gain must have a justified codec/header basis and held-out validation.

For the applicable transfer profile, implement the published fixed-point rule
with checked bounds, signed 64-bit intermediates, explicit clamp order and
defined rounding. Verify negative right-shift/floor behaviour, half-way cases,
and saturating endpoints independently. Ensure reconstructed black offsets are
not applied twice by the normal rawler development path.

Use synthetic coefficient planes, impulses, ramps, negative values and edge
cases to localize each stage; independent same-input sample comparisons remain
the final authority. Correctness may require changes beyond the transfer curve.
Perturb B/A/C individually in disposable local NEF copies and compare with the
oracle to test field extraction, rounding and domain assumptions independently
of scene variety. Keep this diagnostic path distinct from production profile
acceptance so a deliberate header change cannot broaden support accidentally.

Gate B: both local HE★ inputs match the oracle exactly over their declared
sensor output domain, without per-file fitted settings. Also compare black and
white levels, active/default crop, CFA phase and as-shot white balance against
the reference metadata, normalizing representations such as gain ratios versus
inverse neutral coefficients. Resolve and document discrepancies; array parity
does not establish metadata correctness.

Unknown codec/profile combinations return a clear unsupported-format error.
Identify support by validated model/geometry, component depths/counts,
decomposition and quantization fields, packet modes, CDT layout and vendor
extension, not compression tag 14 alone. Unknown vendor bits cannot be silently
ignored. Cross-check B against MakerNote black levels for this validated
profile; do not assume the equality is a universal codec rule. An allowlist
must have a written rationale and tests. Initial claim: the specific cropped
Z f HE★ profile above, matching the recorded oracle version.

## Phase 3: robustness and broader coverage

Use redistributable public samples with recorded licenses/hashes in addition
to the two private inputs. Cover HE and HE★ separately; obtain at least one
additional body and both full-frame and cropped dimensions where available.
Include low-light, high-contrast and saturated scenes. Reserve held-out inputs
for validation rather than fitting. Publish the exact tested profile matrix.

Exercise bottom/top rows, last columns and partial coefficient groups; packet
prediction resets, raw-GCLI mode, slice transitions, sign padding and odd
supported component sizes. Add reference-backed golden arrays or hashes for
the sensor stage. Tiny synthetic fixtures belong in normal CI. Missing required
reference data must fail the designated parity job, not silently count as a pass.

Reject truncation, inconsistent dimensions, overflowing sizes/offsets, unsupported
depth/decomposition, impossible packet lengths, malformed headers, and invalid
allocation requests. Use checked arithmetic before indexing or allocating and
establish explicit memory/work bounds. Exercise malformed and mutated streams
in a subprocess with time/memory limits; don't rely solely on panic catching.
Test that invalid input returns an error without hanging or terminating the app.

Gate C: exact parity across the claimed profile matrix, reference cases run
without missing-data skips, and no unresolved corruption/robustness failures.
Expand the supported subset only when its own oracle comparisons pass.

## Phase 4: integrate with Spektrafilm

Use a pinned local/vendor rawler patch for reproducibility until an appropriate
upstream release exists. Keep the change focused on NEF HE support. Document
upstream commit ancestry and how to remove the patch later.
Keep the experimental decoder behind an explicit Cargo feature, disabled in
normal builds until the validation/release gates are satisfied. Dispatch known
HE profiles before legacy size-based uncompressed/sNEF heuristics.

Preserve the shared linear-sRGB loader used by preview and export. Test loading
both local inputs through the packaged native helper, As Shot and custom RAW
white balance, orientation, reduced previews and full-resolution f64 export.
Verify that saving/exporting uses decoded RAW data, not the embedded JPEG, and
that failed decoding does not retain stale processed output as a successful
render. Native and cross-platform frontends should surface a useful error.

Regression-test existing lossless Nikon NEFs plus available non-Nikon RAWs.
Measure source decode time and peak memory separately from film rendering;
compare with the experimental baseline and investigate material regressions.
Build precision-f32 and precision-f64 variants and run appropriate workspace
checks. Commit neither private photographs nor raw absolute library paths.

Gate D: packaged-app import/edit/export works for the declared supported subset,
with passing numerical and existing-format regression results. Deliver an
experimental build separately until distribution status and support claims are
settled; preserve an uncomplicated rollback to stock rawler.

## Scope, uncertainty and delivery decisions

The next concrete milestone is Gate A, followed by exact parity on the two local
files. Passing these is more informative than a speculative implementation-time
promise. A failed comparison is a diagnosis task, not permission to reduce the
acceptance criterion. If reference acquisition or a profile remains unresolved,
deliver the reproducible evidence and narrow the support claim explicitly.

The product owner can assess completion from a short report: supported inputs,
zero/nonzero mismatches, actual reference cases executed, successful app export,
and outstanding limitations. Image-processing advice from the owner is not a
dependency.
Generate this report from the harness, including input/output hashes, oracle
version, negative-control results, compared sample counts, metadata comparisons,
the literal supported profile, required cases executed/skipped and export
success. Required reference cases skipped must be zero.

Public distribution has a separate licensing/patent question. Source-code
licensing is not proof of codec patent permission. Track provenance and identify
the unresolved issue; do not invent a royalty requirement, claim legal clearance,
or let a paid SDK agreement happen without the owner's decision. Local technical
validation can proceed independently of that public-release decision.

## Review disposition

Claude Opus 5.5 reviewed the plan and experimental source with read-only tools.
Its verdict was **proceed with changes**. The reviewed initial plan is retained
in the temporary review packet; the independent critique is saved in the repo.

Accepted revisions: calibrated confidence; explicit cropped-profile scope;
negative controls and independent extractor fixtures; stage-specific diagnosis;
no automatic retention of fitted gains; metadata validation; pre-allocation
dimension checks; vendor-field perturbation tests; early HE dispatch; explicit
experimental feature gating; and a machine-generated acceptance report.

Clarifications to the review: predicted baseline errors and shadow casts remain
inferences until measured. An unexpected matching baseline calls for diagnosis,
not automatic rejection of the oracle. Equality between B and MakerNote black
levels is checked for the supported profile, not generalized without evidence.
Defaulting a gain to unity also requires validation. Source-license compatibility
and patent clearance are not established by the model review. Oracle acquisition
will prefer reversible isolated staging; this review does not require replacing
the user's installed software or an additional blanket approval step.

## Questions for the independent reviewer

Assess this plan as a skeptical image-codec/Rust engineer. Identify missing
assumptions, invalid comparisons, silent failure paths, and minimum changes
needed before implementation. Check whether the fixed-point research actually
fits the experimental reconstruction stages. Challenge confidence and scope.
Prioritize issues by severity and distinguish measured evidence from claims.
Do not implement, modify files, send messages, or access personal photographs.
