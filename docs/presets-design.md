# Presets: design and plan

Status: design agreed with Noah on 2026-10-26, reviewed by Codex (gpt-6-astra, high) the same
day; the review's findings are folded in (list at the end). Branch `worktree-presets`, from main
`dd870f0`. Ticket: `tickets/04-presets.md` in the main checkout's untracked tickets folder.
Follow-up: ticket 18 (Lab controls for settings presets can change).

## What it is

A **Presets** toolbar button next to Adjustments opens a presets panel in the inspector column.
The two panels are exclusive: clicking the other button swaps panels, clicking the active one
hides the column. The panel lists presets, and the user can:

- **apply** one (one undo step, "Apply Preset “Name”"; the Post tab is untouched);
- **create** one from the current Lab settings (New Preset; it asks for a name inline);
- **rename** and **delete** one (delete asks for confirmation);
- **import** files (Import…, several at once, or drag and drop onto the panel);
- **export** one (Export… writes a copy wherever the user picks);
- **Show Presets Folder** in Finder, Explorer or the Linux file manager.

Platforms: macOS, Windows and Linux, together. No "matching preset" checkmark in v1: whether a
preset matches the current state needs a canonical comparison (stock-resolved values, B&W
collapse, f32 controls) that isn't worth building yet.

## File format: Python spektrafilm's GUI state

The format shared on pixls.us (threads 59686, 60317) is Python spektrafilm's GUI-state JSON,
`spektrafilm_gui/persistence.py` at the pinned upstream `28bf883`. Presets are stored in that
format, with no Negs extension. The file name (without `.json`) is the preset name.

Why it works:

- Negs's params are upstream's runtime schema, and since the experimental port (main `13e672e`
  and later) the engine matches `28bf883`. Each GUI-state section maps onto Negs params
  following `spektrafilm_gui/params_mapper.py`; the table is below.
- All 19 forum files translate completely. Of their keys, only two have no Negs field:
  `grain.micro_sublayers` (declared upstream, never read) and `input_gamut_compress.active`,
  which Negs spells as algorithm `"off"`.
- Python's loader (`_merge_into_dataclass`) ignores unknown keys and defaults missing ones, so
  files Negs writes open in the Python app.

### Reading a file

1. **Parse** the bytes in Rust (the helper receives the file contents as a string). Python's
   `json.dump` can write `Infinity`, `-Infinity` and `NaN`; the parser accepts those tokens. Only
   `+Infinity` in the Langmuir knees is meaningful (the linear law); any other non-finite number
   is refused.
2. **Fill missing fields like Python's loader does.** Python merges a file into
   `PROJECT_DEFAULT_GUI_STATE`, the digested Kodak Gold 200 / Supra Endura state (`state.py`), not
   into the chosen stock's defaults. Negs ships that state, frozen from `28bf883` by a Python
   script (`data/presets/spektrafilm-gui-default-28bf883.json`, generated with
   `gui_state_to_dict`, never by Rust), and merges the file onto it with the same rules: unknown
   keys ignored, `null` never replacing a tuple. So a sparse file means in Negs what it means in
   Python, and a missing RAW white balance is daylight, as upstream.
