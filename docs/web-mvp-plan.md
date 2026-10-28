# Web MVP plan

The browser version of Negs, on branch `wasm-investigation` (worktree
`/home/noah/Work/Negs-wasm`). Goal: an MVP that shows what spektrafilm can
do and is pleasant on desktop browsers, iPad and phones. WebGPU is the path
that matters; the CPU engine stays a correct fallback and gets no
optimisation work.

Read first: `docs/web-wasm.md` (what exists, measurements, limits),
`docs/web-wasm-handoff.md` (code map, build and test, gotchas) and
`docs/web-feature-inventory.md` (the feature inventory; the owner's "Do
now", "second stage" and "research" lists at its top are this plan's scope,
the rest is later or never).

## Ground rules

- Work in the order below. Each piece ends at a checkpoint: everything in
  its "Done when" holds, `docs/web-wasm.md` and `docs/web-wasm-handoff.md`
  are updated, and the piece is committed on `wasm-investigation`. **Don't
  push, and don't merge to `main`**, until the owner says so.
- At a 🛑 checkpoint, stop and report to the owner (what changed,
  measurements, screenshots at phone, iPad and desktop sizes) and wait for
  their go-ahead before the next piece.
- Native output must stay byte-identical: render the CLI before and after
  any engine change, with `SPEKTRAFILM_BACKEND=wgpu` and `cpu` (the CLI
  command is under "Build, run, test" in `docs/web-wasm-handoff.md`), and run
  `cargo check --workspace --all-targets` plus the core, gpu, math, model
  and io tests.
- Keep frames presented: a change must not make an sRGB preview fall back to
  reading the scan back (`ChainOutput::Scan`) or to the CPU engine. The
  automation's `presenting` and the stage list show it. Benchmark the test
  card at 200, 1200 and 2048 px and the Sony ARW before and after each
  piece; regressions need a reason.
- Desktop parity: control names, ranges, steps, defaults and behaviour come
  from the desktop apps (macOS `macos/Sources/NegsApp/Inspector.swift` and
  `Parameters.swift`; Linux `linux/src/fixtures.rs`; Windows
  `windows/src/ui/inspector.rs`), not invented. Where they differ, follow
  macOS and note it.

## Testing beyond this machine

- Phone and iPad layouts: Chrome's device emulation over the DevTools
  protocol (`Emulation.setDeviceMetricsOverride` with `mobile: true` and
  touch, for example 390×844 at DPR 3 and 1180×820 at DPR 2), with
  screenshots from `Page.captureScreenshot`. The windowed-Chrome recipe is
  in the handoff.
- A real iPad or iPhone needs a secure context for WebGPU and
  `SharedArrayBuffer`: `http://<LAN IP>` is not one. Serve over HTTPS (a
  local CA such as mkcert, trusted on the device, or a tunnel), keeping the
  COOP/COEP headers. Set this up early in piece 2 and ask the owner to open
  it. The first device questions: does Safari 26 offer WebGPU in the worker
  (the GPU engine runs there; otherwise it falls back to the CPU), does
  presenting on the transferred canvas work, frame times, and when iPadOS
  ends the tab for memory. If WebGPU isn't in workers, the fix is running
  the GPU engine on the main thread (the single-threaded build; a GPU frame
  needs a few ms of CPU), and that comes before more UI work.

## Stage 1: Do now

### 1. Complete parameter state and the engine catalog

Today the page sends only values moved off hand-written JS defaults, grain
as multipliers of the catalog preset, and reads the static
`linux/catalog.json`. Everything after this needs the desktop model: a
complete `RuntimeParams`, the engine's defaults, `nullable_paths`, and the
per-film preset values (grain lanes, DIR gammas, halation baseline) that a
film choice resets.

- Catalog: the desktop helper builds it (`catalog` in
  `crates/spektrafilm-native/src/main.rs`: profiles, `defaults`,
  `nullable_paths`, per-stock grain/DIR/halation presets). The browser can't
  list a directory, so either generate `web/catalog.json` with the native
  helper at build time, or move the per-profile part into
  `spektrafilm-core` and export it from wasm over a known profile list
  (fetching every profile up front costs downloads). Pick one, say why.
  The helper's response must not change.
