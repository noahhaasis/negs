import AppKit
import IOSurface
import XCTest
@testable import NegsApp

/// Ticket 39: solved brightness matches kept across photographs and launches.
final class BrightnessMatchCacheTests: XCTestCase {
    private var directory: URL!

    override func setUp() {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-match-cache-test-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    override func tearDown() { try? FileManager.default.removeItem(at: directory) }

    private func file(_ name: String, bytes: Int = 64) throws -> URL {
        let url = directory.appendingPathComponent(name)
        try Data(repeating: 7, count: bytes).write(to: url)
        return url
    }

    private func request(film: String = "kodak_portra_400", exposure: Double = 0) -> EditState {
        var state = EditState()
        state.film = film
        state.params["camera.exposure_compensation_ev"] = .number(exposure)
        state.params["io.output_color_space"] = .string("sRGB")
        return state
    }

    func testAKeyNamesTheFileTheRequestAndTheEngine() throws {
        let photo = try file("a.nef")
        let key = try XCTUnwrap(BrightnessMatchCache.key(file: photo, request: request(), engine: "1"))
        XCTAssertEqual(BrightnessMatchCache.key(file: photo, request: request(), engine: "1"), key, "The same solve")
        XCTAssertNotEqual(BrightnessMatchCache.key(file: photo, request: request(film: "kodak_gold_200"), engine: "1"), key, "Another film")
        XCTAssertNotEqual(BrightnessMatchCache.key(file: photo, request: request(exposure: 0.5), engine: "1"), key, "Another parameter")
        XCTAssertNotEqual(BrightnessMatchCache.key(file: photo, request: request(), engine: "2"), key, "Another engine")
        XCTAssertNotEqual(BrightnessMatchCache.key(file: try file("b.nef"), request: request(), engine: "1"), key, "Another file")
        // The same path, rewritten or touched: solved again.
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-60)], ofItemAtPath: photo.path)
        XCTAssertNotEqual(BrightnessMatchCache.key(file: photo, request: request(), engine: "1"), key, "Modified")
        let modified = try XCTUnwrap(BrightnessMatchCache.key(file: photo, request: request(), engine: "1"))
        let date = try XCTUnwrap(FileManager.default.attributesOfItem(atPath: photo.path)[.modificationDate] as? Date)
        try Data(repeating: 7, count: 65).write(to: photo)
        try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: photo.path)
        XCTAssertNotEqual(BrightnessMatchCache.key(file: photo, request: request(), engine: "1"), modified, "Another size")
        XCTAssertNil(BrightnessMatchCache.key(file: directory.appendingPathComponent("gone.nef"), request: request(), engine: "1"))
    }

    func testSolvesAreKeptOnDiskAndTheLeastRecentlyUsedGo() throws {
        let url = directory.appendingPathComponent("matches/BrightnessMatches.json")
        let cache = BrightnessMatchCache(url: url, limit: 20)
        cache.store("a", ev: 0.37, limited: true)
        XCTAssertEqual(cache.solved("a")?.ev, 0.37)
        XCTAssertEqual(cache.solved("a")?.limited, true)
        XCTAssertNil(cache.solved("b"))
        cache.flush()
        let reread = BrightnessMatchCache(url: url, limit: 20)
        XCTAssertEqual(reread.solved("a")?.ev, 0.37, "Kept across launches")
        XCTAssertEqual(reread.solved("a")?.limited, true)
        for i in 0..<40 {
            reread.store("k\(i)", ev: Double(i), limited: false)
            // Used, so it outlives the others.
            _ = reread.solved("a")
            Thread.sleep(forTimeInterval: 0.002)
        }
        XCTAssertLessThanOrEqual(reread.count, 20)
        XCTAssertEqual(reread.solved("a")?.ev, 0.37)
        XCTAssertEqual(reread.solved("k39")?.ev, 39)
        XCTAssertNil(reread.solved("k0"))
    }
}

/// The editor finds a solved match instead of solving it again, and a hit
/// is what solving gives.
@MainActor final class BrightnessMatchCacheLifecycleTests: XCTestCase {
    private var models: [EditorModel] = []
    private var restoreDefaults: [() -> Void] = []
    private var directory: URL!

