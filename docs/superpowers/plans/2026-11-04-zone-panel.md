# Zone Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the clickable histogram bands and ⌘ gestures with a zone panel beside the photograph: drag a zone onto a spot, and a linked pin makes that spot's tone that zone.

**Architecture:** The panel is a SwiftUI column directly left of the canvas, so a tile's right edge is the canvas's left edge and no cross-view conversion is needed. All pins and lines live in one overlay view on `ImageScrollView`, above the document and unaffected by its zoom or rotation. The curve stays in Rust, which also reports the pins it can't honour.

**Tech Stack:** Rust (`spektrafilm-core`, `spektrafilm-native`), Swift 5.9, SwiftUI, AppKit, Core Animation, macOS 14.

**Spec:** `docs/superpowers/specs/2026-11-04-zone-panel-design.md`

## Global Constraints

- Nine tiles, 0 at the bottom, VIII at the top, about 56 points wide, spanning the canvas height.
- One pin per zone. A pin targets `ZoneScale.zones[zone].target`.
- Chips are 24 points across at any zoom and upright at any rotation, filled with the zone's grey, labelled with its numeral.
- New pins start at the last-used radius, or `SampleSize.initial` (0.02).
- Each drop, move, resize gesture and delete is one undo step.
- No names or text from the reverse-engineered source in this repository.

## Review Focus

1. A pin whose spot falls outside a new crop: hide its chip and line rather than drawing them over the border (Task 4).
2. Dropping on the border or on another panel tile: cancels, with no pin and no error (Task 4).
3. Opening another photograph while a drop is waiting for a render: nothing lands on the new photograph (Task 3).
4. Undoing after a resize: the radius and the measured tone both go back (Task 3).
5. A selected pin removed by undo: no stale selection, and scrolling pans again (Task 3).

---

### Task 1: Report the pins the curve ignores

**Files:** Modify `crates/spektrafilm-core/src/post.rs`, `crates/spektrafilm-native/src/main.rs`.

**Interfaces:** Produces `AnchorCurve::ignored(&self) -> &[usize]`, indices into the anchors given to `new`. The `render` response gains `"ignored_pins": [index]`, on the reuse path too.

- [ ] **Step 1: Write failing tests** in `post.rs`: `a_crossed_pair_reports_the_one_it_drops` (`[pin(25,60), pin(50,20)]` → `ignored() == [1]`), `order_does_not_change_which_tone_is_kept`, `crossed_end_points_report_the_white_point` (`[pin(70,0), pin(40,100)]` → `[1]`, and the curve still rises), `a_pin_outside_the_ends_is_ignored` (`[pin(30,0), pin(20,50)]` → `[1]`), `non_finite_pins_are_ignored`. In `main.rs`: `render_reports_ignored_pins_when_reusing_the_scan`.
- [ ] **Step 2: Run** `cargo test -p spektrafilm-core --lib post::` and expect a compile failure.
- [ ] **Step 3: Implement.** Carry each anchor's index through `AnchorCurve::new`; the existing rising check replaces `dedup_by`. Exclude `ignored` from equality so order independence still compares the curve.
- [ ] **Step 4: Run** `cargo test -p spektrafilm-core --lib post:: && cargo test -p spektrafilm-native`; expect PASS.
- [ ] **Step 5: Commit** "Report the pins the tone curve can't honour".

### Task 2: Restore the plain histogram

**Files:** Restore `macos/Sources/NegsApp/Histogram.swift` and `macos/Tests/NegsTests/HistogramTests.swift` from `feat/inspector-tabs`. Delete `ZoneHistogramView.swift`, `ZoneHistogramTests.swift`. Move `Zone`, `ZoneScale`, `TonePin`, `PinSource`, `TonePinMarker` to `ZoneSystem.swift`. Make `ToneProbe.swift` self-contained. Test: `macos/Tests/NegsTests/ZoneSystemTests.swift`.

**Interfaces:** Produces `ZoneScale.zones`, `ZoneScale.zone(forTarget:)`, `Zone.target`, and `ZoneScale.grey(_ zone: Int) -> NSColor`. The inspector shows `HistogramView(histogram: model.histogram).frame(height: 88)`.

