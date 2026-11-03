# Handoff: a visual polish pass for the native Windows app

Prepared 2026-11-03. Latest GitHub main: `c721589` (Prep tab merge). `windows/` is
unchanged on main since `f0d0ff9`. This is a Windows implementation task with
before/after screenshots, startup measurements and build checks.

## Goal

Make the Win32 app (`windows/`, common controls) look as considered as the Mac app,
without giving up what it has now: the ≈ 0.33 s time to the adjustments column, the
system's accessibility (UI Automation names on every control), keyboard behaviour and
the per-user install. Stay on Win32 and GDI/wgpu. No new UI framework: WinUI 3, XAML
Islands, Slint and ImGui were considered and rejected for startup, packaging or
accessibility reasons. The ImGui experiment is on the `imgui-docking` branch; don't
merge from it or build on it, though `docs/windows-app.md` there records what it cost.

## Checkout and rules

- Repository `C:\Users\Noah\Projects\negs`; `origin` is GitHub. Fetch, read any
  `AGENTS.md`/`CLAUDE.md`, and start an isolated worktree from current `origin/main`
  on a new branch. `main` itself is checked out in `C:\Users\Noah\Projects\negs-presets`.
- Run every app test with `NEGS_PROFILE_DIR` set to a scratch folder and
  `NEGS_ALLOW_MULTIPLE=1`, so Noah's settings, session and his installed Negs
  (`%LOCALAPPDATA%\Programs\Negs`) are untouched. Set `SPEKTRAFILM_ROOT` to a checkout
  whose `target\release` has the helper and exporters (the main checkout has them).
- Commit in steps (one per area below, plus one for validation notes). Don't push or
  merge without a current instruction. Use accurate attribution for the agent doing
  the work.
- This PC runs **Windows 10 22H2 (build 19045)**. Everything must look right there.
  Windows 11 features are additions behind a build check, verified on Windows 11 if
  one is available, otherwise recorded as unverified.
- Screenshot only the Negs window (its window rectangle), never the whole screen.

## Where things are

- `windows/src/ui/mod.rs`: window, `Fonts::new` (the font set), toolbar
  (`build_toolbar`, `toolbar_images` draws Segoe glyphs into an image list), status
  bar, layout, menus, accelerators, startup order (`run`, `WM_APP_BUILD`).
- `windows/src/ui/inspector.rs`: the adjustments column. `rows()` defines the rows;
  `create_row` makes their controls; `layout_rows` places them; `draw_item` already
  owner-draws group headers, foldouts, colour swatches and border style samples
  (`draw_swatch`); stock tiles are `draw_tile`/`draw_thumbnail`; the histogram is
  `histogram_proc`; the crop panel is `build_crop`/`layout_crop`.
- `windows/src/ui/presets.rs` (custom-drawn list rows), `strip.rs` (custom-painted
  film strip), `empty.rs` (empty state), `dialogs.rs` (task dialogs, Settings),
  `canvas.rs` (`BACKGROUND` is the grey around the photograph).
- Reference look: `macos/Sources/NegsApp/Inspector.swift` and the Mac app itself.
- Startup timing: `windows/measure_startup.ps1` reads the `NEGS_STARTUP_REPORT`
  stamps (`first_window_composited`, `inspector_shown`, `gpu_ready`,
  `first_image_on_screen`, …). `docs/windows-app.md` has the current medians.

## The work, in this order

Each step must keep the startup medians (see Acceptance), keyboard navigation and
the accessible names. Owner-drawn controls keep their real window class, focus,
keyboard handling and `accessible_name`. Only their painting changes.

### 1. Owner-drawn sliders, headers and switches

The stock trackbar, push-like radio buttons and default buttons are most of the dated
look.

- Sliders: paint the trackbars yourself (`NM_CUSTOMDRAW` on `TRACKBAR_CLASS`: channel,
  thumb, ticks) as a thin rounded track, an accent-coloured fill from the default
  value to the thumb (centre-out for bipolar sliders such as Exposure), and a round
  thumb with a hover/pressed state and a visible focus ring. Keep the existing
  double-click/Alt-click reset (`trackbar_subclass`) and the wheel routing.
- Group headers and foldouts (`draw_item`): flatter, with a chevron that matches
  Segoe Fluent Icons, hover background and the same focus ring.
- Lab | Post and the crop panel's Landscape | Portrait: a segmented control (one
  rounded outline, accent-filled selected segment) instead of two push-like buttons.
- Primary buttons (Open Photograph…, Done; Reset Adjustments stays secondary): accent
  fill, white text, rounded. Secondary buttons: subtle fill and border.
- Value boxes beside sliders: borderless until hovered/focused, right-aligned, like
  the Mac's.
- Reset buttons: the existing glyph, but drawn smaller and greyer until hovered.

### 2. Typography and spacing

- Fonts: prefer "Segoe UI Variable Text" (and "… Display" for large titles) when
  installed (Windows 11), else Segoe UI. Keep the font set built from
  `SystemParametersInfoForDpi` so text scaling still applies.
- Clear hierarchy: section titles (semibold, a little larger), labels, values, and
  grey notes/footers; one consistent grey for secondary text.
