# Negs in the browser (WebAssembly)

Investigation on branch `wasm-investigation`, 2026-10-28. Both engines run in
the browser: the CPU engine, and the WebGPU engine through the same resident
WGSL chain the desktop apps use. `web/` is a small app around them: open a
photograph or camera RAW, pick film and paper, move the main sliders, export a
JPEG.

## Results

Same photograph, Portra 400 on Portra Endura, default params. Warm means a
second render with the pipeline cached. Linux, Ryzen (12 threads), GTX 1660
SUPER, Chrome 156; Firefox 158 agrees within a few percent.

| | 0.6 MP | 4.2 MP | 24 MP |
|---|---|---|---|
| Native GPU (Vulkan) | 24 ms | 136 ms | 885 ms |
| Browser WebGPU, single-threaded | 30 ms | | |
| Browser WebGPU, threaded | 41 ms | 110 ms | 0.69 s |
| Browser WebGPU before the frame-cost work, threaded | 122 ms | 320 ms | 1.8 s |
| Native CPU | 0.8 s | | 34 s |
| Browser CPU, single-threaded | 7.0 s | | |
| Browser CPU, threaded | 1.6 s | 12 s | 73 s |

Browser WebGPU times are whole frames as the page sees them: previews
presented on the canvas, 24 MP read back as bytes, as an export is (4.2 MP
read back: 135 ms). The 0.6 MP input is a PNG without a profile, whose sRGB
curve the engine decodes on the CPU every frame and so can't keep uploaded; a
0.8 MP JPEG takes 31 ms. Cold renders add 0.3 to 0.9 s, mostly shader
compilation. The 24 MP render peaks at 0.65 GB of wasm memory (1.2 GB
before): it reads back bytes, not floats.

wasm SIMD (`+simd128`) speeds the CPU engine up by 1.5% and changes no
output, so it isn't on: the CPU path is scalar maths, not loops a compiler
vectorises.

Fidelity, 8-bit sRGB output, against the native renders of the same input:

| | exact | within 1/255 | max |
|---|---|---|---|
| Browser WebGPU (Chrome) vs native wgpu | 99.83% | 100% | 5 |
| Browser WebGPU (Firefox) vs native wgpu | 99.84% | 100% | 5 |
| Browser CPU vs native CPU | 80.1% | 99.31% | 9 |
| Native wgpu vs native CPU, for scale | 60.2% | 96.6% | 11 |

The browser CPU's remaining difference is the pure-Rust dgemm (the Windows
build's, in place of BLAS) and software transcendentals. The threaded and
single-threaded browser CPU outputs are bit-identical.

A 24 MP Sony ARW decodes with rawler in the browser in about 0.8 s (threaded)
and renders like the native app, with rawler patched (see below).

In the app, the same ARW (4000×6000) in a 781×1171 view, WebGPU, threaded:

| | time |
|---|---|
| Open (decode) | 0.8 s |
| First render (full-frame metering, spectral pipeline, shaders) | 1.0 s |
| Settled frame, 0.9 MP | 32 ms (250 ms before) |
| Interactive frame while a slider moves: within budget, so at the settled size | 33 ms |
| Film change (new spectral pipeline) | 300 ms |
| Render for export, 24 MP | 0.7 s warm, 2.2 s the first time |
| Settled frame on the CPU engine (12 threads) | 2.7 s |
| A slide film (Provia 100F), 0.6 MP | 22 ms (about 2 s before, on the CPU) |

A warm frame used to wait at least 40 ms on the GPU, about 7 ms per effect
at any size: that was buffer creation in the browser, not GPU work (see
WebGPU frame cost below). Now a 200 px frame takes about 9 ms, and frames
grow with their pixels. The test card, Chrome, threaded:

| | before | after |
|---|---|---|
| 200×112 | 50 ms | 9 ms |
| 1200×670 | 105 ms | 31 ms |
| 2048×1143 | 210 ms | 59 ms |

"Before" there doesn't count drawing the bytes on the page's canvas, which a
presented frame skips. Presented frames match the bytes read back, which match
the CPU's `display_rgba` of the scan, on every pixel. In the browser, a slide
on WebGPU matches native wgpu within 1/255 (99.4% exact).

## What it took