3. **Validate** (refusals name the reason): not a JSON object; unknown film or paper; a route
   other than `input > film > print > scan` and `input > film > scan`; channel swaps other than
   `[0, 1, 2]`; `simulation.output_color_space` other than sRGB (Python renders into it, Negs's
   previews and the preset's look are defined in sRGB); a gamut algorithm Negs doesn't implement
   (`jzazbz`, which Negs would silently treat as off); any value of the wrong type, shape, enum or
   domain for its Negs field.
4. **Translate** with the table.

### Mapping, after `params_mapper.build_params_from_state`

| GUI state | Negs |
|---|---|
| `simulation.film_stock`, `simulation.print_paper` | `EditState.film`, `.paper` |
| `simulation.route` | `io.scan_film` = route is `input > film > scan` |
| `simulation.print_illuminant`, `print_exposure`, `print_exposure_compensation`, `print_y_filter_shift`, `print_m_filter_shift` | `enlarger.illuminant`, `print_exposure`, `print_exposure_compensation`, `y_filter_shift`, `m_filter_shift` |
| `preflashing.preflash_exposure`, `preflash_y_filter_shift`, `preflash_m_filter_shift` | `enlarger.*` (the section's other fields are ignored, as upstream does) |
| `glare` | `print_render.glare` (upstream never sets `film_render.glare`; it stays at the default) |
| `halation`, `grain`, `couplers` | `film_render.halation`, `.grain`, `.dir_couplers` |
| `chemistry`, `film_chemistry` | `print_render.density_curves_morph`, `film_render.density_curves_morph`; their `development_time` goes to `print_render.development_time` / `film_render.development_time` |
| `film_base`, `print_base` | `film_render.base`, `print_render.base` |
| `camera` (minus `diffusion_filter`), `camera_diffusion` | `camera.*`, `camera.diffusion_filter` (upstream overwrites the camera's own copy) |
| `enlarger_diffusion` | `enlarger.diffusion_filter` |
| `scanner` | `scanner` |
| `input_gamut_compress` | `io.input_gamut_compress`; `active: false` → algorithm `"off"` |
| `output_gamut_compress` | `io.output_gamut_compress` |
| `input_image.rgb_to_raw_method`, `apply_hanatos2025_adaptation_window`, `_surface`, `spectral_gaussian_blur` | `settings.*` |
| `load_raw.white_balance` (+ `temperature`, `tint` when `custom`) | `io.raw_white_balance` |
| `input_image` io fields, `convert`, `display`, saving and workflow fields | ignored: they describe the source image, film-convert, display or saving, not the look |
| `grain.micro_sublayers` | ignored (unused upstream) |
| settings the mapper forces (`use_enlarger_lut`, `use_scanner_lut`, `lut_resolution`, `use_fast_stats`, `preview_max_size`) | ignored: they trade accuracy for GUI speed; Negs computes exactly |

### Writing a file

Every mapped section, plus `simulation.film_stock`, `print_paper`, `route`,
`output_color_space` (sRGB) and `load_raw`. Values are written at full precision, so a preset
exported by Negs and imported again renders identically in Negs. Values Negs leaves to the stock
(grain presets, DIR gammas, halation baseline, development time "stock default") are written
resolved, since nothing in the file format means "the stock's value". A Langmuir knee of
infinity is written as `Infinity`, which Python's `json.load` reads.

Limits, documented rather than hidden:

- **Python's GUI quantizes on load.** Its widgets hold two decimals (three for halation
  strength), and it renders from the widgets, so it renders a Negs export with those rounded
  values. The widget round-trip test below measures the difference.
- **Python's development-time dropdown** only offers the stock's times after a stock change, so
  a loaded B&W preset can fall back to the dropdown's middle entry.
- **Infinite Langmuir knees** become 1e6 in Python's spin boxes, nearly linear.
- **`lightness_compression: null`** (off) doesn't survive Python's loader, which never lets
  `null` replace a tuple; Negs reads files the same way, so such an export comes back on. Negs's
  Lab has no control that turns it off.
- **Python's GUI applies stock presets on the first render** after start-up or a stock change
  (`controller.py`: `_next_runtime_digest_applies_stock_specifics`), then uses the loaded values
  from the next render on. Negs always uses the saved values (next section).

### What a preset carries

Film, paper and every Lab setting, including camera exposure and auto exposure, and the RAW
white-balance mode (plus temperature and tint when custom): the forum presets were made for
daylight-balanced RAWs.

Left out, and kept as they were when applying: Image Size, the 12 MP preview limit, output
encoding, low-memory export, rotation, crop, border, Post, and the CAT16 toggle (a Negs-only
switch back to 0.3.2 behaviour; upstream always uses CAT16, Negs's default).

Applying is deterministic: the Lab settings come from the (normalized) file, and nothing else
from the previous state survives except the excluded settings above.

## Engine prerequisite: saved values beat stock presets

Today `apply_film_specific_params` (`crates/spektrafilm-core/src/pipeline.rs`) re-applies
`data/presets/couplers.toml` to the four DIR gamma arrays and the `(use, antihalation)` table to
`halation_strength` and `halation_first_sigma_um` on every pipeline build, discarding whatever
params say. Five forum files deliberately change the gammas and three the halation strength.

**Policy (approved by Noah): a value set in params is used; an unset one takes the stock's
preset.** It is the deterministic form of what upstream's GUI shows once its first render is
past: `params_mapper` replaces whole groups after `init_params`, and `digest_params` re-applies
stock presets only on the first render after start-up or a stock change. Negs already treats the
grain preset fields this way.

- `DirCouplersParams`' four gammas and `HalationParams`' `halation_strength` and
  `halation_first_sigma_um` become `Option`, `None` (JSON `null`) by default. Presets fill only
  `None` fields; a film without a preset falls back to the schema values. Readers use accessors
  that resolve to the schema values. Default renders don't change. **Done** (`a3c399d`), with
  tests in `crates/spektrafilm-core/tests/stock_preset_overrides.rs` and a check that
  `couplers.toml` sets nothing else. A/B golden run against main: 478/484 renders
  byte-identical on f64, f32 CPU and GPU; the 6 others are `halation_big`, as intended; GPU vs
  f64 76/76.
- The native catalog reports each stock's resolved gammas and halation baseline beside its grain
  preset (for export, and for ticket 18's controls).
- The apps reset these six fields to `null` when the film changes, as macOS and Windows do for
  grain. Linux doesn't reset grain on film change today; it will reset both.
- **Saved sessions:** sessions written before the change hold concrete values for these fields
  that never took effect, and nothing could set them. Restoring such a session drops them. macOS:
  `EditorSession.schema` 1 → 2; Windows and Linux: a schema field in their session files (Linux
  reads its session without one today). The migration runs before validation and default
  merging.
- **Nullable values survive restoring.** `JSONValue.restoring` (macOS), `windows/src/json.rs`
  and `linux/src/parameters.rs` replace a saved `null` inside an array with the default element,
  so a Langmuir `[null, null, null]` (infinity) comes back as `[1, 1, 1]`, and a saved `null`
  `lightness_compression` comes back as the default tuple. The catalog lists the paths where
  `null` is a value (the six fields, the grain preset fields, `development_time`, Langmuir
  elements, `lightness_compression`, `photo_long_side_px`), generated from the Rust schema with a
  test, and the three restorers keep a saved `null` there.
- Golden manifests: only cases that set these fields explicitly change, by design
  (`scripts/gpu_cpu_quality/params/fx/halation_big.json`). Everything else must stay
  byte-identical; verified with an A/B golden run against main.

## Storage

One `<name>.json` per preset in an app-owned folder:

- macOS: `<Application Support>/<data root>/Presets/`
- Windows: `%APPDATA%\Negs\Presets\`
- Linux: `$XDG_DATA_HOME/negs/presets/` (`~/.local/share/negs/presets/`)

**Names.** One policy on every platform: refuse empty names, control characters,
`/ \ : * ? " < > |`, leading dots, trailing dots or spaces, and Windows device names (`CON`,
`PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, also with an extension). Names compare by a
key: NFC-normalized and case-folded. A clash gets " 2", " 3"… by that key. Existing files are
always addressed by the path found on disk (macOS may hand back NFD).

**Writes.** Read and validate one byte buffer; write it to a unique temporary file in the
folder, then link it to its final name without replacing (retrying the next suffix on a clash).
Rename and delete are serialized and check that the file is still the one listed (size and
modification time) before acting. Import copies the original bytes, so a re-export gives back
the imported file; New Preset writes Negs's export.

**Listing.** The panel lists the folder each time it opens, sorted by the name key, so files
copied in by hand appear without a watcher. Files that don't translate are shown greyed with the
reason, and can be deleted. The translation is cached by path, size and modification time.

**Cleanup.**

- macOS "Remove App Data and Quit" deletes the folder; its alert text adds "presets".
- Windows uninstaller: "Remove all app data" deletes them; "Keep settings and editing session"
  becomes "Keep settings, presets and editing session".
- Linux has no cleanup command.

**Preview builds.** macOS hard-codes `dev.negs.app` for the session, the preview cache and
cleanup, so a preview bundle (`dev.negs.app.preview`) shares and can delete production data. The
data root becomes the bundle identifier (unchanged for the release app), covering sessions,
presets, caches and cleanup. Linux already has `NEGS_PROFILE_DIR`; Windows gets the same
override.

## Where the code lives

- **Translation, normalization and validation** are Rust, once, in
  `crates/spektrafilm-core/src/presets.rs`. The native helper exposes them, since all three apps
  already talk to `spektrafilm-native` (the catalog reports `capabilities.presets`):
  - `preset_import { text, params? }` → `{ film, paper, params }`, or an error naming the
    reason. `params` is complete: the preset's values, Negs's defaults where Python's GUI has no
    such setting, and the excluded settings (`presets::KEPT`) copied from the request's `params`
    (the photograph's current ones; defaults without). Without `params` it only checks a file.
  - `preset_export { film, paper, params }` → `{ text }`.
- **Apps** do files, the panel and applying, as one undo step:
  - macOS and Windows replace `EditState.params` with the result and set film and paper without
    `chooseFilm`'s side effects.
  - **Linux** keeps controls apart from params (`values`, `choices`, `params` in
    `linux/src/model.rs`) and rebuilds the params from the controls (`parameters::runtime`). Its
    adapter writes the fragment through all three: control-backed paths into `values`/`choices`
    (development time as the index of the nearest listed time, which is the entry the engine picks
    anyway), the rest into `params`, film and paper without the film handler's side effects.

## UI per platform

| | macOS | Windows | Linux (GTK 4) |
|---|---|---|---|
| Toolbar | "Presets" button beside Adjustments (`NSToolbarItem`, selected state) | toolbar button beside Adjustments, checked state | header-bar toggle beside the Adjustments toggle |
| Panel | SwiftUI list in the inspector column, replacing `AdjustmentInspector` | Win32 ListView (report, `LVS_EDITLABELS`) in the adjustments column | `gtk::Stack` page in the sidebar, `ListBox` rows with `EditableLabel` |
| Show/hide | View ▸ Show Presets ⌥⌘P | View ▸ Presets F8 | F8 |
| New Preset | ⇧⌘S | Ctrl+Shift+S | Ctrl+Shift+S |
| Import Preset… | ⇧⌘O | Ctrl+Shift+O | Ctrl+Shift+O |
| Drag and drop | `onDrop` of file URLs on the panel | `WM_DROPFILES` on the panel | `DropTarget` for `gio::File` lists |
| Apply | click a row; double-click or Return applies it again | the same | the same |
| Rename | context menu, inline; New Preset starts it | F2 or slow click, inline label edit | `EditableLabel` |
| Delete | Delete key or menu, confirmation alert | Del or menu, confirmation | Delete or menu, confirmation |
| Reveal | Show Presets Folder (Finder) | Show Presets Folder (Explorer) | Show Presets Folder (`gio::AppInfo::launch_default_for_uri`) |

The shortcuts are free on all three platforms today. The panel header has "+" (New Preset) and
a "…" menu (Import…, Show Presets Folder). Each row's context menu: Apply, Rename, Export…,
Delete. The open panel is remembered across launches, like the Lab/Post tab. Clicking a preset
applies it, as in Lightroom and Photos, so double-click and Return apply rather than rename (a
change from the first design, where they renamed). New Preset names the file after the film and
starts renaming it. Files the engine refuses are listed greyed, with the reason.

Out of scope: per-film defaults (later, possibly "Use as Default for <film>" on a preset),
thumbnails, manual ordering, the matching-preset checkmark, darktable `.dtpreset` / `.dtstyle`
import (later), Finder double-click (Negs can't claim `.json`).

## Plan

Each step is a commit on `worktree-presets`, dated one minute after main's tip. No merge or push
without Noah's go-ahead.

1. **Engine: stock presets fill only unset fields.** Done (`a3c399d`), including the catalog's
   resolved values and nullable-path list, and an explicit-DIR LUT export test.
2. **Apps: film change resets the six fields (and grain on Linux); session schema and migration;
   nullable paths survive restoring.** Done: macOS and Windows `153345c` (macOS tests pass,
   Windows type-checks for MSVC, its unit tests need Windows), Linux `d45fb65` (built and tested
   on Noah's Omarchy machine, including the live helper schema gate).
3. **Translator** in `spektrafilm-core::presets`, the frozen GUI default state, and the two helper
   commands. **Done:**
   - unit tests in `presets.rs` and the helper;
   - `scripts/parity/check_presets.py`: 5 generated presets and the 18 forum JSON files match
     Python's values exactly and its renders within 1e-7 (1.5e-5 with a diffusion filter, Python's
     float32 FFT). It found the print white/black-correction references missing the preflash, now
     fixed in the engine (`color_reference.rs`, frozen test);
   - `scripts/parity/preset_widget_roundtrip.py`: Python's widgets round a Negs export to 2
     decimals (3 for halation strength) and cap an infinite Langmuir knee at 1e6, 5–9 8-bit levels
     on the presets tried. The forum presets, made in Python, come back unchanged.

   Grain and glare need no statistical check of their own: their values arrive exactly (the
   value check) and the engine's grain is gated statistically already. Tests as planned:
   - every refusal case, sparse files against the frozen default, `Infinity` handling;
   - Negs → file → Negs renders identically (compare renders, not raw params: export resolves
     stock values);
   - **parity:** GUI-state fixtures generated by Python `28bf883` (`build_default_gui_state` plus
     edits, saved with `gui_state_to_dict`, including explicit DIR gammas and halation strength),
     rendered by Python through `build_params_from_state` and `digest_params(...,
     apply_stocks_specifics=False)` with the enlarger and scanner LUTs off, against the Negs f64
     CLI on the translated params (`--scan-film` when the route says so), on identical linear
     float inputs at one resolution, grain and glare off, at the parity tolerance (1e-6);
   - grain and glare from imported presets checked statistically, as the grain gates already are;
   - **widget round trip:** a Negs export loaded through Python's GUI loader and widgets
     (offscreen Qt) and collected back, to measure the GUI's quantization;
   - the forum files are unlicensed: used locally only, never committed or bundled.
4. **macOS app:** data root from the bundle identifier, storage, panel, toolbar, menu and
   shortcuts, drag and drop, cleanup text. XCTest for apply and undo, the excluded settings,
   naming rules and the session migration; an apply → render test through the helper. **Done**
   (`macos/Sources/NegsApp/Presets.swift`, `PresetInspector.swift`, `PresetTests.swift`; 86
   tests pass). Imported development times snap to the stock's listed ones in the engine, so the
   Development picker shows them. The render after applying is covered by the helper's own
   import tests and the parity check, not a separate app test.
5. **Windows app:** the same; `cargo check --target x86_64-pc-windows-msvc`, unit tests for
   storage and naming, installer text.
6. **Linux app:** the adapter and the same UI; tests for apply, film A → B → A, undo, redo and
   restart through `parameters::runtime`; `check_controls.py`/`check_ui.py`; the Keyboard
   Shortcuts window.
7. **Local testing for Noah:** a macOS preview bundle with its own data root, the 19 forum files
   copied into its presets folder (outside the repo). Screenshots on each platform.
8. **Docs:** README feature line; `docs/features-vs-upstream.md`.

## Review (Codex gpt-6-astra, high, 2026-10-26)

| # | Finding | Resolution |
|---|---|---|
| 1 | Upstream's GUI applies stock presets on the first render after start-up or a stock change, so "saved values win upstream" was overstated | Kept the policy, stated as Negs's deterministic choice; parity reference uses `apply_stocks_specifics=False` |
| 2 | Python's widgets round to 2 decimals (3 for halation strength); development-time dropdown is stateful | Documented as limits; widget round-trip test measures it |
| 3 | Linux keeps controls apart from params and rebuilds params from them | Linux adapter writes through `values`/`choices`/`params`; film handler resets grain too |
| 4 | Python fills missing fields from the Gold 200 default GUI state, not engine defaults; missing WB is daylight | Merge onto the frozen `28bf883` default GUI state before translating |
| 5 | `jzazbz` and non-sRGB output pass type checks but can't be reproduced | Semantic validation; refused |
| 6 | `Infinity`/`NaN` and `null` mean different things on each side | Lenient parse in Rust; only +∞ Langmuir accepted; export writes `Infinity` |
| 7 | The three restorers replace saved nulls with defaults; Linux session has no schema | Nullable-path list from the catalog; schema on all three; migrate first |
| 8 | Exact round trip and the checkmark contradict resolved export | Checkmark dropped from v1; round trip compared by render |
| 9 | Parity test narrower than "renders as in Python" | Defined precisely; `--scan-film`; statistical grain/glare; widget round trip; LUT export with explicit DIR |
| 10 | Naming rules incomplete | One portable policy with NFC + case-fold key |
| 11 | Fixed temp files and replace-on-write can clobber presets | Unique temp files, no-replace commit, identity check |
| 12 | The macOS preview bundle shares production data | Data root from the bundle identifier |
