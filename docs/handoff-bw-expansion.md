# Handoff: two-hour B&W film and paper expansion

Prepared 18 October 2026. Repository: `spektrafilm-rs`.

**Mission.** Work autonomously for approximately two hours to make meaningful, usable progress on additional black-and-white films **and photographic papers**. Implement and validate a small, defensible selection. Give the owner visible results and an explanation they can assess without chemistry knowledge.

**The owner is the PM/product owner.** They want accurate, attractive film simulations. They have no chemical expertise and little film experience. Choose processing recipes, fitting methods, and measurement conventions yourself using the evidence. Present product tradeoffs in terms of image appearance, confidence, and effort. Never make them responsible for resolving your scientific uncertainty.

This document is the execution brief for the next agent. Preparing it did not start the two-hour implementation run or add any profiles.

**1. What to deliver**

Aim for **one additional conventional B&W camera film and one real photographic-paper rendering**, integrated into the app when the available evidence supports them. A second film or additional measured development times are stretch goals. A short, well-supported catalogue is more valuable than many renamed donor profiles.

The default film is **Kodak Tri-X 400 / 400TX**, under one explicitly identified processing recipe. T-MAX 100 is the first fallback or second film. For paper, find a suitable manufacturer-documented fixed-grade material, or a documented fixed-grade/filter setting of a variable-contrast paper. Prefer a familiar Ilford product if suitable data exist. A grade-2 setting of Multigrade RC Deluxe is a candidate to investigate, **not an already verified source or implementation**.

Paper must receive real work during the first half of the run. Do not spend the entire budget adding camera films and leave paper as a vague future suggestion. If paper data prove insufficient, deliver the completed film work, a concrete paper prototype outside the normal catalogue where useful, and an exact account of the missing evidence. Do not invent measurements to meet a stock-count target.

Required end products:

- Source points and a reproducible preparation process for each added material.
- Validated profile files and any necessary, narrowly scoped integration changes.
- A review document with actual comparison images and a short product recommendation.
- An evidence record explaining what came from measurements, what was derived, and what remains assumed.
- A fresh candidate macOS build if the changes pass checks and packaging fits the time budget; otherwise working source/CLI results and explicit build status.
- A brief continuation note identifying the highest-value next work.

**2. Read first; do not repeat the whole research project**

Use these existing documents, in this order:

1. [Research guide](adding-film-stocks-research.md): start with sections 4–9 and 11, then consult the rest as needed.
2. [Source inventory](film-stock-research-sources.json): exact primary sources, historical revisions, and relevant forum posts.
3. [Existing B&W implementation handoff](handoff-bw-stocks.md): schema semantics and provenance of the three bundled stocks.
4. [Latest B&W/gamut/package review](review-bw-gamut.md): supersedes earlier validation and packaging status.
5. [Parity runner documentation](../scripts/parity/README.md), [experimental gamut handoff](handoff-experimental-gamut.md), and [upstream differences](upstream-sync.md) when needed.

Reinspect current files and `git status` before editing: handoff commits and test counts are historical evidence, not a guarantee of the state you start with. The research examined Rust `567a43132f64538ec6d877c8d015e319139b80b3`, Python main `3bb2c2d2801ff68b92019cf1dbcbb133d60832bc`, and Python experimental `28bf883e1672e884307edc75852549376e13644e`. Pin whichever reference you actually use.

Existing sources include manually digitised manufacturer graphs plus reconstruction and calibration. Andrea's maintained profile-creator package was not located publicly. Historical code is useful reference material but is not a demonstrated way to regenerate current profiles. Spend at most a short initial investigation looking for an updated creator; continue with a small reproducible B&W preparation tool if it is unavailable.

**3. Autonomy, scope and time**

The brief authorises normal local implementation, public-source research/downloads, testing, rendering, and an isolated candidate build. Continue through routine technical choices. A missing source or tool should trigger a fallback, not an immediate question to the PM.

