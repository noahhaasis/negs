import AppKit
import ImageIO
import SwiftUI
import XCTest
@testable import NegsApp

/// The pins end to end, through the release helper: measuring a tone, holding
/// it, and aiming it at a zone.
@MainActor final class ZoneModelTests: XCTestCase {
    private var directory: URL!
    private var ramp: URL!
    private var other: URL!

    override func setUpWithError() throws {
        UserDefaults.standard.removeObject(forKey: "imageSize")
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
        UserDefaults.standard.removeObject(forKey: "imageSize")
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
        // What a test sets is for this photograph: another opens at the defaults.
        model.open(ramp)
        model.chooseFilm("kodak_portra_400")
        model.choosePaper("kodak_portra_endura")
        model.set("film_render.grain.active", .bool(false))
        model.set("camera.auto_exposure", .bool(autoExposure))
        model.set("enlarger.print_exposure_compensation", .bool(compensation))
        if scanFilm { model.set("io.scan_film", .bool(true)) }
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

    /// Zone 0 on a lighter tone prints it in the middle of the zone's band, like
    /// any other, with what's under it still darker rather than clipped.
    func testZoneZeroPrintsInTheMiddleOfItsBand() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let (spot, above) = (try lstar(model), try lstar(model, at: light))
        XCTAssertGreaterThan(spot, 10)
        try await place(0, on: model)
        XCTAssertEqual(try lstar(model), ZoneScale.zones[0].target, accuracy: 1.5)
        XCTAssertLessThanOrEqual(try lstar(model, at: dark), try lstar(model))
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

    /// Dragging a zone again adds a pin, and the average of a zone's readings
    /// prints as the zone.
    func testPinsOnAZoneAverageTheirReadings() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let target = ZoneScale.zones[5].target
        try await place(5, at: dark, on: model)
        try await place(5, at: light, on: model)
        XCTAssertEqual(model.pins.count, 2)
        XCTAssertEqual(model.selectedPin, 1)
        XCTAssertEqual(model.pinsToShow.map(\.zone), [5, 5])
        XCTAssertEqual(model.pinsToShow.map(\.ignored), [false, false])
        // The ramp's two spots straddle the average, which prints as the zone.
        XCTAssertLessThan(try lstar(model, at: dark), target - 3)
        XCTAssertGreaterThan(try lstar(model, at: light), target + 3)
    }

