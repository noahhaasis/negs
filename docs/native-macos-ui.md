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
macOS 14 (the SwiftUI inspector API). The packaging script bundles the f32 Rust
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
- The window root and inspector have fully specified frames (including ideal
  sizes), which cuts most of the work AppKit's min-size query did per edit by
  re-measuring the inspector. SwiftUI's own inspector container still measures
  the form, at a few milliseconds per edit.
- Previews are sRGB-tagged IOSurfaces built off the main thread and shown as
  Core Animation layer contents. A `CGImage` was converted on the main thread
  at commit (~35 ms per 10 MP frame); `NSImage` drawing redrew on every zoom.
- Finder opens are handled with `onOpenURL`. Overriding the app delegate's
  `application(_:open:)` makes Stage Manager swap the window off stage.

Borders (white, film frame, white with keyline) are presentation only. They
are not render parameters, so a border change never re-renders: the canvas
draws them with Core Animation (the film texture `data/borders/film.png` is a
`contentsCenter` nine-patch). Export and Save Preview add them only when
"Include in export" is on, via `--border` on the f64 exporter and `border` on
the helper's `save`, using `spektrafilm-io::border` after rotation. Both sides
share the same proportional geometry; `BorderTests` pins the Swift sizes to
the Rust tests' values.

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

The preview buffer is capped at 8192 pixels on its longest side; saving retains
the full processed preview resolution. Linear output is encoded only for display
and tagged sRGB. Export runs an independent process: by default the f64
reference (`spektrafilm-f64`, `SPEKTRAFILM_BACKEND=cpu`), or, when the export
panel's Engine pop-up says Fast (GPU), the f32 `spektrafilm` with
`SPEKTRAFILM_BACKEND=wgpu` (located via `SPEKTRAFILM_F32_CLI`, the app bundle,
then `target/release`; a missing f32 tool only disables Fast export). The choice
is stored in UserDefaults (`exportEngine`). A fast export whose log shows a CPU
fallback is reported in the status bar, and a failed fast export suggests the
reference engine. Either way the export writes to a temporary sibling of the
destination, and renames it only after success. Cancel leaves any existing destination untouched. Quitting kills
both child processes and removes their temporary files.

Temporary files (the preview buffer, engine log, and export parameters) live in a
per-launch folder under `$TMPDIR/Spektrafilm`, next to a lock file the app holds
open. After a crash or force quit the lock is released, and the next launch
deletes that folder. It also kills a still-running exporter from that session
(checking its executable path, so a reused process ID is safe) and deletes the
export's `.spektrafilm-*` staging file next to the destination.

The inspector includes film/paper profiles, development times, exposure, RAW WB,
halation, DIR couplers, lens diffusion, grain, glare, print curves, scanner, color
management, enlarger, and output options. Default film/paper pairing and direct
scanning follow the profile metadata. Controls and reset participate in Undo.
JPEG inputs now use the same shared loader for CLI export as for preview.

## Build and run

From the repository root, with Rust and Xcode installed:

```sh
bash scripts/package_macos_app.sh
open dist/spektrafilm-macos/Spektrafilm.app
```

For an unpackaged development run:

```sh
cargo build --release -p spektrafilm-native
cargo build --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64
swift run --package-path macos --scratch-path target/swift-native Spektrafilm
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
python3 scripts/verify_macos_package.py --zip dist/spektrafilm-macos.zip --output-dir /tmp/spektrafilm-package-check
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
SPEKTRAFILM_ROOT=$PWD SPEKTRAFILM_BENCHMARK_PHOTO=/path/to/photo.jpg target/swift-bench/release/SpektrafilmApp
```

On an M-series Mac with a 10 MP JPEG, busy time per pass is about 3–5 ms
median and 20 ms p95 (before: 32 ms median, with the main thread saturated).

An optional native-window smoke test captures light/dark views:

```sh
SPEKTRAFILM_UI_SNAPSHOTS=/tmp/spektrafilm-ui swift test --package-path macos --scratch-path target/swift-native --filter LifecycleTests/testNativeWindowSnapshot
```

System material layers cannot be rasterized by AppKit's offscreen cache API.
Set `SPEKTRAFILM_CAPTURE_WINDOW=1` as well for a window-only screen capture when
screen-recording permission is available. Validate trackpad feel, VoiceOver,
wide-gamut display matching, and large camera RAWs interactively on target Macs;
automated fixture tests do not establish those qualities or a 60 fps guarantee.
