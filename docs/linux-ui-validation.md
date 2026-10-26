# Linux UI preview validation — 2026-10-25

This report covers the UI-first scope requested after the Linux handoff.
It establishes a runnable native interface and initial UI performance evidence.
It does **not** establish film-engine correctness, full feature parity, export
behavior, or production readiness.

## Film-strip selection follow-up

Fixed the strip flashing/reloading on photo selection: same-folder navigation
now keeps the existing thumbnail widgets, only updates the selection highlight,
and keeps the canvas visible until the new image is ready. Selecting the current
photo no longer reloads it or clears undo history. Thumbnail work uses a separate
folder generation so rapid selections neither cancel it nor publish stale images.
Fast opens do not flash a loading status; that status is delayed by 400 ms.

Release build, two state tests and Clippy pass. The real-desktop AT-SPI suite
now passes eight checks, including three completed photo switches with unchanged
thumbnail accessibility-object identities, repeated current-photo clicks with
working Undo, and rapid selection/reselection rejecting stale loads. Evidence
is in `target/linux-validation/ui-accessibility/result.json`. The open review
instance was gracefully restarted to use the fix and restore its UI session.
The startup measurements below describe the initial prototype build, rather
than a new timing run for this follow-up.

## Tested build and machine

The following startup figures describe the initial prototype. Subsequent fixes
and their separate validation are noted above and below.

- Source commit: `1aa6d08` (`Add native GTK Linux UI preview and desktop performance harness`).
- Rust: 1.93.1 (`01f6ddf75`, 2026-02-11); release build, locked separate UI workspace.
- Executable: `target/linux-ui/release/negs`, approximately 1 MiB, dynamically linked.
- Executable SHA-256: `3f0b0f97bdb0e40392083e956cf635add073ebdf30cd35763d229a4bca693d95`.
- Omarchy 4.0.6, Linux 7.2.8-9-omarchy, Hyprland 0.56.2-4, Wayland.
- Ryzen 5 3600, 6 cores / 12 threads, 16 GiB RAM.
- NVIDIA GTX 1660 SUPER, 6 GiB VRAM, NVIDIA driver 615.78.08.
- GTK 4.24.1, libadwaita 1.10.0. GTK uses `GskVulkanRenderer`.
- Dell P3424WEB, 3440 × 1440 at about 59.97 Hz, scale 1.25.

`GDK_DEBUG=vulkan` identified the discrete NVIDIA device, selected device 0 /
queue 0, and loaded GTK's existing pipeline cache. `nvidia-smi` separately
showed the running Negs process with 37 MiB GPU memory after settling at
1320 × 900 logical pixels. This is GTK presentation acceleration; the
Spektrafilm GPU path has not been connected or tested.

## Startup measurements

Five fresh processes per inspector variant, including one explicit photo
launch and four restored UI sessions each. Normal Hyprland tiling produced a
2728 × 1102 logical-pixel window. The input is the embedded Nikon camera JPEG
from the transferred NEF, with original layout size 6048 × 4024; the display
texture is limited to 2400 pixels on the long edge. The folder contains six
real validation photographs prepared by `prepare_demo.py`.

All times below are milliseconds from immediately before `Popen`, using the
same Linux monotonic clock as the application. No compiler ran concurrently.

| Milestone, median of five | Deferred inspector (14 sliders) | Eager inspector (57 sliders) |
|---|---:|---:|
| First frame presented, compositor feedback | 290.7 | 292.1 |
| Inspector controls constructed | 294.8 | 307.8 |
| First real keyboard event received | 330.3 | 314.6 |
| Idle dispatch following that input | 335.5 | 360.2 |
| Camera preview frame presented | 668.6 | 699.7 |
| Peak process RSS, MiB | 262.9 | 266.2 |

Deferred first presentation ranged from 278.7 to 337.8 ms. Its preview
presentation ranged from 657.5 to 708.4 ms. Both variants defer their inspector
until the shell's first paint, so this comparison does **not** demonstrate a
meaningful first-paint improvement from lazy sections. The interval from first
frame submission to inspector construction was 19.5 ms median deferred and
31.3 ms eager; that interval includes the GLib scheduling boundary.

The first input was an F8 generated through Wayland by `wtype`, after the
runner verified the target window's PID/focus. It was delivered in all ten
launches. Deferred launch-to-reception was 295.4–382.8 ms; reception depends
on when the runner observes the window. The median 39.6 ms between starting
`wtype` and receiving the event includes subprocess/protocol overhead and
is **not** a measurement of GTK's own event-handler latency. The following
idle-dispatch timestamp also includes other work before GLib becomes idle.

These are repeated-process launches with **existing OS, driver and GTK shader
caches**. The first exploratory launch was around 703 ms to presentation;
that was a different UI revision and is not a controlled cold-start baseline.
No reboot-cold launch, empty driver cache, or empty GTK pipeline-cache
comparison was performed. There is no developed-image timing because the
film engine is disconnected.

