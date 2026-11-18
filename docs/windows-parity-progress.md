# Windows parity: progress log

Newest first. One entry per phase and per merge: what landed (sha), the gate results,
decisions made with their reasons, and what was carried over. See
[the plan](windows-parity-plan.md) and [the coordinator handoff](windows-parity-coordinator.md).

## Status

| # | Phase | State |
|---|---|---|
| 0 | Baseline | done |
| 1 | Drawing foundation | done, merged `e142fba` |
| 2 | Window shell | done, merged `a9a0763` |
| 3 | Inspector | done: 3a and 3b merged (see log) |
| 4 | Canvas | done, merged `8f5bdde` |
| 5 | Tray | done, merged (see log) |
| 6 | Library | done: crate and glue merged (`c421063`) |
| 7 | Strip and grid | done, merged `f9b8f47` |
| 8 | Remaining features, Flip added (Open in Lightroom last) | done: 8c `1396397`, 8b `4a15d7f`, 8a `ee752ee` merged; follow-ups running |
| 9 | Validation and release | not started |

## 2026-11-19: phase 8a merged, the editor features (`ee752ee`, branch tip `5b369fb`)

- **What users see:**
  - **Flip:** "Flip left to right" in the crop panel's Flip section, and View › Flip
    Horizontal. The preview, exports, thumbnails, and the tray and preset tiles are
    mirrored. `io.flip` is mapped in the library glue and passed to the engine, with
    no engine change.
  - **Enlarger › Shifts move:** separately, together or opposite. A double-click on
    a linked shift is one undo step.
  - **B&W colour filters** in Lab › Camera.
  - **Your negative:**
    - The tray has a Scan group, and opens on it for a negative, or when
      `negative_check` says the photograph looks like one.
    - The label reads "Your negative ⌄ read as … / printed on …".
    - Prep has Film base (Find again, Pick from rebate, Crop to picture,
      Backlight… behind More), Flat field (Choose…/Off, with why it isn't used),
      Balance (Auto, Pick neutral), and the darkroom rolls (Use for whole roll,
      Roll's filtration, "A frame of a roll…").
    - Picks follow the crop's straightening.
  - **Compare:**
    - C, View › Compare, the title bar's new Grid · Photo · Compare switch, or the
      context menu's "Compare 3 and 7 / with 3 / with Original".
    - Side by side or slider (S), Swap, Fit/100%, and Done (Esc). Each photograph
      has a numbered ring and its facts, with the ones that differ underlined; the
      status line says what differs; the inspector says "Editing ① the left one".
    - No strip while comparing, as on the Mac.
    - The other photograph develops as opening it would: a full preview render with
      its match solved first (8b's solve and `BrightnessMatches.json`). Until then a
      stand-in shows: the kept develop, else the developed thumbnail, else the
      embedded preview.
    - The open side shows 208 ms after C, and a stand-in 43 ms. Picture making is
      off the UI thread; it used to freeze the window for 1.7 s.
  - **Original ⌄:** hold, split (as Compare's slider against the original), or side by
    side. For RAW+JPEG it adds Show the RAW / Show the camera JPEG.
  - Plain C compares, as on the Mac; Ctrl+K still crops.
- **Gates:**
  - The full Windows suite on the PC, at `5b369fb`: 276 passed.
  - Mac checks: the cross-check is clean; library-map 7 + 18, tray-rules 19 and
    negs-library all pass.
  - Flipped exports, grain and glare off: on both the PC and the Mac, the flipped
    export is the exact mirror of the unflipped one. The two machines differ by the
    same amount with flip on or off, so flip adds no difference.
  - Shots in `~/.negs/windows-parity/phase8a/{darkroom,daylight,roll}/`. The Mac
    Compare references are in `phase8a/ref/darkroom/`. Side by side and slider match
    the Mac's layout.
  - No ABBA: launch code wasn't touched.
- **Decisions:**
  - Compare has its own helper, so the open photograph's decode stays loaded.
  - Compare's pictures are capped at 3072 px, because the view draws through GDI.
  - `negs-library`'s `change_edits` transform now receives the item. `change_each_edit`,
    `opening_state` and `new_id` are new, for the rolls.
- **Different from the Mac:**
  - Compare's differences wait until both photographs' facts are known.
  - 100% follows the open photograph's scale, at most 3072 px.
  - Backlight's shown state resets for each photograph.
  - The inspector isn't dimmed while the original is active.
  - A flip re-develops the tiles.
  - Not tried: a RAW+JPEG pair in Original ⌄ (the test roll has none).
- **Follow-up (running):**
  - Compare's captions leave out the camera ("Nikon D750").
  - The edited side's film and paper lack their ⌄ menus and lock.
  - The status line's "← → steps the right one" is cut off at 1440 px.

## 2026-11-19: phase 8b merged, faster open, batch export and Open in Lightroom (`4a15d7f`, branch tip `fbb7a11`)

- **What users see:**
  - **Faster open (ticket 39).** While Match camera brightness is pending, the
    helper decodes the RAW once and the camera preview is read beside it. Then the
    match is solved, and the one render at the matched exposure follows. The
    darker or lighter first frame is gone. Matches are kept in
    `BrightnessMatches.json` (2,000, least recently used out), so a photograph you
    come back to skips its solve.
  - The camera preview's Adobe RGB to sRGB conversion uses exact tables:
    975 → 170 ms on an M1, byte-identical over all 2^24 colours.
  - **The stand-in.** As a photograph opens, its developed thumbnail shows on the
    canvas at once, as the Mac's `standIn` does. A launch restore reads it off the
    UI thread.
  - **The film-side release (ticket 41).** Opening the grid and Windows'
    low-memory notification send `release_film_cache`. The helper keeps no film
    side off macOS yet, so for now it answers 0 bytes.
  - **Batch export.** With several photographs selected in the grid,
    File › Export ("Export 8 Photographs…") opens the folder dialog. Its options
    are the single export's, plus File Name (Name_Film, Name or Custom) and If a
    File Exists. Each photograph develops with its own edits; an unsolved match is
    solved in the batch's helper and stored. The status line and the grid's foot
    show "Exporting 1 of 8 photographs" with Cancel. Afterwards they show
    "Exported 7 of 8 · 1 failed" with Show, whose popover lists what failed and
    why, with Retry Failed and Show in Explorer. A batch cut short by quitting is
    offered again at the next launch.
  - **Open in Lightroom (ticket 48)** appears in four places: File, under Export in
    the Output foot, and the strip's and grid's context menus (which also gain
    Export). It develops full-size 16-bit TIFFs (`{name}-Negs.tif`, rated) and hands
    them to Lightroom in one launch. A Lightroom that isn't running is started first
    and given its window and 3 s more.
    - The cloud Lightroom's TIFFs wait in Negs' cache. Each is deleted once
      Lightroom has a copy, or after a week. Classic's go to Pictures › Negs for
      Lightroom and are kept.
    - The menu letters are G (Li&ghtroom) and C (&Classic), because O, L and E are
      taken.
  - **Settings › Upkeep › Lightroom** shows how much was sent ("145.0 MB sent")
    with Delete Sent Copies. Classic's button asks first.
- **Gates:**
  - The PC build at each step.
  - The full suite (`test-parity.cmd`, scratch profile) at `fcf4a6f`: 257 of 257,
    including the three new lifecycle tests (batch, Lightroom hand-off, stand-in).
    After the cleanup commit, the five Lightroom tests pass again at `45f9001`.
    Their detection check (`NEGS_EXPECT_LIGHTROOM`) finds
    `C:\Program Files\Adobe\Adobe Lightroom CC\lightroom.exe`. The hand-off records
    every file in one call.
  - `negs-library` tests pass on the Mac. The Mac cross-check is at its baseline
    warnings, with and without `--tests`.
  - **Export byte-identity**, main `a1114a9` against the branch: an unedited
    `_NHA9790.NEF` with the match on, exported as a 16-bit TIFF from scratch copies
    and profiles. Both files are 144,969,586 bytes with the same SHA-256
    (`B574F0A9…E03A`).
  - **Startup ABBA**, 8 runs each, `_NHA9786.NEF` (a scratch copy). Defender used
    0 CPU-s over 5 s beforehand. Medians in ms from process start:

    | Build | Window composited | Inspector shown | GPU ready | First image |
    |---|---|---|---|---|
    | main `a1114a9` | 90.0 | 194.3 | 623.5 | 8569.6 |
    | branch | 91.5 | 198.2 | 633.4 | 2705.2 |

    The branch's warm runs reuse the solved match. With `BrightnessMatches.json`
    and the cache emptied before every run (4 runs each, ABBA), the first image
    takes 8613.8 ms on main and 3104.4 ms on the branch. The window, inspector and
    GPU stamps are within noise.
  - **Batch** in Darkroom and Daylight: 8 photographs selected. One was deleted
    mid-batch, and it failed as "The photograph is missing." The other 7 PNGs were
    written.
  - **Lightroom live test**, with Noah's OK, one goat photo (`_NHA9788.NEF`, a
    scratch copy), Lightroom not running:
    - Negs wrote `_NHA9788-Negs.tif` (144,969,810 bytes) and started Lightroom one
      second later. It said "Sent 1 photograph to Lightroom".
    - Lightroom opened signed out, on Adobe's Sign in dialog. Nothing reached its
      library: no Negs file appeared under its `Data` folder. Nothing was clicked
      in Lightroom.
    - The warm-start send and the library check wait for Noah to sign in. One
      retry is left.
  - Screenshots in `~/.negs/windows-parity/phase8b/`:
    - `{darkroom,daylight}/`: `grid`, `batch-sheet`, `batch-progress`,
      `batch-outcome` and (Daylight) `batch-popover`. `mac-batch-sheet` is the
      Mac's.
    - `lightroom/`: `file-menu`, `output-foot`, `settings-lightroom`, `sent` and
      `lr-cold` (Lightroom's Sign in dialog).
- **Decisions:**
  - `BrightnessMatches.json` is keyed by the file's identity, not its contents:
    SHA-256 of path, size, mtime, engine and the exact request. It is cheap on the
    UI thread.
  - Cleanup looks in two places. One is the Mac's `*.lrlibrary` in Pictures. The
    other is where Lightroom on Windows keeps local storage:
    `%LOCALAPPDATA%\Adobe\Lightroom CC\Data\<catalog>\originals`, which uses the
    same date folders (seen on the PC).
  - No Ctrl+Alt+E shortcut: on German keyboards Ctrl+Alt is AltGr.
  - The stand-in shows uncropped, unturned and without a border, under the tray's
    stand-in sizing.
- **Different from the Mac or left:**
  - There is no quick first frame: the solve comes first, as on the Mac since
    ticket 39.
  - Batch:
    - No notification is shown while the app is inactive.
    - Extensions are `.jpg`/`.tif`.
    - The folder dialog is used even for a single target that isn't open.
    - Resume is offered after launch's first image.
    - The Custom name example updates on control events only.
  - The helper frees no film memory off macOS until a Vulkan budget exists.
  - Lightroom's cold start with a signed-in account is untested (the Mac's
    dropped files).

## 2026-11-19: phase 8c merged, polish on the strip, grid and tray (`1396397`, branch tip `b5a0fd0`)

- **What users see:**
  - **The tray** paints on the GPU, through the strip's Direct2D swap chain (WARP
    too). A group's name in its foot glides the row there in 0.25 s.
  - **Screen readers** read the tray: each group's name, every tile with its state
    ("current", "before") and its star, the group index ("Show Kodak"), Search, the
    paper lock's switch and Done, in the Mac's words. A default action presses each.
  - **The grid** glides with the wheel (a notch at a time; touchpads and animations
    off scroll at once). The Mac's overlay scroll bar shows at the right edge while
    it scrolls and fades a second later. It widens under the pointer; drag the knob,
    or click above or below it to page. Only the rows on screen are painted.
  - **Thumbnails** are cropped, turned and inside their borders: solid, keyline,
    film and 35mm, through `spektrafilm-io`'s own crop and border.
  - **The open photograph's thumbnail** is filed from its settled preview when it
    is left or the grid opens, under the Mac's conditions, so it shows at once and
    costs no develop.
  - **Kept develops** (`Cache\Renders`) point a loose key at themselves, so a
    photograph still opens on its last develop after an engine update.
  - **The leader's slanted edge** is smooth. The canvas draws the tongue under the
    leader's window, and the window's aliased shape sits 2 px inside it.
  - **Settings › Photographs** has "RAW + JPEG" › "Show camera JPEGs separately"
    and "Edits" › "Save ratings and edits next to photos", in the Mac's words and
    places. Both default to today's behaviour.
  - The window reopens at exactly its last size at 125 % and 150 %.
  - The dead `fonts.glyph` is gone.
- **Gates:**
  - The PC build (`662fe87`).
  - The full Windows suite (`test-parity.cmd`, scratch profile), run because the
    open path files the thumbnail: 243 of 244 passed. The failure was the Settings
    test still expecting the old Photographs sections. It is updated, and its 11
    tests pass.
  - `windows/tray-rules` 17 and `windows/sheet-rules` 19 pass on the Mac. The Mac
    cross-check is at its baseline warnings, with and without `--tests`.
  - `NEGS_GRID_BENCH`, 2 rounds, 1582×953 px at 125 %, GTX 1660 SUPER:

    | Path | Median frame | p95 frame | Frames over 20 ms |
    |---|---|---|---|
    | Direct2D, hardware | 16.67 ms | 16.78–16.86 ms | 1 of 335 |
    | Direct2D on WARP | 33.35 ms | 33.4–50 ms | nearly all (30 fps, as phase 7) |

  - Screenshots in `~/.negs/windows-parity/phase8c/{darkroom,daylight}/`:
    - `strip`, `strip-after`: the leader's smooth edge; the open photograph's
      keyline thumbnail after the grid.
    - `grid`, `grid-developed`: white, 35mm (Kodak Gold 200), keyline and film
      borders, from seeded Mac-format sidecars.
    - `grid-scrolled`: a page down with the widened bar.
    - `tray`: GPU-painted, with the Search field showing over the swap chain.
    - They match the Mac's `04-film-tray` and `09-grid`. The tray's Scan group is
      8a's.
    - Pixel rows across the leader's slant show 3–4 intermediate shades per step.
- **Decisions:**
  - The Mac uses the kept develop only on the canvas (and in Compare), never for
    strip thumbnails. So item 4 became the Mac's actual use: the kept develop stands
    in after an engine update too (`previousKey`). The strip's stand-ins stay the
    developed thumbnail, then an older engine's, then the embedded preview.
  - Thumbnails filed from the preview are reduced to about twice what the crop
    needs, then cropped, turned and bordered. A wide-gamut preview is brought to
    sRGB with the canvas's `REC2020_TO_SRGB`.
  - A thumbnail is filed only when the preview settled for the edits shown and
    they are saved. With no stored edits, it is filed only when the photograph
    shows none (`!open_edited`).
  - The tray's accessibility is MSAA `IAccessible`, which UIA reads through its
    bridge, as the rest of the app does. Search is its own window's object.
  - The leader stays a window. The canvas draws its smooth edge, rather than the
    window becoming layered or composited.
- **Different from the Mac or left:**
  - The developed thumbnail does not yet stand in on the canvas while a photograph
    opens (the Mac's `standIn`). That belongs to the open path (8b's faster open).
  - Entering the grid files the thumbnail but not the kept develop. The Mac saves
    both.

## 2026-11-19: phase 7 merged, the strip and grid (`f9b8f47`, branch tip `774d5ef`)

- **What users see:**
  - **The strip:** 158 DIP of film over the 28 DIP status line, which is the tray's
    186, so the canvas no longer refits when the tray opens.
    - Frames are at the photograph's real proportions. The open one is whole with
      the pencil under it; the others are dimmed, and a reject more.
    - Marks: hand stars, the pick box, the reject cross, the edited dot and "+J".
      Their halo is a real blur.
    - The leader tongue hides and shows the strip. It is hidden while cropping, as
      on the Mac.
    - The strip repaints only when what it shows changes, not on slider moves. The
      scroll bar that covered the pencil is gone.
  - **The grid:** G opens it over the window, unfolding out of the strip.
    - Packed rows; "Showing all ⌄ 34 photographs" (its words open the filter menu,
      × clears it); the count and the Size slider at the foot (`gridSize` in prefs).
    - Selection: click, Ctrl, Shift, a band, arrows, Home/End, Ctrl+A. Ctrl+plus,
      Ctrl+minus and Ctrl+wheel change the size.
    - E, Enter, Esc or a double click fold it back. Ratings, flags and the edit
      commands act on the grid's selection (`Shelf.in_grid`).
  - **Thumbnails:**
    - Developed thumbnails are filed under the Mac's key in `Cache\Thumbnails`. An
      older engine's stands in, then the embedded preview.
    - Missing ones develop on-screen first, then outward; with energy saver on,
      only on-screen ones. Reading covers ±32 around the view.
    - Explorer's thumbnails are no longer used, so `Thumbs.db` is no longer written.
    - The strip uses the library's folder watcher only.
  - **Opening on the last develop:** leaving a photograph keeps its settled develop
    (2560 px) in `Cache\Renders`. It stands in until the new develop arrives.
  - **While cropping:** the status line reads "Cropping · Original · 6016 × 4016 px",
    with Fit and 100% dimmed.
  - Phase 5's items are fixed: the canvas refit, and the tray's hint (it shows only
    when it fits whole).
- **Painting decision: Direct2D on the GPU, not wgpu.** Measured with
  `NEGS_GRID_BENCH`: 300 thumbnails, 6 s of flings, 1582×953 px at 125 %, GTX 1660
  SUPER.

  | Path | Median frame | p95 frame | Frames over 20 ms |
  |---|---|---|---|
  | Direct2D, hardware | 16.67 ms | 16.79 ms | 1 of ~333 |
  | wgpu (Vulkan) | 16.4 ms | 33.3 ms | about a third |
  | Direct2D on WARP | 33.3 ms | 33.4–50 ms | nearly all (30 fps, usable) |

  It is smoother and simpler: it reuses the painter, marks and text, and WARP works
  without Vulkan. The device is made after the canvas's, never during launch.
- **Gates:**
  - The PC build.
  - The full Windows suite (`test-parity.cmd`, scratch profile): 235 passed.
  - `windows/sheet-rules`: 16 pass on the Mac. The Mac cross-check passes, with and
    without `--tests`.
  - Screenshots in `~/.negs/windows-parity/phase7/{darkroom,daylight}/` match the
    Mac's prep, grid and crop references.
    - The first crop shot showed one frame. The cause was a filter the shot script
      left on, not a bug.
- **Startup** (ABBA, 8 runs against main's exe, Defender quiet, a D750 NEF from a
  scratch copy):

  | | first window | inspector | gpu_ready | first image |
  |---|---|---|---|---|
  | main | 103.6 | 215.35 | 666.05 | 8459 |
  | branch | 103.95 | 211.05 | 644.4 | 8513 |

  - The first image is +54 ms (0.6 %).
  - The first try cost about +355 ms. The fix is that the strip's device, thumbnail
    reading, the leader window and the engine stamp now wait until after launch's
    first image.
  - The cost: on a cold start the strip shows bare film until the first image. That
    is accepted for now, and phase 8's faster open shortens it.
  - The absolute numbers are higher than phase 6's because this photograph waits
    about 4 s for the camera-brightness reference. That wait is a phase 8 item.
- **Decisions:**
  - `negs-library` gained `Library::thumbnail_state`, `unedited_state` and
    `thumbnail::unedited_look`. They read sidecars and write nothing.
  - A Windows loose-key stand-in is a small `.latest` file, since Windows lets only
    administrators make symbolic links.
  - The brightness reference for thumbnails stays exact, as opening solves it.
  - Added the `image` crate's `jpeg` feature and `Win32_System_Power`.
- **Carried to phase 8:**
  - Batch export from the grid selection, as the Mac's batch sheet, with `xmp:Rating`.
  - The tray's tiles onto the GPU surface (`ui::tray::paint_tiles` is still on the
    device-context path).
  - Borders on developed thumbnails.
  - Saving the open photograph's thumbnail from its preview when it is left.
  - The last develop as a stand-in for strip thumbnails.
  - The Grid · Photo switch in the title bar and a View menu item.
  - Grid scrolling with inertia and a scroll bar.
  - The leader's anti-aliased slanted edge (it is a window region now).
  - A thumbnail with a brightness match takes about 5 s, mostly the 4 s reference
    step (with faster open).
  - A warning now also names `fonts.glyph`.

## 2026-11-19: phase 5 merged, the tray (branch tip `5eb7e04`)

- **What users see:**
  - Clicking the film or paper name under the print opens a tray along the bottom,
    where the strip and status line were.
    - The tiles are grouped, with each group's name written up the side, favourites
      first.
    - The stock being tried is ringed in pencil, and the stock the tray opened on says
      "before".
    - Stars mark favourites; click one to star or unstar.
  - Trying and keeping:
    - A click tries a stock, and ← → step through them.
    - Return, Done, or a click elsewhere keeps the stock, as one undo step for the
      whole visit. Esc goes back to the stock the tray opened on.
  - The foot: the group index, Search (films), the hint (cut in the middle, as on
    the Mac), the paper lock switch, and Done.
  - While a tried stock develops, its cached tile stands in on the canvas, turned like
    the photograph.
  - The Esc order: typing, then the tray, then the crop, then Image Only.
  - The paper lock and the stock names have the Mac's tooltips (Noah: "if non-invasive").
  - The inspector's Film/Paper rows and fold-out grids are gone, as on the Mac. The
    development-time rows moved to the top of Lab's Film and Print sections.
- **Gates:**
  - Mac cross-check at the warning baseline; 15 tray-rules unit tests pass.
  - PC `test-parity.cmd` 221/221. That includes the new
    `a_tray_visit_reaches_the_sidecar_once_when_kept`: no sidecar after tries and Esc,
    one write when kept, one undo step.
  - On the PC, scratch copies: Esc after 3 tries writes no `.negs`.
  - Screenshots, Darkroom and Daylight: `~/.negs/windows-parity/phase5/`. They match
    `04-film-tray.png` closely.
- **Decided (coordinator):**
  - Drop the inspector's Film/Paper rows, as on the Mac.
  - The canvas refit when the tray opens waits for phase 7's strip height.
  - Tile painting sits behind `ui::tray::paint_tiles`, for phase 7's painting decision.
  - A kept film change writes the sidecar a second time when its brightness match
    finishes, about 5 s later. That is the match's own save (any film change does it),
    not the tray, so it's accepted.
- **Mismatches left:**
  - The canvas refits when the tray opens (phase 7).
  - At a 1440 px window the foot's hint is cut to a stub ("clic…"). Hiding it when
    there's no room would read better (phase 7, with the strip height).
  - No "Scan / Your negative" group (phase 8).
  - Clicking a group jumps instead of a 0.25 s scroll.
  - Search is in Segoe UI with a grey placeholder.
  - The star's shadow is an outline.
  - Screen readers see only "Films"/"Papers".

## 2026-11-19: phase 6b merged, the library glue (`c421063`, branch tip `51c9a91`)

- **What users see:**
  - Each folder is a library. Edits live in `.negs` sidecars next to the photographs and
    are saved 1 s after the last change, on switching photographs, and at quit.
  - The Photo menu has Rating, Flag, Copy Edits (and Copy Edits…), Paste Edits,
    Paste from Previous, Duplicate, Reset Edits, Move to Recycle Bin (Del asks,
    Shift+Del doesn't), Delete Rejected Photographs…, Advance After Rating, and Filter.
  - Ratings and flags are keyed by the typed character, so AltGr layouts work.
  - Ctrl/Shift-click selects in the strip. Notes show in the status line.
  - Library undo shares the editor's stack. Undoing a step at once leaves the files
    byte-identical.
  - The Recycle Bin is used through `IFileOperation`, and undo puts the file back. A
    volume without a bin asks first and says the delete is permanent.
  - Other apps' writes show when the folder changes or the window is activated.
  - Exports carry `xmp:Rating`.
  - Settings › New photographs is now the Mac's Film and Paper pickers. The old "Keep
    the previous edits / Start fresh" choice is gone from the UI; it still drives
    photographs opened without a library.
  - The strip marks (stars, flag, cross) are minimal stand-ins until phase 7.
- **Gates:**
  - Windows suite 204/204. Crate `NEGS_LIBRARY_SEEDS=25` passes on Windows. Mapping
    tests pass 11/11.
  - All checklist items passed on scratch copies: no writes on open, the save debounce,
    undo byte-identical, the Recycle Bin and back (SHA-256 identical, `$I` records
    cleaned), the network share asking, other apps' writes, quit inside the debounce,
    New photographs.
  - **Mac↔Windows round trip:** a Mac fixture (crop, border, match, stars, pick) opens
    correctly on Windows. After an edit on Windows, the sidecar is byte-identical to the
    Mac dev app's for the same keys. The Mac opens it and leaves it untouched.
  - **Bug found and fixed on the PC:** serde_json without `float_roundtrip` read the
    catalog defaults one ulp off, so every Windows-edited sidecar also stored three
    defaults as changes. `float_roundtrip` is now on, and a mapping test covers it.
  - **Full startup ABBA after phase 6** (10 each, Defender quiet, the library opening
    on a scratch copy), medians in ms, main d056c64 → head:

    | Stamp | Base | Head |
    |---|---|---|
    | `first_window_composited` | 82.2 | 88.5 |
    | `inspector_shown` | 181.5 | 184.2 |
    | `gpu_ready` | 430.5 | 428.3 |
    | `first_image_on_screen` | 1175.3 | 1158.7 |

    The +6 ms is all in the DwmFlush wait; every stamp up to `window_shown` is within
    1 ms (vsync phase). Against main including phase 4, `first_image_on_screen` is not
    slower, so phase 4's +33 ms was noise.
- **Decided (coordinator):**
  - Rating, flag and delete act on the open photograph. Paste, Reset and Duplicate act
    on the strip's selection when it shows. No batch export until the phase 7 grid.
  - The library's new-photo defaults override the old Windows choice, as on the Mac.
  - Only fixed drives count as having a Recycle Bin.
  - A file too big for the bin: Windows asks, and Yes reports an error. Fine for now.
- **Found, not caused by 6b:**
  - Every unedited RAW waits about 4 s for the camera-brightness reference before it
    is placed, so a new RAW takes 7–8 s to appear on this PC. This is on main already.
    Look at it with faster open (ticket 39) in phase 8.
  - The strip's shell thumbnails make Windows write `Thumbs.db` into network folders.
    Phase 7 replaces those thumbnails.
- **Carried over:**
  - Settings has no switches for sidecars or RAW+JPEG pairing yet (both are on).
  - The strip watches its folder separately from the library's watcher (phase 7).

## 2026-11-19: phase 4 merged, the canvas (`8f5bdde`, branch tip `5726c97`)

- **What users see:**
  - The caption above the print (EXIF facts left, the file name right; the name goes first
    when the line is short; a portrait's caption is centred).
  - The label under it: "Film ⌄ printed on Paper ⌄". Each name opens the fold-out
    tiles, or a menu when previews are off or in Image Only.
  - The paper lock on "printed on". **Keep paper and the custom background settings are
    gone**, as on the Mac.
  - The Mac's 48/54/68 layout; the empty window's words are centred on the print.
  - Cropping: trim marks start it, a straighten dial with quarter-turn arrows, and the crop
    panel in the ink style (Cancel, yellow Done, "return applies · esc cancels").
  - F is Image Only, holding T shows the photograph upside down, and Esc leaves Image Only
    first.
  - Settings › Look › Viewer › Sheet: the theme's own plus the 7 sheets. The caption and
    label take the sheet's ink; the light table glow is contrast-gated against the sheet.
  - The exact fit (ticket 38): a fitted photograph is Lanczos-resampled in linear light,
    on the GPU or on a thread for the software canvas, after a 150 ms pause in sizing.
  - The window reopens at its last size, within 1 px at 125% (it used to drift a pixel
    per launch).
  - Phase 2 carry-overs: the status line and strip hide while a file is dragged in; the
    Developing tag's moving line works on the software canvas.
- **Gates:**
  - Mac cross-check clean. PC build passed; `test-parity.cmd` 203 passed, 0 failed.
  - Exact fit against Pillow LANCZOS in linear light: CPU mean |Δ| 0.0000/255, max 1;
    GPU (6016×4016 develop, three sizes) mean ≤ 0.054/255, max 1, detail ratio 1.0000–1.0001.
  - Startup ABBA, 8 each, medians in ms (base b360278 → head):
    `first_window_composited` 85.7 → 86.6, `inspector_shown` 181.8 → 179.5,
    `gpu_ready` 410.2 → 420.1, `first_image_on_screen` 1170.4 → 1203.7.
    Three head runs had `gpu_ready` ≈ 540 and pulled the last median up; the others
    were ≈ 1170, like base. Defender was scanning the freshly copied exes. To be
    settled by the full ABBA after phase 6.
  - Screenshots, Darkroom and Daylight: `~/.negs/windows-parity/phase4/`. Photo and crop
    views match the Mac's layout and type closely.
- **Decided (agent, accepted):** Rotate starts a crop; the hand cursor over the label's
  targets; the Developing tag on the theme background; `text.rs` replaced by DirectWrite;
  the exact fit only when fitted, settled, unrotated, not cropping or sizing, and
  zoomed out; the crop shade blends encoded colour as Core Animation does.
- **Mismatches left:**
  - No Flip in the crop panel: the Windows engine has no `io.flip` parameter, and that
    needs an engine change (Noah's call).
  - While cropping, the status line still shows the size and develop time; the Mac shows
    "Cropping · Original · 6016 × 4016 px" and dims Fit/100%. Carry to phase 7.
  - The window size can be 1 px off at fractional scales.
- **Decided by Noah:**
  - The paper lock gets a tooltip if it's non-invasive. Phase 5 adds it with the Mac's
    `ControlHelp.paperLock`/`paperKept` wording.
  - Flip comes to Windows. The engine already takes it (`spektrafilm-native`'s
    `IoParams::flip`, applied after the crop and before the turn), so the work is on the
    Windows side:
    - the `io.flip` edit state, mapped in the library glue;
    - the crop panel's Flip tick box;
    - a mirrored preview, and passing the flag to the engine.

    Added to phase 8.

## 2026-11-19: speed-ups agreed with Noah

Phases were slow: most of the time went to chasing startup noise, retaking
screenshots, polishing commits, and the single PC. Agreed:

1. **Phase 6 glue runs alongside phase 4.** It is built on the Mac with the cross-check
   and gets the PC only at the end, for its build and the Mac↔Windows round trip.
   Phase 4 owns the PC until then. Merge conflicts in `model.rs`/`ui/mod.rs` get
   resolved at merge.
2. **Startup gates only where launch code changes.** One full ABBA after phase 6.
3. **Leaner briefs:** screenshots in Darkroom plus one light theme, taken once;
   commits as they come, no history polishing; the Windows test suite only in phases
   4 and 8.
4. **Phases 4 and 5 stay separate agents,** so neither runs out of context.

## 2026-11-19: phase 3b, presets tiles and Settings (branch `win-p3b-presets-settings`)

- **What users see:**
  - Presets are tiles in two columns, as in `PresetInspector.swift`: the photograph
    developed through each preset, its name in Newsreader 15, its film, and "printed
    on" its paper (or "not printed · film scan"). The preset applied last to the
    photograph has the pencil ring (`applied_presets` in the model, by photograph, as
    the Mac's `appliedPresets`). A click applies, a drag sorts live, dropped files
    import before the tile under the pointer, and the foot says "click to apply · drag
    to sort · drop files to import" (or "drop to import" with the loose drop ring).
  - Empty: "No presets yet", the Mac's sentence with "New" upright, Import Presets….
  - Each tile's menu: Apply, Rename (F2), Export…, Show in Folder, Delete… (Del, the
    alert card). Arrows move between tiles; Enter or Space applies; Shift+F10 opens the
    menu. Each tile is a push button named by the preset, with "Applied last. film,
    printed on paper" as its MSAA value. Renaming is a field over the name with the
    pencil under it.
  - Only the tiles on screen and a row either side are developed
    (`Model::set_visible_presets`); the pane is still built on first show.
  - Settings is an owned, themed window (its own 38 DIP band, Close only), made on the
    first Ctrl+, and hidden by Esc, Ctrl+W or Close. Tabs as the inspector's:
    Photographs · Speed · Upkeep · Look, with the Mac's wording. Changes apply at once.
    It reopens on the last tab.
  - Look: the six themes as miniatures with the BOX ring on the chosen one and
    `AppTheme`'s hint under them.
  - Photographs › Films › Fotufilm films: All / From datasheets / None, stored as the
    Mac's `fotufilmFilms`. It filters the film menu, the grid and the film previews in
    the Windows app (no `crates/` change); the film in use stays listed.
- **Where the Windows settings went:**
  - Photographs › New photographs: Edits (Keep the previous edits / Start fresh) and
    Match to camera brightness. The Mac has Film/Paper and sidecar Edits here
    instead; Edits stays until sidecars (phase 6).
  - Photographs › Films: Fotufilm films, and Keep paper when changing film. The Mac
    replaced Keep paper with the paper lock (phase 4).
  - Speed: previews, Limit preview to 12 MP, Use less memory for export.
  - Upkeep: updates (automatic, Check Now with the last check), or "not configured".
  - Look › Viewer: the custom viewer background and its colour. **This is the slot
    for phase 4's viewer sheet picker**, which replaces it.
  - Left out until their features exist: RAW + JPEG, sidecar Edits, the thumbnail
    cache, Lightroom, Remove All App Data and Quit….
- **Startup**, one ABBA (main `aea6dff` vs branch, 6 runs, Defender idle: MsMpEng
  0 cpu-s in the 5 s before and after). `inspector_shown` medians 173.4 / 189.1 ms
  (+15.7). First window 77.5 / 78.5 ms; first image 1158 / 1150 ms. B was bimodal
  (170–175 and 203–206). An earlier 8-run ABBA in the same session gave 182.3 /
  180.1 ms (−2.2), so this is noise. Settings and the Presets pane are not built at
  launch.
- **Slider drag (3a's WARP path vs the pre-3a GDI path),** dragging Exposure with a
  temporary trace (not committed): no dispatch over 16.7 ms on either. The inspector's
  paints per mouse move are about 1.6 ms new against 0.8 ms old: trackbar 0.32 ms
  both, the value field 0.30 / 0.18, the rows 0.17 / 0.15, plus two 0.31 ms buttons
  (value box and reset). The film strip repaints on every move on both paths (2.6 ms):
  that's the largest cost, and it isn't 3a's.
- **Tests:** 190/190 on the PC with a scratch `NEGS_PROFILE_DIR`. The Mac cross-check
  is clean (the same 14 warnings as main).
- **Tooling:** `mac_shot.sh` has `shotfront` and `clickfront` (`winshot front`). In a
  `pc_shot` job, `"front": true` targets the app's frontmost window, and a `"drag"`
  step presses, moves and lets go. `shot.ps1` posts WM_CLOSE to the main window:
  `CloseMainWindow` hit a tooltip's SysShadow window while a tip was up.
- **Merged** after the coordinator's `12f5da4`. **Incident:** 3a's `test-parity.cmd` ran
  the suite without `NEGS_PROFILE_DIR`, and a test overwrote Noah's real
  `%APPDATA%\Negs\settings.json` (19 Nov 09:04; recent files became a temp file,
  everything else defaults). The earlier contents can't be recovered. Fixed:
  `test-parity.cmd` now sets a scratch profile, and test builds default to a temp
  profile in `session.rs`. Noah was told.
- **Decided (coordinator):** Keep paper goes when the paper lock lands in phase 4, as on
  the Mac. The strip repainting on every slider move goes to phase 7 (the strip
  rewrite), not before.
- **Mismatches left vs macOS:**
  - Keep paper and the Edits choice are in Settings until the paper lock and sidecars
    (Keep paper went in phase 4; the Edits choice goes with phase 6's glue).
  - The custom background is where the viewer sheet goes (replaced in phase 4).
  - The rename field types in Segoe UI 15 (GDI EDIT), with the Newsreader pencil
    under it.
  - Tiles and tab words show the focus box after a click once keyboard cues are on,
    as Windows does.
  - The theme swatches tick the chosen theme even when `NEGS_THEME` forces another.
    This only happens in the harness.
  - Scrolling with many presets and the "can't be read" tile were not shot on the PC.
- Screenshots: `~/.negs/windows-parity/phase3b/`, with the Mac Settings references in
  `phase3b/mac/`.

## 2026-11-19: phase 3a merged (branch tip `786d7f8`)

- **What users see:** the Mac's inspector.
  - Pane header: Adjustments · Presets as words, with the edit count and Reset.
  - Histogram · Waveform as words. The light-theme histogram is now added up and laid
    over the well, as the Mac's is.
  - Prep · Lab · Output in Newsreader 21 with per-tab counts. A count opens a menu with
    Reset for each edit and Reset Tab.
  - Every row style. Changed values are 700 paper with the pencil under them.
  - "N more" lines, with View › Show All Controls in place of Advanced.
  - The focus box. The Export foot, on Output only.
  - MSAA names, values and roles were checked.
- **Painting:** every Canvas paint now goes to a single-threaded WARP device, created on
  its own thread at launch. The old Direct2D DC path is the fallback. Screenshots of
  the inspector are pixel-identical between the two paths.
- **Startup:** the cause is confirmed. Direct2D's worker threads are starved while the
  engine starts: delaying the background threads by 400 ms closed the gap to +3 ms.
  ABBA medians for `inspector_shown`:
  - quiet PC: +4.9 ms (199.3 / 204.2);
  - Defender busy after a reboot: +42.6 ms (186.7 / 229.3).

  First window +2.6 to +7.5 ms. **Noah accepted up to +40 ms for now; recheck after
  phase 6.** Next options:
  - text through `IDWriteBitmapRenderTarget` with no D3D at all;
  - engine thread priorities during startup (needs Noah's OK).
- **Tests:** 185/186 on the PC. The one failure was a "graphics driver reset" in a GPU
  export lifecycle test this branch doesn't touch; the 40 lifecycle tests re-run all
  pass. An earlier run had 186/186. The Mac cross-check is clean.
- **"Struck tab":** macOS never strikes a tab. It strikes Print and its modules while
  scanning (Windows matches), and Camera and Grain/Halation/DIR for a negative scan.
  That part comes with "Your negative" in phase 8.
- **Mismatches left vs macOS:**
  - the tone graph's segmented buttons;
  - the border samples, still GDI;
  - the rounded Longest side field;
  - value-box text sits high while typing;
  - no Input Colour summary;
  - the scroll bar is always shown;
  - wheel up lowers a slider's value (as before).
- Screenshots: `~/.negs/windows-parity/phase3a/`.

## 2026-11-18: phase 2 merged (`a9a0763`, branch tip `171cd81`)

- **What users see:** six themes live (View › Theme, saved as `appTheme`, Darkroom
  default; Plain follows Windows light/dark; `NEGS_THEME` forces a name, light/dark =
  Plain/Plain dark). 38 DIP title bar (Direct2D): menu row File Edit Photo View Help,
  file name Newsreader 15/600, themed caption buttons, snap layouts kept. No toolbar or
  canvas pill (`chrome.rs` deleted); items in menus (shortcut map in
  `docs/windows-ui-features.md` / release notes). Status line in words (size, develop
  time, export progress + Cancel, Original ⌄ hold/menu, Fit, 100%). Alert card replaces
  task dialogs (About stays native). Empty window, dropping, Developing tag, can't
  develop on the canvas (`Model::develop_error`).
- **Gates:** cross-check clean (14 old warnings); PC build; 182 Windows tests pass.
  Startup ABBA 6 runs: A 86.4 / 181.1, B 81.9 / 176.4 (first window / inspector). Fixed
  on the way: `d2d::prepare` now starts at the top of `run()`, `d2d::warm` sets the first
  frame's type styles on that thread.
- **Decided (coordinator):** no duplicate View › Reset All Adjustments (Windows has one
  reset, Photo › Reset Edits Ctrl+Shift+R); the big shell commit `0836731` stays whole.
  Rotate/Crop/Fit/Actual in View as on the Mac; Crop shows Ctrl+K, C works too.
- **Not exercised on the PC:** runtime theme switch from the menu, the drop state, Try
  Again, holding Original, keyboard on the confirmations. 150 % not testable per run
  (PC is at 125 %, system-wide).
- **Carried over:** phase 3: inspector controls (still old painting in theme colours),
  Export foot, a stray glyph left of "No developed photograph" in the empty histogram.
  Phase 4: film → paper label (dropped from the status line), empty-window words centred
  on the print area, whether the status line hides while dragging (match macOS), the
  Developing tag's line is still on the software canvas. Phase 6: library items in Photo
  and the status line's library notes; folder wording in the empty window.
- Screenshots: `~/.negs/windows-parity/phase2/`.

## 2026-11-18: phase 6 crate merged (branch tip `7718029`)

- **What landed:** `crates/negs-library` (no dependencies; workspace member; `Cargo.lock`
  gains only its own entry). Exact JSON (JSONEncoder's `.prettyPrinted` `" : "`, sorted
  keys, Swift's `Double.description` incl. ties-to-even; JSONDecoder's quirks), sidecar
  read/patch/atomic write, edits as the sidecar's own JSON, folder listing in Finder order
  with RAW+JPEG/HEIF pairs, filter + wording, selection, delete plans + `Bin` trait,
  thumbnail looks/keys (CryptoKit goldens), xmp:Rating embedding (PNG/JPEG/TIFF), and a
  `Library` with actions returning an `Outcome` (the Mac's note text) and byte-exact undo.
  API and the Swift each part mirrors: `crates/negs-library/README.md`.
- **Gate (crate part):** `cargo test --release -p negs-library`: 48 pass; every fixture
  reads and rewrites to the Mac encoder's bytes (`tests/mac-encoded/`);
  `NEGS_LIBRARY_SEEDS=25` random-action test passes (1.3 s; 200 × 200 also passes).
  Re-run by the coordinator before merging.
- **Decided (coordinator):**
  - `docs/sidecar-fixtures/` stay as they are (hand-written `": "`); the crate checks
    that spacing is the only difference. Not worth touching shared fixtures.
  - Undoing a rating/flag after another app's write restores only the fields the action
    changed (keeps the other app's change). Safer than restoring the whole summary.
  - Case-insensitive sidecar names on Linux: decide in the Linux port, not now.
- **Left for the Windows glue (phase 6 proper):** Recycle Bin `Bin` (IFileOperation),
  folder watcher + refresh on activate, 1 s save debounce, one undo stack interleaving
  editor steps with `library.undo()`, brightness-match inputs, `EditDefaults` from the
  catalog, settings persistence (filter, copied groups), view restore after undo,
  batch-export naming, `xmp::embed_in_file` after exports, and making `change_edits`
  public for whole-roll actions. Then the Mac↔Windows sidecar round trip.

## 2026-11-18: phase 1 merged (`e142fba`, branch tip `fe662f4`)

- **What landed:** `windows/marks/` (crate `negs-marks`, no Win32: pencil, rings,
  `HandStar`, `RejectCross`, `PickBox`, Plain variants; 16 tests against the Swift's
  numbers, `cargo test --manifest-path windows/marks/Cargo.toml --target-dir target/marks`).
  `ui/d2d.rs`: software `ID2D1DCRenderTarget` into the GDI back buffer via
  `Canvas::d2d(scale, plain, |p| …)`, cached text formats/layouts/geometry, lost-target
  redraw; Newsreader from memory (`IDWriteFactory6` axes on 1803+, named instances on
  1703–1709, Segoe UI before). `ui/theme.rs`: `ThemeName` + token tables for all six
  themes, checked against `AppTheme.swift` by script. `ui/sampler.rs`:
  `NEGS_DRAW_SAMPLER=<theme>` draws every primitive. The visible app is unchanged.
- **Startup** (ABBA, 6 runs, A = main, B = branch): first window 90.4 / 88.4, inspector
  184.4 / 181.0, GPU ready 426 / 429, photograph 1191 / 1182. Within noise.
  D2D + DWrite + fonts cost ~2.5 ms, made on a `negs-drawing` thread right after
  `inspector_shown` (`drawing_ready` stamp).
- **Plain on Windows:** follows Windows' light/dark (or `NEGS_THEME`), Segoe UI Variable
  where present, else Segoe UI. Cheap (~20 lines), kept.
- **Decided (coordinator):** the Barlow Condensed OFL notice now ships with the Windows
  package too (it was already compiled in through `spektrafilm-io` but uncredited); a
  licence requirement, not a design choice.
- **Carried over:** the marks' halo is two translucent strokes (a real blur needs a
  device-context target: phase 7). Highlighter uses normal blending, not multiply. The
  1703–1709 font path is untested. CI doesn't run `negs-marks` tests (needs a
  workflow edit; do it in phase 9).
- Screenshots: `~/.negs/windows-parity/phase1/`.

## 2026-11-18: phase 0, baseline

- **Tooling** (`windows/tools/`, see its README): `pc_console.sh` runs PowerShell in
  Noah's console session through a scheduled task; `pc_shot.sh` + `pc/shot.ps1` launch,
  drive (SendKeys, clicks) and capture the Windows app's window; `pc/startup.ps1` gives
  ABBA-interleaved startup medians with windows closed automatically; `mac_shot.sh`
  captures an isolated copy of the macOS dev app (System Events clicks; CGEvents posted
  from here don't reach SwiftUI). `C:\Users\Noah\.negs\build-parity.cmd <branch>`
  fetches from the Mac and builds in the new `C:\Users\Noah\Projects\negs-parity`
  worktree (cold build 1 min 55 s).
- **Startup baseline** (`startup.ps1`, 6 runs + warm-up, `docs\card.jpg`, medians in
  ms; two builds of `83e8c79`, interleaved): first window composited 88.3 / 86.3,
  inspector shown 182.2 / 183.5, GPU ready 438.6 / 432.4, photograph on screen
  1211.4 / 1205.5. These are lower than `docs/windows-app.md`'s 106/257 because this
  harness and machine state differ; **compare only against runs of this harness**, A/B
  interleaved with a `main` build. Gate: first window and inspector within ~10 ms.
- **Reference photographs:** eight D750 NEFs (`_NHA9786` … `_NHA9810`), at
  `C:\Users\Noah\.negs\photos\ref` and `~/.negs/windows-parity/photos`.
- **Reference screenshots** in `~/.negs/windows-parity/ref/`: `windows-0.17.0/`
  (dark and light: empty, Prep, Lab, Output, film fold-out, crop, presets, File menu,
  Settings) and `macos-main/{darkroom,daylight,plain}/` (Prep, Lab, Output, film tray,
  crop, presets, grid, Image Only). macOS Settings is a separate window and not
  captured yet.
- **Mac Screen Recording** had to be granted to Rex (the host app) for `screencapture`;
  it took effect without a restart.

## 2026-11-18: survey and plan

- **Windows `main` worktree:** `negs-presets` was fast-forwarded to `83e8c79` from
  GitHub. It built cleanly there: helper, both exporters and `Negs.exe`, with LTO off
  and 16 codegen units, in about 2 min for the Windows app.
- **Mac cross-check:** the `x86_64-pc-windows-msvc` cross-check of `windows/` works
  (41 s cold).
- **PC to Mac:** `git fetch mac main` works from the PC.
- **Decisions:** Noah's five answers are in the plan's "Decided" section. The
  coordinator chose wgpu for the strip, grid and tray tiles (to be confirmed by
  measurement in phase 7).
- **Phase 0 still to do:** re-measure startup, write the scheduled-task screenshot
  script, and take reference screenshots of both apps on the same photos (empty window,
  photo with each tab, crop, presets, settings, light and dark).
