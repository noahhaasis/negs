import AppKit
import XCTest
@testable import NegsApp

/// The pins end to end, through the release helper: measuring a tone, holding
/// it, and aiming it at a zone.
@MainActor final class ZoneModelTests: XCTestCase {
    private var directory: URL!
    private var ramp: URL!
    private var other: URL!

    override func setUpWithError() throws {
        _ = NSApplication.shared
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("negs-zone-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        ramp = directory.appendingPathComponent("ramp.png")
        other = directory.appendingPathComponent("other.png")
        try writeGradient(to: ramp)
        try writeGradient(to: other, mirrored: true)
    }

    override func tearDownWithError() throws {
        if directory != nil { try? FileManager.default.removeItem(at: directory) }
    }

    /// A horizontal grey ramp in sRGB, the tones a pick can read.
    private func writeGradient(to url: URL, mirrored: Bool = false) throws {
        let (width, height) = (64, 48)
        let bitmap = try XCTUnwrap(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
            bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: width * 3, bitsPerPixel: 24
        ))
        let base = bitmap.bitmapData!
        for y in 0..<height {
            for x in 0..<width {
                let t = mirrored ? Double(width - 1 - x) : Double(x)
                let value = UInt8((t / Double(width - 1) * 255).rounded())
                let pixel = base + y * bitmap.bytesPerRow + x * 3
                pixel[0] = value; pixel[1] = value; pixel[2] = value
            }
        }
        try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: url)
    }

    private func paths() throws -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(
            helper: root.appendingPathComponent("target/release/spektrafilm-native"),
            exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
            data: root.appendingPathComponent("data")
        )
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else {
            throw XCTSkip("Build the release helper first")
        }
        return paths
    }

    /// A settled model on the ramp, with grain off for stable measurements.
    private func makeModel(
        scanFilm: Bool = false,
        autoExposure: Bool = false,
        compensation: Bool = true
    ) async throws -> EditorModel {
        let model = EditorModel(previewCache: RenderedPreviewCache(
            file: directory.appendingPathComponent("preview-\(UUID().uuidString).cache")
        ))
        await model.start(paths: try paths())
        XCTAssertTrue(model.ready, model.error ?? "No engine")
        model.chooseFilm("kodak_portra_400")
        model.choosePaper("kodak_portra_endura")
        model.set("film_render.grain.active", .bool(false))
        model.set("camera.auto_exposure", .bool(autoExposure))
        model.set("camera.placement_ev", .number(0))
        model.set("enlarger.print_exposure_compensation", .bool(compensation))
        if scanFilm { model.set("io.scan_film", .bool(true)) }
        model.open(ramp)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        return model
    }

    private func waitUntil(_ predicate: () -> Bool, timeout: TimeInterval = 90) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(predicate(), "Timed out waiting for the engine")
    }
    /// Pins the tone at `point` (a ⌘-click) and waits for the render.
    private func pin(_ point: CGPoint = CGPoint(x: 0.5, y: 0.5), radius: Double = 0, on model: EditorModel) async throws {
        await model.addPin(at: point, radius: radius)
        try await waitUntil { model.previewCurrent }
    }

    /// The pins, in parameter order.
    private func anchors(_ model: EditorModel) -> [(input: Double, target: Double)] {
        let anchors = model.state.params["post.tone_anchors"].array
        return anchors.indices.map { index in
            (model.state.params["post.tone_anchors.\(index).input"].double,
             model.state.params["post.tone_anchors.\(index).target"].double)
        }
    }

    /// The tone under a point of the preview, as the canvas measures it.
    private func lstar(_ model: EditorModel, at point: CGPoint = CGPoint(x: 0.5, y: 0.5)) throws -> Double {
        try XCTUnwrap(
            TonePick.circle(centre: point, radius: 0).sample(try XCTUnwrap(model.image))?.lstar
        )
    }
}

