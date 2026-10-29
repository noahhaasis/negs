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
web/build.sh --threads     # web/pkg (single-threaded) + web/pkg-threads (rayon workers) + web/catalog.json
node web/serve.mjs         # http://localhost:8137/ ; ?threads=0 single-threaded, ?backend=cpu
node web/node-test.mjs in.png kodak_portra_400 kodak_portra_endura 1024 out.ppm
```

`serve.mjs` serves `web/` and `data/` (as `/data/`) with COOP/COEP headers.
It saves what the page POSTs to `/result` in `web/results/` (gitignored).
Restart it after editing it: a running server keeps its old routes.

To automate, open
`/?image=results/x.jpg&backend=gpu|cpu&max=2048&runs=3&name=foo` (add
`&interactive=1`, `&film=<id>`, `&paper=<id>` or `&params=<json>` as
needed; `params` merges into the full edit, as a preset over the defaults
would).
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

### On an iPad or iPhone

WebGPU and `SharedArrayBuffer` need a secure context, and `http://<LAN IP>`
isn't one. `web/dev-cert.sh` makes a local certificate authority (name
constraints limit it to localhost, `*.local` and private addresses) and a
certificate for this machine's names and current LAN addresses, in
`~/.local/share/negs-dev-certs` (outside `web/`, so nothing serves the
keys). `node web/serve.mjs --https` then serves on port 8443, COOP/COEP
included. Once per device:

