# Film and paper previews

Branch `stock-previews`. Film and paper previews: the Linux app is done and is the
reference; macOS is done too ([handoff and results](handoff-stock-previews-macos.md#macos-results)).
[Preset previews](#preset-previews-macos-linux-and-windows-done): macOS, Linux and Windows.
Windows did both on 2026-11-02 ([results](handoff-stock-previews-windows.md#windows-results)).
The experiment hooks are gone on all three platforms ([finishing the feature](handoff-stock-previews-windows.md#finishing-the-feature-last-agent)).

## Behaviour (all platforms)

- In Film & Paper, **Film** and **Paper** are foldouts rather than drop-down menus. They are
  **collapsed by default**. The collapsed row shows the title on the left and the current
  stock's name next to the expand arrow.
- An open foldout shows a **two-column grid**, grouped under the same headings as the old
  menu (Color Negative, Color Slide, Motion Picture, Black & White for films; Color Paper,
  Motion Picture Print, Black & White for papers), in catalog order. Each tile is a
  thumbnail of the photograph with the stock name below it, ellipsized, with the full name
  as a tooltip. The current stock's tile is outlined in the accent colour. Clicking a tile
  does exactly what choosing that stock from the menu did, as one undo step
  ("Change Film" / "Change Paper"). Noah approved the tile size: about 92 pt tall, two
  columns filling a ~340 pt inspector.
- What each tile shows: the current edit **with that stock chosen**. For a film tile, that
  means the film's own paired paper, grain/DIR/halation presets reset, and scan mode set,
  i.e. exactly what `chooseFilm` / `choose_film` would produce. For a paper tile, the
  current film is kept. The tile includes every other current setting, **including crop
  and straighten**.
- Thumbnails are made only while a foldout is open and visible. They are made **one helper
  request at a time**, and only while the main preview has nothing to do: no render
  running or pending, no slider being dragged. The main preview always comes first.
- Out-of-date tiles: after an edit to the same photograph, keep the old thumbnail
  **dimmed** (opacity ≈ 0.55) until the new one arrives. On a **different photograph**,
  tiles become a **blank placeholder** (the grey tile background) immediately, never the
  previous photo's picture.
- A failed thumbnail leaves the placeholder and is not retried until the next edit (no
  spinning).
- **Setting:** "Film and paper previews", subtitle "Show each stock on your photograph. Turn
  off to use simple menus on slower computers.", **on by default**, stored with the other
  app preferences (a missing value means on, so it can't affect startup). Turning it off
  switches back to the old menus immediately without a restart, drops every thumbnail
  texture, and sends `release_thumbnails` to the helper. Turning it on again shows the
  foldouts. Build both controls up front and toggle which one is visible.
- While "Scan film directly" is on, the paper control (menu or foldout) is hidden, as
  before.

## Web (branch `web-prep-port`)

Film and paper previews, no preset previews yet. `web/stocks.js` builds the foldouts beside
the menus (both are built; the switch shows one), `app.js` `pumpPreviews` schedules the
tiles on screen (an IntersectionObserver), and the wasm engine's `thumbnail` renders them
on WebGPU with its own reduced source, 40 cached pipelines and per-film auto-exposure EVs.
The setting is Settings (the gear) › *Film and paper previews*: on by default, and turned
off after a memory crash with a note. Filled grids hold ~100 MiB of the iPhone's tab
(+160–250 MiB while they fill, briefly). Without a photograph the menus show. Measurements
and open items:
[web Prep port](web-prep-port-plan.md#built-2026-11-04). Check: `web/checks/stocks.test.mjs`.

## Helper protocol (`crates/spektrafilm-native`)

```json
{"id": N, "command": "thumbnail", "input": "<photo path>", "source_generation": G,
 "film": "<id>", "paper": "<id>", "params": { ...same runtime params as render... },
 "long_side": 320, "preview_path": "<scratch file>.rgba",
 "crop": {"x":0.1,"y":0.1,"width":0.8,"height":0.8,"angle":3.0}}
```

- `crop` is optional (send it only when the edit has a crop or straighten; same shape as
  `render`/`save`). `long_side` is 16–1024. The reply has `width`, `height`, `backend`,
  `display_color_space`, `elapsed_ms`, `cached_pipeline` and `timing_ms`. The RGBA buffer is
  written exactly like `render`'s preview buffer, so read it the same way and use the same
  colour handling as the main preview (`display_color_space`).
- It **does not touch** the main render's retained output (Save keeps working) or the main
  pipeline cache. It reuses the decoded source (the same `source_generation` rules as
  `render`). It keeps its own downsampled source and an LRU of up to 40 calibrated
  pipelines keyed like the main render's pipeline cache, so edits that don't change a
  stock's calibration (exposure, grain, post…) re-render in ~7–10 ms per tile.
- `{"command": "release_thumbnails"}` → `{}` frees those caches.
- Use the existing render timeout. A thumbnail takes ≤ ~70 ms on the Linux box.

Engine change that all platforms get for free: the input gamut compression baked into the
spectral LUT (`spektrafilm-core/src/input_gamut.rs`, `remap`) now runs rows in parallel.
The output is bit-identical (render hashes compared against `main`). It cut pipeline
calibration from ~140 ms to ~45 ms, which also makes ordinary film changes faster.

## Preset previews (macOS, Linux and Windows done)

- Each row of the Presets panel shows a thumbnail (72 × 48 pt, grey until it arrives) of
  the photograph **with that preset applied**, exactly what clicking it would produce,
  including the crop. Same scheduling, dimming and placeholder rules as the stock tiles;
  thumbnails are made only while the Presets list is on screen, in list order. An invalid
  preset shows a warning symbol in its placeholder. A renamed preset keeps its thumbnail.
- The setting is now **"Film, paper and preset previews"**, subtitle "Show each film,
  paper and preset on your photograph. Turn off to use simple menus and lists on slower
  computers."
  Off shows the old text-only rows.
- Request: the same `thumbnail` command plus `"preset": "<the preset file's text>"`, with the
  current `film`, `paper` and `params`. The helper applies the preset over them exactly as
  `preset_import` does (the preset's film and paper replace `film` and `paper`).
- **Tile key:** a preset replaces every setting except `presets::KEPT`, so key the tile on
  the request with `params` cut down to those paths and without `film`/`paper`. Lab edits
  then leave preset tiles alone. The catalog lists the paths as `preset_kept`; with an older
  helper that lacks it, key on everything.
- On a RAW, a preset whose **white balance** differs from the photograph's needs its own
  decode (~1.5 s for 60 MP). Only the effective white balance counts (temperature and tint
  only in Custom mode, `RawWhiteBalance::effective`). The helper keeps one copy per white
  balance (`thumbnail_wb_sources`, up to 4) reduced to `THUMBNAIL_FRAME_MAX`, so crop changes
  don't decode again, and never replaces the main render's decoded source. Its pixels
  match a full-resolution reduction to within 1/255. Noah chose exact thumbnails over
  using the photograph's white balance (2026-11-02).
- **Order:** on a RAW, the app renders the presets that need their own decode last,
  grouped by white balance (`PreviewRules.renderOrder`); preset tiles wait while the
  engine is still checking the preset files, so the order covers all of them.

## Linux reference implementation

Preset previews on Linux (2026-11-02): `preset_ui.rs` keeps a thumbnail per preset file
(`PresetThumb`, 72 × 48, kept across list rebuilds and renames), the file's text and RAW
white balance from the check, and `wanted_preset_thumbnails` (list order, `render_order`,
`preset_key` on `catalog::preset_kept`). `stock_browser::pump` schedules stock and preset
tiles together; grids and the list count as on screen only while mapped, so the hidden
Adjustments page no longer renders grids while Presets shows. The preference is `previews`.
Measured on the Linux box: 6 presets on a 24 MP JPEG ≈ 0.4 s; on a 24 MP ARW with two other
white balances ≈ 2.4 s (as-shot ones first in ~0.2 s, then ~1 s per extra decode).

- `linux/src/stock_browser.rs`: the whole feature (grid, scheduling (`pump`), staleness,
  setting).
- `linux/src/inspector.rs`: builds the menu and the grid side by side (`stock_browser::rows`).
- `linux/src/preferences.rs`: the `previews` preference and the Settings switch.
- `linux/src/runtime.rs`: `pump` after each main render completes and when one starts, so a
  new photograph blanks the tiles at once.
- `linux/style.css`: `.stock-grid`, `.stock-tile`, `.selected-stock`, `.stock-thumb`.

## Linux measurements (24 MP JPEG, 23 films + 10 papers, 320 px thumbnails)

| | |
|---|---|
| First open, all 33 thumbnails | ~2 s (50–70 ms each, filling in progressively) |
| After an edit (pipelines cached) | ~0.25 s for all 33 |
| Crop 80 % + 3° / tight 50 % crop | ~0.4 s / ~0.7 s for all 33 after an edit |
| Slider latency | at most one in-flight thumbnail (≤ ~50 ms) before the first drag frame |
| Memory, collapsed or setting off | +0 |
| Memory, both grids open | helper ≈ +60 MB, app ≈ +15 MB |
| Startup | unchanged (window appears ~335–360 ms either way; inspector build +~2 ms) |

Measured with `linux/bench_stock_previews.py` (helper only) and `linux/measure_stock_browser.py`
(Linux app), which, with the apps' measurement-only switches (`NEGS_STOCK_EXPAND`,
`NEGS_STOCK_THUMB_PX`, `NEGS_STOCK_THUMB_LOG`, `NEGS_WIDGET_CAPTURE_MS`), were removed on 2026-11-02 once all three platforms were measured.
Get them from the history (`git log -- linux/bench_stock_previews.py`) to measure again.
