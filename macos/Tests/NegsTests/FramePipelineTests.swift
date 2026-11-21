import AppKit
import ImageIO
import IOSurface
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

/// The canvas's frame pipeline against the real helper, checked with the
/// invariants of `macos/Specs/FramePipeline.tla`: regressions for what the
/// model checker found, and seeded random sequences of user actions.
@MainActor final class FramePipelineTests: XCTestCase {
    private static var photos: [URL] = []
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

    /// Two photographs above the quick frame's minimum size and one below it.
    private func testPhotos() throws -> [URL] {
        if !Self.photos.isEmpty { return Self.photos }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-frame-pipeline-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let sizes: [(Int, Int, Int)] = [(3000, 2000, 40), (2400, 2400, 25), (1200, 800, 60)]
        Self.photos = try sizes.enumerated().map { (index: Int, size: (Int, Int, Int)) throws -> URL in
            let (width, height, tile) = size
            let url = directory.appendingPathComponent("photo-\(index).png")
            var pixels = [UInt8](repeating: 255, count: width * height * 4)
            for y in 0..<height {
                for x in 0..<width {
                    let i = (y * width + x) * 4
                    pixels[i] = UInt8((x * 255) / width)
                    pixels[i + 1] = UInt8((y * 255) / height)
                    let check: Int = (x / tile + y / tile) % 2
                    pixels[i + 2] = UInt8(check * 160 + 40 * index)
                }
            }
            let context = try XCTUnwrap(CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
            return url
        }
        return Self.photos
    }

    private func makeModel(scale: CGFloat?, match: Bool) async throws -> EditorModel {
        let model = EditorModel()
        models.append(model)
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "recentFiles"].map { ($0, defaults.object(forKey: $0)) }
        restoreDefaults.append { for (key, value) in saved { defaults.set(value, forKey: key) } }
        model.autoPlaceNewImages = match
        model.canvasScale.fixed = scale
        // Its own matches: a solve, not another test's cached result.
        model.matchCache = BrightnessMatchCache(url: FileManager.default.temporaryDirectory
            .appendingPathComponent("negs-frame-pipeline-matches-\(UUID().uuidString).json"))
        await model.start(paths: paths(), restoration: Task { nil })
        return model
    }

