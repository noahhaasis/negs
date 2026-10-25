# Windows app: what to carry over from macOS

This is the feature list of the macOS app (`macos/Sources/NegsApp`, up to
`fe01386` on main), written for the Win32 port. It answers one question: what
does a Negs user get, and what does a Windows user expect instead?

**Port the behaviour, not the look.** Everything under *Behaviour* is the
contract: same parameters, same results, same edge cases, same undo steps.
Everything under *On Windows* is a recommendation that follows Windows
conventions (Win32 and the common controls, light mode, as decided in
`docs/cross-platform-port.md`). Where the Mac app does something only because
it is a Mac, this list says so and leaves it out. Items marked **Ask Noah**
need a decision before building them.

Read alongside: `docs/native-macos-ui.md` (design and startup notes),
`docs/windows-agent-handoff.md` (rules, machines, engine state), and the Swift
sources named in each section.

Contents: 1 Window · 2 Menus and shortcuts · 3 Toolbar · 4 Opening ·
5 Film strip · 6 Canvas · 7 Show Original · 8 Inspector · 9 Crop ·
10 Borders · 11 Save Preview and Export · 12 Undo · 13 Status, progress and
errors · 14 Quitting and the session · 15 Settings, updates, About ·
16 Engine processes · 17 Startup · 18 Accessibility · 19 Not to port ·
20 Open decisions.

---

## 1. Window and layout

`EditorWindowController.swift`, `NegsApp.swift`

Behaviour:

- One editing window, one photograph at a time. Closing the window quits.
- Columns: the canvas (min width 680) and a trailing adjustments column
  (300–440 wide, resizable, collapsible). Below the canvas: the film strip
  (optional) and the status bar.
- Title is the file name, or "Negs" with nothing open. A subtitle shows
  `Film → Paper`, or only the film when scanning film directly.
- Window frame and column widths persist across launches.
- An empty state with nothing open: "Open a Photograph", a short tagline, an
  **Open Photograph…** button, "Or drop a RAW, TIFF, PNG, or JPEG file here",
  and up to 5 recent files as links (tooltip: full path).
- The whole canvas is a drop target for a file; it shows a highlight
  outline while a file is dragged over it.

On Windows:

- Title bar text `DSC_1234.NEF - Negs` (document first, then app, as Windows
  apps do). Windows has no subtitle: show `Film → Paper` in a status-bar part.
- Persist `GetWindowPlacement` (including maximized) and the column width.
  Restore onto a visible monitor if the saved one is gone.
- Splitter between canvas and adjustments: a drag bar; dragging it closed
  hides the adjustments, like the toggle.
- Drop: `IDropTarget` (or `DragAcceptFiles`), accept the first file, ignore
  folders. Optional: F11 full screen, which Windows users expect from image
  viewers.
- Per-monitor DPI awareness v2. Layout in DIPs; re-layout on `WM_DPICHANGED`.

## 2. Menus and keyboard shortcuts

`MainMenu.swift`

Every command is enabled or disabled from the model's state, and some
titles change (see below). The Mac app menu (About, Services, Hide, Quit)
and Window menu are macOS conventions: drop them.

Suggested Windows menu bar (with `&` access keys):

| Menu | Item | Mac key | Windows key | Enabled when |
|---|---|---|---|---|
| File | Open Photograph… | ⌘O | Ctrl+O | always |
| File | Recent Photographs ▸ (up to 10) | — | — | list not empty |
| File | Save Preview… | ⌘S | Ctrl+S | preview current, not saving, not cropping |
| File | Export Photograph… | ⇧⌘E | Ctrl+Shift+E | photo open, engine ready, not exporting, not cropping |
| File | Export LUT… | ⇧⌘L | Ctrl+Shift+L | catalog loaded, not saving/exporting/cropping; no photo needed |
| File | Settings… (Mac: app menu) | ⌘, | — | always |
| File | Exit (Mac: Quit / Close) | ⌘Q / ⌘W | Alt+F4 | always |
| Edit | Undo *action* | ⌘Z | Ctrl+Z | something to undo |
| Edit | Redo *action* | ⇧⌘Z | Ctrl+Y (also Ctrl+Shift+Z) | something to redo |
| Edit | Cut / Copy / Paste / Delete / Select All | ⌘X… | Ctrl+X… | forwarded to the focused edit control |
| Edit | Reset All Adjustments (Mac: View menu) | ⇧⌘R | Ctrl+Shift+R | catalog known, not cropping |
| View | Film Strip (check) | ⌥⌘F | F6 | always |
| View | Adjustments (check) | ⌥⌘I | F7 | always |
| View | Fit Photograph | ⌘0 | Ctrl+0 | photo open, not cropping |
| View | Actual Size | ⌘1 | Ctrl+1 | photo open, not cropping |
| View | Zoom In / Zoom Out (new) | — | Ctrl+= / Ctrl+- | photo open, not cropping |
| Image | Rotate Clockwise (Mac: View) | ⌘R | Ctrl+R | photo open (also while cropping) |
| Image | Crop / Done Cropping (Mac: View) | ⌘K | C, or Ctrl+K | cropping, or a developed image exists |
| Help | Check for Updates… (Mac: app menu) | — | — | updates configured |
| Help | About Negs | — | — | always |

Notes:

- **Avoid Ctrl+Alt shortcuts.** On German and many other layouts Ctrl+Alt is
  AltGr, which types characters. That rules out ports of ⌥⌘F and ⌥⌘I; F6/F7
  follow Lightroom's panel keys. Pick a crop key with Noah (**Ask Noah**: Mac
  uses ⌘K; Windows photo editors use C or R; Ctrl+K usually means "link").