    /// Removing one of a zone's pins leaves the others averaging.
    func testRemovingOnePinKeepsTheZonesOthers() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, at: dark, on: model)
        try await place(5, at: light, on: model)
        model.removeSelectedPin()
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(model.pins.count, 1)
        XCTAssertEqual(try lstar(model, at: dark), ZoneScale.zones[5].target, accuracy: 1.5)
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

    /// Tab hides the pins and lines, not what they do, and the panel stays.
    func testHidingPinsKeepsThePanelAndTheEffect() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "showTonePins")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "showTonePins")
        }
        try await place(5, on: model)
        let printed = try lstar(model)
        model.pinsVisible = false
        XCTAssertTrue(model.pinsToShow.isEmpty)
        XCTAssertNil(model.selectedPin)
        XCTAssertTrue(model.showsZonePanel, "The panel stays up")
        XCTAssertEqual(model.pins.count, 1)
        XCTAssertTrue(model.previewCurrent, "Hiding doesn't develop again")
        XCTAssertEqual(try lstar(model), printed)
        model.pinsVisible = true
        XCTAssertEqual(model.pinsToShow.count, 1)
    }

    /// A zone dropped while the pins are hidden brings them back, so the new
    /// pin can be seen and sized.
    func testDroppingAZoneShowsThePins() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "showTonePins")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "showTonePins")
        }
        model.pinsVisible = false
        XCTAssertTrue(model.canPlaceZones)
        try await place(5, on: model)
        XCTAssertTrue(model.pinsVisible)
        XCTAssertEqual(model.pinsToShow.first?.selected, true)
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

    /// Through a real window: the proof is the export's size whatever happens,
    /// and nothing changes the window's size or position, which stays the user's.
    func testAProofNeverTouchesTheWindowOrResizesThePhotograph() async throws {
        let model = try await makeModel()
        defer {
            model.shutdown()
        }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        window.setFrame(NSRect(x: 80, y: 80, width: 1100, height: 640), display: true)
        try await Task.sleep(for: .milliseconds(300))
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let scroll = try XCTUnwrap(all(try XCTUnwrap(window.contentView)).compactMap { $0 as? ImageScrollView }.first)
        var frame = window.frame
        func settle() async throws {
            try await waitUntil { model.previewCurrent }
            try await Task.sleep(for: .milliseconds(400))
        }
        func assertProof(_ step: String, line: UInt = #line) throws {
            XCTAssertEqual(window.frame, frame, step, line: line)
            let document = try XCTUnwrap(scroll.documentView).frame.size
            let onScreen = max(document.width, document.height) * scroll.magnification * window.backingScaleFactor
            let export = max(model.exportPixels.width, model.exportPixels.height)
            XCTAssertEqual(onScreen, export, accuracy: export * 0.01, step, line: line)
        }
        model.set(ImageSize.targetPath, .number(2000))
        try await settle()
        try await settle()
        XCTAssertTrue(window.styleMask.contains(.resizable), "The window stays resizable")
        try assertProof("Entering")
        model.rotate()
        try await settle()
        try assertProof("A different orientation")
        model.set("io.rotate_cw", .number(0))
        try await settle()
        try assertProof("Turned back")
        model.filmstripVisible.toggle()
        try await settle()
        try assertProof("Film strip")
        model.filmstripVisible.toggle()
        try await settle()
        model.set(ImageSize.targetPath, .number(1000))
        try await settle()
        try assertProof("A new export size")
        model.open(other)
        try await settle()
        try assertProof("Another photograph")
        frame = NSRect(x: 120, y: 120, width: 1300, height: 800)
        window.setFrame(frame, display: true)
        try await settle()
        try assertProof("The user resizes the window")
        try await settle()
        XCTAssertEqual(window.frame, frame, "Leaving")
    }

    /// A panned proof stays on the same part of the photograph when the preview
    /// changes resolution, as it does during a slider drag.
    func testAProofKeepsItsPlaceWhenThePreviewChangesResolution() async throws {
        let model = try await makeModel()
        defer {
            model.limitPreview(false)
            model.shutdown()
        }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        func settle() async throws {
            try await waitUntil { model.previewCurrent }
            try await Task.sleep(for: .milliseconds(400))
        }
        model.limitPreview(false)
        model.set(ImageSize.targetPath, .number(6000))
        try await settle()
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let scroll = try XCTUnwrap(all(try XCTUnwrap(window.contentView)).compactMap { $0 as? ImageScrollView }.first)
        let document = try XCTUnwrap(scroll.documentView)
        XCTAssertTrue(scroll.canScroll)
        let before = document.frame.size
        let visible = scroll.contentView.bounds
        scroll.contentView.scroll(to: CGPoint(x: visible.minX + visible.width * 0.2, y: visible.minY))
        scroll.reflectScrolledClipView(scroll.contentView)
        func place() -> CGFloat { scroll.contentView.bounds.midX / document.frame.width }
        let placed = place()
        let onScreen = scroll.magnification * before.width
        model.limitPreview(true)
        try await settle()
        XCTAssertLessThan(document.frame.width, before.width, "The preview's resolution changed")
        XCTAssertEqual(place(), placed, accuracy: 0.01)
        XCTAssertEqual(scroll.magnification * document.frame.width, onScreen, accuracy: onScreen * 0.01)
    }

    /// A window on a folder of `photos` frames, with real mouse events to send it.
    private func openRoll(_ model: EditorModel, photos: Int = 0) async throws -> (EditorWindowController, NSWindow, NSScrollView) {
        for index in 0..<photos { try FileManager.default.copyItem(at: ramp, to: directory.appendingPathComponent("z\(index).png")) }
        model.filmstripVisible = true
        model.filmstripHeight = FilmRollLayout.defaultStripHeight
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.showWindow(nil)
        window.setFrame(NSRect(x: 80, y: 80, width: 1300, height: 800), display: true)
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(700))
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let scrolls = all(try XCTUnwrap(window.contentView)).compactMap { $0 as? NSScrollView }
        let roll = try XCTUnwrap(scrolls.first { abs($0.bounds.height - FilmRollLayout.defaultStripHeight) < 1 }, "\(scrolls.map(\.bounds))")
        return (controller, window, roll)
    }

    /// `time` is when the event happened, as a real event carries it, so a busy
    /// test machine can't change how fast a drag looks.
    private func send(_ type: NSEvent.EventType, _ location: CGPoint, to window: NSWindow, clicks: Int = 1, time: TimeInterval? = nil) throws {
        window.sendEvent(try XCTUnwrap(NSEvent.mouseEvent(with: type, location: location, modifierFlags: [],
            timestamp: time ?? ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            eventNumber: 0, clickCount: clicks, pressure: 1)))
    }

    /// The strip runs across under the photograph, the width of the window.
    func testTheFilmRollSitsUnderThePhotograph() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        let (_, window, roll) = try await openRoll(model)
        defer { window.orderOut(nil) }
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let canvas = try XCTUnwrap(all(try XCTUnwrap(window.contentView)).compactMap { $0 as? ImageScrollView }.first)
        let inWindow = roll.convert(roll.bounds, to: nil)
        XCTAssertEqual(inWindow.minX, 0, accuracy: 1)
        XCTAssertLessThanOrEqual(inWindow.maxY, canvas.convert(canvas.bounds, to: nil).minY + 1, "It's under the photograph")
        XCTAssertGreaterThan(inWindow.width, 800)
        XCTAssertEqual(canvas.convert(canvas.bounds, to: nil).minX, 0, accuracy: 1, "The zones sit on the photograph's edge")
        let height = canvas.bounds.height
        model.filmstripVisible = false
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(canvas.bounds.height, height + FilmRollLayout.defaultStripHeight, accuracy: 1, "Hidden, the canvas takes its room")
    }

    /// Real clicks: a single click opens nothing, and neither does double-clicking
    /// the film around a frame. Double-clicking the photograph does.
    func testOnlyADoubleClickOnThePhotographOpensIt() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        let (_, window, roll) = try await openRoll(model)
        defer { window.orderOut(nil) }
        XCTAssertEqual(model.source, ramp)
        let bounds = roll.convert(roll.bounds, to: nil)
        let frame = FilmRollLayout.frameLength(stripHeight: FilmRollLayout.defaultStripHeight, aspect: 4.0 / 3)
        // other.png sorts first, so it's the first frame.
        let photo = CGPoint(x: bounds.minX + frame / 2, y: bounds.midY)
        // The gap before the first photograph.
        let gap = CGPoint(x: bounds.minX + 1, y: bounds.midY)
        func click(_ point: CGPoint, times: Int) async throws {
            for count in 1...times {
                try send(.leftMouseDown, point, to: window, clicks: count)
                try send(.leftMouseUp, point, to: window, clicks: count)
            }
            try await Task.sleep(for: .milliseconds(200))
        }
        try await click(photo, times: 1)
        XCTAssertEqual(model.source, ramp, "One click doesn't")
        try await Task.sleep(for: .milliseconds(500))
        try await click(gap, times: 2)
        XCTAssertEqual(model.source, ramp, "The black around it doesn't")
        try send(.mouseMoved, photo, to: window)
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(model.source, ramp, "Pointing at it doesn't")
        try await Task.sleep(for: .milliseconds(500))
        try await click(photo, times: 2)
        XCTAssertEqual(model.source, other, "A double-click on the photograph does")
    }

    /// Real drags on the strip's top edge: it grows and shrinks within its limits,
    /// the canvas gives way, the window stays put, and the size is remembered.
    /// Dragging on the film itself doesn't resize it.
    func testDraggingTheTopEdgeResizesTheStrip() async throws {
        let model = try await makeModel()
        let saved = (model.filmstripVisible, UserDefaults.standard.object(forKey: "filmstripHeight"))
        defer {
            model.filmstripVisible = saved.0
            model.shutdown()
            UserDefaults.standard.set(saved.1, forKey: "filmstripHeight")
        }
        let (_, window, roll) = try await openRoll(model, photos: 12)
        defer { window.orderOut(nil) }
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let canvas = try XCTUnwrap(all(try XCTUnwrap(window.contentView)).compactMap { $0 as? ImageScrollView }.first)
        let frame = window.frame
        let canvasHeight = canvas.bounds.height
        let x = roll.convert(roll.bounds, to: nil).midX
        func drag(fromY start: CGFloat, by delta: CGFloat) async throws {
            try send(.leftMouseDown, CGPoint(x: x, y: start), to: window)
            for step in 1...3 {
                try send(.leftMouseDragged, CGPoint(x: x, y: start + delta * CGFloat(step) / 3), to: window)
                try await Task.sleep(for: .milliseconds(30))
            }
            try send(.leftMouseUp, CGPoint(x: x, y: start + delta), to: window)
            try await Task.sleep(for: .milliseconds(200))
        }
        func top() -> CGFloat { roll.convert(roll.bounds, to: nil).maxY }
        try await drag(fromY: top(), by: 60)
        XCTAssertEqual(model.filmstripHeight, 240, accuracy: 0.5)
        XCTAssertEqual(roll.bounds.height, 240, accuracy: 1)
        XCTAssertEqual(canvas.bounds.height, canvasHeight - 60, accuracy: 1, "The canvas gives way")
        XCTAssertEqual(window.frame, frame, "The window stays put")
        XCTAssertEqual(UserDefaults.standard.double(forKey: "filmstripHeight"), 240, accuracy: 0.5)
        let frameLength = FilmRollLayout.frameLength(stripHeight: 240, aspect: 4.0 / 3)
        XCTAssertGreaterThan(try XCTUnwrap(roll.documentView).frame.width, frameLength * 13, "The frames grow with it")
        try await drag(fromY: top(), by: -100)
        XCTAssertEqual(model.filmstripHeight, 140, accuracy: 0.5, "Down")
        // Past the limits.
        try await drag(fromY: top(), by: 1000)
        XCTAssertEqual(model.filmstripHeight, FilmRollLayout.stripHeightRange.upperBound, accuracy: 0.5)
        XCTAssertEqual(window.frame, frame, "Even at the top")
        try await drag(fromY: top(), by: -1000)
        XCTAssertEqual(model.filmstripHeight, FilmRollLayout.stripHeightRange.lowerBound, accuracy: 0.5)
        // The middle of the film only scrolls.
        let before = model.filmstripHeight
        try await drag(fromY: roll.convert(roll.bounds, to: nil).midY, by: 40)
        XCTAssertEqual(model.filmstripHeight, before)
        // A new window reads the remembered size.
        model.filmstripHeight = 210
        let reopened = EditorModel()
        defer { reopened.shutdown() }
        XCTAssertEqual(reopened.filmstripHeight, 210)
    }

    /// With RAW only on, the strip holds just the folder's RAW files, and the
    /// setting is remembered. Its numbers and clicks follow what's shown.
    func testTheStripCanShowOnlyRawFiles() async throws {
        let model = try await makeModel()
        let saved = (model.filmstripVisible, UserDefaults.standard.object(forKey: "filmstripRawOnly"))
        defer {
            model.filmstripVisible = saved.0
            model.shutdown()
            UserDefaults.standard.set(saved.1, forKey: "filmstripRawOnly")
        }
        model.filmstripRawOnly = false
        // Not real RAW files, which the strip only lists until they scroll into view.
        for index in 0..<30 { try Data().write(to: directory.appendingPathComponent("r\(index).nef")) }
        for index in 0..<20 { try Data().write(to: directory.appendingPathComponent("s\(index).DNG")) }
        let (_, window, roll) = try await openRoll(model, photos: 10)
        defer { window.orderOut(nil) }
        // Each frame fits its own photograph; these empty files are read as 3:2.
        func film(_ files: [URL]) -> CGFloat {
            files.reduce(0) { $0 + FilmRollLayout.frameLength(stripHeight: FilmRollLayout.defaultStripHeight,
                                                              aspect: PhotoFolder.aspect(of: $1) ?? 3.0 / 2) }
        }
        let all = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
            .filter { ["png", "nef", "dng"].contains($0.pathExtension.lowercased()) }
        XCTAssertEqual(all.count, 62)
        let raws = all.filter { $0.pathExtension.lowercased() != "png" }
        let document = try XCTUnwrap(roll.documentView)
        XCTAssertEqual(document.frame.width, film(all), accuracy: 1, "All of them: 50 RAW, 10 copies, ramp and other")
        model.filmstripRawOnly = true
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(document.frame.width, film(raws), accuracy: 1, "Only the RAW files")
        XCTAssertEqual(UserDefaults.standard.object(forKey: "filmstripRawOnly") as? Bool, true)
        let reopened = EditorModel()
        defer { reopened.shutdown() }
        XCTAssertTrue(reopened.filmstripRawOnly)
        model.filmstripRawOnly = false
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(document.frame.width, film(all), accuracy: 1)
    }

    /// Edits save themselves beside the photograph shortly after they're made, so
    /// every photograph keeps its own. Only looking at one writes nothing.
    func testEditsSaveThemselvesPerPhotograph() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.open(other)
        let settings = directory.appendingPathComponent("other.json")
        try await Task.sleep(for: .seconds(1.5))
        XCTAssertFalse(FileManager.default.fileExists(atPath: settings.path), "Opening it isn't an edit")
        model.set("post.contrast", .number(40))
        try await waitUntil({ FileManager.default.fileExists(atPath: settings.path) }, timeout: 5)
        XCTAssertEqual(try PhotoSettingsFile.read(beside: other)?.state.params["post.contrast"].double, 40)
        // Going to another photograph at once doesn't lose an edit just made.
        model.set("post.contrast", .number(55))
        model.open(ramp)
        XCTAssertEqual(try PhotoSettingsFile.read(beside: other)?.state.params["post.contrast"].double, 55)
        // Coming back, it's as it was left.
        model.open(other)
        XCTAssertEqual(model.value("post.contrast").double, 55)
    }

    /// A folder that can't be written doesn't interrupt editing with an alert.
    func testAFailedAutomaticSaveStaysQuiet() async throws {
        let model = try await makeModel()
        defer {
            model.shutdown()
            try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: directory.path)
        }
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: directory.path)
        model.set("post.contrast", .number(40))
        try await Task.sleep(for: .seconds(1.5))
        XCTAssertNil(model.error)
    }

    /// A photograph with no edits opens at the defaults, whatever the last one
    /// had. Export choices aren't edits, so they stay.
    func testAnUneditedPhotographOpensAtTheDefaults() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let contrast = model.defaultValue("post.contrast").double
        model.set("post.contrast", .number(contrast + 40))
        model.set(ImageSize.targetPath, .number(1500))
        model.chooseFilm("kodak_ektar_100")
        model.open(other)
        XCTAssertEqual(model.value("post.contrast").double, contrast, "It doesn't inherit the edit")
        XCTAssertNotEqual(model.film, "kodak_ektar_100", "Nor the film")
        XCTAssertEqual(model.value(ImageSize.targetPath).double, 1500, "The size is an export choice")
        // Its own edits come back, without having been passed on.
        model.open(ramp)
        XCTAssertEqual(model.value("post.contrast").double, contrast + 40)
        XCTAssertEqual(model.film, "kodak_ektar_100")
    }

    /// ⌘D makes the current settings what the folder's unedited photographs open
    /// with, without pins or crop. It's a snapshot, and it's remembered.
    func testSettingTheFoldersDefault() async throws {
        let model = try await makeModel()
        let third = directory.appendingPathComponent("third.png")
        try FileManager.default.copyItem(at: ramp, to: third)
        let elsewhere = directory.appendingPathComponent("elsewhere", isDirectory: true)
        try FileManager.default.createDirectory(at: elsewhere, withIntermediateDirectories: true)
        let away = elsewhere.appendingPathComponent("away.png")
        try FileManager.default.copyItem(at: ramp, to: away)
        defer {
            model.clearFolderDefault()
            model.shutdown()
        }
        let contrast = model.defaultValue("post.contrast").double
        // Edited first, with its own file.
        model.open(other)
        model.set("post.contrast", .number(contrast + 10))
        model.open(ramp)
        model.set("post.contrast", .number(contrast + 40))
        model.chooseFilm("kodak_ektar_100")
        await model.placeZone(5, at: CGPoint(x: 0.5, y: 0.5))
        model.setFolderDefault()
        model.open(third)
        XCTAssertEqual(model.value("post.contrast").double, contrast + 40, "It opens with the default")
        XCTAssertEqual(model.film, "kodak_ektar_100")
        XCTAssertTrue(model.pins.isEmpty, "Pins belong to their own photograph")
        model.open(other)
        XCTAssertEqual(model.value("post.contrast").double, contrast + 10, "Its own edits win")
        // A later edit doesn't move the default, until it's set again.
        model.open(ramp)
        model.set("post.contrast", .number(contrast + 25))
        model.open(third)
        XCTAssertEqual(model.value("post.contrast").double, contrast + 40)
        // Another folder has its own.
        model.open(away)
        XCTAssertEqual(model.value("post.contrast").double, contrast)
        // A new launch remembers.
        model.open(third)
        let reopened = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("again.cache")))
        defer { reopened.shutdown() }
        await reopened.start(paths: try paths())
        reopened.open(third)
        XCTAssertEqual(reopened.value("post.contrast").double, contrast + 40)
    }

    /// Pins are points on the curve. Editing one takes its zone's pins off the
    /// photograph, in one undo, leaving a plain point; points stop at their neighbours.
    func testTheToneCurveEditsPinsAsPoints() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        // What a person does comes in separate events, each its own undo.
        func nextEvent() { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
        await model.placeZone(5, at: CGPoint(x: 0.3, y: 0.5))
        await model.placeZone(5, at: CGPoint(x: 0.7, y: 0.5))
        await model.placeZone(7, at: CGPoint(x: 0.9, y: 0.5))
        XCTAssertEqual(model.curvePoints.map(\.target), [50, 70], "A zone's pins are one point")
        XCTAssertEqual(model.curvePoints[0].anchors.count, 2)
        XCTAssertTrue(model.curvePoints.allSatisfy(\.pinned))
        nextEvent()
        let before = model.state
        let zone5 = model.curvePoints[0]
        let upper = model.curvePoints[1]
        model.beginInteraction()
        var held = model.moveCurvePoint(zone5.anchors, input: zone5.input, target: 55)
        held = model.moveCurvePoint(held, input: zone5.input, target: 200)
        model.endInteraction(action: "Edit Tone Curve")
        nextEvent()
        XCTAssertEqual(model.pins.count, 2)
        XCTAssertEqual(model.pinsToShow.count, 1, "Only zone 7 is left on the photograph")
        let moved = try XCTUnwrap(model.curvePoints.first)
        XCTAssertFalse(moved.pinned)
        XCTAssertEqual(moved.target, upper.target - 0.5, "It stops at its neighbour")
        undo.undo()
        XCTAssertEqual(model.state, before, "One undo brings both pins back")
        // Added and removed.
        let added = model.addCurvePoint(input: 5, target: 3)
        XCTAssertEqual(model.curvePoints.first?.input, 5)
        model.removeCurvePoint(added)
        XCTAssertEqual(model.state, before)
        // Copied to another photograph, the points are plain.
        model.copyToneCurve()
        XCTAssertTrue(model.canPasteToneCurve)
        model.open(other)
        model.pasteToneCurve()
        XCTAssertEqual(model.curvePoints.map(\.target), [50, 70])
        XCTAssertFalse(model.curvePoints.contains(where: \.pinned))
        XCTAssertTrue(model.pinsToShow.isEmpty)
        // Reset takes it all off, undoably.
        nextEvent()
        let pasted = model.state
        model.resetToneCurve()
        nextEvent()
        XCTAssertTrue(model.pins.isEmpty)
        undo.undo()
        XCTAssertEqual(model.state, pasted)
    }

    /// Real mouse events, in the sidebar's Form: a click adds a point and holds
    /// it for the drag, and letting go outside the box takes it off.
    func testTheToneCurveBoxTakesTheMouse() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let host = NSHostingView(rootView: Form { Section("Tone") { ToneCurveBox(model: model) } }.formStyle(.grouped))
        let window = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 400, height: 420), styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        try await Task.sleep(for: .milliseconds(400))
        func send(_ type: NSEvent.EventType, _ point: CGPoint) throws {
            window.sendEvent(try XCTUnwrap(NSEvent.mouseEvent(
                with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)))
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        }
        // About the middle of the box, which is at the top of the page.
        let start = CGPoint(x: 200, y: 420 - 130)
        try send(.leftMouseDown, start)
        XCTAssertEqual(model.curvePoints.count, 1, "The click adds one")
        let first = try XCTUnwrap(model.curvePoints.first)
        try send(.leftMouseDragged, CGPoint(x: start.x + 30, y: start.y + 30))
        try send(.leftMouseUp, CGPoint(x: start.x + 30, y: start.y + 30))
        let moved = try XCTUnwrap(model.curvePoints.first)
        XCTAssertEqual(model.curvePoints.count, 1)
        XCTAssertGreaterThan(moved.input, first.input)
        XCTAssertGreaterThan(moved.target, first.target)
        // Grabbed again by its handle, and dragged well out of the box.
        let handle = CGPoint(x: start.x + 30, y: start.y + 30)
        try send(.leftMouseDown, handle)
        XCTAssertEqual(model.curvePoints.count, 1, "It takes hold rather than adding another")
        try send(.leftMouseDragged, CGPoint(x: handle.x, y: 10))
        try send(.leftMouseUp, CGPoint(x: handle.x, y: 10))
        XCTAssertTrue(model.curvePoints.isEmpty)
    }

    /// A whole edit goes across, except what's particular to the photograph.
    func testCopyingAnEditLeavesPinsAndCropBehind() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let contrast = model.defaultValue("post.contrast").double
        model.set("post.contrast", .number(contrast + 30))
        model.chooseFilm("kodak_ektar_100")
        await model.placeZone(5, at: CGPoint(x: 0.5, y: 0.5))
        model.set("io.rotate_cw", .number(1))
        model.copyEdit()
        XCTAssertTrue(model.canPasteEdit)
        model.open(other)
        await model.placeZone(3, at: CGPoint(x: 0.5, y: 0.5))
        model.set(ImageSize.targetPath, .number(1234))
        let pins = model.pins
        model.pasteEdit()
        XCTAssertEqual(model.value("post.contrast").double, contrast + 30)
        XCTAssertEqual(model.film, "kodak_ektar_100")
        XCTAssertEqual(model.pins, pins, "Its own pins")
        XCTAssertEqual(model.value("io.rotate_cw").double, 0, "Its own rotation")
        XCTAssertEqual(model.value(ImageSize.targetPath).double, 1234)
    }

    /// Opening a photograph sets the rest of its folder developing, without the
    /// grid, and nothing new starts while the open photograph is being edited.
    func testOpeningAPhotographDevelopsItsFolder() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        XCTAssertFalse(model.gridVisible)
        try await waitUntil { model.previewCurrent }
        try await waitUntil({ (model.rollDeveloper?.total ?? 0) >= 2 && model.rollDeveloper?.finished == model.rollDeveloper?.total }, timeout: 30)
        let developer = try XCTUnwrap(model.rollDeveloper)
        XCTAssertNotNil(developer.frames[other], "The other photograph's frame is waiting")
        try await waitUntil { !developer.isDeveloping }
        model.beginInteraction()
        developer.invalidateAll()
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertEqual(developer.finished, 0, "It waits for the edit")
        model.endInteraction()
        try await waitUntil({ developer.finished == developer.total }, timeout: 30)
    }

    /// In the window, the panel draws flush along the landscape photograph's top
    /// and exactly as wide, with black at its left.
    func testTheZonePanelSitsOnThePhotograph() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set(ImageSize.targetPath, .number(1000))
        try await waitUntil { model.previewCurrent }
        let (controller, window, _) = try await openRoll(model)
        defer { window.orderOut(nil) }
        _ = controller
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let content = try XCTUnwrap(window.contentView)
        let canvas = try XCTUnwrap(all(content).compactMap { $0 as? ImageScrollView }.first)
        let photo = try XCTUnwrap(canvas.documentView as? PhotographView)
        let rect = canvas.overlay.convert(photo.bounds, from: photo)
        let placement = try XCTUnwrap(model.zonePlacement)
        XCTAssertTrue(placement.horizontal)
        XCTAssertEqual(placement.frame.maxY, canvas.overlay.bounds.height - rect.maxY, accuracy: 0.5, "Flush with its top")
        XCTAssertEqual(placement.frame.minX, max(rect.minX, 0), accuracy: 0.5)
        XCTAssertEqual(placement.frame.maxX, min(rect.maxX, canvas.bounds.width), accuracy: 0.5, "As wide")
        let bitmap = try XCTUnwrap(content.bitmapImageRepForCachingDisplay(in: content.bounds))
        content.cacheDisplay(in: content.bounds, to: bitmap)
        func grey(_ canvasPoint: CGPoint) throws -> CGFloat {
            let inWindow = canvas.convert(CGPoint(x: canvasPoint.x, y: canvasPoint.y), to: content)
            let scale = CGFloat(bitmap.pixelsWide) / content.bounds.width
            let y = content.isFlipped ? inWindow.y : content.bounds.height - inWindow.y
            return try XCTUnwrap(bitmap.colorAt(x: Int(inWindow.x * scale), y: Int(y * scale))?.usingColorSpace(.sRGB)).redComponent
        }
        let tile = placement.frame.width / 11
        func at(_ zone: CGFloat) -> CGPoint { CGPoint(x: placement.frame.minX + tile * (zone + 0.15), y: placement.frame.midY) }
        XCTAssertLessThan(try grey(at(0)), 0.15, "Zone 0's tile")
        XCTAssertGreaterThan(try grey(at(10)), 0.85, "Zone 10's tile")
        // A real drag from a tile onto the photograph places a pin of that zone.
        let from = canvas.convert(CGPoint(x: at(5).x + tile * 0.35, y: at(5).y), to: nil)
        let to = canvas.convert(CGPoint(x: rect.midX, y: canvas.overlay.bounds.height - rect.midY), to: nil)
        try send(.leftMouseDown, from, to: window)
        try await Task.sleep(for: .milliseconds(50))
        try send(.leftMouseDragged, CGPoint(x: (from.x + to.x) / 2, y: (from.y + to.y) / 2), to: window)
        try await Task.sleep(for: .milliseconds(50))
        try send(.leftMouseDragged, to, to: window)
        try await Task.sleep(for: .milliseconds(50))
        try send(.leftMouseUp, to, to: window)
        try await waitUntil { !model.pins.isEmpty }
        XCTAssertEqual(model.pins.first?.zone, 5)
    }

    /// F leaves only the photograph, then brings back exactly what was showing.
    func testImageOnlyHidesEverythingAndComesBack() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        await model.placeZone(5, at: CGPoint(x: 0.5, y: 0.5))
        let (controller, window, roll) = try await openRoll(model)
        defer { window.orderOut(nil) }
        _ = controller
        model.inspectorVisible = true
        try await Task.sleep(for: .milliseconds(400))
        let split = try XCTUnwrap(window.contentViewController as? NSSplitViewController)
        let sidebar = try XCTUnwrap(split.splitViewItems.last)
        XCTAssertFalse(sidebar.isCollapsed)
        XCTAssertFalse(model.pinsToShow.isEmpty)
        XCTAssertNotNil(roll.window)
        model.toggleImageOnly()
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertTrue(model.imageOnly)
        XCTAssertFalse(model.showsZonePanel)
        XCTAssertTrue(model.pinsToShow.isEmpty)
        XCTAssertTrue(sidebar.isCollapsed)
        XCTAssertTrue(model.inspectorVisible, "Collapsing it for F isn't hiding it")
        XCTAssertNil(roll.window, "The strip's gone")
        XCTAssertEqual(window.standardWindowButton(.closeButton)?.isHidden, true)
        XCTAssertEqual(window.titleVisibility, .hidden)
        // The grid brings its controls back while it's up, and goes back to F after.
        model.toggleGrid()
        XCTAssertFalse(model.imageOnly)
        model.toggleGrid()
        XCTAssertTrue(model.imageOnly)
        model.toggleImageOnly()
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertFalse(model.imageOnly)
        XCTAssertFalse(sidebar.isCollapsed)
        XCTAssertTrue(model.showsZonePanel)
        XCTAssertFalse(model.pinsToShow.isEmpty)
        XCTAssertEqual(window.standardWindowButton(.closeButton)?.isHidden, false)
        XCTAssertEqual(window.titleVisibility, .visible)
    }

    /// Image Size is one setting for every photograph, whatever's saved with
    /// each, and it's remembered.
    func testImageSizeIsGlobal() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set(ImageSize.targetPath, .number(1500))
        model.set("post.contrast", .number(model.defaultValue("post.contrast").double + 5))
        model.saveSettings()
        model.open(other)
        XCTAssertEqual(model.value(ImageSize.targetPath).double, 1500, "Another photograph has it too")
        model.set(ImageSize.targetPath, .number(2000))
        model.open(ramp)
        XCTAssertEqual(model.value(ImageSize.targetPath).double, 2000, "Not what's saved with this one")
        model.chooseImageSize(.original)
        model.open(other)
        XCTAssertEqual(model.value(ImageSize.targetPath), .null)
        model.set(ImageSize.targetPath, .number(1800))
        let reopened = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("again.cache")))
        defer { reopened.shutdown() }
        await reopened.start(paths: try paths())
        reopened.open(ramp)
        XCTAssertEqual(reopened.value(ImageSize.targetPath).double, 1800, "A new launch remembers")
    }

    /// G swaps the grid in and out, and it stands down while typing or cropping.
    func testGTogglesTheGrid() async throws {
        let g: UInt16 = 5
        XCTAssertTrue(EditorWindowController.togglesGrid(keyCode: g, modifiers: [], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesGrid(keyCode: g, modifiers: [], firstResponder: NSTextView()))
        XCTAssertFalse(EditorWindowController.togglesGrid(keyCode: g, modifiers: [.command], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesGrid(keyCode: 3, modifiers: [], firstResponder: nil))
        let model = try await makeModel()
        defer { model.shutdown() }
        model.toggleGrid()
        XCTAssertTrue(model.gridVisible)
        XCTAssertNotNil(model.rollDeveloper)
        XCTAssertFalse(model.showsZonePanel)
        model.toggleGrid()
        XCTAssertFalse(model.gridVisible)
        XCTAssertNotNil(model.rollDeveloper, "It stays, so coming back doesn't start over")
        model.toggleGrid()
        model.beginCrop()
        XCTAssertFalse(model.gridVisible, "Cropping needs the photograph")
        model.toggleGrid()
        XCTAssertFalse(model.gridVisible, "And G waits while cropping")
        model.endCrop(apply: false)
    }

    /// Through the real engine: each photograph is developed with its own final
    /// edit, once. A second run reads the disk cache, and an edit develops again.
    func testTheDeveloperShowsEachPhotographsFinalEdit() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        // Other is much brighter than the ramp as edited.
        model.open(other)
        model.set("post.brightness", .number(90))
        model.open(ramp)
        model.set("post.brightness", .number(-90))
        try await waitUntil { model.previewCurrent }
        let cache = directory.appendingPathComponent("rollcache", isDirectory: true)
        let files = try PhotoFolder.snapshot(in: directory)
        XCTAssertEqual(files.map(\.name), ["other.png", "ramp.png"])
        func mean(_ image: CGImage) -> CGFloat {
            let bitmap = NSBitmapImageRep(cgImage: image)
            var total: CGFloat = 0
            for x in stride(from: 0, to: bitmap.pixelsWide, by: max(1, bitmap.pixelsWide / 20)) {
                total += bitmap.colorAt(x: x, y: bitmap.pixelsHigh / 2)?.usingColorSpace(.sRGB)?.redComponent ?? 0
            }
            return total / CGFloat(bitmap.pixelsWide / max(1, bitmap.pixelsWide / 20))
        }
        let paths = try paths()
        let first = try RollDeveloper(paths: paths, cache: cache) { model.frameRequest(for: $0) }
        defer { first.stop() }
        first.show(files: files)
        try await waitUntil { first.finished == 2 && first.frames.count == 2 }
        XCTAssertEqual(first.developedCount, 2)
        XCTAssertGreaterThan(mean(try XCTUnwrap(first.frames[other])), mean(try XCTUnwrap(first.frames[ramp])) + 0.1, "Each with its own edit")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: cache.path).filter { $0.hasSuffix(".jpg") }.count, 2)
        // A new run never develops them again.
        let second = try RollDeveloper(paths: paths, cache: cache) { model.frameRequest(for: $0) }
        defer { second.stop() }
        second.show(files: files)
        try await waitUntil { second.frames.count == 2 }
        XCTAssertEqual(second.developedCount, 0, "From the cache")
        // An edit to the open photograph develops just that one.
        let before = mean(try XCTUnwrap(second.frames[ramp]))
        model.set("post.brightness", .number(90))
        second.invalidate(ramp)
        try await waitUntil { second.developedCount == 1 && abs(mean(second.frames[ramp]!) - before) > 0.1 }
        XCTAssertEqual(second.developedCount, 1)
    }

    /// There's no toolbar. The sidebar's switch reaches the presets, and crop has
    /// its own Done and Cancel.
    func testTheWindowHasNoToolbar() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let saved = UserDefaults.standard.object(forKey: "inspectorPane")
        defer { UserDefaults.standard.set(saved, forKey: "inspectorPane") }
        model.showPane(.adjustments)
        let (_, window, _) = try await openRoll(model)
        defer { window.orderOut(nil) }
        XCTAssertNil(window.toolbar)
        model.showPane(.presets)
        XCTAssertEqual(model.inspectorPane, .presets)
        model.showPane(.adjustments)
    }

    /// In a real window: four photographs across, each developing with its own
    /// edit. One click does nothing; a double-click opens that photograph to edit.
    func testTheGridOpensAPhotographOnADoubleClick() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        let (_, window, _) = try await openRoll(model, photos: 10)
        defer { window.orderOut(nil) }
        XCTAssertEqual(model.source, ramp)
        model.toggleGrid()
        try await Task.sleep(for: .milliseconds(700))
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let scrolls = all(try XCTUnwrap(window.contentView)).compactMap { $0 as? NSScrollView }
        XCTAssertFalse(scrolls.contains { $0 is ImageScrollView }, "The photograph has gone")
        let grid = try XCTUnwrap(scrolls.first { $0.bounds.height > 400 })
        let document = try XCTUnwrap(grid.documentView)
        let bounds = grid.convert(grid.bounds, to: nil)
        let width = PhotoGridLayout.cellWidth(available: bounds.width)
        // 12 photographs make three rows.
        let cell = PhotoGridLayout.cellHeight(cellWidth: width)
        XCTAssertEqual(document.frame.height, cell * 3 + PhotoGridLayout.rowGap * 2 + PhotoGridLayout.margin * 2, accuracy: 2)
        // They all develop, with the final edits.
        let developer = try XCTUnwrap(model.rollDeveloper)
        try await waitUntil({ developer.total == 12 && developer.finished == 12 }, timeout: 30)
        XCTAssertNotNil(developer.frames[ramp])
        // The third photograph of the first row: other, ramp, z0.
        let top = document.convert(document.bounds, to: nil).maxY
        let point = CGPoint(x: bounds.minX + PhotoGridLayout.margin + (width + PhotoGridLayout.columnGap) * 2 + width / 2,
                            y: top - PhotoGridLayout.margin - PhotoGridLayout.lineHeight - PhotoGridLayout.lineGap - PhotoGridLayout.boxHeight(cellWidth: width) / 2)
        try send(.leftMouseDown, point, to: window, clicks: 1)
        try send(.leftMouseUp, point, to: window, clicks: 1)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertTrue(model.gridVisible, "One click doesn't")
        XCTAssertEqual(model.source, ramp)
        for clicks in 1...2 {
            try send(.leftMouseDown, point, to: window, clicks: clicks)
            try send(.leftMouseUp, point, to: window, clicks: clicks)
        }
        try await waitUntil({ !model.gridVisible }, timeout: 5)
        XCTAssertEqual(model.source, directory.appendingPathComponent("z0.png"))
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertTrue(all(try XCTUnwrap(window.contentView)).contains { $0 is ImageScrollView }, "The photograph is back")
    }

    /// Going back to a photograph shows its developed frame at once, with no gap
    /// and no embedded preview first.
    func testGoingBackToAPhotographShowsItAtOnce() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await waitUntil { model.previewCurrent }
        model.open(other)
        try await waitUntil { model.previewCurrent }
        model.open(ramp)
        XCTAssertNotNil(model.image, "In the same instant")
        XCTAssertNil(model.placeholder)
        XCTAssertFalse(model.previewCurrent, "It's confirmed by a render behind it")
        try await waitUntil { model.previewCurrent }
    }

    /// An edit means a different frame: going back after one never shows the old look.
    func testAnEditedPhotographDoesNotShowItsOldFrame() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await waitUntil { model.previewCurrent }
        let old = try XCTUnwrap(model.image)
        model.set("post.brightness", .number(60))
        try await waitUntil { model.previewCurrent }
        model.open(other)
        try await waitUntil { model.previewCurrent }
        model.open(ramp)
        XCTAssertNotNil(model.image)
        XCTAssertFalse(model.image === old, "The edited frame, not the first one")
    }

    /// A photograph not in memory comes from the disk, in a new launch too. Until
    /// it's read the last photograph stays up rather than a blank.
    func testAFrameComesBackFromTheDiskAfterARestart() async throws {
        let file = directory.appendingPathComponent("shared.cache")
        let first = EditorModel(previewCache: RenderedPreviewCache(file: file))
        await first.start(paths: try paths())
        first.open(ramp)
        try await waitUntil { first.previewCurrent }
        await first.flushPreviewCache()
        first.shutdown()
        XCTAssertFalse(try FileManager.default.contentsOfDirectory(atPath: file.path + ".frames").isEmpty)

        let second = EditorModel(previewCache: RenderedPreviewCache(file: file))
        defer { second.shutdown() }
        await second.start(paths: try paths())
        second.open(other)
        try await waitUntil { second.previewCurrent }
        second.open(ramp)
        XCTAssertTrue(second.image != nil || second.placeholder != nil, "Never blank: the last photograph holds until this one's read")
        try await waitUntil({ second.image != nil }, timeout: 5)
        XCTAssertTrue(second.showingCachedPreview, "From the disk, not a new develop")
    }

    /// The status bar's caption in full screen names the photograph and its stock.
    func testTheFullScreenCaption() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        XCTAssertNil(model.fullScreenCaption)
        model.windowFullScreen = true
        let caption = try XCTUnwrap(model.fullScreenCaption)
        XCTAssertTrue(caption.hasPrefix("ramp.png"), caption)
        XCTAssertTrue(caption.contains(model.filmName), caption)
        XCTAssertTrue(caption.contains(model.paperName), caption)
    }

    /// Moving to a photograph with no frame yet shows nothing in between: the last
    /// one stays up until the new one's final develop replaces it.
    func testNoInBetweenImageShowsWhileAPhotographDevelops() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await waitUntil { model.previewCurrent }
        let leaving = try XCTUnwrap(model.image)
        model.open(other)
        let deadline = Date().addingTimeInterval(30)
        while model.image == nil, Date() < deadline {
            XCTAssertTrue(model.placeholder === leaving, "Only the last photograph, never a preview")
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertNotNil(model.image)
        XCTAssertNil(model.placeholder)
    }

    /// The grid's reveal waits for the frames around where it opens, then holds.
    func testTheDeveloperSaysWhenTheFramesNearbyAreReady() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        for index in 0..<30 { try FileManager.default.copyItem(at: ramp, to: directory.appendingPathComponent("z\(index).png")) }
        let files = try PhotoFolder.snapshot(in: directory).filter { $0.url.pathExtension == "png" }
        let developer = try RollDeveloper(paths: try paths(), cache: directory.appendingPathComponent("near", isDirectory: true)) { model.frameRequest(for: $0) }
        defer { developer.stop() }
        XCTAssertFalse(developer.nearReady)
        developer.show(files: files, centre: 15)
        XCTAssertFalse(developer.nearReady, "Nothing's developed yet")
        XCTAssertEqual(developer.nearTotal, 25)
        try await waitUntil { developer.nearReady }
        // All of them, in memory, the moment it says so.
        for index in 3...27 {
            XCTAssertNotNil(developer.frames[files[index].url], "\(index)")
        }
    }

    /// Going in and out of the grid develops nothing again, and has its frames ready
    /// at once. Nor does a change that doesn't affect them, or moving between photos.
    func testGoingInAndOutOfTheGridDevelopsNothingAgain() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        let (_, window, _) = try await openRoll(model, photos: 6)
        defer { window.orderOut(nil) }
        model.toggleGrid()
        try await waitUntil({ model.rollDeveloper?.total == 8 && model.rollDeveloper?.finished == 8 }, timeout: 30)
        let developer = try XCTUnwrap(model.rollDeveloper)
        let developed = developer.developedCount
        XCTAssertEqual(developed, 8)
        XCTAssertFalse(developer.isDeveloping)
        for _ in 0..<3 {
            model.toggleGrid()
            try await Task.sleep(for: .milliseconds(150))
            model.toggleGrid()
            XCTAssertTrue(model.rollDeveloper === developer, "The same developer")
            XCTAssertTrue(developer.nearReady, "Ready the moment it's back")
            try await Task.sleep(for: .milliseconds(200))
            XCTAssertFalse(developer.isDeveloping)
        }
        // Another photograph, and back.
        model.toggleGrid()
        model.open(other)
        try await waitUntil { model.previewCurrent }
        model.open(ramp)
        try await waitUntil { model.previewCurrent }
        model.toggleGrid()
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertEqual(developer.developedCount, developed, "The same edits, the same frames")
        // Image Size is about the export, not these frames.
        model.toggleGrid()
        model.set(ImageSize.targetPath, .number(1500))
        try await waitUntil { model.previewCurrent }
        model.toggleGrid()
        developer.invalidateAll()
        try await waitUntil({ developer.finished == 8 }, timeout: 10)
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertEqual(developer.developedCount, developed, "Not after a new Image Size either")
        // A real edit does, of just that photograph.
        model.set("post.brightness", .number(70))
        developer.invalidate(ramp)
        try await waitUntil({ developer.developedCount == developed + 1 }, timeout: 10)
    }

    /// Another folder is another roll, with a developer of its own.
    func testAnotherFolderGetsItsOwnDeveloper() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.toggleGrid()
        let first = try XCTUnwrap(model.rollDeveloper)
        model.toggleGrid()
        let elsewhere = directory.appendingPathComponent("elsewhere", isDirectory: true)
        try FileManager.default.createDirectory(at: elsewhere, withIntermediateDirectories: true)
        let away = elsewhere.appendingPathComponent("away.png")
        try FileManager.default.copyItem(at: ramp, to: away)
        model.open(away)
        model.toggleGrid()
        XCTAssertFalse(model.rollDeveloper === first)
    }

    /// Grid frames read only as much of a RAW as they need; nothing else does.
    func testGridFramesReadLessOfTheFile() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let request = try XCTUnwrap(model.frameRequest(for: ramp))
        XCTAssertEqual(request.command["decode_long_side"]?.double, 1200)
        XCTAssertEqual(request.command["long_side"]?.double, 800)
    }

    /// Nothing but the person moves the film: opening a photograph in view leaves
    /// it, even at the far side, and only a frame wholly out of view brings it round.
    func testTheStripDoesNotCentreTheOpenPhotograph() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        let (_, window, roll) = try await openRoll(model, photos: 40)
        defer { window.orderOut(nil) }
        func offset() -> CGFloat { roll.contentView.bounds.origin.x }
        func file(_ index: Int) -> URL { directory.appendingPathComponent("z\(index).png") }
        let frame = FilmRollLayout.frameLength(stripHeight: FilmRollLayout.defaultStripHeight, aspect: 4.0 / 3)
        XCTAssertEqual(FilmRollLayout.defaultStripHeight, 180)
        XCTAssertEqual(roll.bounds.height, 180, accuracy: 1)
        let bounds = roll.convert(roll.bounds, to: nil)
        // Double-clicking a frame at the strip's far side, sorted other, ramp, z0, z1…
        let left = roll.contentView.bounds.origin.x
        let index = Int((bounds.width - frame) / frame)
        let point = CGPoint(x: bounds.minX + (CGFloat(index) + 0.5) * frame - left, y: bounds.midY)
        for clicks in 1...2 {
            try send(.leftMouseDown, point, to: window, clicks: clicks)
            try send(.leftMouseUp, point, to: window, clicks: clicks)
        }
        try await waitUntil { model.source == file(index - 2) }
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertEqual(offset(), left, accuracy: 0.5, "A double-click doesn't move the film")
        // A photograph further along, as from Open or the arrow keys.
        model.open(file(30))
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(400))
        let revealed = offset()
        XCTAssertEqual(revealed, 32 * frame + frame - bounds.width, accuracy: 2, "It comes just into view, at the near edge")
        // Scrolled away by hand, it stays until something's wholly out of view.
        model.open(file(29))
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(offset(), revealed, accuracy: 0.5, "A frame in view doesn't move it")
    }

    /// Opening the window on a photograph far along the folder shows its frame.
    func testTheStripOpensWithTheOpenPhotographInView() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        for index in 0..<40 { try FileManager.default.copyItem(at: ramp, to: directory.appendingPathComponent("z\(index).png")) }
        model.open(directory.appendingPathComponent("z30.png"))
        try await waitUntil { model.previewCurrent }
        let (_, window, roll) = try await openRoll(model)
        defer { window.orderOut(nil) }
        let frame = FilmRollLayout.frameLength(stripHeight: FilmRollLayout.defaultStripHeight, aspect: 4.0 / 3)
        let start = 32 * frame
        let visible = roll.contentView.bounds
        XCTAssertLessThan(visible.minX, start + frame, "\(visible)")
        XCTAssertGreaterThan(visible.maxX, start, "\(visible)")
    }

    /// Real drags: the film follows the mouse, coasts on from a flick, stops, and
    /// opens nothing. Holding still before letting go leaves it where it is.
    func testDraggingTheRollScrollsItWithInertia() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        let (_, window, roll) = try await openRoll(model, photos: 40)
        defer { window.orderOut(nil) }
        let source = model.source
        let middle = roll.convert(roll.bounds, to: nil).midY
        func offset() -> CGFloat { roll.contentView.bounds.origin.x }
        roll.contentView.scroll(to: NSPoint(x: 600, y: roll.contentView.bounds.origin.y))
        roll.reflectScrolledClipView(roll.contentView)
        try await Task.sleep(for: .milliseconds(100))
        let start = offset()
        // A flick left.
        var x: CGFloat = 900
        var clock = ProcessInfo.processInfo.systemUptime
        try send(.leftMouseDown, CGPoint(x: x, y: middle), to: window, time: clock)
        for _ in 0..<6 {
            clock += 0.016
            x -= 30
            try send(.leftMouseDragged, CGPoint(x: x, y: middle), to: window, time: clock)
        }
        let moved = offset()
        XCTAssertEqual(moved, start + 180, accuracy: 35, "It followed the pointer")
        try send(.leftMouseUp, CGPoint(x: x, y: middle), to: window, time: clock + 0.005)
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertGreaterThan(offset(), moved + 20, "It coasts on after letting go")
        // A fast flick takes about 3 seconds to fall to the stopping speed.
        try await Task.sleep(for: .seconds(4))
        let rested = offset()
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertEqual(offset(), rested, accuracy: 0.5, "And comes to rest")
        XCTAssertEqual(model.source, source, "Dragging opens nothing")
        // A drag that holds still before letting go.
        clock = ProcessInfo.processInfo.systemUptime
        try send(.leftMouseDown, CGPoint(x: 900, y: middle), to: window, time: clock)
        for step in 1...4 {
            clock += 0.016
            try send(.leftMouseDragged, CGPoint(x: 900 + CGFloat(step) * 25, y: middle), to: window, time: clock)
        }
        let held = offset()
        try send(.leftMouseUp, CGPoint(x: 1000, y: middle), to: window, time: clock + 0.25)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(offset(), held, accuracy: 0.5, "No coast after a pause")
        // A press stops a coast.
        var y: CGFloat = 700
        clock = ProcessInfo.processInfo.systemUptime
        try send(.leftMouseDown, CGPoint(x: y, y: middle), to: window, time: clock)
        for _ in 0..<5 {
            clock += 0.016
            y -= 40
            try send(.leftMouseDragged, CGPoint(x: y, y: middle), to: window, time: clock)
        }
        try send(.leftMouseUp, CGPoint(x: y, y: middle), to: window, time: clock + 0.005)
        try await Task.sleep(for: .milliseconds(80))
        try send(.leftMouseDown, CGPoint(x: y, y: middle), to: window)
        try await Task.sleep(for: .milliseconds(60))
        let pressed = offset()
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(offset(), pressed, accuracy: 0.5, "A press stops it")
        try send(.leftMouseUp, CGPoint(x: y, y: middle), to: window)
    }

    /// The file has the pixels Image Size asks for, not the screen's scale times
    /// them: read back from real exports through both engines.
    func testAnExportHasThePixelsImageSizeAsksFor() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set(ImageSize.targetPath, .number(2000))
        try await waitUntil { model.previewCurrent }
        for engine in ExportEngine.allCases {
            let url = directory.appendingPathComponent("export-\(engine.rawValue).png")
            await model.export(to: url, engine: engine, size: .full, save: SaveSettings())
            let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil), "\(engine)")
            let properties = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
            XCTAssertEqual(properties[kCGImagePropertyPixelWidth] as? Int, 2000, "\(engine)")
            XCTAssertEqual(properties[kCGImagePropertyPixelHeight] as? Int, 1500, "\(engine)")
        }
    }

    /// The DPI in Settings reaches real files, in every format, through both
    /// export engines, and changes none of their pixels.
    func testAnExportCarriesTheDPIFromSettings() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "exportDPI")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "exportDPI")
        }
        model.set(ImageSize.targetPath, .number(1024))
        try await waitUntil { model.previewCurrent }
        model.exportDPI = 144
        XCTAssertEqual(model.exportSaveSettings.dpi, 144)
        for engine in ExportEngine.allCases {
            for ext in ["png", "jpg", "tif"] {
                let url = directory.appendingPathComponent("dpi-\(engine.rawValue).\(ext)")
                var save = model.exportSaveSettings
                save.bitDepth = ext == "tif" ? 16 : 8
                await model.export(to: url, engine: engine, size: .full, save: save)
                let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil), "\(engine) \(ext)")
                let properties = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
                XCTAssertEqual((properties[kCGImagePropertyDPIWidth] as? NSNumber)?.doubleValue ?? 0, 144, accuracy: 0.1, "\(engine) \(ext)")
                XCTAssertEqual((properties[kCGImagePropertyDPIHeight] as? NSNumber)?.doubleValue ?? 0, 144, accuracy: 0.1, "\(engine) \(ext)")
                XCTAssertEqual(properties[kCGImagePropertyPixelWidth] as? Int, 1024, "\(engine) \(ext)")
                XCTAssertEqual(properties[kCGImagePropertyPixelHeight] as? Int, 768, "\(engine) \(ext)")
            }
        }
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

    /// Settings saved for one photograph, written beside `other`.
    private func saved(on model: EditorModel, film: String? = nil, name: String = "other.json", image: String = "other.png") throws -> EditState {
        var state = model.state
        state.params["post.contrast"] = .number(33)
        state.params["post.tone_anchors"] = .array([.object([
            "input": .number(30), "target": .number(50),
            "source": .object(["x": .number(0.5), "y": .number(0.5), "radius": .number(0.04)])
        ])])
        state.crop = CropSettings(rect: CGRect(x: 0.1, y: 0.1, width: 0.5, height: 0.5))
        if let film { state.film = film }
        let data = try JSONEncoder().encode(PhotoSettingsFile(image: image, state: state))
        try data.write(to: directory.appendingPathComponent(name))
        return state
    }

    func testOpeningAPhotographLoadsItsSettings() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        _ = try saved(on: model)
        model.open(other)
        XCTAssertEqual(model.value("post.contrast").double, 33)
        XCTAssertEqual(model.pins.count, 1)
        XCTAssertEqual(model.pins[0].zone, 5)
        XCTAssertEqual(model.state.crop.rect.width, 0.5, accuracy: 1e-9)
        XCTAssertNil(model.error)
        try await waitUntil { model.previewCurrent }
    }

    func testTheFallbackNameLoadsToo() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        _ = try saved(on: model, name: "other.png.json")
        model.open(other)
        XCTAssertEqual(model.pins.count, 1)
    }

    /// What a save writes, an open reads back unchanged.
    func testSavingThenReopeningRestoresTheEdit() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await place(5, on: model)
        model.set("post.contrast", .number(12))
        model.saveSettings()
        let edited = model.state
        model.open(other)
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.pins.isEmpty)
        model.open(ramp)
        XCTAssertEqual(model.state, edited)
        try await waitUntil { model.previewCurrent }
    }

    func testAnotherPhotographsFileIsNotLoaded() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        _ = try saved(on: model, image: "other.nef")
        model.open(other)
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertNil(model.error)
    }

    func testAnUnrelatedJSONFileIsIgnored() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try Data("{\"mine\": true}".utf8).write(to: directory.appendingPathComponent("other.json"))
        model.open(other)
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertNil(model.error)
    }

    /// A damaged file of its own says so, and the photograph still opens.
    func testADamagedFileIsReportedAndThePhotographOpens() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try Data("{\"negsSettings\": 1, \"image\": \"other.png\", \"state\": 7}".utf8).write(to: directory.appendingPathComponent("other.json"))
        model.open(other)
        XCTAssertEqual(model.source, other)
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertNotNil(model.error)
        model.error = nil
        try await waitUntil { model.previewCurrent }
    }

    func testAnUnavailableFilmIsReportedAndThePhotographOpens() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        _ = try saved(on: model, film: "no_such_film")
        model.open(other)
        XCTAssertNotEqual(model.state.film, "no_such_film")
        XCTAssertTrue(model.films.contains { $0.id == model.state.film }, "It opens at the defaults")
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertTrue(try XCTUnwrap(model.error).contains("film"))
    }

    /// Loading is one undo step, back to what opening alone would have given.
    func testLoadingSettingsIsOneUndoStep() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        _ = try saved(on: model)
        model.open(other)
        XCTAssertEqual(undo.undoActionName, "Load Settings")
        undo.undo()
        XCTAssertTrue(model.pins.isEmpty)
        XCTAssertNotEqual(model.value("post.contrast").double, 33)
        try await waitUntil { model.previewCurrent }
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
