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
     (`display_present.wgsl`), with nothing read back. Since piece 6 the
     canvas keeps them as a texture and draws them where the view puts
     the photograph (item 14).
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
   for itself (`touch-action: none`; piece 6 zooms and pans it, item 14),
   and the page doesn't zoom; the inspector keeps `manipulation` (no
   double-tap zoom). Safe-area
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
   preview size, which iOS may not allow a tab (piece 6 caps what it
   renders, item 14, but not the decoded source).

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

13. **Correctness against the reference, and GPU memory** (2026-10-29,
   before piece 5). The browser's WebGPU renders against the f64 CPU
   engine the desktop apps export with (`spektrafilm-f64`,
   `SPEKTRAFILM_BACKEND=cpu`) and against native wgpu (Vulkan), with the
   same params: `web/checks/correctness.sh`. 63 cases on four photographs
   (the test card at 2752 px; three landscapes at 1600 px, with their ICC
   profiles) and the 24 MP Sony ARW, all rendered in one page as a viewer
   would (the pipeline cache carries across them), each presented and then
   read back at full size:

   | Cases | Browser vs native wgpu | Browser vs f64 reference |
   |---|---|---|
   | 20 film/paper pairs: every colour-negative family on Endura, Premier, Supra, Ultra, Edge, Crystal Archive, 2383 and 2393; B&W on 2302 and MGRC | ≥ 99.989% exact, all within 1/255 | worst pixel ΔE00 1.23, at most 0.002% of pixels over 1, at most one 8-bit step |
   | 7 scanned directly: Velvia, Provia, Ektachrome, Kodachrome, Tri-X reversal; Portra 400 and Vision3 250D as negatives | ≥ 99.990%, all within 1/255 | ΔE00 ≤ 1.23, one step |
   | 29 settings on Portra 400 / Endura: ±2 EV, auto exposure off, matrix and highlight metering, lens blur, 120 and 16 mm formats, lens diffusion (Black Pro-Mist; Glimmerglass with warmth), print diffusion, halation off and strong, DIR off and strong, film and print curves, film base off and tuned, print base off, enlarger shifts, preflash, print exposure without compensation, scanner black/white correction and blur without sharpening, CAT16 off, output and input gamut compression off, Mallett upsampling, linear output | ≥ 99.986%, all within 1/255 | ΔE00 ≤ 1.23, one step |
   | ARW 24 MP, Portra 400 and Velvia | ≥ 99.996%, all within 1/255 | ΔE00 ≤ 1.22, one step |

   These ran with grain and both glares off, since those are random. The
   one-step differences sit in dark tones, where one sRGB code is about
   1 ΔE00. Native wgpu agrees with the reference just as closely, so the
   browser adds nothing to the GPU engine's known f32 difference
   (`docs/gpu-vs-cpu-quality.md`). A linear output is shown with the sRGB
   curve applied for display on both paths (`preview::display_rgba`), so
   the reference conversion applies it too.

   With grain and glare on (Portra 400, Velvia, Double-X on 2302, Vision3
   500T on 2383 on the card, Gold 200 on the ARW), the browser draws the
   same grain as native wgpu: 99.5–99.97% of pixels exact, 99.99% within
   1/255, at most 12 steps where a grain count lands on the other side of
   a threshold. The reference's grain is a different random field, so
   compare the image averaged over 33×33 px: p99 ΔE00 0.69–1.11, and no
   colour bias (mean ΔL\*, Δa\*, Δb\* within ±0.004). The reference
   differs from itself as much when its grain is drawn again (the input
   shifted one pixel): Vision3 500T p99 1.15 (browser 0.97), Portra 400
   1.05 (0.90).

   **A memory bug it found.** With 63 cases run in one page, the last one,
   the ARW with grain at 24 MP, came out black: Chrome logged
   `vkAllocateMemory failed with VK_ERROR_OUT_OF_DEVICE_MEMORY` and an
   invalid `effect_scratch_1`, and the page showed the black frame without
   an error. Rendered alone, the same frame is right. In wgpu 24's WebGPU
   backend, dropping a buffer does nothing: its memory comes back when the
   garbage collector takes the JavaScript `GPUBuffer`, which in the
   engine's worker can be many renders later. So every full-image buffer a
   render let go of (image-pool sets evicted for size, resident inputs of
   an earlier photograph, effect buffers such as the diffusion FFT grids)
   kept its memory. On this 6 GB GPU (1.5 GB of it the desktop's), Chrome's
   GPU process sat at 2.4–2.9 GB between renders. A phone or iPad has far
   less.

   The fix destroys them explicitly in browsers (`release` in
   `wgpu_backend.rs`; natively a dropped buffer is freed and nothing
   changes):
   - the buffers a chain makes for itself, once it's submitted
     (`create_buffer_init` and the effects' own allocations register with
     `transient`; WebGPU keeps the memory until the submitted work is done);
   - image-pool buffers a render didn't reuse, and sets the pool lets go;
   - evicted resident inputs and a replaced TC LUT.

   `web/checks/gpu-memory.mjs` cycles four photographs, the 24 MP ARW and
   effects, and logs Chrome's GPU memory after each frame. Before, it was
   2.4–2.9 GB after most frames. After, it follows the frame: 1.7 GB right
   after a 24 MP render, 0.5–0.8 GB after the others. The 63 cases pass
   with no allocation error. Native renders are byte-identical: the CLI on
   wgpu and cpu, in sRGB, Display P3, Adobe RGB and ProPhoto, encoded and
   linear, with both diffusions and an enlarger shift, and the ARW.
   Default browser renders are byte-identical to before (card at 200, 1200
   and 2048 px and the ARW, bytes and canvas), and frames cost the same:
   card 8–16 / 26–35 / 58–71 ms, ARW 69–87 ms (before: 7–14 / 29–36 /
   60–72, 74–84).

14. **Zoom, pan and true resolution** (MVP piece 6, done before piece 5
   at the owner's request). The canvas fills the view at its device
   pixels, and the photograph moves on it without the film chain.

   **What stays on the GPU.** A presented frame's display bytes become the
   *shown image*: an RGBA8 texture with mip levels (`Shown` in
   `wgpu_backend.rs`), written by `make_shown` in `display_present.wgsl`
   from the scan, or from the bytes a CPU-displayed frame read back
   (`present_rgba`). A *view* (`View`: the canvas's size and the whole
   photograph's rectangle on it, in device pixels) says where to draw it,
   and `WgpuBackend::show` (the wasm `Engine.show`) draws it there again:
   one render pass, no chain. The plan said to keep the scan; I kept the
   bytes instead, because an sRGB preview needs nothing else, they're a
   third of the memory (4 bytes a pixel, 128 MB with mips at 24 MP, against
   288 MB of f32), and drawn 1:1 they are exactly the bytes, so no image
   pool buffer has to outlive its render. Display-only Post adjustments
   (later) will need the scan, or the adjusted bytes made in the chain. The
   last two sizes stay (a slider's interactive and settled frames), within
   256 MB; opening another photograph frees them and its uploaded inputs
   (`forget_photograph`).

   **Drawing.** An image within a pixel of its rectangle is drawn 1:1 at
   whole pixels (`View::place`): the fitted frame, rendered to the fitted
   size, and a full frame at 100%. Anything else is sampled as macOS's
   canvas layer filters: trilinear below 1, from mip levels made the first
   time a view shrinks that image, bilinear above. Around the photograph is
   the page's background. Without presenting (the CPU engine,
   `?present=0`) the page draws the bytes on its own canvas the same way
   (`paint` in `app.js`).

   **The view** (`web/view.js`), as desktop's canvas
   (`PreviewCanvas.swift`): fitted until zoomed, and fitted again on
   resize; pinch, and Ctrl/⌘- or Option-wheel (a trackpad's pinch arrives
   as Ctrl-wheel) zoom around the pointer; a drag or a plain scroll pans; a
   photograph smaller than the view is centred and a larger one can't leave
   it; zoom runs from 0.5% to 1600% in CSS pixels, as macOS's
   magnification. 100% is one photograph pixel per device pixel (desktop's
   Actual Size). Fit and 100% buttons sit at the bottom right with the zoom
   level. Two differences from macOS, both from the plan: a double-click or
   double-tap toggles fit and 100% around the point (macOS double-click only
   fits), and Fit still stops at 100% in CSS pixels, as the web did before
   (macOS enlarges a small photograph to fit). No ⌘0/⌘1: browsers keep ⌘1
   for their tabs. No pan momentum.

   **What renders.** A settled frame has as many pixels as the view covers
   (`renderSide`), so all of them from 100% on: true resolution, the
   whole photograph. That's desktop's interactive-frame rule; desktop's
   settled frame is the export size unless "Limit preview to 12 MP" is on.
   It's capped at 40 MP, 12 MP on touch screens, by the GPU's buffer
   limits, and by its largest texture (the device now asks for the
   adapter's, 16384 here, not WebGPU's default 8192). The CPU engine
   renders at most 4 MP, or the fitted size where that's more, since every
   frame takes seconds there. A zoom or resize renders 200 ms after it stops, and only
   when the view needs more pixels than the last settled frame or, fitted,
   other ones. Zooming out and panning never render. Interactive frames
   keep the 90 ms budget, so while a slider moves at 100% they're shown
   magnified. When the frame has fewer pixels than the photograph, the
   status line says so as desktop's does: "4000 × 6000 · shown at 71% · …"
   (with desktop's Image Size hint as its tooltip), and it wraps rather
   than cut that off on a phone.

   `web/checks/zoom.mjs` drives it with wheel events (and a two-finger
   pinch on a touch screen), the 24 MP ARW, Chrome, GPU memory from
   `nvidia-smi`:

   | | Desktop (750×1235 view) | Phone (390×844 @3, touch) |
   |---|---|---|
   | Fitted | 686×1029, 1:1, "shown at 17%" | 964×1446, 1:1, "shown at 24%" |
   | Actual Size | renders 4000×6000, 1.29 s (the first full-size render: 0.4 s uploading it) | the 12 MP cap: 2828×4242, 0.60 s, "shown at 71%" |
   | 60 wheel pans in 1 s | 59 display passes, 4.0 ms median, 4.3 ms p90 (call to GPU done); no render | 60 passes, 3.9 / 4.2 ms; no render |
   | Canvas against a render read back | at 100%, the export: 100% of the 750×1235 shown pixels exact | at 71%, where the capped frame is 1:1, against a render at that size: 100% exact |
   | Zoom out to 25% | no render; 41 ms the first time (mips of 24 MP) | no render; 23 ms |
   | Ctrl-wheel to 502% | no render | no render |
   | Pinch, fingers 240 → 80 px apart | | zoom × 0.333 |
   | Fit again | renders 686×1029 in 32 ms, 1:1 | 964×1446 in 167 ms |
   | Chrome's GPU memory | 112 MiB fitted, 1644 after the 100% render, 551 fitted again | 228 fitted, 973 at 100%, 389 fitted again |

   The engine's wasm memory stays 954 MiB throughout (the decoded ARW; no
   frame adds to it). On the CPU engine, the card fitted is 1:1 and exact,
   Actual Size renders the 4 MP cap (2677×1494, "shown at 97%", 11 s), and
   wheel pans redraw the page's canvas without rendering.

   The automation (`?image=…`) now shows its frame 1:1 and compares the
   canvas with the image read back (`canvas` in its JSON, a line in
   `web/bench.sh`): 100% exact at 200, 1200 and 2048 px and on the ARW.
   Benchmarks, before / after: card 7–13 / 6–10, 30–34 / 27–36,
   57–70 / 56–69 ms; ARW 71–88 / 72–83 ms. `web/checks/controls.js`: all
   99 rows presented, none rebuilt, 24–74 ms. `inspector.js` and
   `touch.mjs` pass (a pinch on the photograph now zooms it 6×; the page
   stays at scale 1).

   Native output is unchanged: everything new is browser-only. The CLI
   before and after renders byte-identical files (card default, both
   diffusions with a magenta shift, Display P3, on wgpu and cpu; the ARW on
   wgpu), `cargo check --workspace --all-targets` passes, and so do the
   core, gpu, math, model, io and spektrafilm-native tests.

   Not seen on a device yet: pinch and double-tap on iOS, Safari without
   Chrome's device-pixel box (the view sizes from `devicePixelRatio` there
   and watches it change), and how much of the 12 MP cap's GPU memory an
   iPhone allows.

15. **Placeholder Image and the sample photograph** (MVP piece 7). Whatever
   opens shows at once from a placeholder, with desktop's "Developing"
   badge (macOS `ActivityBadge`), until its first developed frame takes its
   place. The empty screen also offers a sample photograph, which the
   owner chose (`DSC_6377.NEF`, a Nikon Z f), as a 3.4 MB lossy DNG that
   downloads only when asked for, so the page's first load is unchanged.

   **The placeholder** (`web/placeholder.js`), as Linux `photo.rs`
   `decode`: a JPEG or PNG through the browser's `createImageBitmap` (it
   applies the file's orientation, as the engine does); a RAW through the
   camera's JPEG preview inside it. The JPEGs are found by walking each
   candidate's markers to its real end (desktop takes the first end-of-image
   marker after each start, which an EXIF thumbnail cuts short). Only ones a
   browser decodes count (SOF0–2, not a CR2's lossless raw data), and not
   the strips or tiles a TIFF's raw directory (CFA or LinearRaw) points at:
   a lossy DNG's raw data is JPEG tiles of linear camera RGB. It takes the
   smallest preview covering the stage's long side, or the largest. A
   TIFF-based RAW's primary orientation replaces the preview's own EXIF tag
   (patched to 1 in a copy before decoding), so the orientation is applied
   once; other containers keep the preview's. The largest preview's size
   stands in for the photograph's until the engine says, and the view keeps
   the placeholder's place when it does (`view.resize`). It's drawn on the
   page's canvas; the GPU canvas shows from the first presented frame.

   **The sample.** The NEF is 31.6 MB and doesn't compress (gzip, zstd,
   brotli and xz all save under 1%), over Cloudflare Pages' 25 MiB per-file
   limit and slow on a phone. `web/sample/make-dng.py` makes a lossy DNG of
   it, as Adobe's DNG Converter would: libraw (`dcraw_emu`, AHD) demosaics
   it to linear camera RGB with rawler's black and white levels and crop,
   a 2.2 curve takes it to 8 bits, 512 px JPEG tiles (quality 85) hold it,
   and a `LinearizationTable` undoes the curve; the camera's 1620 × 1080
   preview is IFD0 and the raw data a SubIFD, with the Z f's D65 matrix
   (rawler's) and the as-shot balance (`WB_RBLevels`). rawler's DNG decoder
   reads it unchanged. Its renders against the NEF's (Portra 400 on
   Endura, grain off, 1600 px): mean colour within 0.5/255 per channel;
   pixels differ by the demosaic (libraw's AHD, rawler's own) and the JPEG,
   and at 100% (the full 6048 × 4032 through the CLI) NEF, quality 85 and
   quality 92 (5.0 MB) look the same, with no banding in the dark water or
   the sky. Quality 85 is 3.0 MB, 3.4 MB with the preview. "Try a sample
   photo" downloads it with "Downloading N%" in the badge (from
   `Content-Length`, which `serve.mjs` now sends).

   **Measured** (`web/checks/placeholder.js`, localhost, Chrome on the
   GTX 1660 SUPER, ms after the open):

   | Photograph | Placeholder | Decoded | First frame |
   |---|---|---|---|
   | Sony ARW, 24 MP portrait (preview 1616 × 1080) | 104 | 1119 | 2357 |
   | Sample DNG, by its button (with its download) | 37 | 601 | 1027 |
   | Sample NEF, for comparison | 98 | 832 | 1400 |
   | Sample NEF on the CPU engine | 114 | 1394 | 5753 |

   The ARW's preview is 0.25% narrower than its 6000 × 4000 frame, so its
   swap moves the edge by about 2 device pixels; the sample's matches
   exactly. With a full 6048 px preview (picked when the stage is wider
   than 1620 px) a placeholder took 379 ms. The check also ports
   `photo.rs`'s orientation test (all eight orientations in both byte
   orders over a conflicting EXIF tag, downscaling, a bare JPEG and an
   invalid container tag keeping the preview's), checks that the DNG's
   placeholder is its preview at 256, 600 and 2000 px, and that the page
   starts on the empty screen with no sample download. `inspector.js`,
   `touch.mjs` and `zoom.mjs` pass unchanged. Nothing in the engine
   changed, so native output and the benchmarks are untouched.

