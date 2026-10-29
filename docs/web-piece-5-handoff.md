# Web MVP: handoff for piece 5

For the agent picking up the browser version of Negs at piece 5 of
`docs/web-mvp-plan.md`. Read the plan, `docs/web-wasm.md`,
`docs/web-wasm-handoff.md` (build, serve, tools, gotchas) and
`docs/web-feature-inventory.md` first. This file adds where things stand,
a `/tmp` cleanup to do before anything else, and what piece 5 touches.

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
| **5. Colour management and output colour space** | | **next** |
| 6. Resident scan, zoom, true-resolution preview | | 🛑 after it |
| 7. Placeholder while developing | | Stage 1 🛑 after it |

There is no 🛑 after piece 5: commit it and go on to piece 6. Stop at the
🛑 after piece 6.

Still open from piece 3: nobody has opened the page on a real iPad or
iPhone (ufw blocks ports 8137 and 8443 from the LAN; the owner's call), and
the 24 MP ARW takes 954 MiB in the engine at a phone's preview size
(piece 6).

## First: clean up `/tmp`

Piece 4 hit `Disk quota exceeded` partway through: `/tmp` is a 7.8 GB
tmpfs, and `/tmp/claude-1000` had reached 5.4 GB. Its symptom is quiet. A
`cdp.mjs` run exits with code 13 and prints nothing, because Chrome can't
write its profile. Almost all of the space is Chrome profiles from earlier
sessions' benchmarks and page checks.

The owner asked for this cleanup. What was there at the end of piece 4
(under `/tmp/claude-1000/-home-noah-Work-Negs/`):

| Folder | Size | What |
|---|---|---|
| `6143adf9-…` | 4.1 GB | piece 3's scratch: 60 `cdp-*` Chrome profiles, `p3-*` and `p2-*` screenshots |
| `ba7e0e5e-…` | 477 MB | piece 4's session folder (`tasks/` holds its background-task output); earlier renders |
| `f4cba253-…` | 365 MB | `adw16.meson`, `demo.jpg`, `dl/`: looks like other Negs work (Linux), not the web |
| `1ecb165a-…` | 213 MB | earlier web session: `chrome/` profile, ARW renders |
| `2ab84e89-…` | 182 MB | earlier web session: `chrome/`, `ff/` profiles, SIMD comparison renders |
| `798deb0f-…` | 128 MB | piece 4's scratchpad: CLI before/after renders, helper scripts |
| `56b14bc1-…` | 0 | empty |

Outside it, `/tmp/negs-upstream-ui-review` (89 MB) belongs to other work.

Do this:

1. Make sure nothing is using a folder before you delete it:
   `lsof +D <folder> 2>/dev/null | head`, and no test Chrome running
   (`pgrep -af '/opt/google/chrome/chrome' | grep claude-1000`). Your own
   session's folder is the one your scratchpad is in; leave it.
2. Delete the Chrome profiles everywhere: every `cdp-*`, `chrome/`, `ff/`
   and `bench/` under those folders. They're reproducible and are the bulk.
3. Delete the whole folders of the finished web sessions: `6143adf9-…`,
   `1ecb165a-…`, `2ab84e89-…`, `798deb0f-…`, `ba7e0e5e-…` and the empty
   `56b14bc1-…`. Nothing in them is needed: the baselines the docs quote
   are in `web/results/` in the worktree, not in `/tmp`.
4. Leave `f4cba253-…` (apart from any Chrome profiles in it) and
   `/tmp/negs-upstream-ui-review` alone. They look like other work, so ask
   the owner before removing them.
5. Check: `du -sh /tmp/claude-1000` and `df -h /tmp`. Report what you
   freed.

Then stop it from filling up again:

- `web/cdp.mjs` makes a profile with `mkdtempSync` and never removes it,
  and `web/bench.sh` defaults to `mktemp -d` and leaves its profile too.
  Make both delete the profile they made when Chrome exits (only one they
  created, not a `$BENCH_PROFILE` or `$CDP_TMP` directory the caller
  passed). Then piece 4's `rm -rf <scratch>/bench/*` after every run isn't
  needed. Commit that with a line in the handoff doc.
  **Done:** both now delete the profile they made once Chrome has exited
  (`cdp.mjs` also on a failed run). The cleanup freed 5.0 GB (`/tmp` went
  from 5.6 GB used to 586 MB). `ba7e0e5e-…/tasks` and `56b14bc1-…` were
  left: live Claude sessions hold them. `f4cba253-…` and
  `/tmp/negs-upstream-ui-review` are untouched, waiting on the owner.
- Keep large renders (the helper's `.rgba` previews are 17 MB each) only as
  long as a comparison needs them.

## Done before piece 5: correctness

`docs/web-wasm.md` item 13: the browser matches native wgpu and the f64
reference on 63 cases (films, papers, scans, settings, the ARW), and a GPU
memory leak that blacked out a long session's 24 MP frame is fixed. Piece 5's
table therefore goes in item 14, not 13. Rerun
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

**The GPU display pass.**

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

- The dev server runs at `node web/serve.mjs` (port 8137); piece 4 left it
  running, and `serve.mjs --https` on 8443 from piece 3. Restart after
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
  (phone layout). Rerun the last two after UI changes.
- `negs` in the page: `state()`, `edit(path, value)`, `renders`, `idle()`,
  `open(url)`, `read(maxDim)` (render, read back the bytes) and
  `balance(...)`.
- Messages: `showMessage(text, error, ms)` with `ms` stays up through the
  renders that follow and hides itself after `ms`; without it, the next
  finished render hides it. A download-progress message wants the first.
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
