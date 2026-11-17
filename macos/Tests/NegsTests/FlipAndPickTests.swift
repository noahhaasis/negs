import AppKit
import XCTest
@testable import NegsApp

/// Flip (`io.flip`) and the negative's picks (ticket 33): a click on the
/// canvas lands on the photograph's own pixel, the frame the helper's
/// `negative_base` and `negative_neutral` read, whatever the crop,
/// straightening, flip and turns.
@MainActor final class FlipAndPickTests: XCTestCase {
    private func assertEqual(_ a: CGPoint, _ b: CGPoint, accuracy: CGFloat = 1e-6, _ message: String = "",
                             file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(a.x, b.x, accuracy: accuracy, "\(a) ≠ \(b) \(message)", file: file, line: line)
        XCTAssertEqual(a.y, b.y, accuracy: accuracy, "\(a) ≠ \(b) \(message)", file: file, line: line)
    }

    private static let orientations = (0..<4).flatMap { turns in [false, true].map { Orientation(turns: turns, flip: $0) } }

    // MARK: Orientation

    /// Like `ImageBuf::flipped_rotated_cw`: mirrored left to right, then turned clockwise.
    func testOrientationMirrorsThenTurns() {
        let topLeft = CGPoint(x: 0.1, y: 0.2)
        assertEqual(Orientation(flip: true).shown(topLeft), CGPoint(x: 0.9, y: 0.2))
        assertEqual(Orientation(turns: 1).shown(topLeft), CGPoint(x: 0.8, y: 0.1))
        // Mirrored to the top right, then turned to the bottom right.
        assertEqual(Orientation(turns: 1, flip: true).shown(topLeft), CGPoint(x: 0.8, y: 0.9))
        // A flip and a half turn mirror top to bottom.
        assertEqual(Orientation(turns: 2, flip: true).shown(topLeft), CGPoint(x: 0.1, y: 0.8))
        for orientation in Self.orientations {
            assertEqual(orientation.unshown(orientation.shown(topLeft)), topLeft, "\(orientation)")
        }
        XCTAssertEqual(Orientation(turns: -1), Orientation(turns: 3))
    }

    /// Flip mirrors the photograph left to right as shown, however it's turned.
    func testFlipMirrorsAsShown() {
        let point = CGPoint(x: 0.1, y: 0.3)
        for orientation in Self.orientations {
            let before = orientation.shown(point), after = orientation.flippedAsShown.shown(point)
            assertEqual(after, CGPoint(x: 1 - before.x, y: before.y), "\(orientation)")
            XCTAssertEqual(orientation.flippedAsShown.flippedAsShown, orientation)
        }
    }

    /// The rebate finder's edge is the photograph's own; the sentence names it as shown.
    func testBaseEdgeIsNamedAsShown() {
        XCTAssertEqual(Orientation().shownEdge("left"), "left")
        XCTAssertEqual(Orientation(flip: true).shownEdge("left"), "right")
        XCTAssertEqual(Orientation(turns: 1).shownEdge("left"), "top")
        XCTAssertEqual(Orientation(turns: 1, flip: true).shownEdge("left"), "bottom")
        XCTAssertEqual(Orientation(turns: 3).shownEdge("top"), "left")
        let status = NegativeScan.BaseStatus(found: true, picked: false, edge: "left")
        XCTAssertEqual(status.sentence(as: Orientation(flip: true)), "Film base found along the right edge.")
    }

    /// The crop rectangle stays in the unflipped photograph's frame and turns with it.
    func testCropRectangleFlipsWithThePhotograph() {
        let rect = CGRect(x: 0.1, y: 0.2, width: 0.3, height: 0.4)
        let shown = CropSettings.shown(rect, rotation: 0, flip: true)
        XCTAssertEqual(shown.minX, 0.6, accuracy: 1e-9); XCTAssertEqual(shown.minY, 0.2, accuracy: 1e-9)
        for orientation in Self.orientations {
            let r = CropSettings.shown(rect, rotation: orientation.turns, flip: orientation.flip)
            // Its corners are where the orientation shows the rectangle's corners.
            let a = orientation.shown(CGPoint(x: rect.minX, y: rect.minY)), b = orientation.shown(CGPoint(x: rect.maxX, y: rect.maxY))
            XCTAssertEqual(r.minX, min(a.x, b.x), accuracy: 1e-9); XCTAssertEqual(r.maxY, max(a.y, b.y), accuracy: 1e-9)
            let back = CropSettings.unrotated(r, rotation: orientation.turns, flip: orientation.flip)
            XCTAssertEqual(back.minX, rect.minX, accuracy: 1e-9); XCTAssertEqual(back.minY, rect.minY, accuracy: 1e-9)
            XCTAssertEqual(back.width, rect.width, accuracy: 1e-9); XCTAssertEqual(back.height, rect.height, accuracy: 1e-9)
        }
    }

