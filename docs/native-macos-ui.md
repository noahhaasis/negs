# Native macOS UI

## Plan

1. Add a SwiftUI application targeting macOS 14+, with a standard unified
   toolbar, resizable inspector, grouped forms, system typography, SF Symbols,
   light/dark appearance, keyboard commands, and native open/save panels.
2. Host the preview in an AppKit `NSScrollView` for trackpad magnification,
   scrolling, drag panning, fit and actual-size views. Explicitly tag the image
   as sRGB so macOS can color-manage wide-gamut displays.
3. Keep the numerical engine in Rust. Add a persistent, versioned JSON-lines
   helper retaining the input, calibrated pipeline, GPU backend, and output.
   Share calibration-cache and resolution policy with the existing GUI.
4. Port all existing adjustment sections, profile pairing, B&W development
   times, RAW redevelopment, rotation, preview save and f64 CPU export.
   Debounce edits, reject stale responses, surface errors, and terminate child
   processes on cancellation/quit. Keep preview and export resolution separate.
5. Make the macOS packaging entry point build the native app and both helpers.
   Retain the egui application for Windows/Linux. Verify Swift compilation,
   protocol behavior, Rust regression tests, real rendering, export, and bundle.

## Design references

- [Apple: Inspectors in SwiftUI](https://developer.apple.com/videos/play/wwdc2023/10161/)
- [Apple: NSScrollView](https://developer.apple.com/documentation/appkit/nsscrollview)

Use platform controls and semantic colors, without custom-drawing imitations
of macOS chrome. The image is the primary content; adjustments live in the
trailing inspector. The app uses one editing window and a recent-files menu.

## Implementation

All five steps are implemented. macOS packaging now launches the Swift app;
the cross-platform egui binary remains buildable but is unmaintained: it gets
no new work or fixes, and the macOS app will later be ported to other platforms. The minimum native-app OS is
macOS 14 (Observation and AppKit's inspector toolbar separator). The packaging script bundles the f32 Rust
helper, f32 CLI, f64 exporter, profile/LUT data, and source archive, then ad-hoc
signs the app. It does not notarize it.

The helper uses serial, newline-delimited JSON on stdin/stdout. `catalog` returns
protocol version 1, the complete Rust defaults, and profile metadata. `render`
returns dimensions/timing and writes an RGBA display buffer into the client's
private temporary directory; `save` writes the retained float output and checks
the requested render ID. Diagnostics go to stderr. EOF releases the engine.
Input decoding, calibrated pipelines, and the source resampled to the working
size ([Image Size](image-size.md)) or 12 MP limit are cached, so an edit that keeps the image size skips
the full-resolution resample (at 0.5 working resolution on a 10 MP JPEG, about
420 ms per edit before, 90 ms after). Both UIs use the same
`spektrafilm-core::preview` cache key and resize policy.

The Swift model renders one request at a time and coalesces edits made in the
meantime into the next request. While a slider is dragged, each finished render
is shown and the drag is one undo step; otherwise only the latest edit's render
is shown, so a discrete change never flashes an older state. The current preview
stays visible during an update; an "Updating" badge appears only if the update
takes longer than 400 ms. Opening a photograph shows an ImageIO thumbnail (the
embedded preview for RAW files) under a "Developing" badge until the first
render arrives. RAW WB changes redevelop the source.

UI responsiveness depends on a few SwiftUI/AppKit details:

- Sliders are continuous and snap in their binding. A stepped SwiftUI `Slider`
  draws one tick mark per step on macOS; the temperature slider alone had
  ~23,000, and laying them out took about 12 s at launch and again on edits.
- The model uses Observation, and each parameter path has its own observable
  value, so an edit re-evaluates only its own row, not every control.
- The split view sizes the canvas and inspector columns. Their hosting
  controllers have no sizing options, so SwiftUI never measures the form for
  the window's size limits (SwiftUI's own inspector container did, at a few
  milliseconds per edit).
- Previews are sRGB-tagged IOSurfaces built off the main thread and shown as
  Core Animation layer contents. A `CGImage` was converted on the main thread
  at commit (~35 ms per 10 MP frame); `NSImage` drawing redrew on every zoom.
- Finder opens arrive at the app delegate's `application(_:open:)`. Under the
  SwiftUI app lifecycle, overriding it made Stage Manager swap the window off
  stage; under AppKit's it doesn't (checked with Stage Manager on).

## Startup

Session restoration first checks a single lossless rendered-preview cache in
`~/Library/Caches/dev.negs.app`. Its SHA-256 identity includes the rendering
settings and file identities for the photograph, helper and engine data.
Crop, rotation and borders are reapplied by the canvas from the saved session.
A match appears without flashing the raw thumbnail or showing an Updating
badge. The helper still renders in the background to restore its float output
for Save Preview and Fast export; saving waits for that render as before.

Rendering edits invalidate the disk frame immediately through the serial cache
queue. Only a completed, current, noninteractive render can publish a new frame,
using an atomic rename and a pixel checksum. Quit drains pending cache I/O.
If an edit was unfinished at quit, or the cache is missing, corrupt or stale,
startup uses the raw thumbnail and Developing badge as before. Only one display
frame is retained; float output and undo history are never persisted.

AppKit runs the application, menus, window, and toolbar
(`EditorWindowController`, `MainMenu`); the canvas and inspector are SwiftUI
views in `NSHostingController`s. Measured on an M1 Pro on battery, from launch
to the window on screen (median of 10):

| | Window | Responsive |
|---|---|---|
| SwiftUI `App`, toolbar, and `.inspector` | 925 ms | ~1,000 ms |
| AppKit shell, adjustments in the first frame | 780 ms | 780 ms |
| AppKit shell, adjustments once the window is on screen (shipped) | 550 ms | ~800 ms |

- Main-thread layout is nearly all of it, not the Rust helper. SwiftUI toolbar
  items cost ~20 ms each with SF Symbols (7 of them ~150 ms); the same
  `NSToolbar` costs ~36 ms. The adjustments form costs ~220 ms with every
  group collapsed and more per expanded group.
- On macOS 26, standard controls are expensive whichever framework creates
  them: an `NSSlider` runs a small SwiftUI graph per instance (~4 ms each),
  and a 33-item pop-up measures every item (~19 ms). Fewer controls in the
  first frame is what helps, not rewriting them in AppKit.
- The adjustments column is empty for the window's first frame and fills in
  once the window is on screen, with nothing moving.
- `EditorModel.launch()` starts the helper and requests the catalog
  synchronously in `applicationWillFinishLaunching`. The last catalog is
  cached in UserDefaults, keyed by the helper binary, so the inspector shows
  the stocks at once and isn't rebuilt when the helper answers. Edits are
  possible as soon as the catalog is known; rendering waits for the helper.
- With that cached catalog, the session is read (a few kilobytes, on
  another thread, waited for at most 0.1 s so a photograph on an
  unresponsive volume can't hold up launch) and restored in
  `applicationWillFinishLaunching`, and the saved frame and its histogram
  are read on a background task while AppKit builds the window.
  `EditorModel.finishLaunching`, in `applicationDidFinishLaunching`, takes
  the frame, waiting at most 0.15 s, before the window is ordered in, so the
  first frame shows the photograph. Waiting for the helper's catalog first,
  or for a main-actor task, showed it about 0.4 s later: Swift's main-actor
  tasks don't run until AppKit's back-to-back launch passes finish.
- That restore is provisional until the helper answers. A Finder open
  arrives between `applicationWillFinishLaunching` and
  `applicationDidFinishLaunching` and undoes it (`discardLaunchRestore`),
  back to no photograph and the defaults, so the chosen photograph starts
  from the defaults and a failed open leaves nothing to save over the
  session. A different catalog from the helper also undoes it and restores
  the session under that catalog, as a launch without a cached one does.
- Measured on 2026-11-01 (M1 Pro, CI-equivalent SDK 15.5 build, a CR2
  session on the Post tab): photograph visible 675 → 265 ms from exec, main
  thread settled 707 → 569 ms. The inspector's first build (~180 ms of the
  rest) is now the largest cost; making it cheaper would change its look
  (a lighter layout than the grouped `Form`, ~90 ms; a custom slider instead
  of `NSSlider`, ~85 ms), which was declined.

Borders (solid, film frame, keyline, 35mm) are presentation only. They
are not render parameters, so a border change never re-renders: the canvas
draws them with Core Animation (the film texture `data/borders/film.png` is a
`contentsCenter` nine-patch). Solid and keyline share a mat width and colour;
keyline adds a line width and colour; film's width adds black around the
frame. Widths are fractions of the long edge (the inspector shows percent),
colours 8-bit sRGB `#rrggbb`. Export and Save Preview add them only when
"Include in export" is on, via `--border`, `--border-width`, `--border-color`,
`--keyline-width` and `--keyline-color` on the f64 exporter and `border` plus
`border_options` on the helper's `save`, using `spektrafilm-io::border` after
rotation. Both sides share the same proportional geometry; `BorderTests` pins
the Swift sizes to the Rust tests' values.

Crop (toolbar crop button, ⌘K) is presentation only as well. Crop mode shows
the whole developed photograph without its border, with a dimmed outside,
corner and edge handles, a thirds grid while dragging, and an inspector panel
for Original (the default), Freeform, Square, 16:9, 3:2, 4:3, 5:4, 7:5, and
Custom ratios with a landscape/portrait switch. A dial below the photograph
(and an Angle slider in the panel) straightens it by up to 45° either way,
like the iPhone's: the crop shrinks about its centre to stay on the turned
photograph, and grows back if the angle returns within the same drag. Done
(Return) applies the crop and any rotation made while cropping as one undo
step; Cancel (Escape) discards both. The rectangle is stored as a fraction of
the unrotated photograph, in the frame turned by the angle, so it turns with
quarter rotations. The canvas clips the photograph with a masking layer;
Save Preview (`crop` on the helper's `save`) and Export (`--crop
x,y,w,h,angle` on the exporter) cut the same pixels with
`spektrafilm-io::crop` (bilinear when straightened) before rotation and the
border. `CropTests` pins the
Swift rounding to the Rust tests' values.

Show Original (the split-square button at the right of the status bar, or
holding `\`) swaps the photograph layer's contents for the undeveloped
photograph while held; the crop, rotation, and border stay, so nothing moves.
RAW files show Apple's default RAW look (`CIRAWFilter` with lens correction
off, matching the engine's framing); other files show the file itself through
ImageIO, with its color profile. The app decodes it at the developed preview's
size the first time the pointer reaches the button, not through the helper,
and keeps it until another photograph opens. While hidden the IOSurface is
purgeable, so macOS can reclaim it and the next press decodes again. A 24 MP
RAW takes about 0.5 s for the first decode in a launch and 0.2 s after that;
a "Loading Original" badge shows while the button is held before it arrives.
Core Image's full-size intermediates (about 1 GB for a 24 MP RAW) become
purgeable a few seconds after each decode.

The preview buffer is capped at 8192 pixels on its longest side; saving retains
the full processed preview resolution. Linear output is encoded only for display
and tagged sRGB. Export runs an independent process: by default the f32
`spektrafilm` with `SPEKTRAFILM_BACKEND=wgpu` (Fast (GPU)), or, when the export
panel's Engine pop-up says Reference (CPU, 64-bit), the f64 reference
(`spektrafilm-f64`, `SPEKTRAFILM_BACKEND=cpu`). The f32 tool is located via `SPEKTRAFILM_F32_CLI`, the app bundle,
then `target/release`; a missing f32 tool only disables Fast export. The choice
is stored in UserDefaults (`exportEngine`); without one, Fast is offered. A fast export whose log shows a CPU
fallback is reported in the status bar, and a failed fast export suggests the
reference engine. A fast export skips the process when the current preview was
rendered at export size (preview not limited, or the photograph within 12 MP):
it saves that preview instead, which is byte-identical to the f32 exporter's
file (render responses carry `export_resolution`; `scripts/check_native_macos.py`
asserts the identity with grain on). If that save fails, for example because an
edit arrived meanwhile, the exporter runs as usual. Either way the export writes to a temporary sibling of the
destination, and renames it only after success. Cancel leaves any existing destination untouched. Quitting kills
both child processes and removes their temporary files.

Temporary files (the preview buffer, engine log, and export parameters) live in a
per-launch folder under `$TMPDIR/Negs`, next to a lock file the app holds
open. After a crash or force quit the lock is released, and the next launch
deletes that folder. It also kills a still-running exporter from that session
(checking its executable path, so a reused process ID is safe) and deletes the
export's `.negs-*` staging file next to the destination.

The inspector includes film/paper profiles, development times, exposure, RAW WB,
halation, DIR couplers, lens diffusion, grain, glare, print curves, scanner, color
management, enlarger, and output options. Default film/paper pairing and direct
scanning follow the profile metadata. Controls and reset participate in Undo.
JPEG inputs now use the same shared loader for CLI export as for preview.

## The zone system

The **Zones** button in the toolbar's view group (Z) shows and hides the zone
panel, a column on the photograph's left edge with a tile for each of Ansel Adams'
eleven zones: 0 at the bottom, 10 at the top. The zones are evenly spaced in L\*, at
zone × 10, which puts zone 5 at 18 % gray. On the negative a zone is a stop, but
five stops above 18 % is far past white, so these are the tones a print compresses
that range onto.

To place a pin, drag a tile onto the photograph and let go. The averaged tone under
the pin then prints as that zone. Dragging zone 0 onto something lighter raises the
black point, and zone 10 sets the white point. Each further pin recalculates the
curve. A zone has one pin, so dragging a placed tile moves its pin. A drop during a
render waits for it and then measures.

On the canvas:

- a placed pin is selected. With a pin selected, the scroll wheel sizes its
  circle, and the tone is measured again when the wheel stops. A new pin starts
  at the size last used.
- a pin is a small dot on the exact spot, with its zone's number beside it, so
  its circle can shrink well below the label. Click the dot or number to select
  it, click empty photograph to deselect, and drag it to measure another spot. ⌫ deletes the selected pin and Esc deselects.
- with nothing selected, plain drag and scroll pan, including inside a pin's
  circle. Double-click fits or shows 1:1. Crop mode has its own gestures.

Placing, moving, resizing and deleting are each one undo step.

Pins and their lines are drawn by `PinOverlayView`, a subview of the canvas's
scroll view above the document. The photograph's zoom, rotation and crop don't
touch it, so a marker is the same size and upright everywhere. It redraws on every
change of the clip view's bounds. A pin whose spot is outside the crop is hidden.

`post.tone_anchors` lives in Post, the finishing stage, which already tabulates
its tone curve once into a 4096-entry LUT on √Y, applied on the CPU at save and by
the same table in the GPU display pass, so the preview and the file agree without
a shader change. The pins bend what the Post knobs produced. The curve is
piecewise linear in L\*: the pins, and an end point for each end, and it must keep
rising. A pin it can't honour, such as zone VI on something darker than zone V's
spot, has no effect and is never removed: `AnchorCurve` is the one rule for this,
the helper reports those pins as `ignored_pins` with each render, and their lines
turn red.

A pin's input comes from the helper's `sample_tone` (`render_id`, `x`, `y`,
`radius`), which reads the retained render through the knobs alone, not the
preview: the preview already shows the other pins' curve.

Because Post runs on the clipped scan, a pin shapes the tones the film produced
but cannot recover a highlight that clipped; the exposure controls do that. Hiding
the panel hides the pins but keeps their effect. A preset neither carries nor
replaces a photograph's pins, a session restores them, and opening another file
starts with none.

## Settings files

File › Save Settings (⇧⌘S) writes everything edited on the photograph, including
the film and paper, every parameter, the pins, crop and border, as readable JSON
beside it: `DSC_0001.json` for `DSC_0001.NEF`. Saving again replaces that
photograph's file. If the name holds another photograph's settings, as a RAW and its
JPEG would, or a JSON file that isn't ours, it writes `DSC_0001.NEF.json` instead and
never overwrites the other file.

Opening a photograph loads its settings file, as one undo step. Only a file saved
for that photograph counts: other photographs' files and other JSON are ignored. A
damaged file, or one naming a film or paper that isn't available, is reported and
the photograph opens without it. A session restored at launch wins over the file,
since it's the newer edit.

## Build and run

From the repository root, with Rust and Xcode installed:

```sh
bash scripts/package_macos_app.sh
open dist/negs-macos/Negs.app
```

For an unpackaged development run:

```sh
cargo build --release -p spektrafilm-native
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
swift run --package-path macos --scratch-path target/swift-native NegsApp
```

Build f32 and f64 targets separately to avoid Cargo feature unification. Open
`macos/Package.swift` in Xcode for Swift editing. When running outside the checkout,
set `SPEKTRAFILM_ROOT` to its absolute path. The app also accepts
`SPEKTRAFILM_NATIVE_HELPER`, `SPEKTRAFILM_F64_CLI`, and `SPEKTRAFILM_DATA_DIR` overrides.
The packaged app locates its executables and resources relative to its bundle.

## Validation

```sh
cargo test -p spektrafilm-native -p spektrafilm-gui
cargo test -p spektrafilm-cli --test process_failures
swift test --package-path macos --scratch-path target/swift-native
python3 scripts/check_native_macos.py
python3 scripts/verify_macos_package.py --zip dist/negs-macos.zip --output-dir /tmp/negs-package-check
```

Swift tests cover full-schema preservation, inspector paths against Rust defaults,
profile metadata, protocol recovery, IOSurface preview conversion, rapid edits
while a render is in flight, undo/redo including one step per slider drag,
slider snapping, canvas rotation, cancellation preserving an existing output,
cleanup of files and exporters left by a crashed launch, and the export engine
choice (tool lookup, binary and backend, CPU-fallback detection). The zone
tests pin the ladder, the gesture map, the overlay's chips and links through zoom,
rotation, pan and crop, and, through the release helper, that a dropped zone prints
at its tone with print compensation on and off, that later pins read the tones
before the curve, that a drop during a render lands, that a crossed pin is marked and
recovers, and one undo step per action. The engine's own tests cover the curve
(order-independent end points, reported pins, a rising map) and the measurement.
`testRealExportsFromJobList` runs real exports through `ExportJob` when
`SPEKTRAFILM_EXPORT_JOBS` names a JSON job list (source, destination, params,
film, paper, engine), for end-to-end engine comparisons.
The Python integration check renders real images, checks rotation/resizing,
rejects stale save requests, recovers from an invalid input, scans B&W reversal
film, verifies clean EOF, and exports at f64. Run with `SPEKTRAFILM_BACKEND=cpu`
for CPU or in a normal macOS session for Metal.

An interaction benchmark is compiled only with a flag. It opens a photograph,
drags a slider at 60 Hz for 4 s on the real event loop, and prints how long each
main run-loop pass stayed busy (keep the window on screen, as Stage Manager and
App Nap throttle background windows):

```sh
swift build --package-path macos --scratch-path target/swift-bench -c release -Xswiftc -DSPEKTRAFILM_BENCHMARK
SPEKTRAFILM_ROOT=$PWD SPEKTRAFILM_BENCHMARK_PHOTO=/path/to/photo.jpg target/swift-bench/release/NegsApp
```

On an M-series Mac with a 10 MP JPEG, busy time per pass is about 3–5 ms
median and 20 ms p95 (before: 32 ms median, with the main thread saturated).

It also reports the live frame rate and how long the export-size frame takes
after letting go. `SPEKTRAFILM_BENCHMARK_PARAM=camera.exposure_compensation_ev`
drags Exposure instead of print exposure (its 0.1 EV steps repeat values, so
fewer frames are requested), and `SPEKTRAFILM_BENCHMARK_SHOTS=/tmp/negs-drag`
saves window screenshots during the drag and after letting go.

While a slider moves, the helper renders at the canvas's resolution
(`interactive_scale`: screen pixels per photo pixel at the current zoom), with
every effect on, from an area-averaged copy of the source. The status line says
"Preview". Letting go renders at export size, byte-identical to a render that
never dragged. On an M1 Pro, 24 MP NEF and 60 MP Leica Q3 drag frames at a
≈1960 px canvas take ≈90 ms (11 fps), against 0.85 s and 2.4 s at export size.

An optional native-window smoke test captures light/dark views:

```sh
SPEKTRAFILM_UI_SNAPSHOTS=/tmp/negs-ui swift test --package-path macos --scratch-path target/swift-native --filter LifecycleTests/testNativeWindowSnapshot
```

System material layers cannot be rasterized by AppKit's offscreen cache API.
Set `SPEKTRAFILM_CAPTURE_WINDOW=1` as well for a window-only screen capture when
screen-recording permission is available. Validate trackpad feel, VoiceOver,
wide-gamut display matching, and large camera RAWs interactively on target Macs;
automated fixture tests do not establish those qualities or a 60 fps guarantee.

The film strip below the canvas lists JPEG, PNG, TIFF and supported RAW files
from the current photograph's folder in Finder-style filename order. Click a
thumbnail to open it with the current adjustments (crop and rotation reset,
as with Open). The toolbar's bottom-panel button and View > Show/Hide Film
Strip (⌥⌘F) toggle it; visibility is remembered between launches. During crop
editing the strip is disabled until Done or Cancel.

Folder enumeration runs off the main thread when the strip appears, the
folder changes, or FSEvents reports external additions, removals, renames or
writes. Notifications are batched with 200 ms latency and a one-event buffer;
there is no idle polling. The watch stops when the strip disappears. Live
refreshes preserve the selection and scroll position. A lazy horizontal row requests 192-pixel thumbnails only as
cells appear, through a serial background decoder and a 16 MiB cache. RAW
files use embedded previews only, with a generic photo icon when unavailable.
Offscreen cells release their images; hiding the strip cancels queued work.
File size and modification time invalidate only changed thumbnails, allowing
unfinished copies to recover when later writes arrive. The strip does not use
the rendering engine or retain full-size images.

`swift test --package-path macos --filter PhotoBrowserTests` covers real folder
events, partially copied images, cache reuse, burst batching and idle shutdown.
Set `NEGS_FILMSTRIP_BENCHMARK=1` to compare 1,000-file enumeration with the prior
implementation, or `NEGS_FILMSTRIP_SNAPSHOTS=/tmp/negs-filmstrip` to run the window
smoke test, which opens a newly arrived image through the already visible strip.
