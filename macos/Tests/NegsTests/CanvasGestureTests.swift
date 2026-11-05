import AppKit
import IOSurface
import XCTest
@testable import NegsApp

@MainActor final class CanvasGestureTests: XCTestCase {
    private let size = CGSize(width: 400, height: 300)

    private func makeView(rotation: Int = 0, crop: CropSettings = CropSettings()) -> PhotographView {
        _ = NSApplication.shared
        let cropped = crop.size(of: size)
        let display = rotation % 2 != 0 ? CGSize(width: cropped.height, height: cropped.width) : cropped
        let view = PhotographView(frame: CGRect(origin: .zero, size: display))
        view.show(NSImage(size: size), size: size, rotation: rotation, crop: crop)
        view.layout()
        return view
    }

    private func viewPoint(_ unit: CGPoint, in view: PhotographView) -> CGPoint {
        let photo = CGPoint(x: unit.x * size.width, y: (1 - unit.y) * size.height)
        return view.layer!.convert(photo, from: view.photographLayer)
    }

    // MARK: The gesture map

    func testADoubleClickWinsOverAPin() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 2, pinUnderPointer: 3), .zoomToggle)
    }

    func testAPlainPressPans() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, pinUnderPointer: nil), .pan)
    }

    func testClickingAPinSelectsIt() {
        XCTAssertEqual(
            CanvasGesture.start(clickCount: 1, pinUnderPointer: 3),
            .selectPin(3)
        )
    }

    // MARK: The coordinate map

    func testUnitPointRoundTripsThroughCropAngleAndRotation() {
        let crop = CropSettings(rect: CGRect(x: 0.2, y: 0.2, width: 0.6, height: 0.6), angle: 7)
        for rotation in 0..<4 {
            let view = makeView(rotation: rotation, crop: crop)
            for unit in [CGPoint(x: 0.5, y: 0.5), CGPoint(x: 0.45, y: 0.55)] {
                let back = view.unitPoint(at: viewPoint(unit, in: view))
                XCTAssertNotNil(back, "rotation \(rotation), \(unit)")
                XCTAssertEqual(back?.x ?? .nan, unit.x, accuracy: 1e-6)
                XCTAssertEqual(back?.y ?? .nan, unit.y, accuracy: 1e-6)
            }
        }
    }

    func testPointOnTheBorderIsNil() {
        let view = makeView()
        let corner = CGPoint(x: view.clipLayer.bounds.maxX, y: view.clipLayer.bounds.maxY)
        XCTAssertNil(view.unitPoint(at: view.layer!.convert(corner, from: view.clipLayer)))
    }

    func testPointOutsideTheCropIsNil() {
        let crop = CropSettings(rect: CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5))
        let view = makeView(crop: crop)
        XCTAssertNil(view.unitPoint(at: viewPoint(CGPoint(x: 0.1, y: 0.1), in: view)))
        XCTAssertNotNil(view.unitPoint(at: viewPoint(CGPoint(x: 0.5, y: 0.5), in: view)))
    }
}

extension CanvasGestureTests {
    func testDoubleClickZoomsAboutTheClickedPoint() {
        _ = NSApplication.shared
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        scroll.contentView = CenteringClipView()
        scroll.documentView = NSView(frame: NSRect(x: 0, y: 0, width: 1000, height: 800))
        scroll.fitting = true
        scroll.layoutSubtreeIfNeeded()
        scroll.applyZoom()
        let point = CGPoint(x: 700, y: 200)
        let before = scroll.contentView.convert(point, from: scroll.documentView)
        scroll.toggleZoom(about: point)
        XCTAssertFalse(scroll.fitting)
        let after = scroll.contentView.convert(point, from: scroll.documentView)
        XCTAssertEqual(before.x, after.x, accuracy: 1)
        XCTAssertEqual(before.y, after.y, accuracy: 1)
        // A second double-click fits the whole photograph again.
        scroll.toggleZoom(about: point)
        XCTAssertTrue(scroll.fitting)
    }

