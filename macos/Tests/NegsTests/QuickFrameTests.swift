import AppKit
import ImageIO
import IOSurface
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

/// Ticket 28: a discrete edit shows a canvas-size frame first, then the
/// export-size one.
final class QuickFrameStepTests: XCTestCase {
    private func step(interacting: Bool = false, scale: CGFloat? = 0.25, pixels: Double = 24e6, hasFrame: Bool = true,
                      done: Bool = false, reuses: Bool = false) -> EditorModel.RenderStep {
        EditorModel.renderStep(interacting: interacting, screenScale: scale, framePixels: pixels,
                               hasFrame: hasFrame, quickFrameDone: done, reusesHelperOutput: reuses)
    }

    func testDiscreteEditsOfLargePhotosRenderAQuickFrameFirst() {
        XCTAssertEqual(step(), .quick(0.25))
        XCTAssertEqual(step(done: true), .settled, "Its quick frame is done: settle")
        XCTAssertEqual(step(hasFrame: false), .settled, "A new photograph has no frame to replace yet")
        XCTAssertEqual(step(reuses: true), .settled, "Post-only edits reuse the helper's output")
        XCTAssertEqual(step(pixels: 4e6), .settled, "Small photographs render fast enough")
        XCTAssertEqual(step(scale: 0.8), .settled, "The canvas shows most pixels anyway")
        XCTAssertEqual(step(scale: 1), .settled, "Zoomed in")
        XCTAssertEqual(step(scale: nil), .settled, "No canvas")
    }

    func testDragsAreUnchanged() {
        XCTAssertEqual(step(interacting: true), .drag(0.25))
        XCTAssertEqual(step(interacting: true, pixels: 1e6, hasFrame: false, done: true, reuses: true), .drag(0.25))
        XCTAssertEqual(step(interacting: true, scale: 1), .settled)
    }

    func testReuseKeyIgnoresOnlyPostAndThePreviewPath() {
        let command: [String: JSONValue] = [
            "command": .string("render"), "film": .string("a"), "preview_path": .string("/x"),
            "params": .object(["post": .object(["contrast": .number(0)]), "camera": .object(["exposure_compensation_ev": .number(0)])])
        ]
        var post = command
        post["params"]?["post.contrast"] = .number(20); post["preview_path"] = .string("/y")
        XCTAssertEqual(EditorModel.reuseKey(command), EditorModel.reuseKey(post))
        var camera = command
        camera["params"]?["camera.exposure_compensation_ev"] = .number(1)
        XCTAssertNotEqual(EditorModel.reuseKey(command), EditorModel.reuseKey(camera))
        var scaled = command
        scaled["interactive_scale"] = .number(0.25)
        XCTAssertNotEqual(EditorModel.reuseKey(command), EditorModel.reuseKey(scaled))
    }
}

@MainActor final class QuickFrameLifecycleTests: XCTestCase {
    private static var photo: URL?
    private var models: [EditorModel] = []
    private var restoreDefaults: [() -> Void] = []

    override func tearDown() async throws {
        for model in models { model.shutdown() }
        models = []
        for restore in restoreDefaults { restore() }
        restoreDefaults = []
    }

    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    /// A 6 MP textured photograph: above the quick frame's minimum size.
    private func testPhoto() throws -> URL {
        if let photo = Self.photo { return photo }
        let (width, height) = (3000, 2000)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-quick-frame-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("photo.png")
        var pixels = [UInt8](repeating: 255, count: width * height * 4)
        for y in 0..<height {
            for x in 0..<width {
                let i = (y * width + x) * 4
                pixels[i] = UInt8((x * 255) / width)
                pixels[i + 1] = UInt8((y * 255) / height)
                pixels[i + 2] = UInt8(((x / 40 + y / 40) % 2) * 160 + 40)
            }
        }
        let context = try XCTUnwrap(CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        Self.photo = url
        return url
    }

    /// A model with the photograph open and its first frame settled. The
    /// canvas shows a quarter of the photo's pixels per side, or isn't
    /// there at all (`scale` nil: no quick frames, as before ticket 28).
    private func openModel(scale: CGFloat?, match: Bool = false) async throws -> EditorModel {
        let model = EditorModel()
        models.append(model)
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "freshEditsForNewPhotos"].map { ($0, defaults.object(forKey: $0)) }
        restoreDefaults.append { for (key, value) in saved { defaults.set(value, forKey: key) } }
        model.autoPlaceNewImages = match
        model.freshEditsForNewPhotos = true
        model.canvasScale.fixed = scale
        await model.start(paths: paths(), restoration: Task { nil })
        model.open(try testPhoto())
        try await settled(model)
        return model
    }

