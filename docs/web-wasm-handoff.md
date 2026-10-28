# Handoff: Negs web demo

For the next agent continuing the browser (WebAssembly) demo. Read
`docs/web-wasm.md` first: it covers what was done, the measurements and the
limits.

## Where things are

- Worktree `/home/noah/Work/Negs-wasm`, branch `wasm-investigation`, made
  from `main` at 5a04c47. Work is committed on this branch and not
  pushed. Commit each finished piece here; don't push or merge to `main`
  until the owner says so (for this branch that overrides `AGENTS.md`'s
  commit-to-`main` rule).
- `target/linux-toolchain` in this worktree is a symlink to the main
  checkout's toolchain (Rust 1.93.1). That toolchain has the
  `wasm32-unknown-unknown` target, `rust-src` and `wasm-bindgen` 0.2.121 in
  `cargo/bin`. `web/build.sh` sources `linux/core-env.sh`, so it works from a
  plain shell.
- For native builds, `source linux/core-env.sh` first. Through the same
  symlink, it also finds the OpenBLAS sysroot, which native builds need.

## Build, run, test

```sh
web/build.sh --threads     # web/pkg (single-threaded) + web/pkg-threads (rayon workers)
node web/serve.mjs         # http://localhost:8137/ ; ?threads=0 single-threaded, ?backend=cpu
node web/node-test.mjs in.png kodak_portra_400 kodak_portra_endura 1024 out.ppm
```

`serve.mjs` serves `web/` and `data/` (as `/data/`) with COOP/COEP headers.
It saves what the page POSTs to `/result` in `web/results/` (gitignored).

To automate, open
`/?image=results/x.jpg&backend=gpu|cpu&max=2048&runs=3&name=foo` (add
`&interactive=1` or `&params=<json>` as needed).
Then `web/results/foo.ppm` and `foo.json` hold the image, timings, per-stage
breakdown, memory and adapter. Headless Chrome only gets SwiftShader on this
machine (NVIDIA GTX 1660 SUPER, Wayland). For real WebGPU, run a windowed
Chrome:

```sh
google-chrome-stable --ozone-platform=wayland --window-size=300,300 \
  --user-data-dir=<scratch dir> --enable-unsafe-webgpu --enable-features=Vulkan \
  --ignore-gpu-blocklist "http://localhost:8137/?…"
```

Run `/opt/google/chrome/chrome` directly: the `google-chrome-stable`
wrapper appends the user's `chrome-flags.conf`, whose `--enable-features`
replaces `Vulkan`. Use `--ozone-platform=x11`: on Wayland, Vulkan breaks 2D
canvas uploads and the app's canvas stays empty. With
`--remote-debugging-port=9333` the page can be driven and screenshotted over
the DevTools protocol. Kill test Chrome with
`pgrep -f '^/opt/google/chrome/chrome .*<scratch dir>' | xargs -r kill`, not
`pkill -f`, which matches your own shell. A Chrome left running through many
renders can lose its GPU state and crash the engine; restart it between
benchmark runs.

Firefox 158 works too, with `dom.webgpu.enabled` set in a profile's
`user.js`.

`web/bench.sh <name> '<query>'` runs that windowed Chrome for you and
prints first, warm and whole-frame times and the last frame's stages, e.g.
`web/bench.sh card-1200 'image=results/card.jpg&backend=gpu&max=1200&runs=8'`.
Test inputs live in the gitignored `web/results/`: `card.jpg` (2048 px),
`card-1024.png`, `card-24mp.jpg`, `card-60mp.jpg`, `sony.arw` (24 MP) and
`truncated.arw` (the panic test). Earlier runs' JSON there are the
baselines `docs/web-wasm.md` quotes (`s0-*` before the frame-cost work,
`s3-*` and `f-*` after).

To compare two renders, convert them with `magick x.png x.ppm` and run
`node web/compare.mjs a.ppm b.ppm` (exact, within 1/255, max). The expected agreement is in `docs/web-wasm.md`. The native CLI
renders a reference:

```sh
SPEKTRAFILM_ROOT=$PWD SPEKTRAFILM_BACKEND=wgpu|cpu target/release/spektrafilm process in.png -o out.png \
  --film … --paper … --no-metadata [--iters N]
```

## Code map

- `crates/spektrafilm-wasm/src/lib.rs`, the browser API:
  - `registerData`, `Engine.newCpu`, `Engine.newGpu` (async), `load` (returns
    `[width, height]`) and `render(film, paper, params, maxDim, interactive)`
    (a Promise of `Rendered`);
  - `setInstance` and the panic hook, which broadcasts panics on
    `negs-engine-panic`;
  - `Rendered` carries the RGBA, backend, `elapsedMs`, `stages` and
    `missing`;
  - `memoryBytes`, and `initThreadPool` with the `threads` feature.

  The engine state is an `Rc<RefCell<State>>`, so `render` can return a
  `'static` future. It caches the pipeline (by
  `preview_pipeline_cache_key`), one resampled source per kind (interactive,
  settled), the metering frame and the metered auto-exposure EV.
