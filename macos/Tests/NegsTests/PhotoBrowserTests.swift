import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

final class PhotoBrowserTests: XCTestCase {
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
                XCTAssertEqual(model.source?.lastPathComponent, "Photo19.png")
            }
        }
    }
}
