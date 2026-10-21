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
resolution or 12 MP limit are cached, so an edit that keeps the image size skips
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
choice (tool lookup, binary and backend, CPU-fallback detection).
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