1. **Target plumbing.** `wasm32-unknown-unknown`. `getrandom` needs its
   `wasm_js` backend (`.cargo/config.toml` plus the feature) and rawler's
   `uuid` its `js` feature.
2. **BLAS.** wasm uses the pure-Rust dgemm the Windows build already has
   (`spectral_service.rs`, `cpu_backend.rs` and the two manifests).
3. **Time.** `std::time::Instant::now()` panics on this target. The engine
   uses `web-time`, which is `std::time` natively. rawler calls `Instant` in
   its demosaic (Bayer PPG, X-Trans, CR3), so it is vendored in
   `vendor/rawler` with the same change and patched in with
   `[patch.crates-io]` (see `vendor/README.md`) until upstream takes it.
4. **Data files.** There is no filesystem, so `spektrafilm_core::data_fs`
   replaces the four `std::fs` reads in the render path: profiles, spectral
   LUT, neutral-filter database and colour filters. Natively it reads disk.
   In the browser the page registers fetched bytes, and a render reports any
   files it missed so the page can fetch them and render again (the
   neutral-filter database falls back silently, so this matters). A first
   render downloads about 4 MB brotli (the 6 MB LUT plus about 15 KB per
   profile).
5. **WebGPU.** Browsers can't block on a GPU readback: a buffer maps only
   between tasks, so `map_async`, `poll(Wait)` and `recv()` would hang. The
   resident chain already has one upload and one readback, so it splits
   cleanly:
   - `WgpuBackend::new_async` uses `BROWSER_WEBGPU`; `new()` blocks on it.
   - `start_film_chain` submits and starts the mapping, and
     `FilmChainReadback::finish` reads the result. Natively `run_film_chain`
     is the two in sequence, and its output is bit-identical to before.
   - `ComputeBackend::submit_film_chain` and `PendingFilmChain` expose the
     split, and `Pipeline::submit_resident` / `finish_submitted` wrap it. The
     browser awaits a oneshot between them.
   - A submit says what to make of the scan (`ChainOutput`): the scan itself,
     as native; sRGB display bytes (`display_rgba8.wgsl`), read back at a
     quarter of the float scan's size; or those bytes drawn on a canvas
     (`display_present.wgsl`), with nothing read back.
     `Pipeline::display_on_gpu` says when the bytes are what
     `preview::display_rgba` makes of the scan (sRGB output, no post
     adjustments), and they are, bit for bit.
   - The capacity checks run on browser buffer limits too: Chrome reports
     2 GB bindings and 4 GB buffers here.
   - `ComputeBackend` is `Send + Sync` except on wasm (`MaybeSendSync`, as
     wgpu's `WasmNotSendSync`), because WebGPU objects belong to one thread.

   The WGSL needed no changes: no extensions, at most 8 storage buffers per
   stage, `override` workgroup sizes, and workgroup memory under 16 KB.
6. **WebGPU frame cost.** In Chrome, a buffer created mapped costs about
   0.3 ms in the GPU process (a shared-memory allocation), which
   `create_buffer_init` does for every small uniform and table, dozens per
   effect: that was the 40 ms floor. Compute passes and bind groups cost
   microseconds (`web/webgpu-costs.html` measures each). So in browsers:
   - `WgpuBackend::create_buffer_init` writes contents through the queue
     instead, natively it's wgpu's;
   - the chain's full-image buffers and readback are pooled for the last two
     sizes (`ImagePool`; an interactive frame and the settled frame), up to
     512 MB. Native renders still allocate per render;
   - the input stays uploaded while its key does
     (`FilmChainParams::image_key`: the page's source at one size), which
     saves its upload, 9 ms at 2.3 MP, on every slider move;
   - the TC LUT is uploaded again only when it changes (on every platform).
7. **Entry crate** `spektrafilm-wasm` (wasm-bindgen):
   - `Engine.newCpu()` / `await Engine.newGpu()`.
   - `load(bytes, maxDim)`: PNG/JPEG/TIFF through the `image` crate and
     `spektrafilm_io::colour::convert`, ICC included; RAW through the new
     `spektrafilm_io::load_raw_bytes`.
   - `await render(film, paper, paramsJson, maxDim, interactive, present)`
     gives RGBA8, or draws it on the engine's canvas (`setCanvas`, an
     `OffscreenCanvas` the page transfers to the worker), with the backend,
     per-stage timings and the missing data files.
   - `load` returns the size; `render` renders the
     loaded source at most `maxDim` on the longer side, resampled once per
     size (area average for interactive frames, Catmull–Rom for settled
     ones, as native).
   - The auto-exposure metering frame is made once per photograph from the
     full source, as native, so every render size meters alike, and the
     metered EV is kept while the film, metering method and input colour
     don't change (`Pipeline::with_autoexposure_ev`). Metering took 30 to
     70 ms of every frame before.

   - `catalog(stocks)`: the stock catalog, below.

   Slide films scan in the resident chain, as natively (the native apps
   reach it through `process_with_backend`). Whatever it can't render
   renders on the CPU engine, never the per-stage GPU methods, which block.
8. **Threads.** rayon on Web Workers through `wasm-bindgen-rayon`. This
   needs:
   - std rebuilt with atomics (`-Zbuild-std`; `RUSTC_BOOTSTRAP=1` on the
     stable toolchain, plus `rust-src`) and explicit shared-memory linker
     flags;
   - a cross-origin isolated page (COOP/COEP headers);
   - the engine running in a worker, because rayon can't block the main
     thread.

   Native passes: `cargo check --workspace --all-targets`, and the core, gpu,
   math, model and io tests.
9. **The edit and the catalog** (MVP piece 1). The page keeps the edit as
   desktop does (macOS `EditState`): film, paper and a complete
   `RuntimeParams` from the engine's defaults, with the values a film's
   presets fill (grain lanes, DIR gammas, halation baseline, development
   times) null until edited. It sends the whole object on every render.
   Choosing a film nulls those and pairs the paper as `chooseFilm` and
   `pairFilm` do; "Scan film directly" is `io.scan_film`, on for slides,
   and hides the paper and the print sections.

   The catalog is `spektrafilm_core::catalog`, which the native helper's
   `catalog` now calls (its response is byte-identical). `web/build.sh`
   generates `web/catalog.json` from it with the wasm engine in Node
   (`web/make-catalog.mjs`), rather than the page resolving presets at
   run time: the page can't list `data/profiles`, and resolving every
   stock's presets would mean downloading all 33 profiles (5 MB) first.
   Generating it from the same build keeps it in step with the engine and
   needs no native toolchain.

   With default params every render is byte-identical to before: test card
   at 200, 1200 and 2048 px, ARW at 2048 (bytes and canvas), Provia and
   Tri-X on WebGPU, card and ARW on the CPU. Scanning Portra 400 directly
   matches the CLI's `--scan-film` on native wgpu within 1/255 on 99.998%
   of pixels (max 4); the printed card at its full 2752 px is 99.994%
   (max 10), as before this piece.

   Where the web differed from desktop it now follows it: a film without a
   target paper (Double-X) keeps the paper; the web used to pick Kodak
   2302. The default film stays Portra 400 (macOS starts on Gold 200), as
   the benchmarks use it.
10. **The Lab controls** (MVP piece 2). `web/controls.js` is the macOS
   inspector's control set (`adjustmentGroups` and the Camera section:
   titles, paths, ranges, steps), grouped as there: Camera with Lens
   Diffusion, Film, Print and Scan & Color, with each effect's switch in
   its group's header. Grain shows the film's preset lanes, null until
   edited and null again when every lane is back at the preset; a B&W film
   shows one granularity and one uniformity lane, as macOS. Print
   Diffusion (`enlarger.diffusion_filter`) follows
   `docs/print-diffusion-plan.md` on main (after Enlarger, upstream's
   ranges), which the desktop apps haven't shipped yet. Metering shows
   while auto exposure is on; the Print section is disabled while scanning
   film, with desktop's footer. Each control has a reset button while it
   is off its default, a value that types a new one when clicked (snapped
   and clamped as macOS `snap`; Escape cancels), and the title still
   resets on double-click. Reset Adjustments keeps film, paper, rotation
   and Image Size. Colour management and Output are piece 5, Balance Print
   piece 4. Linux calls grain Blur "Post blur"; the web follows macOS.

   Every control presents its frame on WebGPU (none reads back or falls
   to the CPU). Test card at a 1200 px view, after moving each control
   (`web/checks/controls.js`): every frame is Resident at 13 to 56 ms
   (warm), except these, which rebuilt the spectral pipeline until piece 4
   (item 12; now every one of them is a 15–32 ms frame):

   | Control | Rebuild | Frame |
   |---|---|---|
   | Enlarger: Exposure compensation (toggle) | 146 ms | 165 ms |
   | Enlarger: Magenta shift, Yellow shift | 117 ms | 137–158 ms |
   | Enlarger: Preflash exposure, magenta, yellow | 117 ms | 151–188 ms |
   | Metering method (re-meters, no rebuild) | 42 ms meter | 54 ms |

   Switching an effect on costs one slower frame the first time (Lens or
   Print Diffusion about 110 ms, Halation 41 ms), its shaders compiling.
   Preflash magenta and yellow change nothing without a preflash exposure,
   and Exposure compensation nothing without an exposure offset; grain dye
   clouds and clump size and blur act below a pixel at preview size and
   change the full 2752 px card by at most 1/255, natively too.

   Against the native CLI on wgpu with the same params JSON, at the card's
   full 2752 px, one case per group (lens blur, lens diffusion, film
   curves, grain lanes, grain micro-structure, halation, DIR, film base,
   preflash, exposure compensation, print diffusion, print curves, print
   base, glare, scanner, metering): 99.99–100% of pixels within 1/255, max
   2 to 13, the agreement the default render already has at this size
   (99.994%, max 10). Default renders stay byte-identical, and the
   benchmarks are unchanged (card 10 / 29 / 59–68 ms, ARW 71–83 ms).

