import XCTest
@testable import NegsApp

@MainActor final class ImageSizeZoomTests: XCTestCase {
    /// Choosing Longest side resizes the input, so look at it at actual size.
    func testChoosingLongestSideZoomsToOneToOne() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: "imageSizeLongestSide")
        defer { defaults.set(saved, forKey: "imageSizeLongestSide") }

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-size-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        await model.start(paths: paths)
        XCTAssertTrue(model.ready, model.error ?? "No engine")

        model.chooseImageSize(.original)
        XCTAssertTrue(model.fitImage, "Changing to Original leaves the zoom alone")
        model.chooseImageSize(.longestSide)
        XCTAssertFalse(model.fitImage, "Longest side shows the resized input at 1:1")
    }
}
