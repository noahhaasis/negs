import AppKit
import ImageIO
import IOSurface
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

@MainActor final class OriginalTests: XCTestCase {
    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    /// Upright like the engine's loader, and no larger than asked.
    func testJPEGIsOrientedAndScaled() throws {
        let fixture = try makeJPEG(orientation: 6)
        defer { try? FileManager.default.removeItem(at: fixture) }
        let full = try XCTUnwrap(OriginalImage.load(fixture, raw: false, maxPixels: 8192))
        XCTAssertEqual([full.width, full.height], [64, 96])
        let small = try XCTUnwrap(OriginalImage.load(fixture, raw: false, maxPixels: 48))
        XCTAssertEqual(max(small.width, small.height), 48)
    }

    /// The same framing as the engine's Apple RAW decode, at the preview's size.
    func testRAWMatchesTheEngineDecodeSize() throws {
        guard let path = ProcessInfo.processInfo.environment["SPEKTRAFILM_RAW_SAMPLE"] else { throw XCTSkip("Set SPEKTRAFILM_RAW_SAMPLE to a RAW file") }
        let url = URL(fileURLWithPath: path)
        let extent = try XCTUnwrap(CIRAWFilter(imageURL: url)?.outputImage).extent
        let surface = try XCTUnwrap(OriginalImage.load(url, raw: true, maxPixels: 2048))
        XCTAssertEqual(max(surface.width, surface.height), 2048, accuracy: 2)
        XCTAssertEqual(Double(surface.width) / Double(surface.height), extent.width / extent.height, accuracy: 0.002)
    }

    func testVolatileSurfaceKeepsItsPixelsWithoutPressure() throws {
        let surface = try XCTUnwrap(IOSurface(properties: [.width: 4, .height: 4, .bytesPerElement: 4]))
        surface.makeVolatile()
        XCTAssertTrue(surface.makeNonVolatile())
    }

    func testHoldingShowsTheOriginalUntilReleased() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        let fixture = try makeJPEG(orientation: 1)
        defer { try? FileManager.default.removeItem(at: fixture) }
        XCTAssertFalse(model.canShowOriginal)
        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        model.holdOriginal(true)
        // The badge says Loading Original until the decode arrives.
        XCTAssertTrue(model.holdingOriginal)
        XCTAssertNil(model.original)
        try await waitUntil { model.original != nil }
        XCTAssertEqual([model.original?.width, model.original?.height], [96, 64])
        model.holdOriginal(false)
        XCTAssertNil(model.original)
        // The second press shows the kept decode at once.
        model.holdOriginal(true)
        XCTAssertNotNil(model.original)
        model.beginCrop()
        XCTAssertNil(model.original)
        XCTAssertFalse(model.canShowOriginal)
        model.holdOriginal(true)
        XCTAssertNil(model.original)
    }

    /// The original takes the developed photograph's place inside the same
    /// crop and border. Writes before and after snapshots to compare.
    func testOriginalKeepsTheCanvasInPlace() async throws {
        guard let folder = ProcessInfo.processInfo.environment["SPEKTRAFILM_UI_SNAPSHOTS"] else { throw XCTSkip("Set SPEKTRAFILM_UI_SNAPSHOTS for the window smoke test") }
        _ = NSApplication.shared
        let model = EditorModel()
        await model.start(paths: paths())
        let raw = ProcessInfo.processInfo.environment["SPEKTRAFILM_RAW_SAMPLE"]
        let fixture = try raw.map { URL(fileURLWithPath: $0) } ?? makeJPEG(orientation: 1)
        model.open(fixture)
        model.border(\.enabled).wrappedValue = true
        model.border(\.style).wrappedValue = .film
        try await waitUntil(seconds: 60) { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        window.setContentSize(NSSize(width: 1320, height: 860))
        window.appearance = NSAppearance(named: .darkAqua)
        controller.showWindow(nil)
        defer { window.orderOut(nil); model.shutdown() }
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        func photoLayer() throws -> CALayer {
            func find(_ view: NSView) -> PhotographView? { (view as? PhotographView) ?? view.subviews.lazy.compactMap(find).first }
            let photo = try XCTUnwrap(find(window.contentView!))
            // The clip layer's one sublayer holds the photograph.
            let layers: [CALayer] = (photo.layer?.sublayers ?? []).flatMap { $0.sublayers ?? [] }
            return try XCTUnwrap(layers.first { $0.contentsGravity == .resize && $0.contents != nil })
        }
        func snapshot(_ name: String) throws -> CGRect {
            let view = window.contentView!
            view.layoutSubtreeIfNeeded()
            let bitmap = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: bitmap)
            try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: URL(fileURLWithPath: folder).appendingPathComponent("\(name).png"))
            let layer = try photoLayer()
            return layer.convert(layer.bounds, to: window.contentView!.layer)
        }
        try await Task.sleep(nanoseconds: 400_000_000)
        let developed = try snapshot("developed")
        XCTAssertTrue(try photoLayer().contents as AnyObject === model.image)
        model.holdOriginal(true)
        try await waitUntil(seconds: 20) { model.original != nil }
        try await Task.sleep(nanoseconds: 400_000_000)
        let original = try snapshot("original")
        XCTAssertTrue(try photoLayer().contents as AnyObject === model.original)
        XCTAssertEqual(original, developed)
        model.holdOriginal(false)
        try await Task.sleep(nanoseconds: 400_000_000)
        XCTAssertTrue(try photoLayer().contents as AnyObject === model.image)
    }

    private func waitUntil(seconds: TimeInterval = 20, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !predicate(), Date() < deadline { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertTrue(predicate(), "Timed out")
    }

    /// A 96 × 64 JPEG with the given EXIF orientation.
    private func makeJPEG(orientation: Int) throws -> URL {
        let context = CGContext(data: nil, width: 96, height: 64, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
        context.setFillColor(CGColor(red: 0.8, green: 0.4, blue: 0.2, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 96, height: 64))
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-original-\(UUID().uuidString).jpg")
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, context.makeImage()!, [kCGImagePropertyOrientation: orientation] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return url
    }
}
