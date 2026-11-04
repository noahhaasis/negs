# Zone panel design

## Why

Placing tones by clicking histogram bands was indirect, and the pins were hard to read. The user wants to act directly: take a zone and put it on the part of the photograph that should become that tone, and see that connection on screen.

Success is that someone can drag zone 0 onto a shadow and watch the black point rise, add more zones, and always see which zone each spot is tied to.

## What it does

### The panel

- A **Zones** button in the toolbar's view group, with zoom and crop, shows and hides the panel.
- The panel is a column on the left edge of the photograph, about 56 points wide, spanning the canvas height. It sits above the film strip, not beside it.
- It holds nine tiles, one per zone: 0 at the bottom, then I to VIII with VIII at the top. Each tile is filled with its zone's grey and shows its Roman numeral.
- A tile shows a small dot when its zone is placed, and the selected pin's tile is outlined in the accent colour.
- The inspector stays on the right and the film strip stays below, running under both the panel and the photograph.

### Placing a pin

1. Press on a tile and drag onto the photograph. A line follows the pointer from the tile, ending in the pin and its averaging circle, at the size last used.
2. Letting go over the photograph places the pin and selects it. Letting go anywhere else cancels.
3. Scroll to size the placed pin's circle. The tone is measured again as it changes.
4. The pin's averaged tone becomes that zone in the tone curve, and the photograph updates. Dragging zone 0 onto something that is really zone III raises the black point. Each further pin recalculates the curve.

### Working with a placed pin

- Click a pin to select it. Click empty photograph to deselect.
- Drag a selected pin to measure a different spot. Its line and circle follow from where it was grabbed.
- Scroll to resize the selected pin's circle. With nothing selected, scrolling pans as it does today. Scrolling does nothing during a drag.
- Delete removes the selected pin, and Esc deselects.
- Dragging a tile that is already placed moves that zone's pin: a zone has one pin.
- Each drop, move, resize gesture and delete is one undo step. A resize is one step however many scroll events it takes.

### Reading the pins

Pins must stay readable on any part of any photograph. Each pin is a chip about 24 points across on screen at any zoom, upright at any rotation, filled with its zone's grey and labelled with its zone's numeral, with a light edge and a dark outer outline. Lines and circles have the same light-on-dark casing. The selected pin's edge, line and circle use the accent colour.

Lines run from the tile to the pin, follow pan and zoom, and clip at the canvas edge when the pin is out of view.

### Pins the curve can't honour

The tone curve must keep rising, so some pairings are impossible, such as zone VI on something darker than zone V's spot. A pin in that state has a red line, has no effect, and starts working again as soon as moving a pin resolves it. It is never removed automatically.

### Where a pin lands

At the middle of its zone in L\*, except zone 0, which is L\* 0, and zone VIII, which is L\* 100. These are the existing `ZoneScale` targets.

### Hiding the panel

Hiding the panel hides the tiles, lines and pins, but keeps their effect on the photograph. Pin gestures do nothing while it is hidden.

### What goes away

- The ⌘-click, ⌘-scroll and ⌘' pin gestures, and the Show Tone Pins menu item. The toolbar button and a menu item for the panel replace them.
- Holding pins: every pin now belongs to a zone.
- Clickable histogram bands. The inspector goes back to the plain histogram from `feat/inspector-tabs`, along with any lightness binning and curve code only the zone chart used.
- The pin-specific captions under the histogram.

## What stays

- `post.tone_anchors` as the stored form, `AnchorCurve` as the curve, and `sample_tone` as the measurement, read before the pins' own curve.
- Pins sit in Post, so they cannot recover highlights the film clipped.
- Pins are saved per photograph, never carried by presets, and start empty on a new file.
- A pin's circle uses the same units as today: fractions of the uncropped photograph, with the radius a fraction of its shorter side.

## Goals

- Everything under "What it does", in the macOS app.
- Export matches the preview, as the parity check already asserts.

## Non-goals

- Windows, Linux and web interfaces. They keep working from the same params.
- Merging `origin/main`, or its Prep | Lab | Output layout, which the user rejected.
- Curve shapes other than the existing piecewise-linear one.
- Placing pins without a pointer. Tiles and pins still get VoiceOver labels.

## Anti-goals

These would make it a failure even if it technically worked:

- A pin that silently does nothing, or a drop that silently vanishes.
- Pins or lines that disappear against the photograph's tones.
- Panning that stops working because the pointer is over a large circle with nothing selected.
- Inspector or canvas layout jumping when pins are placed.

## Constraints

- Existing macOS 14 deployment target, SwiftUI inside the AppKit window and split view.
- The canvas is an AppKit scroll view and the panel is outside it, so the lines must be drawn in a layer spanning both.
- `sample_tone` only answers for the current render. A drop or move made while a render is running must wait for that render and then measure, not be lost as `measureTone` currently allows.
- No names, addresses or text from the reverse-engineered source in this repository.

## Left to the builder

- How the line overlay is built, as long as it follows pan and zoom during live gestures.
- How the app learns which pins the curve ignored: asking the helper or sharing the rule. There must be one source of truth, and `AnchorCurve` currently drops those pins silently.
- Debouncing re-measurement while scrolling.

## Calls Claude made

These were not specified by the user:

- The panel is shown by default on first launch, then remembered.
- The canvas's 680-point minimum width includes the panel.
- The panel and pin gestures are unavailable while cropping or showing the Original.
- A new pin starts at the size last used, or a radius of 2% of the shorter side the first time.
- Pins are 24 points, up from 18, because 18 was reported as hard to read.
- Pins saved by earlier test builds with a "holding" target take the zone that target falls in.
