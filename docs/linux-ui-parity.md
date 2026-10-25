# Linux UI parity audit

Audited against the current macOS sources (`Inspector.swift`, `Border.swift`,
`Crop.swift`, `EditorModel.swift`, `MainMenu.swift`) and the Behavior contract
in `windows-ui-features.md`, on 2026-10-25. This is an inventory of the Linux
preview after adding configurable borders, not a release-readiness claim.

**The current editor is a partial UI prototype.** Film processing and exports
remain intentionally disconnected at the user's request. Some gaps also need
UI work independent of that connection; they should not all be described as
waiting only for the engine.

| Area | Linux preview now | Remaining UI/behavior work |
|---|---|---|
| Window and appearance | Native GTK/libadwaita; light/dark; breakpoint sidebar; narrow layout | Resizable inspector handle, saved window geometry, inspector group/tab persistence, more monitor/scaling coverage |
| Installation | Per-user preview installer, desktop entry/icon/Open With declarations, licensed border data, source archive | Arch package/PKGBUILD and other distribution formats, uninstall/update handling, dependency packaging and validation on other machines |
| Opaque photographs | Opaque canvas; installer applies the Omarchy exception to focused/inactive/fullscreen windows | Support/test other Hyprland configuration versions; a binary alone cannot override compositor policy |
| Open | Native picker, drop, path argument, background decoding, retained canvas during load, stale-result rejection | Recent-files UI, supported-RAW extension parity, single-instance file-open forwarding, error/recent removal semantics |
| Film strip | Same-folder widgets/textures retained; selection highlight; current-photo click is a no-op | Virtualization beyond 24 cells, native folder watching, natural ordering, reusable invalidated thumbnail cache, keyboard previous/next and scroll-to-selection |
| Canvas | Fit, scaled actual size, zoom/pan, quarter rotation, opaque presentation | Full-resolution detail loading, pixel/color parity, zoom limits/gesture behavior in every crop mode, mixed-display color management |
| Original | Shows the camera/file original in this phase | Hold-to-compare/toggle, focus-release behavior, keyboard shortcuts and retained developed render once engine is connected |
| Histogram | Background calculation and native drawing | Histogram of the developed, cropped image; stale histogram rejection independent of decoding |
| Film and paper | Six sample film choices and four paper choices | Complete engine catalog, grouped stocks, film/paper development times, slide scan defaults, target-paper pairing, direct-scan visibility/disabled states, B&W behavior |
| Image Size | Original/Longest-side selector only | Editable pixel target, validation/errors, source/crop/output readouts, preview-vs-export percentage, saved target and working-size policy |
| Camera | Exposure, format, lens blur and auto-exposure switch | Metering selector and conditional visibility |
| RAW white balance | Missing | As Shot/Daylight/Tungsten/Custom mode, temperature and tint fields, RAW-only group and re-decode behavior |
| Film/print effects | Parameter sliders for Film Curves, Grain, Halation, DIR, diffusion, Enlarger, Print Curves and Glare | Per-effect on/off switches, disabled controls when off, diffusion-family selector, grain sublayers switch, enlarger exposure-compensation switch |
| Scanner/color | Lens blur, unsharp, white/black levels, output gamut and spectral selector | White/black correction switches, input gamut, CAT16, sRGB encoding and less-memory export options; color/B&W conditional behavior |
| Post | Brightness, Contrast, Whites, Blacks and Saturation, with the original −100…100 step-1 ranges | Connect real parameters/defaults; original tone/color grouping and help text |
| Borders | Add border; Solid/Film/Keyline/35mm; mat/line/outer-film thickness; mat/line sRGB picker and hex entries; Instagram extension; include-in-export flag; real textures and film-specific 35mm edge print; undo/session storage | Dynamic disabling of incompatible 35mm choices, visual style thumbnails, exact developed/export/crop pixel parity once exports exist |
| Crop | Corner resizing, moving, 3:2/Square/Original buttons, accept/cancel | Straighten dial/angle, edge handles, complete aspect list, custom ratios, orientation controls, projected size, Original source-aspect handling, crop reset, draft rotation transaction, state-based command disabling |
| Undo/reset | Local model, undo/redo, grouped slider pointer edits, global reset | Per-control reset buttons/context menus, modified indicators, named Undo/Redo labels, full field commit/cancel semantics, all original reset-preservation rules |
| Save Preview / export | Option dialog sketches, disabled write action | Dedicated preview save flow, destination chooser, full format/bit-depth/quality/resizing/metadata controls, remembered choices, progress and cancel/error states |
| LUT | Dialog works without a photo | Complete settings and file chooser, real output/cancellation |
| Settings/session | Atomic local UI session; isolated test profile; app-only appearance dialog | Complete preferences, recent/session cleanup, preview cache and cache invalidation, startup restoration of developed output |
| Menus/accessibility | Main shortcuts, native control names and AT-SPI value/action checks | Full command inventory/enablement, missing shortcuts, crop/strip accessibility, keyboard/pointer/screen-reader usability across all states |

The sample values in `fixtures.rs` are not engine defaults. The first Post
sketch had temperature/tint placeholders and different tone ranges; that has
been corrected to the original five Post controls. Existing saved prototype
values remain local UI state until the real parameter schema is connected.

The six-film/four-paper menus are deliberately recorded as samples; the
repository currently has 23 camera-film profiles and 10 print-stage profiles.
The live helper catalog must decide their grouping, order, defaults and pairing.

The installer is `python3 linux/install_preview.py` after building. It installs
only in the invoking user's XDG locations. On a machine with the current
Hyprland Lua config, it installs/includes the Negs-only opacity rule and reloads
the running compositor with an error check. It does not select default file
associations. On another compositor, the normal opaque GTK surface applies;
an administrator/user-imposed compositor opacity rule remains outside GTK's
control. This is not yet a published or validated cross-distro package.