Keep your changes focused on B&W films, papers, their preparation, calibration, and necessary integration. Existing colour behaviour and unrelated work must be preserved. Broad colour-model migration, new spectral upsamplers, infrastructure rewrites, continuous variable-contrast chemistry, and infrared film are outside this run. Do not contact forum participants or the original author, publish a release, buy equipment, upload user images, or alter another checkout without separate authorisation. Follow the environment's tool-approval rules.

At the start, record the clock time and a roughly 120-minute deadline. Use elapsed wall time to prioritise; do not idle to fill the budget. Avoid ending after the first easy result while useful verification or paper work remains. Protect the final 20–25 minutes for checks and reviewable output.

| Elapsed time | Intended outcome |
|---|---|
| 0–15 min | Inspect baseline, read the focused handoffs, identify film and paper candidates and their evidence gaps. Start source checks for both. |
| 15–35 min | Secure and inspect graph images; extract traceable points; decide the smallest defensible film and paper scope. |
| 35–75 min | Implement the preparation path and first complete film/paper render. Check source overlays before polishing appearance. |
| 75–95 min | Improve the first pair, or add a second film/measured time only if the first pair is sound. Assemble comparison images. |
| 95–120 min | Stop adding scope. Complete required checks, candidate packaging if feasible, the PM review, and the continuation note. |

These are checkpoints, not instructions to rush bad data into the catalogue. If paper remains scientifically blocked around minute 40, document the exact missing quantity, use the best defensible fallback for the working demo, and continue useful work. Clearly identify that fallback as an approximation or the existing 2302 print-film path.

**4. Data sources and known traps**

Film sources already located:

