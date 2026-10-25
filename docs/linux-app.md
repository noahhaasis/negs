# Linux UI preview

The current Linux work is a **UI-only GTK4/libadwaita prototype**, as requested
on 2026-10-25. The Rust film engine, helper protocol, numerical validation,
RAW development and real exports are deliberately deferred. No changes have
been made to the engine or to the macOS/Windows clients.

The client lives in `linux/`, in a separate Cargo workspace and lockfile.
It uses gtk4-rs 0.10.3 and libadwaita-rs 0.8.1, targeting GTK 4.16 and
libadwaita 1.6 APIs. The machine used for validation has GTK 4.24.1 and
libadwaita 1.10.0. The UI builds with Rust 1.93.1.

## Build and see it

```sh
bash linux/cargo.sh build --release --locked
python3 linux/prepare_demo.py
bash linux/run.sh --demo --fresh
```

Run these from the worktree in the graphical session. The executable is
`target/linux-ui/release/negs`. `prepare_demo.py` extracts the real Nikon
camera JPEG and prepares smaller display copies of the transferred TIFFs
under ignored `target/linux-validation/ui-demo/`. It requires ImageMagick;
it does not modify the transferred files. Photos are not compiled into the app.

For a per-user installation of this preview:

```sh
python3 linux/install_preview.py
```

This copies the executable, border assets/licenses and source archive under
`$XDG_DATA_HOME/negs-ui-preview`, installs a launcher and a matching desktop
entry/icon, and offers Open With support without changing defaults. When an
existing Hyprland Lua configuration is found, it also installs the Negs-only
opacity exception, reloads the active Hyprland session, and checks for errors.
The installer is tested with isolated XDG roots; distribution packaging and
dependency installation remain pending. `--no-reload` is for staging/tests.

To open your own JPEG, PNG, TIFF or supported embedded RAW preview:

```sh
bash linux/run.sh /absolute/path/to/photograph.jpg
```

Without an argument, the app restores its last UI session. `--fresh` skips
restoration. `--demo` explicitly selects the validation photograph.
`NEGS_APPEARANCE=light|dark|system` selects an appearance for review; the
default follows the desktop. The in-app Appearance dialog changes only
this application. Missing toolbar glyphs fall back to installed Adwaita
icons; no desktop settings are changed.

`linux/cargo.sh` puts build products in `target/linux-ui`. If Cargo is not
on PATH it uses the isolated Rust installation in `target/linux-toolchain`.
On another machine install Rust normally, plus GTK/libadwaita development
libraries, pkg-config and the Adwaita icon theme. This UI workspace does not
need OpenBLAS, Fortran, or the film engine's dependencies.

On Omarchy, install the app-specific compositor exception so photographs remain
fully opaque when the window is focused, unfocused or fullscreen:

```sh
python3 linux/install_omarchy_rules.py
hyprctl reload
hyprctl configerrors
```

This is installed on the validation machine. The installer backs up changed
files, writes `~/.config/hypr/negs-window-rules.lua`, and includes it after
Omarchy's defaults. It exempts only `dev.negs.app` and its UI-preview variant
from Omarchy's default-opacity tag and sets all three opacity values to an
absolute 1. Other applications keep their desktop behavior. GTK cannot undo
compositor opacity through widget styling. The canvas also paints its own
opaque neutral background, including behind transparent input pixels.

## What can be tried

- Open dialog, file drop, background preview decoding, error messages and a
  bounded folder strip. The strip opens photos with the current local values.
  Switching within a folder retains its thumbnail widgets and scroll position;
  selecting the current photograph preserves its editing state. Thumbnail work
  has a separate folder generation so rapid photo selections do not cancel it.
- Native Lab/Post controls, synchronized sliders and number fields, deferred
  inspector construction, and lazily constructed effect sections. An eager
  inspection mode creates all 60 sliders, including the three border sliders;
  the normal first inspector creates 17. The original startup report predates
  the border controls and counts 57/14 respectively.
- Local undo/redo/reset. Slider pointer events are observed without taking
  over GTK's drag gesture, and one drag is committed as one edit.
- Fit, actual size accounting for fractional surface scaling, pointer-centered
  Control+scroll zoom, panning, rotation, crop corners and crop ratios.
- Configurable Solid/Film/Keyline/35mm borders, thickness and opaque sRGB colors,
  film-specific strip textures, Instagram extension and saved export inclusion;
  light/dark appearance; export and
  LUT option dialogs with the saving action visibly disabled.
