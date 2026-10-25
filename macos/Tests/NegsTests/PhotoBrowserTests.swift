import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

final class PhotoBrowserTests: XCTestCase {
    @MainActor private func eventually(file: StaticString = #filePath, line: UInt = #line, _ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(5)
        while !condition() {
            guard Date() < deadline else {
                XCTFail("Folder did not update within five seconds", file: file, line: line)
                throw NSError(domain: "PhotoBrowserTests", code: 1)
            }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
    }

    private func temporaryDirectory() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    func testFolderFiltersFilesAndUsesFinderOrdering() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        for name in ["Photo10.JPG", "Photo2.NEF", "Photo1.png", "Photo3.tiff", ".hidden.jpg", "notes.txt", "movie.mov", "unsupported.heic"] {
            try Data().write(to: directory.appendingPathComponent(name))
        }
        let nested = directory.appendingPathComponent("album.jpg")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try Data().write(to: nested.appendingPathComponent("nested.jpg"))
        let files = try PhotoFolder.images(in: directory)
        XCTAssertTrue(files.contains(directory.appendingPathComponent("Photo1.png")), "Keep the opened URL spelling even through symlinked folders")
        XCTAssertEqual(files.map(\.lastPathComponent),
                       ["Photo1.png", "Photo2.NEF", "Photo3.tiff", "Photo10.JPG"])
        XCTAssertThrowsError(try PhotoFolder.images(in: directory.appendingPathComponent("missing")))
    }

    func testThumbnailIsSmallOrientedAndHandlesMissingFiles() async throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("portrait.jpg")
        let context = try XCTUnwrap(CGContext(data: nil, width: 800, height: 400, bitsPerComponent: 8,
            bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        context.setFillColor(CGColor(red: 0.3, green: 0.5, blue: 0.8, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 800, height: 400))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), [kCGImagePropertyOrientation: 6] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let thumbnails = PhotoThumbnails()
        let result = await thumbnails.image(for: url)
        let image = try XCTUnwrap(result)
        XCTAssertEqual(image.width, 96)
        XCTAssertEqual(image.height, 192)
        let cached = await thumbnails.image(for: url)
        XCTAssertTrue(cached === image)
        let missing = await thumbnails.image(for: directory.appendingPathComponent("missing.nef"))
        XCTAssertNil(missing)
    }

    @MainActor func testFolderEventsRefreshCopiesRenamesDeletesAndBatchesThenStayIdle() async throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let first = directory.appendingPathComponent("Photo1.png")
        let second = directory.appendingPathComponent("Photo2.png")
        try Data().write(to: first)
        let watcher = try XCTUnwrap(PhotoFolderWatcher(directory: directory))
        var files = try PhotoFolder.snapshot(in: directory)
        var scans = 0
        let observer = Task { @MainActor in
            for await _ in watcher.changes {
                guard !Task.isCancelled else { return }
                files = try PhotoFolder.snapshot(in: directory)
                scans += 1
            }
        }
        defer { observer.cancel(); watcher.stop() }

        try Data().write(to: second)
        try await eventually { files.map(\.url) == [first, second] }
        // An existing, initially empty file gets more bytes as copying proceeds.
        try Data(repeating: 7, count: 512).write(to: second)
        try await eventually { files.first { $0.url == second }?.size == 512 }
        let renamed = directory.appendingPathComponent("Photo10.png")
        try FileManager.default.moveItem(at: first, to: renamed)
        try await eventually { files.map(\.url) == [second, renamed] }
        try FileManager.default.removeItem(at: second)
        try await eventually { files.map(\.url) == [renamed] }

        let beforeBurst = scans
        for index in 20..<140 {
            try Data().write(to: directory.appendingPathComponent("Photo\(index).jpg"))
        }
        try await eventually { files.count == 121 }
        XCTAssertLessThan(scans - beforeBurst, 10, "A burst should be batched, not scanned once per file")
        try await Task.sleep(nanoseconds: 600_000_000)
        let settled = scans
        try await Task.sleep(nanoseconds: 500_000_000)
        XCTAssertEqual(scans, settled, "An idle folder must not trigger periodic scans")

