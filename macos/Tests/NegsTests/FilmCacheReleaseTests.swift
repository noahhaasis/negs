import AppKit
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
private let data = root.appendingPathComponent("data")

/// Ticket 41: the helper frees its kept film side when the editor goes back
/// to the grid and on memory pressure, and never while editing.
@MainActor final class FilmCacheReleaseTests: XCTestCase {
    private var paths: EnginePaths {
        EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                    exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: data)
    }

    private func fixture(in dir: URL, _ name: String, seed: Int) throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        for i in 0..<(96 * 64 * 3) { bitmap.bitmapData![i] = UInt8((i + seed) % 251) }
        let url = dir.appendingPathComponent(name)
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate())
    }

    private func model() async throws -> EditorModel {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "sidecars"].map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(false, forKey: "autoPlaceNewImages")
        defaults.set(false, forKey: "sidecars")
        let model = EditorModel(sidecars: nil)
        addTeardownBlock { @MainActor in model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        return model
    }

    func testTheGridAndMemoryPressureReleaseTheFilmSideButEditingDoesNot() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("negs-film-cache-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let a = try fixture(in: dir, "a.png", seed: 0), b = try fixture(in: dir, "b.png", seed: 7)
        let model = try await model()
        let release = model.filmCacheRelease

        // Editing and switching photographs send nothing.
        model.open(a)
        try await waitUntil { model.image != nil && !model.rendering }
        model.set("enlarger.print_exposure", .number(1.2))
        try await waitUntil { !model.rendering }
        model.open(b)
        try await waitUntil { model.image != nil && !model.rendering }
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertTrue(release.sent.isEmpty, "nothing is released while editing")

        // Back to the grid: the helper frees the kept film side.
        model.showGrid()
        XCTAssertEqual(model.mode, .grid)
        try await waitUntil { release.sent.count == 1 }
        XCTAssertEqual(release.sent.first?.reason, .editorClosed)
        XCTAssertGreaterThan(release.sent.first?.releasedBytes ?? 0, 0, "the helper had a film side to free")

        // Memory pressure, warning and critical; a normal event is ignored.
        release.memoryPressure(.normal)
        release.memoryPressure(.warning)
        try await waitUntil { release.sent.count == 2 }
        XCTAssertEqual(release.sent.last?.reason, .memoryPressure)
        XCTAssertEqual(release.sent.last?.releasedBytes, 0, "already freed")

        // Back in the editor, a render keeps a film side again, and critical pressure frees it.
        model.editCurrent()
        try await waitUntil { model.mode == .photo && model.image != nil && !model.rendering }
        let shown = model.image
        model.set("enlarger.print_exposure", .number(1.1))
        try await waitUntil { model.image !== shown && !model.rendering }
        XCTAssertEqual(release.sent.count, 2)
        release.memoryPressure(.critical)
        try await waitUntil { release.sent.count == 3 }
        XCTAssertGreaterThan(release.sent.last?.releasedBytes ?? 0, 0)
    }
}
