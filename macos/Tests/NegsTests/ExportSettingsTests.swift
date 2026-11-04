import XCTest
@testable import NegsApp

@MainActor final class ExportSettingsTests: XCTestCase {
    func testFormatChoosesDepthAndQuality() throws {
        try withModel { model in
            model.exportFormat = .png
            model.exportBitDepth = 16
            XCTAssertEqual(model.exportSaveSettings.bitDepth, 16)

            model.exportFormat = .jpeg
            XCTAssertEqual(model.exportSaveSettings.bitDepth, 8, "JPEG is always 8-bit")
            model.exportQuality = 80
            XCTAssertEqual(model.exportSaveSettings.jpegQuality, 80)
            model.exportBitDepth = 16
            XCTAssertEqual(model.exportSaveSettings.bitDepth, 8, "JPEG ignores a 16-bit setting")

            model.exportFormat = .png
            XCTAssertEqual(model.exportSaveSettings.bitDepth, 16, "PNG remembers its own depth")
        }
    }

    func testQualityStaysWithinItsRange() throws {
        try withModel { model in
            model.exportQuality = 0
            XCTAssertEqual(model.exportQuality, SaveSettings.qualityRange.lowerBound)
            model.exportQuality = 200
            XCTAssertEqual(model.exportQuality, SaveSettings.qualityRange.upperBound)
        }
    }

    func testProPhotoFallsBackAtEightBits() throws {
        try withModel { model in
            model.choosePreviewSpace("ProPhoto RGB")
            model.exportBitDepth = 8
            XCTAssertEqual(model.exportSaveSettings.colorSpace, SaveSettings.prophotoFallback)
            model.exportBitDepth = 16
            XCTAssertEqual(model.exportSaveSettings.colorSpace, SaveSettings.prophoto)
        }
    }

    /// Runs a test with a model, restoring every preference the export
    /// settings touch afterwards.
    private func withModel(_ body: (EditorModel) throws -> Void) throws {
        let defaults = UserDefaults.standard
        let keys = ["outputFormat", "bitDepth.png", "bitDepth.tiff", "jpegQuality", "colorSpace",
                    "exportEngine", "exportEdge", "exportEdgePixels"]
        let saved = keys.map { ($0, defaults.object(forKey: $0)) }
        defer { for (key, value) in saved { defaults.set(value, forKey: key) } }

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-export-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        try body(model)
    }

    /// A save adjusts before it straightens, as the preview and a Fast export
    /// do, so a pin or Post edit on a straightened crop saves what was shown.
    @MainActor func testSavesApplyPostBeforeTheCrop() {
        let model = EditorModel()
        defer { model.shutdown() }
        let command = model.saveCommand(to: URL(fileURLWithPath: "/tmp/out.png"), size: .full, save: SaveSettings())
        XCTAssertEqual(command["crop_after_post"], .bool(true))
    }
}