Raw reports: `target/linux-validation/ui-performance-final/{lazy,eager}-*.json`
and `summary.json`. Earlier exploratory reports are retained in separate
directories; they are not the final benchmark.

## Interaction and idle

Three further launches ran 240 frame-clock-driven slider/value updates while
zooming the existing photo texture on each update:

| Run | Updates | Duration | Callback median / p95 / max |
|---|---:|---:|---:|
| 1 | 240 | 3965 ms | 0.007 / 0.009 / 0.028 ms |
| 2 | 240 | 3964 ms | 0.007 / 0.011 / 0.051 ms |
| 3 | 240 | 3967 ms | 0.007 / 0.010 / 0.039 ms |

This is approximately the monitor's 60 Hz cadence. Callback figures cover
the local control/model update, not a full frame's GPU time. This test does
not emulate a physical pointer drag, include film rendering, prove end-to-end
60 fps under engine load, or establish latency between physical input and
display. RSS was 262.4–262.8 MiB in these runs.

Reports: `target/linux-validation/ui-interaction-final/`. A separate settled
two-second idle sample recorded no accounted CPU ticks in `/proc/PID/stat`;
this is a coarse single sample, not an exact zero-CPU guarantee. Evidence:
`target/linux-validation/ui-idle.json`.

## Functional and visual checks

- Release build, formatting, and Clippy with `-D warnings`: pass.
- Two state tests: pass (240-value gesture becomes one undo step, exact
  restoration, redo invalidation, no-op edit handling).
- Native AT-SPI integration: five checks pass. An actual GTK numeric control
  accepts and reports a value; Undo restores it; Redo restores the edit; Reset
  restores the UI fixture; the platform tree contains named controls/actions
  and the photograph canvas. The test explicitly enables GTK's AT-SPI bridge
  in its process. No screen-reader usability claim is made.
- Real Wayland F8 reception: 10/10 startup launches.
- Inspected GTK widget captures: dark Lab, light Post with keyline, crop,
  export dialog, 760-pixel narrow layout, and 480-pixel compact layout. Below
  the breakpoint the inspector hides and remains available through F7.
- Export-dialog test closes cleanly after force-closing its modal in the
  bounded test shutdown path.
- Actual compositor screenshot of the review window inspected at 125% scale;
  the app remains open for the user. Desktop compositor effects are present
  in that image, unlike the widget-only captures.

The configured `Yaru-gray` icon theme omitted undo/redo, rotate, and inspector
icons. The UI now adds installed Adwaita editing icons as an application-local
fallback. The corrected toolbar was inspected. No global theme/configuration
was changed.

The initial AT-SPI test looked for the canvas before its photo had loaded;
the final test waits for it and assigns the custom canvas an image role.
The modal shutdown defect and GTK sizing warnings from custom slider geometry
were fixed. The final ten startup logs contain no GTK warnings.

Accessibility evidence: `target/linux-validation/ui-accessibility/{result,tree}.json`.
Build/test outputs and the numerical engine baseline are separate concerns;
no engine baseline has been generated in this UI-only scope.

## See and run the result

```sh
bash linux/run.sh --demo --fresh
```

The running review instance uses an isolated profile under
`target/linux-validation/ui-review-profile`. Its PID is recorded in
`ui-review.pid`; close it normally when finished.

Local visual artifacts (intentionally ignored rather than committed with photos):

- [Actual Wayland compositor capture](../target/linux-validation/ui-desktop.png)
- [Current dark widget capture](../target/linux-validation/ui-review-widget.png)
- [Light Post/keyline view](../target/linux-validation/ui-visual-light/lazy-widget.png)
- [Crop view](../target/linux-validation/ui-visual-crop/lazy-widget.png)
- [Export options](../target/linux-validation/ui-final-export/lazy-widget.png)
- [Compact 480-pixel view](../target/linux-validation/ui-final-compact/lazy-widget.png)

The light/crop/export captures were made during iteration; the final icon
fallback is shown by the review and compact captures. The reports include
actual sizes and scale rather than assuming the requested default window
size controls a tiled window.

## Remaining validation

### Opaque photograph follow-up

Omarchy's default-opacity tag applied whole-surface opacity of 0.985 focused
and 0.96 inactive, so even opaque image pixels showed the desktop behind them.
A Negs-only user rule now removes that tag and sets focused, inactive and
fullscreen opacity to `1 override`. The rule and an idempotent installer live
in `linux/omarchy/negs-window-rules.lua` and `linux/install_omarchy_rules.py`.
The user configuration was backed up before installation; `hyprctl reload`
succeeded and `hyprctl configerrors` was empty. The canvas now independently
paints an opaque neutral background before its texture.