- A libadwaita breakpoint changes the inspector into an overlay below 900 sp.
  F7 opens/closes it; F6 toggles the film strip.

Keyboard commands include Ctrl+O, Ctrl+Z, Ctrl+Shift+Z/Ctrl+Y, Ctrl+0,
Ctrl+1, Ctrl++/-, Ctrl+R, and Ctrl+Shift+E. With the canvas focused, C opens
crop; Return accepts it and Escape cancels it. Text-entry widgets own text
input; the plain-letter shortcut is attached to the canvas.

The photographed image remains a camera/file preview when film controls
change. The interface states this explicitly. `fixtures.rs` contains sample
UI values, **not** engine defaults or a second implementation of the parameter
schema. Replace that fixture with the helper's catalog when wiring the engine.

Settings/session data use `$XDG_CONFIG_HOME/negs/ui-preview` (normally
`~/.config/negs/ui-preview`). `NEGS_PROFILE_DIR` isolates tests. Session writes
use a temporary sibling and rename. The preview has its own application ID,
`dev.negs.app.UiPreview`, and currently allows multiple instances for testing.
The preview installer provides a per-user desktop entry and optional Open With
registration. It has not been installed into the validation user's application
directories; tests use isolated XDG locations. No distro package or updater has
been published.

## Validation commands

```sh
bash linux/cargo.sh test --release --locked
bash linux/cargo.sh clippy --locked -- -D warnings
bash linux/cargo.sh fmt --check
python3 linux/check_ui.py
python3 linux/check_borders.py
python3 linux/measure_ui.py --runs 5
python3 linux/measure_ui.py --runs 3 --benchmark \
  --out target/linux-validation/ui-interaction
```

The UI checks and timing runners require an unlocked Hyprland session. Python's AT-SPI/GObject
introspection and `wtype` are used for real accessibility/input checks.
The benchmark understands this machine's Hyprland Lua dispatch API and only
targets the process it launches. It refuses a locked/asleep desktop. It sends
a harmless F8 only after verifying window focus.

For alternate layouts, use `--size 760,820`, `--appearance light`, and
`--scene post|crop|export`. Optional floating/resize operations target only the
test window and do not create persistent window rules. Without `--size`, normal
Hyprland tiling applies. Captures made by `NEGS_WIDGET_CAPTURE` are GTK widget
renders; they are distinct from compositor screenshots.

`NEGS_STARTUP_REPORT=/path/report.json` records separate main-entry, map,
frame-submission, compositor-presentation, inspector-ready, input-reception,
input-idle-dispatch, preview-texture, and preview-presentation milestones.
`measure_ui.py` supplies a monotonic process-launch origin. Without that origin,
times start at main entry. `NEGS_QUIT_AFTER_MS` bounds automated launches and
closes an open Adwaita dialog before the window.

The interaction benchmark drives the real GTK frame clock for 240 updates,
changing a slider and zooming the existing texture. It measures UI callback
cost and frame cadence; it does not simulate physical pointer input or the
film-processing workload. `developed_image_ms` is explicitly null.

See [linux-ui-validation.md](linux-ui-validation.md) for machine evidence,
measurements, screenshots, and remaining limits.
See [linux-ui-parity.md](linux-ui-parity.md) for the explicit control-by-control
UI gaps; this prototype does not yet provide the full original interface.

## Deferred work

Connect the persistent helper and its real catalog/defaults; coalesce renders
and reject stale replies; build the developed-preview cache; implement both
export paths and cancellation; validate border/35mm parity with exports and
carry over the full crop/straighten and Image Size semantics; complete profile
grouping, RAW WB/B&W behavior and
recent files; virtualize and watch folders; implement single-instance file-open
activation and packaging. Then execute the numerical, color, capacity and
full application checks from `linux-agent-handoff.md`.

The current histogram describes the decoded preview rather than a developed,
crop-limited output. RAW support only extracts an embedded JPEG with a bounded
scan; it is not rawler development or a validated RAW orientation/framing path.
The display preview is capped at 2400 pixels and its source-size layout is
retained for zoom; it is not a full-resolution export buffer. Wide-gamut/ICC
and mixed-monitor color correctness are not established by this UI preview.
The folder strip is capped at 24 cells, uses a bounded scan of 200 candidates,
and has no watcher, natural sorting or reusable thumbnail cache yet.
