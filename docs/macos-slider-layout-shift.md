# macOS slider layout shift

Investigated 2026-10-28 from the [forum recording](https://discuss.pixls.us/t/gpu-accelerated-macos-gui/61485/37).
The downloaded recording is 60.24 seconds at 674 × 1716 pixels. Around
2–3.5 seconds, the Enlarger controls move down by 94 video pixels while
the histogram, tab selector, and Reset Adjustments button stay fixed.

## Reproduction

On the affected build:

1. Open a photograph and turn off **Limit preview to 12 MP**. Use **Fit**
   with a photograph larger than the canvas in screen pixels. A smaller
   window or a larger **Longest side** makes this easier to reproduce.
2. In **Lab**, expand **Enlarger**. Scroll until its sliders are visible
   and **Image Size** is above the visible area.
3. Drag **Magenta shift**, holding long enough for a reduced preview to
   finish. Release and wait for the settled render. The controls move down
   during the reduced preview and back up after the settled render.

At 100% zoom, the renderer skips the temporary resolution reduction, so
this trigger does not occur. Render timing affects how easily it is seen.

The mechanism reproduces on Apple Silicon running macOS 26.5.2. The
regression test uses a 96 × 64 PNG enlarged to a 2400-pixel longest side
in a 1100 × 860 native window; this forces different drag and settled
resolutions without requiring a large fixture. Confirmation on the
reporter's Intel Mac remains outstanding. Windows is a separate investigation.

## Cause and fix

`ImageSizeSection` conditionally displays an explanatory paragraph when
`previewScale < 1`. Each reduced drag frame changed `previewScale`, inserting
the paragraph into the form above the visible sliders. The settled render
removed it again. The form's scroll offset stayed constant while the rows
below the paragraph moved.

Before the fix, the native-window test measured:

| Phase | Form height (pt) | Print exposure Y in inspector (pt) | Scroll offset (pt) |
| --- | ---: | ---: | ---: |
| Before drag | 1563.5 | 352.5 | 812.5 |
| Reduced preview | 1610.5 | 399.5 | 812.5 |
| Settled render | 1563.5 | 352.5 | 812.5 |

The 47-point movement matches the recording's 94-pixel movement at 2× scale.

`EditorModel.previewScale` now describes the settled preview. Temporary
drag frames leave it unchanged. The Image Size notice remains stable
throughout a drag and still reports the settled preview's reduction when
the 12 MP limit or display texture cap applies.

## Validation

```sh
# Requires target/release/spektrafilm-native.
swift test --package-path macos --filter InspectorLayoutTests
swift test --package-path macos
```

`InspectorLayoutTests` drives the model's interaction lifecycle through the
real native window and Rust helper. It measures a slider's position, form
height, and scroll offset before, during, and after rendering. Both an
unlimited preview and a preview capped at 12 MP are covered. Optional
`NEGS_INSPECTOR_LAYOUT_SNAPSHOTS=/path/to/folder` saves inspector images;
AppKit's offscreen capture omits some system material and control layers.

The original reproduction failed on form height, slider movement during
the drag, and slider movement at release. After the fix, all three measured
positions stayed constant. Both layout tests pass. The full macOS suite
ran 79 tests with 6 opt-in tests skipped and no failures.

## Linux counterpart

Linux's Image Size readout had a related trigger: it replaced its preview
notice with “Developing the current adjustments…” whenever the retained
revision became stale. This changed the wrapping above the sliders on edits,
drag frames, and release. Computing the notice from a temporary frame could
also report the drag resolution instead of the settled preview resolution.

The Linux runtime now stores the settled preview scale separately from the
displayed frame. Edits and interactive frames preserve it; settled renders
and cached preview restoration update it. Changing the source clears it.

The opt-in GTK regression uses the actual inspector and Magenta shift slider
with synthetic helper replies, independently of engine timing. It checks the
readout, form height, slider position, and scroll offset before an edit, while
queued, during a reduced preview, at release, and after settling. Both full
and reduced settled resolutions are covered, along with source replacement.
With the old readout logic, the queued edit increased the form height from
2219 to 2236 logical pixels and moved the slider from Y=400 to Y=417 while
the scroll offset stayed at 661. With the fix, those measurements stay
constant throughout each interaction.

```sh
bash linux/cargo.sh test --locked --offline
# Requires a desktop; do not set NEGS_UI_ONLY.
source linux/desktop-env.sh
bash linux/cargo.sh test --locked --offline slider_previews_keep_inspector_layout_stable -- --ignored --test-threads=1
```

The Linux suite passed 26 tests with 5 opt-in tests skipped. The explicit
GTK layout regression also passed.
