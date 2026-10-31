# Task: RAW white balance in the web editor

Assignment: a separate **Sol 6.1** agent with reasoning **xhigh**.
Status: **approved scope; ready for implementation**. This is step 1 of
[web-presets-design.md](web-presets-design.md), the full approved implementation
handoff. Noah requested UI support as well as processing support. Preparing this
document did not launch an agent or implement the feature.

## Goal

Add desktop-style input RAW white balance to the browser editor so a later
preset import can actually render its saved WB settings. A state field that is
serialized but ignored by RAW development is not sufficient.

Scope is WB only. Do not implement presets, their menu/dialogs, links, a preset
library, or new undo/revert behavior. The additional development-time, gamut and
spectral-upsample controls are approved as step 2 of the full handoff; they belong
to that following phase, not this focused WB task.

## Read first

- [raw-white-balance.md](raw-white-balance.md): established modes, semantics,
  validation, reprocessing, rollback and tests. Follow relevant decoder links
  when needed, but the browser uses rawler, not Apple's CIRAWFilter.
- `crates/spektrafilm-io/src/white_balance.rs`: shared types and validation.
- `crates/spektrafilm-io/src/lib.rs`: RAW bytes loaders taking `RawWhiteBalance`.
- `crates/spektrafilm-io/src/bounded.rs`: bounded decoder also already takes WB.
- `crates/spektrafilm-wasm/src/lib.rs`: `Engine::load`, `decode`, `decodeBounded`,
  `decode_photo`, `decode_photo_mode`, `keep` and `forget`.
- `web/app.js`: parameter bindings/synchronization, render scheduling,
  `loadPhoto`, `decodeKept`, `open`, `reloadPhoto`, Reset, original and export.
- `web/engine.js`, `web/worker.js`, `web/decoder.js`: both load routes and workers.
- `web/controls.js`, `web/index.html`, and the Camera section in
  `macos/Sources/NegsApp/Inspector.swift`: existing UI and desktop placement.
- `web/sample/README.md`: lazy sample loading and memory/offline requirements.
- Existing checks in `web/checks/`, and `web/build.sh`.

## Current gap

RuntimeParams already contains `io.raw_white_balance` with mode, temperature and
tint. The shared I/O library supports all four modes and validates custom values.
However, `decode_photo_mode` currently passes `Default::default()` to both the
normal RAW loader and bounded RAW loader. Neither the main engine load nor the
throwaway decoder accepts the edit's WB. Rendering afterward cannot correct
those already-decoded source pixels merely by changing RuntimeParams.

The editor's photo state currently does not retain the decoder's `InputColour`
metadata. The throwaway decoder already transfers serialized colour metadata to
`loadDecoded`; expose appropriate source metadata to the editor in both routes
so RAW-only UI can use actual decoder classification, not filename extensions.
`InputColour` has `source` and `scene_referred` fields; use the actual shared
source classification for RAW versus developed raster inputs.

## UI

Use a **RAW White Balance** disclosure in Camera, after the basic camera controls
and before Lens Diffusion, following desktop. On mobile it belongs in the existing
Camera tab, not a new top-level tab.

- Modes: As Shot, Daylight, Tungsten (2850 K), Custom. Serialized values stay
  `as_shot`, `daylight`, `tungsten`, `custom`.
- Default: As Shot; retain custom temperature 5500 K and tint 1.0 when inactive.
- Custom fields only while Custom is selected:
  - Temperature (K): 1667–25000, step 1.
  - Tint: 0.01–4, step 0.01. Tint 1 is neutral; this is the established shared
    ACES-green multiplier, not a new photographic tint formula.
- Use existing slider, type-in and per-control reset conventions and responsive
  touch targets. No iOS focus zoom from undersized numeric inputs.
- Hide the RAW group for decoded JPEG/PNG/TIFF inputs, as desktop does. Retain WB
  selection in state when opening a raster so the next RAW still uses it; do not
  silently set the saved mode to As Shot just because the current photo is RGB.
- Preserve monochrome RAW no-tint semantics from the shared loader.

## Processing and integration requirements

1. Thread validated WB through both main-engine and throwaway-worker decode
   routes, and through the opt-in bounded route. Use shared `RawWhiteBalance`
   and shared I/O processing, not a new JS/GPU temperature approximation.
2. Preserve old/default As Shot behavior and account for existing WASM callers
   such as `web/node-test.mjs`, browser correctness tests and catalog generation.
   Rebuild/check both normal and threaded bindings rather than leaving mixed API
   versions in use. Keep any generated asset changes consistent with repo policy.
3. Opening a RAW uses the current WB selection. Opening another photo retains
   the selection, while normal new-photo crop/rotation reset behavior remains.
