import AppKit
import XCTest
@testable import NegsApp

@MainActor final class ImageSizeZoomTests: XCTestCase {
    /// Choosing Longest side resizes the input, so look at it at actual size.
    func testChoosingLongestSideZoomsToOneToOne() async throws {
        let directory = try temporaryDirectory()
        let model = try await makeModel(in: directory)
        defer { model.shutdown() }
        let saved = UserDefaults.standard.object(forKey: "imageSizeLongestSide")
        defer { UserDefaults.standard.set(saved, forKey: "imageSizeLongestSide") }

        model.chooseImageSize(.original)
        XCTAssertTrue(model.fitImage, "Changing to Original from Original leaves the zoom alone")
        model.chooseImageSize(.longestSide)
        XCTAssertFalse(model.fitImage, "Longest side shows the resized input at 1:1")
        model.chooseImageSize(.original)
        XCTAssertTrue(model.fitImage, "Original fits the source again")
    }

    /// Switching photographs keeps the fit or 1:1 setting the user chose.
    func testChangingPhotographsKeepsTheZoomSetting() async throws {
        let directory = try temporaryDirectory()
        let model = try await makeModel(in: directory)
        defer { model.shutdown() }
        let saved = UserDefaults.standard.object(forKey: "imageSizeLongestSide")
        defer { UserDefaults.standard.set(saved, forKey: "imageSizeLongestSide") }

        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 32, pixelsHigh: 24,
            bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 32 * 3, bitsPerPixel: 24))
        memset(bitmap.bitmapData!, 120, bitmap.bytesPerRow * bitmap.pixelsHigh)
        let fixture = directory.appendingPathComponent("fixture.png")
        try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: fixture)

        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        XCTAssertTrue(model.fitImage)
        model.chooseImageSize(.longestSide)
        XCTAssertFalse(model.fitImage)
        model.open(fixture)
        XCTAssertFalse(model.fitImage, "A new photograph keeps the 1:1 setting")
    }

    private func temporaryDirectory() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-size-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        return directory
    }

    private func makeModel(in directory: URL) async throws -> EditorModel {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        await model.start(paths: paths)
        XCTAssertTrue(model.ready, model.error ?? "No engine")
        return model
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(predicate(), "Timed out waiting for image engine")
    }
}
