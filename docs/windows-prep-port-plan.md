# Plan: porting the macOS Prep-era features to Windows

**Status (2026-11-03):** done on branch `windows-prep-port`, not released. Decisions: GPU
Prep deferred; Post edits, Start fresh and the Highlights range follow the Mac. What was
built and how it was checked: `docs/windows-app.md`, "Prep, Lab basics and camera
brightness". Built ahead of the UI polish pass rather than after it (Phase B below), so the
two branches will conflict in `ui/inspector.rs`.

Prepared 2026-11-03 on `windows-ui` at `a169713`. `origin/main` is now `6ec07ac`
(the Windows tone graph merge), so the Mac sources are the same.
Survey range: the 17 macOS commits from `fafda8f` (Prep import) to `6b4d456`
(faster Prep), all after Release 0.13.0 and none in a release note yet
(`release/notes/unreleased.md` is empty). The tone graph (`d351348`) is already
ported (`e38d1b5`).

## What is already shared

Windows drives the same `spektrafilm-native` helper as the Mac, so most of the engine
work arrived with the merge and only needs the app to use it:

- `prep.*` params: validated on read, in the catalog defaults, so Windows
  sessions, presets (Prep kept like Post), Reset and undo already carry them.
  Windows renders Prep. It has no controls for it yet.
- The prepared-frame cache (`Pipeline::with_prepared_frame`) lives in the helper and
  works on Windows (measured below).
- `place_exposure` (camera brightness) is a helper command. The app supplies the
  reference image.
- The tone graph's film histogram already counts Prep (`0152ae0`).
- The Apple RAW 9 → 8 → rawler fallback is macOS only. Windows already uses rawler.

## Feature survey and Windows gap

| # | macOS feature (commit) | Windows today | Port work |
|---|---|---|---|
| 1 | **Prep tab**: Tone (Highlights −180…100, Shadows, Contrast, Lift, Gain), Color (Color Boost, Saturation), Detail (Midtone Detail), Exposure (EV) (`fafda8f`) | none | rows + model paths |
| 2 | **Prep / Lab / Output** tabs; Prep opens first; Post renamed Output (`124a4ac`, `afb5052`) | Lab \| Post | 3-way switch; migrate `inspector_tab` "Post" → "Output" |
| 3 | White balance, lens corrections and input colour move Lab → Prep (`fc887dd`) | in Lab | move rows |
| 4 | Output keeps only Image Size and Border; Post Tone/Saturation removed; restored sessions reset `post` to neutral (`d84ee33`) | Post has Brightness/Contrast/Whites/Blacks/Saturation | remove rows + session migration |
| 5 | Film and paper pinned above the tabs; stock grid opens as a fixed 290 pt scrolling window (`740c537`) | stocks inside Lab | move pickers out of the scrolling rows |
| 6 | Scan film directly at the top of Print; paper row reads "Skipped – scanning film" (`740c537`) | elsewhere | move + label |
| 7 | Lab exposure renamed **Negative exposure** (`afb5052`) | "Exposure" | rename |
| 8 | **Lab basics + Show all controls** (per-group `basic` count; changed settings always show) (`d84ee33`) | all rows | new `When` predicate + toggle pref |
| 9 | **Film format** menu: 110 … 4×5 writing the long edge, Custom… shows the slider (`b3a3deb`) | slider only | choice row + "Custom kept" model state |
| 10 | **Settings › New photographs**: Keep previous edits / Start fresh (default **Start fresh**) (`e5aa243`, `d84ee33`) | always keeps | pref + open path |
| 11 | **Limit preview to 12 MP** and **Use less memory for export** become app preferences, outside undo/reset (`fc887dd`) | per-photo, in Lab › Output | move to Settings; adopt old per-photo value once |
| 12 | **Match camera brightness**: a per-photo live switch, re-solved whenever the developed brightness changes; Exposure becomes an offset on top; notes for failures; Save/Export wait; letterbox trimming; "Match original brightness" for JPEG/PNG/TIFF; Settings › Match new RAWs (default on); Auto exposure/Metering move to Prep and hide while matched (`fafda8f`…`b23b496`) | none | biggest item, see below |
| 13 | **Scopes**: RGB histogram + RGB waveform switch, resizable 88–420 pt (default 88, remembered), honour crop/straightening, exclude borders, off the main thread (`fafda8f`, `124a4ac`, `8e4055c`, `740c537`) | fixed 88 px histogram | waveform, resize grip, rotation-aware sampling |
| 14 | **Slider fill from default** (`124a4ac`) | stock trackbar | already in the polish pass (owner-drawn fill from default) |
| 15 | **Tooltips for every control and group**, texts in `ControlHelp.swift`, test for coverage (`6f14361`) | ~15 tooltips | port table + coverage test |
| 16 | **Favourite films and papers**: star on tiles, favourites lead the grid, render first; plain menus list them on top (`d89db31`) | none | prefs + grid sections + star hit-target |
| 17 | Faster Prep: parallel guided base, kept prepared frames (`6b4d456`) | in helper | nothing; measured below |

