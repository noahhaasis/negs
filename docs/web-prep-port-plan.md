# Plan: camera brightness, the Prep tab and stock previews on the web

**Status (2026-11-04):** plan only, nothing built. Main at `c92b539`. Template:
[the Windows port plan](windows-prep-port-plan.md), which shipped the same Mac features on
Windows.

Three macOS features to bring to the browser app (`web/`), each **only if it stays fast
enough on a phone and doesn't get the tab killed**:

1. **Match camera brightness**: solve `prep.exposure_ev` against the RAW's embedded camera
   JPEG (or a JPEG/PNG's own pixels). Spec: [exposure placement](exposure-placement.md).
2. **Prep | Lab | Output tabs with a Prep tab**: Exposure, Tone, Color, Detail before the
   film. Spec: [prep](prep.md), ticket 26.
3. **Film and paper previews**: the foldout grids of the photograph through every stock.
   Spec: [stock previews](stock-previews.md).

## Where the web is today

| | macOS | Web |
|---|---|---|
| Prep params in the engine | yes | yes (shared engine; neutral, no controls) |
| Prepared-frame cache (`Pipeline::with_prepared_frame`) | helper keeps it | **no**: every frame with Prep on would redo Prep on the CPU |
| GPU upload with Prep on | re-uploads the prepared frame (no source key) | same code, so same re-upload |
| `place_exposure` | helper command | no wasm binding |
| Reference image | `CameraBrightnessReference.swift` (ImageIO) | none; `web/placeholder.js` already finds and decodes the embedded JPEGs |
| Stock thumbnails | helper `thumbnail` command, LRU of 40 pipelines | none |
| Inspector | scopes, stocks pinned, Prep \| Lab \| Output | Lab \| Post panes on desktop; a bottom sheet with one tab per section on phones |
| Post Tone/Saturation | removed (round 2, use Prep) | present, and cheap: the display pass re-runs on the kept scan, no film render |
| Histogram, undo, session restore | yes | no (out of scope here) |

Two facts that shape the plan:

- **Any non-zero `prep.exposure_ev` makes Prep active** (`PrepParams::is_identity` compares
  every field). So Match camera brightness, on by default for RAWs on the Mac, puts nearly
  every RAW through the CPU Prep path and a per-frame upload, even with all Prep sliders at
  zero. Feature 1 costs what Prep's exposure pass costs.
- **Prep is CPU work; Post on the web is GPU work.** Natively Prep at 24 MP is ≈ 225 ms
  settled / 28 ms per drag frame on 10 cores, almost all `powf`/`atan2`/`cos`/`sin`. wasm
  uses software transcendentals and the iPhone gets 4 threads. The guided filter's radius is
  1 % of the long side, so the preview at viewport size does look like the export.

Phone limits we already know (iPhone 13 Pro, `iPhone14,3`, 6 GB, iOS 26.6):
the WebContent process is killed at **2.2–2.7 GiB** footprint, WebGPU memory counts against
it, and wasm memory never shrinks, so every peak is permanent for the tab. Current peaks:
**1372 MiB** (24 MP RAW, whole sequence) and **1724 MiB** (60 MP Leica, bounded).
Sources: [iPhone memory handoff](iphone-memory-handoff.md),
[stability follow-up](iphone-ipad-stability-followup.md).

## Decisions and Phase 0 results (2026-11-04)

Noah: remove Post's tone controls and put a Prep tab first (layout refined later); take
the GPU path for Prep and the brightness match; the grain difference of the GPU paths is
approved (comparison page: worktree `web-preview-size`, `target/grain-compare/fuji.html`).

Measured with `web/checks/preview-bench.js` (worktree `web-preview-size`, which also has
`feature/prep-gpu` merged and measurement switches for both GPU paths), Fuji RAF 6240×4160,
Portra 400 → Endura. Settled frame times; drag frames cost the same at the same size.

| Frame | Mac Chrome (M1 Pro) | iPhone 13 Pro, Safari 26.6 | iPhone footprint (12 MP copy) |
|---|---|---|---|
| 1200 (fitted on the phone) | 35 ms | 136 ms | 430–610 MiB |
| 1200, exposure-only Prep (CPU) / folded | 40 / 32 ms | 202 / 138 ms | |
| 1200, full Prep (CPU) / GPU core | 75 / – ms | 347 / 408 ms | |
| 2048 | 85 ms | 398 ms | 720–850 MiB |
| 2048, exposure-only Prep (CPU) / folded | 105 / 84 ms | 701 / 401 ms | |
| 2048, full Prep (CPU) / GPU core | 207 / – ms | 1026 / 1109 ms | |
| 3072 | 178 ms | 920 ms | 1.21–1.47 GiB |
| 4096 | 312 ms | **tab killed** at ≥ 1622 MiB | |
| Film change at 1200 | 155 ms | 215–315 ms | |

- The web renders at the view's size, so a 2048/4096 preview cap buys little: nothing on the
  phone's fitted view, only big desktop windows and zoom.
- A 2048 px working copy instead of 12 MP: same frame times; −110 MiB steady, −210 MiB at
  open (1.09 vs 1.30 GiB, the decode dominates). Not worth losing zoom detail by itself.
- **Zoom on the phone is the memory risk:** 4096 frames after Prep frames killed the tab.
  The touch cap (12 MP ≈ 4240 px) is too high once Prep is in.
- **Exposure fold (point 6):** free on both; CPU exposure-only Prep costs +48 % (fitted) /
  +76 % (2048) on the phone, mostly the per-frame CPU pass and re-upload. Pixels: grain off
  ≤ 1 code on 0.003 %; grain on 0.05 % of values, isolated specks up to 18 codes, grain
  strength per tone within ±0.04 %.
- **GPU Prep core:** −32 % on the Mac at full size (2135 → 1444 ms), but +5–18 % slower on
  the phone: its CPU side (encode + guided base, `submit` 285 vs 187 ms at 1 MP) costs more
  than the CPU core it replaces there. Needs profiling before it helps phones.
- wasm has no prepared-frame cache, so every frame with Prep on redoes Prep (all rows above).

## Phase 0: measure before building UI (go / no-go)

Everything here works through the existing URL harness (`?image=&params=<json>&runs=&interactive=1`,
results to `web/results/`) and the device runner, with small engine prototypes where noted.
Same build and fixtures for every row. On the iPhone, a fresh tab per run, charging, Low
Power Mode off, footprint sampled with `web/checks/footprint.py`.

Fixtures: the 24 MP control RAW, the Fuji RAF (6240 × 4160), the 60 MP Leica (bounded
route), `card.jpg`, and one letterboxed-preview RAW (Leica M8 or M Typ 240) if we have one.

**0a. Prep cost.** No code needed beyond `params`:

| Case | Desktop Chrome | Desktop Safari | iPhone |
|---|---|---|---|
| Settled frame, Prep neutral (baseline) | | | |
| Settled, `prep.exposure_ev` = 0.7 only | | | |
| Settled, Highlights −50 / Midtone Detail +40 | | | |
| Drag (`interactive=1`), same three cases | | | |
| Lab edit (print exposure) with Prep on | | | |
| Full-size export, Prep off / on (time + footprint peak) | | | |
| 60 MP Leica bounded export with Prep on (footprint peak) | | | |

Then prototype the prepared-frame cache in wasm (step 1 below) and rerun the Lab-edit row.

**0b. Brightness solve.** Prototype a wasm `place_exposure` on the CPU backend (the proxy is
≤ 256 px; the solve runs ~13 bracket + up to 8 refine + highlight back-off renders on one
pipeline). Time it on the three browsers; native is 120–280 ms.

**0c. Thumbnails.** Prototype a wasm `thumbnail` (step 3 below) and time per tile: spectral
pipeline calibration (the web measured 300 ms per film change before calibration was
parallelised; native is now ~45 ms), the 320 px render on GPU and on CPU, and memory per
cached pipeline in wasm and on the GPU. Also: does a thumbnail render on the GPU evict the
main render's pooled buffers (the pool drops same-label buffers before allocating new
ones)?

