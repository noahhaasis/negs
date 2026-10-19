import AppKit
import XCTest
@testable import SpektrafilmApp

final class SessionTests: XCTestCase {
    func testRoundTripAndMovedPhotographBookmark() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = dir.appendingPathComponent("photo.png")
        try Data("photo".utf8).write(to: photo)
        let store = SessionStore(file: dir.appendingPathComponent("settings/session.json"))
        var state = EditState()
        state.params = .object(["io": .object(["rotate_cw": .number(3)]), "exposure": .number(1.5)])
        state.border = BorderSettings(enabled: true, style: .keyline, export: false)
        state.crop.rect = CGRect(x: 0.1, y: 0.2, width: 0.7, height: 0.6)
        state.crop.angle = 2
        let session = EditorSession(source: photo, state: state, inspectorVisible: false, fitImage: false)
        try await store.save(session)
        let loaded = try await store.load()
        XCTAssertEqual(loaded?.state, state)
        XCTAssertEqual(loaded?.inspectorVisible, false)
        XCTAssertEqual(loaded?.fitImage, false)
        XCTAssertNotNil(loaded?.bookmark)
        let moved = dir.appendingPathComponent("moved.png")
        try FileManager.default.moveItem(at: photo, to: moved)
        let restored = try await store.load()
        XCTAssertEqual(restored?.source.standardizedFileURL, moved.standardizedFileURL)
    }

    func testMissingCorruptAndFutureSessions() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("session.json")
        let store = SessionStore(file: file)
        let absent = try await store.load()
        XCTAssertNil(absent)
        try Data("not JSON".utf8).write(to: file)
        do { _ = try await store.load(); XCTFail("Corrupt session accepted") } catch {}
        let missing = EditorSession(source: dir.appendingPathComponent("missing.png"), state: EditState())
        try await store.save(missing)
        do { _ = try await store.load(); XCTFail("Unavailable photograph accepted") } catch {}
        var future = missing; future.schema = 99
        XCTAssertThrowsError(try future.validate())
        var invalid = missing; invalid.state.crop.rect = CGRect(x: 0, y: 0, width: -1, height: 1)
        XCTAssertThrowsError(try invalid.validate())
    }

    func testSavedValuesMergeWithNewEngineDefaults() {
        let defaults: JSONValue = .object(["camera": .object(["ev": .number(0), "newField": .bool(true)]), "newGroup": .number(7)])
        let saved: JSONValue = .object(["camera": .object(["ev": .number(1.5), "removed": .number(9)]), "removedGroup": .bool(true)])
        let merged = defaults.restoring(saved)
        XCTAssertEqual(merged["camera.ev"], .number(1.5))
        XCTAssertEqual(merged["camera.newField"], .bool(true))
        XCTAssertEqual(merged["newGroup"], .number(7))
        XCTAssertEqual(merged["camera.removed"], .null)
        XCTAssertEqual(merged["removedGroup"], .null)
        XCTAssertEqual(JSONValue.number(1).restoring(.string("wrong type")), .number(1))
    }

    func testUpdaterRequiresBothHTTPSFeedAndValidPublicKey() {
        let key = Data(repeating: 1, count: 32).base64EncodedString()
        XCTAssertFalse(UpdateController.validConfiguration([:]))
        XCTAssertFalse(UpdateController.validConfiguration(["SUFeedURL": "https://example.com/feed.xml"]))
        XCTAssertFalse(UpdateController.validConfiguration(["SUFeedURL": "http://example.com/feed.xml", "SUPublicEDKey": key]))
        XCTAssertFalse(UpdateController.validConfiguration(["SUFeedURL": "https://example.com/feed.xml", "SUPublicEDKey": "bad"]))
        XCTAssertTrue(UpdateController.validConfiguration(["SUFeedURL": "https://example.com/feed.xml", "SUPublicEDKey": key]))
    }
}

@MainActor final class SessionLifecycleTests: XCTestCase {
    private var paths: EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                           exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func fixture() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("session-\(UUID().uuidString).png")
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64,
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                      colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        memset(bitmap.bitmapData!, 127, bitmap.bytesPerRow * bitmap.pixelsHigh)
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    func testRestoresBeforeFirstRenderIncludingSlidePaperCropAndRotation() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build release helper first") }
        let photo = try fixture()
        defer { try? FileManager.default.removeItem(at: photo) }
        var saved = EditState()
        saved.film = "kodak_trix"
        saved.params = .object(["io": .object(["scan_film": .bool(true), "rotate_cw": .number(1), "upscale_factor": .number(0.5)]),
                                "camera": .object(["exposure_compensation_ev": .number(1.5)])])
        saved.crop.rect = CGRect(x: 0.1, y: 0.1, width: 0.8, height: 0.8)
        saved.border = BorderSettings(enabled: true, style: .keyline, export: false)
        let session = EditorSession(source: photo, state: saved, inspectorVisible: false)
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths, restoration: Task { session })
        let deadline = Date().addingTimeInterval(30)
        while !model.previewCurrent && model.error == nil && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertNil(model.error)
        XCTAssertTrue(model.previewCurrent)
        XCTAssertEqual(model.imageSize, CGSize(width: 48, height: 32))
        XCTAssertEqual(model.imageRotation, 1)
        XCTAssertEqual(model.film, saved.film)
        XCTAssertEqual(model.crop, saved.crop)
        XCTAssertEqual(model.border, saved.border)
        XCTAssertTrue(model.value("io.scan_film").bool)
        XCTAssertEqual(model.value("camera.exposure_compensation_ev"), .number(1.5))
        XCTAssertFalse(model.inspectorVisible)
    }

    func testExplicitOpenWinsWhileSessionIsLoading() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build release helper first") }
        let previous = try fixture(), chosen = try fixture()
        defer { try? FileManager.default.removeItem(at: previous); try? FileManager.default.removeItem(at: chosen) }
        let model = EditorModel()
        defer { model.shutdown() }
        let restore = Task<EditorSession?, Error> {
            try await Task.sleep(for: .milliseconds(250))
            return EditorSession(source: previous, state: EditState())
        }
        let startup = Task { await model.start(paths: paths, restoration: restore) }
        model.open(chosen)
        await startup.value
        XCTAssertEqual(model.source, chosen)
        XCTAssertTrue(model.ready)
    }

    func testUpdateRestartWaitsForExportWithoutCancellingIt() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build release helper first") }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let exporter = dir.appendingPathComponent("slow-exporter")
        try Data("#!/bin/sh\nsleep 1\necho completed > \"$4\"\n".utf8).write(to: exporter)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: exporter.path)
        let photo = try fixture()
        defer { try? FileManager.default.removeItem(at: photo) }
        let model = EditorModel()
        let updates = UpdateController()
        defer { updates.stop(); model.shutdown() }
        await model.start(paths: EnginePaths(helper: paths.helper, exporter: exporter, data: paths.data))
        model.open(photo)
        updates.schedule(afterStartupOf: model)
        let output = dir.appendingPathComponent("export.png")
        let export = Task { await model.export(to: output, engine: .reference) }
        let deadline = Date().addingTimeInterval(5)
        while !model.exporting && Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(model.exporting)
        var restarted = false
        let installed = expectation(description: "Restart allowed after export")
        XCTAssertTrue(updates.postponeRestart { restarted = true; installed.fulfill() })
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertFalse(restarted)
        XCTAssertTrue(model.exporting)
        await export.value
        await fulfillment(of: [installed], timeout: 5)
        XCTAssertEqual(try String(contentsOf: output, encoding: .utf8), "completed\n")
        XCTAssertNil(model.error)
        XCTAssertFalse(model.exporting)
    }
}
