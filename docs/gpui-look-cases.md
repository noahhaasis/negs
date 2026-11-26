# GPUI Look-Parity Cases (`docs/gpui-look-cases.md`)

Live inventory of every visible difference between **Win32** (`windows/src/ui/`), **SwiftUI** (`macos/Sources/NegsApp/`), and **GPUI** (`app/src/`), settled case by case by Noah.

**Baselines (same photo `DSC_0262.NEF`, 1280×840 DIPs, all six themes):**
- PC Win32 & GPUI: `~/.negs/gpui-look/baseline-phase6/<theme>-{win32,gpui}.png` (1584×1042 @ 125%)
- Mac SwiftUI & GPUI: `~/.negs/gpui-look/2026-11-26-photo/<theme>-{swiftui,gpui-mac}.png` (2560×1720 / 2560×1680 @ 2×)

---

## Batch 1 — Area 1 (Themes & Typography) + Area 2 (Platform Chrome & Title Bar)

> **Already identical (no case needed):** All 14 base sRGB colour tokens (`bg`, `panel`, `film`, `grid`, `line`, `track`, `dim`, `label`, `paper`, `accent`, `fill`, `on_fill`, `warn`, `blank`), the 3 theme flags (`dark`, `highlighter`, `plain`), Contact Sheet's dark-film ink override (`accent: 0xe2492f`), and the shared opacity/dimming constants are 100% hex-identical across `crates/negs-app/src/theme.rs`, `macos/Sources/NegsApp/AppTheme.swift`, `windows/src/ui/theme.rs`, and `app/src/theme_gpui.rs`.