- [ ] **Step 1: Write failing tests** `testGreysRiseFromBlackToPaperWhite` and move `testTheZonesTileTheAxisFromTheLadder`, `testZoneTargetsSetTheBlackAndWhitePoints` over.
- [ ] **Step 2: Run** `swift test --package-path macos --scratch-path target/swift-native --filter ZoneSystemTests`; expect a compile failure.
- [ ] **Step 3: Implement** the moves and restore. Move the shared L\* to grey conversion from `PhotographView.tone` to `ZoneScale`.
- [ ] **Step 4: Run** the full Swift suite; expect PASS.
- [ ] **Step 5: Commit** "Go back to the plain histogram".

### Task 3: Build the pin model around zones

**Files:** Modify `EditorModel.swift`, `ZoneSystem.swift`, `CanvasGesture.swift`. Test: `ZoneModelTests.swift`.

**Interfaces:**

```swift
struct TonePinMarker { let index: Int; let zone: Int; let source: PinSource; let selected: Bool; let ignored: Bool }
var zonePanelVisible: Bool            // UserDefaults "showZonePanel", default true
var canPlaceZones: Bool               // image != nil, not cropping, Original not showing or held
var zoneDrag: ZoneDrag?               // struct ZoneDrag { zone: Int; point: CGPoint }, canvas points, top-left origin
var sampleRadius: Double              // last used
var pinsToShow: [TonePinMarker]       // empty when the panel is hidden or Original shows
func placeZone(_ zone: Int, at centre: CGPoint) async
func movePin(_ index: Int, to centre: CGPoint) async
func resizeSelectedPin(scrollDelta: Double, precise: Bool)
func selectPin(_ index: Int?)
func removeSelectedPin()
```

Removes `addPin`, `aimSelectedPin`, `showsPins`, `toggleTonePins`, `canPin`, `pinMarkers`.