- `crates/spektrafilm-core/src/data_fs.rs` reads data from disk natively and
  from a registry in the browser. It records misses, which `render` returns,
  and the page fetches them and retries (`web/engine.js`).
- GPU split:
  - `WgpuBackend::start_film_chain` returns a `FilmChainReadback`, and
    `finish` reads it;
  - `ComputeBackend::submit_film_chain` and `PendingFilmChain` expose this;
  - `Pipeline::submit_resident` and `finish_submitted` wrap it.

  The browser awaits a oneshot between submit and finish. Never call a
  blocking GPU method in the browser. Anything the resident chain doesn't
  cover goes to `CpuBackend`.
- Display output: a submit takes a `ChainOutput` (the scan, sRGB bytes, or
  those drawn on the canvas `WgpuBackend::set_canvas` was given), and
  `finish` gives a `ChainResult`. `Pipeline::display_on_gpu` says when the
  bytes equal `preview::display_rgba` of the scan. Shaders:
  `display_rgba8.wgsl` and `display_present.wgsl`. A presented frame reads
  back four bytes as its completion fence: wgpu 24's
  `on_submitted_work_done` panics in browsers.
- Per-frame cost (see `docs/web-wasm.md`, "WebGPU frame cost"):
  `WgpuBackend::create_buffer_init` (never `device.create_buffer_init` in
  the chain: mapped-at-creation buffers cost 0.3 ms each in Chrome),
  `ImagePool`/`ImageSet` (full-image buffers, wasm only), `resident_input`
  (by `FilmChainParams::image_key`) and `tc_lut_buffer`.
  `web/webgpu-costs.html` measures what WebGPU objects cost in a browser.
- `MaybeSendSync` (gpu `lib.rs`) means `ComputeBackend` isn't `Send + Sync`
  on wasm32.
- `spektrafilm_io::load_raw_bytes` does RAW decoding from memory, with
  rawler.
- `vendor/rawler`: rawler 0.8.0 with `web_time::Instant` (see
  `vendor/README.md`), patched in from the workspace `Cargo.toml`.
- Web side:
  - `web/index.html` and `web/app.js`: the app and the automation hook.
    `app.js` keeps one render in flight and only the newest request
    queued, renders an interactive frame while a slider moves (its scale
    adapts to a 90 ms budget, at least 0.25 on WebGPU) and the settled frame
    at device resolution when it stops. The WebGPU engine presents on a
    canvas the page transfers to its worker (`attachCanvas`; a new one per
    engine, since a canvas transfers once); otherwise the page draws the
    bytes. The automation hook (`?image=…`) also saves what the canvas shows
    as `<name>-canvas.ppm`; `?present=0` reads the bytes back instead;
  - `web/engine.js`: render-and-fetch loop inside the worker;
  - `web/worker.js`: the engine (either build) in a worker;
  - `web/build.sh` and `web/serve.mjs`: build and serve. The server maps
    `/catalog.json` to `linux/catalog.json`, the film and paper list.

## Known gotchas

- **rawler is vendored** for its `Instant` calls, which panic on wasm32.
  An upstream PR would let `vendor/rawler` go; ask the user before opening
  one.
- **A panic kills the module** (`RuntimeError: unreachable`). On a rayon
  thread it also hangs the engine. The page restarts the worker on either
  signal and reopens the last photograph.
- **The threaded build** needs:
  - `RUSTC_BOOTSTRAP=1` with `-Zbuild-std`;
  - explicit linker flags: `--shared-memory --import-memory` and the TLS
    exports (rustc doesn't add them);
  - its own target dir, `target/wasm-threads`.

  `wasm-bindgen-rayon`'s worker imports the package by directory URL, hence
  `--out-name index` and the directory-to-`index.js` rule in `serve.mjs`.
  rayon must not run on the main thread.
- **`RUSTFLAGS` replaces `.cargo/config.toml`'s flags.** Keep
  `--cfg getrandom_backend="wasm_js"` in any custom `RUSTFLAGS`.
- **`pkill -f "node serve.mjs"` also matches your own shell.** Kill by port
  instead:
  `kill $(ss -ltnpH 'sport = :8137' | grep -o 'pid=[0-9]*' | cut -d= -f2)`.
- **wasm32 memory is capped at 4 GB.** The GPU path peaks at about 3 GB for
  60 MP. The CPU path can't render past about 30 MP.

## Next steps

`docs/web-mvp-plan.md`: the MVP, in pieces with checkpoints.