## Quality and performance checks run today (this PC: Ryzen 5 3600, Windows 10 19045, Vulkan)

**Builds and tests on `windows-ui` (`a169713`)**: release helper and Windows app build;
Windows app 101/101 tests; core + native (release) 186/186. Clippy (newly installed)
finds 57 warnings in `negs-windows`: style lints (19 collapsible `if`s) and dead
code (`render_backend_gpu`, `session_store`, `finish_rgb`, fields never read).
None is a bug. The "dangling pointer" lint is an intentional `MAKEINTRESOURCE`-style
cast. Worth a separate small cleanup, not part of this port.

**Prep through the helper, as the Windows app drives it** (24 MP D750 NEF, Portra
400 → Endura, median of 6; harness: the scratchpad `prep_bench.py`):

| Render | 12 MP limit | Full size |
|---|---|---|
| Settled, print exposure, Prep neutral | 544 ms | 1181 ms |
| Settled, a Prep control changes (Highlights / Midtone Detail) | 864 / 851 ms | 1775 / 1801 ms |
| Settled, print exposure with Prep on (prepared frame kept) | 557 ms | 1214 ms |
| Drag @0.35, print exposure | 75 ms | 122 ms |
| Drag @0.35, Highlights | 109 ms | 195 ms |
| Drag @0.35, print exposure with Prep on | 70 ms | 120 ms |
| `place_exposure` (camera brightness solve) | 284 ms | — |

So Prep's own controls cost +35 ms (12 MP) / +73 ms (full) per drag frame and
+0.3 / +0.6 s per settled render. Lab edits with Prep on cost nothing extra
(the cache works). The live brightness match costs ≈ 0.3 s of helper time after the
settled preview, once per open or look change, which is affordable.

**GPU Prep (`mac/feature/prep-gpu`, `bad9d21`)**: as shipped on that branch it is
macOS only (`supports_prep_core` = `cfg!(target_os = "macos")`). Windows renders are
bit-identical to main and equally fast. Its Prep tests pass here (9/9). Turning the
gate on in a scratch build (Vulkan):

- Highlights drag 109 → 75 ms (12 MP), 195 → 127 ms (full). Settled Prep change
  864 → 579 ms (12 MP). Prep's controls become free, as on the Mac.
- Pixels vs CPU Prep, 8-bit preview: grain off, max 1 code on 0.004% of bytes,
  the Mac's rounding level. Grain on, 0.5% of pixels differ by 1 code and
  ≈ 0.02% by 3–16 codes, scattered through the midtones: the rounding moves a few
  grain samples. Not visible as such, but not "bit for bit" either, and the
  Reference export (CPU) stays the reference.
- Not checked: other GPU vendors (only this AMD card), the CPU fallback, the
  golden matrix.

## Plan

Most of this is in `windows/src/ui/inspector.rs`, which the **UI polish pass**
(`claude/dazzling-euclid-d631bd`, `docs/handoff-windows-ui-polish.md`, in progress,
uncommitted `draw.rs`/`theme.rs`) is rewriting. That handoff keeps `rows()` and the
model calls stable "so a later Prep port only adds rows", and already contains the
slider fill from default (item 14) and a segmented control for the tab switch.
So: do model-side work now, inspector work after the polish lands. Port behaviour,
not look. Follow `docs/windows-ui-features.md` conventions (Win32 controls, DIPs,
accessible names, keyboard, no Ctrl+Alt shortcuts).

**Phase A: model and prefs, no inspector changes (can start now, parallel to polish)**

