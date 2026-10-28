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
3. Try it on an iPad (Safari 26): that WebGPU runs in the worker and
   presents there, the frame times, and how much memory a tab may use before
   iPadOS ends it.
4. Wide-gamut output and post adjustments on the GPU, so they present too
   (they read the scan back now).
5. Decode an sRGB-encoded input's curve on the GPU (or once per source):
   inputs read without a profile pay a CPU decode and a full upload every
   frame, 10 ms at 0.6 MP.

## Running it

```sh
web/build.sh --threads          # web/pkg and web/pkg-threads; needs wasm-bindgen-cli 0.2.121
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
