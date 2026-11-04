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
        for modifiers: NSEvent.ModifierFlags in [[], [.shift], [.control], [.command]] {
            XCTAssertEqual(
                CanvasGesture.start(clickCount: 2, modifiers: modifiers, pinUnderPointer: nil),
                .zoomToggle, "\(modifiers)"
            )
        }
    }

    func testCommandPins() {
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .command, pinUnderPointer: nil), .pin)
        // ⌘ over an existing pin still samples a new one.
        XCTAssertEqual(CanvasGesture.start(clickCount: 1, modifiers: .command, pinUnderPointer: 2), .pin)
    }

    func testAPlainDragPans() {
        for modifiers: NSEvent.ModifierFlags in [[], [.shift], [.control], [.control, .shift]] {
            XCTAssertEqual(
                CanvasGesture.start(clickCount: 1, modifiers: modifiers, pinUnderPointer: nil),
                .pan, "\(modifiers)"
            )
        }
    }

    func testClickingAPinSelectsIt() {
        XCTAssertEqual(
            CanvasGesture.start(clickCount: 1, modifiers: [], pinUnderPointer: 3),
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
    /// A pin's chip sits exactly where its tone was measured.
    func testAPinsChipSitsWhereItsToneWasMeasured() throws {
        let view = makeView()
        for unit in [CGPoint(x: 0.3, y: 0.3), CGPoint(x: 0.5, y: 0.5), CGPoint(x: 0.8, y: 0.7)] {
            view.setPins([
                TonePinMarker(
                    index: 0, number: 1, lstar: 50,
                    source: PinSource(centre: unit, radius: 0.1), selected: false
                )
            ])
            let expected = viewPoint(unit, in: view)
            let actual = try XCTUnwrap(view.chipCentres.first)
            XCTAssertEqual(actual.x, expected.x, accuracy: 0.5, "x at \(unit)")
            XCTAssertEqual(actual.y, expected.y, accuracy: 0.5, "y at \(unit)")
        }
    }
    /// A chip is the same size on screen at any zoom, rotation or border.
    func testAChipKeepsItsScreenSize() throws {
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: 400, height: 400))
        scroll.allowsMagnification = true; scroll.minMagnification = 0.01; scroll.maxMagnification = 16
        let view = PhotographView()
        scroll.documentView = view
        let marker = TonePinMarker(index: 0, number: 1, lstar: 50, source: PinSource(centre: CGPoint(x: 0.5, y: 0.5), radius: 0), selected: false)
        for (rotation, border) in [(0, false), (1, false), (0, true)] as [(Int, Bool)] {
            let frame = rotation == 1 ? CGSize(width: 200, height: 800) : CGSize(width: 800, height: 200)
            view.show(NSObject(), size: CGSize(width: 800, height: 200), rotation: rotation)
            view.setFrameSize(border ? CGSize(width: frame.width * 2, height: frame.height * 2) : frame)
            view.layoutSubtreeIfNeeded(); view.layout()
            view.setPins([marker])
            for zoom: CGFloat in [0.25, 1, 2] {
                scroll.magnification = zoom
                let chip = try XCTUnwrap(view.chipLayers.first)
                XCTAssertEqual(chip.bounds.width * zoom, 18, accuracy: 0.01, "rotation \(rotation) border \(border) zoom \(zoom)")
            }
        }
    }

    /// Its number reads upright on a turned photograph.
    func testAChipStaysUprightOnATurnedPhotograph() throws {
        let view = makeView()
        view.show(NSObject(), size: CGSize(width: 800, height: 200), rotation: 1)
        view.setFrameSize(CGSize(width: 200, height: 800)); view.layout()
        view.setPins([TonePinMarker(index: 0, number: 1, lstar: 50, source: PinSource(centre: CGPoint(x: 0.5, y: 0.5), radius: 0), selected: false)])
        let chip = try XCTUnwrap(view.chipLayers.first)
        let total = chip.affineTransform().concatenating(view.clipLayer.affineTransform())
        XCTAssertEqual(total.b, 0, accuracy: 1e-6)
        XCTAssertEqual(total.a, 1, accuracy: 1e-6)
    }

    /// Only the chip takes the pointer: a plain drag inside a large circle
    /// still pans.
    func testARingDoesNotTakeThePointer() throws {
        let view = makeView()
        let centre = CGPoint(x: 0.5, y: 0.5)
        view.setPins([TonePinMarker(index: 0, number: 1, lstar: 50, source: PinSource(centre: centre, radius: 0.4), selected: false)])
        let point = viewPoint(centre, in: view)
        XCTAssertEqual(view.pinIndex(at: point), 0)
        XCTAssertNil(view.pinIndex(at: CGPoint(x: point.x + 30, y: point.y)))
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