    /// While cropping, the dial shows the straightening as seen: a flipped
    /// photograph's is the other way round, and Done stores it unflipped.
    func testCropSessionMirrorsTheAngle() {
        var start = EditState()
        start.crop = CropSettings(rect: CGRect(x: 0.1, y: 0.1, width: 0.5, height: 0.5), angle: 4)
        let session = CropSession(start: start, photo: CGSize(width: 600, height: 400), rotation: 0, flip: true)
        XCTAssertEqual(session.angle, -4)
        XCTAssertEqual(session.settings.angle, 4)
        // The rectangle as shown is mirrored; dragging it moves the stored one the other way.
        XCTAssertEqual(session.shown.minX, 0.4 * 600, accuracy: 1e-9)
        session.shown = session.shown.offsetBy(dx: 60, dy: 0)
        XCTAssertEqual(session.rect.minX, 0.0, accuracy: 1e-9)
        // Flipping back shows the stored angle again.
        session.update(photo: CGSize(width: 600, height: 400), rotation: 0, flip: false)
        XCTAssertEqual(session.angle, 4)
        XCTAssertEqual(session.settings.angle, 4)
    }

    /// Flip is shown, saved and copied with the photograph, but never renders again.
    func testFlipIsGeometryNotARender() throws {
        var state = EditState()
        let rendered = state.rendered
        state.params["io.flip"] = .bool(true)
        XCTAssertEqual(state.rendered, rendered)
        XCTAssertNoThrow(try state.validate())
        state.params["io.flip"] = .number(1)
        XCTAssertThrowsError(try state.validate())
        XCTAssertEqual(EditGroup.of(section: "io", key: "flip"), .cropAndRotation)
    }

    // MARK: The canvas

    /// Top left red, top right green, bottom left blue, bottom right white.
    private func quadrants() throws -> CGImage {
        let pixels: [UInt8] = [255, 0, 0, 255, 0, 255, 0, 255,
                               0, 0, 255, 255, 255, 255, 255, 255]
        let provider = try XCTUnwrap(CGDataProvider(data: Data(pixels) as CFData))
        return try XCTUnwrap(CGImage(width: 2, height: 2, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 8, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                     bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider, decode: nil,
                                     shouldInterpolate: false, intent: .defaultIntent))
    }