- Params in the page: one state object (a full `RuntimeParams` as JSON),
  defaults from the catalog, nulls where the desktop leaves preset-filled
  values unset (`Parameters.swift` `JSONValue.restoring`,
  `Stock.presetPaths`). Choosing a film resets its preset-filled values, as
  desktop's `EditorModel.chooseFilm`/`pairFilm` do; automatic paper pairing
  and slide handling as there.
- The "Scan film directly" toggle for any film.
- The existing controls keep working; grain Amount/Size become the real
  lanes in piece 2.

Done when: renders with default params are byte-identical to before
(automation, test card and ARW, GPU and CPU); a film change resets exactly
what desktop resets; the scan toggle works on a negative.

### 2. All Lab controls, desktop layout

Every control in the "Do now" list, from a single control schema
(`web/controls.js` or similar) ported from `linux/src/fixtures.rs` and the
macOS inspector: camera lens blur; lens diffusion; film curves; full grain
(B&W stocks show one lane); full halation; DIR couplers; film base; metering
method; enlarger compensation and preflash; print curves and print base;
full glare; print diffusion (`enlarger.diffusion_filter`, set only by
presets on desktop; mirror the planned UI in `docs/print-diffusion-plan.md`
or leave it for presets, and note which); scanner lens blur, unsharp and
white/black correction; group on/off switches. Per-control reset (a visible
button, not only double-click), click or tap to type a value, and Reset
Adjustments (keeps film, paper and the like, as desktop).

Measure every slider: all should be Resident (a frame, no rebuild) except
the ones in `preview_pipeline_cache_key`. List the rebuilding ones with
their cost; piece 3 deals with the enlarger.

Done when: every control changes the render as the same control does on
desktop (spot-check against the CLI with the same params JSON, GPU against
GPU within 1/255), the list of rebuilding controls is in the docs. 🛑 Owner
review on desktop.

### 3. Layout for iPad and phones

The same inspector, made for touch: sidebar on desktop and iPad landscape,
a bottom sheet or tabs on phones; slider thumbs and reset targets sized for
fingers; the canvas keeps pinch and scroll for piece 6 (`touch-action`);
no hover-only affordances. Group collapse state remembered (localStorage,
wrapped in try/catch).

Done when: screenshots at 390×844, 820×1180 and 1180×820 (emulated) and
1440×900 look right, sliders work by touch in emulation, and, if the owner
has opened it on a device, their notes are addressed. 🛑 Owner review.

### 4. Enlarger shifts without a rebuild, and Balance Print

Y/M filter shifts, preflash and print exposure compensation are in
`preview_pipeline_cache_key`, so in the browser every move rebuilds the
spectral pipeline. Measure the rebuild first. Then update a built pipeline
instead, as `Pipeline::set_print_filter_shifts` (`pub(crate)`) does for the
balance solver, extended to preflash and compensation if they allow it,
and key the pipeline cache without them. The result must equal a full
rebuild with the same params (byte-identical, or explain the difference).
Balance Print: export the helper's `balance_print`
(`spektrafilm_core::print_balance`) from wasm and add the button.

Done when: a shift slider moves at frame rate with no rebuild in the stage
list; the rebuilt-vs-updated comparison is in the docs.

### 5. Colour management and output colour space

Controls for output gamut, input gamut, CAT16 and the spectral upsampling
method. Arctic and Mallett need their 6 MB LUTs: fetch them only when
chosen, with progress.

Output colour space (sRGB, Display P3, Adobe RGB, ProPhoto) and the
encoding toggle: today any non-sRGB output reads the scan back. Extend the
GPU display pass (`display_rgba8.wgsl`, `display_present.wgsl`,
`Pipeline::display_on_gpu`) to show such a scan on the sRGB canvas: decode
its curve (Adobe and ProPhoto are pure gammas; the chain encodes only sRGB
on the GPU, `gpu_encodes_output`), convert to sRGB, clip, encode. Decide and
document what the web preview of a wide-gamut output should be: an sRGB
rendition here, a P3 canvas later (unverified in wgpu; see research), and
how it relates to desktop's Rec. 2020 display path
(`preview::display_rgba_with_post`). Export keeps encoding the chosen space
from a readback.

Done when: every output space presents (no readback) and matches a CPU
reference conversion of the read-back scan within 1/255; export still
produces the chosen space.

### 6. The resident scan, zoom and true-resolution preview

The architecture piece. Desktop applies display-only changes without
re-rendering; the web should too, and zoom needs it.

