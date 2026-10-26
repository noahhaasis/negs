> Historical UI-only inventory at commit 693223d. The engine connection, exports,
> cache, RAW development and packaging were subsequently implemented; see
> [linux-app.md](linux-app.md) and [linux-core-validation.md](linux-core-validation.md).

# Linux UI parity audit

Audited against the current macOS sources (`Inspector.swift`, `Border.swift`,
`Crop.swift`, `EditorModel.swift`, `MainMenu.swift`) and the Behavior contract
in `windows-ui-features.md`, on 2026-10-25. This inventories the native GTK
presentation work; it is not a release-readiness or numerical-parity claim.

**Film processing and file writing remain disconnected in this UI revision.**
The photograph is explicitly labeled as a camera/file preview. The native
controls retain local presentation values; their labels must be translated to
real `RuntimeParams` and merged with live helper defaults during core integration.
The repository catalog snapshot contains real profile metadata, not engine
parameter defaults. See `linux-core-agent-handoff.md` for the engine work.

| Area | Linux UI now | Remaining work |
|---|---|---|
| Window and appearance | Native GTK/libadwaita, light/dark/system, narrow-layout overlay, pointer/keyboard inspector resize handle, saved geometry/sidebar width/tab/expanded groups/visibility | More monitor/scaling coverage and window-manager geometry coverage |
| Installation | Per-user preview installer, desktop entry/icon/Open With declarations, licensed border data, source archive | Arch package/PKGBUILD and other distribution formats, uninstall/update handling, dependency packaging and other-machine validation |
| Opaque photographs | Opaque canvas; installer applies focused/inactive/fullscreen Omarchy exception | Other Hyprland configuration versions; compositor policy cannot be overridden by the binary |
| Open | Native picker/drop/path, background decode, retained canvas while loading, stale-result rejection, 12 recent paths/clear/missing-path removal, expanded RAW extension list, GApplication file-open forwarding outside isolated test profiles | Full RAW development through rawler; individual RAW formats may lack a usable embedded JPEG; decoder/color fidelity and error coverage |
| Film strip | Horizontal virtualized native ListView, natural numeric ordering, native GIO folder watcher with debounce, changed-middle model splicing, two thumbnail workers, 192-entry cache keyed by path/size/mtime, stale item rejection, same-folder model stability, scroll-to-selection, canvas/strip keyboard selection and Alt+Left/Right | Large-folder stress and screen-reader usability coverage; scans are bounded at 10,000 supported visible files |
| External file changes | Add/rename/delete update the strip; an in-place modification invalidates the affected visible thumbnail; changed selected source reloads camera preview while retaining edits | Engine retained-render/cache invalidation, and explicit recovery when a selected source is deleted |
| Canvas | Fit, scaled actual size, pointer zoom, pan, quarter turns, opaque source presentation | Full-resolution detail loading, pixel/color parity and mixed-display color management |
| Original | Original action/button and canvas backslash command clearly explain that the camera original is already displayed | Actual hold/toggle comparison, release/focus handling and retained developed render after the helper is connected |
| Histogram | Background camera-preview calculation and native drawing; photo-generation rejection | Developed/cropped histogram and independent engine-result generation handling |
| Film and paper | All 23 camera films and 10 print-stage profiles, grouped searchable menus, readable selected names, actual development-time options, positive-film scan defaults, metadata target-paper pairing, print hiding/disabling for direct scan, B&W color-control visibility | Replace checked metadata snapshot with live helper catalog/defaults and calibrate the real parameter behavior |
| Image Size | Original/Longest-side mode, editable remembered target, integer/range/128-MP enlargement validation, source/working/cropped readouts, commit/focus-loss/Escape behavior, reset preservation and session storage | Actual working-frame resize, preview percentage and helper/export sizing parity |
| Camera | Auto exposure with conditional seven-method metering, exposure, format and blur | Connect real defaults and render parameters |
| RAW white balance | RAW-only As Shot/Daylight/Tungsten/Custom controls, conditional 1667–25000 K temperature and 0.01–4 tint, local undo/session | Real RAW re-decode and retained original handling |
| Film/print effects | Effect switches in disclosure headers, insensitive children while off, four diffusion families, grain sublayers/count, enlarger exposure compensation, all numeric controls | Real defaults/parameter mapping and processing |
| Scanner/color/output | White/black correction and conditional level sensitivity, output/input gamut, CAT16, spectral method, preview limit, sRGB encoding and less-memory export choices | Real defaults/parameter mapping and processing |
| Post | Original five tone/color sliders at −100…100, step 1 | Real processing/defaults and final help-text tuning |
| Borders | Solid/Film/Keyline/35mm, sample thumbnails, incompatible 35mm menu disabling/warning, mat/line/film thickness, sRGB color/hex controls, Instagram and export inclusion, real film-specific textures, undo/session, one-step color runs and per-control resets | Exact developed/export/crop pixel parity |
| Crop | Full-source Original ratio, Freeform/Square/16:9/3:2/4:3/5:4/7:5/Custom, orientation controls and saved custom values, corners/edges/move, keyboard nudge, accessible straighten slider/number, projected dimensions, Reset, Done/Cancel, one crop/straighten/rotation undo transaction, conflicting command disabling, wrapping narrow-layout controls | Pointer/cursor and screen-reader polish; compare exact crop pixel rounding, straightened boundary geometry and export parity with the engine |
| Undo/reset | Named menu commands, grouped slider drags, per-number reset/modified display and right-click reset, choice/switch resets, numeric Escape handling, global reset preserves film/paper/rotation/crop/border/Image Size | Native text-edit undo routing coverage; exact live-default comparison |
| Save Preview / export | Separate flows, real destination picker without writing, default film-based names/extensions, same-file identity rejection, Fast/Reference choice, PNG/JPEG 8-bit and TIFF 16-bit readouts, JPEG quality, full/long/short edge and bounded pixel field, border/metadata, remembered format/engine/edge/quality/metadata/folder, live explanation; write button disabled with explicit reason | Real retained-preview save, atomic exports, real metadata/format support, cancellation/progress/error and actual backend reporting |
| LUT | Photo-independent 33³ Cube options and destination picker, sRGB and exclusions explained, disabled write action | Actual LUT job, file comments, cancellation and errors |
| Settings/session | App-only appearance settings, clear recent list, accurate distribution-update information, atomic bounded session, validated crop/rotation, preferences and full draft crop saved on graceful close, save error prevents closing | Developed-preview cache and invalidation, full application-data cleanup UI, wider restore/corrupt-session coverage |
| Menus/accessibility | Open/recent/save/export/LUT, named undo/redo/reset, view/crop/rotation/navigation, settings/shortcuts/about/quit; F6/F7/F11, numeric/slider/canvas/thumbnail names, native keyboard controls | Remaining command inventory polish, true Original state, full pointer/keyboard/screen-reader assessment |

`fixtures.rs` deliberately remains an explicit local UI fixture. Film metadata
is extracted reproducibly by `python3 linux/generate_catalog.py` into the checked
`linux/catalog.json`; its attribution and the source profile licenses are retained.
No profile curves or film-processing code are copied into the GTK client.

Validation commands:

```sh
bash linux/cargo.sh test --release --locked --offline
bash linux/cargo.sh clippy --locked --offline -- -D warnings
python3 linux/check_ui.py
python3 linux/check_borders.py
python3 linux/check_controls.py
```

The new control check uses actual GTK signals for stock pairing, development
choices, invalid/valid Image Size input, crop transactions/command states, reset
preservation and remembered export options. It then uses AT-SPI plus real GIO
notifications on copied photographs to check folder add/rename/delete and
in-place thumbnail/source refresh while retaining edits. This is evidence of
native behavior, not proof of physical-pointer/screen-reader usability or film
rendering. Startup/visual captures taken while root engine compilation is active
are diagnostic; final performance measurements must use a quiet machine.