- Spacing on an 8 px grid (DIPs): row height, gaps between groups, panel padding,
  toolbar button size and gaps, status bar height. Revisit `layout_rows`,
  `layout_crop`, the presets panel and the empty state together so they agree.
- Align label and control columns across all rows; truncate long stock names with
  ellipses consistently.

### 3. Dark mode

Follow the system app theme (`AppsUseLightTheme` under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`), switching live on
`WM_SETTINGCHANGE` ("ImmersiveColorSet").

- Title bar: `DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)` (attribute 20, or
  19 before Windows 10 20H1). Works on this PC.
- Your own painting: one palette (background, surface, text, secondary text, border,
  accent) used by every owner-draw and `WM_CTLCOLOR*` handler, replacing the
  `GetSysColor(COLOR_BTNFACE)`/hard-coded greys now spread through the UI.
- Common controls: `SetWindowTheme(hwnd, "DarkMode_Explorer", nullptr)` for scrollbars,
  list views and edit boxes; `"DarkMode_CFD"` for combo boxes.
- Menus and the scrollbars of the main window need the undocumented uxtheme ordinals
  (`SetPreferredAppMode` #135, `AllowDarkModeForWindow` #133, `FlushMenuThemes` #136).
  Load them dynamically, behind a check of the build number, and fall back to light
  menus if they are missing. Record this risk in the docs.
- Canvas surround: a darker grey in dark mode (`canvas.rs` `BACKGROUND` becomes
  theme-dependent; the photograph's own pixels and colour management don't change).
- Task dialogs and the system file dialogs follow the system themselves; check them.
- High contrast: when `SPI_GETHIGHCONTRAST` is on, skip all custom colours and use
  system colours, so the app stays usable.

### 4. Windows 11 additions (behind a build check, ≥ 22621)

- Rounded window corners: `DWMWA_WINDOW_CORNER_PREFERENCE` = round.
- Mica backdrop: `DWMWA_SYSTEMBACKDROP_TYPE` = `DWMSBT_MAINWINDOW`. Mica only shows
  where nothing paints opaquely, so apply it to the title bar/toolbar region (with
  step 5) rather than trying to make every GDI control transparent. If it can't be
  made to look clean, skip it and say why.
- On Windows 10 these calls are skipped; nothing else changes.

### 5. Custom title bar with the toolbar in it

Merge the toolbar into the title bar like modern Windows apps and the Mac's unified
toolbar.

- `DwmExtendFrameIntoClientArea` plus `WM_NCCALCSIZE` to remove the standard caption;
  `WM_NCHITTEST` returning `HTCAPTION` for the empty drag area, `HTMINBUTTON`,
  `HTMAXBUTTON`, `HTCLOSE` for the caption buttons (so Windows 11 snap layouts work on
  maximise), and resize borders.
- Draw the app icon, title and the caption buttons with correct hover, pressed and
  inactive-window states, in light and dark.
- Keep the menu bar reachable: it can move into the title bar as well, or stay below.
  Alt, F10 and the mnemonics must still work.
- Full screen (F11), maximised, snapped and multi-monitor DPI changes must still
  look right.
- This is the riskiest step. If it costs startup or breaks window management, stop
  and report rather than ship it half-done.

### 6. Small motion

Short fades or slides (≈ 120–150 ms) when switching Lab/Post, opening a group or
foldout, and showing the crop panel, using the existing timers. Respect
`SPI_GETCLIENTAREAANIMATION` (off: no animation). No animation runs while idle.

## Out of scope

The Prep tab, favourites, tooltips and film-format menu that landed on macOS in
`c721589` and the commits before it; the tone graph (`docs/handoff-windows-followups.md`);
engine, model and export behaviour; the Linux app. Keep `rows()` and the model calls as
they are, so a later Prep port only adds rows.

## Acceptance

- **Startup:** run `windows/measure_startup.ps1` (6 runs after a warm-up, warm
  profile, `docs/card.jpg`) on `origin/main` and on the branch. Medians of
  `first_window_composited`, `inspector_shown` and `first_image_on_screen` may not get
  worse by more than 10 ms. Record both tables in `docs/windows-app.md`.
- **Tests:** `cargo test --locked --manifest-path windows/Cargo.toml` and
  `cargo build --release --locked --manifest-path windows/Cargo.toml` pass with no new
  warnings.
- **Screenshots** (window only), before and after, light and dark: empty state; a
  photograph with the Lab tab, groups open and a film foldout; the Post tab with a
  border; the Presets panel with thumbnails; crop mode; Settings. Also one at 150 %
  scaling and one in high contrast. Put them under `docs/windows-ui-polish/` and link
  them from `docs/windows-app.md`.
- **Behaviour** still correct, exercised in the release build: tab order and focus
  rings through the whole column, slider keys, double-click and Alt-click reset,
  Enter/Escape in value boxes, the context menu's Reset to Default, Show Original
  (hold O or \), drag and drop onto the canvas and the Presets panel, F6/F7/F8/F11,
  live switching between light and dark.
- **Accessibility:** Narrator (or Accessibility Insights) still reads every control's
  name and value, as before.
- Record anything unverified (for example Windows 11 features on this Windows 10 PC).
