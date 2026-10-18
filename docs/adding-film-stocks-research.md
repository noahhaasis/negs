# Adding film stocks to Spektrafilm: research and practical guide

Research date: **18 October 2026**. Written for someone who can work with software but has no chemistry or film-photography background.

**You can begin adding useful B&W profiles without shooting film, developing it, or buying measurement equipment.** The most practical route is to extract published manufacturer data, turn it into a documented profile, and validate the result in the simulator. Colour profiles are possible too, but missing dye data and colour calibration make them substantially harder. A convincing rendering, agreement with a datasheet, and agreement with a particular lab's film output are three different levels of evidence.

This guide distinguishes what Andrea Volpato documented, what the current code does, and my recommendations. It does not introduce any new film profiles or change the application.

**What I examined.** I retrieved all available posts in the [technical discussion](https://discuss.pixls.us/t/spektrafilm-tech-discussions/57512), its [original discussion](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209), and the linked [digitisation campaign](https://discuss.pixls.us/t/2026-q2-data-sheets-digitization-campaign/58032): 140, 1,023, and 21 posts respectively. I searched the complete retrieved text and read the relevant discussions about data, measurements, profile construction, B&W, and model limitations. I checked manufacturer PDFs, including visually inspecting selected graphs, the author's public repository list and current upstream main tree, historical profile-generation code, the sibling Python checkout, and this Rust implementation. These are the posts available at retrieval, not a claim to have recovered deleted posts or read every linked page on the internet. A machine-readable [source inventory](film-stock-research-sources.json) records the scope and versions.

**1. Where the original data came from**

Andrea Volpato, `arctic` on PIXLS, described the original project in February 2025 as a simulation built from published film data and photographic principles. He is a trained chemist, but also said he was not an expert in darkroom techniques or colour science. His original input was manufacturer measurements published in technical documents, combined with models and corrections. That is different from personally measuring every stock. See his [opening explanation](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/1).

The most direct answer to your question is his [March 2025 explanation of adding profiles](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/143): he manually extracted PDF graphs with **WebPlotDigitizer**, reconstructed missing individual dye data, unmixed channels, and made small corrections so neutral ramps behaved sensibly. Portra was unusually straightforward; other negatives required more adjustment.

There is concrete provenance in the historical repository too. At commit `9428440eb2476910d8ada11a86ea11368ddd84d1`:

- The [Portra 400 source note](https://github.com/andreavolpato/spektrafilm/blob/9428440eb2476910d8ada11a86ea11368ddd84d1/src/spectral_film_lab/data/film/negative/kodak_portra_400/info.txt) identifies Kodak **E4050, 2016**.
- The [generic dye source note](https://github.com/andreavolpato/spektrafilm/blob/9428440eb2476910d8ada11a86ea11368ddd84d1/src/spectral_film_lab/data/film/negative/generic_a/info.txt) identifies **Film A, page 112**, in *Digital Color Management*. Thus some starting dye shapes came from textbook material rather than a stock-specific measurement.
- The [historical reconstruction code](https://github.com/andreavolpato/spektrafilm/blob/9428440eb2476910d8ada11a86ea11368ddd84d1/src/profiles_factory/reconstruct.py) adjusts generic dye shapes and masking contributions against the available spectral-density observations. This is evidence of a reconstruction procedure, not evidence that all missing quantities were independently known.

His [June 2026 account](https://discuss.pixls.us/t/2026-q2-data-sheets-digitization-campaign/58032/18) adds that profile making includes guessed graph continuations, donor data when information is absent, and individual repairs for inconsistent sheets. He identifies Fuji Crystal Archive Type II as an example with missing characteristic curves. The reliable conclusion is that the profiles combine published observations with modelling decisions; they are not untouched transcriptions of complete laboratory datasets.

He listed [125px's film archive](https://125px.com/docs/film/), Digitaltruth, Filmtypes, and Analogfilm.space as document-finding resources in [post 34](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/34). Later participants contributed manufacturer PDFs and archived links in [post 548](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/548). For new work, record the actual manufacturer document and revision, even if a mirror is where you found it.

**2. Did he run experiments?**

**Yes: extensive computational experiments and comparisons are documented. I did not find a documented campaign in these sources in which he exposed, developed, and spectrally measured all the films himself.** The original stock data were measured by manufacturers and other published sources. This finding does not prove he has never made a physical measurement.

Examples of his experiments include:

| Work | What it establishes | What it does not establish |
|---|---|---|
| Virtual exposure ramps and neutral ramps | How the model handles exposure and unwanted casts | That a real roll processed in a lab will match exactly |
| Simulated granularity and sharpness measurements | Whether the model can reproduce published statistical targets | The exact microscopic grain pattern of a particular emulsion |
| Comparing measured material spectra with reconstructed spectra | Errors in RGB-to-film exposure prediction | A new measurement of the film stock itself |
| Reviewing community film/print comparisons | Whether results look plausible and where the model may fail | A controlled calibration of every stock and process |

The [grain discussion](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/230) explains matching simulated density variation to manufacturer granularity measurements. In [post 601](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/601), he compares simulated sharpness with Portra's published MTF and describes the defaults as roughly tuned. Community physical comparisons are discussed in [post 600](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/600); they are useful evidence, but they are not a comprehensive stock-measurement programme.

Much of the technical thread is about a separate problem: **guessing plausible spectra from digital RGB**. Skin, leaves, paints, and minerals can have different spectra even when they share the same RGB appearance. Andrea and others used existing measured reflectance datasets to investigate that ambiguity. Those datasets inform the light presented to virtual film; they do not replace the film's sensitivity and development data. See [technical post 28](https://discuss.pixls.us/t/spektrafilm-tech-discussions/57512/28) and the [new upsampler description](https://discuss.pixls.us/t/spektrafilm-tech-discussions/57512/50).

His stated goal is a general model rather than an exact reproduction of one laboratory's output. He explains the variation between hypothetical labs in [technical post 44](https://discuss.pixls.us/t/spektrafilm-tech-discussions/57512/44). That is a useful expectation for your profiles as well.

**3. The photographic concepts you actually need**

You do not need to understand chemical reaction mechanisms to make a first profile. You need to understand what the graphs measure and how their numbers enter the software.

| Term | Meaning for this work |
|---|---|
| Spectral sensitivity | How strongly a film responds at each wavelength. This controls how coloured objects become film exposures, including in B&W. |
| Characteristic curve, or D–log H / D–log E curve | How much developed density results from each amount of exposure, under a specified processing recipe. |
| Density | A logarithmic measure of light blocking. Transmission density `D = -log10(T)`: density 1 transmits 10%; density 2 transmits 1%. |
| Base plus fog | Density present even in an unexposed, processed negative. It must be accounted for once. |
| Toe and shoulder | The low- and high-exposure ends of the characteristic curve. Their shape affects shadow and highlight behaviour. |
| Developer and development time | Processing choices that affect the curve and image structure. A stock name alone does not specify these. |
| Granularity | Measured density variation at a stated aperture and density. It is related to visible grain but is not a complete grain description. |
| MTF | How contrast changes with the size of fine detail; useful for assessing sharpness models. |
| Densitometer status | The spectral weighting of the instrument that measured density. Important when translating colour measurements into dye contributions. |

For a beginner, Kodak's free [*Basic Photographic Sensitometry Workbook*, H-740](https://www.kodak.com/content/products-brochures/Film/Basic-Photographic-Sensitometry-Workbook.pdf) is a better starting point than a chemistry textbook. It explains density, characteristic curves, development families, speed, and measurement. One stop of exposure is a factor of two, equivalent to approximately `0.30103` on a base-10 log-exposure axis. A whole log-exposure unit is about 3.32 stops.

For conventional B&W negatives, more exposure usually produces more density. Printing that negative on light-sensitive material reverses the tonal relationship again. A slide/reversal film produces a positive directly. This distinction matters when choosing a template and judging a direct scan.

**4. What your current application already supports**

I checked the Rust checkout at `567a43132f64538ec6d877c8d015e319139b80b3`. It contains **31 film/print profiles, three of them B&W**. The B&W files were imported from Python experimental `28bf883e1672e884307edc75852549376e13644e`, as recorded in the existing [B&W handoff](handoff-bw-stocks.md).

| Profile ID | Actual material | Role |
|---|---|---|
| `kodak_doublex` | Double-X 5222 | B&W camera negative; development family |
| `kodak_trix` | Tri-X **7266 reversal** | B&W positive; **not Tri-X 400 / 400TX** |
| `kodak_2302` | Kodak 2302 | B&W motion-picture print film; **not photographic paper** |

These identities come from the files themselves: [Double-X](../data/profiles/kodak_doublex.json), [Tri-X](../data/profiles/kodak_trix.json), and [2302](../data/profiles/kodak_2302.json). The current public upstream main tree I checked, `3bb2c2d2801ff68b92019cf1dbcbb133d60832bc`, contains 28 profiles and does not contain these three B&W files. Branch and commit matter more than a version number alone.

Normal stock additions can be **data-driven**. The [native catalogue](../crates/spektrafilm-native/src/main.rs) enumerates JSON files in `data/profiles/`; the [profile loader](../crates/spektrafilm-core/src/profile.rs) reads them. You generally do not need a new Rust enum entry or a new shader for an ordinary stock that fits the existing model.

There are four significant limits:

- **The wavelength grid is 380–780 nm in 5 nm steps**, or 81 samples. The [spectral constants](../crates/spektrafilm-math/src/spectral.rs) and existing tables assume this domain. Infrared film extending beyond it is a larger engine change.
- B&W development families select the **nearest stored time**, with the middle entry as the default. The current loader does not continuously interpolate a measured development-time surface.
- A stock JSON does not contain a complete stock-specific grain or halation calibration. The [runtime parameters](../crates/spektrafilm-core/src/params.rs) hold those controls; selecting another stock does not automatically load its published RMS granularity number. Density curves can still change the resulting texture.
- Native macOS builds normally use the **data copied into the app bundle**. Editing repository data will not update an already packaged copy. The [resource locator](../macos/Sources/Spektrafilm/Engine.swift) supports `SPEKTRAFILM_DATA_DIR`, and the [packaging script](../scripts/package_macos_app.sh) copies `data` into the bundle.

**5. Which B&W stocks are good starting points?**

These are recommendations based on data availability and compatibility with the existing engine. They are not claims that the resulting profiles have already been built or validated.

| Candidate | Data located | Assessment |
|---|---|---|
| **Kodak Tri-X 400 / 400TX** | F-4017: labelled log-sensitivity curves, characteristic-curve families, granularity, MTF | Strong first project; also fills a gap hidden by the existing `kodak_trix` name |
| **Kodak T-MAX 100** | F-4016: quantitative sensitivity and development curves, granularity, MTF | Strong first or second project; a useful fine-grain comparison |
| **Fujifilm Acros II** | AF3-0258E / AF3-0259E: sensitivity, characteristic curves, granularity, processing information | Good next project; preserve the film generation and format distinction |
| **Ilford HP5 Plus** | Current sheet, an older equal-energy relative log-sensitivity graph, and historical XTOL development families | Feasible, with unusually useful historical sources; identify their age and recipe |
| **Fomapan 100 Classic** | Manufacturer graphs, development family, granularity | Useful alternative; check how its tungsten spectrogram is defined before converting it |
| FP4 Plus, Delta 100/400, T-MAX 400/P3200 | Historical XTOL curve families found; other required data need a stock-by-stock audit | Promising expansion set, with careful checking of film generations |
| XP2 Super | Manufacturer describes a chromogenic B&W film | Treat separately: its dye image and grain behaviour make the conventional silver approximation less suitable |

Primary sources supporting this ranking: [Kodak F-4017](https://kodakprofessional.com/sites/default/files/wysiwyg/film/f4017_trix_320400.pdf), [Kodak F-4016](https://kodakprofessional.com/sites/default/files/wysiwyg/pro/resources/f4016_TMax_100.pdf), [Acros II 135](https://asset.fujifilm.com/www/us/files/2020-08/73080443d0289b1a7b116add14a9bdfc/films_neopan100acros2_135_01_0.pdf), [HP5 current sheet](https://www.ilfordphoto.com/amfile/file/download/file/1903/product/691/), [older Ilford sheet, mirrored](https://www.99tarbox.com/rhoadescameras/Camera%20PDFs/Ilford%20HP5%20Plus.pdf), [Foma's product data](https://www.foma.cz/en/fomapan-100), and [Ilford's XP2 explanation](https://www.ilfordphoto.com/amfile/file/download/file/1909/product/703/XP2).

My suggested sequence is **Tri-X 400 → T-MAX 100 → HP5 Plus or Acros II**. Choose HP5 earlier if that is the stock you most want; the older spectral graph makes it a realistic option.

**6. A practical B&W profile-making process**

**First, define the exact target.** A useful profile identifies the stock, generation, format, developer, dilution, temperature, development time, and agitation where known. For example, “Tri-X 400, 35 mm, D-76, the processing conditions specified by the chosen curve” is a meaningful target. “Tri-X look” leaves too many variables unresolved. Initially choose one developer and one measured development time.

**Next, assemble a source packet.** Keep the PDF, its document number/revision, the pages used, extracted graph images, digitisation projects, original numeric points, processing script, assumptions, and output profile. Keep published observations separate from corrected and extrapolated values. The current profiles' general `metadata.datasource` text is not enough to reconstruct every per-stock decision; you can improve on that for new work.

**Digitise spectral sensitivity.** In [WebPlotDigitizer](https://automeris.io/docs/digitize/), load the PDF or graph image, calibrate both axes using widely spaced known points, trace the curve, and export the numerical data. Save the project as well, because its calibration is part of the evidence. The [export documentation](https://www.automeris.io/docs/data/) describes preserving data and project information.

There are several traps:

- An axis labelled **log sensitivity** already contains logarithms. If the tick values are, for example, 1, 2, and 3 at even spacing, calibrate those coordinates linearly. Do not apply a second logarithmic-axis interpretation.
- Choose the curve for the stated density criterion. Some Kodak sheets supply sensitivity at both 0.3 and 1.0 above minimum density. They are not duplicate curves.
- Preserve relative spectral shape. Do not independently rescale colour regions until they look attractive.
- An underbar or bar over a digit in older Kodak log notation can indicate a negative characteristic. OCR may drop it. Check the image rather than relying on extracted PDF text.
- A graph that ends does not prove sensitivity is physically zero immediately beyond the last plotted point. Record the measured domain and any cutoff or continuation you assume.

**Take special care with Ilford's spectral plots.** The November 2018 HP5 sheet labels its graph as a tungsten-light wedge spectrogram. The older eight-page sheet also provides an **equal-energy relative log-sensitivity curve**, with a labelled vertical axis. I visually checked both. For a first implementation, use that explicit curve, identify its source age, and verify that using older data is appropriate for your intended stock generation. Do not casually treat the newer tungsten trace as the same numeric quantity. If converting an illuminant-weighted trace, establish whether its ordinate is linear, logarithmic, or some other presentation before removing the illuminant contribution. The general operation is division in linear sensitivity, or subtraction in log sensitivity, only when the measurement definition actually supports it. The [older primary document](https://www.99tarbox.com/rhoadescameras/Camera%20PDFs/Ilford%20HP5%20Plus.pdf) is particularly useful here.

**Digitise the characteristic curve separately.** Preserve its exposure units and processing caption. Trace enough points around the toe and changing curvature, not just a few points in the straight region. Keep each development time separate. A development-time table or a contrast-index graph is not a full density-versus-exposure curve: neither specifies the whole toe and shoulder.

**An especially useful source is the [German XTOL booklet](https://125px.com/docs/techpubs/kodak/xtolEntwickler.pdf) linked in the original discussion.** Its printed pages **32–34** (PDF pages **34–36**, counting from one) contain characteristic-curve families annotated with contrast-index values. The heading says “Kontrastindex-Kurven (CI),” but the figures contain much more than a CI-versus-time graph. I visually inspected all three pages; text extraction misses most of their content.

They cover Kodak films, Agfa films, HP5 Plus, FP4 Plus, Delta 100/400, Pan F Plus, and Fuji 400/1600. For example, the HP5 panel identifies XTOL at **21°C**, with curves for **4, 6, 8, 10, and 13 minutes**. The captioned CI values run from **0.39 to 0.87**. These are historical source observations, not measurements made during this research. They can support a documented HP5/XTOL development family, subject to verifying the film generation, exposure-axis interpretation, and processing conditions. Do not assume that an old Kodak or Fuji film name identifies its current successor. Development-time tables elsewhere in the booklet remain recommendations rather than full response curves.

**Separate base/fog from image density.** For a negative, the low-exposure baseline can inform base plus fog. If the source curve includes that baseline, subtract it from the image-density curve before also representing it in `base_density`. Adding it in both places would count it twice. The current conventional B&W profiles use a wavelength-independent `channel_density` of 1: a practical neutral-absorber model. That is a reasonable declared starting assumption, not proof that all real bases and silver images are spectrally neutral.

**Fit and normalise the curve.** The engine expects a usable profile over its working exposure domain, not arbitrary points in the PDF's original coordinate system. Use a smooth monotonic interpolant or a justified fitted model. Compare the result with the original points, and label extrapolation outside their range. If the source never reaches a plateau, its last plotted point is not a measured maximum density.

The bundled B&W files use sums of normal cumulative-distribution functions. Those fits support smooth curve behaviour and a sublayer representation; they do not uniquely reveal the actual film's microscopic layers. The Rust loader evaluates a supplied B&W fitted model as authoritative, so changing only the sampled curve while retaining the old fit will not work. Either regenerate all related fields coherently or deliberately use the supported sampled-curve fallback, with its limitations. See [the loader](../crates/spektrafilm-core/src/profile.rs).

Exposure normalisation deserves its own explicit step. Inspect an existing compatible profile's response to the reference illuminant and a neutral ramp, then document how the new curve's exposure origin is chosen. The [historical factory](https://github.com/andreavolpato/spektrafilm/blob/9428440eb2476910d8ada11a86ea11368ddd84d1/src/profiles_factory/factory.py) explicitly shifted the log-exposure origin; it is evidence that preprocessing is needed, not a current B&W calibration recipe to copy blindly. There is no simple `ISO` field in the Rust profile that creates all of this behaviour for you.

**Resample and write the JSON.** Use exactly the engine's wavelength grid. Preserve the distinction between an unmeasured value and a measured numeric value. For log sensitivity, `0` means sensitivity **one** after exponentiation; it does not mean no sensitivity. The current loader preserves `null` as missing and the spectral calculation gives it zero contribution. This is an engine convention: if you use it outside the measured domain, document the resulting truncation assumption.

**Validate without texture first.** Disable stochastic grain, halation, sharpening, and other spatial effects while assessing the curves. Test a neutral exposure ramp, coloured patches, dark and bright inputs, and the toe/shoulder extensions. Compare computed densities with source data before judging final RGB. A correct-looking image can hide a mistaken exposure origin or duplicated fog.

**Then choose a viewing path and calibrate texture.** Existing 2302 can provide a temporary B&W print path, but its result is a camera-negative plus motion-picture-print-film combination. It is not an Ilford photographic-paper rendering. Direct negative scans are useful diagnostics and should be interpreted according to the scan route, rather than assumed to be finished positive photographs. Add stock-specific grain settings separately and compare granularity under the stated physical sampling conditions. A single RMS number cannot uniquely determine grain shape or its behaviour at every exposure.

**Finally, extend the profile only with evidence.** Once one development curve works, add other measured times for the same processing recipe. Do not relabel a gamma adjustment as a measured push process. Exposure index, actual exposure, and changes to development are separate variables.

**7. The JSON and integration details for this Rust port**

Use [Double-X](../data/profiles/kodak_doublex.json) as the structural reference for a conventional B&W negative. Use [Tri-X 7266](../data/profiles/kodak_trix.json) only when modelling a reversal stock. Copying a file is a schema aid; replace its calibration and provenance before calling it a different stock.

For a B&W negative, the important identity fields are:

```json
{
  "stock": "kodak_trix400_d76_35mm",
  "name": "Kodak Tri-X 400 / 400TX — D-76, 35 mm",
  "type": "negative",
  "support": "film",
  "stage": "filming",
  "use": "still",
  "channel_model": "bw",
  "densitometer": "diffuse_visual"
}
```

This is **an illustrative `info` fragment**, not a complete loadable profile and not a claim that this calibration exists. Fill the other metadata and measurement fields from your chosen source, including the sensitivity density criterion and reference illuminant. Do not keep donor values merely because the file loads.

For `T` measured development times, the current B&W layout is:

| Field | Shape / meaning |
|---|---|
| `wavelengths` | 81 values, 380 to 780 inclusive |
| `log_sensitivity` | `81 × 1` |
| `channel_density` | `81 × 1`; conventional bundled B&W stocks use ones |
| `base_density` | `81 × T`, or one shared column |
| `log_exposure` | `N` increasing coordinates; bundled files use 256 samples |
| `density_curves` | `N × T`; columns are development times, **not RGB** |
| `development_time` | `T` stored times |
| `density_curves_layers` | When present, `N × sublayers × T` |
| `density_curves_model` | When present, parameter rows index development time |

The 256 exposure samples are the bundled convention; the loader chiefly requires compatible lengths. Use the established convention initially. The engine broadcasts the B&W channel internally; do not manually turn its development-family columns into RGB channels.

The current B&W fitted-model loader accepts `cdfs` and `norm_cdfs`. The newer experimental Python schema also discusses `sept_norm_cdfs` and skew parameters, which are not supported by that Rust B&W loading path. Future upstream profiles may therefore need a compatible export or additional porting work.

Once constructed:

1. Save the file as `data/profiles/<stock-id>.json` and make `info.stock` match that ID.
2. Validate its shapes, finiteness, monotonic axes, source overlays, and fitted/sampled consistency. A valid JSON document alone is insufficient.
3. Restart the application so it reloads its catalogue. Confirm it is using your edited data directory, or rebuild the macOS package.
4. Supply a print stock explicitly, or set an intentional `target_print`. A B&W negative with no target is not a reversal stock.
5. Run deterministic rendering checks, then the relevant Python/Rust comparison with the same profile and compatible model on both sides.

Illustrative CLI use **after creating the proposed profile**:

```bash
./target/release/spektrafilm list-profiles --data-dir data

SPEKTRAFILM_BACKEND=cpu ./target/release/spektrafilm-f64 process input.tif \
  -o trix400-2302.tif --film kodak_trix400_d76_35mm \
  --paper kodak_2302 --data-dir data
```

These commands were checked against the CLI definitions; the proposed new stock was not created or rendered during this research. The existing [parity runner documentation](../scripts/parity/README.md) explains development-time selection, direct scans, and controlled comparisons. **Parity proves two implementations agree; it does not prove the underlying film model is physically accurate.**

For colour film/print combinations, inspect [the neutral-filter database](../data/filters/neutral_print_filters.json). Its keys identify print stock, illuminant, and film stock. [Pipeline construction](../crates/spektrafilm-core/src/pipeline.rs) falls back to parameter defaults when the combination is absent. A new colour stock therefore also needs an intentional neutral-print calibration; otherwise a cast can be a missing database entry rather than faulty stock data. B&W printed onto colour paper also needs deliberate colour balancing.

**8. Why colour profiles require more work**

For colour you normally need three sensitivity functions, three characteristic curves with the correct measurement conventions, base/minimum and midscale spectral-density information, and a model for the individual dye contributions. Some PDFs provide only the **combined** spectral density of a neutral film patch, not separate spectra for cyan, magenta, and yellow.

The model concept is approximately:

```text
total spectral density = base spectral density
                       + sum of channel amount × channel spectral contribution
transmission = 10 ^ (-total spectral density)
```

A total spectrum does not uniquely determine the individual contributions. Moreover, a colour densitometer's red reading can include absorption from more than one dye. A plotted red density curve is therefore not automatically a pure cyan-dye amount. The [historical factory](https://github.com/andreavolpato/spektrafilm/blob/9428440eb2476910d8ada11a86ea11368ddd84d1/src/profiles_factory/factory.py) explicitly contains crosstalk and unmixing operations.

Masking couplers add another complication. The orange mask changes as image dyes form. The model can represent a decreasing masking component as a negative contribution in an individual effective channel spectrum. That does not mean negative physical absorption is allowed for the complete film. Do not clip every negative channel contribution indiscriminately. Andrea explains this modelling choice in the [upstream introduction](https://github.com/andreavolpato/spektrafilm/blob/main/README.md).

Development-inhibitor-release (DIR) interactions are another model component. A neutral characteristic curve does not uniquely identify how differently exposed colour layers inhibit one another, so digitising three curves is not enough to determine every saturation or adjacency-effect parameter. Treat generic coupler settings as model assumptions unless independently calibrated; Andrea's [original presentation](https://discuss.pixls.us/t/spectral-film-simulations-from-scratch/48209/1) explicitly describes the saturation amount as a guess.

A sensible colour workflow is: digitise the available observations; reconstruct or obtain missing dye components; account for the measuring instrument; normalise exposure and spectral response; check that simulated measurements recover the published observations; fit print balance; then inspect neutral ramps and colours across exposure. Keep source points, fitted parameters, and calibration changes separate.

Useful candidates and limitations:

| Candidate | What I verified | Recommendation |
|---|---|---|
| **EASTMAN EXR 100T 5248 / 7248** | March 1999 Kodak sheet includes characteristic, sensitivity, minimum/midscale spectral-density, granularity, and MTF graphs | Strong candidate for a first additional colour negative after the B&W workflow works; still requires dye reconstruction |
| EASTMAN 5247 | Kodak TI0835, revised June 1993, is available | Interesting historical target; distinguish revisions rather than promising a universal 5247 look |
| HARMAN Phoenix II | July 2025 sheet supplies sensitivity and processing/characteristic information; Andrea notes the missing developed-film absorption data | An experimental donor-dye profile may be useful, but cannot be presented as fully measured |
| Other Fuji slides / colour stocks | Many manufacturer sheets and archived links exist | Audit each individually; availability of a PDF does not establish completeness or consistency |
| Aerochrome / other IR stocks | The discussion explores extrapolating near-IR behaviour | Defer: wider spectral support and missing information in ordinary RGB input make this much more than adding a JSON file |

Sources: [Kodak 5248 sheet](https://125px.com/docs/motionpicture/kodak/5248.pdf), [Kodak 5247 TI0835](https://125px.com/docs/motionpicture/kodak/ti0835.pdf), [Phoenix II sheet](https://www.harmanphoto.co.uk/amfile/file/download/file/1968/product/2208/), [Andrea on Phoenix's missing absorption data](https://discuss.pixls.us/t/2026-q2-data-sheets-digitization-campaign/58032/7), and [his discussion of near-IR extrapolation](https://discuss.pixls.us/t/spektrafilm-tech-discussions/57512/76).

For 5248, the spectral-density graph supplies minimum and midscale totals; it does **not** eliminate the need to reconstruct separate dyes. Its tungsten balance must also be represented consistently. An attractive frame from a movie is a weak calibration target because lighting, printing, timing, scanning, and grading all affect it.

**9. The profile creator is a real missing piece**

It would be misleading to tell you there is currently a verified, supported “import a PDF → create an accurate stock” command.

Commit [`dfb0219`](https://github.com/andreavolpato/spektrafilm/commit/dfb0219405506e2393652e880f95c7bf6a5f68e5), dated 11 May 2026, records the extraction of the profile creator. In the [June follow-up](https://discuss.pixls.us/t/2026-q2-data-sheets-digitization-campaign/58032/18), Andrea said a public working suite was intended but needed documentation, maintenance, and automation of manual judgement. As of this research, I did not find that maintained suite in his public repositories, the checked main tree, or the sibling experimental checkout.

Historical code survives in this local Git history and is useful for understanding the methods. Some wrappers import `spektrafilm_profile_creator`, but an import name in old code does not establish that the package is available. Old raw data and reconstruction routines are educational starting points, not a demonstrated way to regenerate today's profiles exactly. The runtime JSONs are **processed outputs** of this preparation work.

If you choose to contact Andrea yourself, the useful request is specific: access to the current creator, current B&W exposure-normalisation and fitting recipes, raw digitised points, and whether a small contribution of source PDFs plus WebPlotDigitizer projects would be helpful. I have not contacted him or posted on your behalf.

**10. What experiments would help later?**

For the first B&W additions, **start with published data and software validation**. Own measurements become valuable when a source lacks important data or when you want to characterise a particular processing workflow.

| Goal | Suitable work | Limits |
|---|---|---|
| Check general appearance | Photograph repeatable scenes and exposure brackets on film and digital; record processing and the complete output path | Scans and prints introduce additional rendering decisions |
| Measure a B&W tone curve | Known exposure steps or a sensitometer/step-tablet setup, controlled processing, calibrated transmission-density measurements, repeats | Characterises that recipe and sample; does not measure spectral sensitivity |
| Measure spectral sensitivity | Calibrated narrowband exposures with known energy at each wavelength; determine exposure needed for a chosen density criterion | Requires control of bandwidth, stray light, energy, processing, and density measurement |
| Obtain missing colour-dye spectra | Suitable spectral transmission measurements plus a justified way of separating layer/dye contributions | A basic RGB scan cannot uniquely supply all the missing spectra |
| Validate grain | Uniform exposures, known physical scale, suitable density measurement/sampling, repeated patches | Scanner noise, sharpening, compression, and blur must be separated from film variation |

This is my suggested measurement strategy, based on the quantities the model needs. Kodak's [sensitometry workbook](https://www.kodak.com/content/products-brochures/Film/Basic-Photographic-Sensitometry-Workbook.pdf) provides the basic exposure/step-tablet/densitometer framework; its treatment does not by itself specify every part of a modern spectral profiling rig.

A linearised camera or scanner can help estimate B&W transmission in a controlled setup, but requires dark-offset correction, illumination normalisation, and checks for clipping and nonlinearity. An ordinary lab JPEG is already a processed image. Treating its pixel values as raw transmission would mix the scanner's rendering into your film curve.

The camera spectral-measurement setups discussed late in the technical thread are primarily about **digital sensor sensitivities**. They should not be confused with a ready-made method of measuring undeveloped film: film requires exposure followed by processing and density assessment. A monitor-calibration spectrometer also should not be assumed to support the spectral transmission measurements needed for film.

**11. B&W photographic paper deserves its own project**

Adding camera stocks expands the available tonal and spectral responses. Adding a B&W photographic-paper profile would expand the output medium, which can matter just as much to the final appearance.

A fixed-grade paper, or a clearly specified fixed-filter rendering of one paper, is a manageable first target. A proper **variable-contrast** paper is more involved: its blue/green-sensitive components respond differently to the spectrum of the enlarger light, changing the combined characteristic curve. Ilford describes this mechanism in its [Multigrade contrast-control document](https://www.ilfordphoto.com/amfile/file/download/file/1824/product/2078/). Merely applying a gamma slider is an approximation to that behaviour.

The existing one-channel B&W material with a single spectral response cannot by itself express independent spectral exposures for those mixed components. A set of documented fixed-grade profiles is a practical intermediate option; faithful continuous filter-controlled behaviour needs additional modelling. Andrea explicitly identified this issue in the [B&W roadmap discussion](https://discuss.pixls.us/t/2026-q2-data-sheets-digitization-campaign/58032/15).

XP2 is another case to keep distinct. Ilford identifies it as a **dye-image chromogenic film** and describes an exposure/grain relationship unlike conventional B&W. It may fit a one-channel approximation creatively, but the neutral-silver and generic-grain assumptions need reconsideration. See [the XP2 datasheet](https://www.ilfordphoto.com/amfile/file/download/file/1909/product/703/XP2).

**12. A realistic contribution plan for you**

1. **Learn to read the graphs.** Work through the relevant parts of H-740 and inspect the existing Double-X profile beside its source data. You can do this without handling film.
2. **Choose one stock and one recipe.** My default recommendation is Tri-X 400 / 400TX, clearly distinguished from the bundled reversal film. T-MAX 100 is equally reasonable if fine grain interests you more.
3. **Make the source packet.** Collect the exact PDF, trace sensitivity and one characteristic curve, preserve calibration projects, and list missing quantities. This is already a useful contribution even before the final JSON is ready.
4. **Build a small reproducible B&W preparation script.** It should read original points, handle baseline and exposure normalisation explicitly, fit/interpolate, resample, and generate both the profile and validation plots. Use the existing schema and avoid hidden edits to the final JSON.
5. **Validate the response.** Check source overlays, neutral and colour ramps, exposure extremes, and a controlled print path. Then add a documented texture preset. Request review of assumptions rather than asking whether a single attractive photo looks film-like.
6. **Repeat with a second stock.** This tests whether the method generalises or accidentally encodes the first film.
7. **Add measured development families and a paper.** Keep developer identities separate. Start with a fixed paper/grade before variable contrast.
8. **Attempt a well-documented colour stock.** EXR 5248 is a defensible next candidate once dye reconstruction and neutral-print calibration are understood or the current creator becomes available.

For each finished profile, use a short provenance record such as:

```text
Material / generation / format:
Manufacturer document / revision / page / URL / file hash:
Developer / dilution / temperature / agitation / development time:
Spectral ordinate definition / density criterion / measured wavelength domain:
Characteristic-curve units / base+fog treatment / exposure-origin convention:
Raw points / digitisation project / processing script revision:
Borrowed data / extrapolation / normalisation changes:
Grain, halation and sharpness sources or explicitly generic settings:
Validation performed / known limits:
Profile author / upstream attribution / licence and modification notice:
```

Preserve the attribution and licence notices when modifying upstream profiles; the bundled profile metadata identifies CC BY-SA 4.0. Use your own author/modification information honestly rather than retaining a donor profile's claim of authorship for new work. Keep the exact source document identifiable instead of redistributing manufacturer publications unnecessarily.

The achievable first result is a **documented, datasheet-based B&W profile with explicit assumptions**. Real-film comparison can strengthen it later. Neither a chemistry degree nor prior film shooting is a prerequisite for that first stage; careful interpretation, reproducible data preparation, and honest validation are the skills that matter most.
