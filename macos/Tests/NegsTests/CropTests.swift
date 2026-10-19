import AppKit
import XCTest
@testable import NegsApp

@MainActor final class CropTests: XCTestCase {
    private func assertEqual(_ a: CGRect, _ b: CGRect, accuracy: CGFloat = 1e-9, file: StaticString = #filePath, line: UInt = #line) {
        for (x, y) in zip([a.minX, a.minY, a.width, a.height], [b.minX, b.minY, b.width, b.height]) {
            XCTAssertEqual(x, y, accuracy: accuracy, "\(a) ≠ \(b)", file: file, line: line)
        }
    }

    /// The same values as `pixels_round_edges_and_keep_one_pixel` in `spektrafilm-io::crop`.
    func testPixelsRoundLikeTheExporter() {
        let crop = CropSettings(rect: CGRect(x: 0.1004, y: 0, width: 0.5, height: 1))
        XCTAssertEqual(crop.pixels(in: CGSize(width: 1000, height: 800)), CGRect(x: 100, y: 0, width: 500, height: 800))
        let sliver = CropSettings(rect: CGRect(x: 0.999, y: 0, width: 0.0001, height: 1))
        XCTAssertEqual(sliver.pixels(in: CGSize(width: 10, height: 10)), CGRect(x: 9, y: 0, width: 1, height: 10))
        XCTAssertEqual(CropSettings().size(of: CGSize(width: 96, height: 64)), CGSize(width: 96, height: 64))
    }

    /// Clockwise like `ImageBuf::rotated_cw`: the top-left corner turns to the top right.
    func testRectangleTurnsWithThePhotograph() {
        let topLeft = CGRect(x: 0, y: 0, width: 0.5, height: 0.25)
        assertEqual(CropSettings.shown(topLeft, rotation: 1), CGRect(x: 0.75, y: 0, width: 0.25, height: 0.5))
        assertEqual(CropSettings.shown(topLeft, rotation: 2), CGRect(x: 0.5, y: 0.75, width: 0.5, height: 0.25))
        let any = CGRect(x: 0.1, y: 0.2, width: 0.3, height: 0.4)
        for rotation in 0..<4 {
            assertEqual(CropSettings.unrotated(CropSettings.shown(any, rotation: rotation), rotation: rotation), any)
        }
        assertEqual(CropSettings.shown(any, rotation: 4), any)
    }

    func testResizeKeepsTheRatioAndStaysInside() {
        let bounds = CGSize(width: 600, height: 400)
        let start = CGRect(x: 100, y: 100, width: 300, height: 200)
        // Pull the bottom-right corner past the photograph: 3:2, clamped to the edge.
        let corner = CropGeometry.resize(start, handle: .init(x: 1, y: 1), to: CGPoint(x: 900, y: 900), ratio: 1.5, in: bounds, minimum: 10)
        assertEqual(corner, CGRect(x: 100, y: 100, width: 450, height: 300))
        // The right edge grows the height about its centre.
        let edge = CropGeometry.resize(start, handle: .init(x: 1, y: 0), to: CGPoint(x: 460, y: 0), ratio: 1.5, in: bounds, minimum: 10)
        assertEqual(edge, CGRect(x: 100, y: 80, width: 360, height: 240))
        // Freeform moves only the dragged edges, never past the minimum.
        let free = CropGeometry.resize(start, handle: .init(x: -1, y: -1), to: CGPoint(x: 395, y: -50), ratio: nil, in: bounds, minimum: 20)
        assertEqual(free, CGRect(x: 380, y: 0, width: 20, height: 300))
        let moved = CropGeometry.move(start, by: CGSize(width: 1000, height: -1000), in: bounds)
        assertEqual(moved, CGRect(x: 300, y: 0, width: 300, height: 200))
        assertEqual(CropGeometry.fit(ratio: 1, around: CGPoint(x: 580, y: 200), in: bounds), CGRect(x: 200, y: 0, width: 400, height: 400))
    }

    func testHandlesUnderThePointer() {
        let frame = CGRect(x: 100, y: 100, width: 300, height: 200)
        XCTAssertEqual(CropGeometry.handle(at: CGPoint(x: 104, y: 96), frame: frame), .init(x: -1, y: -1))
        XCTAssertEqual(CropGeometry.handle(at: CGPoint(x: 398, y: 200), frame: frame), .init(x: 1, y: 0))
        XCTAssertEqual(CropGeometry.handle(at: CGPoint(x: 250, y: 302), frame: frame), .init(x: 0, y: 1))
        XCTAssertEqual(CropGeometry.handle(at: CGPoint(x: 250, y: 200), frame: frame), .move)
        XCTAssertNil(CropGeometry.handle(at: CGPoint(x: 20, y: 20), frame: frame))
    }

