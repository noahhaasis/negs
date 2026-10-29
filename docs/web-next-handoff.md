# Web MVP: handoff for the next agent

For the agent picking up the browser version of Negs. Read, in this order:
`docs/web-mvp-plan.md` (the pieces, their order and their "Done when"),
`docs/web-wasm.md` (what exists, measurements, limits; items 14 to 17
are the last four pieces), `docs/web-wasm-handoff.md` (code map, build,
serve, tools, gotchas) and `docs/web-feature-inventory.md`. This file
says where things stand and how to build piece 11, **Borders**, which is
next. Short notes for pieces 9 and 10 follow it; notes for piece 5, which
comes later, are at the end.

## Where things stand

Worktree `/home/noah/Work/Negs-wasm`, branch `wasm-investigation`. Commit
each finished piece there; **don't push and don't merge to `main`** until
the owner says so (this overrides AGENTS.md's commit-to-main rule for this
branch). Commits end with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

| Piece | Commit | State |
|---|---|---|
| 1. Full params and the engine catalog | `9807e6c` | done |
| 2. All Lab controls, desktop layout | `feda605` | done, 🛑 reviewed |
| HTTPS for device testing | `d350950` | done |
| 3. iPad and phone layout | `842da9c` | done |
| 4. Enlarger shifts without a rebuild, Balance Print | `adcee97` | done (`docs/web-wasm.md` item 12) |
| 6. Resident scan, zoom, true-resolution preview | `fc6322d` | done (item 14); approved |
| 7. Placeholder Image, sample photo | `9d4f4b4` | done (item 15) |
| 8. Before/After toggle (Show Original) | `3aa412c` | done (item 16) |
| 15. Post: Tone and Color | `7ee6e80` | done (item 17) |
| **11. Borders and Fit for Instagram** | | **next**: the Post tab's Border section; below |
| 9. Export LUT | | after 11 |
| 10. Export Preset | | after 9 |
| 🛑 | | owner review of 7–11 and 15, after 10 |
| 5. Colour management and output colour space | | after the review; notes at the end |
| Stage 1 checkpoint | | 🛑 |

The owner asked for pieces 7–11 (their features) ahead of everything else
left, so piece 5 moved after them. On 2026-10-29 they asked for the whole
Post tab next: piece 15 (Tone and Color) is done, and its Border section
is piece 11. Then 9 and 10, then one 🛑 review of them all. Carry on
through 11, 9 and 10 unless the owner says otherwise; report at the end of
each piece all the same.

What piece 7 settled with the owner, which later pieces shouldn't undo:

- **The page starts on the empty screen** and downloads nothing big. The
  owner wants the first load fast. "Try a sample photo" there opens
  `web/sample/DSC_6377.dng`, a 3.4 MB lossy DNG that
  `web/sample/make-dng.py` made from the owner's NEF (31.6 MB, not in the
  repository, at `~/Downloads/raws/DSC_6377.NEF`). The NEF must not come
  back into the repository.
- A photograph shows at once from its placeholder (`web/placeholder.js`)
  with a "Developing" badge (`setBadge`), until the first frame.

Open items that aren't yours to settle:

- The owner's Cloudflare packager (`web/package-static.mjs`, uncommitted)
  doesn't copy `web/sample/` yet; the owner knows.
