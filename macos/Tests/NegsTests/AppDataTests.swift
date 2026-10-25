import AppKit
import XCTest
@testable import NegsApp

final class AppDataTests: XCTestCase {
    func testRemovesAppDataAndPreferencesWithoutFollowingPhotographOrExportPaths() throws {
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try fm.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: root) }
        let originals = root.appendingPathComponent("Photographs"), exports = root.appendingPathComponent("Exports")
        try fm.createDirectory(at: originals, withIntermediateDirectories: true)
        try fm.createDirectory(at: exports, withIntermediateDirectories: true)
        let photo = originals.appendingPathComponent("source.raw"), exported = exports.appendingPathComponent("print.png")
        let photoBytes = Data("original photograph".utf8), exportBytes = Data("finished export".utf8)
        try photoBytes.write(to: photo); try exportBytes.write(to: exported)
        let directories = ["Support", "Caches", "HTTPStorages", "Saved State", "Logs"].map { root.appendingPathComponent($0) }
        for directory in directories {
            try fm.createDirectory(at: directory, withIntermediateDirectories: true)
            try Data("app data".utf8).write(to: directory.appendingPathComponent("data"))
        }
        // Neither the source bookmark/path nor the last export folder is a
        // cleanup target, even when recorded in the session or preferences.
        try JSONEncoder().encode(EditorSession(source: photo, state: EditState())).write(to: directories[0].appendingPathComponent("session.json"))
        try fm.createSymbolicLink(at: directories[1].appendingPathComponent("photographs"), withDestinationURL: originals)
        let domain = "negs-cleanup-test-\(UUID().uuidString)"
        let unrelated = domain + "-other"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain); defaults.removePersistentDomain(forName: unrelated) }
        defaults.set(exported.path, forKey: "lastSaveFolder")
        defaults.set([photo.path], forKey: "recentFiles")
        defaults.setPersistentDomain(["keep": "other application"], forName: unrelated)
        let temporary = root.appendingPathComponent("Temporary")
        let scratch = try ScratchSpace(root: temporary)
        scratch.remove()
        let locations = AppDataLocations(directories: directories, temporary: temporary, preferencesDomain: domain)
        XCTAssertEqual(AppDataCleaner.removeFiles(at: locations), [])
        XCTAssertTrue(AppDataCleaner.removePreferences(domain: domain, defaults: defaults))
        XCTAssertTrue(defaults.persistentDomain(forName: domain)?.isEmpty ?? true)
        XCTAssertEqual(defaults.persistentDomain(forName: unrelated)?["keep"] as? String, "other application")
        XCTAssertTrue(directories.allSatisfy { !fm.fileExists(atPath: $0.path) })
        XCTAssertFalse(fm.fileExists(atPath: temporary.path))
        XCTAssertEqual(try Data(contentsOf: photo), photoBytes)
        XCTAssertEqual(try Data(contentsOf: exported), exportBytes)
        XCTAssertEqual(AppDataCleaner.removeFiles(at: locations), [], "Cleanup is safe to retry after missing files")
    }

    func testCleanupPreservesAnotherRunningLaunchAndReportsItsTemporaryDirectory() throws {
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? fm.removeItem(at: root) }
        let temporary = root.appendingPathComponent("Temporary")
        let running = try ScratchSpace(root: temporary)
        defer { running.remove() }
        let file = running.directory.appendingPathComponent("preview.rgba")
        try Data([1, 2, 3, 4]).write(to: file)
        let locations = AppDataLocations(directories: [], temporary: temporary, preferencesDomain: "unused")
        let failures = AppDataCleaner.removeFiles(at: locations)
        XCTAssertEqual(failures.count, 1)
        XCTAssertTrue(failures[0].contains(temporary.path))
        XCTAssertEqual(try Data(contentsOf: file), Data([1, 2, 3, 4]))
        running.remove()
        XCTAssertEqual(AppDataCleaner.removeFiles(at: locations), [])
    }

    func testDirectoryAndTemporaryLinksAreRemovedWithoutDeletingTheirTargets() throws {
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let originals = root.appendingPathComponent("Originals")
        try fm.createDirectory(at: originals, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: root) }
        let file = originals.appendingPathComponent("photo.raw")
        try Data([5, 6, 7]).write(to: file)
        let cache = root.appendingPathComponent("Cache"), temporary = root.appendingPathComponent("Temporary")
        try fm.createSymbolicLink(at: cache, withDestinationURL: originals)
        try fm.createSymbolicLink(at: temporary, withDestinationURL: originals)
        let locations = AppDataLocations(directories: [cache], temporary: temporary, preferencesDomain: "unused")
        XCTAssertEqual(AppDataCleaner.removeFiles(at: locations), [])
        XCTAssertEqual(try Data(contentsOf: file), Data([5, 6, 7]))
    }
}

@MainActor final class AppDataLifecycleTests: XCTestCase {
    func testShutdownDuringStartupPreventsRestorationAndSessionRecreation() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = dir.appendingPathComponent("photo.png")
        try Data("photograph".utf8).write(to: photo)
        let cache = RenderedPreviewCache(file: dir.appendingPathComponent("cache/preview"))
        let model = EditorModel(previewCache: cache)
        defer { model.shutdown() }
        let restoration = Task<EditorSession?, Error> {
            try await Task.sleep(for: .milliseconds(200))
            return EditorSession(source: photo, state: EditState())
        }
        let startup = Task { await model.start(paths: paths, restoration: restoration) }
        try await Task.sleep(for: .milliseconds(100))
        model.shutdown()
        await model.flushPreviewCache()
        await startup.value
        model.open(photo)
        XCTAssertNil(model.source)
        XCTAssertNil(model.session, "Normal termination must not re-save the discarded session")
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.file.path))
        XCTAssertEqual(try Data(contentsOf: photo), Data("photograph".utf8))
    }
}