    func testSessionLocksRatiosAndTurnsThemWithThePhotograph() {
        let session = CropSession(start: EditState(), photo: CGSize(width: 600, height: 400), rotation: 0)
        XCTAssertFalse(session.modified)
        session.choose(.square)
        assertEqual(session.shown, CGRect(x: 100, y: 0, width: 400, height: 400))
        session.choose(.threeTwo)
        session.orient(portrait: true)
        assertEqual(session.shown, CGRect(x: 166.666666, y: 0, width: 266.666666, height: 400), accuracy: 1e-3)
        // A quarter turn turns the rectangle, and the ratio follows it.
        let rect = session.rect
        session.update(photo: CGSize(width: 400, height: 600), rotation: 1)
        XCTAssertEqual(session.rect, rect)
        XCTAssertFalse(session.orientationPortrait)
        XCTAssertEqual(session.ratio!, 1.5, accuracy: 1e-9)
        session.choose(.custom)
        session.setCustom(width: 4, height: 5)
        XCTAssertTrue(session.orientationPortrait)
        XCTAssertEqual(session.shown.width / session.shown.height, 0.8, accuracy: 1e-9)
        session.reset()
        XCTAssertEqual(session.settings, CropSettings(rect: CropSettings.full, aspect: .original, custom: CGSize(width: 4, height: 5)))
        XCTAssertFalse(session.modified)
    }

    func testOriginalIsTheDefaultAspect() {
        XCTAssertEqual(CropSettings().aspect, .original)
        let session = CropSession(start: EditState(), photo: CGSize(width: 400, height: 600), rotation: 0)
        XCTAssertEqual(session.ratio!, 2.0 / 3, accuracy: 1e-9, "The photograph's own shape, portrait")
        // Pulling a corner keeps the photograph's shape.
        let resized = CropGeometry.resize(session.shown, handle: .init(x: 1, y: 1), to: CGPoint(x: 300, y: 100), ratio: session.ratio, in: session.photo, minimum: 10)
        XCTAssertEqual(resized.width / resized.height, 2.0 / 3, accuracy: 1e-9)
    }

    func testStraighteningShrinksTheCropOntoTheTurnedPhotograph() {
        let photo = CGSize(width: 600, height: 400)
        let session = CropSession(start: EditState(), photo: photo, rotation: 0)
        session.beginStraightening()
        session.straighten(to: 5)
        XCTAssertEqual(session.angle, 5)
        let shown = session.shown
        XCTAssertTrue(CropGeometry.fits(shown, angle: 5, photo: photo))
        XCTAssertEqual(shown.width / shown.height, 1.5, accuracy: 1e-6, "The crop keeps its shape")
        XCTAssertEqual(shown.midX, 300, accuracy: 1e-6); XCTAssertEqual(shown.midY, 200, accuracy: 1e-6)
        XCTAssertLessThan(shown.width, 600)
        // As large as fits: one more percent wouldn't.
        let larger = shown.insetBy(dx: -shown.width * 0.005, dy: -shown.height * 0.005)
        XCTAssertFalse(CropGeometry.fits(larger, angle: 5, photo: photo))
        // Back to zero in the same drag restores the whole photograph.
        session.straighten(to: 0)
        assertEqual(session.shown, CGRect(origin: .zero, size: photo), accuracy: 1e-6)
        session.straighten(to: -3.04)
        session.endStraightening()
        XCTAssertEqual(session.angle, -3)
        XCTAssertEqual(session.settings.angle, -3)
        XCTAssertTrue(session.settings.cropped)
        XCTAssertTrue(session.modified)
        session.straighten(to: 99)
        XCTAssertEqual(session.angle, 45)
    }

    func testConstrainStopsAtTheTurnedEdge() {
        let photo = CGSize(width: 600, height: 400)
        let start = CropGeometry.inscribe(CGRect(x: 250, y: 150, width: 100, height: 100), angle: 10, photo: photo)
        XCTAssertEqual(start, CGRect(x: 250, y: 150, width: 100, height: 100), "A crop that fits keeps its size")
        let far = start.offsetBy(dx: 1000, dy: 0)
        let stopped = CropGeometry.constrain(from: start, to: far, angle: 10, photo: photo)
        XCTAssertTrue(CropGeometry.fits(stopped, angle: 10, photo: photo))
        XCTAssertGreaterThan(stopped.minX, start.minX)
        XCTAssertFalse(CropGeometry.fits(stopped.offsetBy(dx: 1, dy: 0), angle: 10, photo: photo))
        // The turned corners poke out of the unturned frame.
        let bounds = CropGeometry.bounds(of: photo, angle: 10)
        XCTAssertLessThan(bounds.minY, 0); XCTAssertGreaterThan(bounds.maxX, 600)
    }

