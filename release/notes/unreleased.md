# Negs (unreleased)

- The Linux AppImage now prefers X11/XWayland when available to avoid a
  reported GTK Wayland crash. Explicit `GDK_BACKEND` choices are respected;
  Wayland-only desktops continue to use Wayland.
- Linux's Film grid now supports search, Return to select and Escape to clear.
  Large-photo edits show a quick canvas-size preview before settling; brightness
  matching holds the previous frame until the exposure is resolved. Quick frames
  stay outside Save Preview and the launch cache. The shared gamut-coordinate
  and pipeline caches are included, and new profiles start with four favourite films.
- The Windows Film grid now has a search field: every word must match a film's
  name, id or group, ignoring case and accents; Return chooses the first match
  and Escape clears it. Large-photo edits (a film, paper, menu or Undo) show a
  quick canvas-size preview before the full-size one. With camera brightness
  matching on, the previous frame stays until the matched exposure is ready, so
  the old exposure is never shown, and Save and Export wait for the matched,
  full-size preview.
- Linux now has Prep, live camera/original brightness matching, resizable RGB
  scopes, the interactive Print Curves graph, favourite films/papers and help
  for inspector controls and groups. Prep / Lab / Output and the related
  settings follow the macOS/Windows functionality; visual polish is deferred.
- Linux opens new photographs with fresh adjustments by default. Settings ›
  New photographs can restore the previous carry-over behavior. Restored sessions
  keep their own edits. Preview-limit and export-memory options are app settings
  and stay outside Undo and Reset Adjustments.
- Linux's old Post tone/saturation controls have been removed. Updating resets
  their saved adjustments to neutral, matching macOS/Windows. There is no exact
  conversion from these finished-image edits to the new pre-film Prep controls.
- Linux Fit stops at 1:1, the window reserves room for both columns, and Settings
  offers a custom viewer background.