**Proposed gates** (Noah to adjust). Each feature ships on phones only if it passes there;
desktop browsers are expected to pass easily.

| Feature | Speed on the iPhone | Memory on the iPhone |
|---|---|---|
| Prep | Prep drag frame ≤ 1.5× an exposure drag and ≤ ~100 ms at fitted size; settled ≤ +250 ms; export ≤ +30 % | ≤ +150 MiB peak over baseline; Leica bounded sequence ≤ 2.0 GiB; 0 reloads in 3 fresh-tab runs |
| Match | solve ≤ 1.5 s after the first frame, never delays it | no measurable peak (reference ≤ 512 px) |
| Previews | all 33 tiles ≤ ~10 s on first open, ≤ ~3 s after an edit; first drag frame delayed by ≤ one tile | ≤ +100 MiB with both grids open; nothing retained after closing/turning off beyond wasm's high-water mark |

Report Phase 0 as a table in this file, then decide per feature before Phase 2.

## Phase 1: engine and wasm (no UI)

1. **Prepared frame in wasm.** Give the wasm `Engine` a `PreparedFrame` like the helper's, so
   Lab edits with Prep on skip Prep (bit-identical; the key already covers everything Prep
   reads). Check whether the GPU backend can keep the prepared frame resident under a key
   derived from `PreparedKey`, instead of re-uploading it every frame. Same maths, so no
   output change on any platform.