- Nobody has opened the page on a real iPad or iPhone (ufw blocks ports
  8137 and 8443 from the LAN; the owner's call).

## The owner's uncommitted work: don't commit it

`web/credits.html`, `web/package-static.mjs`, `web/static/`,
`docs/cloudflare.md`, and two lines in `web/index.html` (the empty
screen's "Your photo stays in your browser." and the sidebar's "Credits &
source" link). Leave them as they are. When you commit `web/index.html`,
stage your version without those two lines, as piece 7 did:

```sh
grep -v -F -e '      <p>Your photo stays in your browser.</p>' \
  -e '      <div class="hint"><a href="credits.html" target="_blank" rel="noopener" style="color: inherit">Credits &amp; source</a></div>' \
  web/index.html > <scratch>/index.html
git update-index --cacheinfo 100644,$(git hash-object -w <scratch>/index.html),web/index.html
```

Then `git diff web/index.html` must show exactly those two lines left
unstaged.

## What piece 8 settled

- **The original is the engine's decoded source** (the helper's
  `original`), as the owner chose: flat next to the film, unlike macOS's
  Apple develop. Don't swap in the camera preview without asking.
- **The canvas has a third image**, `Canvas.original`, counted in the
  shown images' 256 MB budget; a `View` with `original` draws it. A new
  display state (piece 11's border) has to draw on the original too, or
  say why not: macOS's original keeps the border.
- **The page's original is keyed by size only.** Piece 5 must add the
  output colour space to that key, and fix the non-sRGB original's bytes
  along with the frames' (item 16, last paragraph).
- **The badge has two owners**: `setBadge` (opening: "Downloading",
  "Developing") and `originalBadge` over it. Anything new that shows a
  state in the badge picks one, or adds one to `showBadge`.
- **A long job in the engine's worker blocks its display passes**, whose
  fences resolve only when the worker yields. Piece 8's first CPU-made
  24 MP original held a switch for 330 ms; the GPU path fixed it. Keep
  that in mind for exports and LUTs (pieces 9–11): a pan during one stalls.

## What piece 15 settled

- **Post runs in the display pass.** `display_post.wgsl` is appended to
  `display_present.wgsl` and `display_rgba8.wgsl`; `ChainOutput::Rgba8`
  and `Present` carry a `DisplayPost`. Anything new in those shaders keeps
  Post's bindings (7 and 8 in `display_present`, 3 and 4 in
  `display_rgba8`) and order: clip, Post, encode, round.
- **A Post-only change is `WgpuBackend::repost`**, not a render: it makes
  the shown image again from the pooled scan (`ShownScan`, `ImagePool::scan`).
  The wasm crate decides by `look_key` (everything but `post`). A border
  is outside `state.params`, so it never reaches `look_key` or the film
  chain; keep it that way.
- **Owner's decision, 2026-10-29:** at 100% on a 24 MP photograph the pool
  lets the scan go and Post falls back to a film render (~570 ms). The
  owner chose to leave it so for now; don't keep the scan without asking.
- **The inspector has panes.** Desktop: a Lab | Post switch (`showPane`,
  `data-pane` on each panel, `.side.sheet` on the phone's layout). Phones:
  one tab per `sectionId`, so all of `POST_SECTIONS` share the Post tab
  (`tab: "Post"`); sections that share a tab keep their titles (`titled`).
  Sections take `controls`, optional `groups` and an optional `footer`.

## Piece 11: Borders and Fit for Instagram

The Post tab's third section, after Tone and Color. Read the plan's piece 11
first (`docs/web-mvp-plan.md`); this section settles its open points and
says where things are.

### What desktop does (macOS is the reference)

`macos/Sources/NegsApp/Inspector.swift` `post`, the `Section("Border")`:

- **Add border** (toggle; everything below is disabled while it's off).
- **Style picker**: four drawn swatches, Solid, Keyline, Film, 35mm
  (`BorderStylePicker`, `BorderSwatch` in `Border.swift`; their help texts
  are `BorderStyle.help`). The swatches use the current mat and line
  colours. 35mm is disabled when the photograph isn't 3:2 or 3:4
  (`FilmFormat`, within 1%), with the note "35mm needs a 3:2 or 3:4
  photograph. This one is 16:9; crop it to 3:2 or 4:3 to use it."
  (`Inspector.ratio` names the ratio). The web has no crop yet: keep the
  note's first two sentences and drop the advice to crop.
- **Per style** (`borderControls`): Solid: Width (0–15%, step 0.1) and
  Color. Keyline: Width, Line width (0.05–2%, step 0.05), Color, Line
  color. Film: Width (0–15%, step 0.1; help "Adds black around the film
  frame."). 35mm: nothing.
- **Extend to 3:4 for Instagram** (help: "Adds white above and below, or
  at the sides, so Instagram shows the whole photograph in a 3:4 portrait
  post.") and **Include in export** (help: "Adds the border to exported
  photographs and saved previews.").
- `BorderSettings` defaults: off, Solid, export on, Instagram off, width
  0.03, film width 0, line 0.004, colour `#ffffff`, line colour `#000000`.
  Widths are fractions of the photograph's long edge; the sliders show
  percent. Solid and Keyline share the width and the colour.
- **The border isn't part of the edit's params.** Reset Adjustments keeps
  it (macOS `reset`, and the button's help text). A border change never
  renders: on macOS it only lays out layers.

Linux and Windows have the same section (`linux/src/border_ui.rs`,
`windows/src/ui/inspector.rs` `BorderToggle`); follow macOS where they
differ, and note it.

### Settled for this piece (2026-10-29)

The first point was put to the owner on 2026-10-29, who didn't object; if
they say otherwise in review, the preview's textures change, not the rest.

- **The preview follows desktop's textures, not the export's.** macOS
  previews the 120 frame as a nine-patch of `film.png` and the 35mm strip
  as the pre-rendered `35mm/{full,half}/<film>.png` (100 px/mm, no grain,
  downsampled to 4096 px), while the exporter draws the strip at the
  export's resolution (`film35::Renderer`). So the 35mm preview at 100%
  won't match the export pixel for pixel, and the 120 frame may differ
  slightly in its alpha edge. The plan's "preview at 100% matches the
  export" holds exactly for **Solid, Keyline, the Instagram extension and
  where the photograph sits**; for Film and 35mm, check the geometry and
  look at screenshots.
- **The photograph's pixels can't be byte-identical to the CLI's**: the
  web's own render already differs from native within 1/255 on about
  0.01% of pixels (item 13). Read the plan's "exports byte-identical to
  the CLI's" as: (1) the export's size and the photograph's offset equal
  the CLI's for every style, with and without Instagram; (2) the border's
  pixels equal `add_border` (and `extend_for_instagram`) run natively on
  the web's own unbordered export; (3) against the CLI's bordered export,
  item 13's agreement. Say what differs where the film and 35mm textures
  blend over the photograph (see Export below).

### Where the code is

- **Geometry and export**: `crates/spektrafilm-io/src/border.rs`
  (`add_border`, `margin`, `keyline_width`, `film_geometry`,
  `instagram_canvas`, `extend_for_instagram`, `FilmFormat`, the nine-patch
  in `overlay_nine_patch`/`axis`) and `film35.rs` (`Strip::load`, `canvas`,
  `turned`, `window`, `Renderer`; the edge print per stock is a table
  there, `STOCKS`). The native order, which the web should keep: render,
  Post, (crop, rotate: not on the web yet), border, Instagram, resize
  (`spektrafilm-native` `main.rs` around line 300; CLI flags `--border`,
  `--border-width`, `--border-color`, `--keyline-width`,
  `--keyline-color`, `--instagram` in `spektrafilm-cli`).
- **Preview geometry**: `Border.swift` mirrors border.rs
  (`canvas(for:)`, `instagramCanvas`, `keyline`, `BorderStyle.film(for:)`,
  `FilmStrip.canvas/turned`, and `BorderLayers.layout`, which says what's
  drawn where: the canvas's background is the mat's colour, or black
  around film, or white under Instagram's extension around film).
- **Textures**, `data/borders/` (see its README and licences):
  `film.png` 2518 × 1550, 170 KB, MIT; `35mm/material.png` 4750 × 4375,
  498 KB, the export's dust and base density; `35mm/frames.json`, the
  window as fractions; `35mm/full/<film>.png` 3800 × 3500 (≈ 430 KB) and
  `35mm/half/<film>.png` 1900 × 3500 (≈ 280 KB), the preview strips,
  `generic.png` for films without an edge print. Both 35mm sets are
  CC BY-SA 4.0. The export's 35mm font (`BarlowCondensed-SemiBold.ttf`,
  107 KB, OFL) is `include_bytes!`'d into `spektrafilm-io`, so the wasm
  grows by at least that once `add_border` is linked: measure the
  module's size before and after.
- **The web side**: `web/view.js` (the view: `fitZoom`, `clamp`, `rect()`
  is where the photograph is drawn), `fs_view` in
  `crates/spektrafilm-shaders/wgsl/display_present.wgsl` and its `View`
  uniform (`WgpuBackend::encode_view`, `View::place`, `Engine::show` in the
  wasm crate), `paint()` in `web/app.js` (the page's own canvas: the CPU
  engine, the placeholder, an unpresented frame), `exportJpeg` in
  `app.js`, `download` in `app.js` (fetch with progress), and
  `POST_SECTIONS`/`buildControls` (sections) in `controls.js`/`app.js`.

### How to build it

**1. Settings and controls.** Keep a `border` object in the page beside
`state` (not in `state.params`), with macOS's fields and defaults. The
controls bind to `state.params` paths today (`binding(c)`); add a kind of
binding for border fields (sliders showing percent of a fraction, as
`borderSlider` does; `<input type="color">` gives `#rrggbb` sRGB, which is
what the exporter takes). The style picker is new UI: four buttons with
small drawn swatches (CSS or inline SVG) after macOS's, `aria-pressed`,
disabled with the note when 35mm doesn't fit. Add the section to
`POST_SECTIONS` (`tab: "Post"`, title "Border") so it lands on the Post
pane and the phone's Post tab. A border change calls the view's change
path (a display pass), never `schedule()`. Check the section at desktop
and phone sizes (`SHOT=`, `VIEW=390x844@3m`), and that the picker's
buttons are big enough to tap.

**2. Geometry in one place on the page.** Write `web/border.js` mirroring
`Border.swift`'s geometry (canvas, Instagram canvas, keyline, film inset
and nine-patch pad, the strip's canvas and turn, from `frames.json`), as
macOS mirrors border.rs. Prove it against the engine: for a grid of
photograph sizes (landscape, portrait, square, 3:2, 3:4, odd sizes, 1 px
edges) and every style and Instagram, the canvas size and the
photograph's offset equal what `add_border`/`extend_for_instagram`
produce (`web/node-test.mjs` runs the wasm engine in Node; or a native
test that prints the sizes for the page to compare).

**3. The view fits the canvas.** `view.js` places a photograph today. Give
it the bordered canvas's size and the photograph's offset in it:
`fitZoom` and `clamp` use the canvas (with Instagram, the whole 3:4);
`rect()` stays the photograph's rectangle (the engine's `show`, `paint`,
`renderSide` and the checks rely on it); add `canvasRect()`. 100% is still
one photograph pixel per device pixel. Turning the border on or off, or
changing its width, re-fits a fitted view and keeps a zoomed one's centre.

**4. The preview in the display pass** (the plan: "a border change is a
`show()`"). Extend `View` (Rust `View`, the `View` WGSL uniform,
`Engine::show`'s arguments, `sendView`/`viewKey` in `app.js`) with the
canvas rectangle and the border: style, mat colour, line rectangle and
colour, the Instagram fill, and for the textures their placement. In
`fs_view`, per device pixel: page background outside the canvas; inside
it the Instagram fill, then the mat (or black), the keyline ring, the
photograph (shown or original, sampled as today), then the texture over
everything with its alpha (the 120 frame overlaps the photograph's edge;
the strip covers all but its window). The textures: upload `film.png` or
the film's strip once per choice, with mip levels (the shown images'
`fs_mip` pass makes them), and bind a 1 × 1 texture when there's none.
The nine-patch: corners keep `pad` canvas pixels (`film_geometry`), edges
and centre stretch, laid out landscape and turned for portrait, as
`BorderLayers.layout` does. The strip: scaled so its window covers the
photograph, turned when `turned`. Show Original keeps the border (macOS
does). The page's own `paint()` draws the same with 2D canvas calls
(`fillRect`; nine `drawImage`s for the nine-patch; a rotated `drawImage`
for the strip) for the CPU engine; the placeholder may skip it. The
canvas outside the photograph is then no longer `BACKGROUND`, so check
`zoom.mjs` and `original.mjs`, which compare inside the photograph only.

**5. Fetching textures.** Nothing on the first load (piece 7's rule):
fetch `film.png` when Film is chosen and the film's strip (on 404,
`generic.png`) when 35mm is chosen, with progress through `download`, and
keep them for the session. A film change with 35mm on fetches that film's
strip. Show the border once its texture is there; until then, the
canvas with no texture (macOS shows the photograph alone until the strip
loads).

**6. Export.** In the worker, after the `Rgba8` render (which has Post):
a new wasm entry point takes the bytes and the border, makes an
`ImageBuf` of the encoded sRGB values (byte / 255; `Encoding` sRGB,
encoded), runs `add_border` and, with Instagram, `extend_for_instagram`,
and rounds back to bytes. "Include in export" off skips it.
- `add_border` and `Strip::load` read their textures with `image::open`
  from a `data_dir`. `spektrafilm-io` can't use `spektrafilm_core::data_fs`
  (core depends on io), so split them: a variant that takes the decoded
  textures (or a loader closure), with the path functions as thin
  wrappers so native output doesn't change. The wasm side reads the bytes
  with `data_fs::read` under `/data/borders/...`, so a missing texture
  comes back in `missing` and `engine.js` fetches it and calls again, as
  renders do. The export also needs `35mm/material.png`.
- The native exporter borders the float scan before quantising; the web
  borders 8-bit bytes. Inside the photograph and in flat border areas the
  two agree exactly; where the 120 frame's alpha edge or the strip blends
  over the photograph, expect at most one step. Measure and report it.
- Memory: at 24 MP the bytes become a 288 MB `ImageBuf` (f32), the
  bordered copy is larger, Instagram larger again, and the output bytes
  follow. Measure the wasm memory's high-water mark on the ARW for 35mm
  with Instagram against the 4 GB cap (and note it for piece 6's
  budgets). If it's too much, border the bytes directly instead of going
  through floats.
- Name the file as today; the JPEG still comes from the `OffscreenCanvas`.

### Done when

- Every style, with and without Instagram, on the card (2752 × 1536,
  16:9: 35mm is disabled), the ARW (4000 × 6000, 3:2) and a 3:4 photograph
  (make one in `web/results/` with `magick`, e.g. a 3:4 crop of the card):
  the export's size and offset equal the CLI's, its border equals native
  `add_border` on the same unbordered bytes, and the whole file agrees
  with the CLI's within item 13's tolerance.
- The canvas at 100% matches the export pixel for pixel for Solid,
  Keyline and Instagram (a `compareCanvas` over the whole canvas, not only
  the photograph); Film and 35mm sit where the export puts them
  (screenshots at fit and 100%, landscape and portrait).
- Any border change is display passes only: no new entry in
  `negs.renders` (a `negs.border(settings)` hook for checks, like
  `negs.edit`). Times in a table, desktop and phone.
- Textures download only when chosen, with progress, and once.
- Native output unchanged: the CLI with each `--border` style, with and
  without `--instagram`, byte-identical before and after the
  `spektrafilm-io` split; `cargo check --workspace --all-targets` and the
  core, gpu, math, model, io and spektrafilm-native tests pass.
- `post.mjs`, `zoom.mjs`, `original.mjs`, `placeholder.js`, `touch.mjs`
  and `inspector.js` still pass; a new `web/checks/border.mjs` covers the
  above.
- `docs/web-wasm.md` item 18, the feature inventory's Borders row, and
  this handoff updated for pieces 9 and 10.

### For the owner, not for you to change

- **Licences.** The textures' licences (`film.png` MIT, the 35mm material
  and strips CC BY-SA 4.0 with the attribution in `data/borders/README.md`,
  the font OFL) belong on the credits page, which is the owner's
  uncommitted `web/credits.html`. Don't edit it; list what it needs in
  your report.
- The owner's packager (`web/package-static.mjs`, uncommitted) copies
  `data/profiles`, `data/filters` and the spectral LUTs, not
  `data/borders/`. Say so in the report.

## Pieces 9 and 10 (after 11)

- **Engine entry points that read the data directory.**
  `lut_export::cube` (piece 9) and `presets::export`/`import` (piece 10)
  take a `data_dir: &Path`. Do as piece 11 does for `add_border`:
  variants that take what the wasm crate holds, with the path-based
  functions as wrappers so native output doesn't change.
- **Exports go through the worker** and block its display passes while
  they run (piece 8's lesson below): a pan during a LUT export stalls.

## Lessons from pieces 7 and 8

- **Keep the first load light.** Anything big (samples, textures, LUTs)
  downloads when it's asked for, with progress (`download` in `app.js`
  reads the body against `Content-Length`, which `serve.mjs` now sends).
- **Starting the engine resets the canvases' visibility.**
  `attachCanvas` hid the page's canvas while a placeholder showed on it;
  it now keeps it for `early`. Anything new drawn on the page's canvas
  (the CPU engine's original) needs the same care.
- **`loading` gates renders while a photograph opens**, and `draw` leaves
  the placeholder until the opening photograph's first frame. A new
  display state (the original) shouldn't fight it: disable it while
  `loading`.
- **A lossy DNG's raw data is JPEG tiles.** Any code that looks for JPEGs
  in a RAW must skip the raw directories' strips and tiles
  (`rawData` in `placeholder.js`).
- **Don't time a click by `setTimeout`.** Show Original first suppressed
  the click after a mouse press with a flag cleared on a timeout, and a
  check failed once in five runs. It now goes by the press's
  `pointerType`, and `detail === 0` for keyboard and screen readers.
- **Don't run a compressor on a user's file without `-c`** (or `-k`):
  `gzip file` replaces it. Piece 7 did that to the owner's NEF by mistake
  and had to restore it. Work on copies in your scratchpad.

## Piece 5 (after the review)

### Done before piece 5: correctness

`docs/web-wasm.md` item 13: the browser matches native wgpu and the f64
reference on 63 cases (films, papers, scans, settings, the ARW), and a GPU
memory leak that blacked out a long session's 24 MP frame is fixed. Piece 6
took item 14 and piece 7 item 15; later pieces take the next free number. Rerun
`web/checks/correctness.sh` after engine changes. Add cases for the new
output spaces once they present, with the reference converted the way the
display converts them.

## Ground rules that bite in piece 5

- **Native output must stay byte-identical.** If you change the engine
  (`spektrafilm-core`, `spektrafilm-gpu`, the shaders), render the CLI
  before and after with `SPEKTRAFILM_BACKEND=wgpu` and `cpu` and compare,
  and run `cargo check --workspace --all-targets` plus the core, gpu, math,
  model, io and spektrafilm-native tests. Piece 4 built the HEAD binaries
  into the scratchpad first (`git stash push crates/`, build, copy, `git
  stash pop`) so it could render before and after at any time. Include a
  non-sRGB output (`--color-space "Display P3"`, `"Adobe RGB (1998)"`,
  `"ProPhoto RGB"`) in the CLI set this time.
- **Frames stay presented.** The point of this piece is that non-sRGB
  outputs present too. `negs.renders` and the stage list show it.
- **Benchmark before and after:** test card at 200, 1200 and 2048 px and
  the Sony ARW (`web/bench.sh`), and `node web/compare.mjs` against
  `web/results/base-*.ppm` (default renders must stay byte-identical).
  Latest numbers: card 7–14 / 29–36 / 60–72 ms, ARW 74–84 ms (item 12).
  Take your own baseline first. Add a P3 card render to the set, and
  measure what it costs today: it reads the scan back.
- **Desktop parity:** follow macOS where the apps differ, and note it.

## Piece 5: what the plan asks

> Controls for output gamut, input gamut, CAT16 and the spectral upsampling
> method. Arctic and Mallett need their 6 MB LUTs: fetch them only when
> chosen, with progress.
>
> Output colour space (sRGB, Display P3, Adobe RGB, ProPhoto) and the
> encoding toggle: today any non-sRGB output reads the scan back. Extend the
> GPU display pass (`display_rgba8.wgsl`, `display_present.wgsl`,
> `Pipeline::display_on_gpu`) to show such a scan on the sRGB canvas: decode
> its curve […], convert to sRGB, clip, encode. Decide and document what
> the web preview of a wide-gamut output should be […] and how it relates
> to desktop's Rec. 2020 display path (`preview::display_rgba_with_post`).
> Export keeps encoding the chosen space from a readback.
>
> Done when: every output space presents (no readback) and matches a CPU
> reference conversion of the read-back scan within 1/255; export still
> produces the chosen space.

### The controls (macOS)

`macos/Sources/NegsApp/Inspector.swift`:

- **Color Management**, a group in Scan & Color (line 140), no on/off
  switch:
  - Output gamut, `io.output_gamut_compress.algorithm`:
    `off, oklch, oklrab, cam16ucs, aces_rgc`;
  - Input gamut, `io.input_gamut_compress.algorithm`: `off, xy`;
  - CAT16 adaptation, `settings.use_cat16` (toggle);
  - Spectral upsampling, `settings.rgb_to_raw_method`:
    `hanatos2025, arctic2026beta04, mallett2019`.
  Option titles go through `optionTitle` in `web/controls.js`. Take the
  titles for these values from macOS `Parameters.swift` (Linux:
  `linux/src/parameters.rs`; Windows: `windows/src/state.rs`).
- **Output**, a disclosure at the end of Scan & Color (around line 518):
  - Colour space: `sRGB, Display P3, Adobe RGB (1998), ProPhoto RGB`
    (`SaveSettings.colorSpaces`, `EditorModel.swift` line 1379). On macOS
    this isn't part of the edit. It's the app's `previewSpace`: the
    preview renders in it (`renderState` sets `io.output_color_space`),
    it's remembered for the next save, and a save in another space
    switches it. Sessions keep the edit without it. Mirror that: keep it
    outside `state.params`, apply it when rendering (`params()` in
    `app.js`), and remember it per viewer (`remember`).
  - "Encode with the colour space's curve", `io.output_cctf_encoding`
    (help: "Off writes linear values, for further processing.").
  - "Limit preview to 12 MP" and "Use less memory for export" are
    desktop-only (piece 6 does the web's memory caps); leave them out and
    say so.

The controls check (`web/checks/controls.js`) moves every control. Make
sure it covers the new ones and doesn't choke on the colour space, which
isn't a param.

### What I found in the code (not yet acted on)

**Which of these rebuild, and which need to.**
`preview_pipeline_cache_key` (`crates/spektrafilm-core/src/preview.rs`)
holds `input_gamut`, `output_color_space`, `output_gamut` and, in
`settings`, `rgb_to_raw_method` and `use_cat16`. So every one of them
rebuilds the spectral pipeline today (piece 4 measured a rebuild at
117–142 ms plus a re-meter, on the card).

- Upsampling method, input gamut and CAT16 change the TC LUT or the
  calibration, so a rebuild is right for them. Measure it: a method change
  also downloads its LUT the first time.
- **Output colour space and output gamut may not need one.** In
  `new_with_spectral` they're read only to build `output_gamut`
  (`OutputGamutCompress::build`), and `with_params` already rebuilds that
  from the new params. The chain reads the output space per render
  (`try_gpu_resident`: `output_adaptation`, `output_encode_matrix`; the
  CPU path's `encode_output`). `color_reference.rs` doesn't read it.
  - If that holds, take them out of the key the way piece 4 did: a core
    test that builds with one output space and gamut, updates to others
    through `with_params`, and compares with fresh builds bit for bit,
    then the CLI and helper comparisons.
  - That would make the colour-space picker and the output gamut frame-rate
    changes. The feature inventory already calls output gamut "Resident",
    which is wrong today.
- The metered auto-exposure EV is keyed by the pipeline key
  (`key_of(&state.pipeline)` in the wasm render). Check whether metering
  reads any of these (it reads `params.settings` whole). If only the key
  changed, the re-meter would be wasted.

**The LUTs.** `data/luts/spectral_upsampling/`: `hanatos2025` (the
default, already fetched on the first render), `arctic2026beta04` and
`mallett2019`, 5.97 MB each as `.npy`
(`spectral_service::load_method_lut`, `METHODS` around line 82). They
already arrive on demand: `render` reports the missing file, and
`web/engine.js` fetches it and retries. What's missing is progress:
`fetchData` awaits `arrayBuffer()` with no feedback. Read the body as a
stream against `Content-Length`, post progress from the worker to the
page, and show it (the `busy` indicator, or `showMessage` with a
percentage). Check what `serve.mjs` sends (compression,
`Content-Length`) and how long a first Arctic render takes.

**Today's wide-gamut preview is wrong, not just slow.** For a non-sRGB
output, `display_on_gpu` is false (`output.name != SRGB`). The chain
returns the float scan (`ChainOutput::Scan`), and the wasm render turns it
into bytes with `preview::display_rgba(&output, usize::MAX, encoding)`.
For any non-sRGB encoding that function gives desktop's
**Rec. 2020 with the sRGB transfer** (`DisplaySpace::Rec2020SrgbTransfer`),
dithered. The desktop tags those bytes so the system converts them. The
page draws them on an sRGB canvas as if they were sRGB, so a P3, Adobe or
ProPhoto output looks desaturated. Confirm with a render, and fix it as
part of the GPU work, not separately.

**The GPU display pass.** Since piece 6, presenting writes a frame's
bytes into a texture (`make_shown` in `display_present.wgsl`) that a view
pass draws; a frame displayed on the CPU (`present_rgba`) goes into the
same texture. A non-sRGB output's conversion belongs in `make_shown` and
`display_rgba8.wgsl`; the view pass only places bytes.

- `display_rgba8.wgsl` and `display_present.wgsl` both take `encoded` and,
  if the scan isn't encoded, apply the sRGB curve, then clip and round.
- What a non-sRGB output needs, per pixel: decode the output's curve if
  the scan is encoded, then a 3×3 matrix from the output's primaries and
  white to sRGB (with chromatic adaptation where the whites differ:
  ProPhoto is D50, the others D65). Then clip to [0, 1], sRGB-encode and
  round. Upload the matrix and the curve as uniforms.
- The curves (`Cctf` in `crates/spektrafilm-math/src/colorspace.rs`): P3
  uses `Srgb`; Adobe RGB is `Gamma` (check the exponent the space
  carries); ProPhoto is `Romm`, which has a linear segment near black.
  It's not a pure gamma, as the plan says. Implement exactly what
  `Cctf::decode` does.
- P3 already has the sRGB curve, so `gpu_encodes_output()` is true for it
  and the chain encodes it on the GPU. Adobe and ProPhoto aren't encoded on
  the GPU (`finish_resident` encodes them on the CPU after a readback). So
  for those, either teach `post_scan.wgsl` their curves, or have the
  display pass take the linear scan and skip the decode, which is simpler
  and exact.
- The matrix should be the one the CPU reference uses. Use the same
  `colorspace` functions (`icc_rgb_to_rgb`, Bradford, as
  `display_rgba_with_post` uses for Rec. 2020), not a new derivation.
- Then `display_on_gpu` becomes true for every output space (still not
  with post adjustments, which are Stage 1's "later" list).
- Only the wasm render calls `display_on_gpu`. The native helper and CLI
  use `ChainOutput::Scan`, so their output shouldn't move. Still render
  them before and after.

**The CPU reference.** "Matches a CPU reference conversion of the
read-back scan within 1/255": write that reference as a function. Take the
scan in the output space, convert it to sRGB bytes without dither, and use
it both in the wasm render's CPU fallback and in a test. Compare the
GPU's bytes (`ChainOutput::Rgba8`) with it for each space and both
encodings. The gpu tests run on this machine (Vulkan), so a native test
can do it. In the browser, compare `negs.read(maxDim)` (bytes) and the
saved canvas against the same function's output.

**What the preview should be.** The plan asks you to decide and document
it:

- An **sRGB rendition** (clip to sRGB) is what an sRGB canvas can show and
  needs nothing new. Colours outside sRGB clip, so the preview of a P3
  output differs from what a P3 display would show of the export.
- A **P3 canvas**: WebGPU's `GPUCanvasConfiguration` has
  `colorSpace: "display-p3"`. Whether wgpu 24's web backend exposes it is
  unverified (the plan's research item). Check wgpu's
  `SurfaceConfiguration` and its webgpu backend source in
  `~/.cargo/registry` before deciding. If it's reachable, a P3 canvas
  shows P3 and Adobe outputs whole on P3 displays (most Macs, iPads and
  iPhones), and ProPhoto clipped.
- **Desktop** shows non-sRGB outputs as Rec. 2020 with the sRGB transfer,
  dithered, because the system converts that for the display. The web
  can't hand the compositor a Rec. 2020 canvas.

Recommend one, implement the sRGB rendition at least, and write down the
difference from desktop.

**Export.** The page exports a JPEG from the canvas: `exportJpeg` in
`app.js` renders without presenting, puts the bytes in an
`OffscreenCanvas` and calls `convertToBlob`. For a non-sRGB output those
bytes are today's Rec. 2020 bytes, with no profile, so a non-sRGB export
is wrong now too. "Export still produces the chosen space" needs bytes in
the chosen space and its ICC profile:

- `spektrafilm_io::save` is path-based. `save_jpeg` (`save.rs` around
  line 160) uses an encoder that already takes the profile
  (`set_icc_profile`, from `crate::icc::profile(space, linear)`). Make the
  encoders write to any `Write`, keep the path functions as wrappers (the
  native output must not change), and export an `encode` from wasm that
  takes the read-back scan. The canvas route can't embed a profile.
- Memory: a float readback at 24 MP was 1.2 GB peak before the bytes-only
  path (item 6). Measure an export of the ARW in each space against the
  4 GB cap, and note it for piece 6's budget.
- Check the file: open it with `magick identify -verbose` (profile name)
  and compare its pixels with the CLI's export in the same space
  (`--color-space`), within 1/255.

### How to prove it

- **Engine:** CLI before/after (wgpu and cpu; sRGB, P3, Adobe, ProPhoto;
  encoded and linear) byte-identical. If the key changes, a with_params
  test as piece 4's, plus the helper's previews before and after.
- **GPU bytes:** for each space and encoding, the GPU display bytes match
  the CPU reference within 1/255, natively in a gpu test and in the
  browser (`negs.read` and the canvas).
- **Presenting:** `web/checks/controls.js` (with the new controls):
  every row presented, and which ones rebuild, with times. Put a table in
  `docs/web-wasm.md` item 14, and update the feature inventory's
  "Resident" claims you've checked.
- **LUT download:** a cold Arctic and Mallett render's time, size and the
  progress you show.
- **Export:** one file per space, with its profile and pixels checked.
- Benchmarks before and after, and screenshots of the new controls at
  desktop and phone sizes (`SHOT=`, `VIEW=390x844@3m`).

## Practical notes

- The dev server runs at `node web/serve.mjs` (port 8137); piece 7 left it
  running (restarted for its `Content-Length`). Restart after
  editing `serve.mjs`; kill by port:
  `kill $(ss -ltnpH 'sport = :8137' | grep -o 'pid=[0-9]*' | cut -d= -f2)`.
- After changing Rust, `web/build.sh --threads` (also regenerates
  `web/catalog.json`). The single-threaded build shares `target/` with
  native cargo, so it waits on a running `cargo test`.
- Measure on a quiet machine. Piece 4's first enlarger timings were 3–6×
  too slow because `cargo test` was running alongside.
- Page checks: `web/checks/controls.js` (every control),
  `enlarger.js`, `updated.js` (updated vs fresh renders; its header says
  how), `balance.js`, `inspector.js` (desktop wiring; its `fieldFocused`
  is false in this unfocused window, before piece 4 too) and `touch.mjs`
  (phone layout), `zoom.mjs` (zoom and pan) and `placeholder.js`
  (piece 7), `original.mjs` (piece 8). Rerun the last five after UI changes.
- `negs` in the page: `state()`, `edit(path, value)`, `renders`, `idle()`,
  `open(url)`, `read(maxDim)` (render, read back the bytes),
  `balance(...)`, `view`, `views`, `placeholder()`, `timeline`, `badge()`,
  `openSample()` and Show Original's `original(on)`, `showingOriginal()`,
  `originalMade()`, `prepareOriginal()` and `readOriginal(maxDim)`;
  `docs/web-wasm-handoff.md` lists them all.
- Messages: `showMessage(text, error, ms)` with `ms` stays up through the
  renders that follow and hides itself after `ms`; without it, the next
  finished render hides it. The badge (`setBadge`) is for states the
  photograph is in (downloading, developing, the original).
- Controls that need the engine read `host` in `sync()`. `startEngine`
  calls `sync()` once the engine is up.
- Keep temporary files in your scratchpad; use
  `BENCH_PROFILE=$(mktemp -d -p <scratch>)` and `CDP_TMP=<scratch>` until
  the scripts clean up after themselves.
- The shell's working directory resets between commands. Use absolute
  paths, or `cd /home/noah/Work/Negs-wasm &&` first.
- Chrome in these test profiles fails `blob()` on a large response; read
  an `arrayBuffer()`. A Chrome left running through many renders can lose
  its GPU state; restart it between benchmark runs.
- Don't open an upstream rawler PR without asking.