- [ ] **Step 1: Write failing tests:** `testPlacingAZoneMakesTheSpotThatTone` (spot prints at the zone's target ±1.5 L\*), `testDraggingZoneZeroRaisesTheBlackPoint`, `testASecondPinRecalculatesFromTheOriginalTones`, `testDraggingAPlacedZoneMovesItsPin`, `testPlacingDuringARenderWaitsAndLands`, `testOpeningAnotherPhotographDropsAWaitingPlacement`, `testResizingIsOneUndoStepAndRemeasures` (several scrolls; one undo restores radius and input), `testScrollWithNoSelectionDoesNothing`, `testUndoingAPlacementClearsTheSelection`, `testACrossedPinIsMarkedIgnoredAndRecovers`, `testHidingThePanelHidesPinsButKeepsTheEffect`, `testSavedHoldingPinsTakeTheZoneTheirTargetFallsIn`, plus the existing per-photograph and restoration tests rewritten for `placeZone`.
- [ ] **Step 2: Run** `swift test --package-path macos --scratch-path target/swift-native --filter ZoneModelTests`; expect a compile failure.
- [ ] **Step 3: Implement.** `measureTone` loops: wait for `previewCurrent` and no render, request, and retry if the revision moved. It returns nil only when the photograph changes, cropping starts or the render failed. A resize updates `sampleRadius` and a `pendingRadius` shown by `pinsToShow` at once, then after 200 ms of quiet measures and writes one edit, "Resize Tone Pin". Read `ignored_pins` in `render()`. Clear `selectedPin` when it no longer indexes a pin.
- [ ] **Step 4: Run** the suite; expect PASS. Keep the canvas compiling by adapting call sites minimally.
- [ ] **Step 5: Commit** "Make pins belong to zones".

### Task 4: Draw pins and links in one overlay

**Files:** Create `macos/Sources/NegsApp/PinOverlay.swift`. Modify `PreviewCanvas.swift`, `CanvasGesture.swift`, `NegsApp.swift`. Test: `CanvasGestureTests.swift`.

**Interfaces:**

```swift
enum ZoneGeometry { static func tileCentreY(zone: Int, height: CGFloat) -> CGFloat } // from the top; zone 8 in the top row
final class PinOverlayView: NSView {  // hitTest returns nil; autoresizes with the scroll view
    func update(pins: [TonePinMarker], dragged: ZoneDrag?, in photo: PhotographView)
    func pin(at point: CGPoint) -> Int?  // chip only, in overlay coordinates
}
extension CanvasScale { func unitPoint(canvasPoint: CGPoint) -> CGPoint? }  // top-left canvas points
extension PreviewCanvas { var pins; var dragged: ZoneDrag?; var onSelectPin: (Int?) -> Void; var onMovePin; var onScrollPin: (Double, Bool) -> Void; var onDeleteKey; var onEscapeKey }
```

`CanvasGesture.Start` becomes `zoomToggle | pan | selectPin(Int)`. Removes the sample circle, flags monitor, tracking area and ⌘ handling.

- [ ] **Step 1: Write failing tests:** `testAChipIs24PointsAtAnyZoomRotationAndBorder`, `testAChipIsUprightOnATurnedPhotograph`, `testAChipSitsWhereItsToneWasMeasured` at zoom 0.25 and 2, `testALinkRunsFromTheTileToThePin`, `testALinkFollowsPanAndZoom`, `testAPinOutsideTheCropIsHidden`, `testARingDoesNotTakeThePointer`, `testScrollResizesOnlyWithAPinSelected`, `testClickingEmptyPhotographDeselects`, `testADropOnTheBorderIsNil`, `testIgnoredLinksAreRed`, `testTileRowsRunFromZeroAtTheBottom`.
- [ ] **Step 2: Run** `swift test --package-path macos --scratch-path target/swift-native --filter CanvasGestureTests`; expect a compile failure.
- [ ] **Step 3: Implement** `PinOverlayView` as a dark casing layer under white, accent and red line layers, ring layers likewise, and one chip layer per pin. Redraw on `update`, on `contentView` bounds changes (posts cover pan and zoom), and on layout. Dragged pins use a stored centre until the model updates.
- [ ] **Step 4: Render** chips and links on black, mid-grey and white tones to PNG with a temporary test, look at them, and delete the test.
- [ ] **Step 5: Run** the suite; expect PASS.
- [ ] **Step 6: Commit** "Draw pins and their links in one overlay".

### Task 5: Add the panel and its toolbar button

**Files:** Create `macos/Sources/NegsApp/ZonePanel.swift`. Modify `NegsApp.swift`, `EditorWindowController.swift`, `MainMenu.swift`, `docs/native-macos-ui.md`. Test: `ZoneSystemTests.swift`, `InspectorLayoutTests.swift`.

**Interfaces:** `ZonePanel(model:)`, 56 points wide; `EditorCanvas` wraps it and `CanvasArea` in an `HStack` in a coordinate space named `"zoneCanvas"`, and subtracts 56 from x to get canvas points. Toolbar item `.zones` in the view group after the film strip button, accent when on. Menu "Show Zones"/"Hide Zones", ⌥⌘Z. "Delete Tone Pin" stays.

- [ ] **Step 1: Write failing tests:** `testTheToolbarButtonTogglesThePanel`, `testThePanelIsHiddenWhileCropping`, `testTileLabelsNameTheZoneAndWhetherItIsPlaced`, and extend the inspector layout test so placing a pin moves nothing.
- [ ] **Step 2: Run** the filter; expect a compile failure.
- [ ] **Step 3: Implement** tiles with `DragGesture(minimumDistance: 2, coordinateSpace: .named("zoneCanvas"))`. `onChanged` sets `model.zoneDrag`. `onEnded` clears it and, if `canvasScale.unitPoint` is non-nil, starts `placeZone`. Make the photograph first responder when a pin is selected so Esc and Delete work. Rewrite the zone section of the docs from the spec.
- [ ] **Step 4: Run** the full Swift, Rust and `scripts/check_native_macos.py` checks; expect PASS.
- [ ] **Step 5: Commit** "Add the zone panel".