2. **Prep memory on export.** With Prep on, `prepare_input_from` makes a second full-size
   f32 frame (24 MP: ~290 MB; 60 MP: ~720 MB) next to the source. Apply Prep in place on the
   copy the export already owns, and make it work inside the bounded/banded export route
   (the guided base needs the whole frame first, then the per-pixel core can run in bands).
   Same order of operations, so bit-identical; test it against the current path.
3. **Shared brightness reference.** Move the platform-neutral half of
   `windows/src/brightness.rs` (declared colour space from EXIF/maker notes, Adobe RGB →
   sRGB, `trimming_bars`, `same_shape`, the failure-message mapping) into `spektrafilm-io`, so
   Windows, Linux and the web share it. Decode the chosen embedded JPEG in Rust (`image`
   crate, with JPEG DCT downscaling so a full-size embedded JPEG never decodes at 24 MP):
   pick the smallest preview with long side ≥ 160 px, like ImageIO's thumbnail path does.
   Same result on Chrome, Safari and Firefox, unlike canvas colour handling.
   For JPEG/PNG/TIFF ("Match original brightness") use the source the engine already
   decoded, reduced to 512 px and converted to linear sRGB, instead of decoding the file a
   second time; check it gives the Mac's EVs.
4. **wasm `place_exposure(film, paper, params, reference)`** on the kept source (the bounded
   route's kept source is ≥ the 256 px proxy, so fine). CPU backend first; an async GPU
   variant only if 0b says it's too slow.
5. **Shared thumbnail code.** Move the helper's `thumbnail` / `release_thumbnails` logic
   (downsampled source, crop, LRU of calibrated pipelines keyed like the preview pipeline
   cache) out of `spektrafilm-native/src/main.rs` into a module both use, then bind it in
   wasm. Its LRU size and long side become parameters: the phone gets a smaller LRU if 0c
   shows memory pressure. Render thumbnails on the backend 0c picks, without touching the
   main render's pool or retained output.

Tests: wasm vs native EV on the RAW samples from
[raw-compatibility](raw-compatibility-2026-11-01.md) within 0.05 EV of the Mac (letterboxed
ones included); thumbnail pixels vs the helper's within 1/255; `web/checks/` additions for
each binding (node, CPU engine), like `raw-thumbnail.test.mjs`.

## Phase 2: the tabs and Prep (UI)

Port behaviour, not the Mac look; follow the web's own conventions (`controls.js` sections,
`remember()` for prefs, the sheet on phones).