11. **Phones and iPads** (MVP piece 3). No desktop app has a touch
   layout, so this one is new. Where a sidebar doesn't fit beside the
   photograph (narrower than 900 px upright, or than 640 px: phones and
   iPads held upright), the inspector is a bottom sheet with a tab per
   section (Stock, Camera, Film, Print, Scan). The sheet rests at just
   its tabs, at the room the photograph leaves at full width (40–62% of
   the screen), or at 80%; its grabber drags between them or steps on a
   tap. Elsewhere (desktop, iPad and phones on their side) it stays the
   sidebar, as macOS; on screens under 600 px tall the engine's footer
   scrolls with the panel.

   On a touch screen (`pointer: coarse`) the targets grow (26 px slider
   thumbs, 44×26 switches, 40 px reset buttons, 48 px group rows), the
   value fields look like fields without hovering, the hint says "Tap",
   and fields and pickers are 16 px, below which iOS zooms in on focus.
   Chrome's own slider swallows a vertical swipe that starts on it, which
   on a phone is most of the panel, and ignores `touch-action`. So on touch
   screens the slider takes no input and its track does: a sideways drag
   anywhere on it moves the value from where it was (as iOS sliders do,
   with no jump on touch), a vertical one scrolls, and a mouse or pen there
   (an iPad's trackpad) sets the value under it. Sliders are filled from
   the minimum, as on desktop. The photograph's area takes pinch and pan
   for itself (`touch-action: none`) for piece 6, and the page doesn't
   zoom; the inspector keeps `manipulation` (no double-tap zoom). Safe-area
   insets pad the sheet and the sidebar. Which groups are open, the tab
   and the sheet's height are remembered in `localStorage` (wrapped, so a
   private window still works).

   `web/checks/touch.mjs` checks it with real touches over the DevTools
   protocol on an emulated 390×844 phone: tabs, a group opening and being
   remembered, a touch drag (interactive frames, then settled, all
   presented), a vertical swipe from a slider (scrolls 151 px, value
   unchanged), the reset button, the grabber (drag up snaps to 675 px of
   844, a tap steps to the 114 px tabs), a trackpad press and drag (Film
   format 91 at three quarters, 33 at a quarter), and a pinch on the
   photograph (page scale stays 1). The desktop wiring check and a mouse
   drag on a desktop slider are unchanged. Default renders stay
   byte-identical; card 9–11 / 28–31 / 57–74 ms, ARW 72–83 ms.

   Not seen on a device yet: Safari's handling of the slider tracks,
   `100dvh` with its toolbars, the safe areas, and whether WebGPU runs in
   its worker. The 24 MP ARW takes 954 MiB in the engine at a phone's
   preview size, which iOS may not allow a tab (piece 6's memory caps).

12. **Enlarger shifts without a rebuild, and Balance Print** (MVP
   piece 4). The magenta and yellow shifts, the preflash (exposure and its
   shifts), Exposure compensation and the print exposure normalization
   left `preview_pipeline_cache_key`. `Pipeline::with_params`, which every
   cached render already goes through, now re-filters the enlarger: the
   print illuminant from the neutral C/M/Y plus the shifts, and the
   preflash's raw exposure, in a `filter_enlarger` that `new_with_spectral`
   calls too, so both take the same f32 → f64 route. It then renormalizes
   the print exposure as before. Nothing else a build makes reads these
   values: the colour reference and the chain's parameters are made per
   render from the pipeline's fields, and the GPU caches only the TC LUT
   (by content). The simplified `Pipeline::new` (tests only) keeps its
   enlarger as built.

   The key is shared, so the desktop helper (macOS, Linux) and the egui
   GUI reuse their pipeline across these edits too. I kept one key rather
   than a web-only one: the update is exact (below), and a second key
   would be a second list of what a pipeline can't change. On the 4.2 MP
   card at the helper's limited preview, an enlarger edit there now takes
   about 167 ms instead of 300 ms.

   Rebuilt against updated:

   - A core test (`with_params_refilters_the_enlarger_like_a_new_pipeline`)
     builds a pipeline with a magenta shift and a preflash, updates it to
     six other combinations of shifts, preflash, normalization,
     compensation and EV, and compares each with a pipeline built for
     them: the illuminant, the preflash raw and the exposure factor bit
     for bit, the params, and the CPU-processed image. All equal; without
     the re-filter it fails on the first case.
   - In the browser (`web/checks/updated.js`), four cumulative edits made
     with `negs.edit` against the same params rendered fresh with
     `?params=`, the card at 1200 px on WebGPU: byte-identical, and none
     of the edits rebuilt.
   - The helper: one session rendering default, two enlarger edits and
     default again, against fresh sessions and the previous binary:
     byte-identical previews.
   - The CLI (which builds its pipeline once) with `wgpu` and `cpu`, card
     default and with shifts, preflash and compensation, and the ARW:
     byte-identical to before.

   Every enlarger edit, test card in a 772×431 view
   (`web/checks/enlarger.js`), WebGPU:

   | Control | Before | After |
   |---|---|---|
   | Magenta, Yellow shift | 171–212 ms (rebuild 122–138, re-meter 32–49) | 15–17 ms (first 36) |
   | Preflash exposure, magenta, yellow | 161–219 ms (rebuild 117–141, re-meter 29–44) | 15–16 ms |
   | Exposure compensation (toggle) | 223–225 ms (rebuild 138–142) | 15–16 ms |
   | Print exposure, Exposure (EV), for scale | 41–49 ms | 16–17 ms |

   A rebuild also re-metered, because the metered EV is kept by the
   pipeline's key; the metering doesn't read the enlarger, so it now stays.
   `web/checks/controls.js` has no control left that rebuilds (99 rows,
   all presented, 16–65 ms).

   **Balance Print** is `balancePrint(film, paper, params)` in the wasm
   crate, `print_balance::solve_neutral_filter_shifts` as the helper's
   `balance_print` calls it, on the CPU. It answers the files it missed as
   `render` does, and the worker fetches them and solves again (a missing
   neutral-filter database falls back silently, so it checks even when it
   solved). For four cases (Portra 400 on Endura, the same with a tuned
   film base, Gold 200 on Crystal Archive with shifts and preflash, Vision3
   250D on 2383) it gives the native helper's shifts to every printed
   digit, in 177–286 ms (native 158–203 ms). The button sits at the end of
   Enlarger as on macOS, with its help text as a tooltip; it reads
   "Balancing…" and is disabled while it runs, while scanning film and
   until the engine has started, sets both shifts as one change and shows
   "Print balanced: magenta -0.01, yellow 0.01" for four seconds (macOS
   puts this in its status line; the page has none). The frame after it
   doesn't rebuild. `web/checks/balance.js` checks it.

   Default renders are byte-identical to before (card at 200, 1200 and
   2048 px and the ARW, bytes and canvas; card and ARW on the CPU).
   Benchmarks, before / after this piece: card 8–15 / 7–14, 26–30 /
   29–36, 58–71 / 60–72 ms; ARW 71–89 / 74–84 ms.

## Limits and open problems

- **Memory.** wasm32 caps memory at 4 GB. The GPU path peaks at 0.65 GB for
  24 MP (an export, reading back bytes) and 3.0 GB for 60 MP (measured
  reading back floats, before that). The CPU path needs 2.8 GB at 24 MP, so it can't
  render much past 30 MP. Most of this is copies: source, decoded clone, f32
  upload, readback, output and RGBA. Memory64 is not usable with
  wgpu/wasm-bindgen yet.
- **Panics abort.** On this target a panic is `unreachable`, which kills the
  module. `catch_unwind` around rawler's develop doesn't help. A panic on one
  of rayon's threads is worse: it aborts that thread and leaves the engine
  waiting forever. The engine's panic hook therefore posts the message on a
  `BroadcastChannel`, tagged with the engine's instance, and the page
  replaces the worker and reopens the last photograph (a truncated ARW is
  the test case).
- **Presenting needs WebGPU in a worker** and `OffscreenCanvas`
  (`transferControlToOffscreen`). Without them the page draws the bytes the
  engine reads back, as before; with `?present=0` too.
- **Firefox paces presented frames.** On Linux (WebGPU behind a pref) its
  presented 1200 px frames alternate between about 80 and 120 ms, where
  reading the bytes back takes a steady 92 ms and Chrome presents in 31 ms.
  It looks like compositor pacing, not GPU work; not looked into further.
- **RAW.** rawler only: no Apple decoder, so no Nikon HE★, the same as on
  Windows and Linux. Those fail cleanly ("NEF compression
  Some(HighEfficencyStar) is not supported").
- **Availability.** WebGPU ships in Chrome and Edge, Safari 26 and Firefox
  141+ (Windows). On Linux I enabled it in Firefox with `dom.webgpu.enabled`.
  Chrome on Linux needs `--enable-unsafe-webgpu --enable-features=Vulkan`,
  and with `--ozone-platform=wayland` Vulkan breaks 2D canvas uploads
  (`putImageData` draws nothing), so use `--ozone-platform=x11`. Headless
  Chrome on this machine only gets SwiftShader. The CPU engine is the
  fallback everywhere.
- **Hosting.** Threads need COOP/COEP headers, which GitHub Pages can't set
  (a service-worker shim can). wasm-bindgen-rayon's worker imports the
  package by directory URL, which a bundler or `index.js` naming handles.
- **Download.** 7.4 MB wasm (1.5 MB brotli; `wasm-opt` not tried) plus the
  4 MB LUT.
- **Not wired.** Export beyond an 8-bit sRGB JPEG from the canvas
  (`spektrafilm_io::save` is path-based; the encoders work in memory), RAW
  white balance, crop, borders, LUT export, presets, EXIF, and the golden
  suite in a browser.

## Next steps

1. Land the engine changes. They are small and leave native output
   unchanged.
2. Upstream the rawler `web-time` change and drop `vendor/rawler`.
3. Try it on an iPad (Safari 26), over `serve.mjs --https` (see the
   handoff): that WebGPU runs in the worker and presents there, the frame
   times, and how much memory a tab may use before iPadOS ends it.
4. Wide-gamut output and post adjustments on the GPU, so they present too
   (they read the scan back now).
5. Decode an sRGB-encoded input's curve on the GPU (or once per source):
   inputs read without a profile pay a CPU decode and a full upload every
   frame, 10 ms at 0.6 MP.

## Running it

```sh
web/build.sh --threads          # web/pkg, web/pkg-threads and web/catalog.json; needs wasm-bindgen-cli 0.2.121
node web/serve.mjs              # http://localhost:8137/
node web/node-test.mjs in.png kodak_portra_400 kodak_portra_endura 1024 out.ppm   # CPU, headless
```

The engine always runs in a worker (`web/worker.js`): the threaded build when
the page is cross-origin isolated, the single-threaded one otherwise or with
`?threads=0`. `?backend=cpu` picks the CPU engine. The page also takes
`?image=<url>&max=<px>&runs=<n>&name=<n>[&interactive=1][&params=<json>][&present=0]`,
renders, and posts the image and timings to `web/results/` (`frames` are
whole frames as the page sees them; with presenting, `<name>-canvas.ppm` is
what the canvas shows).
