import AppKit
import IOSurface
import XCTest
@testable import NegsApp

final class RenderedPreviewCacheTests: XCTestCase {
    func testLosslessRoundTripKeysCorruptionAndInvalidation() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = dir.appendingPathComponent("photo.raw"), helper = dir.appendingPathComponent("helper")
        let data = dir.appendingPathComponent("data")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        try Data([1]).write(to: photo); try Data([2]).write(to: helper)
        let profile = data.appendingPathComponent("film.json")
        try Data([3]).write(to: profile)
        let paths = EnginePaths(helper: helper, exporter: helper, data: data)
        let cache = RenderedPreviewCache(file: dir.appendingPathComponent("cache/frame"))
        var state = EditState()
        state.params = .object(["io": .object(["rotate_cw": .number(0)]), "exposure": .number(1)])
        let key = try XCTUnwrap(RenderedPreviewCache.key(source: photo, state: state, paths: paths))
        let rgba = Data([1, 2, 3, 255, 10, 11, 12, 255, 70, 80, 90, 255, 250, 251, 252, 255])
        let surface = try EditorModel.previewSurface(rgba, width: 2, height: 2)
        cache.save(surface, photoSize: CGSize(width: 4000, height: 3000), sourceSize: CGSize(width: 2000, height: 1500), workingSize: CGSize(width: 8000, height: 6000), raw: true, key: key)
        let loaded = await cache.load(source: photo, state: state, paths: paths)
        let frame = try XCTUnwrap(loaded)
        XCTAssertEqual(bytes(frame.surface), bytes(surface))
        XCTAssertEqual(frame.photoSize, CGSize(width: 4000, height: 3000)); XCTAssertTrue(frame.raw)
        XCTAssertEqual(frame.sourceSize, CGSize(width: 2000, height: 1500))
        XCTAssertEqual(frame.workingSize, CGSize(width: 8000, height: 6000))

        XCTAssertEqual(colorSpaceName(frame.surface), CGColorSpace.sRGB as String)

        // A wide preview's proxy comes back tagged Rec. 2020, sRGB curve.
        let wide = try EditorModel.previewSurface(rgba, width: 2, height: 2, displaySpace: "Rec. 2020 (sRGB transfer)")
        XCTAssertEqual(colorSpaceName(wide), CGColorSpace.itur_2020_sRGBGamma as String)
        cache.save(wide, photoSize: CGSize(width: 4000, height: 3000), sourceSize: CGSize(width: 2000, height: 1500), workingSize: CGSize(width: 8000, height: 6000), raw: true, key: key)
        let reloaded = await cache.load(source: photo, state: state, paths: paths)
        XCTAssertEqual(colorSpaceName(try XCTUnwrap(reloaded).surface), CGColorSpace.itur_2020_sRGBGamma as String)
        cache.save(surface, photoSize: CGSize(width: 4000, height: 3000), sourceSize: CGSize(width: 2000, height: 1500), workingSize: CGSize(width: 8000, height: 6000), raw: true, key: key)
        await cache.flush()

        // Canvas edits reuse the very same pixels with their restored framing.
        var decorated = state
        decorated.params["io.rotate_cw"] = .number(1)
        decorated.crop.rect = CGRect(x: 0.1, y: 0.2, width: 0.7, height: 0.6)
        decorated.border.enabled = true
        XCTAssertEqual(RenderedPreviewCache.key(source: photo, state: decorated, paths: paths), key)
        // With a Longest side target the crop's extent is part of the frame;
        // its position is not.
        var sized = decorated
        sized.params["io.photo_long_side_px"] = .number(3000)
        let sizedKey = RenderedPreviewCache.key(source: photo, state: sized, paths: paths)
        XCTAssertNotEqual(sizedKey, key)
        sized.crop.rect.origin = CGPoint(x: 0.3, y: 0.1)
        XCTAssertEqual(RenderedPreviewCache.key(source: photo, state: sized, paths: paths), sizedKey)
        sized.crop.rect.size.width = 0.5
        XCTAssertNotEqual(RenderedPreviewCache.key(source: photo, state: sized, paths: paths), sizedKey)
        for change in ["exposure", "film", "paper", "resolution"] {
            var changed = state
            switch change {
            case "exposure": changed.params["exposure"] = .number(2)
            case "film": changed.film = "other"
            case "paper": changed.paper = "other"
            default: changed.limitPreview = true
            }
            let stale = await cache.load(source: photo, state: changed, paths: paths)
            XCTAssertNil(stale, change)
        }
        for url in [photo, helper, profile] {
            let original = try Data(contentsOf: url)
            try Data([4, 5]).write(to: url)
            XCTAssertNotEqual(RenderedPreviewCache.key(source: photo, state: state, paths: paths), key)
            try original.write(to: url)
        }
        let newKey = try XCTUnwrap(RenderedPreviewCache.key(source: photo, state: state, paths: paths))
        cache.save(surface, photoSize: frame.photoSize, sourceSize: frame.sourceSize, workingSize: frame.workingSize, raw: true, key: newKey)
        await cache.flush()
        var corrupt = try Data(contentsOf: cache.file); corrupt[corrupt.count - 1] ^= 1
        try corrupt.write(to: cache.file)
        let invalid = await cache.load(source: photo, state: state, paths: paths)
        XCTAssertNil(invalid)
        cache.save(surface, photoSize: frame.photoSize, sourceSize: frame.sourceSize, workingSize: frame.workingSize, raw: true, key: newKey)
        cache.invalidate()
        await cache.flush()
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.file.path), "An earlier queued save must not resurrect an invalidated frame")
    }

    private func bytes(_ surface: IOSurface) -> Data {
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        var data = Data()
        for y in 0..<surface.height { data.append(surface.baseAddress.advanced(by: y * surface.bytesPerRow).assumingMemoryBound(to: UInt8.self), count: surface.width * 4) }
        return data
    }
}