- Undo and Redo titles carry the action name ("Undo Change Film"); see §12.
- The Mac renames "Hide/Show Film Strip" and checks "Show Adjustments".
  On Windows a checked menu item for both is the convention.
- The Recent submenu: numbered `&1 DSC_1234.NEF` …, full path as the status
  text or tooltip. A missing file is removed when chosen, with an error
  (§4).
- Use an accelerator table, so shortcuts work without opening the menu.
  Shortcuts must not fire while an edit control (a value being typed, the
  crop ratio, the export pixels) has focus, apart from Ctrl+Z inside the
  field, which the edit control owns.

## 3. Toolbar

`EditorWindowController.swift`

Behaviour: icon-only toolbar with tooltips that name the shortcut.
Left: Open, Film Strip (accent-tinted when shown). Centre: Fit, Actual Size;
Rotate, Crop (accent-tinted while cropping). Right: Export, Adjustments.
While cropping, Export is replaced by **Cancel** and **Done** (Done is the
prominent/default button). Enabled states as in the menu table.

On Windows:

- A Win32 toolbar (`TOOLBARCLASSNAME`, flat, with tooltips) under the menu,
  or a thin command bar. Toggle buttons (`BTNS_CHECK`) for Film Strip,
  Adjustments and Crop show their state the Windows way, as pressed buttons,
  instead of a tint.
- Crop's Done/Cancel: on Windows, put them at the bottom of the crop panel
  (§9), Done as the default push button, Cancel next to it; Enter and Esc
  work anywhere in crop mode. Keep them in the toolbar too only if it looks
  right.
- Icons: Segoe Fluent Icons (Windows 11) / Segoe MDL2 Assets (Windows 10)
  glyphs, drawn at the current DPI, or bundled icons. SF Symbols don't exist.

## 4. Opening photographs

`EditorModel.swift` (`openPanel`, `open`, `thumbnail`), `NegsApp.swift`

Behaviour:

- Supported: PNG, TIFF, JPEG and RAW. RAW extensions: dng cr2 cr3 nef nrw arw
  srf sr2 raf orf rw2 pef ptx srw x3f iiq 3fr ari bay crw dcr drf erf fff k25
  kdc mef mos mrw rwl (`EditorModel.rawExtensions`; the helper decides what
  it can decode).
- Ways in: the Open dialog, drag and drop, the film strip, Recent, Finder
  "Open With" (registered as an alternate viewer, never the default), and a
  path argument on the command line.
- The Open dialog starts in the current photograph's folder with that file
  selected.
- Opening a file: clears undo history, crop, rotation, the shown original,
  and the histogram; keeps all adjustments, film, paper and border; fits the
  view; adds the file to the top of Recent (max 10, no duplicates).
- An unreadable or missing file: error "“name” can’t be opened. It may have
  been moved or deleted." and it leaves Recent.
- At once, a quick thumbnail is shown at the photograph's full layout size
  (RAW: the embedded preview; others: a downsampled decode with EXIF
  orientation), with a **Developing** badge until the first render arrives.
  The canvas does not jump when the render replaces it.
- An explicit open at launch wins over restoring the last session (§14).

On Windows:

- `IFileOpenDialog` with filters "Photographs (*.nef;*.dng;…;*.jpg;*.png;*.tif)",
  one entry per kind if useful, and "All files (*.*)". `SetFolder` to the
  current photo's folder and `SetFileName` to its name. Use its own
  `SetClientGuid`, separate from the Export dialog's, so Open and Export
  remember their own folders (the Mac had to fix exactly that, `ae22869`).
- **Single instance.** Double-clicking a photo in Explorer while Negs runs
  starts a second process on Windows. Forward the path to the running window
  (named mutex + `WM_COPYDATA`, or a named pipe), bring it to the front
  (`AllowSetForegroundWindow`), and exit. macOS does this for free.
