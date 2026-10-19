import IOSurface
import XCTest
@testable import NegsApp

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

    /// A rebuilt helper or another data folder makes the cached catalog stale.
    func testCatalogCacheIsKeyedByHelperAndData() throws {
        let fm = FileManager.default
        let directory = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try fm.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: directory) }
        let helper = directory.appendingPathComponent("spektrafilm-native")
        try Data("#!/bin/sh\n".utf8).write(to: helper)
        let suite = "negs-tests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let paths = EnginePaths(helper: helper, exporter: helper, data: directory)
        let catalog = JSONValue.object(["protocol": .number(1), "profiles": .array([.object(["id": .string("kodak_gold_200")])])])
        XCTAssertNil(CatalogCache.load(for: paths, defaults: defaults))
        CatalogCache.save(catalog, for: paths, defaults: defaults)
        XCTAssertEqual(CatalogCache.load(for: paths, defaults: defaults), catalog)
        XCTAssertNil(CatalogCache.load(for: EnginePaths(helper: helper, exporter: helper, data: directory.appendingPathComponent("other")), defaults: defaults))
        try fm.setAttributes([.modificationDate: Date(timeIntervalSinceNow: 60)], ofItemAtPath: helper.path)
        XCTAssertNil(CatalogCache.load(for: paths, defaults: defaults))
    }

    func testLocateFindsEachToolByOverrideThenAppThenCheckout() throws {
        let fm = FileManager.default
        let directory = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? fm.removeItem(at: directory) }
        let app = directory.appendingPathComponent("App/MacOS"), resources = directory.appendingPathComponent("App/Resources")
        let checkout = directory.appendingPathComponent("checkout"), release = checkout.appendingPathComponent("target/release")
        for folder in [app, resources.appendingPathComponent("data"), release, checkout.appendingPathComponent("data")] {
            try fm.createDirectory(at: folder, withIntermediateDirectories: true)
        }
        func tool(_ url: URL) throws {
            try Data("#!/bin/sh\n".utf8).write(to: url)
            try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        }
        for name in ["spektrafilm-native", "spektrafilm-f64", "spektrafilm"] { try tool(release.appendingPathComponent(name)) }
        func locate(_ env: [String: String] = [:]) throws -> EnginePaths {
            try EnginePaths.locate(environment: env.merging(["SPEKTRAFILM_ROOT": checkout.path]) { a, _ in a }, executableDir: app, resourceDir: resources, workingDir: directory)
        }
        var paths = try locate()
        XCTAssertEqual(paths.exporter, release.appendingPathComponent("spektrafilm-f64"))
        XCTAssertEqual(paths.fastExporter, release.appendingPathComponent("spektrafilm"))
        XCTAssertEqual(paths.data, resources.appendingPathComponent("data"))
        // The app bundle's copy wins over the checkout's.
        try tool(app.appendingPathComponent("spektrafilm"))
        paths = try locate()
        XCTAssertEqual(paths.fastExporter, app.appendingPathComponent("spektrafilm"))
        XCTAssertEqual(paths.exporter, release.appendingPathComponent("spektrafilm-f64"))
        // The override wins over both.
        let custom = directory.appendingPathComponent("custom-f32")
        try tool(custom)
        XCTAssertEqual(try locate(["SPEKTRAFILM_F32_CLI": custom.path]).fastExporter, custom)
        // A missing f32 tool only disables fast export; the app still starts.
        try fm.removeItem(at: app.appendingPathComponent("spektrafilm"))
        try fm.removeItem(at: release.appendingPathComponent("spektrafilm"))
        paths = try locate()
        XCTAssertNil(paths.fastExporter)
        XCTAssertThrowsError(try ExportEngine.fast.executable(in: paths))
        XCTAssertEqual(try ExportEngine.reference.executable(in: paths), release.appendingPathComponent("spektrafilm-f64"))
        XCTAssertNil(try locate(["SPEKTRAFILM_F32_CLI": directory.appendingPathComponent("missing").path]).fastExporter)
    }

    func testExportEngineDefaultsToFastAndDetectsTheBackend() {
        XCTAssertEqual(ExportEngine.reference.backend, "cpu")
        XCTAssertEqual(ExportEngine.fast.backend, "wgpu")
        let key = "exportEngine", saved = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(saved, forKey: key) }
        UserDefaults.standard.removeObject(forKey: key)
        XCTAssertEqual(ExportEngine.selected, .fast)
        ExportEngine.selected = .reference
        XCTAssertEqual(ExportEngine.selected, .reference)
        UserDefaults.standard.set("bogus", forKey: key)
        XCTAssertEqual(ExportEngine.selected, .fast)
        XCTAssertTrue(ExportEngine.ranOnGPU(log: "Image loaded\nBackend: wgpu (GPU)\nPipeline: 1 ms"))
        XCTAssertFalse(ExportEngine.ranOnGPU(log: "WARN wgpu backend requested but unavailable; falling back\nBackend: CPU (rayon)"))
        XCTAssertTrue(ExportEngine.ranOnGPU(backend: "wgpu (GPU)"))
        XCTAssertFalse(ExportEngine.ranOnGPU(backend: "CPU (rayon)"))
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
