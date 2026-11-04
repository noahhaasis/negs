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

    func testDoubleClickWinsOverEveryModifier() {
        for modifiers: NSEvent.ModifierFlags in [[], [.shift], [.control], [.control, .option], [.command]] {
            for inside in [true, false] {
                XCTAssertEqual(
                    CanvasGesture.start(clickCount: 2, modifiers: modifiers, insideSelection: inside),
                    .zoomToggle, "\(modifiers) inside \(inside)"
                )
            }
        }
    }

    func testShiftPans() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .shift, insideSelection: false), .pan)
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .shift, insideSelection: true), .pan)
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: [.shift, .control], insideSelection: true), .pan)
    }

    func testPlainDownStartsASelection() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: [], insideSelection: false), .select)
    }

    func testPlainDownInsideTheSelectionMovesIt() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: [], insideSelection: true), .move)
    }

    func testControlPicksSmallAndControlOptionPicksLarge() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .control, insideSelection: false), .pickSmall)
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: [.control, .option], insideSelection: false), .pickLarge)
    }

    func testCommandAloneIsAPlainSelection() {
        // ⌘ white-balance picks are out of scope, so ⌘ alone selects.
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .command, insideSelection: false), .select)
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .command, insideSelection: true), .move)
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
    func testSelectionIsAnchoredAtMouseDownInBothDirections() throws {
        let view = makeView()
        let (a, b) = (CGPoint(x: 50, y: 50), CGPoint(x: 200, y: 150))
        let forward = try XCTUnwrap(view.selection(from: a, to: b))
        let backward = try XCTUnwrap(view.selection(from: b, to: a))
        XCTAssertEqual(forward.rect, backward.rect)
        XCTAssertEqual(forward.rect.minX, 0.125, accuracy: 1e-9)
        XCTAssertEqual(forward.rect.minY, 0.5, accuracy: 1e-9)
        XCTAssertEqual(forward.rect.width, 0.375, accuracy: 1e-9)
        XCTAssertEqual(forward.rect.height, 1.0 / 3, accuracy: 1e-9)
    }

    func testSelectionClampsToTheCrop() throws {
        let view = makeView()
        let selection = try XCTUnwrap(view.selection(from: CGPoint(x: -100, y: -80), to: CGPoint(x: 500, y: 400)))
        XCTAssertEqual(selection.rect.minX, 0, accuracy: 1e-9)
        XCTAssertEqual(selection.rect.minY, 0, accuracy: 1e-9)
        XCTAssertEqual(selection.rect.width, 1, accuracy: 1e-9)
        XCTAssertEqual(selection.rect.height, 1, accuracy: 1e-9)
    }

    func testAClickWithoutADragClearsTheSelection() {
        let view = makeView()
        XCTAssertNil(view.selection(from: CGPoint(x: 100, y: 100), to: CGPoint(x: 100, y: 100)))
        XCTAssertNil(view.selection(from: CGPoint(x: 100, y: 100), to: CGPoint(x: 100.2, y: 100.2)))
    }

    func testMovingKeepsTheSelectionsSize() {
        let view = makeView()
        let original = CropSettings(rect: CGRect(x: 0.2, y: 0.2, width: 0.3, height: 0.3))
        let moved = view.moving(original, by: CGPoint(x: 40, y: 30))
        XCTAssertEqual(moved.rect.width, original.rect.width, accuracy: 1e-9)
        XCTAssertEqual(moved.rect.height, original.rect.height, accuracy: 1e-9)
        XCTAssertEqual(moved.rect.minX, 0.3, accuracy: 1e-9)
        // y is top-down, so moving up by 30 px of 300 takes 0.1 off it.
        XCTAssertEqual(moved.rect.minY, 0.1, accuracy: 1e-9)
        // The edge stops at the crop rather than leaving it.
        let clamped = view.moving(original, by: CGPoint(x: 10_000, y: -10_000))
        XCTAssertEqual(clamped.rect.maxX, 1, accuracy: 1e-9)
        XCTAssertEqual(clamped.rect.maxY, 1, accuracy: 1e-9)
        XCTAssertEqual(clamped.rect.width, original.rect.width, accuracy: 1e-9)
    }
}

extension CanvasGestureTests {
    func testSelectionOnARotatedPhotoCoversWhatWasDragged() throws {
        // Four quadrants in the photograph: red, green / blue, white.
        let image = try surface(width: 40, height: 40) { x, y in
            if y < 20 { return x < 20 ? (255, 0, 0) : (0, 255, 0) }
            return x < 20 ? (0, 0, 255) : (255, 255, 255)
        }
        // The shown top-left quarter, in the photograph's own coordinates.
        let shown = CGRect(x: 0, y: 0, width: 0.5, height: 0.5)
        for rotation in 0..<4 {
            let view = makeView(rotation: rotation)
            // y is up in the view, so the shown top-left starts at half height.
            let display = view.bounds.size
            let selection = try XCTUnwrap(
                view.selection(
                    from: CGPoint(x: 0, y: display.height / 2),
                    to: CGPoint(x: display.width / 2, y: display.height)
                )
            )
            let expected = CropSettings.unrotated(shown, rotation: rotation)
            XCTAssertEqual(selection.rect.minX, expected.minX, accuracy: 1e-9, "rotation \(rotation)")
            XCTAssertEqual(selection.rect.minY, expected.minY, accuracy: 1e-9, "rotation \(rotation)")
            XCTAssertEqual(selection.rect.width, expected.width, accuracy: 1e-9, "rotation \(rotation)")
            XCTAssertEqual(selection.rect.height, expected.height, accuracy: 1e-9, "rotation \(rotation)")
            let sample = try XCTUnwrap(TonePick.region(selection).sample(image, output: .identity))
            // Red, blue, white, green as the photograph turns clockwise.
            switch rotation {
            case 0: XCTAssertEqual(sample.red, 255, accuracy: 1)
            case 1: XCTAssertEqual(sample.blue, 255, accuracy: 1)
            case 2: XCTAssertEqual(sample.red, 255, accuracy: 1)
            default: XCTAssertEqual(sample.green, 255, accuracy: 1)
            }
        }
    }

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
}