1. In Safari, open `http://<LAN IP>:8137/ca.crt` (the plain server hands
   out the authority's certificate) and allow the profile download.
2. Settings → General → VPN & Device Management: install the "Negs dev
   server" profile.
3. Settings → General → About → Certificate Trust Settings: turn on full
   trust for it.
4. Open `https://<LAN IP>:8443/`.

Run `web/dev-cert.sh` again when the LAN address changes; the authority
stays, so the device keeps trusting it. This machine's ufw firewall blocks
both ports from the network until they're allowed (`sudo ufw allow from
192.168.178.0/24 to any port 8137,8443 proto tcp`). To test the TLS server
here, `CHROME_FLAGS=--ignore-certificate-errors node web/cdp.mjs
https://localhost:8443/ -`.

`web/bench.sh <name> '<query>'` runs that windowed Chrome for you and
prints first, warm and whole-frame times and the last frame's stages, e.g.
`web/bench.sh card-1200 'image=results/card.jpg&backend=gpu&max=1200&runs=8'`.
`$BENCH_PAGE` points it at another page than `index.html` (for example a
copy of the previous `app.js` served as `old.html`, for before/after
renders). Test inputs live in the gitignored `web/results/`: `card.jpg`
(2752×1536),
`card-1024.png`, `card-24mp.jpg`, `card-60mp.jpg`, `sony.arw` (24 MP) and
`truncated.arw` (the panic test). Earlier runs' JSON there are the
baselines `docs/web-wasm.md` quotes (`s0-*` before the frame-cost work,
`s3-*` and `f-*` after).

`web/cdp.mjs <url> <script.js>` opens the page in the same windowed Chrome,
prints console messages, Chrome's log (where WebGPU errors land) and
exceptions, and runs a script in the page
(`SHOT=x.png` saves a screenshot, `VIEW=390x844@3m` emulates a touch device's
viewport, with `pointer: coarse`). A script ending in `.mjs` runs in Node
instead, as `export default async ({ send, evaluate }) => result`, for
input the page can't make itself (`Input.dispatchTouchEvent`,
`Input.dispatchMouseEvent`). The page exposes `negs` for such checks:
`state()` (a copy of the edit), `edit(path, value)`, `renders` (each
finished frame's size, time, stages and whether it presented), `idle()`,
`open(url)` (opens a photograph as the Open button does), `read(maxDim)`
(the edit rendered and read back as bytes), `balance(film, paper,
params)` (the engine's Balance Print, without changing the edit), `view`
(the view: `zoom()`, `fit()`, `actualSize(zoom)`, `rect()`), `views` (each
display pass's time), `renderSide()`, `photo()`, `placeholder()` (the
placeholder showing, its size and whether the page's canvas shows it),
`timeline` (each opening's `open`, `placeholder`, `decoded` and `frame`
times), `badge()` (the badge's text, or false), `openSample()`,
`shown()` (where the
engine drew its frame), and Show Original's `original(on)` (hold it or
let go), `showingOriginal()`, `originalMade()` (its size, time and whether
the engine keeps it), `prepareOriginal()` and `readOriginal(maxDim)` (its
bytes, shown nowhere). `views` also counts the page's own paint as a
display pass when the engine doesn't present.
`web/checks/controls.js` moves every Lab control and reports its frame (a
`pipeline` stage is a rebuild, `meter` a re-meter);
`web/checks/enlarger.js` moves the Enlarger's controls a few steps each;
`web/checks/updated.js` saves renders of enlarger edits made in one page,
to compare with the same params rendered fresh (`?params=`; its header
says how); `web/checks/balance.js` runs Balance Print on the cases in
`web/checks/balance-cases.json` and presses the button (send the same
cases to the helper's `balance_print` to compare);
`web/checks/correctness.sh [case,…]` renders
`web/checks/correctness-cases.json` in the page (`correctness.js`, with
`negs.start(film, paper, edits)`: Reset Adjustments, the pickers' film and
paper, then the edits) and the same params with the f64 CPU CLI and native
wgpu, and compares them (`correctness.py`: 8-bit agreement, CIEDE2000, and
for grain a 33×33 average; it needs `uv`, and writes to
`target/web-correctness`); `web/checks/gpu-memory.mjs` logs Chrome's GPU
memory over a long session (`nvidia-smi`);
`web/checks/inspector.js` tests the inspector's wiring;
`VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/
web/checks/touch.mjs 2000` tests the phone layout by touch;
`web/checks/zoom.mjs` zooms and pans (fit, Actual Size, wheel pans with
their display passes' times, the canvas against a render read back, 25%,
Ctrl-wheel, a pinch with `VIEW=…m`, Fit) and logs GPU memory;
`web/checks/placeholder.js` tests the placeholder's orientation handling
and a lossy DNG's preview, and times opening the ARW and the sample;
`web/checks/original.mjs` tests Show Original by mouse, keyboard and (with
`VIEW=…m`) touch: each switch's display passes and no render, the view
kept, the canvas against the original's bytes, and with
`HELPER=results/helper-….rgba` (its header says how to make one) the full
size against the helper's `original`. Chrome fails
`blob()` on a large response in these test profiles; read an
`arrayBuffer()`.

To compare two renders, convert them with `magick x.png x.ppm` and run
`node web/compare.mjs a.ppm b.ppm` (exact, within 1/255, max). The expected agreement is in `docs/web-wasm.md`. The native CLI
renders a reference (`source linux/core-env.sh` first, or it can't find
`libopenblas.so.0`):

```sh
SPEKTRAFILM_ROOT=$PWD SPEKTRAFILM_BACKEND=wgpu|cpu target/release/spektrafilm process in.png -o out.png \
  --film … --paper … --no-metadata [--iters N]
```

## Code map

- `crates/spektrafilm-wasm/src/lib.rs`, the browser API:
  - `registerData`, `Engine.newCpu`, `Engine.newGpu` (async), `load` (returns
    `[width, height]`) and `render(film, paper, params, maxDim, interactive)`
    (a Promise of `Rendered`);
  - `balancePrint(film, paper, params)`: the helper's `balance_print`, as
    `Balanced` (`mFilterShift`, `yFilterShift`, `elapsedMs`, `missing`);
  - `original(params, maxDim, present)`: the helper's `original` (the
    source before any film) at the settled frame's size, kept on the canvas
    (from the resident input on the GPU for an sRGB display) or returned as
    a `Rendered`'s bytes; `show(…, original)` draws it in the developed
    frame's place (`docs/web-wasm.md` item 16);
  - `setInstance` and the panic hook, which broadcasts panics on
    `negs-engine-panic`;
  - `Rendered` carries the RGBA, backend, `elapsedMs`, `stages` and
    `missing`;
  - `memoryBytes`, and `initThreadPool` with the `threads` feature.

  The engine state is an `Rc<RefCell<State>>`, so `render` can return a
  `'static` future. It caches the pipeline (by
  `preview_pipeline_cache_key`), one resampled source per kind (interactive,
  settled), the metering frame and the metered auto-exposure EV. The key
  leaves out what `Pipeline::with_params` follows: the enlarger's shifts,
  preflash and exposure normalization (it re-filters the enlarger), and
  exposure compensation. Anything new a pipeline's build reads must go in
  the key, or be redone in `with_params`.
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
- The canvas (`docs/web-wasm.md` item 14, wasm only in `wgpu_backend.rs`):
  `Canvas` holds the surface, `display_present.wgsl`'s three pipelines
  (`make_shown` writes a frame's bytes into the shown texture, `fs_mip`
  makes its mip levels, `fs_view` draws it), the page's `View` and the
  last two `Shown` textures. A presented chain and `present_rgba` both end
  in `encode_view`; `show(view, on_drawn)` draws it again alone (a texel
  copied to a 4-byte buffer is its fence). `View::place` decides 1:1 or
  sampled; `paint` in `app.js` makes the same decision for the page's own
  canvas, so change both together. `image_limits` gives the page its caps;
  `forget_photograph` frees the shown textures, the original and inputs on
  `load`. `Canvas.original` is Show Original's image (`set_original`
  from bytes, `set_original_from_input` from a resident input through
  `make_shown`); a `View` with `original` draws it (`drawn`) in place of
  the shown image, and `trim_shown` counts it in the shown images' budget.
- GPU memory in browsers (`docs/web-wasm.md` item 13): a dropped
  `wgpu::Buffer` keeps its memory until the garbage collector takes its
  JavaScript object. A buffer made for one render goes through
  `WgpuBackend::create_buffer_init` or `transient` (destroyed once the
  chain is submitted), the image pool, or a cache that calls `release`
  when it lets one go. Never leave a large one to `Drop`.
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
- `spektrafilm_core::catalog`: the stock catalog (profiles with their
  pairing and resolved presets, `defaults`, `nullable_paths`). The native
  helper's `catalog` adds its protocol fields to it; the wasm crate's
  `catalog(stocks)` exports it, and `web/make-catalog.mjs` writes
  `web/catalog.json` (gitignored) with it at build time.
- `vendor/rawler`: rawler 0.8.0 with `web_time::Instant` (see
  `vendor/README.md`), patched in from the workspace `Cargo.toml`.
- Web side:
  - `web/index.html` and `web/app.js`: the app and the automation hook.
    `state` is the edit: `film`, `paper` and a full `RuntimeParams`
    (`params`) from the catalog's defaults, sent whole on every render.
    `chooseFilm`, `choosePaper` and `pairFilm` follow macOS
    `EditorModel`; `PRESET_PATHS` is macOS `Stock.presetPaths`. Controls
    read the state through `syncs`, run by `sync()` after any change,
    and write it through `binding(control)`;
  - `web/controls.js`: the Lab controls (macOS `adjustmentGroups`), option
    titles, and desktop's value snapping and formatting;
  - the touch layout: `index.html`'s `pointer: coarse` and sheet media
    queries (the sheet's query is also `SHEET` in `app.js`), `buildSheet`
    (tabs, the grabber's snap points), `touchSlider` (slider tracks on
    touch screens) and `remembered`/`remember` (per-viewer state in
    `localStorage`).
    `app.js` keeps one render in flight and only the newest request
    queued, renders an interactive frame while a slider moves (its scale
    adapts to a 90 ms budget, at least 0.25 on WebGPU) and the settled frame
    when it stops, at `renderSide()`: the device pixels the view covers,
    within `MAX_PIXELS`, the engine's `limits` and, on the CPU,
    `CPU_PIXELS`. The WebGPU engine presents on a canvas the page transfers
    to its worker (`attachCanvas`; a new one per engine, since a canvas
    transfers once); otherwise the page draws the bytes (`paint`, from an
    `ImageBitmap` in `frame`). `showView` sends the view (`sendView`: one
    `show` in flight, the newest next; a render sends it first if it
    changed), and `viewSettled` renders when the settled view needs more
    pixels. The automation hook (`?image=…`) shows its frame 1:1, compares
    the canvas with the bytes and saves what the canvas shows as
    `<name>-canvas.ppm`; `?present=0` reads the bytes back instead;
  - `web/view.js`: the view (fit, zoom, pan and their gestures), in the
    stage's device pixels; `resize` keeps a placeholder's place when the
    photograph's real size arrives;
  - `web/placeholder.js`: what shows while a photograph opens (a RAW's
    camera preview, or the browser's decode), oriented as Linux
    `photo.rs`. `open(name, data, { early, quiet })` in `app.js` shows it
    (`showEarly`) on the page's canvas and drops it at the first frame
    (`draw`), or restores what it covered if the photograph fails
    (`dropEarly(true)`); `loading` holds renders and the first frame's
    swap until the engine has the new photograph. `setBadge` is the
    "Developing" badge; Show Original's badge shows over it
    (`originalBadge`, `showBadge`), so neither clears the other;
  - Show Original (its section in `app.js`): `prepareOriginal` (on hover,
    and when the settled frame's size changes while held), `holdOriginal`,
    `updateOriginal` (the badge, the button, and `showingOriginal`, which
    `sendView` and `paint` read), `forgetOriginal` on a new photograph or
    engine; the button's pointer and `\` handlers are in the wiring;
  - the sample: the empty screen's "Try a sample photo" runs
    `openSample`, which downloads `web/sample/DSC_6377.dng` with progress
    (`download`); `web/sample/make-dng.py` makes that lossy DNG from the
    NEF (not in the repository; the owner has it). `openFile` (the Open
    button, drops, `negs.open`) cancels a download in progress;
  - `web/engine.js`: render-and-fetch loop inside the worker, and the
    same for Balance Print;
  - `controls.js` kind `action` is a command button; `actions` in `app.js`
    runs it and says when it's enabled (Balance Print: `balancePrint`).
    `showMessage(text, error, ms)` with `ms` hides itself then and survives
    the renders that follow (`pump` hides other messages after a render);
  - `web/worker.js`: the engine (either build) in a worker;
  - `web/build.sh` and `web/serve.mjs`: build and serve;
  - `web/make-catalog.mjs`, `web/bench.sh`, `web/cdp.mjs` and
    `web/compare.mjs`: the catalog, benchmarks, page driving and image
    comparison.

## Known gotchas

- **WGSL reserves more words than you'd think**: `smooth` (a sampler's
  name) made the whole display module invalid, and a frame then presents
  nothing with no error but in Chrome's log (`cdp.mjs` prints it).
- **`globalThis.status` is `window.status`**, a string: a check's helper of
  that name silently becomes text.
- **Chrome's device emulation** (`VIEW=…@3`) reports the device-pixel
  content box in CSS pixels; `view.js` trusts it only when it agrees with
  `devicePixelRatio`.
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