    private func surface(
        width: Int, height: Int, _ pixel: (Int, Int) -> (r: UInt8, g: UInt8, b: UInt8)
    ) throws -> IOSurface {
        let surface = try XCTUnwrap(IOSurface(properties: [
            .width: width, .height: height, .bytesPerElement: 4,
            .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
            .pixelFormat: kCVPixelFormatType_32BGRA
        ]))
        surface.lock(options: [], seed: nil)
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        for y in 0..<height {
            for x in 0..<width {
                let (r, g, b) = pixel(x, y)
                let point = base + y * surface.bytesPerRow + x * 4
                point[0] = b; point[1] = g; point[2] = r; point[3] = 255
            }
        }
        surface.unlock(options: [], seed: nil)
        return surface
    }
    // MARK: The overlay

    private func marker(_ index: Int = 0, zone: Int = 5, at unit: CGPoint = CGPoint(x: 0.5, y: 0.5), radius: Double = 0, selected: Bool = false, ignored: Bool = false) -> TonePinMarker {
        TonePinMarker(index: index, zone: zone, source: PinSource(centre: unit, radius: radius), selected: selected, ignored: ignored)
    }

    private func makeCanvas(rotation: Int = 0, crop: CropSettings = CropSettings(), viewport: CGFloat = 600, frame: CGSize? = nil) -> (ImageScrollView, PhotographView) {
        _ = NSApplication.shared
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: viewport, height: viewport))
        scroll.allowsMagnification = true; scroll.minMagnification = 0.01; scroll.maxMagnification = 16
        scroll.hasHorizontalScroller = true; scroll.hasVerticalScroller = true
        let cropped = crop.size(of: size)
        let display = frame ?? (rotation % 2 != 0 ? CGSize(width: cropped.height, height: cropped.width) : cropped)
        let view = PhotographView(frame: CGRect(origin: .zero, size: display))
        scroll.documentView = view
        scroll.tile()
        view.show(NSImage(size: size), size: size, rotation: rotation, crop: crop)
        view.layout()
        return (scroll, view)
    }

    private func overlayPoint(_ unit: CGPoint, _ scroll: ImageScrollView, _ view: PhotographView) -> CGPoint {
        scroll.overlay.convert(viewPoint(unit, in: view), from: view)
    }

    /// Set up as the app does: the clip view, scrollers and document all arrive
    /// after the overlay, which must still end up on top and tracking the clip.
    func testTheOverlayStaysOnTopOfTheAppsScrollViewSetup() throws {
        _ = NSApplication.shared
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: 200, height: 200))
        scroll.contentView = CenteringClipView()
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true; scroll.scrollerStyle = .overlay
        scroll.allowsMagnification = true
        let view = PhotographView(frame: CGRect(origin: .zero, size: size))
        scroll.documentView = view
        scroll.tile()
        view.show(NSImage(size: size), size: size, rotation: 0)
        view.layout()
        XCTAssertTrue(scroll.subviews.last === scroll.overlay, "\(scroll.subviews)")
        XCTAssertEqual(scroll.overlay.frame, scroll.bounds)
        scroll.showPins([marker()], dragged: nil)
        let before = try XCTUnwrap(scroll.overlay.links[0]).to
        scroll.contentView.scroll(to: CGPoint(x: 40, y: 20))
        XCTAssertEqual(try XCTUnwrap(scroll.overlay.links[0]).to.x, before.x - 40, accuracy: 0.5, "It follows the app's own clip view")
    }

    func testAMarkerKeepsItsSizeAtAnyZoomRotationAndBorder() throws {
        for (rotation, frame) in [(0, nil), (1, nil), (0, CGSize(width: 800, height: 600))] as [(Int, CGSize?)] {
            let (scroll, view) = makeCanvas(rotation: rotation, frame: frame)
            scroll.showPins([marker()], dragged: nil)
            for zoom: CGFloat in [0.25, 1, 2] {
                scroll.magnification = zoom
                view.layout()
                let chip = try XCTUnwrap(scroll.overlay.chipLayer(0))
                XCTAssertEqual(chip.bounds.width, ZoneGeometry.markerDiameter + 3)
                XCTAssertEqual(chip.affineTransform(), .identity, "Upright, untouched by the photograph")
            }
        }
    }

    /// The marker is smaller than the old chip, so a small circle still shows.
    func testACircleSmallerThanTheOldChipStillShows() {
        let (scroll, _) = makeCanvas()
        scroll.showPins([marker(radius: 0.03)], dragged: nil) // 9 points at 1:1
        XCTAssertEqual(scroll.overlay.drawnCircles, 1)
        scroll.showPins([marker(radius: 0)], dragged: nil)
        XCTAssertEqual(scroll.overlay.drawnCircles, 0)
    }

    /// The numeral sits beside the marker, off the spot it measures, and can be
    /// clicked like the marker.
    func testTheLabelSitsBesideTheMarker() throws {
        let (scroll, view) = makeCanvas()
        let unit = CGPoint(x: 0.5, y: 0.5)
        scroll.showPins([marker(at: unit)], dragged: nil)
        let centre = overlayPoint(unit, scroll, view)
        let label = try XCTUnwrap(scroll.overlay.labelFrame(0))
        XCTAssertGreaterThan(label.minX, centre.x + ZoneGeometry.markerDiameter / 2)
        XCTAssertEqual(label.midY, centre.y, accuracy: 0.5)
        XCTAssertEqual(scroll.overlay.pin(at: CGPoint(x: label.midX, y: label.midY)), 0)
        XCTAssertEqual(scroll.overlay.pin(at: centre), 0)
    }

    /// Near the canvas's right edge the label moves to the marker's left.
    func testTheLabelStaysOnTheCanvas() throws {
        let (scroll, view) = makeCanvas(viewport: 410)
        let unit = CGPoint(x: 0.99, y: 0.5)
        scroll.showPins([marker(at: unit)], dragged: nil)
        let centre = overlayPoint(unit, scroll, view)
        XCTAssertLessThan(centre.x, scroll.overlay.bounds.width)
        let label = try XCTUnwrap(scroll.overlay.labelFrame(0))
        XCTAssertLessThan(label.maxX, centre.x)
    }

    func testAChipSitsWhereItsToneWasMeasured() throws {
        for rotation in [0, 1] {
            let (scroll, view) = makeCanvas(rotation: rotation)
            for zoom: CGFloat in [0.5, 2] {
                scroll.magnification = zoom
                let unit = CGPoint(x: 0.3, y: 0.7)
                scroll.showPins([marker(at: unit)], dragged: nil)
                let chip = try XCTUnwrap(scroll.overlay.chipLayer(0))
                let expected = view.convert(view.layer!.convert(CGPoint(x: unit.x * size.width, y: (1 - unit.y) * size.height), from: view.photographLayer), to: scroll.overlay)
                XCTAssertEqual(chip.position.x, expected.x, accuracy: 0.5, "rotation \(rotation) zoom \(zoom)")
                XCTAssertEqual(chip.position.y, expected.y, accuracy: 0.5, "rotation \(rotation) zoom \(zoom)")
            }
        }
    }

    /// Zone 0's tile is the bottom row and 10's the top; a link starts at its
    /// tile, on the canvas's left edge.
    func testALinkRunsFromTheTileToThePin() throws {
        XCTAssertEqual(ZoneGeometry.tileCentreY(zone: 10, height: 1100), 50)
        XCTAssertEqual(ZoneGeometry.tileCentreY(zone: 0, height: 1100), 1050)
        let (scroll, view) = makeCanvas()
        let unit = CGPoint(x: 0.5, y: 0.5)
        scroll.showPins([marker(zone: 10, at: unit)], dragged: nil)
        let link = try XCTUnwrap(scroll.overlay.links[0])
        XCTAssertEqual(link.from.x, 0)
        XCTAssertEqual(link.from.y, scroll.overlay.bounds.height - 600.0 / 22, accuracy: 0.01, "Layers are y-up, so the top row is high")
        XCTAssertEqual(link.to.x, overlayPoint(unit, scroll, view).x, accuracy: 0.5)
    }

    func testALinkFollowsPanAndZoom() throws {
        let (scroll, view) = makeCanvas(viewport: 200)
        let unit = CGPoint(x: 0.5, y: 0.5)
        scroll.showPins([marker(at: unit)], dragged: nil)
        let start = try XCTUnwrap(scroll.overlay.links[0]).to
        scroll.contentView.scroll(to: CGPoint(x: 80, y: 30))
        let panned = try XCTUnwrap(scroll.overlay.links[0]).to
        XCTAssertEqual(panned.x, start.x - 80, accuracy: 0.5)
        XCTAssertEqual(panned.y, start.y - 30, accuracy: 0.5)
        scroll.magnification = 2
        XCTAssertEqual(try XCTUnwrap(scroll.overlay.links[0]).to.x, overlayPoint(unit, scroll, view).x, accuracy: 0.5)
    }

    func testAPinOutsideTheCropIsHidden() {
        let crop = CropSettings(rect: CGRect(x: 0, y: 0, width: 0.4, height: 1))
        let (scroll, _) = makeCanvas(crop: crop)
        scroll.showPins([marker(at: CGPoint(x: 0.8, y: 0.5))], dragged: nil)
        XCTAssertNil(scroll.overlay.chipLayer(0))
        XCTAssertNil(scroll.overlay.links[0])
        XCTAssertNil(scroll.overlay.pin(at: .zero))
    }

    /// Only the chip takes the pointer: a plain drag inside a large circle pans.
    func testARingDoesNotTakeThePointer() throws {
        let (scroll, view) = makeCanvas()
        let unit = CGPoint(x: 0.5, y: 0.5)
        scroll.showPins([marker(at: unit, radius: 0.4)], dragged: nil)
        let point = overlayPoint(unit, scroll, view)
        XCTAssertEqual(scroll.overlay.pin(at: point), 0)
        XCTAssertNil(scroll.overlay.pin(at: CGPoint(x: point.x, y: point.y + 30)))
    }

    func testIgnoredLinksAreRed() {
        let (scroll, _) = makeCanvas()
        scroll.showPins([marker()], dragged: nil)
        XCTAssertFalse(scroll.overlay.isShowingIgnoredLink)
        scroll.showPins([marker(ignored: true)], dragged: nil)
        XCTAssertTrue(scroll.overlay.isShowingIgnoredLink)
    }

    func testADraggedZoneFollowsThePointerFromTheTop() throws {
        let (scroll, _) = makeCanvas()
        scroll.showPins([], dragged: ZoneDrag(zone: 3, point: CGPoint(x: 100, y: 40)))
        let centre = try XCTUnwrap(scroll.overlay.dragChipCentre)
        XCTAssertEqual(centre.x, 100)
        XCTAssertEqual(centre.y, scroll.overlay.bounds.height - 40)
        scroll.showPins([], dragged: nil)
        XCTAssertNil(scroll.overlay.dragChipCentre)
    }

    // MARK: Pointer and wheel

    private func scrollEvent(_ delta: Int32) throws -> NSEvent {
        let event = try XCTUnwrap(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: delta, wheel2: 0, wheel3: 0))
        return try XCTUnwrap(NSEvent(cgEvent: event))
    }

    private func mouse(_ type: NSEvent.EventType, at point: CGPoint) throws -> NSEvent {
        try XCTUnwrap(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
    }

    /// Pushing the wheel up, away from you, grows the circle. Pulling it down
    /// shrinks it to one pixel, and it never passes half the photograph.
    func testScrollingScalesTheCircleWithinItsLimits() {
        XCTAssertGreaterThan(SampleSize.radius(from: 0.1, scrollDelta: 10, precise: true), 0.1)
        XCTAssertLessThan(SampleSize.radius(from: 0.1, scrollDelta: -10, precise: true), 0.1)
        XCTAssertGreaterThan(SampleSize.radius(from: 0.1, scrollDelta: 1, precise: false), SampleSize.radius(from: 0.1, scrollDelta: 1, precise: true))
        XCTAssertEqual(SampleSize.radius(from: 0.5, scrollDelta: 100, precise: true), 0.5)
        XCTAssertEqual(SampleSize.radius(from: SampleSize.smallest, scrollDelta: -100, precise: true), 0)
        XCTAssertEqual(SampleSize.radius(from: 0, scrollDelta: 1, precise: true), SampleSize.smallest)
    }

    /// The direction follows the hand, not the natural scrolling setting: a
    /// wheel pulled down reports a negative delta with it off, and a positive
    /// one, flagged inverted, with it on.
    func testTheWheelsDirectionIgnoresNaturalScrolling() {
        XCTAssertEqual(SampleSize.handDelta(scrollingDeltaY: -3, inverted: false), -3)
        XCTAssertEqual(SampleSize.handDelta(scrollingDeltaY: 3, inverted: true), -3)
    }

    /// With a pin selected the wheel sizes it; otherwise it pans as ever.
    func testScrollResizesOnlyWithAPinSelected() throws {
        let (scroll, _) = makeCanvas(viewport: 200)
        var steps: [Double] = []
        scroll.onScrollPin = { delta, _ in steps.append(delta) }
        scroll.showPins([marker()], dragged: nil)
        scroll.scrollWheel(with: try scrollEvent(-30))
        XCTAssertTrue(steps.isEmpty)
        scroll.showPins([marker(selected: true)], dragged: nil)
        let held = scroll.contentView.bounds.origin
        scroll.scrollWheel(with: try scrollEvent(-30))
        XCTAssertEqual(steps.count, 1)
        XCTAssertEqual(scroll.contentView.bounds.origin, held, "It doesn't pan while sizing")
    }

    /// A click inside a pin's circle selects it; the marker isn't the only target.
    func testClickingInsideACircleSelectsThePin() throws {
        let (scroll, view) = makeCanvas()
        var selections: [Int?] = []
        view.onSelectPin = { selections.append($0) }
        scroll.showPins([marker(at: CGPoint(x: 0.5, y: 0.5), radius: 0.4)], dragged: nil)
        let inside = viewPoint(CGPoint(x: 0.5, y: 0.3), in: view)
        view.mouseDown(with: try mouse(.leftMouseDown, at: inside))
        view.mouseUp(with: try mouse(.leftMouseUp, at: inside))
        XCTAssertEqual(selections, [0])
    }

    /// A drag that starts inside a circle still pans, and selects nothing.
    func testDraggingInsideACircleDoesNotSelect() throws {
        let (scroll, view) = makeCanvas()
        var selections: [Int?] = []
        view.onSelectPin = { selections.append($0) }
        scroll.showPins([marker(at: CGPoint(x: 0.5, y: 0.5), radius: 0.4)], dragged: nil)
        let inside = viewPoint(CGPoint(x: 0.5, y: 0.3), in: view)
        let away = CGPoint(x: inside.x + 20, y: inside.y)
        view.mouseDown(with: try mouse(.leftMouseDown, at: inside))
        view.mouseDragged(with: try mouse(.leftMouseDragged, at: away))
        view.mouseUp(with: try mouse(.leftMouseUp, at: away))
        XCTAssertEqual(selections, [])
    }

    /// A hand's small wobble during a click is still a click.
    func testASmallWobbleIsStillAClick() throws {
        let (scroll, view) = makeCanvas()
        var selections: [Int?] = []
        view.onSelectPin = { selections.append($0) }
        scroll.showPins([marker(at: CGPoint(x: 0.5, y: 0.5), radius: 0.4)], dragged: nil)
        let inside = viewPoint(CGPoint(x: 0.5, y: 0.3), in: view)
        let wobble = CGPoint(x: inside.x + 1, y: inside.y + 1)
        view.mouseDown(with: try mouse(.leftMouseDown, at: inside))
        view.mouseDragged(with: try mouse(.leftMouseDragged, at: wobble))
        view.mouseUp(with: try mouse(.leftMouseUp, at: wobble))
        XCTAssertEqual(selections, [0])
    }

    /// Where circles overlap, the smallest under the pointer wins, so a small
    /// circle inside a big one can still be reached.
    func testTheSmallestOverlappingCircleWins() throws {
        let (scroll, view) = makeCanvas()
        let centre = CGPoint(x: 0.5, y: 0.5)
        scroll.showPins([marker(0, at: centre, radius: 0.3), marker(1, zone: 7, at: centre, radius: 0.1)], dragged: nil)
        let point = scroll.overlay.convert(viewPoint(CGPoint(x: 0.5, y: 0.5), in: view), from: view)
        let near = CGPoint(x: point.x, y: point.y + 20)   // inside both
        let far = CGPoint(x: point.x, y: point.y + 60)    // inside the big one only
        XCTAssertEqual(scroll.overlay.pinCircle(at: near), 1)
        XCTAssertEqual(scroll.overlay.pinCircle(at: far), 0)
        XCTAssertNil(scroll.overlay.pinCircle(at: CGPoint(x: point.x, y: point.y + 120)))
    }

    func testClickingEmptyPhotographDeselects() throws {
        let (scroll, view) = makeCanvas()
        var selections: [Int?] = []
        view.onSelectPin = { selections.append($0) }
        scroll.showPins([marker(selected: true)], dragged: nil)
        let empty = viewPoint(CGPoint(x: 0.1, y: 0.1), in: view)
        view.mouseDown(with: try mouse(.leftMouseDown, at: empty))
        view.mouseUp(with: try mouse(.leftMouseUp, at: empty))
        XCTAssertEqual(selections, [nil])
    }

    /// A drop maps from the canvas's top left to the photograph; the border and
    /// the panel are not drop targets.
    func testADropOffThePhotographIsNil() throws {
        let (scroll, view) = makeCanvas()
        let scale = CanvasScale()
        scale.view = scroll
        // A canvas point is from the top left. The overlay is the one view here
        // known to be y-up, whichever way the scroll view runs.
        let inOverlay = scroll.overlay.convert(viewPoint(CGPoint(x: 0.25, y: 0.75), in: view), from: view)
        let centre = try XCTUnwrap(scale.unitPoint(canvasPoint: CGPoint(x: inOverlay.x, y: scroll.overlay.bounds.height - inOverlay.y)))
        XCTAssertEqual(centre.x, 0.25, accuracy: 0.01)
        XCTAssertEqual(centre.y, 0.75, accuracy: 0.01)
        XCTAssertNil(scale.unitPoint(canvasPoint: CGPoint(x: -10, y: scroll.overlay.bounds.height / 2)), "Over the panel")
        XCTAssertNil(scale.unitPoint(canvasPoint: CGPoint(x: inOverlay.x + 500, y: scroll.overlay.bounds.height - inOverlay.y)), "On the empty canvas beside the photograph")
    }

    /// The unit point reads the photograph as shown: near the top of a
    /// top-black gradient is a dark tone, near the bottom a light one.
    func testUnitPointIsNotMirrored() throws {
        _ = NSApplication.shared
        let (w, h) = (Int(size.width), Int(size.height))
        let surface = try XCTUnwrap(IOSurface(properties: [
            .width: w, .height: h, .bytesPerElement: 4,
            .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, w * 4),
            .pixelFormat: kCVPixelFormatType_32BGRA
        ]))
        surface.lock(options: [], seed: nil)
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        for y in 0..<h {
            let value = UInt8(y * 255 / (h - 1))
            for x in 0..<w {
                let p = base + y * surface.bytesPerRow + x * 4
                p[0] = value; p[1] = value; p[2] = value; p[3] = 255
            }
        }
        surface.unlock(options: [], seed: nil)

        let view = PhotographView(frame: CGRect(origin: .zero, size: size))
        view.show(surface, size: size, rotation: 0)
        view.layout()
        let top = try XCTUnwrap(view.unitPoint(at: viewPoint(CGPoint(x: 0.5, y: 0.1), in: view)))
        let sample = try XCTUnwrap(TonePick.circle(centre: top, radius: 0).sample(surface))
        XCTAssertLessThan(sample.lstar, 35, "the top of the file is dark: \(top)")
    }
}