        watcher.stop()
        try await observer.value
        try Data().write(to: directory.appendingPathComponent("after-stop.jpg"))
        try await Task.sleep(nanoseconds: 300_000_000)
        XCTAssertEqual(scans, settled)
        print("Filmstrip: 120 arrivals used \(settled - beforeBurst) scans; idle and stopped watches used 0")
    }

    func testThumbnailRecoversAfterPartialCopyAndKeepsUnchangedCache() async throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("copy.png")
        try Data().write(to: url)
        let incomplete = try XCTUnwrap(PhotoFolder.snapshot(in: directory).first)
        let thumbnails = PhotoThumbnails()
        let failed = await thumbnails.image(for: incomplete)
        XCTAssertNil(failed)

        func writeImage(width: Int, height: Int) throws {
            let context = try XCTUnwrap(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
        }
        try writeImage(width: 400, height: 200)
        let complete = try XCTUnwrap(PhotoFolder.snapshot(in: directory).first)
        XCTAssertNotEqual(incomplete, complete)
        let decoded = await thumbnails.image(for: complete)
        let image = try XCTUnwrap(decoded)
        try Data().write(to: directory.appendingPathComponent("other.jpg"))
        let unchanged = try XCTUnwrap(PhotoFolder.snapshot(in: directory).first { $0.url == url })
        let cached = await thumbnails.image(for: unchanged)
        XCTAssertTrue(cached === image, "New arrivals must not decode unchanged thumbnails again")
        try writeImage(width: 200, height: 400)
        let replaced = try XCTUnwrap(PhotoFolder.snapshot(in: directory).first { $0.url == url })
        let updated = await thumbnails.image(for: replaced)
        XCTAssertFalse(updated === image)
        XCTAssertEqual(updated?.height, 192)
        XCTAssertEqual(updated?.width, 96)
    }

    func testFolderScanBenchmark() throws {
        guard ProcessInfo.processInfo.environment["NEGS_FILMSTRIP_BENCHMARK"] != nil else {
            throw XCTSkip("Set NEGS_FILMSTRIP_BENCHMARK to compare directory enumeration")
        }
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        for index in 0..<1000 { try Data().write(to: directory.appendingPathComponent("Photo\(index).jpg")) }
        // The pre-watcher enumeration, measured against the metadata snapshot.
        func originalScan() throws -> [URL] {
            try FileManager.default.contentsOfDirectory(at: directory,
                includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles])
                .filter { PhotoFolder.extensions.contains($0.pathExtension.lowercased())
                    && (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true }
                .map { directory.appendingPathComponent($0.lastPathComponent) }
                .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
        }
        var before: [Double] = [], after: [Double] = []
        for _ in 0..<12 {
            var start = Date()
            XCTAssertEqual(try originalScan().count, 1000)
            before.append(Date().timeIntervalSince(start) * 1000)
            start = Date()
            XCTAssertEqual(try PhotoFolder.snapshot(in: directory).count, 1000)
            after.append(Date().timeIntervalSince(start) * 1000)
        }
        print("Filmstrip 1000-file scan median: original \(before.sorted()[6]) ms; live metadata \(after.sorted()[6]) ms")
    }

    @MainActor func testFilmstripWindow() async throws {
        guard let folder = ProcessInfo.processInfo.environment["NEGS_FILMSTRIP_SNAPSHOTS"] else {
            throw XCTSkip("Set NEGS_FILMSTRIP_SNAPSHOTS for the film strip window smoke test")
        }
        _ = NSApplication.shared
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        for index in 1...30 {
            let context = try XCTUnwrap(CGContext(data: nil, width: index % 3 == 0 ? 400 : 600, height: 400,
                bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
            context.setFillColor(NSColor(calibratedHue: CGFloat(index) / 30, saturation: 0.35, brightness: 0.65, alpha: 1).cgColor)
            context.fill(CGRect(x: 0, y: 0, width: 600, height: 400))
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(directory.appendingPathComponent("Photo\(index).png") as CFURL,
                UTType.png.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
        }
        let model = EditorModel()
        let previousVisibility = model.filmstripVisible
        defer { model.filmstripVisible = previousVisibility; model.shutdown() }
        model.filmstripVisible = true
        model.open(directory.appendingPathComponent("Photo18.png"))
        let listed = try PhotoFolder.images(in: directory)
        XCTAssertTrue(listed.contains(model.source!), "Source: \(model.source!); listed: \(listed[17])")
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        window.setContentSize(NSSize(width: 1100, height: 700))
        controller.showWindow(nil)
        defer { window.orderOut(nil) }
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        for (name, appearance, visible) in [("light", NSAppearance.Name.aqua, true), ("dark", .darkAqua, true), ("hidden", .aqua, false)] {
            window.appearance = NSAppearance(named: appearance)
            if model.filmstripVisible != visible {
                let toggle = try XCTUnwrap(window.toolbar?.items.first { $0.itemIdentifier.rawValue == "filmstrip" })
                XCTAssertTrue(NSApp.sendAction(try XCTUnwrap(toggle.action), to: toggle.target, from: toggle))
                XCTAssertEqual(model.filmstripVisible, visible)
            }
            try await Task.sleep(nanoseconds: 800_000_000)
            let view = try XCTUnwrap(window.contentView)
            view.layoutSubtreeIfNeeded()
            let bitmap = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: bitmap)
            try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: URL(fileURLWithPath: folder).appendingPathComponent("\(name).png"))
            if name == "light" {
                // A newly arrived image sorts immediately after the selection.
                // Opening it via the strip proves the mounted view refreshed.
                try FileManager.default.copyItem(at: directory.appendingPathComponent("Photo18.png"),
                    to: directory.appendingPathComponent("Photo18a.png"))
                try await Task.sleep(nanoseconds: 700_000_000)
                XCTAssertEqual(model.source?.lastPathComponent, "Photo18.png")
                // The selected thumbnail is centered; click its right neighbour.
                let split = try XCTUnwrap(window.contentViewController as? NSSplitViewController)
                let canvas = try XCTUnwrap(split.splitView.subviews.first)
                let point = NSPoint(x: canvas.frame.width / 2 + 104, y: 80)
                for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                    let event = try XCTUnwrap(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                        context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
                    window.sendEvent(event)
                }
                try await Task.sleep(nanoseconds: 100_000_000)
                XCTAssertEqual(model.source?.lastPathComponent, "Photo18a.png")
            }
        }
    }
}