- Keep the last chain output (the scan) resident on the GPU, and let a
  display-only call re-run just the display pass with new parameters:
  viewport rectangle and scale now, Post and Show Original later. This is a
  new engine entry point next to `render`; the film chain doesn't run.
- The canvas becomes the view's size in device pixels; the present shader
  samples the visible part of the resident scan (area-averaged below 100%,
  nearest or bilinear above). Pan and zoom are then display passes only.
- Zoom: pinch, Ctrl/⌘-wheel, double-tap or double-click to toggle fit and
  100%, Actual Size and Fit buttons; desktop's range and behaviour
  (`macos/Sources/NegsApp/PreviewCanvas.swift`).
- True resolution: zooming past what the current render holds renders the
  full photograph (grain and sharpening form at render resolution, so a
  viewport-sized preview isn't what export gives). Cap it by memory and
  `maxTextureDimension2D`/buffer limits, lower on phones, and show desktop's
  "shown at N%" notice whenever the preview isn't at export resolution.
- Memory: a resident 24 MP scan is 288 MB of GPU memory; budget it with the
  image pool (`MAX_POOLED_BYTES`) and drop it when a new photo opens.

Done when: zoom and pan run at display speed (measure a pan frame), 100%
shows the export's pixels (compare a crop with an export render), the
notice is right, and memory stays within the caps on an emulated phone.
🛑 Owner review.

### 7. Placeholder while developing

Show something at once instead of a blank view for ~1.8 s (24 MP ARW):
for JPEG/PNG/TIFF the browser's decode (`createImageBitmap`) on the main
thread; for RAW the embedded preview (desktop: `linux/src/photo.rs`
`decode`, macOS `EditorModel.swift`), with orientation applied once (the
test in `photo.rs` covers it). A "Developing" badge as on desktop.

Done when: opening the ARW shows the placeholder in well under a second and
the developed frame replaces it in place.

### Stage 1 checkpoint 🛑

Full benchmark table (test card, ARW, a slide, 24 MP export; Chrome and, if
possible, Firefox and a device), fidelity against native, docs updated.
Owner review of the whole MVP before stage 2.

## Stage 2

### 8. Show Original

Hold (desktop: a button or `\`) or a toggle on touch: the source without
film, through the display pass of piece 6 from the resident input (a new
small output; the input is already uploaded by key). Desktop shows rawler's
develop without film on Linux and Windows (the helper's `original`).

### 9. RAW white balance

As Shot, Daylight, Tungsten 2850 K, Custom (1667–25000 K, tint 0.01–4), as
desktop (`docs/raw-white-balance.md`, `spektrafilm-io/src/white_balance.rs`).
First find out whether Daylight-decoded data plus a linear adjustment
equals each mode (then it folds into the front-pass matrix and moves at
frame rate) or whether it needs a re-decode (about 0.8 s at 24 MP; then
debounce, keep the previous frame, and watch memory). Byte-compare with the
desktop decode.

### 10. Download and caching

A service worker that caches the wasm, JS and data (the LUT is 6 MB, 4 MB
brotli), with versioned cache names; lazy fetches for the upsampling LUTs
and anything else optional; it can also set COOP/COEP for hosts that can't
(GitHub Pages; `coi-serviceworker`-style). Measure first-visit and
repeat-visit start-up.

### 11. About and licences

An About panel: Negs and spektrafilm credits, the profile attributions (in
`linux/catalog.json`), and the licences of what the page ships (rawler
LGPL-2.1 and other crates, `film.png` MIT and the 35mm material CC BY-SA
when borders arrive). Check what the desktop About screens list.

## Research

### R1. HEIC (iPhone photos)

Report only, no implementation. What does Safari's file input hand over
for an HEIC photo (converted to JPEG, and when not: `accept` values,
"Most Compatible" camera setting, Files vs Photos)? Can the browser decode
it (Safari `createImageBitmap`/`<img>` of HEIC; at what bit depth and
colour space, P3)? Would a wasm decoder (libheif/libde265) be feasible,
and what are its size, speed and licensing/patent issues? Is ProRAW DNG
read by rawler? Recommend one approach.

## Later (not scheduled)

Everything else in `docs/web-feature-inventory.md`: crop, rotate,
straighten, borders, Image Size (note: it forces the CPU engine on the web
until the wasm crate resolves it as the helper does), Post, histogram,
export formats and options, presets, undo, sessions, and so on.