extension ZoneModelTests {
    /// A ⌘-click pins the tone where it is: the curve passes through it, so
    /// the photograph doesn't change.
    func testAPinHoldsItsTone() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let before = try lstar(model)
        try await pin(on: model)
        XCTAssertEqual(model.pins.count, 1)
        XCTAssertEqual(model.pins[0].input, before, accuracy: 0.1)
        XCTAssertEqual(model.pins[0].target, before, accuracy: 0.1)
        XCTAssertEqual(model.selectedPin, 0)
        XCTAssertEqual(try lstar(model), before, accuracy: 1.0)
    }

    /// A pin reads the tone before the curve: pulling one tone down must not
    /// change what a second pin elsewhere measures.
    func testASecondPinReadsTheToneBeforeTheFirstPinsCurve() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let light = CGPoint(x: 0.75, y: 0.5)
        let untouched = try lstar(model, at: light)
        try await pin(on: model)
        model.aimSelectedPin(atZone: 2)
        try await waitUntil { model.previewCurrent }
        XCTAssertLessThan(try lstar(model, at: light), untouched - 3, "The curve should have moved this tone")
        try await pin(light, on: model)
        XCTAssertEqual(try XCTUnwrap(model.pins.last).input, untouched, accuracy: 1)
    }

    /// Holding a tone leaves a photograph with Post edits as it was.
    func testAPinKeepsThePostEdit() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set("post.brightness", .number(40))
        try await waitUntil { model.previewCurrent }
        let probe = CGPoint(x: 0.25, y: 0.5), before = try lstar(model, at: probe)
        try await pin(on: model)
        XCTAssertEqual(try lstar(model, at: probe), before, accuracy: 1.5)
    }

    /// Clicking a band aims the selected pin at that zone, and the tone
    /// prints there.
    func testAimingAPinLandsTheToneOnTheZone() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pin(on: model)
        let target = ZoneScale.zones[5].target
        model.aimSelectedPin(atZone: 5)
        XCTAssertEqual(anchors(model).last?.target, target)
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(try lstar(model), target, accuracy: 1.5)
    }

    /// The curve is applied to the scan after the print, so the print's own
    /// exposure compensation can't cancel a pin.
    func testAimingWorksWithPrintCompensationOnAndOff() async throws {
        for compensation in [true, false] {
            let model = try await makeModel(compensation: compensation)
            defer { model.shutdown() }
            try await pin(on: model)
            let target = ZoneScale.zones[4].target
            model.aimSelectedPin(atZone: 4)
            try await waitUntil { model.previewCurrent }
            XCTAssertEqual(try lstar(model), target, accuracy: 1.5, "compensation \(compensation)")
        }
    }

    /// Zone 0 is the black point and VIII paper white, whichever order the
    /// pins were made in.
    func testTheEndsAreTheBlackAndWhitePoints() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pin(CGPoint(x: 0.05, y: 0.5), on: model)
        model.aimSelectedPin(atZone: 0)
        try await waitUntil { model.previewCurrent }
        XCTAssertLessThan(try lstar(model, at: CGPoint(x: 0.05, y: 0.5)), 1.0)

        try await pin(CGPoint(x: 0.95, y: 0.5), on: model)
        model.aimSelectedPin(atZone: 8)
        try await waitUntil { model.previewCurrent }
        XCTAssertGreaterThan(try lstar(model, at: CGPoint(x: 0.95, y: 0.5)), 99.0)
        // And a later interior pin leaves both ends alone.
        let ends = anchors(model)
        XCTAssertEqual(ends.first { $0.target < 0.5 }?.target, 0)
        XCTAssertEqual(ends.first { $0.target > 99.5 }?.target, 100)
    }

    func testAimingIsOneUndoStep() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        try await pin(on: model)
        let held = anchors(model)[0].target
        model.aimSelectedPin(atZone: 5)
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(undo.undoActionName, "Aim Tone Pin")
        undo.undo()
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(anchors(model)[0].target, held)
    }

    func testRemovingAPinIsOneUndoStep() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        try await pin(on: model)
        model.removeSelectedPin()
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertEqual(undo.undoActionName, "Remove Tone Pin")
        undo.undo()
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(model.pins.count, 1)
    }

    /// A zone holds one pin: a second tone measured there re-aims the first.
    func testOnePinPerZone() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pin(CGPoint(x: 0.44, y: 0.5), on: model)
        let zone = ZoneScale.zone(forTarget: model.pins[0].target)
        try await pin(CGPoint(x: 0.48, y: 0.5), on: model)
        XCTAssertEqual(model.pins.count, 1, "the second tone replaced the first")
        XCTAssertEqual(ZoneScale.zone(forTarget: model.pins[0].target), zone)
    }

    /// ⌘' hides every pin at once, and hiding drops the selection.
    func testToggleTonePinsHidesThemAll() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pin(CGPoint(x: 0.3, y: 0.5), on: model)
        try await pin(CGPoint(x: 0.7, y: 0.5), on: model)
        XCTAssertEqual(model.pins.count, 2)
        XCTAssertEqual(model.pinMarkers.count, 2)
        model.toggleTonePins()
        XCTAssertFalse(model.showsPins)
        XCTAssertTrue(model.pinMarkers.isEmpty, "all of them hide together")
        XCTAssertEqual(model.pins.count, 2, "hiding is not deleting")
        XCTAssertNil(model.selectedPin)
        model.toggleTonePins()
        XCTAssertEqual(model.pinMarkers.count, 2)
    }

    /// A pin remembers the circle it averaged, so the canvas can draw its
    /// chip and ring.
    func testPinsCarryTheirCircle() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pin(CGPoint(x: 0.3, y: 0.6), radius: 0.1, on: model)
        let source = try XCTUnwrap(model.pins[0].source)
        XCTAssertEqual(source.centre.x, 0.3, accuracy: 0.001)
        XCTAssertEqual(source.centre.y, 0.6, accuracy: 0.001)
        XCTAssertEqual(source.radius, 0.1, accuracy: 1e-9)
        XCTAssertEqual(model.pinMarkers.first?.number, 1)
        XCTAssertEqual(model.pinMarkers.first?.source.radius ?? 0, 0.1, accuracy: 1e-9)
    }

    /// A new photograph starts with no pins: they describe the tones of the
    /// file they were measured on.
    func testANewPhotographKeepsNoPins() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pin(on: model)
        model.set("camera.placement_ev", .number(1))
        model.open(other)
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertEqual(model.value("camera.placement_ev").double, 0)
        XCTAssertNil(model.selectedPin)
    }

    /// A saved session keeps its pins: an empty default array takes the saved
    /// entries, so `post.tone_anchors` comes back whole.
    func testSavedPinsSurviveRestoration() {
        let pin = JSONValue.object([
            "input": .number(49.2), "target": .number(36.35),
            "source": .object(["x": .number(0.4), "y": .number(0.5), "radius": .number(0.02)])
        ])
        let defaults = JSONValue.object(["post": .object(["tone_anchors": .array([])])])
        let saved = JSONValue.object(["post": .object(["tone_anchors": .array([pin])])])
        XCTAssertEqual(defaults.restoring(saved), saved)
    }
}
