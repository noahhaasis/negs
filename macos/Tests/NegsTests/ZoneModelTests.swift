import AppKit
import ImageIO
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

    /// The toolbar's Pins button, in the group with zoom and crop, does what Z does.
    func testTheToolbarButtonTogglesThePins() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "showTonePins")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "showTonePins")
        }
        let controller = EditorWindowController(model: model)
        let toolbar = try XCTUnwrap(controller.window?.toolbar)
        let groups = toolbar.items.compactMap { $0 as? NSToolbarItemGroup }
        let group = try XCTUnwrap(groups.first { $0.subitems.contains { $0.label == "Crop" } })
        let button = try XCTUnwrap(group.subitems.first { $0.label == "Pins" })
        let before = model.pinsVisible
        _ = (button.target as AnyObject).perform(button.action)
        XCTAssertNotEqual(model.pinsVisible, before)
        XCTAssertTrue(model.showsZonePanel)
    }

    /// Through a real window: the proof is the export's size whatever happens,
    /// and nothing changes the window's size or position, which stays the user's.
    func testAProofNeverTouchesTheWindowOrResizesThePhotograph() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "proofMode")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "proofMode")
        }
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        model.proofing = false
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
        model.proofing = true
        try await settle()
        XCTAssertTrue(window.styleMask.contains(.resizable), "The window stays resizable")
        XCTAssertFalse(model.canZoom)
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
        model.proofing = false
        try await settle()
        XCTAssertEqual(window.frame, frame, "Leaving")
    }

    /// A panned proof stays on the same part of the photograph when the preview
    /// changes resolution, as it does during a slider drag.
    func testAProofKeepsItsPlaceWhenThePreviewChangesResolution() async throws {
        let model = try await makeModel()
        let saved = UserDefaults.standard.object(forKey: "proofMode")
        defer {
            model.limitPreview(false)
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "proofMode")
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
        model.proofing = true
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

    /// The roll runs down the window's far left, the zones next to it and the
    /// canvas after them, and a zone's drop still lands where it's let go.
    func testTheFilmRollSitsAtTheFarLeft() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        model.filmstripVisible = true
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        window.setFrame(NSRect(x: 80, y: 80, width: 1300, height: 800), display: true)
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(400))
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let scrolls = all(try XCTUnwrap(window.contentView)).compactMap { $0 as? NSScrollView }
        let canvas = try XCTUnwrap(scrolls.first { $0 is ImageScrollView })
        let left = canvas.convert(canvas.bounds, to: nil).minX
        XCTAssertEqual(left, FilmRollLayout.columnWidth + ZoneGeometry.panelWidth, accuracy: 1)
        let roll = try XCTUnwrap(scrolls.first { abs($0.bounds.width - FilmRollLayout.columnWidth) < 1 }, "\(scrolls.map(\.bounds))")
        XCTAssertEqual(roll.convert(roll.bounds, to: nil).minX, 0, accuracy: 1)
        XCTAssertGreaterThan(roll.bounds.height, 600, "It runs the window's height")
        let film = try XCTUnwrap(roll.documentView).frame.height
        XCTAssertGreaterThanOrEqual(film, roll.bounds.height, "With two photographs the film still runs the window's height")
        model.filmstripVisible = false
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(canvas.convert(canvas.bounds, to: nil).minX, ZoneGeometry.panelWidth, accuracy: 1, "Hidden, the canvas takes its room")
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

    /// Real clicks on the roll: the film around a frame does nothing, the
    /// photograph opens it. Pointing at a frame opens nothing.
    func testOnlyAFramesPhotographOpensIt() async throws {
        let model = try await makeModel()
        let saved = model.filmstripVisible
        defer { model.filmstripVisible = saved; model.shutdown() }
        model.filmstripVisible = true
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        window.setFrame(NSRect(x: 80, y: 80, width: 1300, height: 800), display: true)
        try await waitUntil { model.previewCurrent }
        try await Task.sleep(for: .milliseconds(600))
        XCTAssertEqual(model.source, ramp)
        func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
        let scrolls = all(try XCTUnwrap(window.contentView)).compactMap { $0 as? NSScrollView }
        let roll = try XCTUnwrap(scrolls.first { abs($0.bounds.width - FilmRollLayout.columnWidth) < 1 })
        let document = try XCTUnwrap(roll.documentView)
        // The first frame, other.png, sorts before ramp.png, at the top of the film.
        let frameHeight = FilmRollLayout.frameHeight(stripWidth: FilmRollLayout.stripWidth, texture: FilmRollLayout.defaultTexture)
        let top = document.convert(document.bounds, to: nil).maxY
        func send(_ type: NSEvent.EventType, _ location: CGPoint) throws {
            window.sendEvent(try XCTUnwrap(NSEvent.mouseEvent(with: type, location: location, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                eventNumber: 0, clickCount: 1, pressure: 1)))
        }
        func click(_ location: CGPoint) async throws {
            try send(.leftMouseDown, location)
            try send(.leftMouseUp, location)
            try await Task.sleep(for: .milliseconds(150))
        }
        let middle = top - frameHeight / 2
        try await click(CGPoint(x: 8, y: middle))
        XCTAssertEqual(model.source, ramp, "The sprockets don't open it")
        try await click(CGPoint(x: FilmRollLayout.stripWidth - 8, y: middle))
        XCTAssertEqual(model.source, ramp, "Nor the far edge")
        try send(.mouseMoved, CGPoint(x: FilmRollLayout.stripWidth / 2, y: middle))
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(model.source, ramp, "Pointing at it doesn't")
        try await click(CGPoint(x: FilmRollLayout.stripWidth / 2, y: middle))
        XCTAssertEqual(model.source, other, "The photograph does")
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
        let film = model.state.film
        _ = try saved(on: model, film: "no_such_film")
        model.open(other)
        XCTAssertEqual(model.state.film, film)
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