    private func waitUntil(timeout: TimeInterval = 60, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(2)) }
    }

    // MARK: The model's invariants and liveness

    /// `Quiet` in the spec: nothing runs or waits, and the current edit's
    /// settled frame shows (or its error), with no match left pending.
    private func quiet(_ model: EditorModel) -> Bool {
        !model.rendering && !model.renderScheduled && !model.matchingBrightness && !model.holdingFrameForMatch
            && (model.renderFailed || model.previewCurrent && (!model.brightnessMatchPending || model.matchDecodeFailedHere))
    }

    private func describe(_ model: EditorModel) -> String {
        "rendering \(model.rendering), scheduled \(model.renderScheduled), matching \(model.matchingBrightness), "
            + "held \(model.holdingFrameForMatch), current \(model.previewCurrent), failed \(model.renderFailed), "
            + "pending \(model.brightnessMatchPending), decode failed \(model.matchDecodeFailedHere), "
            + "interacting \(model.interacting), cropping \(model.cropping), image \(model.image != nil), "
            + "shown \(String(describing: model.shownFrame)), now \(model.currentFrame)"
    }

    /// The spec's safety invariants, as far as the model shows them.
    private func violations(_ model: EditorModel) -> [String] {
        var found: [String] = []
        let now = model.currentFrame
        if model.image != nil, let shown = model.shownFrame, shown.frame.generation != now.generation {
            found.append("ImageOfThisPhoto")
        }
        if model.image != nil, model.placeholder != nil || model.standIn != nil { found.append("NoPlaceholderOverFrame") }
        if model.previewCurrent, model.shownFrame.map({ $0.frame == now && $0.step == .settled }) != true {
            found.append("PreviewCurrentIsTrue")
        }
        if model.holdingFrameForMatch, !model.matchingBrightness { found.append("HeldHasSolve") }
        return found
    }

    // MARK: Regressions

    /// Model checker: a slider the match depends on, dragged where the
    /// canvas shows every pixel (no drag frames, so each frame is settled),
    /// left the match pending on release, so the exposure never followed and
    /// Save waited out its 30 s.
    func testADragAtFullScaleSolvesTheMatchOnRelease() async throws {
        let model = try await makeModel(scale: nil, match: true)
        model.open(try testPhotos()[0])
        try await waitUntil { self.quiet(model) }
        guard model.brightnessMatchActive else { throw XCTSkip("The test photo can't be matched: \(model.brightnessMatchNote ?? "")") }
        model.beginInteraction()
        model.set("camera.exposure_compensation_ev", .number(0.7))
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.brightnessMatchPending, "The drag changed what the match depends on")
        model.endInteraction()
        try await waitUntil(timeout: 20) { self.quiet(model) }
        XCTAssertTrue(quiet(model), describe(model))
        XCTAssertFalse(model.brightnessMatchPending)
    }

    /// Model checker: a match whose decode failed left the edit's frame held
    /// for a solve that never came, though the render itself succeeded.
    func testAFrameIsNotHeldForAMatchWhoseDecodeFailed() async throws {
        let model = try await makeModel(scale: nil, match: true)
        model.open(try testPhotos()[0])
        try await waitUntil { self.quiet(model) }
        guard model.brightnessMatchActive else { throw XCTSkip("The test photo can't be matched") }
        var failed = false
        model.matchDecodeWillRun = {
            guard !failed else { return }
            failed = true
            throw EngineError(message: "decode failed for the test")
        }
        model.set("camera.exposure_compensation_ev", .number(0.7))
        try await waitUntil(timeout: 20) { failed && self.quiet(model) }
        XCTAssertTrue(failed)
        XCTAssertTrue(quiet(model), describe(model))
        XCTAssertFalse(model.holdingFrameForMatch)
        XCTAssertEqual(model.shownFrame?.frame, model.currentFrame)
    }

    /// Random sequences: an edit the match depends on, made while cropping
    /// (no hold, the frames show), left the match pending when the crop was
    /// cancelled.
    func testCancellingACropSolvesTheMatchAnEditMadeStale() async throws {
        let model = try await makeModel(scale: nil, match: true)
        model.open(try testPhotos()[0])
        try await waitUntil { self.quiet(model) }
        guard model.brightnessMatchActive else { throw XCTSkip("The test photo can't be matched") }
        model.beginCrop()
        XCTAssertTrue(model.cropping)
        model.set("camera.exposure_compensation_ev", .number(0.7))
        try await waitUntil { model.previewCurrent }
        XCTAssertTrue(model.brightnessMatchPending)
        model.endCrop(apply: false)
        try await waitUntil(timeout: 20) { self.quiet(model) }
        XCTAssertTrue(quiet(model), describe(model))
        XCTAssertFalse(model.brightnessMatchPending)
    }

    // MARK: Random sequences

    /// SplitMix64: the same seed gives the same actions. The helper's timing
    /// isn't seeded, so a seed replays the actions, not the interleaving.
    private struct Random {
        var state: UInt64
        mutating func next() -> UInt64 {
            state &+= 0x9E37_79B9_7F4A_7C15
            var z = state
            z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
            z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
            return z ^ (z >> 31)
        }
        mutating func below(_ n: Int) -> Int { Int(next() % UInt64(n)) }
        mutating func chance(_ p: Double) -> Bool { Double(next() % 10_000) / 10_000 < p }
        mutating func pick<T>(_ items: [T]) -> T { items[below(items.count)] }
    }

    /// Random user actions with random pauses between them, checking the
    /// invariants all along, and that the canvas settles each time the user
    /// stops. NEGS_PIPELINE_SEEDS (default 3) runs, NEGS_PIPELINE_STEPS
    /// (default 40) actions each, NEGS_PIPELINE_SEED a single seed.
    func testRandomActionSequencesKeepTheInvariantsAndSettle() async throws {
        let environment = ProcessInfo.processInfo.environment
        let seeds: [UInt64] = environment["NEGS_PIPELINE_SEED"].flatMap(UInt64.init).map { [$0] }
            ?? Array(1...UInt64(environment["NEGS_PIPELINE_SEEDS"].flatMap(Int.init) ?? 3))
        let steps = environment["NEGS_PIPELINE_STEPS"].flatMap(Int.init) ?? 40
        for seed in seeds {
            try await runSequence(seed: seed, steps: steps)
            for model in models { model.shutdown() }
            models = []
        }
    }

    private func runSequence(seed: UInt64, steps: Int) async throws {
        var random = Random(state: seed)
        let photos = try testPhotos()
        let model = try await makeModel(scale: random.pick([nil, 0.25, 1]), match: random.chance(0.5))
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        var log: [String] = []
        var failures: [String] = []

        func check(_ when: String) {
            for violation in violations(model) where failures.count < 5 {
                failures.append("seed \(seed): \(violation) \(when)\n  \(describe(model))\n  after \(log.suffix(8))")
            }
        }
        /// Polls the invariants while time passes.
        func pause(_ milliseconds: Int) async throws {
            let deadline = Date().addingTimeInterval(Double(milliseconds) / 1000)
            repeat { try await Task.sleep(for: .milliseconds(1)); check("while waiting") } while Date() < deadline
        }
        /// The user stops: the canvas settles (`Settles` in the spec).
        func settle() async throws {
            let deadline = Date().addingTimeInterval(30)
            while !quiet(model), Date() < deadline { try await Task.sleep(for: .milliseconds(2)); check("while settling") }
            if !quiet(model), failures.count < 5 {
                failures.append("seed \(seed): never settled\n  \(describe(model))\n  after \(log.suffix(8))")
            }
        }
        /// An undoable action, as one undo group.
        func act(_ name: String, _ action: () -> Void) {
            log.append(name)
            undo.beginUndoGrouping(); action(); undo.endUndoGrouping()
            check("after \(name)")
        }

        model.open(photos[0]); log.append("open 0")
        try await settle()
        let films = ["kodak_portra_400", "kodak_ektar_100", "kodak_gold_200", "fujifilm_c200"].filter { id in model.films.contains { $0.id == id } }
        for _ in 0..<steps where failures.isEmpty {
            switch random.below(14) {
            case 0, 1:
                let film = random.pick(films.filter { $0 != model.film })
                act("film \(film)") { model.chooseFilm(film) }
            case 2:
                let ev = Double(random.below(9) - 4) * 0.25
                act("prep exposure \(ev)") { model.exposureOffset.wrappedValue = ev }
            case 3:
                let ev = Double(random.below(9) - 4) * 0.25
                act("camera exposure \(ev)") { model.set("camera.exposure_compensation_ev", .number(ev)) }
            case 4:
                let contrast = Double(random.below(41) - 20)
                act("post contrast \(contrast)") { model.set("post.contrast", .number(contrast)) }
            case 5:
                let on = !model.matchesBrightness
                act("match \(on)") { model.setMatchesBrightness(on) }
            case 6, 7:
                // A drag: some moves, maybe none (a press), then the release.
                let path = random.pick(["camera.exposure_compensation_ev", "post.contrast", "prep.exposure_ev"])
                let moves = random.below(5)
                log.append("drag \(path) ×\(moves)")
                model.beginInteraction(); check("on press")
                for _ in 0..<moves {
                    model.set(path, .number(Double(random.below(9) - 4) * (path == "post.contrast" ? 5 : 0.25)))
                    try await pause(random.below(60))
                }
                if random.chance(0.3) { try await pause(random.below(400)) }
                undo.beginUndoGrouping(); model.endInteraction(); undo.endUndoGrouping()
                check("on release")
            case 8:
                if undo.canUndo { log.append("undo"); undo.undo(); check("after undo") }
            case 9:
                let photo = random.below(photos.count)
                log.append("open \(photo)")
                model.open(photos[photo]); check("after open")
            case 10:
                guard model.image != nil else { continue }
                log.append("crop")
                model.beginCrop(); check("on crop")
                if random.chance(0.5) {
                    let ev = Double(random.below(9) - 4) * 0.25
                    act("camera exposure \(ev) while cropping") { model.set("camera.exposure_compensation_ev", .number(ev)) }
                }
                try await pause(random.below(200))
                model.endCrop(apply: false); check("after crop")
            case 11:
                let scale = random.pick([nil, 0.25, 0.5, 1] as [CGFloat?])
                log.append("scale \(String(describing: scale))")
                model.canvasScale.fixed = scale
            case 12:
                if random.chance(0.3) { log.append("kill helper"); model.killHelperForTesting() }
                else if model.renderFailed { log.append("retry"); model.retryDevelop() }
            default:
                break
            }
            // The pause before the next action: none, a moment, or until it settles.
            switch random.below(4) {
            case 0: await Task.yield()
            case 1, 2: try await pause(random.below(250))
            default: log.append("settle"); try await settle()
            }
        }
        if failures.isEmpty { try await settle() }
        XCTAssertTrue(failures.isEmpty, failures.joined(separator: "\n"))
        print("FRAME-PIPELINE seed \(seed): \(log.count) actions, \(model.renderLog.count) renders\(failures.isEmpty ? "" : ", FAILED")")
    }
}