- File association: the installer registers a ProgID and adds it to
  `OpenWithProgids` and `Capabilities\FileAssociations` for the extensions
  above, without taking the default (the Mac's `LSHandlerRank = Alternate`).
- Recent: also call `SHAddToRecentDocs` so the taskbar Jump List shows recent
  photos (needs an AppUserModelID and the registration above).
- Placeholder thumbnail: WIC (`IWICBitmapDecoder`, scaled via
  `IWICBitmapSourceTransform`, honour the EXIF orientation). RAW without a
  WIC codec: the embedded JPEG preview (from the helper or a small Rust
  function), or nothing. Never a full RAW development just for this.

## 5. Film strip

`PhotoBrowser.swift` (`fe01386`)

Behaviour:

- A horizontal strip under the canvas (≈112 pt high) with every supported
  photo in the current photo's folder: thumbnail (96×68, aspect fit) and
  middle-truncated file name. The current photo is outlined in the accent
  colour and scrolled to the centre.
- Click a thumbnail to open it. Left/Right arrows (strip focused) open the
  previous/next photo.
- Order: natural sort (Finder order: `IMG_2` before `IMG_10`). Hidden files
  are skipped. No folder watcher.
- Thumbnails decode lazily for visible cells, one at a time, from a bounded
  cache (~16 MB / 160 images), and are dropped when they scroll away.
  RAW uses only embedded previews.
- States: spinner while scanning; "This folder couldn’t be read."; "No
  supported photographs in this folder."
- Disabled while cropping. Visibility persists (default: shown).

On Windows:

- A `WC_LISTVIEW` in icon view with a single row and a virtual item list, or
  a small custom-drawn strip (the cross-platform plan allows custom drawing
  here). Keep it cheap at launch.
- Order with `StrCmpLogicalW` (Explorer's natural order). Skip
  `FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM`.
- Thumbnails: `IShellItemImageFactory::GetImage` returns Explorer's cached
  thumbnail, which is fast and uses the user's RAW codec when one is
  installed; fall back to the embedded preview for RAW without a codec.

## 6. Canvas: viewing the photograph

`PreviewCanvas.swift`, `NegsApp.swift` (`CanvasArea`, `ActivityBadge`)

Behaviour:

- Shows the developed preview, cropped, rotated and with the border (§10),
  over a neutral background. Rotation, crop and border never re-render:
  they are applied when drawing.
- Zoom range 0.5 %–1600 %. **Fit** fits the whole canvas (border included).
  **Actual Size** shows one image pixel per physical screen pixel. Zooming
  by gesture turns fitting off; while fitting, resizing the window refits.
- Pan by dragging (open/closed hand cursor), scrolling, or scroll bars.
  Double-click fits. Option+scroll zooms around the pointer.
- When a higher-resolution render replaces a smaller one, the same region
  stays on screen at the same apparent size. When only the border changes,
  the photograph stays where it is. A different crop (such as an undone one)
  refits.
- The preview buffer is sRGB; macOS colour-manages it to the display.
- Badges at the top centre, after 400 ms only: **Developing** (first render),
  **Updating** (a slow update; the old preview stays visible),
  **Loading Original** / **Original** (§7). None while a slider is dragged.
- Rendering rules worth keeping exactly (`EditorModel.render`): one render at
  a time; edits meanwhile coalesce into the next request; while a slider is
  dragged every finished frame is shown, and the request carries
  `interactive_scale` (screen pixels per photo pixel, when below 1) so the
  helper renders only canvas resolution; on release, one full render. A
  discrete change only shows its own render, never an older one. Stale
  results (older revision or older photo) are dropped.

On Windows:

- wgpu renders into the canvas HWND (decided). Zoom and pan are a transform
  on the existing texture: no re-render, no CPU copy per frame.
- Mouse wheel scrolls; Shift+wheel scrolls sideways; **Ctrl+wheel zooms**
  about the pointer (the Windows convention, and what a precision touchpad
  pinch sends). Ctrl+= / Ctrl+- / Ctrl+0 / Ctrl+1 from the menu. Space+drag
  is optional.
- Actual Size = 1 image pixel per physical pixel at any DPI scale.
- **Colour management (Ask Noah).** Windows doesn't colour-manage a swap chain
  by default, so the sRGB preview looks oversaturated on wide-gamut monitors.
  Options: accept it (like most Win32 apps); use the monitor's ICC profile
  (`GetICMProfile`, convert in the shader); or Windows 11 Advanced Color with
  an FP16 scRGB swap chain.
- Badges: a small overlay drawn in the canvas (wgpu) or a layered child
  window. No animation needed.

## 7. Show Original

`NegsApp.swift` (`OriginalButton`), `Original.swift`, `EditorWindowController.holdOriginal`

Behaviour:

- Hold the button at the right of the status bar, or hold `\` (not in a text
  field), to show the undeveloped photo in place of the developed one; crop,
  rotation and border stay, so nothing moves. Releasing shows the developed
  photo again. Losing focus while held releases it.
- Decoded at the preview's size when the pointer first reaches the button,
  then kept until another photo opens (purgeable while hidden).
- RAW: Apple's default RAW look, lens correction off (to match the engine's
  framing). Others: the file through ImageIO, with its colour profile.
- Disabled before the first render and while cropping. Screen readers get a
  toggle instead of press-and-hold.

On Windows:

- Non-RAW: WIC with colour management. **RAW (Ask Noah):** Core Image doesn't
  exist. Options: the helper renders an undeveloped RAW (e.g. rawler + plain
  sRGB, no film), WIC with Microsoft's Raw Image Extension when installed, or
  the embedded JPEG preview (may differ in framing). The framing must line up
  with the developed preview.
- `\` is AltGr+ß on German keyboards. Match the typed character
  (`WM_CHAR`), track the key release by scan code, and allow AltGr; consider
  a second key that needs no modifier.
- Release on `WM_KILLFOCUS` / `WM_ACTIVATE` (inactive).

## 8. Inspector: adjustments

`Inspector.swift`, `Parameters.swift`, `Histogram.swift`

### 8.1 Layout

- Fixed at the top: an **RGB histogram** (88 pt tall), then a two-way tab
  switch **Lab | Post**. Fixed at the bottom: **Reset Adjustments**. Between
  them, the selected tab's sections scroll.
- The selected tab and which groups are expanded persist (default: Lab, with
  only *Enlarger* expanded).
- The whole inspector is disabled until the catalog (stocks, defaults) is
  known. While cropping it is replaced by the crop panel (§9).

### 8.2 Histogram

- 256 bins per channel of the developed preview **within the crop** (the
  border never counts), sampled on a grid of at most 1 M pixels, computed
  off the UI thread, and newer requests supersede older ones.
- Drawn as red, green and blue filled areas added together on a dark well
  (overlaps mix to cyan, magenta, yellow, white). The scale ignores the pure
  black and white bins so clipping spikes run off the top.

### 8.3 Controls

Every control is bound to an engine parameter path. Defaults come from the
engine's catalog, never from the UI. Values snap to the step (integer
parameters reject fractions). Ranges and steps (`Inspector.swift` and
`Parameters.swift`):

**Lab tab**

*Film & Paper*
- Film (grouped list: Color Negative, Color Slide, Motion Picture, Black &
  White) → `film`
- Development (only if the film has more than one time): "Profile default"
  or each time in minutes → `film_render.development_time` (null = default)
- Scan film directly (checkbox) → `io.scan_film`
- Paper (grouped: Color Paper, Motion Picture Print, Black & White; hidden
  while scanning directly) → `paper`
- Print development (as for film) → `print_render.development_time`

*Camera*
- Auto exposure → `camera.auto_exposure`; when on, Metering (Average,
  Median, Center-Weighted, Partial, Matrix, Multi-Zone, Highlight-Weighted)
  → `camera.auto_exposure_method`
- Exposure (EV) −5…5, 0.1 → `camera.exposure_compensation_ev`
- Film format (mm) 4…120, 0.1 → `camera.film_format_mm`
- Lens blur (µm) 0…100, 0.1 → `camera.lens_blur_um`
- *RAW White Balance* (group, RAW files only): White balance (As Shot,
  Daylight, Tungsten, Custom) → `io.raw_white_balance.mode`; when Custom,
  Temperature (K) 1667…25000, 1 and Tint 0.01…4, 0.01. Changing it
  re-develops the RAW.
- *Lens Diffusion* (group with on/off → `camera.diffusion_filter.active`):
  Filter (Black Pro-Mist, Glimmerglass, Pro-Mist, CineBloom); Strength 0…2;
  Spatial scale 0.1…3; Halo warmth −1.5…1.5; Core, Halo, Bloom intensity
  0…2; Halo size, Bloom size 0.1…3 (all step 0.01)

*Film*
- *Grain* (on/off → `film_render.grain.active`): Particle area (µm²)
  0.05…1, 0.01; Post blur 0…3, 0.01; Sublayers (checkbox); Sublayer count
  1…4, 1
- *Halation* (on/off): Amount 0…3; Spatial scale 0.1…5; Scatter amount 0…3;
  Scatter scale 0.1…5; Bounces 1…5 (step 1); Bounce decay 0…1
- *DIR Couplers* (on/off): Amount 0…2; Diffusion (µm) 0…100, 0.1; Tail (µm)
  0…400, 1; Tail weight 0…1

*Print* (disabled while scanning directly, with the footer "Printing is
skipped while scanning film directly.")
- *Enlarger*: Print exposure 0.1…5; Exposure compensation (checkbox);
  Magenta shift, Yellow shift −50…50, 0.1; Preflash exposure 0…0.5;
  Preflash magenta, Preflash yellow −50…50, 0.1
- *Print Curves* (on/off): Gamma, Fast gamma, Slow gamma, Red, Green, Blue
  gamma 0.5…2; Developer exhaustion 0…1
- *Glare* (on/off): Amount 0…0.2, 0.001; Roughness 0…2; Blur (px) 0…5

*Scan & Color*
- *Scanner*: Lens blur (px) 0…5; Unsharp radius (px) 0…3; Unsharp amount
  0…2; White correction (checkbox); White level 0.5…1; Black correction
  (checkbox); Black level 0…0.5
- *Color Management*: Output gamut (Off, OKLCh, OKLrab, CAM16-UCS, ACES RGC);
  Input gamut (Off, xy Chromaticity); CAT16 adaptation (checkbox); Spectral
  upsampling (Hanatos 2025, Arctic 2026 (Alpha), Mallett 2019)
- *Output*: Limit preview to 12 MP (app setting, preview only; exports keep
  the full Image Size); Encode output as sRGB →
  `io.output_cctf_encoding`; Use less memory for export →
  `settings.serial_grain`

Exact paths for every group are in `adjustmentGroups` in
`Inspector.swift`; display names for option values in `optionTitle`
(`Parameters.swift`).

**Post tab**
- *Tone*: Brightness, Contrast, Whites, Blacks −100…100, 1 (`post.*`), with
  the footer "Adjusts the finished scan/print. To recover a clipped
  highlight, use exposure in the Lab."
- *Color*: Saturation −100…100, 1
- *Border*: see §10

### 8.4 Control behaviour

- **Slider row:** title, value at the right, slider below. Clicking the
  value turns it into a number field with the value selected; Enter, Tab or
  clicking away commits, Esc cancels. Off its default, a small reset button
  appears (tooltip "Reset to <default>"). Double-clicking the title resets.
  Every control has a context menu **Reset to Default** (disabled at the
  default). "Modified" compares numbers with a 1e-6 relative tolerance.
- **Group header:** chevron + icon + title; clicking anywhere on the title
  expands/collapses; groups with an effect switch have an on/off switch in
  the header, and their controls are disabled while it is off.
- **Film choice side effects:** picking a film resets its development time,
  sets *Scan film directly* to whether the film is a slide (positive), and
  switches the paper to the film's target print if there is one (resetting
  the print development time). Picking a paper resets the print development
  time.
- **Reset Adjustments:** restores engine defaults on both tabs and turns off
  *Limit preview*. Keeps film, paper, rotation, crop, border and Image Size's
  Longest side (an older edit's working-resolution factor resets to 1); sets
  *Scan film directly* from the film. Undoable. Tooltip says so.
- **Image Size** (Lab, between *Film & Paper* and *Camera*; see
  [image-size.md](image-size.md)): *Size* is Original or Longest side, plus a
  read-only "Scaled N% (older edit)" entry while an older edit's
  `io.upscale_factor` is not 1. *Longest side* (16…65,535 px →
  `io.photo_long_side_px`, factor set to 1) commits on Enter or focus loss;
  Esc restores it; an invalid or over-limit value stays in the field with its
  error and changes nothing. Choosing Longest side offers the last value used
  (a preference, not part of the edit), else the crop's own longest side.
  *Photograph* shows the exported, cropped size before borders, from the
  engine's resolver; a note says when the preview shows fewer pixels. Footer:
  "Resizes before film simulation, including halation and grain."

On Windows:

- Scrollable child panel of standard controls: trackbar (`TRACKBAR_CLASS`,
  `TBS_NOTICKS`, horizontal) mapped to integer steps; a narrow **buddy edit
  box** with the value beside it. That is the Windows idiom; it replaces
  click-to-type and keeps the value always editable (commit on Enter or focus
  loss, Esc restores, validate the range).
- Checkboxes for toggles and for the group on/off switches.
  `CBS_DROPDOWNLIST` combo boxes for choices. The film and paper lists need
  headers: an owner-drawn combo with non-selectable header rows, or a popup
  menu with separators/submenus per group.
- Collapsible groups: a header button (chevron glyph + title + checkbox) that
  shows/hides its rows and re-lays out the panel. The Lab/Post switch: a
  `WC_TABCONTROL`, or two radio-style buttons.
- Reset: right-click menu "Reset to Default" on label, trackbar and edit;
  double-click the label; optional small reset button.
- **Mouse wheel over a trackbar must scroll the panel**, not change the
  value, unless that trackbar has keyboard focus. Win32 trackbars take the
  wheel by default, a classic annoyance in scrolling panels.
- Keyboard: Tab order follows the visual order; arrow/PageUp/PageDown on a
  focused trackbar change it; access keys on labels.
- Trackbar drag: `TB_THUMBTRACK` → begin interaction (one undo step, fast
  frames); `TB_ENDTRACK`/`TB_THUMBPOSITION` → end interaction and the full
  render. Keyboard steps are single edits.
- Startup cost on macOS came from building ~40 controls per expanded group
  and big pop-ups; build the inspector after the window is on screen and
  create rows of collapsed groups lazily (§17).
- Histogram: a small custom-drawn control (GDI or Direct2D additive fills on
  dark), computed on a worker thread from the preview buffer.

## 9. Crop and straighten

`Crop.swift`; doc: `docs/native-macos-ui.md` (Crop)

Behaviour:

- Enter crop mode from the toolbar or menu (needs a developed image). The
  canvas shows the whole developed photo, unrotated by the crop, without the
  border, fitted (no zoom while cropping).
- Overlay: everything outside the crop dimmed (55 % black), a white outline,
  thick corner brackets, a rule-of-thirds grid while dragging a handle, and a
  fine 9×9 grid while straightening.
- Drag a corner or edge to resize (follows the dragged edge, minimum ≈40 px
  on screen), drag inside to move (one axis at a time, so it slides along the
  turned photo's edges). Cursors show the handle's resize direction or a
  hand.
- **Straighten dial** under the photo: a ruler from −45° to 45° with major
  ticks every 5°, dragged sideways (7 px per degree), snapping to 0 near 0
  (|angle| < 0.4°), double-click resets, value above it ("1.5°"). The crop
  shrinks about its centre to stay on the turned photo and grows back if the
  angle returns within the same drag.
- **Crop panel** (replaces the inspector): Aspect ratio radio list —
  Original, Freeform, Square (6×6), 16:9, 3:2 (35mm), 4:3 (645), 5:4 (4×5),
  7:5 (5×7), Custom (width × height fields, 0.1–1000); Orientation
  Landscape/Portrait (disabled for Freeform and Square); Angle slider with
  value and reset; Size "W × H px" of the cropped preview; **Reset Crop**
  (disabled when unmodified); a help footer.
- **Done** (Enter) applies crop and any rotation made while cropping as one
  undo step "Crop". **Cancel** (Esc) discards both. While typing a ratio,
  Enter/Esc belong to the field.
- Rotate works during crop and turns the crop with it.
- While cropping, these are disabled: Save Preview, Export, Fit, Actual
  Size, Reset Adjustments, the film strip, Show Original.
- Stored as a fraction of the unrotated photo plus angle; the exporter and
  Save Preview cut the same pixels (`spektrafilm-io::crop`). Pixel rounding
  must match the Rust side; `CropTests.swift` pins the values. Port those
  tests.

On Windows:

- Draw the overlay, handles and dial in the canvas (wgpu, or Direct2D on a
  layered overlay). Standard resize cursors: `IDC_SIZENWSE`, `IDC_SIZENESW`,
  `IDC_SIZEWE`, `IDC_SIZENS`, `IDC_SIZEALL` (move), as in Windows Photos.
- Panel: radio buttons, edit boxes, trackbar, a push button, and Done
  (default button) / Cancel at the bottom right of the panel.
- Arrow keys nudging the crop are a cheap, welcome extra.
- The dial needs keyboard and UIA support (left/right in 0.5° steps, like the
  Mac's VoiceOver action).
- No haptics.

## 10. Borders

`Border.swift`, Post tab in `Inspector.swift`; doc: `docs/native-macos-ui.md`

Behaviour:

- Presentation only: changing the border never re-renders. The canvas draws
  it; Save Preview and Export add it only when **Include in export** is on.
- Controls: **Add border** (master switch; everything else disabled while
  off); **style picker** with four drawn samples — Solid, Film, Keyline,
  35mm — each with a tooltip; styles that don't fit the photo are disabled
  (35mm needs 3:2 or 3:4, with the caption "35mm needs a 3:2 or 3:4
  photograph. This one is 16:9; crop it to 3:2 or 4:3 to use it.").
- Per style: Solid and Keyline share Width (0–15 % of the long edge, 0.1)
  and Color; Keyline adds Line width (0.05–2 %, 0.05) and Line color; Film
  has Width (black added around the frame, 0–15 %); 35mm has no settings
  (the strip comes from `data/borders/35mm`, per film stock, else generic).
- **Extend to 3:4 for Instagram**: pads to a 3:4 portrait canvas (mat colour
  for mats, white for film borders).
- Colours are 8-bit sRGB `#rrggbb`. A run of colour changes within one
  second becomes one undo step.
- Geometry must match `spektrafilm-io::border` exactly (`BorderTests.swift`).

On Windows:

- Style picker: four owner-drawn toggle buttons with a swatch and caption
  (or a radio group with small images).
- Colour: a swatch button opening the common `ChooseColorW` dialog
  (`CC_FULLOPEN | CC_RGBINIT`); one undo step per dialog OK.
- Draw border and film textures in the canvas (the film frame is a
  nine-patch; the 35mm strip turns for portrait photos).

## 11. Save Preview and Export

`EditorModel.savePanel`, `FormatPicker`, `Engine.swift` (`ExportJob`, `ExportEngine`)

Behaviour:

- **Save Preview** saves the current preview (no re-render) with crop,
  rotation, border and size. Only when the preview is current.
- **Export LUT…**, directly below Export Photograph in File on both platforms,
  exports a 33³ `.cube` with sRGB encoded input/output. It snapshots the current
  film/paper settings, including manual exposure, development, print filters and
  color controls. No photo is required. The dialog and file comments explain
  the exclusions: auto exposure, grain, halation/scatter, highlight boost,
  glare, diffusion, blur, sharpening, RAW development, resizing and Post edits.
  DIR color inhibition remains active without spatial diffusion. Export uses
  the same calibrated spectral pipeline as the preview, in one grid render,
  and preserves the active preview. `spektrafilm export-lut --params` shares
  this implementation.
- **Export** develops the photo at full resolution with one of two engines:
  - **Fast (GPU)** (default) — `spektrafilm` f32 CLI with
    `SPEKTRAFILM_BACKEND=wgpu`. If the current preview is already at export
    size, the app saves the preview instead (same bytes, no re-render).
  - **Reference (CPU, 64-bit)** — `spektrafilm-f64`, CPU.
- Dialog options next to the usual name/folder:
  - Format: PNG (lossless, 8-bit) · JPEG (smaller, 8-bit) · TIFF (16-bit).
    The extension follows the format.
  - Engine (Export only), with tooltip.
  - Size: Full Size · Long Edge · Short Edge, plus a pixel field (16–65,535,
    default 2048; disabled at Full Size). Fits the finished file, border
    included, never enlarges.
  - A message that updates with the choices: "Looks the same as Reference,
    in a fraction of the time." / "The most precise result. Can take about
    half a minute for large photos." / "Saves the preview as shown." plus
    " Crops the photograph.", " Adds the film border, extended to 3:4 for
    Instagram.", " Fits 2048 px on the long edge."
- Default name `<photo name>_<film id>.<ext>`. Starts in the last save folder
  (if it still exists). Format, engine, size and folder are remembered.
- Refuses to overwrite the source photo ("Choose a different filename to
  preserve the original photograph."), comparing file identity, not only the
  path.
- Writes to a temporary file next to the destination and renames it into
  place on success; cancel or failure leaves an existing file untouched.
- While exporting: status bar shows a spinner, "Exporting · 12 s" (ticking),
  and a **Cancel** button. Export and further exports are disabled; editing
  continues.
- Results in the status bar: "Exported name.png", "Export cancelled", or
  "Exported name.png on the CPU (32-bit): no GPU was available for Fast
  export". A failed export shows an error with the last 4,000 characters of
  the exporter's log, and for Fast the hint to try Reference.

On Windows:

- `IFileSaveDialog`. Put the formats in the **file type filter**
  (`SetFileTypes`), which is where Windows users look for them and which
  updates the extension (`SetDefaultExtension` on type change). Add Engine,
  Size and the message with **`IFileDialogCustomize`** (combo boxes, an edit
  box, static text; `IFileDialogControlEvents` to react). Own
  `SetClientGuid` for Export and for Save Preview. `FOS_OVERWRITEPROMPT`
  gives the native overwrite question. Fallback if customisation is too
  limited: a small "Export" options dialog before the file dialog
  (Lightroom-style); **Ask Noah** if you go that way.
- Atomic replace: `MoveFileExW(…, MOVEFILE_REPLACE_EXISTING |
  MOVEFILE_WRITE_THROUGH)` (`rename` fails on Windows if the target exists).
  A dot prefix doesn't hide the temporary file: set `FILE_ATTRIBUTE_HIDDEN`.
  Expect sharing violations if the destination is open in another app;
  report them clearly.
- Same-file check: compare volume serial + file index
  (`GetFileInformationByHandle`), which handles case and hard links.
- Progress: status-bar part with a marquee progress bar, elapsed seconds and
  Cancel; also **taskbar progress** (`ITaskbarList3::SetProgressState`,
  indeterminate) and flash the taskbar button when an export finishes while
  Negs is in the background.

## 12. Undo and redo

`EditorModel` (`apply`, `registerUndo`, `beginInteraction`, `endCrop`)

Behaviour (port exactly; this is what users feel):

- Every edit of a parameter, film, paper, border, crop, rotation or preview
  limit is undoable, with an action name: Change Adjustment, Change Film,
  Change Paper, Change Border, Change Border Color, Change Preview
  Resolution, Reset Adjustments, Crop.
- A slider drag (and a border width drag) is one step. A run of colour
  changes is one step (§10). Crop mode's changes are one step on Done,
  nothing on Cancel. Rotation inside crop mode belongs to the crop step.
- Opening another photo clears the history. Undo restores state exactly and
  only re-renders when render parameters changed (border/crop/rotation
  don't).

On Windows: own a small undo stack (no `NSUndoManager`). Edit menu titles
"&Undo Change Film\tCtrl+Z". Ctrl+Z inside a focused edit box undoes the
typing, not the document.

## 13. Status bar, progress and errors

`NegsApp.swift` (`StatusBar`), `EditorModel.report`

Behaviour:

- Hidden until a photo is open, an export runs, or there is a message.
  Left: status text
  (selectable): "6048 × 4024 · developed in 0.74 s", or during a drag
  "Preview · 1600 × 1064 · developed in 0.09 s", "Saved name.png", update
  messages, errors in short. Right: export progress (§11) and the Show
  Original button (§7).
- Errors go to an alert titled "Negs" with an OK button; status reads
  "Could not complete the operation". A failed render keeps the last good
  preview.

On Windows: `STATUSCLASSNAME` with parts (status text · `Film → Paper` ·
export progress · Show Original). Errors via `TaskDialogIndirect` (main
instruction + content, expandable details for the exporter log). Text in
the status bar is copyable via a context menu "Copy".

## 14. Quitting and the session

`NegsApp.swift` (`applicationShouldTerminate`), `Session.swift`

Behaviour:

- Quitting during an export asks: "An export is still in progress." /
  "Quitting cancels it. Any existing file at the destination is left
  unchanged." with **Continue Exporting** and **Quit**. Quitting is refused
  while a Save Preview is being written, or while an update installs during
  an export.
- On quit, the session is saved once (never on each edit): the photo path,
  the full edit state (params, film, paper, preview limit, border, crop,
  including a crop being drawn), adjustments visibility and fit. A failed
  save cancels quitting with an error.
- On launch, the session is restored in parallel with the engine start, but
  only if no photo was opened explicitly. Restoration validates the schema,
  crop values, rotation and file size (≤1 MB); unknown film/paper or a
  missing photo leave a status message instead. Saved params merge into
  current engine defaults (new parameters keep defaults, removed ones drop).

On Windows:

- `WM_CLOSE` → TaskDialog with command buttons "Continue exporting" / "Quit".
- Also save on `WM_QUERYENDSESSION`/`WM_ENDSESSION` (log off, shutdown, and
  Windows Update restarts), where no dialog may block. Register with
  `RegisterApplicationRestart` if restoring after an update restart is
  wanted.
- Session file: `%APPDATA%\Negs\session.json` (macOS:
  `~/Library/Application Support/dev.negs.app/session.json`). The JSON
  format can be identical. Store the plain path; there is no bookmark.

## 15. Settings, updates and About

`Updates.swift`, `MainMenu.swift`

Behaviour:

- Settings window: "Automatically check for updates" toggle, "Updates are
  installed only when you choose to restart Negs.", last check time, a
  **Check for Updates…** button; in unconfigured builds "Updates are not
  configured for this build."
- Sparkle starts 3 s after launch and only once the first render is done,
  so it never delays startup. A downloaded update waits for a running
  export or save before restarting. Signed feed, EdDSA key; no update
  without both feed and key.
- About: standard panel (name, version, credits).

Persisted preferences (macOS `UserDefaults` keys): `recentFiles`,
`filmstripVisible`, `expandedAdjustments`, `inspectorTab`, `exportEngine`,
`outputFormat`, `exportEdge`, `exportEdgePixels`, `lastSaveFolder`,
`catalogCache`, window frame `editor`, split view `editor`.

On Windows:

- Preferences in `HKCU\Software\Negs` or a JSON file in `%APPDATA%\Negs`.
  The catalog cache is a cache: `%LOCALAPPDATA%\Negs`.
- Settings dialog: a modal dialog (dialog template or simple window) with
  OK/Cancel, opened from File ▸ Settings… (or Tools ▸ Options…).
- **Updates (Ask Noah):** WinSparkle is the closest equivalent (Sparkle
  appcast format, EdDSA signatures, a callback to delay shutdown while
  exporting). Alternatives: winget/MSIX updates. Whatever is chosen, keep the
  deferred start and the "wait for the export" rule.
- About: a small dialog or `TaskDialog` with version, licence and credits
  (the same credits as the Mac app and the site).

## 16. Engine processes

`Engine.swift`, `crates/spektrafilm-native`

Behaviour:

- Locating tools: env override, else next to the executable, else
  `target/release` under `SPEKTRAFILM_ROOT` (default: working dir).
  Overrides: `SPEKTRAFILM_NATIVE_HELPER`, `SPEKTRAFILM_F64_CLI`,
  `SPEKTRAFILM_F32_CLI`, `SPEKTRAFILM_DATA_DIR`. The app starts without the
  f32 CLI; only Fast export then fails, with a message.
- Helper: `spektrafilm-native --data-dir <data>`, newline-delimited JSON
  over stdin/stdout, one request at a time with an `id` the reply must
  echo, stderr to `engine.log` in the scratch folder. Commands:
  - `catalog` → `protocol` (must be 1), `defaults`, `profiles` (`id`,
    `name`, `paper`, `positive`, `bw`, `target_print`, `development_times`).
  - `render` with `input`, `source_generation`, `film`, `paper`, `params`,
    `limit_preview`, `preview_path`, optional `interactive_scale` → `width`,
    `height` (RGBA8 buffer at `preview_path`, ≤ 8192², alpha ignored),
    `photo_width/height`, `output_width/height`, `elapsed_ms`, `raw`,
    `export_resolution`, `backend`.
  - `save` with `render_id`, `output`, `rotate_cw`, optional `crop {x, y,
    width, height, angle}`, `border` + `border_options` + `instagram`,
    `fit {long_edge|short_edge}`.
  - The preview is never rotated by the helper; `io.rotate_cw` is sent as
    null in render requests and applied at save/export.
- If the helper dies, the request fails with "The image engine stopped
  unexpectedly. Open the image again to retry." and the next request
  restarts it.
- Exporter: `<exe> process <src> -o <staging> --film F --paper P --params
  params.json --data-dir D [--scan-film] [--crop x,y,w,h,angle] [--border
  STYLE --keyline-width W --border-color #rrggbb --keyline-color #rrggbb
  [--border-width W] [--instagram]] [--long-edge N | --short-edge N]`, env
  `SPEKTRAFILM_BACKEND=cpu|wgpu`. "Backend: wgpu" in its log means it ran on
  the GPU. The line is printed after the render and names the backend that
  actually rendered: on Windows a render that fails the GPU capacity check
  reports `Backend: CPU (rayon)` although wgpu was requested. The helper's
  `render` reply `backend` follows the same rule. Treat either as "fell back
  to the CPU" and say so, as the Mac app does.
- Scratch: a per-launch folder under the temp dir with a lock file. Startup
  sweeps folders whose lock is free (left by a crash), killing their
  recorded exporter (after checking the executable path) and deleting
  staging files.
- Quit kills the helper and any exporter and removes the scratch folder.

On Windows:

- `CreateProcessW` with `CREATE_NO_WINDOW` (no console flashing), inherited
  pipe handles only for the child (`STARTUPINFOEX` +
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`), quoted arguments
  (`CommandLineToArgvW` rules; paths with spaces and non-ASCII names).
- Put the helper and exporters in a **Job Object** with
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: Windows then kills them when Negs
  dies, which replaces most of the pid-recording crash cleanup. Keep the
  sweep for staging files.
- Scratch: `%TEMP%\Negs\<id>`; the lock is the lock file held open without
  `FILE_SHARE_DELETE`/`FILE_SHARE_WRITE`.
- Pipe I/O on a worker thread, never the UI thread; post results back with
  `PostMessage`.

## 17. Startup (hard requirement)

`docs/native-macos-ui.md` (Startup), `NegsApp.swift`, `EngineClient`, `CatalogCache`

Techniques from the Mac that carry over:

- Start the helper and send `catalog` before creating the window, so both
  run while the window is built.
- Cache the last catalog keyed by helper path + modification time + data
  dir, so the inspector shows stocks immediately; the helper's answer
  replaces it only if different, and keeps edits made meanwhile.
- First frame: window, menu, toolbar, empty canvas. Build the adjustments
  once the window is visible.
- Load the session file concurrently with the catalog; apply it before the
  first render so no render with defaults is wasted.
- Edits are possible as soon as the catalog is known; rendering waits for
  the helper.
- Defer the updater (and anything else non-essential) until after the first
  render.
- Measure first painted frame, first interaction and first usable image
  (see the handoff). The Mac has a benchmark hook (`InteractionBenchmark.swift`,
  `SPEKTRAFILM_BENCHMARK_*` env vars, never reopens a session); give the
  Windows app an equivalent.

## 18. Accessibility

The Mac app labels the canvas ("Photograph preview", with zoom/pan help),
histogram, film strip, border styles, ratio fields, the straighten dial
(adjustable) and the Show Original toggle. On Windows the common controls
provide UI Automation; the custom-drawn parts (canvas, histogram, film strip
if custom, border swatches, crop overlay and dial, Show Original) need UIA
names, roles and keyboard equivalents. High-contrast mode should not hide
text or focus rectangles.

## 19. Mac-only: don't port

- App menu items Services, Hide, Hide Others, Show All; the Window menu;
  `⌘W` closing a window while the app keeps running (Negs quits anyway).
- Stage Manager handling, `application(_:open:)` quirks, Force Touch haptics
  on the dial.
- IOSurface/Core Animation plumbing, purgeable memory, Core Image caches
  (`docs/native-macos-ui.md` and the memory notes). Use what's equivalent on
  Windows, not a copy.
- macOS-only performance workarounds: SwiftUI per-row Observation, stepped
  `Slider` tick marks, `NSHostingController` sizing options. They explain
  the Mac code, not requirements.
- Dark mode (Noah: doesn't matter on Windows). Light mode only.

## 20. Open decisions for Noah

1. Startup spike result (Win32 vs WinUI 3) and go-ahead on this UI plan.
2. Colour management of the preview on wide-gamut monitors (§6).
3. Show Original for RAW without Core Image (§7).
4. Export options inside the save dialog (`IFileDialogCustomize`) vs a
   separate options dialog first (§11).
5. Crop shortcut (§2), and whether `\` alone is enough for Show Original on
   layouts where it needs AltGr (§7).
6. Updater (WinSparkle vs store/winget) and installer type, which decides
   file associations and the Jump List (§4, §15).
