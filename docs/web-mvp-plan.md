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
the rest is later or never). `docs/web-next-handoff.md` says where the work
stands and what to do first.

## Order of work

The pieces are numbered as first planned; work in this order:

1. Pieces 1–4: done.
2. Piece 6 (resident scan, zoom): done ahead of piece 5 at the owner's
   request, and approved.
3. **The owner's features, next and in this order:** 7 Placeholder Image
   (done), 8 Before/After toggle (done), 9 Export LUT, 10 Export Preset, 11 Borders and
   Fit for Instagram. They come before everything else left in the plan.
   On 2026-10-29 the owner asked for the whole Post tab next: its Tone and
   Color sections (piece 15, below) and its Border section (piece 11), so
   those two come first, then 9 and 10.
4. Piece 5 (colour management), then the Stage 1 checkpoint 🛑.
5. Stage 2 (pieces 12–14), and the HEIC research.

## Ground rules

- Work in the order above. Each piece ends at a checkpoint: everything in
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

### 6. The resident scan, zoom and true-resolution preview

Done (`docs/web-wasm.md` item 14), waiting for the owner's review.

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

### 7. Placeholder Image (owner's list)

Show something at once instead of a blank view for ~1.8 s (24 MP ARW):
for JPEG/PNG/TIFF the browser's decode (`createImageBitmap`) on the main
thread; for RAW the embedded preview (desktop: `linux/src/photo.rs`
`decode`, macOS `EditorModel.swift`), with orientation applied once (the
test in `photo.rs` covers it). A "Developing" badge as on desktop.

Done when: opening the ARW shows the placeholder in well under a second and
the developed frame replaces it in place.

The owner added: the placeholder image is `DSC_6377.NEF`, then: keep the
empty screen, offer that photograph there as a sample, as a lossy DNG, so
the first load stays fast. Done: `docs/web-wasm.md` item 15.

### 8. Before/After toggle (owner's list)