    /// The canvas turns the photograph clockwise inside the crop, and a
    /// straightened crop's size rounds its sides like the exporter.
    func testCanvasStraightensInsideTheCrop() throws {
        let crop = CropSettings(rect: CGRect(x: 0.2, y: 0.1, width: 0.5, height: 0.25), angle: -3)
        XCTAssertEqual(crop.size(of: CGSize(width: 1000, height: 800)), CGSize(width: 500, height: 200))
        let context = try XCTUnwrap(CGContext(data: nil, width: 4, height: 4, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        let view = PhotographView(frame: .zero)
        let straightened = CropSettings(rect: CropGeometry.inscribe(CGRect(x: 0, y: 0, width: 1, height: 1), angle: 10, photo: CGSize(width: 1, height: 1)), angle: 10)
        view.show(image, size: CGSize(width: 400, height: 400), rotation: 0, crop: straightened)
        view.setFrameSize(view.displaySize)
        view.layoutSubtreeIfNeeded()
        let photo = try XCTUnwrap(view.layer?.sublayers?.flatMap { $0.sublayers ?? [] }.first { $0.contents as AnyObject === image })
        XCTAssertTrue(try XCTUnwrap(photo.superlayer).masksToBounds)
        // The photograph's top-left corner turns 10° clockwise about its centre:
        // in y-up layer coordinates, its direction from the centre turns from 135° to 125°.
        let corner = photo.convert(CGPoint(x: 0, y: 400), to: photo.superlayer)
        let center = photo.convert(CGPoint(x: 200, y: 200), to: photo.superlayer)
        XCTAssertEqual(atan2(corner.y - center.y, corner.x - center.x) * 180 / .pi, 135 - 10, accuracy: 1e-6)
    }

    func testCropEditsDoNotChangeTheRenderedState() {
        var state = EditState()
        let rendered = state.rendered
        state.crop = CropSettings(rect: CGRect(x: 0, y: 0, width: 0.5, height: 0.5))
        XCTAssertEqual(state.rendered, rendered)
    }

    /// The canvas shows only the crop's pixels: layer contents are y-up, so
    /// the top-left quarter must not come out as the bottom-left one.
    func testCanvasShowsTheCroppedPart() throws {
        // Top left red, top right green, bottom left blue, bottom right white.
        let pixels: [UInt8] = [255, 0, 0, 255, 0, 255, 0, 255,
                               0, 0, 255, 255, 255, 255, 255, 255]
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        let image = try XCTUnwrap(CGImage(width: 2, height: 2, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 8, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        for (crop, expected) in [(CGRect(x: 0, y: 0, width: 0.5, height: 0.5), [255, 0, 0]), (CGRect(x: 0, y: 0.5, width: 0.5, height: 0.5), [0, 0, 255]),
                                 (CGRect(x: 0.5, y: 0, width: 0.5, height: 0.5), [0, 255, 0])] {
            let view = PhotographView(frame: .zero)
            view.show(image, size: CGSize(width: 2, height: 2), rotation: 0, crop: CropSettings(rect: crop))
            XCTAssertEqual(view.displaySize, CGSize(width: 1, height: 1))
            view.setFrameSize(NSSize(width: 1, height: 1))
            view.layoutSubtreeIfNeeded()
            let context = try XCTUnwrap(CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
            view.layer!.render(in: context)
            let rendered = context.data!.assumingMemoryBound(to: UInt8.self)
            XCTAssertEqual((0..<3).map { Int(rendered[$0]) }, expected, "\(crop)")
        }
    }

    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    func testDoneIsOneUndoStepAndCancelRestoresTheRotation() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        XCTAssertTrue(model.ready, model.error ?? "Engine did not start")
        let fixture = try makeFixture()
        defer { try? FileManager.default.removeItem(at: fixture) }
        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo

        model.beginCrop()
        let session = try XCTUnwrap(model.cropSession)
        XCTAssertFalse(model.canSave, "Saving waits until the crop is done")
        session.choose(.square)
        // Turning while cropping turns the square with the photograph.
        model.rotate()
        XCTAssertEqual(model.imageRotation, 1, "Turning doesn't wait for a render")
        XCTAssertTrue(model.previewCurrent)
        undo.beginUndoGrouping(); model.endCrop(apply: true); undo.endUndoGrouping()
        XCTAssertNil(model.cropSession)
        XCTAssertEqual(model.photoSize, CGSize(width: 64, height: 64))
        XCTAssertEqual(model.state.crop.aspect, .square)
        XCTAssertEqual(model.value("io.rotate_cw"), .number(1))
        undo.undo()
        XCTAssertFalse(model.state.crop.cropped, "One undo removes the crop and the turn")
        XCTAssertEqual(model.value("io.rotate_cw"), .number(0))
        undo.redo()
        XCTAssertEqual(model.photoSize, CGSize(width: 64, height: 64))
        XCTAssertEqual(model.value("io.rotate_cw"), .number(1))

        try await waitUntil { model.previewCurrent }
        model.beginCrop()
        model.cropSession?.reset()
        model.rotate()
        model.endCrop(apply: false)
        XCTAssertEqual(model.value("io.rotate_cw"), .number(1), "Cancel restores the rotation")
        XCTAssertEqual(model.state.crop.aspect, .square, "Cancel keeps the previous crop")
        XCTAssertEqual(model.photoSize, CGSize(width: 64, height: 64))
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(20)
        while !predicate(), Date() < deadline { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertTrue(predicate(), "Timed out waiting for image engine")
    }

    private func makeFixture() throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        memset(bitmap.bitmapData!, 120, 96 * 64 * 3)
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-crop-\(UUID().uuidString).png")
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }
}