| Material | Primary document | Use / caution |
|---|---|---|
| Tri-X 400 / 400TX | [Kodak F-4017, October 2021](https://kodakprofessional.com/sites/default/files/wysiwyg/film/f4017_trix_320400.pdf) | Sensitivity and multiple processing curves. Choose the correct 400TX format and graph caption; distinguish D-76 and T-MAX processing. |
| T-MAX 100 | [Kodak F-4016](https://kodakprofessional.com/sites/default/files/wysiwyg/pro/resources/f4016_TMax_100.pdf) | Quantitative sensitivity, characteristic families, granularity and sharpness data. |
| HP5 Plus | [Older Ilford sheet, mirrored](https://www.99tarbox.com/rhoadescameras/Camera%20PDFs/Ilford%20HP5%20Plus.pdf) | Page 3 includes an equal-energy relative log-sensitivity curve. Identify the source age and whether it suits the target generation. |
| Multiple historical films | [German Kodak XTOL booklet](https://125px.com/docs/techpubs/kodak/xtolEntwickler.pdf) | Printed pages 32–34 / one-based PDF pages 34–36 contain development-curve families annotated with CI. Text extraction misses the graphs. |
| Acros II | Manufacturer links in the source inventory | Preserve the generation and format; do not substitute old Acros data silently. |

In the XTOL booklet, the HP5 panel identifies **21°C** and **4, 6, 8, 10, 13 minutes**, with CI values **0.39, 0.53, 0.62, 0.75, 0.87** respectively. Verify the image, full processing context and axes before use. Historical Fuji/Kodak names do not necessarily identify current emulsions.

For paper, use [Ilford's contrast-control explanation](https://www.ilfordphoto.com/amfile/file/download/file/1824/product/2078/) to understand the mechanism, then locate the **specific product/generation's** technical sheet. The general contrast-control document alone does not supply a complete calibration for every paper. Seek a characteristic curve for a specified grade/filter and developer, exposure sensitivity information, baseline/maximum reflection density, and spectral or reflectance information where available. Record missing quantities explicitly.

The following files were downloaded during research and may still exist under `/private/tmp/`:

```text
spektrafilm-trix400-2021.pdf
spektrafilm-hp5-old.pdf
spektrafilm-hp5-2018.pdf
spektrafilm-xtol-german.pdf
spektrafilm-trix400-p8.png
spektrafilm-xtol-p1.png    # rendered PDF page 35, printed page 33
spektrafilm-xtol-p2.png    # rendered PDF page 36, printed page 34
```

Temporary files are optional caches, not project dependencies. Ghostscript (`gs`) was available for rasterising PDF graphs. Use image inspection: OCR omits negative-log bars and entire scanned plots. Use numerical graph extraction with checked axis calibration, manual tracing or a documented image/vector method; preserve the raw points and overlays. Never estimate a whole response from marketing descriptions or ask an LLM to invent plausible-looking samples.

Important distinctions:

- “Log sensitivity” is already logarithmic; do not log it a second time.
- A tungsten wedge spectrogram is not automatically an equal-energy sensitivity curve. Establish its measurement definition before correcting for illumination.
- A graph endpoint is not proof of a physical cutoff or a measured maximum density.
- A development-time table or a CI-versus-time plot is not a full exposure-response curve. Inspect actual axes; the XTOL booklet includes additional curve families despite its section title.
- A measured curve is tied to a developer, dilution, temperature, agitation and film version. Keep those identities intact.

**5. Scientific and product defaults**

Develop the most faithful source-based baseline first. Make it attractive through sensible exposure, printing, output and texture defaults. Preserve the distinction between those creative settings and the underlying stock calibration.

Default choices you may make without asking the PM:

- One well-documented, ordinary development recipe per film. Prefer a moderate-contrast result suitable for varied photographs.
- A normal/middle paper contrast grade first; add softer/harder grades only with supporting data.
- The existing 81-wavelength grid and compatible profile model.
- A neutral conventional B&W absorber as an explicit model assumption when appropriate, without claiming measured stock-specific spectral neutrality.
- Conservative, documented texture settings when exact granularity calibration is absent. Do not claim a generic texture is stock-measured.
- A provisional “experimental” label when a materially important quantity is borrowed or insufficiently supported.

Accuracy and appearance are separate acceptance questions. Manufacturer-graph agreement is evidence about the model's inputs; software comparisons establish numerical consistency; pleasing photos establish usefulness. None alone proves an exact match to real film and a particular lab. Do not report an invented “accuracy percentage.”

For variable-contrast paper, a fixed grade/filter profile is an acceptable first scope. A single B&W sensitivity plus one curve does not implement independent blue/green-sensitive components under changing enlarger filters. Avoid promising faithful continuous contrast control unless you actually implement and validate the required model. Do not call Kodak 2302 photographic paper or relabel it as an Ilford material.

**6. Implementation map and compatibility requirements**

The existing three B&W IDs are `kodak_doublex` (5222 camera negative), `kodak_trix` (**7266 reversal**, not 400TX), and `kodak_2302` (motion-picture print film). Keep them intact. Use distinct, descriptive IDs and readable names for new work; identify developer/grade variants without misleading the user.

Key files:

- [Profile loader](../crates/spektrafilm-core/src/profile.rs) and [existing profiles](../data/profiles/).
- [Pipeline calibration](../crates/spektrafilm-core/src/pipeline.rs), [printing](../crates/spektrafilm-core/src/stages/printing.rs), [scanning](../crates/spektrafilm-core/src/stages/scanning.rs), and [colour/black-white references](../crates/spektrafilm-core/src/color_reference.rs).
- [Runtime parameters](../crates/spektrafilm-core/src/params.rs), [grain model](../crates/spektrafilm-model/src/grain.rs), and [spectral grid](../crates/spektrafilm-math/src/spectral.rs).
- [Native catalogue](../crates/spektrafilm-native/src/main.rs), [native resource locator](../macos/Sources/Spektrafilm/Engine.swift), and [macOS packager](../scripts/package_macos_app.sh).

Use a small preparation tool under `scripts/` with raw source points/recipes in a separate, clearly identified data directory. Produce stable JSON and validation plots from the same explicit recipe. Make its invocation reproducible. A second invocation should not introduce unexplained changes.

Build provisional profiles outside `data/profiles/`; a broken file can prevent native catalogue loading. Move only profiles that meet the stated integration gates into the normal catalogue. An experimental demo may use a separate complete data directory containing the existing LUTs and other required resources; a folder of standalone JSON files is not a complete `--data-dir`.

Current B&W semantics to preserve:

- `wavelengths`: 380–780 nm inclusive, 5 nm steps, 81 samples.
- `log_sensitivity` and `channel_density`: `81 × 1` before render-time expansion.
- `density_curves`: `N × T`, where `T` counts development times, **not RGB channels**; bundled exposure grids have 256 points.
- `base_density`: one shared column or a column per development time. Avoid counting base/fog again in net image-density curves.
- `density_curves_layers`: when present, `N × sublayers × T`.
- B&W `cdfs` / `norm_cdfs` fitted models are authoritative on load. Update fit parameters and sampled fields coherently; a stale fit overrides edited samples. Newer Python model types may require a compatible export.
- Missing log sensitivity is not numeric zero: `10^0 = 1`. JSON `null` currently means missing and contributes zero sensitivity at runtime; this truncation must still be justified in your source notes.
- Development-time selection currently uses the nearest entry. Do not advertise continuous measured interpolation.
- Photographic paper normally needs `support: "paper"`, `stage: "printing"`, `channel_model: "bw"`, and an appropriate increasing-density-with-exposure convention. Confirm the exact fields against the loader and full pipeline rather than copying a reversal-film identity.
- Set an intentional default pairing for new camera negatives. Test that paper discovery and both preview/export use the correct resource directory.
- Grain controls reside in runtime parameters; profile selection does not load a measured RMS target automatically. Any per-stock presets must actually be applied by the UI/export path if advertised. Keep optional refinements additive.

Do not modify existing golden values merely to make changes pass. Isolate necessary fixes, justify them, and preserve colour regressions. For a new physical-paper path, inspect transmission/reflection-density conventions, scan normalisation and baseline behaviour explicitly; a file that deserialises is not proof that those conventions are correct.

**7. Evidence and acceptance gates**

For every material, retain a concise source record: document URL/revision/hash/page/graph, measurement conditions, raw points, extraction/calibration method, fit/normalisation recipe, measured range, extrapolation and borrowed data, author/attribution/licence, and validation results. Label historical data as historical. Keep new authorship and inherited attribution accurate.

Minimum checks for an integrated profile:

1. **Data agreement:** overlay extracted points on the original graph; compare the fitted curve against those points. Use tolerances justified by graph resolution and line thickness. Exact six-decimal agreement with a low-resolution drawing is not a credible scientific claim.
2. **Physical sanity:** axes, dimensions, finite parameters, valid fit widths, appropriate monotonicity, no duplicated baseline, sensible toe/shoulder continuation, and coherent sensitivity/exposure normalisation. Keep total physical density/transmission valid over the supported range.
3. **End-to-end behaviour:** neutral ramp, coloured patches before B&W conversion, exact black, very dark values, exposure brackets, and bright/HDR values. Check intermediate exposure and density as well as final output; test LUT on/off where applicable.
4. **Paper behaviour:** fixed film across paper comparisons; correct orientation, white/black normalisation, plausible contrast and neutral or explicitly sourced tint. Confirm results are from the actual new paper profile.
5. **Software regression:** relevant targeted tests plus the required workspace checks. Cover real risks such as family selection, model-vs-samples consistency or paper routing with meaningful tests. Do not add tests that merely assert your own generated file equals itself.
6. **Usability:** the new names appear in the correct selectors, defaults choose the intended film/paper route, and preview and export use matching profiles. Exercise Metal preview if available and record the actual backend.

Commands from the current project, subject to current environment instructions:

```bash
cargo test --workspace --locked
cargo test --workspace --locked --features spektrafilm-cli/precision-f64
PYTHONDONTWRITEBYTECODE=1 ../spektrafilm-env/bin/python \
  -m unittest discover -s scripts/parity -p 'test_*.py'
```

The last command is particularly relevant if touching the comparison machinery. Record actual outcomes and skipped checks. A prior handoff's passing counts are not results for your changes. If baseline checks already fail, distinguish pre-existing failures from new regressions and continue unaffected work.

For new profiles, use a clean extracted/pinned Python reference with the same compatible data if feasible. Preserve the user's sibling `../spektrafilm` and its working changes; do not switch/reset it or write your profiles into it. If live Python comparison cannot consume the new material within this budget, report that limit and use an independently calculated small reference for the new path. Never create purported Python/physical reference values from the Rust function under test. Numerical parity and empirical accuracy must be reported separately.

**8. Give the PM something visual and understandable**

Write `docs/bw-expansion-review.md` and put its persistent evidence under `docs/bw-expansion-review/` or another clearly linked repository directory. Keep figures compact and retain full-size crops where grain matters. `/tmp` may hold large intermediates, but it must not be the only location of the final evidence.

Use a few representative photos: a portrait, foliage or coloured objects, and a difficult bright/dark scene. Use repository/previously authorised inputs or clearly licensed public examples with source/credit recorded. Do not search unrelated personal folders. If suitable photos are unavailable, render diagnostic charts and say explicitly that photographic appearance has not been assessed.

Produce two kinds of comparison:

- **Controlled comparison:** same source, crop, format, exposure policy and effects settings. Compare films through the same paper, then papers using the same film. Show what the changed material actually does.
- **Recommended appearance:** your proposed everyday default, including any deliberate exposure/contrast/grain tuning. Save the exact recipe. Keep these creative changes identifiable so they cannot be mistaken for new measurements.

Show both a normal view and a detail crop if making grain/sharpness claims. Do not change per-image settings silently to make one stock win. Judge skin/coloured-object brightness separation, highlight detail, usable shadows, grain scale, and whether the result is pleasant at ordinary viewing size. These are evaluations, not proof of a lab match.

The review should begin with one short recommendation and a table like this, filled with actual outcomes:

| Option | What you will notice | Evidence and limits | Status | Recommendation |
|---|---|---|---|---|
| New film + specified paper | Plain-language difference visible in the examples | Manufacturer curves used; texture or other assumptions identified | Ready to try / Experimental / Blocked / Not attempted | What the PM should do next |

Use **Ready to try** to mean integrated and checked for the scope claimed, not physically proven identical to real film. Use **Experimental** when important material behaviour is approximated. Use **Blocked** only to describe a specific unresolved requirement for an item; continue other work within the run. A status in this review does not instruct any goal-management tool status change.

Good decision request: “Use the balanced version as the default, or favour the punchier version shown in B? I recommend balanced because it retains more highlight detail.”

Bad decision request: “Which density threshold, developer dilution or CDF sigma should I use?” Resolve those from sources or keep the result provisional. Ask at most two material product questions at the end, each with a recommendation and visible consequences. If there is no meaningful decision, give a recommendation without manufacturing a question.

**9. Packaging, workspace care and finish**

The native app normally reads copied bundle data. Restart it against the intended data directory via the documented `SPEKTRAFILM_DATA_DIR` mechanism, or build a fresh package. Rebuilding only one binary does not update all preview/export components.

If packaging, choose a **new, unused** output directory, for example `dist/spektrafilm-bw-candidate-<unique-run-id>`. The package script removes its `DIST_DIR`, so inspect it and set this override explicitly. Reuse existing build caches where appropriate, but preserve previous release packages. Verify the candidate includes your exact profiles and binaries; use [the package verifier](../scripts/verify_macos_package.py) according to its current help. Do not claim a package was tested if you only ran the source CLI. A candidate package is for local review, not a public release.

At preparation time, untracked work included `.claude/`, `TODO.txt`, `problem.txt`, `dist/`, the research files, and performance-study files. Record your own starting status and preserve everything unrelated. Do not reset or clean the workspace. Do not include unrelated changes in any snapshot or commit. No commit or publication is required by this brief.

Before the deadline, leave `docs/handoff-bw-expansion-results.md` with files changed, generated artifacts, exact commands/settings, sources, pass/fail/skip results, unresolved issues and the next concrete step. Distinguish completed implementation from prototypes and research. If there is no usable new paper, identify the missing quantity and the best evidence-backed way to obtain or approximate it; do not simply write “paper needs more research.”

Final message to the PM: link the visual review and candidate app if available; state what they can try, why you recommend it, and the most material uncertainty. Use familiar language. Put detailed technical evidence in the linked notes. Never claim a real-film measurement, a validated paper, a finished build, or a test result you did not obtain.