4. A WB edit re-develops from the original compressed photo bytes, never from the
   previous WB's decoded RGB. Debounce expensive custom slider edits; avoid a
   full RAW decode for every pointer event. Inactive custom-field changes do not
   require RAW reprocessing until Custom becomes active.
5. Re-development is not a new-photo open. Preserve film/paper, all other Lab and
   Post settings, border, crop, rotation, zoom/pan and sample selection. Do not
   call a path that clears geometry or cancels the current sample unnecessarily.
6. Coordinate pending decode, render and export. Track source identity and
   effective WB/generation. Stale completions must not install pixels for an
   older source or WB into the live engine, even with overlapping worker calls.
   Engine changes/restarts and Reset during a pending decode must stay coherent.
7. A central settings-change path must notice WB differences, not only the WB
   controls' DOM handlers. Reset and future programmatic preset application must
   trigger the same correct reprocessing. A narrow documented integration hook
   is fine; do not implement presets in this task.
8. **Reset Adjustments:** keep its current desktop-style behavior: retain film,
   paper, crop, rotation, border and image size, reset parameter adjustments
   (including Post), and return WB to As Shot. Actually re-develop a loaded RAW
   if necessary. No new pre-import snapshot/Undo/Revert feature.
9. Invalidate derived source-dependent caches, including Show Original, and
   re-render from the new decoded source. Original must reflect the current RAW
   input WB, not an older source image.
10. Export cannot render old decoded pixels with newly selected WB params. It
    must wait for the matching source/settings pair or remain unavailable until
    ready. Preserve existing export snapshot/locking policy for coherent output.
11. On decode failure, report the reason and restore a coherent last-successful
    source/WB pair. Do not claim the failed WB is applied while showing/exporting
    old pixels. Restore only the failed WB portion, not unrelated newer edits.
12. Respect mobile memory design: decode in the short-lived worker when capped;
    do not retain multiple full decoded photos or long-lived decoder pools just
    to support rollback. Re-develop from existing original bytes when needed.
    Clean up superseded workers, thread pools, transfer buffers and subscriptions.
13. Developed raster pixels are unchanged by RAW WB. Do not reset scanner White
    correction or reinterpret it as input WB; it is a separate setting.
14. No sample download at startup, no extra prefetching, no change to Sample 1's
    offline policy. If adding a runtime module, include it in shell/static/offline
    manifests as appropriate.

## Acceptance tests

Add focused automated coverage for:

- All four modes and custom serialization/validation using shared semantics;
  invalid active custom values fail clearly, default As Shot stays unchanged.
- RAW pixels actually change for Daylight/Tungsten/Custom where expected; switching
  back to As Shot returns to baseline. Custom 6504 K / tint 1 equals Daylight.
  Compare to shared rawler reference results, not Apple's macOS decoder pixels.
- Both main-engine and throwaway-worker paths; opt-in bounded decoding continues
  to accept the selected WB. CPU and WebGPU consume the same developed source.
- True RAW classification (including samples); raster inputs remain unchanged;
  shared monochrome no-tint behavior is retained.
- Rapid WB edits, Reset during re-decode, opening a new file during re-decode,
  sample changes and engine restarts cannot install a stale source/WB pair.
- Failure rollback preserves unrelated adjustments and leaves valid preview and
  export state. Retrying can succeed.
- Reset from non-default WB re-develops As Shot while keeping film/paper and
  geometry. Programmatic parameter updates use the same processing hook.
- Crop/rotation/view/border/Post survive WB changes; Show Original refreshes.
- Export immediately after a WB edit waits for the correct source or is safely
  disabled, and preview/export settings agree.
- UI mode selection, custom visibility, per-control reset, mobile layout and
  keyboard/type-in behavior; no unexpected sample requests on page load.

Useful existing commands (new checks should be documented with their invocation):

```sh
cargo test -p spektrafilm-io
bash web/build.sh --threads
node --test web/checks/*.test.mjs
node web/checks/samples.mjs
```

Use the existing CDP/Safari harnesses for browser/physical-device checks where
available. Do not report device testing as completed if no device is available.
A native `cargo check` alone does not type-check code hidden behind wasm32 cfg.

## Delivery

- Focused WB implementation and tests, with a concise summary of changed paths.
- Exact commands run and their results, including any unavailable device tests.
- Document the programmatic apply/reprocess contract for the preset implementer.
- List remaining limitations and any integration notes for the approved preset
  implementation that follows this task.
- Preserve unrelated user changes (the checkout is already dirty). Do not expand
  this focused task into presets or the step-2 controls. Do not commit, push or
  deploy without approval.