@MainActor final class RenderedPreviewLifecycleTests: XCTestCase {
    func testStartupUsesExactFinishedFrameAndInterruptedEditFallsBackToRawPreview() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let data = dir.appendingPathComponent("data")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = dir.appendingPathComponent("photo.png")
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 8, pixelsHigh: 6, bitsPerSample: 8,
                                      samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        memset(bitmap.bitmapData!, 127, bitmap.bytesPerRow * bitmap.pixelsHigh)
        try bitmap.representation(using: .png, properties: [:])!.write(to: photo)
        let helper = dir.appendingPathComponent("helper")
        let script = """
        #!/usr/bin/python3
        import json, sys, time, pathlib
        for line in sys.stdin:
            c = json.loads(line)
            if c['command'] == 'catalog':
                r = {'protocol': 1, 'profiles': [{'id': 'kodak_gold_200'}, {'id': 'kodak_portra_endura', 'paper': True}],
                     'defaults': {'io': {'rotate_cw': 0, 'scan_film': False}, 'camera': {'exposure_compensation_ev': 0}}}
            elif c['command'] == 'render':
                with open(__file__ + '.renders', 'a') as log:
                    log.write(json.dumps(c['params']) + '\\n')
                time.sleep(1)
                pathlib.Path(c['preview_path']).write_bytes(bytes([10, 30, 80, 255]) * 48)
                r = {'width': 8, 'height': 6, 'photo_width': 80, 'photo_height': 60, 'working_width': 80, 'working_height': 60,
                     'source_width': 80, 'source_height': 60, 'raw': False, 'export_resolution': True, 'backend': 'cpu'}
            else:
                r = {}
            print(json.dumps({'id': c['id'], 'result': r}), flush=True)
        """
        try Data(script.utf8).write(to: helper)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)
        let paths = EnginePaths(helper: helper, exporter: helper, data: data)
        let cache = RenderedPreviewCache(file: dir.appendingPathComponent("frame.cache"))
        let first = EditorModel(previewCache: cache)
        defer { first.shutdown() }
        await first.start(paths: paths)
        first.open(photo)
        let renderLog = URL(fileURLWithPath: helper.path + ".renders")
        try await waitUntil { FileManager.default.fileExists(atPath: renderLog.path) }
        first.set("camera.exposure_compensation_ev", .number(0.5))
        try await waitUntil { ((try? String(contentsOf: renderLog, encoding: .utf8)) ?? "").split(separator: "\n").count == 2 }
        await first.flushPreviewCache()
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.file.path), "The superseded render must never publish a cache")
        try await waitUntil { first.previewCurrent }
        first.rotate()
        let session = try XCTUnwrap(first.session)
        await first.flushPreviewCache(); first.shutdown()

        let restored = EditorModel(previewCache: cache)
        defer { restored.shutdown() }
        await restored.start(paths: paths, restoration: Task { session })
        try await waitUntil { restored.image != nil }
        XCTAssertNil(restored.placeholder, "No raw flash on a cache hit")
        XCTAssertFalse(restored.previewCurrent, "The matching frame must appear before rendering finishes")
        XCTAssertEqual(restored.imageSize, CGSize(width: 80, height: 60))
        XCTAssertEqual(restored.workingSize, CGSize(width: 80, height: 60), "Image Size readouts restore with the frame")
        XCTAssertEqual(restored.sourceSize, CGSize(width: 80, height: 60))
        XCTAssertEqual(restored.imageRotation, 1)
        try await Task.sleep(for: .milliseconds(450))
        XCTAssertFalse(restored.showsActivity, "A matching cache needs no Updating badge")
        restored.set("camera.exposure_compensation_ev", .number(2))
        let interrupted = try XCTUnwrap(restored.session)
        restored.shutdown(); await restored.flushPreviewCache()
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.file.path))

        let unfinished = EditorModel(previewCache: cache)
        defer { unfinished.shutdown() }
        await unfinished.start(paths: paths, restoration: Task { interrupted })
        try await waitUntil { unfinished.placeholder != nil }
        XCTAssertNil(unfinished.image)
        XCTAssertFalse(unfinished.previewCurrent)
        XCTAssertEqual(unfinished.value("camera.exposure_compensation_ev"), .number(2))
        try await waitUntil { unfinished.showsActivity }
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(8)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertTrue(predicate())
    }
}

/// The colour space an IOSurface is tagged with.
private func colorSpaceName(_ surface: IOSurface) -> String? {
    IOSurfaceCopyValue(surface, kIOSurfaceColorSpace)
        .flatMap { CGColorSpace(propertyListPlist: $0 as CFPropertyList) }?.name as String?
}
