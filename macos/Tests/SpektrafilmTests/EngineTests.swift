import IOSurface
import XCTest
@testable import SpektrafilmApp

final class EngineTests: XCTestCase {
    func testParameterEditsPreserveUnknownFieldsAndArraySiblings() throws {
        let data = Data(#"{"scanner":{"unsharp_mask":[0.7,0.8]},"future":{"precise":0.341234567890123,"enabled":true},"nullable":null}"#.utf8)
        var params = try JSONDecoder().decode(JSONValue.self, from: data)
        let untouched = params["future"]
        params["scanner.unsharp_mask.1"] = .number(1.25)
        XCTAssertEqual(params["scanner.unsharp_mask.0"], .number(0.7))
        XCTAssertEqual(params["scanner.unsharp_mask.1"], .number(1.25))
        XCTAssertEqual(params["future"], untouched)
        XCTAssertEqual(params["nullable"], .null)
        XCTAssertEqual(try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(params)), params)
    }

    func testCatalogMatchesEveryInspectorControl() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let helper = root.appendingPathComponent("target/release/spektrafilm-native")
        guard FileManager.default.isExecutableFile(atPath: helper.path) else {
            throw XCTSkip("Build spektrafilm-native in release mode before the integration test")
        }
        let client = try EngineClient(paths: EnginePaths(helper: helper, exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data")))
        defer { client.stop() }
        let (_, catalog) = try await client.request(["command": .string("catalog")])
        XCTAssertEqual(catalog["protocol"], .number(1))
        let defaults = catalog["defaults"]
        for group in adjustmentGroups {
            for control in group.controls {
                XCTAssertNotEqual(defaults[control.id], .null, "Missing Rust parameter: \(control.id)")
                switch control {
                case .slider(_, _, let range, _): XCTAssertTrue(range.contains(defaults[control.id].double), control.id)
                case .toggle: if case .bool = defaults[control.id] {} else { XCTFail(control.id) }
                case .choice(_, _, let options): XCTAssertTrue(options.contains(defaults[control.id].string), control.id)
                }
            }
        }
        let stocks = catalog["profiles"].array.map(Stock.init)
        XCTAssertEqual(stocks.first { $0.id == "kodak_gold_200" }?.target, "kodak_portra_endura")
        XCTAssertTrue(stocks.first { $0.id == "kodak_trix" }?.positive ?? false)
        XCTAssertGreaterThan(stocks.first { $0.id == "kodak_doublex" }?.times.count ?? 0, 1)
        // A failed request must not corrupt the serial protocol session.
        do {
            _ = try await client.request(["command": .string("save"), "render_id": .number(99), "output": .string("/unused.png")])
            XCTFail("Expected no-preview error")
        } catch { XCTAssertTrue(error.localizedDescription.contains("No preview")) }
        let (id, next) = try await client.request(["command": .string("catalog")])
        XCTAssertEqual(id, 3)
        XCTAssertEqual(next["profiles"], catalog["profiles"])
    }

    func testSaveRefusesTheOriginalUnderAnotherSpelling() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let original = folder.appendingPathComponent("Photo.JPG")
        try Data("original".utf8).write(to: original)
        let link = folder.appendingPathComponent("link.jpg")
        try FileManager.default.linkItem(at: original, to: link)
        XCTAssertTrue(EditorModel.sameFile(link, original), "hard link")
        if (try? folder.resourceValues(forKeys: [.volumeSupportsCaseSensitiveNamesKey]).volumeSupportsCaseSensitiveNames) == false {
            XCTAssertTrue(EditorModel.sameFile(folder.appendingPathComponent("photo.jpg"), original), "case-only difference")
        }
        XCTAssertFalse(EditorModel.sameFile(folder.appendingPathComponent("photo_kodak_gold_200.png"), original))
    }

    func testInvalidPreviewBuffersAreRejected() throws {
        let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: path) }
        try Data([0, 0, 0]).write(to: path)
        XCTAssertThrowsError(try EditorModel.readPreview(path, width: 1, height: 1))
        XCTAssertThrowsError(try EditorModel.readPreview(path, width: 0, height: 1))
        try Data([10, 20, 30, 7]).write(to: path)
        let surface = try EditorModel.readPreview(path, width: 1, height: 1)
        XCTAssertEqual(surface.width, 1)
        surface.lock(options: .readOnly, seed: nil)
        let pixel = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        XCTAssertEqual([pixel[0], pixel[1], pixel[2], pixel[3]], [30, 20, 10, 255], "RGBA becomes opaque BGRA")
        surface.unlock(options: .readOnly, seed: nil)
        let space = IOSurfaceCopyValue(surface, kIOSurfaceColorSpace) as? NSDictionary
        XCTAssertEqual(space, CGColorSpace(name: CGColorSpace.sRGB)?.copyPropertyList() as? NSDictionary)
    }
}