    private func settled(_ model: EditorModel) async throws {
        try await waitUntil { model.error != nil || model.previewCurrent && !model.matchingBrightness && !model.brightnessMatchPending }
        XCTAssertNil(model.error)
    }

    private func waitUntil(timeout: TimeInterval = 60, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(2)) }
        XCTAssertTrue(predicate(), "Timed out")
    }

    private func bytes(_ surface: IOSurface?) throws -> Data {
        let surface = try XCTUnwrap(surface)
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        return Data(bytes: surface.baseAddress, count: surface.bytesPerRow * surface.height)
    }

    /// Times film clicks on a real photograph, with and without quick
    /// frames, alternating: time to the first visible change and to the
    /// settled frame. NEGS_QUICK_FRAME_BENCH is the photograph,
    /// NEGS_QUICK_FRAME_SCALE the canvas's screen pixels per photo pixel
    /// (default: a 1960 px long edge), NEGS_QUICK_FRAME_MATCH=1 matches
    /// brightness.
    func testFilmClickTimings() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let path = environment["NEGS_QUICK_FRAME_BENCH"] else { throw XCTSkip("Set NEGS_QUICK_FRAME_BENCH") }
        Self.photo = URL(fileURLWithPath: path)
        defer { Self.photo = nil }
        let match = environment["NEGS_QUICK_FRAME_MATCH"] == "1"
        let probe = try await openModel(scale: nil, match: match)
        let longEdge = max(probe.imageSize.width, probe.imageSize.height)
        let scale = environment["NEGS_QUICK_FRAME_SCALE"].flatMap(Double.init).map { CGFloat($0) } ?? 1960 / longEdge
        probe.shutdown()
        let quick = try await openModel(scale: scale, match: match), direct = try await openModel(scale: nil, match: match)
        let films = ["kodak_portra_400", "kodak_ektar_100", "kodak_portra_160", "kodak_ultramax_400"]
        var results: [String: [(first: Double, settled: Double)]] = [:]
        for round in 0..<6 {
            for (name, model) in [("quick", quick), ("direct", direct)] {
                let film = films.filter { $0 != model.film }[round % 3]
                let before = model.image, clock = ContinuousClock.now
                model.chooseFilm(film)
                var first: Duration?
                while !(model.previewCurrent && !model.brightnessMatchPending && !model.matchingBrightness) {
                    if first == nil, model.image !== before { first = ContinuousClock.now - clock }
                    try await Task.sleep(for: .milliseconds(1))
                }
                let settled = ContinuousClock.now - clock
                // The first round warms up the film's profile.
                if round > 0 { results[name, default: []].append(((first ?? settled) / .milliseconds(1), settled / .milliseconds(1))) }
                try await Task.sleep(for: .milliseconds(300))
            }
        }
        // Time to read both helpers' footprints (`footprint -p`), quick first.
        if let seconds = environment["NEGS_QUICK_FRAME_HOLD"].flatMap(Double.init) {
            print("QUICK-FRAME holding \(seconds) s")
            try await Task.sleep(for: .seconds(seconds))
        }
        for (name, runs) in results.sorted(by: { $0.key < $1.key }) {
            let median = { (values: [Double]) in values.sorted()[values.count / 2] }
            print("QUICK-FRAME \(name) \(Int(longEdge)) px scale \(String(format: "%.3f", scale)) match \(match): first \(Int(median(runs.map(\.first)))) ms, settled \(Int(median(runs.map(\.settled)))) ms; runs \(runs.map { "\(Int($0.first))/\(Int($0.settled))" })")
        }
    }

    /// A colour negative other than the current film.
    private func otherFilm(_ model: EditorModel) -> String {
        let id = model.film == "kodak_portra_400" ? "kodak_ektar_100" : "kodak_portra_400"
        XCTAssertTrue(model.films.contains { $0.id == id })
        return id
    }

    private func steps(_ model: EditorModel, after start: Int) -> [EditorModel.RenderStep] {
        Array(model.renderLog.dropFirst(start))
    }

    /// A film click shows a quick frame marked Preview, then the
    /// export-size frame, identical to the one rendered without the quick
    /// frame.
    func testFilmClickShowsAQuickFrameThenTheSameSettledFrame() async throws {
        let quick = try await openModel(scale: 0.25)
        let start = quick.renderLog.count
        let before = quick.image
        quick.chooseFilm(otherFilm(quick))
        try await waitUntil { quick.image !== before }
        XCTAssertFalse(quick.previewCurrent)
        XCTAssertFalse(quick.canSave)
        XCTAssertTrue(quick.status.hasPrefix("Preview"), quick.status)
        XCTAssertLessThan(try XCTUnwrap(quick.image).width, 1000, "Canvas-size frame")
        try await settled(quick)
        XCTAssertEqual(steps(quick, after: start), [.quick(0.25), .settled])
        XCTAssertFalse(quick.status.hasPrefix("Preview"), quick.status)
        XCTAssertEqual(try XCTUnwrap(quick.image).width, 3000)

        let direct = try await openModel(scale: nil)
        let directStart = direct.renderLog.count
        direct.chooseFilm(otherFilm(direct))
        try await settled(direct)
        XCTAssertEqual(steps(direct, after: directStart), [.settled])
        XCTAssertEqual(try bytes(quick.image), try bytes(direct.image), "The settled frame is byte-identical")
    }

    /// Clicking through films while full renders run: each edit renders at
    /// most a quick and a full frame, superseded ones aren't shown, and the
    /// last film settles.
    func testRapidFilmSwitchingDoesNotPileUpRenders() async throws {
        let model = try await openModel(scale: 0.25)
        let start = model.renderLog.count
        let films = ["kodak_portra_160", "kodak_ektar_100", "kodak_portra_400", "kodak_ultramax_400", "kodak_portra_800"]
            .filter { id in id != model.film && model.films.contains { $0.id == id } }
        XCTAssertGreaterThan(films.count, 2)
        for film in films {
            model.chooseFilm(film)
            try await Task.sleep(for: .milliseconds(150))
        }
        try await settled(model)
        let log = steps(model, after: start)
        XCTAssertLessThanOrEqual(log.count, 2 * films.count, "\(log)")
        XCTAssertLessThan(log.filter { $0 == .settled }.count, films.count, "Superseded edits skip their full render: \(log)")
        XCTAssertEqual(log.last, .settled)
        XCTAssertEqual(model.film, films.last)
        XCTAssertFalse(model.status.hasPrefix("Preview"), model.status)
    }

    /// Undo right after a quick frame settles the restored film.
    func testUndoBeforeTheSettledFrameSettlesTheRestoredFilm() async throws {
        let model = try await openModel(scale: 0.25)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        let film = model.film, start = model.renderLog.count
        undo.beginUndoGrouping(); model.chooseFilm(otherFilm(model)); undo.endUndoGrouping()
        try await waitUntil { model.status.hasPrefix("Preview") }
        undo.undo()
        XCTAssertEqual(model.film, film)
        try await settled(model)
        let log = steps(model, after: start)
        XCTAssertLessThanOrEqual(log.filter { $0 == .settled }.count, 2, "\(log)")
        XCTAssertEqual(log.last, .settled)
        XCTAssertEqual(model.film, film)
        XCTAssertTrue(model.previewCurrent)
    }

    /// Post-only edits keep reusing the helper's output: no quick frame,
    /// also right after a film change settled.
    func testPostOnlyEditsSkipTheQuickFrame() async throws {
        let model = try await openModel(scale: 0.25)
        var start = model.renderLog.count
        model.set("post.contrast", .number(20))
        try await settled(model)
        XCTAssertEqual(steps(model, after: start), [.settled])
        model.chooseFilm(otherFilm(model))
        try await settled(model)
        start = model.renderLog.count
        model.set("post.contrast", .number(-20))
        try await settled(model)
        XCTAssertEqual(steps(model, after: start), [.settled])
    }

    /// An edit the helper refuses fails its quick frame quietly, then
    /// reports the export-size render's error once, without retrying.
    func testAFailingEditReportsOnceAndStops() async throws {
        let model = try await openModel(scale: 0.25)
        let start = model.renderLog.count
        XCTAssertNotEqual(model.value("io.upscale_factor"), .null)
        model.set("io.upscale_factor", .number(5))
        try await waitUntil { model.renderFailed }
        try await Task.sleep(for: .milliseconds(1500))
        XCTAssertEqual(steps(model, after: start), [.quick(0.25), .settled])
        XCTAssertNotNil(model.error)
        model.error = nil
    }

    /// A slider grabbed right after a quick frame: letting go goes straight
    /// to export size, as after any drag.
    func testADragRightAfterAQuickFrameSettlesOnRelease() async throws {
        let model = try await openModel(scale: 0.25)
        let start = model.renderLog.count
        model.chooseFilm(otherFilm(model))
        try await waitUntil { model.status.hasPrefix("Preview") }
        model.beginInteraction()
        model.set("camera.exposure_compensation_ev", .number(0.5))
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertFalse(model.previewCurrent)
        model.endInteraction()
        try await settled(model)
        let log = steps(model, after: start)
        XCTAssertEqual(log.first, .quick(0.25), "\(log)")
        XCTAssertEqual(log.last, .settled)
        XCTAssertEqual(log.lastIndex(of: .quick(0.25)), 0, "No quick frame after the drag: \(log)")
    }

    /// With the brightness match on, the match runs on the quick frame's
    /// decode, before any export-size render: one settled render, of the
    /// matched exposure.
    /// Letting go of a slider renders at export size, without a quick frame.
    func testADragSettlesWithoutAQuickFrame() async throws {
        let model = try await openModel(scale: 0.25)
        let start = model.renderLog.count
        model.beginInteraction()
        model.set("camera.exposure_compensation_ev", .number(0.5))
        try await waitUntil { model.status.hasPrefix("Preview") }
        model.set("camera.exposure_compensation_ev", .number(0.7))
        model.endInteraction()
        try await settled(model)
        let log = steps(model, after: start)
        XCTAssertFalse(log.contains(.quick(0.25)), "\(log)")
        XCTAssertEqual(log.last, .settled)
    }

    func testTheMatchRunsBeforeTheSettledRender() async throws {
        let model = try await openModel(scale: 0.25, match: true)
        guard model.brightnessMatchActive else { throw XCTSkip("The test photo can't be matched: \(model.brightnessMatchNote ?? "")") }
        let start = model.renderLog.count
        model.choosePaper("kodak_2302")
        try await settled(model)
        let log = steps(model, after: start)
        XCTAssertEqual(log.first, .quick(0.25), "\(log)")
        XCTAssertEqual(log.filter { $0 == .settled }.count, 1, "\(log)")
        XCTAssertEqual(log.last, .settled)
        XCTAssertEqual(model.state.brightnessMatch?.inputs, model.matchInputs(model.state))
    }

    /// Turning the match off while a frame waits for it shows that frame
    /// once the solve in flight ends, and settles.
    func testTurningTheMatchOffReleasesAHeldFrame() async throws {
        let model = try await openModel(scale: 0.25, match: true)
        guard model.brightnessMatchActive else { throw XCTSkip("The test photo can't be matched") }
        model.choosePaper("kodak_2302")
        try await waitUntil(timeout: 20) { model.holdingFrameForMatch || model.previewCurrent }
        guard model.holdingFrameForMatch else { throw XCTSkip("The match finished before the held frame was seen") }
        model.setMatchesBrightness(false)
        try await waitUntil(timeout: 30) { model.previewCurrent }
        XCTAssertTrue(model.previewCurrent)
    }

    /// Matching turned on while a quick frame shows, before its settled
    /// render starts (at exposure 0 that's no new edit): the settled render
    /// waits for the solve, so the old exposure never renders at export size.
    func testMatchingTurnedOnBeforeTheSettledRenderWaitsForTheSolve() async throws {
        let model = try await openModel(scale: 0.25)
        XCTAssertEqual(model.value("prep.exposure_ev").double, 0)
        let start = model.renderLog.count
        var turnedOn = false
        // The settled render is scheduled and about to start: the window.
        model.willRender = { [unowned model] in
            guard !turnedOn, model.status.hasPrefix("Preview") else { return }
            turnedOn = true
            model.setMatchesBrightness(true)
        }
        model.chooseFilm(otherFilm(model))
        try await waitUntil { turnedOn }
        try await settled(model)
        model.willRender = nil
        guard model.brightnessMatchActive, abs(model.value("prep.exposure_ev").double) >= 0.05 else {
            throw XCTSkip("The match doesn't move the test photo's exposure: \(model.brightnessMatchNote ?? "")")
        }
        let log = steps(model, after: start)
        XCTAssertEqual(log.filter { $0 == .settled }.count, 1, "Only the matched exposure renders at export size: \(log)")
        XCTAssertEqual(log.last, .settled)
        XCTAssertEqual(model.state.brightnessMatch?.inputs, model.matchInputs(model.state))
        XCTAssertTrue(model.previewCurrent)
    }

    /// A relaunched helper keeps no output: a post-only edit after it shows
    /// a quick frame first, as any edit without kept output does.
    func testARelaunchedHelperKeepsNoOutput() async throws {
        let model = try await openModel(scale: 0.25)
        try await waitUntil { !model.previews.busy }
        model.killHelperForTesting()
        let start = model.renderLog.count
        model.set("post.contrast", .number(20))
        try await settled(model)
        XCTAssertEqual(steps(model, after: start), [.quick(0.25), .settled])
    }
}