6. **Desktop sidebar:** film and paper pinned above a **Prep | Lab | Output** switch
   (today's `#panes`), Reset Adjustments pinned below. Prep opens first. Remembered
   `pane` "post" maps to "output".
7. **Prep sections** (new `PREP_SECTIONS` in `controls.js`): RAW White Balance (moves from
   Camera), Brightness (Match switch, Exposure as an offset on the match, the note line,
   Auto exposure and Metering with the Mac's show rule), Tone (Highlights −180…100,
   Shadows, Contrast, Lift, Gain), Color (Color Boost, Saturation), Detail (Midtone Detail).
   Camera exposure is renamed **Negative exposure**.
8. **Output:** Border (and anything else the web adds later, e.g. export size). Post
   Tone/Saturation: see decision 1.
9. **Phones:** the sheet keeps a tab per section; new order Stock · Prep · Camera · Film ·
   Print · Scan · Output (decision 2). The tab strip already scrolls.
10. **Camera brightness, model side** (port of `EditorModel` §Camera brightness, as Windows
    did): match state `{ ev, inputs, highlightLimited }` per photograph; `matchInputs`
    nulls the same paths as the Mac; one solve at a time, only after the settled preview,
    queued behind main renders in the worker; a result applies only if its inputs are still
    current; failure notes for the photograph vs until the next edit; export waits for a
    pending match. Setting *Match new RAWs to camera brightness* (decision 4).
11. **Tutorial:** `web/tutorial.js` steps and `checks/tutorial-layouts.mjs` reference the
    sections and panes; update them with the tabs.

If Prep fails the phone gate but the match passes, the match still ships: exposure-only Prep
is one multiply pass. It then sits in the Camera section with Auto exposure.

## Phase 3: film and paper previews (UI)

12. Foldouts for Film and Paper in the pinned stock area, collapsed by default, 2-column
    grid grouped like the menus, current stock outlined, tile = the edit with that stock
    chosen (film tiles take the film's paired paper and preset resets, as `chooseFilm`).
13. Scheduling in `app.js`: tiles only while a grid is open and on screen; one thumbnail
    request at a time, only when no render is running or pending and no slider is held;
    tiles whose pipeline is cached first. Dim stale tiles (0.55), blank on a new photograph,
    no retry after a failure until the next edit.
14. Setting *Film and paper previews*, plain menus when off, `release_thumbnails` on
    turning off. Default on desktop; on phones per decision 5.
15. Later, not in this plan: preset previews in `preset-ui.js`, favourite stocks.

## Phase 4: iPhone validation

Add phases to `web/checks/ipad.mjs` (run with `SAFARI_DEVICE_TYPE=iPhone`,
`SAFARI_TEST_DIR=target/iphone-tests`, one phase per fresh tab, footprint sampled throughout):

- `prep`: every Prep slider to both ends and back; 20 s of rapid Prep drags; Lab edits with
  Prep on; zoom to the touch cap and drag Prep there; full-size export with Prep on; edit
  after export.
- `match`: open the RAW with matching on; five film changes and a paper change (each
  re-solves); a JPEG; the letterboxed RAW; a RAW without a usable preview (the note shows,
  nothing else breaks); export while a solve is pending.
- `stocks`: open both grids and wait for all tiles; drag a slider while tiles fill; edit and
  wait for the refresh; switch photo; close; turn previews off; repeat three times; export
  with the grids open.
- Re-run the existing `smoke`, `zoom`, `heavy`, `export`, `after-export` and the 60 MP
  Leica bounded sequence on the final build, and compare with the pre-feature baseline
  from the same day.

A run fails on any reload, worker replacement, panic or blank canvas, even if recovery
works (rules in [Apple device testing](apple-device-testing.md)). Record per step: frame
times, tile fill times, solve time, WebContent and GPU-process footprint peaks, wasm memory,
backend and thread count, build hashes. Keep forced-CPU and default-GPU results separate.
Also run once on the iPad Air 5 and in desktop Chrome, Safari and Firefox.

Write the results into this file and `docs/web-wasm.md`, and the "Open" lines of
`docs/prep.md` and `docs/stock-previews.md`.

## Order and branches

One worktree, `.claude/worktrees/web-prep-port` from main. Phase 0 first and report back.
Then Phase 1 steps 1–4 → Phase 2 → device run → Phase 1 step 5 → Phase 3 → device run.
Step 3 (shared reference) also benefits Linux, which still lacks the match.

## Decisions for Noah

1. **Post Tone/Saturation on the web.** The Mac removed them for Prep. On the web Post is
   the cheap one (no film render) and Prep the expensive one. Options: follow the Mac (one
   look everywhere), or keep Post on the web. Recommend: follow the Mac if Prep passes the
   phone gate, keep Post otherwise.
2. **Phone tab order.** Recommend Stock · Prep · Camera · Film · Print · Scan · Output.
3. **If Prep is too slow on phones.** Options: (a) desktop browsers only, with the match
   still on phones; (b) GPU Prep from `feature/prep-gpu` for the web: fast, but rounding-level
   differences (and a few changed grain specks) against the CPU, which you declined for
   speed before; (c) Prep on phones with coarser drag frames. Recommend (a), then
   revisit (b) for all platforms together.
4. **Match new RAWs on by default on the web,** as on the Mac? It adds one solve after
   every RAW open. Recommend yes if 0b passes.
5. **Previews on phones.** On by default if 0c passes; otherwise off on touch devices with
   the setting to turn them on.
6. **Folding exposure-only Prep into the GPU front-pass matrix** (like auto exposure ×
   exposure compensation) would make the match free on every platform, but moves those
   renders by float rounding. Only worth asking if Phase 0 shows the exposure-only pass
   matters.