    override func setUp() async throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-match-cache-model-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    override func tearDown() async throws {
        for model in models { model.shutdown() }
        models = []
        for restore in restoreDefaults { restore() }
        restoreDefaults = []
        try? FileManager.default.removeItem(at: directory)
    }

    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    /// A textured 1200 × 800 photograph; `seed` makes it another one.
    private func photo(_ name: String, seed: Int) throws -> URL {
        let (width, height) = (1200, 800)
        let url = directory.appendingPathComponent(name)
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: width * 3, bitsPerPixel: 24)!
        for y in 0..<height {
            for x in 0..<width {
                let i = (y * width + x) * 3
                bitmap.bitmapData![i] = UInt8((x * 255) / width)
                bitmap.bitmapData![i + 1] = UInt8(((y + seed * 40) * 255 / (height + seed * 40)) % 256)
                bitmap.bitmapData![i + 2] = UInt8(((x / (30 + seed) + y / 30) % 2) * 150 + 30 + seed * 20)
            }
        }
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    private func model(cache: BrightnessMatchCache) async -> EditorModel {
        let model = EditorModel()
        models.append(model)
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages"].map { ($0, defaults.object(forKey: $0)) }
        restoreDefaults.append { for (key, value) in saved { defaults.set(value, forKey: key) } }
        model.autoPlaceNewImages = true
        model.matchCache = cache
        await model.start(paths: paths(), restoration: Task { nil })
        return model
    }

    private func open(_ photo: URL, in model: EditorModel) async throws {
        model.open(photo)
        try await settle(model)
    }

    private func settle(_ model: EditorModel) async throws {
        let deadline = Date().addingTimeInterval(60)
        while !(model.error != nil || model.previewCurrent && !model.matchingBrightness && !model.brightnessMatchPending), Date() < deadline {
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertNil(model.error)
        XCTAssertTrue(model.previewCurrent)
    }

    private func bytes(_ surface: IOSurface?) throws -> Data {
        let surface = try XCTUnwrap(surface)
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        return Data(bytes: surface.baseAddress, count: surface.bytesPerRow * surface.height)
    }

    /// A → B → A solves A once; the second open finds it, with the same
    /// state and the same frame as solving it again in a fresh cache would.
    func testGoingBackToAPhotographFindsItsMatch() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build release helper first") }
        let a = try photo("a.png", seed: 0), b = try photo("b.png", seed: 1)
        let cache = BrightnessMatchCache(url: directory.appendingPathComponent("cache/BrightnessMatches.json"))
        let model = await model(cache: cache)
        try await open(a, in: model)
        guard model.brightnessMatchActive else { throw XCTSkip("The test photo can't be matched: \(model.brightnessMatchNote ?? "")") }
        let solvedState = model.state, solvedFrame = try bytes(model.image)
        XCTAssertEqual(model.matchSolves, 1)
        try await open(b, in: model)
        XCTAssertEqual(model.matchSolves, 2)
        try await open(a, in: model)
        XCTAssertEqual(model.matchSolves, 2, "Found, not solved again")
        XCTAssertEqual(model.state, solvedState)
        XCTAssertEqual(try bytes(model.image), solvedFrame)
        XCTAssertNil(model.brightnessMatchNote?.range(of: "Matching"))

        // A fresh cache solves A again: the same state.
        let fresh = await self.model(cache: BrightnessMatchCache(url: directory.appendingPathComponent("fresh/BrightnessMatches.json")))
        try await open(a, in: fresh)
        XCTAssertEqual(fresh.matchSolves, 1)
        XCTAssertEqual(fresh.state, solvedState, "A hit is what solving gives")

        // Kept across launches.
        cache.flush()
        let relaunched = await self.model(cache: BrightnessMatchCache(url: cache.url))
        try await open(a, in: relaunched)
        XCTAssertEqual(relaunched.matchSolves, 0)
        XCTAssertEqual(relaunched.state, solvedState)

        // Another film is another request; going back to the first finds it.
        let film = model.film
        let other = model.films.first { $0.id != film && !$0.positive }?.id
        let otherFilm = try XCTUnwrap(other)
        model.chooseFilm(otherFilm)
        try await settle(model)
        let afterOther = model.matchSolves
        XCTAssertEqual(afterOther, 3)
        model.chooseFilm(film)
        try await settle(model)
        XCTAssertEqual(model.matchSolves, afterOther, "The first film's match is found")
        XCTAssertEqual(model.state.brightnessMatch, solvedState.brightnessMatch)
        XCTAssertEqual(model.value("prep.exposure_ev"), solvedState.params["prep.exposure_ev"])

        // A modified file is solved again.
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-3600)], ofItemAtPath: a.path)
        try await open(b, in: model)
        let before = model.matchSolves
        try await open(a, in: model)
        XCTAssertEqual(model.matchSolves, before + 1, "The file changed")
    }
}
