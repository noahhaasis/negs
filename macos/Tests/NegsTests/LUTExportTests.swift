import XCTest
@testable import NegsApp

@MainActor final class LUTExportTests: XCTestCase {
    func testLUTExportWithoutPhotographUsesSettingsAndReplacesDestination() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let helper = root.appendingPathComponent("target/release/spektrafilm-native")
        guard FileManager.default.isExecutableFile(atPath: helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: EnginePaths(helper: helper, exporter: helper, data: root.appendingPathComponent("data")))
        XCTAssertNil(model.error)
        XCTAssertNil(model.source)
        XCTAssertTrue(model.canExportLUT)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let destination = directory.appendingPathComponent("look.cube")
        await model.exportLUT(to: destination)
        XCTAssertNil(model.error)
        let first = try String(contentsOf: destination, encoding: .utf8)
        XCTAssertTrue(first.contains("LUT_3D_SIZE 33\n"))
        XCTAssertEqual(first.split(separator: "\n").filter { $0.first?.isNumber ?? false }.count, 33 * 33 * 33)
        model.set("camera.exposure_compensation_ev", .number(1.5))
        await model.exportLUT(to: destination)
        XCTAssertNil(model.error)
        let changed = try String(contentsOf: destination, encoding: .utf8)
        XCTAssertNotEqual(first, changed)
        model.set("post.brightness", .number(80))
        await model.exportLUT(to: destination)
        XCTAssertNil(model.error)
        XCTAssertEqual(changed, try String(contentsOf: destination, encoding: .utf8))
        XCTAssertNil(model.source)
        XCTAssertFalse(model.saving)
        XCTAssertTrue(model.canExportLUT)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["look.cube"])
    }
}