A test with an alpha-zero PNG region and an opaque colored region compared
actual focused/inactive Wayland captures against GTK's rendered pixels at
125% scale. Both states matched exactly: background RGB (24, 25, 27) and image
RGB (80, 120, 160). The compositor reported opacity 1, with no default-opacity
tag. Evidence: `target/linux-validation/opacity-check/result.json`. Release
build, Clippy and the state tests pass. The compositor exception was also
applied to the open review window and persists across launches.

### Configurable borders and installed-preview follow-up

Validated source `7b3ae74` with Rust 1.93.1 and the same actual Wayland
desktop. The release executable's SHA-256 was
`aac392424db492245bb183eb99483a0c5ad2f19fc222b31aa74042118f3f0277`.
Later UI work should record its own revision/results rather than treating
this follow-up as validation of that work.

Borders now expose mat, keyline and outer-film thickness; opaque native sRGB
color pickers and hex entries; Solid/Film/Keyline/35mm; Instagram extension;
and include-in-export state. Film and 35mm previews use the licensed project
textures. The original Post controls and ranges have also been restored.
No Spektrafilm render or export is connected in this tested revision.

Release build, formatting, Clippy with `-D warnings`, and four state/border
tests pass. The real AT-SPI regression suite passes all eight checks,
including stable film-strip object identities and stale-load rejection.
Four additional border checks pass: native numeric thickness controls with
Undo/Redo; hex entry activation changes the actual compositor pixels to
`#233445`; thickness/color are saved; and native fields restore correctly
on the next launch. Evidence:
`target/linux-validation/border-controls/result.json`,
`configured-keyline.png` and the existing accessibility report.

Inspected widget captures show the scanned film edge and stock-specific
35mm rebate/perforations, plus light-theme keyline controls:
`target/linux-validation/{border-film,border-35mm,border-keyline}/lazy-widget.png`.
These are presentation checks, not proof of export geometry/pixel parity.

The per-user preview installer was run repeatedly with isolated XDG
config/data/bin roots. Its main-config include remains idempotent; border
assets, license, source archive and desktop integration are present. The
installed copy then launched in the real Wayland session and loaded 35mm
assets from its own installed data directory. Evidence:
`target/linux-validation/install-smoke/{result,installed-run}.json` and
`installed-35mm.png`. Follow-up packaging also includes the UI audit and
validation documentation alongside the installed README and source archive.
The isolated smoke did not install the application into the user's actual
application directories. Arch/other-distro dependencies and packaging remain
separate work; this installer is explicitly for the UI preview.

One additional interaction run with the expanded border inspector completed
240 frame-clock updates in 3967.1 ms; callback p95 was 0.011 ms. The deferred
inspector initially constructed 17 sliders (60 with all groups eager), versus
the historical 14/57 above. First compositor presentation was 341.7 ms and
camera-preview presentation 725.2 ms in this one run. It used existing
OS/driver caches and is not a repeated median or a cold-start comparison.
Evidence: `target/linux-validation/border-ui-performance/lazy-00.json`.
`developed_image_ms` remains null.

### RAWmakase opacity comparison

A separate read-only investigation confirmed that current RAWmakase upstream
also documents an Omarchy-specific window rule removing `default-opacity`
and setting opacity to `1 1`. Its application creates an opaque Wayland
surface and writes preview alpha 1, but those declarations do not bypass
compositor opacity. The inspected packaging and runtime contain no automatic
Hyprland-rule installation. See its pinned
[README](https://github.com/pch/rawmakase/blob/433ce6000cb72b44ceaa51557c6fd4f7b68c7a78/README.md#L58-L62)
and [preview shader](https://github.com/pch/rawmakase/blob/433ce6000cb72b44ceaa51557c6fd4f7b68c7a78/crates/rawmakase-engine/src/develop/gpu/present.wgsl#L291).
The installed package is `rawmakase-bin 0.4.4-1`; that release predates the
README recipe. No RAWmakase process was running, so this comparison is based
on source and package inspection rather than a fresh visual test.

Negs retains the same two-part approach: opaque canvas pixels plus an
app-specific installed rule. Its rule uses absolute `1 override` values for
focused/inactive/fullscreen states. Plain opacity values are multipliers,
as documented in the
[Hyprland window-rule reference](https://wiki.hypr.land/Configuring/Basics/Window-Rules/).

### Still pending

This was the UI-only stage's remaining-work list. Subsequent engine integration
and package validation are recorded in [linux-core-validation.md](linux-core-validation.md).
Use [linux-release.md](linux-release.md) for the current release work queue.

Engine integration, numerical parity, real exports, border/35mm export parity,
straighten semantics, developed-preview caching, large watched folders, RAW
development, color management on wide-gamut/mixed displays, different monitor
scales, screen-reader usability, physical pointer/trackpad feel, and packaging
are pending. The native file picker is implemented and its keyboard shortcut
was exercised, but the portal's contents/selection were not independently
asserted by the AT-SPI test. The detailed implementation limits and future
engine connection points are in [linux-app.md](linux-app.md).