Desktop calls it Show Original: macOS holds it on a button in the canvas
(`macos/Sources/NegsApp/NegsApp.swift`, the view around line 288;
`EditorModel.swift` `canShowOriginal`, `loadOriginal`, which starts the
decode when the pointer arrives) and on `\`; Linux has the `original`
action (`linux/src/actions.rs`). It shows the source without film: the
helper's `original` (rawler's develop without film for RAW).

- On the web: hold the button or `\` on desktop, as macOS; a tap toggles
  on touch, where holding hides the photograph under the finger. The HUD
  says "Original" while it shows.
- Through piece 6's display pass: the original becomes a shown texture
  next to the developed one (`Canvas.shown` keeps two), drawn with the
  same view, so switching is a `show()` and needs no film render. Make it
  from the resident input (already uploaded by key) or from piece 7's
  decode, whichever matches desktop's original; say which.
- Zoom and pan keep working, and stay where they were across the switch.

Done when: switching either way is one display frame (measure it), the
stage list shows no render, the original matches desktop's (the helper's
`original` for the card and the ARW, within 1/255 or the difference
explained), and it works by keyboard, mouse and touch.

### 9. Export LUT (owner's list)

Desktop: File ▸ Export LUT… (⇧⌘L, `macos/Sources/NegsApp/MainMenu.swift`;
`EditorModel.swift` `exportLUTPanel`, `exportLUT`), the helper's
`export_lut` (`crates/spektrafilm-native/src/main.rs`), and the engine's
`spektrafilm_core::lut_export::cube`: a 33×33×33 `.cube`, sRGB in and out,
one spectral pipeline over the grid on the CPU. It needs no photograph
(`canExportLUT`).

- Export `cube` from wasm and run it in the worker. It takes a data
  directory and loads the profiles by name; give it the profiles the wasm
  engine already holds (a variant over loaded profiles that `cube` itself
  calls), not a copy of the function. The native helper's output must not
  change.
- Download it as `<film>_<paper>.cube` (`<film>_direct_scan.cube` when
  scanning film directly), and show desktop's explanation of what the LUT
  leaves out (the panel's message in `exportLUTPanel`).
- It must not disturb the resident chain or a render in flight.

Done when: the web's `.cube` is byte-identical to the helper's for the same
film, paper and params (a colour negative, a slide, direct scan and a B&W
stock), its time is measured, and the button is disabled while it runs.

### 10. Export Preset (owner's list)

Desktop's design is `docs/presets-design.md`: the file is Python
spektrafilm's GUI preset JSON. The engine does the translation, once, in
`crates/spektrafilm-core/src/presets.rs` (`export`, `import`, `apply`;
the helper's `preset_export` and `preset_import`). macOS: Import Preset…
(⇧⌘O) and the presets panel's Export… (`docs/presets-design.md` "Menus").

- Export Preset: the current film, paper and params through `export`, as a
  `.json` download. Like `cube`, `export` and `import` take a data
  directory; give them the engine's loaded profiles the same way.
- Import Preset in the same piece, since an exported preset is only useful
  if it comes back: a file input and dropping a `.json` on the page,
  through `import` and `apply` (the excluded settings, `presets::KEPT`,
  stay the photograph's own). Film and paper are set without the film
  change's resets, as desktop.
- A preset library (list, rename, delete, stored in IndexedDB or OPFS) is
  not in this piece; ask the owner whether they want it.

Done when: the web's export is the helper's `preset_export` text for the
same state; importing it renders identically (compare renders, not raw
params, as the design doc says); presets exported by the desktop apps
import; a malformed file shows the engine's error.

### 15. Post: Tone and Color (owner's request, 2026-10-29)

Done (`docs/web-wasm.md` item 17). Desktop's Post tab (macOS
`Inspector.swift` `post`): Brightness, Contrast, Whites and Blacks under
Tone, with its footer, and Saturation under Color, −100 to 100. A Lab |
Post switch on the sidebar, a Post tab on the phone's sheet. Post runs in
the GPU display pass, and a Post-only change makes the shown frame again
from the scan the GPU still holds, without the film chain.

### 11. Borders and Fit for Instagram (owner's list)

The Post tab's Border section.

Desktop: solid, keyline, 120 film and 35mm strip, with the mat width (3%),
the black around film, the keyline width (0.4%), the mat and line colours,
"also on export", and Fit for Instagram, which extends the bordered canvas
to a 3:4 portrait (the mat's colour around a mat, white around film).
Sources: macOS `macos/Sources/NegsApp/Border.swift` (`BorderSettings`,
`canvas`, `instagramCanvas`, the background colours near line 275) and its
inspector controls; the engine's `crates/spektrafilm-io/src/border.rs`
(`add_border`, `instagram_canvas`, `extend_for_instagram`) and `film35.rs`
(the 35mm strip and its edge print, which depends on the film); textures
in `data/borders/` (`film.png` MIT, the 35mm material CC BY-SA; see its
README); `docs/linux-border-performance.md`. Borders are composited after
the film, so a border change never re-renders.

- Preview: in piece 6's display pass, so a border change is a `show()`.
  The view places the bordered canvas (the photograph inside it), Fit fits
  the whole canvas (with Instagram, the whole 3:4), and 100% is still one
  photograph pixel per device pixel. Solid, keyline and the Instagram
  extension are colours and rectangles in `fs_view`; the 120 and 35mm
  borders need their textures, composited at the shown size.
- Textures: fetch them only when a film style is chosen (the inventory
  puts them at 17 MB in all), with progress as for piece 5's LUTs.
- Export: `add_border` (and the Instagram extension) on the export's bytes
  in the worker, on the CPU, as the CLI does; measure it at 24 MP against
  the memory caps.
- Styles that don't fit the photograph (35mm only fits 3:2 and 3:4) are
  disabled, as desktop. The licences of the textures go in the About
  panel (piece 14, or the credits page if that comes first).

Done when: exports are byte-identical to the CLI's (`--border <style>`,
`--border-width`, `--keyline-width`, `--border-color`, `--keyline-color`,
`--instagram`) for every style with and without Instagram, on the card and
the ARW; the preview at 100% matches the export's pixels (as
`web/checks/zoom.mjs` compares the canvas); a border change shows with no
film render; the textures download only when used. 🛑 Owner review of
pieces 7–11.

### 5. Colour management and output colour space

After piece 11 (see "Order of work"); numbered as first planned.

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

### Stage 1 checkpoint 🛑

Full benchmark table (test card, ARW, a slide, 24 MP export; Chrome and, if
possible, Firefox and a device), fidelity against native, docs updated.
Owner review of the whole MVP before stage 2.

## Stage 2

### 12. RAW white balance

As Shot, Daylight, Tungsten 2850 K, Custom (1667–25000 K, tint 0.01–4), as
desktop (`docs/raw-white-balance.md`, `spektrafilm-io/src/white_balance.rs`).
First find out whether Daylight-decoded data plus a linear adjustment
equals each mode (then it folds into the front-pass matrix and moves at
frame rate) or whether it needs a re-decode (about 0.8 s at 24 MP; then
debounce, keep the previous frame, and watch memory). Byte-compare with the
desktop decode.

### 13. Download and caching

A service worker that caches the wasm, JS and data (the LUT is 6 MB, 4 MB
brotli), with versioned cache names; lazy fetches for the upsampling LUTs
and anything else optional; it can also set COOP/COEP for hosts that can't
(GitHub Pages; `coi-serviceworker`-style). Measure first-visit and
repeat-visit start-up.

### 14. About and licences

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
straighten, Image Size (note: it forces the CPU engine on the web
until the wasm crate resolves it as the helper does), histogram,
export formats and options, a preset library, undo, sessions, and so on.