16. **Show Original** (MVP piece 8, the owner's Before/After). Holding the
   button at the bottom right (before the zoom level), or `\`, shows the
   photograph before any film in the developed frame's place, with the
   same view; letting go shows the developed frame again. A tap toggles it
   on touch screens, where holding would hide the photograph under the
   finger, and the button stays tinted while it's on. The badge says
   "Original" ("Loading Original" until it's made).

   **Which original.** The engine's decoded source, as the native helper's
   `original` (Linux and Windows show it): rawler's develop for a RAW, the
   file for a JPEG, with no tone curve, so it looks flat and dark next to
   the film. The owner chose it. macOS shows Apple's RAW develop
   (`CIRAWFilter`), which looks like the camera's own JPEG; a browser has no
   such decoder. The camera's embedded preview (piece 7's placeholder) was
   the other candidate: closer to macOS's look, but about 1620 px (soft at
   100%), missing from some files and framed a little differently.

   **How.** `original_encoding` (and its `convert_linear`) moved from the
   helper to `spektrafilm_core::preview`, so the helper and the browser
   share one copy. `Engine.original(params, maxDim, present)` takes the
   source at the settled frame's size, resampled as that frame's input is
   (the same cached resample, `source_at`), and:
   - for an sRGB display (every output the web offers today), keeps it on
     the canvas from the input the film chain already has on the GPU
     (`set_original_from_input`: the resident input by its key, through
     `make_shown`, the pass a presented frame's scan takes). Resident
     inputs are now created with `STORAGE` usage for this; no native output
     changes. Under 1 ms, at any size;
   - otherwise (another output space, the CPU engine, no canvas) makes the
     bytes with `preview::display_rgba`, as the helper does, and keeps them
     on the canvas (`set_original`) or returns them to the page. The card
     fitted takes 7 ms, 24 MP 290 ms. A first version made every original
     this way; at 24 MP that held the engine's worker for 330 ms, and a
     switch queued behind it.

   The canvas keeps the original in its own slot next to the two shown
   frames (`Canvas.original`), counted in their 256 MB budget (at 24 MP it
   and the settled frame fill it, so the interactive frame's texture goes).
   A view with `original` draws it instead of the developed frame, so
   switching is `show()`: one display pass, no film chain. A render that
   finishes while it shows (a slider moved with `\` held, a settled frame
   after a zoom) updates the developed frame underneath without drawing
   over it. The page keeps one original until the photograph or engine
   changes; when the settled frame grows (zoomed to 100% while held), the
   one it has shows smoothed while the new size's is made. The pointer
   reaching the button makes it ahead of a press, as macOS's hover does.
   On the CPU engine the page keeps the original's bitmap and paints it
   (`paint` in `app.js`). Disabled, as macOS's `canShowOriginal`, until the
   photograph's first frame and while another opens; opening a photograph
   lets go of it.

   **Measured** (`web/checks/original.mjs`, Chrome on the GTX 1660 SUPER):

   | | Card (2752 × 1536) | Sony ARW (4000 × 6000) |
   |---|---|---|
   | Original made, fitted | 686 × 383, 0.9 ms | 686 × 1029, 0.8 ms |
   | Press / release, fitted | 1 display pass each, 5.9 / 4.7 ms; no render | 8.9 / 4.8 ms; no render |
   | At 100%, held | made at 2752 × 1536 in 0.5 ms | made at 4000 × 6000 in 0.5 ms; repeated switches 2–3 ms |
   | Pan while it shows | 20 passes, about 3 ms; no render | the same |
   | Slider while it shows | 1 render; the canvas still shows the original | the same |
   | Canvas against the original's bytes | 100% exact, fitted and at 100% | 99.999%, at most 1 |
   | Full size against the helper's `original` | identical | 15 of 72 million values differ by 1 |
   | `\` held with key repeat, let go | one pass each; ignored in a value field | the same |

   The view (zoom, rectangle) is the same before and after every switch.
   The GPU's bytes differ from the CPU's by at most 1 in a few pixels (f32
   on the GPU, f64 on the CPU), as a presented frame's do. The ARW's 15
   values come from float differences between the wasm and native builds
   of the decode; the card's bytes are identical. A switch right after a
   24 MP read-back took 27–30 ms, the GPU still busy. On a phone
   (`VIEW=390x844@3m`) a tap turns it on and another off, a pass each (4.5
   and 4.7 ms). On the CPU engine a switch repaints the page's canvas (0.1
   ms to issue), the fitted original is exact against its bytes, and at
   100% it's the 4 MP cap's 2677 × 1494, made in 92 ms and drawn sampled.

   Native output is unchanged: the helper's `original` (card and ARW in
   sRGB, the card in Display P3, the ARW in ProPhoto at the 12 MP limit)
   and `render` (the card, an interactive frame, the ARW; wgpu and cpu)
   are byte-identical before and after, and so is the CLI (the card on
   Portra, Velvia in Display P3; wgpu and cpu). `cargo check --workspace
   --all-targets` and the core, gpu, math, model, io and spektrafilm-native
   tests pass, and `zoom.mjs`, `placeholder.js`, `inspector.js` and
   `touch.mjs` pass unchanged.

   For piece 5: the page keys its original by size only, since nothing on
   the web changes how it's shown yet. The output colour space will: add it
   to the key. A non-sRGB output takes the CPU path, whose bytes are
   desktop's Rec. 2020 ones, wrong on an sRGB canvas in the same way as
   today's non-sRGB frames; fix both together.

17. **Post: Tone and Color** (MVP piece 15, the owner's request on
   2026-10-29: the whole Post tab next; Border is piece 11). The Post tab
   of macOS `Inspector.swift`: Brightness, Contrast, Whites and Blacks
   under Tone, with its footer, and Saturation under Color, each −100 to
   100 in steps of 1 (`POST_SECTIONS` in `web/controls.js`). The sidebar
   has macOS's Lab | Post switch (remembered per viewer, `pane`); the
   phone's sheet has a Post tab after Scan. Reset Adjustments resets Post
   too, as on macOS.

   **In the display pass.** `display_post.wgsl` is `PostProcess::apply_rgb`
   with the same tables (`PostProcess::display` hands them over as a
   `spektrafilm_gpu::DisplayPost`) and the same f32 arithmetic. Both
   display shaders append it: `make_shown` (a presented frame) and
   `display_rgba8` (bytes read back: `negs.read`, exports). So
   `Pipeline::display_on_gpu` no longer excludes Post, and a frame with
   Post presents. `ChainOutput::Rgba8` and `Present` carry the
   `DisplayPost`.

   **A Post-only change doesn't run the film.** A shown image remembers
   which render's pooled scan it was made from (`ShownScan`, the
   `ImageSet::generation`). When everything a frame is rendered from but
   Post is the same as the shown frame's (`look_key` in the wasm crate),
   `render` calls `WgpuBackend::repost`: `make_shown` again from that scan
   with the new Post, then the view, a display pass. An interactive frame
   takes the shown frame at its size; a settled one only at its own size.
   When the scan is gone (a render of the same size took the pool's
   buffers, as `negs.read` or an export does, or the pool let them go), it
   renders as before.

   **Measured** (`web/checks/post.mjs`, Chrome on the GTX 1660 SUPER):

   | | Card (2752 × 1536) | Sony ARW (4000 × 6000) |
   |---|---|---|
   | Post slider dragged, fitted (each frame) | display pass, 5–16 ms | 7–18 ms |
   | The same at 100% | display pass, 6–9 ms | **film render, 560–575 ms**: the pool lets a 24 MP frame's buffers go (`MAX_POOLED_BYTES`) |
   | Canvas against the edit rendered and read back | 100% exact, fitted and at 100% | 100% exact |
   | Lab change with Post set | one film render, Post in its display pass; canvas exact | the same |
   | Phone (`VIEW=390x844@3m`), fitted 1098 × 613 | display pass, 5–11 ms; canvas exact | |
   | CPU engine | a film render per frame (130–240 ms interactive, 740–760 ms settled); Post on the CPU as before | |

   `crates/spektrafilm-core/tests/display_post.rs` renders a gradient on
   Portra 400 → Endura on native wgpu and compares the display bytes with
   GPU Post against `PostProcess` plus `preview::display_rgba` on the same
   scan: 18 cases (each control at both ends, combinations, encoded and
   linear output); 2 of 442,368 bytes differ, by 1.

   Native output is unchanged: the CLI (the card on Portra → Endura, and
   Velvia in Display P3; wgpu and cpu) is byte-identical before and after.
   `cargo check --workspace --all-targets` and the core, gpu, math, model,
   io and spektrafilm-native tests pass; `inspector.js`, `controls.js`
   (all 99 Lab controls present), `touch.mjs` and `original.mjs` pass as
   before. `original.mjs`'s fitted card comparison shows one byte off by 1
   (99.999%) with HEAD's wasm too, unlike piece 8's notes.

   For the owner: keeping a 24 MP scan for Post at 100% would hold about
   288 MB more on the GPU between renders (12 MP on phones: 144 MB). Not
   done; say if Post at 100% on large photographs should be a display pass
   too.

## Limits and open problems

- **Memory.** wasm32 caps memory at 4 GB. The GPU path peaks at 0.65 GB for
  24 MP (an export, reading back bytes) and 3.0 GB for 60 MP (measured
  reading back floats, before that). The CPU path needs 2.8 GB at 24 MP, so it can't
  render much past 30 MP. Most of this is copies: source, decoded clone, f32
  upload, readback, output and RGBA. Memory64 is not usable with
  wgpu/wasm-bindgen yet. Preview frames are capped at 40 MP, 12 MP on
  touch screens (item 14); the decoded source and an export aren't.
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
  4 MB LUT. The sample photograph (3.4 MB) downloads only when asked for.
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
   times, pinch and double-tap, and how much memory a tab may use before
   iPadOS ends it (at the 12 MP cap's 100%, about 1 GB of GPU memory here).
4. Wide-gamut output and post adjustments on the GPU, so they present too
   (they read the scan back now).
5. Say so when a GPU allocation fails, rather than presenting a black
   frame: WebGPU reports it as an uncaptured error (item 13), which the
   engine doesn't watch yet.
6. Decode an sRGB-encoded input's curve on the GPU (or once per source):
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
whole frames as the page sees them). It shows the frame 1:1; with
presenting, `<name>-canvas.ppm` is the part of it the canvas shows, and
`canvas` in the timings says how much of that matches the image.
