# Early upscale and Image Size — implementation handoff

Status: implemented on `early-upscale`; see [image-size.md](image-size.md) for the
feature, validation results and remaining limitations.
Prepared 2026-10-25 against Negs `6e70e5e` and Python `spektrafilm@28bf883`.

Work here:

```sh
cd /Users/noahhaasis/Projects/negs/.claude/worktrees/early-upscale
git status --short
```

Branch: `early-upscale`, created from `main` at `6e70e5e`. Implement and
commit on this branch. The main checkout has unrelated untracked work;
leave it alone. This document is the implementation entry point. Read the
[independent Opus review](upscale-opus-review.md) for its source findings,
then follow the decisions below, which supersede the original draft.

## Goal and scope

Restore the Python app's early enlargement: resize the digital input before
film simulation, including halation and grain. Keep Negs's existing crop,
straighten, rotation, borders, Fast export, and Reference export.

The user requested this after reading mikae1's
[first comment](https://discuss.pixls.us/t/gpu-accelerated-macos-gui/61485/20),
[clarification](https://discuss.pixls.us/t/gpu-accelerated-macos-gui/61485/22),
and [3× comparison](https://discuss.pixls.us/t/spektrafilm-darktable-module-implementation-discussion/58744/283).
The comparison shows why grain must be generated at the enlarged working
size. The requested interface is a longest-side pixel target that accounts
for cropping.

In scope: the shared Rust resolver/resampler, CLI and native helper,
native macOS and Windows interfaces, sessions, undo, preview identities,
focused tests, visual evidence, and relevant documentation. Legacy egui
must continue compiling and keep its current factor control; a new egui
Image Size interface is deferred. Viewport rendering beyond the current
8192 px display proxy, new sharpening, and AI upscaling are deferred.

## Product decisions

Add **Image Size** in Lab, after Film & Paper and before Camera. Remove
the existing Working resolution slider from Output.

```text
Image Size
Size             [Original / Longest side]
Longest side     [6000] px
Photograph       6000 × 4000 · 24 MP

Resizes before film simulation, including halation and grain.
```

- Original sets the legacy factor to 1 and clears the pixel target. The
  crop trims the source's native pixels.
- Longest side accepts 16–65,535 px, preserves aspect ratio, and describes
  the **cropped photograph before borders**, Instagram extension, and
  export reduction. Smaller targets also work.
- Commit the text field on Return or focus loss. Invalid input must not
  silently clamp, reset the image, or replace the last good preview.
- Selecting Longest side uses the last valid target in platform preferences;
  otherwise initialize it from the current crop's native longest side,
  bounded to the field's range. Persist that remembered value separately
  from the current edit. A remembered value does not activate the mode.
- A restored factor other than 1 appears as **Scaled N% (older edit)** in
  the Size picker until the user chooses another mode. Loading an old edit
  must not silently convert its factor into a crop-dependent pixel target.
- Reset Adjustments preserves the new pixel target, like crop, rotation,
  and borders. The legacy factor resets to 1, as it does today. Original
  explicitly resets both size mechanisms.
- Keep Limit preview to 12 MP in Output. The Photograph readout always
  describes the uncapped export photograph. Label reduced previews and
  reduced display proxies. The app must not imply output-pixel inspection
  when its texture has fewer pixels than the working render.
- The export panel shows the photograph dimensions and existing border/fit
  summary. Exact bordered file dimensions are not required for v1; do not
  duplicate border geometry in Swift to obtain them.

## Parameters and processing contract

Add `io.photo_long_side_px: Option<u32>`, default `None`, with serde default
handling. Keep it in engine parameters so existing Swift Codable sessions,
restoring defaults, undo, CLI params files, and helper messages can carry
it without a new non-optional `EditState` field.

`None` uses the existing `io.upscale_factor`; None plus factor 1 is Original.
The interface writes factor 1 when setting a target. Helper and CLI reject
a non-null target combined with a factor other than 1 after deserialization.
Never multiply the two or silently pick a winner. Preserve valid legacy
factors, including factors above 1 used through the CLI; reject non-finite
or non-positive factors before resizing.

Keep the existing `--long-edge`, `--short-edge`, and `Fit` semantics:
they reduce the finished, bordered file after simulation. The new early
target travels through `--params`; no new CLI flag is necessary for v1.
Document a params example with `photo_long_side_px` and `--crop`.

Use this order:

```text
load/decode → resolve size → resize source → film/halation/grain
            → print/scan/post → existing crop/straighten/rotation
            → borders/Instagram extension → optional export reduction
```

Do not reuse `io.crop`, `crop_center`, or `crop_size`: they are unused
Python-derived fields with different intended crop semantics.

The crop stays after simulation. Enlarging the full source preserves the
full-frame film-format interpretation: physical pixel size is film format
divided by the full working longest side, as in `filming::pixel_size_um`.
For a 6000 × 4000 source, a central 0.5 × 0.5 crop, and a 6000 px target,
the working image is 12000 × 8000, and the photograph is 6000 × 4000.
Admission checks must use the 96 MP working frame, not the 24 MP crop.

## Shared resolver and float resampler

Put the shared implementation in `spektrafilm-io`, beside crop/resize;
Windows already depends on this crate and does not depend on core.

For source `W0 × H0`, crop fractional extent `cw × ch` (1 × 1 if absent),
and target `T`, resolve in f64:

```text
s = T / max(cw * W0, ch * H0)
W = max(1, round(W0 * s))
H = max(1, round(H0 * s))
```

Ignore crop origin and angle when resolving the working dimensions. The
crop tool's extent already reflects its straighten constraints. This keeps
panning from changing the simulation. Quarter-turn rotation does not change
the longest side. Preserve current legacy-factor rounding on that path.

Use `Crop::pixels` or `straightened_size` afterward to report actual cropped
dimensions. Accept a longest-side difference of at most one pixel from the
requested target. Do not repair rounding by enlarging the developed image.

Return a fallible resolved result containing working dimensions and actual
crop dimensions. Validate crop geometry, non-zero source dimensions, target
range, checked pixel/channel/byte arithmetic, and vector-size bounds before
allocating resampled or pipeline buffers.

Add a named initial limit of **128,000,000 full working pixels** for fixed
pixel targets and legacy enlargement requests. This is a provisional product
bound, not a RAM guarantee. It permits approximately 3× enlargement of a
12 MP camera image (108 MP). The review suggested about 100 MP; 128 MP is
chosen to include that use case. Keep native-size and legacy reduction
behavior for existing large inputs. Adjust this bound only from recorded
memory/performance evidence, and add boundary tests for the chosen value.
The 12 MP preview cap must not bypass validation of an impossible export
target. Source decoding itself can still allocate its original image.

The existing `image::imageops::resize` on `Rgb<f32>` clamps output to [0, 1]
in locked `image 0.25.10`, clipping RAW highlights before film/halation.
Replace **all early bicubic resize paths** with one shared parallel,
separable Catmull–Rom implementation that preserves values above 1. Floor
negative interpolation undershoot at zero. Keep kernel/support, normalized
weights, pixel centers, edge handling, accumulation order, and vertical then
horizontal pass order aligned with the current helper to limit unrelated
changes. Retain the existing f32 interpolation boundary in both builds;
do not introduce a separate f64 interpolation algorithm in this change.

This is an intentional highlight-clipping bug fix for legacy reductions and
12 MP previews too. Do not promise all old renders remain pixel-identical:
differences can arise for source highlights **and cubic overshoot above 1**
even if the source samples lie in [0, 1]. Compare unchanged regions where
the old upper clamp never activated. Do not claim Python pixel parity:
Python uses a different cubic resampler.

## Size ownership and entry points

Provide shared fallible working-image preparation that resolves the request,
checks limits, resamples at most once, and returns processing parameters with
`upscale_factor = 1` and `photo_long_side_px = None`.

- **CLI:** after load, prepare using the existing `--crop` argument, then
  build/run the pipeline with normalized params. Apply the crop afterward.
  Both Fast and Reference use the same preparation policy.
- **Native Render:** add optional `crop: Option<Crop>` to the request, sent
  in Longest side mode for sizing only. Resolve uncapped working dimensions
  before applying preview policies. Resize from the retained source once
  and clear both size mechanisms before `Pipeline::with_params`.
- **Core/library:** use the shared float resampler in `rescale_working`.
  A target reaching a core-only entry point means a full-frame target;
  resolve it through the same policy, not a second implementation. Propagate
  invalid requests through fallible preparation/processing rather than
  treating caught allocation panics as validation. Preserve existing call
  sites with compatibility wrappers if an additive fallible API is needed.
- **LUT export:** clear the new target in `lut_export::per_pixel_params`,
  alongside the factor and spatial settings. Image Size must never enlarge
  the LUT sample grid or change its entries.
- **Original:** accept params containing a target but ignore that target.
  Show Original at no more than source resolution; the canvas aligns it.
  macOS uses its existing ImageIO/Core Image path, Windows uses the helper.
  No enlarged Original buffer is needed.

Keep the current Windows backend rejection → CPU fallback behavior and
report the actual backend. `image_capacity` is a no-op on macOS and CPU;
it is not host memory protection. The resolver's bound and arithmetic checks
must happen before enlargement independently of GPU admission. Keep full
working-size validation for all new target consumers.

## Preview dimensions, identities, and crop lifecycle

Keep these dimensions distinct in the helper and clients:

| Dimension | Meaning |
|---|---|
| Source | Oriented decoded input |
| Working | Uncapped full frame required for the requested photograph |
| Export photograph | Actual crop applied to the working frame, before borders |
| Preview render | Floating-point render after the optional 12 MP/drag policy |
| Display proxy | RGBA texture, currently at most 8192 px on its long side |

Add `working_width/height` and `photo_crop_width/height` to native responses.
Retain `photo_width/height` compatibility for current preview-canvas layout.
Readouts use the new uncapped values. Store the source/working metadata needed
to restore these readouts with a startup cache; invalidate old cache records
that lack it. Display reduction labels must cover both a reduced render and
the 8192 px proxy. True viewport rendering remains deferred.

Drag previews must **never enlarge the source**. Calculate the desired frame
from the current preview/photo dimensions and `interactive_scale`, then
uniformly reduce that frame as needed to fit both source dimensions. Use
`downsample_area` only within that bound, or borrow the source if equal.
Enlarge only the settled frame, subject to the explicit preview cap. Avoid
a bicubic enlargement fallback during drags. Scale policies must preserve
aspect ratio and must not apply the preview cap twice.

`export_resolution` is true only when the retained float output dimensions
equal the resolved uncapped working dimensions. The 8192 px texture limit
does not prevent saving a larger retained float render. An interactive or
12 MP-limited render must never satisfy a larger Fast export through reuse.

In Longest side mode, Swift/Windows `EditState.rendered` retains only the
crop's fractional extent and clears its origin, angle, aspect, and custom
ratio. In Original/legacy mode, crop remains absent from render identity.
Both startup-cache identities use this same mode-dependent rule. Include
resolved working dimensions in the helper's render key; do not include the
whole crop. Compute cropped-size response metadata from the current request
even on a cache hit, because edge rounding can change when a crop is panned.

The existing crop session commits `state.crop` only on Done. Preserve that:
dragging crop handles only redraws the crop UI; Done triggers one render if
the target's working size changed. Cancel retains the prior committed crop
and render; it needs no new render-cancellation feature. Panning a crop with
the same extent should reuse the simulation. Undo/redo, post-only reuse,
source-generation guards, and startup restore must use the new identity.

Compute a draft crop's projected working size without allocation for the crop
readout. Show an over-limit error before Done and reject that invalid commit
without losing the draft or last good image; the user can reduce the target
or adjust the crop. Do not render while the draft crop is changing.

Exports remain cancellable by terminating the CLI. In-flight previews are
currently serial and are not cancellable; edits coalesce into a later render.
Do not add a new preview cancellation system or claim one already exists.

## Exposure and physical scale

Keep full-frame metering; crop position must not change exposure. Current
Rust meters after resize, while the Python reference meters the source before
resize. Do not claim those numerical exposures already match.

Measure auto-exposure EV at Original, 2×, and 3× and with differing crop
extents at a fixed target. Allow at most 0.01 EV drift on representative
fixtures. If the check fails, compute metering from the retained source and
share that compensation between native and CLI **for the new pixel-target
path**, preserving the user's manual exposure and avoiding double metering.
Keep the legacy path's metering semantics except for the documented removal
of early highlight clipping. This is an implementation decision governed by
the check, not a question that needs to go back to the user.

Physical grain/halation parameters must continue to derive pixel size from
the actual buffer passed into simulation. An uncapped export uses the full
working dimensions; a reduced preview keeps the existing approximation at
its own rendered dimensions. Test an isolated effect expressed in
micrometers; do not assume scanner sharpening or every blur control uses
physical units.

## Implementation sequence and relevant files

1. **Shared policy and resampler.** Add the resolver and float resampler in
   `crates/spektrafilm-io/src/`; expose them from `lib.rs`. Add the optional
   parameter in `spektrafilm-core/src/params.rs`. Test arithmetic, rounding,
   pixel limits, interpolation, and HDR behavior before wiring interfaces.
2. **Core/CLI/native.** Update `core/src/pipeline.rs`, `preview.rs`,
   `lut_export.rs`, `spektrafilm-cli/src/main.rs`, and
   `spektrafilm-native/src/main.rs`. Implement one-time preparation,
   crop-aware resolution, normalized params, bounded drag frames, response
   metadata, cache-hit metadata, and the actual-size export reuse predicate.
3. **macOS.** Update `Inspector.swift`, `Parameters.swift`, `EditorModel.swift`,
   `Crop.swift`, and `RenderedPreviewCache.swift`; update export messaging in
   `Engine.swift`/the save panel as needed. Preserve session decoding through
   params defaults. Keep source and working readout metadata separate from
   the currently displayed canvas dimensions.
4. **Windows.** Mirror the controls, identity, validation, reset, cache, and
   status changes in `windows/src/state.rs`, `model.rs`, `engine.rs`,
   `rendered_preview.rs`, `crop.rs`, and `ui/inspector.rs`/`ui/dialogs.rs` as
   needed. Reuse the Rust resolver for size readouts and keep driver-reset
   handling and backend reporting intact.
5. **Verification and evidence.** Run affected checks below, capture the app
   with the new controls, export 100% comparison crops, and record memory and
   timings. Update user/developer docs about Image Size, preview limitations,
   the working-pixel bound, CLI params, and the highlight-clipping correction.

## Acceptance and validation

- **Resolver:** portrait/landscape, full and fractional crops, panned crops,
  straightening, quarter turns, tiny crops, half-pixel boundaries, and random
  valid extents. Longest-side error is at most 1 px. Origin changes keep the
  working dimensions unchanged. Bad targets, mixed target/factor requests,
  invalid geometry, overflow, and over-limit requests fail before enlargement.
- **HDR/interpolation:** constant 0, midgray, 1, and 2.5; bright impulses and
  gradients above 1; cubic overshoot. Confirm values above 1 survive and
  negative undershoot is floored. Compare the old/new resamplers exactly
  where the upper clamp is inactive; isolate and explain changed regions.
- **Placement:** integration-level early enlargement matches explicitly
  preparing the source then processing at factor 1 on the same backend.
  Include a negative control: processing first then enlarging must differ
  with grain/halation enabled. Verify physical halation/blur radius in pixels
  grows approximately 2× for a 2× working scale using an isolated fixture.
- **Sizing:** the 6000 × 4000 / half-extent / 6000-target example resolves to
  12000 × 8000 working pixels and a 6000 ± 1 px photograph. Repeat with
  portrait, rotation, straightening, borders, and an export reduction.
- **Preview/export:** a full-resolution native Save matches Fast CLI export
  pixels, and file bytes where the encoding is deterministic. Use the
  existing deterministic GPU/CPU comparison policy with grain/glare disabled
  for pixelwise Reference comparisons; assess grain statistically and in
  exported 100% crops. Do not demand identical CPU/GPU random grain patterns.
- **Drag/cap/proxy:** test an enlarged image zoomed beyond fit so the old drag
  path would exceed source size. No assertion or helper reset; drag buffers
  stay within the source. Repeat with the 12 MP cap. Neither that cap nor
  the 8192 px proxy reduces export dimensions. Check truthful reduction labels.
- **Lifecycle:** old sessions/params with factor 0.5 still load; defaults gain
  a null target. Test target persistence, remembered target values, Original,
  Reset Adjustments, crop Done/Cancel, target-preserving Undo/Redo, source
  switches, startup-cache restore, and panning without a new simulation.
  Check crop metadata on reused helper renders. Post-only edits still reuse.
- **LUT:** changing Image Size has no effect on `.cube` size or contents.
- **Exposure:** apply the 0.01 EV check and conditional correction above.
- **Capacity/performance:** measure 12 MP → 24, 48, approximately 96, and
  approximately 108 MP working renders on macOS and Windows, Fast and
  Reference. Record peak memory, timing, retained cache footprint, cancellation,
  CPU fallback, and any Windows driver reset. A 128 MP bound does not imply
  every admitted render is comfortable on an 8 GB Mac. Report actual limits.
- **Visual evidence:** show the new controls running in the app and a 3×
  before/after-grain comparison using exported crops at 100%. Include RAW
  highlights and representative colour/B&W stocks. Use exported crops for
  quality review when the app displays a reduced proxy.

Start with affected Rust package tests and Swift tests, then the required
platform checks for the final change:

```sh
cargo test --locked -p spektrafilm-io -p spektrafilm-core -p spektrafilm-native -p spektrafilm-cli
cargo test --locked -p spektrafilm-cli -p spektrafilm-core --features spektrafilm-cli/precision-f64
swift test --package-path macos
cargo check --locked -p spektrafilm-gui
```

Extend `gpu_capacity_fallback.rs`, LUT tests, helper tests, and macOS
`CropTests`, `SessionTests`, `RenderedPreviewCacheTests`, `EngineTests` /
`LifecycleTests` as relevant; use meaningful integration assertions rather
than mirroring view code. Consult `docs/windows-ui-validation.md` and
`scripts/verify_macos_package.py` for platform validation/packaging. Run actual
Windows validation on Windows; record unavailable checks honestly. Once the
affected checks pass, do not repeat unrelated suites without a new reason.

Completion requires the implementation, passing relevant checks, actual app
and export evidence, and a concise report of limitations. This planning
handoff has not run builds or tests and is not implementation evidence.

## Review disposition

The requested Claude Opus 5.5 high-effort review completed successfully.
Its verdict on the draft was **proceed with changes**. Its unedited findings,
model provenance, and inspection limits are in [the review](upscale-opus-review.md).

| Findings | Final disposition |
|---|---|
| H1, D7 | Shared parallel float resampler; remove upper clipping on all early paths; explicitly document legacy changes, including bounded-source cubic overshoot |
| H2, M3, D1–D5, D9–D11 | Resolver in IO; target in params; reject mixed mechanisms; origin-independent extent formula; mode-aware client identity and resolved helper dimensions; clear target/factor after preparation |
| H3, D6 | Explicit checked host-side bound before enlargement; initial 128 MP to include 12 MP at 3×; preserve Windows CPU fallback; no macOS RAM-guarantee claim |
| H4, D12 | Clamp drag frames within source dimensions and keep area downsampling; enlarge settled frames |
| M1, M4 | Separate working/cropped/render/proxy metadata, truthful reduction labels, and exported 100% inspection; update metadata even on reused renders |
| M2 | Quantified exposure check with a specified corrective path for pixel-target mode; no claim of current Python metering parity |
| M5 | Export cancellation only; existing preview coalescing; check Windows resets |
| M6 | Photograph dimensions and existing border/fit summary; no duplicated exact border geometry |
| L1, L2 | Distinct `photo_long_side_px` name; preserve export Fit and leave unused Python crop parameters alone |
| L3, D14 | New egui interface deferred; maintain compilation and existing factor behavior |
| L4 | Original accepts and ignores the target; no enlargement |
| L5 | End-to-end placement assertion plus a late-enlargement negative control and physical-scale check |
| D8, D13 | Preserve the target on Reset Adjustments; retain Lab placement and remove the superseded slider |

The review's suggested equality for every [0, 1] input was narrowed: cubic
overshoot can activate the old clamp even for bounded inputs. Its nominal
100 MP proposal was made concrete at 128 MP with an explicit measurement
requirement. Review recommendations are planning input, not measured parity
or memory results and not certification of the revised plan.