### L1 — First-launch default theme
- **Area / State:** Area 1 (Themes) · First launch with clean profile.
- **Win32:** Matches Windows `AppsUseLightTheme` (`Plain` or `Plain dark`).
- **SwiftUI:** Always `Darkroom`.
- **GPUI:** Always `Darkroom` on all platforms.
- **Original bug:** None (intentional platform split between Win32 and SwiftUI).
- **Options:**
  - **A (Recommended):** Always `Darkroom` on all platforms (matches SwiftUI and current GPUI; keeps Negs' signature look everywhere).
  - **B:** Match OS light/dark (`Plain` / `Plain dark`) on Windows, `Darkroom` on Mac (preserves Win32's Windows behaviour).
  - **C:** Match OS light/dark (`Plain` / `Plain dark`) on all platforms.
- **Decision:** **A** (Noah, 2026-11-26).

### L2 — `Plain` and `Plain dark` typography & mark substitution
- **Area / State:** Area 1 (Themes & Type) · `Plain` and `Plain dark` themes, whole window.
- **Win32 & SwiftUI:** Agree 100% (`Theme.swift:191-195`, `d2d.rs:407-413`): replace Newsreader with system UI sans at **`0.95×` size**, **force upright** (drop all italics on filename, view switch, caption, hints, `"printed on"`), remap weights (`≥650→600`, `≥500→500`, `<500→400`), and draw straight blue marks instead of hand-drawn pencil strokes.
- **GPUI:** Only swaps `serif_family = ui_family` at `1.0×`; **leaves italics italic** (caption, `"printed on"`, hints stay italic sans) and does not remap weights.
- **Original bug:** None.
- **Options:**
  - **A (Recommended):** Match Win32 & SwiftUI (`0.95×` scale, forced upright, remapped weights, straight blue marks).
  - **B:** Keep GPUI (`1.0×` scale, keep italics).
- **Decision:** **A** (Noah, 2026-11-26).

### L3 — Light Table highlighter mark (`#ffd23f @ 0.85`)
- **Area / State:** Area 1 (Themes & Marks) · `Light table` theme — View Switch, Inspector Tabs, changed slider/choice values, Print Label, Film Strip.
- **Win32 & SwiftUI:** Agree (`Theme.swift:332-341`, `d2d.rs:1098-1106`): every active word underline renders as an **`8.5 pt` yellow highlighter swipe** (`#ffd23f @ 0.85`, multiply blend, lifted `5 pt` behind the lower half of the word).
- **GPUI:** Partially implemented only on the canvas Print Label (missing `reach = 4.25 pt` horizontal/vertical outset and multiply blend); everywhere else uses a thin flat bar or no mark.
- **Original bug:** Win32 film strip accidentally lifts Light Table highlighter by `-5` (`d2d.rs:1100`), whereas SwiftUI uses `lift: 0` under the strip thumbnail.
- **Options:**
  - **A (Recommended):** Match Win32 & SwiftUI (full highlighter swipe everywhere `PencilStroke` underlines a word, fixing GPUI's `print.rs` outset and using `lift: 0` on the film strip as SwiftUI does).
  - **B:** Thin `accent` (`#ad7b00`) underline on Light Table instead of highlighter.
- **Decision:** **A** (Noah, 2026-11-26).

### L4 — Editorial Newsreader type scale
- **Area / State:** Area 1 (Type Scale) · Main window chrome & inspector, all non-Plain themes (and `0.95×` sans in Plain if L2=A).
- **Win32 & SwiftUI:** Agree 1:1 (`Theme.swift`, `ink.rs:49-72`):
  - Inspector Tabs (`Prep · Lab · Output`): Newsreader **`21 pt`** (`700` active / `500` inactive, width-matched tracking)
  - Export / Done button: Newsreader **`18 pt`** `700`
  - View Switch (`Grid · Photo · Compare`): Newsreader **`17 pt`** italic `400`
  - Module title (Lab groups): Newsreader **`16 pt`** (`600` on / `400` off)
  - Title bar filename: Newsreader **`15 pt`** `600`
  - Slider / Choice / Readout values: Newsreader **`14.5 pt`** (`400` default / `700` changed)
  - Edit count / `Reset` / `WordButton` / Export summary: Newsreader **`14 pt`** italic
  - Outlined pill buttons: Newsreader **`13.5 pt`** `500`
  - Hints / Notes / `More` lines: Newsreader **`13 pt`** italic
  - Section headers (`BRIGHTNESS`, `TONE`, …): Newsreader **`11 pt`** `700` uppercase, **`0.13 em` tracking**
  - Row labels & Pane header (`Adjustments`, `Presets`): UI sans **`12 pt`** (`400` / `500`)
  - Scopes & Status line: UI sans **`11 pt`** (`500` / `400`)
- **GPUI:** Uses much smaller sans-heavy sizes (13 px sans filename, 12 px sans view switch, 12 px sans tabs, 11.5 px sans values/hints, 10.5 px dim untracked sans headers, 12.5 px sans Export).
- **Original bug:** Win32 `ink::tab_width` has a minor off-by-one tracking multiplier (`n` instead of `n - 1` at `ink.rs:94-95`); GPUI should use exact `(n - 1)` tracking math.
- **Options:**
  - **A (Recommended):** Match Win32 & SwiftUI's editorial Newsreader type scale (with exact `(n - 1)` tab tracking math).
  - **B:** Keep GPUI's smaller UI-sans scale.
- **Decision:** **A** (Noah, 2026-11-26).

### L5 — Font features (`tnum` + `lnum`), optical size, and Windows UI font
- **Area / State:** Area 1 (Typography) · All numeric readouts, EXIF captions, and Windows UI text.
- **Win32 & SwiftUI:** Enable OpenType **`tnum` + `lnum`** (tabular lining figures so numbers never jitter horizontally while dragging sliders) and variable `opsz = clamp(size, 6, 72)`. Win32 uses **`Segoe UI Variable`** on Windows 11 (`d2d.rs:314`).
- **GPUI:** Sets neither `tnum` nor `lnum` (GPUI 0.2.2 `Font.features: FontFeatures` supports both!), hardcodes `"Segoe UI"` on Windows, and cannot vary `opsz` in GPUI 0.2.2 (uses the 16pt optical instance, which is visually close for 11–24 pt).
- **Original bug:** None.
- **Options:**
  - **A (Recommended):** Enable `tnum` + `lnum` via GPUI `FontFeatures` across Newsreader & UI fonts, and prefer `Segoe UI Variable` on Windows 11 (accepting GPUI 0.2.2's fixed 16pt `opsz`).
  - **B:** Leave GPUI font defaults unchanged.
- **Decision:** **A** (Noah, 2026-11-26).

### L6 — Platform chrome & cross-platform layout split
- **Area / State:** Area 2 (Platform Chrome) · macOS vs Windows/Linux window shell & inspector split.
- **Win32:** `38 DIP` title band with in-window menu bar (`x = 6`) on left and `46 × 38` caption buttons on right; resizable inspector split `340..440 DIP`; dynamic min width (`985` w/ inspector, `640` w/o) × **`520 DIP`** min height.
- **SwiftUI:** Native macOS menu bar, `38 pt` transparent title band with traffic lights centered on left; resizable inspector split `340..440 pt`; dynamic min width (`981` w/ inspector, `640` w/o) × **`560 pt`** min height.
- **GPUI:** Fixed **`340 px`** inspector (not resizable); static `800 × 560` min window; adds a macOS-only stock pill in the titlebar; macOS traffic lights sit at `(14, 13)` in a `42 px` band (2 px above center).
- **Originals' bugs:**
  1. SwiftUI never syncs `NSWindow.appearance` to the chosen Negs theme (`EditorWindowController.swift:144-147`), causing macOS system chrome/dialogs to clash when macOS dark/light disagrees with Negs.
  2. Win32 min height is `520` vs SwiftUI `560`.
- **Options:**
  - **A (Recommended):** **Unified layout + fix chrome**: Identical layout on macOS, Windows, and Linux apart from platform chrome (macOS: traffic lights vertically centered in the 38 pt band + native menu bar + theme-matched `NSAppearance`; Windows/Linux: in-window menu bar + `46 × 38` caption buttons). Add resizable `340..440` inspector drag splitter and dynamic min width (`981` w/ inspector, `640` w/o) × `560` min height.
  - **B:** Unified layout, but keep GPUI's fixed `340 px` inspector (no drag resize).
- **Decision:** **A** (Noah, 2026-11-26).

### L7 — Title bar height, surface colour, and bottom hairline
- **Area / State:** Area 2 (Title Bar) · All six themes (especially visible in Light Table, Contact Sheet, Daylight).
- **Win32 & SwiftUI:** Agree 100% — height **`38 pt/DIP`**, surface **`bg`** (matches photo canvas seamlessly!), and a **`1 pt` `line` bottom hairline ONLY above the inspector column** (no horizontal rule over the canvas).
- **GPUI:** Height **`42 px`**, surface **`panel`**, with a **full-width `1 px` `line` bottom border** creating a hard seam over the canvas.
- **Original bug:** None.
- **Options:**
  - **A (Recommended):** Match Win32 & SwiftUI (`38 px` height, `bg` surface, `1 px` `line` hairline only above the inspector column).
  - **B:** Keep GPUI (`42 px`, `panel`, full-width hairline).
- **Decision:** **A** (Noah, 2026-11-26).

### L8 — Title bar left side: filename vs `"Negs"` brand & stock badge
- **Area / State:** Area 2 (Title Bar) · Photo, Grid, Compare, and Empty states.
- **Win32:** Shows **only the filename** (`DSC_0262.NEF`, or Grid selection name, or Compare `"A and B"`, or `"Negs"` when empty) in Newsreader **`15 DIP` `600`**, **middle-truncated**, `14 DIP` after menu bar. Dims to `dim` when window is inactive. No `"Negs"` prefix, no `"/"`, no stock badge.
- **SwiftUI:** Identical content, font, weight, middle truncation, `14 pt` after traffic lights — **visual bug:** never dims to `dim` when the window resigns key (`TitleBar.swift:75-81`).
- **GPUI:** Always shows `"Negs"` (17 px serif) + `"/"` + end-truncated 13 px UI-sans filename (ignores Grid/Compare titles) + a macOS-only `film · paper` pill that clips at 1280 px (causing `DSC_0262.NEF` to clip to `DSC_0262.NEI` in Contact Sheet!).
- **Options:**
  - **A (Recommended):** **Match Win32 (fixing SwiftUI's inactive-dim bug):** Newsreader `15 pt` `600` middle-truncated title only (photo / Grid selection / Compare / `"Negs"` when empty), dimming to `dim` when window is inactive; remove permanent `"Negs"`, `"/"`, and macOS stock badge.
  - **B:** Match SwiftUI literally (same as A, but stays `paper` when inactive).
  - **C:** Keep GPUI (`Negs / filename` + stock badge).
- **Decision:** **A** (Noah, 2026-11-26).

### L9 — Title bar right toolbar buttons (Open, Save, Original, Split, Fit, Theme, Inspector)
- **Area / State:** Area 2 (Title Bar) · Main window.
- **Win32 & SwiftUI:** Agree 100% — **none of these 7 buttons exist in the title bar**; only `Grid · Photo · Compare` sits on the right. (`Original ⌄`, `Fit`, `100%` live in the status line; `Open`, `Save`, `Split`, `Theme`, `Inspector` live in menus, shortcuts, and Settings › Look.)
- **GPUI:** Displays 7 extra boxed pill buttons (`Open`, `Save`, `Original`, `Split`, `Fit (xx%)`, `{Theme} ▾`, `Inspector`) + 3 vertical dividers, crowding the band and mispositioning the Theme popover on Windows (`right: 96` ignores `138 px` of caption buttons). Also hardcodes `Ctrl+O` / `Ctrl+S` in macOS tooltips.
- **Original bug:** None (GPUI-only clutter & bugs).
- **Options:**
  - **A (Recommended):** Match Win32 & SwiftUI — remove all 7 extra buttons from the title bar (moving `Original ⌄` / `Fit` / `100%` back into the status line in Area 4).
  - **B:** Keep a reduced subset of title bar buttons (specify which).
  - **C:** Keep all GPUI title bar buttons (fixing Windows popover position & macOS tooltips).
- **Decision:** **A** (Noah, 2026-11-26).

### L10 — View Switch (`Grid · Photo · Compare`)
- **Area / State:** Area 2 (Title Bar) · Right side when a photograph is open.
- **Win32:** Bare words in **`Grid · Photo · Compare`** order, Newsreader **`17 DIP` italic**, `16 DIP` spacing, `18 DIP` trailing margin, hidden when empty. Active = `paper` + `5 DIP` grease-pencil underline (Light Table: yellow highlighter); hover = `label`; inactive = `dim`; pressed = `0.62` alpha; disabled = `0.36` alpha; keyboard focus = `1.5 DIP` `paper` focus box.
- **SwiftUI:** Identical order, font, size, spacing, underline, hidden when empty — **visual/interaction bugs:** no hover tint (stays `dim`), no `0.62` press feedback, no keyboard focus box, uses `0.38` instead of `0.36` disabled opacity (`CompareCanvas.swift:556-570`).
- **GPUI:** Swapped order (**`Photo · Grid · Compare`**), always visible, 12 px UI-sans boxed pills, no serif/italic, no grease-pencil underline or highlighter.
- **Options:**
  - **A (Recommended):** **Match Win32 (fixing SwiftUI's missing hover/press/focus/opacity):** Bare italic Newsreader `17 pt` `Grid · Photo · Compare`, hidden when empty, active `paper` + grease-pencil underline / Light Table highlighter, hover `label`, pressed `0.62`, disabled `0.36`, keyboard focus box.
  - **B:** Match SwiftUI literally (no hover/press/focus states).
  - **C:** Keep GPUI boxed pills (fixing order to `Grid · Photo · Compare`).
- **Decision:** **A** (Noah, 2026-11-26).

### L11 — Windows/Linux caption buttons (`— □ ×`) & in-window menu bar
- **Area / State:** Area 2 (Title Bar, off-Mac) · Top-left menu bar & top-right window controls.
- **Win32:**
  - Menu bar: at `x = 6 DIP` on `bg` (no `"Negs"` before it), Segoe UI Variable `~12 DIP`, `4 DIP` rounded pill on hover (`paper @ 0.08`) / open (`paper @ 0.14`), `paper` lit / `label` active / `dim` inactive.
  - Caption buttons: `46 × 38 DIP`, crisp 1px-snapped 10 DIP vector glyphs, `label` active / `dim` inactive, hover `paper @ 0.08` / pressed `paper @ 0.14` (Close: `#c42b1c` / `#c83c30` + white), hidden in full screen.
- **GPUI:**
  - Menu bar: sits after `"Negs"` on `panel`; dropdown opens at `top: 26 px` cutting across the bottom of the 42 px title bar (**bug**); dismissal scrim starts at `top: 42 px` so clicking empty titlebar doesn't close an open menu (**bug**); no inactive dimming.
  - Caption buttons: `46 × 42 px`, Segoe MDL2 font glyphs, no inactive dimming, no pressed state.
- **Original bug:** Win32 uses slightly inconsistent foreground/active checks between titlebar (`GetForegroundWindow || GetActiveWindow`) and menubar (`GetForegroundWindow` only); GPUI uses GPUI's unified `window.is_window_active()`.
- **Options:**
  - **A (Recommended):** **Match Win32 + fix GPUI bugs:** Menu bar directly at `x = 6` on `bg` with Win32 hover/open pills, inactive dimming, flush `top: 38` dropdown, and full-window scrim; `46 × 38` custom vector caption buttons with hover/pressed/inactive states (hidden in full screen).
  - **B:** Keep GPUI Segoe MDL2 glyphs and current pill style (only move to `x = 6` on `bg` and fix dropdown offset & scrim).
- **Decision:** **A** (Noah, 2026-11-26).

### L12 — Native macOS & Windows menu bar structure
- **Area / State:** Area 2 (Platform Chrome) · Top-level menus & `Edit` items.
- **SwiftUI (Mac):** `Negs` (About, Check for Updates, Settings, Services, Hide, Hide Others, Show All, Quit), `File`, `Edit` (Undo, Redo, Cut, Copy, Paste, Delete, Select All, Deselect All), **`View`, `Photo`**, `Window`, `Help`.
- **Win32 (PC):** `File`, `Edit` (Undo, Redo, Cut, Copy, Paste, Delete, Select All), **`Photo`, `View`**, `Help`.
- **GPUI:** Missing `Window` and `Help` on macOS; puts `Photo` before `View` on macOS; `Edit` only has `Undo` and `Redo` on all platforms. *(Note: GPUI 0.2.2 native macOS menus do not support dynamic checkmarks/disabled states; off-Mac menus already have full state via `menu_state.rs`.)*
- **Original bug:** None.
- **Options:**
  - **A (Recommended):** Match SwiftUI's menu structure on macOS (`Negs`, `File`, full `Edit`, `View`, `Photo`, `Window`, `Help`) and Win32's on Windows/Linux (adding the missing `Edit` items there too).
  - **B:** Keep `Photo` before `View` on both platforms (still adding missing `Edit`, `Window`, `Help`).
- **Decision:** **A** (Noah, 2026-11-26).

---

## Upcoming Batches (audited or queued)
- **Batch 2 — Area 3 (Inspector):** Pane header, Film/Paper strip removal, Scopes, Prep/Lab/Output tabs, single-line Slider rows & Win32 modified-slider bug, Section/Group headers, TickBox switches, borderless Choices, Italic notes, `RevealedRows` indent, Tone Graph (SwiftUI vs Win32/GPUI), Color Filter `PencilRing`, Border Style swatches, Color Well order, Outlined pills, Output ExportFoot. *(Audit complete; Lab/Output/ToneGraph/Border screenshots next.)*
- **Batch 3 — Area 4 (Film Strip, Status Line & Print Caption/Label):** Strip 4 pt active pencil vs Win32/GPUI 2.1 pt bug, `StripLeader` fold tab, GPUI launch-centring bug, Status line placement/surface/contents (`Original ⌄ | Fit | 100%` vs ICC profile), Print caption `40 mm` fact-popping, Print label tracking & Light Table highlighter offset. *(Audit complete.)*
- **Batch 4 — Areas 5 & 6:** Film/Paper Tray & Presets Sidebar.
- **Batch 5 — Areas 7 & 8:** Contact-Sheet Grid, SheetFlight, Compare & Crop.
- **Batch 6 — Areas 9 & 10:** Settings, Export & Batch dialogs, Alerts, Menus/Tooltips, Empty & Small states.