1. Prefs: `fresh_edits_for_new_photos` (default true), `match_new_raws` (default
   true), `limit_preview` and `serial_grain` as app prefs (adopt a restored
   session's per-photo value once, as `EditorModel` does), `lab_all_controls`,
   `scope_mode`, `scope_height`, `favorite_films`, `favorite_papers`.
2. Open path: Start fresh resets Prep/Lab/Post, white balance and border, keeping
   film/paper, Image Size, output colour. Crop, rotation and lens reset either way.
   Session restore: `post` → defaults, `inspector_tab` "Post" → "Output".
3. Film format: `FILM_FORMATS` table (17/24/35/56/70/84/120 mm) and the
   "Custom kept" flag with the Mac's Start fresh / Keep / Reset semantics.
4. Camera brightness, model side (port of `EditorModel` §Camera brightness):
   `BrightnessMatch { ev, inputs }` in the edit state; `prep.exposure_ev` keeps the
   total; one solve in flight, after the settled preview; apply only if inputs are
   still current, without its own undo step; failure notes per photo vs per inputs;
   Save/Export wait for a pending match. Unit tests mirroring
   `macos/Tests/NegsTests/ExposurePlacementTests.swift`.
5. Reference image in Rust: the embedded preview through rawler (already used by
   `thumbnail.rs`), ≤ 512 px, letterbox trimming (port of
   `CameraBrightnessReference.trimmingBars`), the Mac's colour checks (EXIF
   ColorSpace = sRGB or a named ICC profile), written as tagged PNG. For
   JPEG/PNG/TIFF the WIC decode in `thumbnail.rs`. Best put in a shared Rust
   module so Linux and the web can reuse it.
6. Scopes: waveform in `histogram.rs` (bounded sampling, quarter-turn aware,
   crop/straightening, border excluded), computed on the existing worker, not the
   UI thread.
7. Tooltip table: port `ControlHelp.swift` to a Rust table keyed by param path,
   plus a test that every row's path has a text and that the key sets match the
   Swift file (so the two can't drift).

**Phase B: inspector, after the polish pass merges**

8. Pinned area: scope (resizable grip, RGB/Waveform dropdown), film and paper
   pickers (290 DIP scrolling grid opening on the current stock), Prep | Lab |
   Output switch. Rows below scroll.
9. Prep rows: WB, Brightness (Match switch, Exposure offset, note, Auto
   exposure/Metering with the Mac's show rule), Tone, Color, Detail, Lens / Input
   Colour. Lab: Negative exposure, Film format, Scan film directly at the top of
   Print, Show all controls with `When::Basic` from the Mac's per-group `basic` counts.
   Output: Image Size, Border. Settings dialog: New photographs, Match new RAWs,
   Limit preview, Use less memory for export.
10. Tooltips on every row and group header. Favourite stars on tiles (owner-drawn
    hit-target with an accessible name and a keyboard path, e.g. a context-menu item
    "Add to Favourites"), favourites heading first, rendered first.

**Phase C: validation**

11. Windows tests and release build, core/native tests; startup medians via
    `measure_startup.ps1` (no regression > 10 ms, as the polish pass requires);
    drag/settled timings with the harness above against the table here; tab-switch
    time (the Mac's open item: 110–305 ms) measured on Windows; camera brightness
    on the RAW samples from `docs/raw-compatibility-2026-11-01.md` (EV within the
    Mac's matches, letterboxed files included); screenshots; limitations recorded in
    `docs/windows-app.md` and `docs/prep.md` §Open.

Phases A and B are one sequence of commits on one branch: A's steps share
`model.rs`/`session.rs`, B's share `inspector.rs`, so parallel sessions would collide.
The only piece that stands alone is step 5 (reference extraction).

## Decisions for Noah

1. **GPU Prep on Windows.** It makes Prep's controls free on Vulkan (drag 109 → 75 ms,
   settled −0.3 s at 12 MP), at rounding-level difference that grain turns into a few
   changed grain specks. The branch isn't merged even for the Mac. Options:
   (a) ship Windows Prep on the CPU now (already ≈ the Mac's pre-GPU cost) and decide
   GPU Prep for all platforms later. (b) Merge `feature/prep-gpu` with Windows
   enabled, which needs an NVIDIA/Intel check and the golden matrix. Recommend (a).
2. **Existing Windows users' Post edits.** Following the Mac resets restored
   `post.brightness/contrast/whites/blacks/saturation` to neutral on update, so a
   photograph left with Post edits changes look on relaunch. There is no exact
   mapping to Prep (Post acts after the film, Prep before). Options: follow the Mac
   (recommend; mention it in the release notes), or keep the Post rows on Windows
   until users have moved on.
3. **Start fresh as the new default** changes what Windows users have always had (edits
   carried to the next photograph). Follow the Mac? Recommend yes, so both platforms
   ship 0.14 alike.
4. **Highlights range** (−180…100, 0 at 64% of the track) is still open on the Mac.
   Windows will copy whatever is decided. Until then it ports as is.
5. **Release shape.** One Windows release with all of this (the inspector changes
   shape completely), or Prep controls first in today's Lab | Post layout and the
   reorganisation later? Recommend one release, after the polish pass, so users
   relearn the column once.
