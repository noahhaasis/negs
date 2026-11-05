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
    /// Drops a zone on `point` and waits for the render.
    private func place(_ zone: Int, at point: CGPoint = CGPoint(x: 0.5, y: 0.5), on model: EditorModel) async throws {
        model.sampleRadius = 0
        await model.placeZone(zone, at: point)
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
    private var dark: CGPoint { CGPoint(x: 0.25, y: 0.5) }
    private var light: CGPoint { CGPoint(x: 0.75, y: 0.5) }

    /// A dropped zone makes that spot's tone that zone.
    func testPlacingAZoneMakesTheSpotThatTone() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, on: model)
        XCTAssertEqual(try lstar(model), ZoneScale.zones[5].target, accuracy: 1.5)
        XCTAssertEqual(model.selectedPin, 0)
        XCTAssertEqual(model.pinsToShow.first?.zone, 5)
    }

    /// Zone 0 on a lighter tone raises the black point: that tone and
    /// everything under it print black.
    func testDraggingZoneZeroRaisesTheBlackPoint() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let (spot, above) = (try lstar(model), try lstar(model, at: light))
        XCTAssertGreaterThan(spot, 10)
        try await place(0, on: model)
        XCTAssertEqual(try lstar(model), 0, accuracy: 1.5)
        XCTAssertEqual(try lstar(model, at: dark), 0, accuracy: 1.5)
        let raised = try lstar(model, at: light)
        XCTAssertGreaterThan(raised, 1)
        XCTAssertLessThan(raised, above)
    }

    /// A second zone recalculates the curve from the tones before any pin, and
    /// the first still lands.
    func testASecondPinRecalculatesFromTheOriginalTones() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let untouched = try lstar(model, at: light)
        try await place(4, on: model)
        try await place(7, at: light, on: model)
        XCTAssertEqual(model.pins[1].input, untouched, accuracy: 1)
        XCTAssertEqual(try lstar(model), ZoneScale.zones[4].target, accuracy: 1.5)
        XCTAssertEqual(try lstar(model, at: light), ZoneScale.zones[7].target, accuracy: 1.5)
    }

    /// Dragging a zone that's already placed moves its pin.
    func testDraggingAPlacedZoneMovesItsPin() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, on: model)
        try await place(5, at: light, on: model)
        XCTAssertEqual(model.pins.count, 1)
        XCTAssertEqual(try XCTUnwrap(model.pins[0].source).centre.x, light.x, accuracy: 0.001)
        XCTAssertEqual(try lstar(model, at: light), ZoneScale.zones[5].target, accuracy: 1.5)
    }

    /// A drop while a render is running waits for it instead of vanishing.
    func testPlacingDuringARenderWaitsAndLands() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.sampleRadius = 0
        model.set("post.brightness", .number(20))
        XCTAssertFalse(model.previewCurrent)
        await model.placeZone(5, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(model.pins.count, 1)
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(try lstar(model), ZoneScale.zones[5].target, accuracy: 1.5)
    }

    /// A drop still waiting when another photograph opens lands on nothing.
    func testOpeningAnotherPhotographDropsAWaitingPlacement() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set("post.brightness", .number(20))
        let waiting = Task { await model.placeZone(5, at: CGPoint(x: 0.5, y: 0.5)) }
        await Task.yield()
        model.open(other)
        await waiting.value
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
    }

    /// A burst of scrolling is one undo step that measures the new area, and
    /// undoing restores both.
    func testResizingIsOneUndoStepAndRemeasures() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        try await place(5, on: model)
        let before = model.pins[0]
        let centre = CGPoint(x: 0.2, y: 0.5)
        await model.movePin(0, to: centre)
        try await waitUntil { model.previewCurrent }
        let moved = model.pins[0]
        for _ in 0..<5 { model.resizeSelectedPin(scrollDelta: 60, precise: true) }
        let shown = try XCTUnwrap(model.pinsToShow.first).source.radius
        XCTAssertGreaterThan(shown, 0, "The circle follows the wheel at once")
        XCTAssertEqual(model.pins[0].source?.radius, 0, "It isn't an edit yet")
        try await waitUntil { model.pins[0].source?.radius == shown && model.previewCurrent }
        XCTAssertEqual(undo.undoActionName, "Resize Tone Pin")
        XCTAssertNotEqual(model.pins[0].input, moved.input, "The wider area averages other tones")
        XCTAssertEqual(model.sampleRadius, shown, "The next pin starts at this size")
        undo.undo()
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(model.pins[0].source?.radius, 0)
        XCTAssertEqual(model.pins[0].input, moved.input)
        XCTAssertNotEqual(before.source?.centre, moved.source?.centre)
    }

    func testScrollWithNoSelectionDoesNothing() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, on: model)
        model.selectPin(nil)
        model.resizeSelectedPin(scrollDelta: 60, precise: true)
        XCTAssertEqual(model.pinsToShow.first?.source.radius, 0)
        XCTAssertEqual(model.sampleRadius, 0)
    }

    func testUndoingAPlacementClearsTheSelection() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        try await place(5, on: model)
        XCTAssertEqual(undo.undoActionName, "Place Tone Pin")
        undo.undo()
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertNil(model.selectedPin)
    }

    func testRemovingAPinIsOneUndoStep() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        try await place(5, on: model)
        model.removeSelectedPin()
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertEqual(undo.undoActionName, "Remove Tone Pin")
        undo.undo()
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(model.pins.count, 1)
    }

    /// A pairing the curve can't honour is marked and has no effect, and works
    /// again once a pin moves out of its way.
    func testACrossedPinIsMarkedIgnoredAndRecovers() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(6, at: dark, on: model)
        try await place(3, at: light, on: model)
        XCTAssertEqual(model.pinsToShow.map(\.ignored), [false, true])
        XCTAssertEqual(model.pins.count, 2, "It's never removed automatically")
        await model.movePin(0, to: CGPoint(x: 0.9, y: 0.5))
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(model.pinsToShow.map(\.ignored), [false, false])
        XCTAssertEqual(try lstar(model, at: light), ZoneScale.zones[3].target, accuracy: 1.5)
    }

    /// Hiding the panel hides the pins, not what they do.
    func testHidingThePanelHidesPinsButKeepsTheEffect() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "showZonePanel")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "showZonePanel")
        }
        try await place(5, on: model)
        let printed = try lstar(model)
        model.zonePanelVisible = false
        XCTAssertTrue(model.pinsToShow.isEmpty)
        XCTAssertNil(model.selectedPin)
        XCTAssertEqual(model.pins.count, 1)
        XCTAssertTrue(model.previewCurrent, "Hiding doesn't develop again")
        XCTAssertEqual(try lstar(model), printed)
        model.zonePanelVisible = true
        XCTAssertEqual(model.pinsToShow.count, 1)
    }

    func testThePanelIsHiddenWhileCropping() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        XCTAssertTrue(model.showsZonePanel)
        XCTAssertTrue(model.canPlaceZones)
        model.beginCrop()
        XCTAssertFalse(model.showsZonePanel)
        XCTAssertFalse(model.canPlaceZones)
    }

    /// The toolbar's Zones button, in the group with zoom and crop, toggles it.
    func testTheToolbarButtonTogglesThePanel() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "showZonePanel")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "showZonePanel")
        }
        let controller = EditorWindowController(model: model)
        let toolbar = try XCTUnwrap(controller.window?.toolbar)
        let groups = toolbar.items.compactMap { $0 as? NSToolbarItemGroup }
        let group = try XCTUnwrap(groups.first { $0.subitems.contains { $0.label == "Crop" } })
        let button = try XCTUnwrap(group.subitems.first { $0.label == "Zones" })
        let before = model.zonePanelVisible
        _ = (button.target as AnyObject).perform(button.action)
        XCTAssertNotEqual(model.zonePanelVisible, before)
    }

    /// Pins from before zones held their own tone: each takes the zone its
    /// target falls in.
    func testSavedHoldingPinsTakeTheZoneTheirTargetFallsIn() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set("post.tone_anchors", .array([.object([
            "input": .number(30), "target": .number(30),
            "source": .object(["x": .number(0.5), "y": .number(0.5), "radius": .number(0)])
        ])]))
        XCTAssertEqual(model.pinsToShow.first?.zone, ZoneScale.zone(forTarget: 30))
    }

    /// The menu command saves the photograph's edit, pins included, beside it.
    func testSavingSettingsWritesThePinsBesideThePhotograph() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, on: model)
        model.saveSettings()
        let file = directory.appendingPathComponent("ramp.json")
        let saved = try JSONDecoder().decode(PhotoSettingsFile.self, from: Data(contentsOf: file))
        XCTAssertEqual(saved.image, "ramp.png")
        XCTAssertEqual(saved.state, model.state)
        XCTAssertEqual(saved.state.params["post.tone_anchors"].array.count, 1)
        XCTAssertEqual(model.status, "Saved ramp.json")
    }

    /// A new photograph starts with no pins: they describe the tones of the
    /// file they were measured on.
    func testANewPhotographKeepsNoPins() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, on: model)
        model.open(other)
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
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