    /// The canvas mirrors the photograph before it turns it, as Save does.
    func testCanvasShowsTheFlip() throws {
        let image = try quadrants()
        let red = [255, 0, 0], green = [0, 255, 0], blue = [0, 0, 255], white = [255, 255, 255]
        // The colour in the shown photograph's top-left corner.
        for (turns, expected) in [(0, green), (1, white), (2, blue), (3, red)] {
            let view = PhotographView(frame: .zero)
            view.show(image, size: CGSize(width: 2, height: 2), rotation: turns, flip: true)
            view.setFrameSize(NSSize(width: 2, height: 2))
            view.layoutSubtreeIfNeeded()
            let context = try XCTUnwrap(CGContext(data: nil, width: 2, height: 2, bitsPerComponent: 8, bytesPerRow: 8, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
            view.layer!.render(in: context)
            // The bitmap's first row is the top of the y-up layer.
            let rendered = context.data!.assumingMemoryBound(to: UInt8.self)
            XCTAssertEqual((0..<3).map { Int(rendered[$0]) }, expected, "turns \(turns)")
        }
    }

    /// Where the photograph's point `p` (fractions of the whole) is shown on
    /// a canvas the size of the print, in its y-up points: straightened about
    /// the centre and cut as `spektrafilm-io::crop` does, then flipped and
    /// turned as `ImageBuf::flipped_rotated_cw`. Worked out independently of the layers.
    private func shownPoint(_ p: CGPoint, size: CGSize, crop: CropSettings, orientation: Orientation) -> CGPoint {
        let centre = CGPoint(x: size.width / 2, y: size.height / 2)
        let turned = CropGeometry.turn(CGPoint(x: p.x * size.width, y: p.y * size.height), by: crop.angle, about: centre)
        let cut = crop.pixels(in: size)
        let fraction = CGPoint(x: (turned.x - cut.minX) / cut.width, y: (turned.y - cut.minY) / cut.height)
        let shown = orientation.shown(fraction)
        let display = orientation.turns % 2 == 0 ? cut.size : CGSize(width: cut.height, height: cut.width)
        return CGPoint(x: shown.x * display.width, y: (1 - shown.y) * display.height)
    }

    /// A click maps back through every turn, flip, crop and straightening.
    func testPicksLandOnThePhotographsOwnPixel() throws {
        let image = try quadrants(), size = CGSize(width: 600, height: 400)
        let crops = [CropSettings(), CropSettings(rect: CGRect(x: 0.3, y: 0.2, width: 0.5, height: 0.6)),
                     CropSettings(rect: CropGeometry.inscribe(CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5), angle: 7, photo: CGSize(width: 1, height: 1)), angle: 7)]
        // Points well inside every crop.
        let points = [CGPoint(x: 0.45, y: 0.4), CGPoint(x: 0.55, y: 0.6), CGPoint(x: 0.6, y: 0.45)]
        for crop in crops {
            for orientation in Self.orientations {
                let view = PhotographView(frame: .zero)
                view.show(image, size: size, rotation: orientation.turns, flip: orientation.flip, crop: crop)
                view.setFrameSize(view.displaySize)
                view.layoutSubtreeIfNeeded()
                for p in points {
                    let where_ = shownPoint(p, size: size, crop: crop, orientation: orientation)
                    let picked = try XCTUnwrap(view.photographFraction(where_), "\(crop) \(orientation)")
                    assertEqual(picked, p, accuracy: 1e-6, "\(crop) \(orientation)")
                }
            }
        }
    }

    /// Plain cases, by hand: the corner a click lands in.
    func testPicksByHand() throws {
        let image = try quadrants(), size = CGSize(width: 300, height: 200)
        func pick(_ orientation: Orientation, at shown: CGPoint) throws -> CGPoint {
            let view = PhotographView(frame: .zero)
            view.show(image, size: size, rotation: orientation.turns, flip: orientation.flip)
            view.setFrameSize(view.displaySize)
            view.layoutSubtreeIfNeeded()
            // `shown` is from the top left; the view is y-up.
            return try XCTUnwrap(view.photographFraction(CGPoint(x: shown.x * view.displaySize.width, y: (1 - shown.y) * view.displaySize.height)))
        }
        let nearTopLeft = CGPoint(x: 0.1, y: 0.1)
        // Flipped: the top left shown is the photograph's top right.
        assertEqual(try pick(Orientation(flip: true), at: nearTopLeft), CGPoint(x: 0.9, y: 0.1))
        // A quarter turn clockwise: the top left shown is the photograph's bottom left.
        assertEqual(try pick(Orientation(turns: 1), at: nearTopLeft), CGPoint(x: 0.1, y: 0.9))
        // Half a turn: its bottom right.
        assertEqual(try pick(Orientation(turns: 2), at: nearTopLeft), CGPoint(x: 0.9, y: 0.9))
        // Three quarters: its top right.
        assertEqual(try pick(Orientation(turns: 3), at: nearTopLeft), CGPoint(x: 0.9, y: 0.1))
        // Flipped and a quarter turn: its bottom right.
        assertEqual(try pick(Orientation(turns: 1, flip: true), at: nearTopLeft), CGPoint(x: 0.9, y: 0.9))
    }

    /// Holding T turns the print half round; the click still lands where it's shown.
    func testPicksFollowTheUpsideDownPrint() throws {
        let image = try quadrants(), size = CGSize(width: 300, height: 200)
        let view = PhotographView(frame: .zero)
        view.show(image, size: size, rotation: 1, flip: true)
        view.setFrameSize(view.displaySize)
        view.layoutSubtreeIfNeeded()
        let upright = try XCTUnwrap(view.photographFraction(CGPoint(x: 30, y: 250)))
        view.turned = true
        let turned = try XCTUnwrap(view.photographFraction(CGPoint(x: view.displaySize.width - 30, y: view.displaySize.height - 250)))
        assertEqual(turned, upright)
    }

    // MARK: A real negative

    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func waitUntil(_ seconds: TimeInterval = 60, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !predicate(), Date() < deadline { try await Task.sleep(nanoseconds: 20_000_000) }
        XCTAssertTrue(predicate(), "Timed out")
    }

    /// The film base picked from the rebate of a flipped, turned negative
    /// matches the one found there: the click goes through the canvas's own
    /// mapping into the helper. `NEGS_TEST_NEGATIVE`: a scan with clear film
    /// along its left edge, such as Dslr0001.NEF (not in the repository).
    func testRebatePickOnAFlippedTurnedNegative() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_NEGATIVE"] else { throw XCTSkip("Set NEGS_TEST_NEGATIVE") }
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        model.open(URL(fileURLWithPath: path))
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        model.chooseNegative()
        try await waitUntil { model.negativeBase != nil && model.negativeWorking == nil }
        let found = try XCTUnwrap(model.negativeBase)
        XCTAssertEqual(found.edge, "left")
        let base = { (key: String) in model.value("film_render.base.\(key)").double }
        let foundBase = ["cyan", "magenta", "yellow"].map(base)

        // Turned a quarter, then flipped as shown.
        model.turn(clockwise: true)
        model.flip()
        model.endCrop(apply: true)
        XCTAssertEqual(Orientation(turns: model.imageRotation, flip: model.imageFlip), Orientation(turns: 3, flip: true))
        XCTAssertEqual(model.negativeBase?.sentence(as: Orientation(turns: 3, flip: true)), "Film base found along the top edge.")

        // Picking the base shows the whole frame, flipped and turned, as the canvas does.
        let image = try XCTUnwrap(model.image)
        let view = PhotographView(frame: .zero)
        view.show(image, size: model.imageSize, rotation: model.imageRotation, flip: model.imageFlip, crop: CropSettings())
        view.setFrameSize(view.displaySize)
        view.layoutSubtreeIfNeeded()
        func pick(_ p: CGPoint) async throws -> [Double] {
            let point = shownPoint(p, size: model.imageSize, crop: CropSettings(), orientation: Orientation(turns: model.imageRotation, flip: model.imageFlip))
            let fraction = try XCTUnwrap(view.photographFraction(point))
            assertEqual(fraction, p, accuracy: 1e-6)
            model.negativePick = .base
            model.pickedNegativePoint(fraction)
            try await waitUntil { model.negativeWorking == nil && model.negativeBase?.picked == true }
            return ["cyan", "magenta", "yellow"].map(base)
        }
        let picked = try await pick(CGPoint(x: 0.008, y: 0.5))
        let wrong = try await pick(CGPoint(x: 0.992, y: 0.5))
        let off = { (a: [Double]) in zip(a, foundBase).map { abs($0 - $1) }.max()! }
        print("found \(foundBase), picked on the rebate \(picked) (off \(off(picked))), picked opposite \(wrong) (off \(off(wrong)))")
        XCTAssertLessThan(off(picked), 0.05)
        XCTAssertGreaterThan(off(wrong), off(picked))
    }

    /// Off the shown photograph, as on the border's mat, a click picks nothing:
    /// it would land on pixels cropped away.
    func testPicksOffThePhotographAreIgnored() throws {
        let image = try quadrants()
        let view = PhotographView(frame: .zero)
        view.show(image, size: CGSize(width: 300, height: 200), rotation: 0, crop: CropSettings(rect: CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5)))
        // A mat of 20 points all round.
        view.setFrameSize(NSSize(width: view.displaySize.width + 40, height: view.displaySize.height + 40))
        view.layoutSubtreeIfNeeded()
        XCTAssertNil(view.photographFraction(CGPoint(x: 10, y: 10)))
        // A point in from the crop's top-left corner (y-up).
        assertEqual(try XCTUnwrap(view.photographFraction(CGPoint(x: 21, y: 119))), CGPoint(x: 76.0 / 300, y: 51.0 / 200))
    }
}
