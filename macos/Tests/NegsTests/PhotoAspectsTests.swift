import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

/// Each photograph's shape for the strip's frames and the grid's rows
/// (§4.4, §4.6): read from the header, turned by its orientation, and
/// replaced by the thumbnail actually shown.
@MainActor final class PhotoAspectsTests: XCTestCase {
    private var folder: URL!

    override func setUpWithError() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("aspects-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: folder) }

    private func image(width: Int, height: Int) -> CGImage {
        let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
        return context.makeImage()!
    }

    /// A JPEG `width` × `height` pixels, stored with EXIF `orientation`.
    private func jpeg(_ name: String, width: Int, height: Int, orientation: Int = 1) -> URL {
        let url = folder.appendingPathComponent(name)
        let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, image(width: width, height: height),
                                   [kCGImagePropertyOrientation: orientation] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return url
    }

    func testTheHeaderGivesTheUprightShape() throws {
        let landscape = jpeg("a.jpg", width: 60, height: 40)
        let turned = jpeg("b.jpg", width: 60, height: 40, orientation: 6)
        XCTAssertEqual(try XCTUnwrap(PhotoAspects.header(of: landscape)), 1.5, accuracy: 0.001)
        XCTAssertEqual(try XCTUnwrap(PhotoAspects.header(of: turned)), 40.0 / 60, accuracy: 0.001, "orientations 5–8 stand it on its side")
        XCTAssertNil(PhotoAspects.header(of: folder.appendingPathComponent("missing.jpg")))
    }

    func testOutwardFromTheCurrentPhotograph() {
        XCTAssertEqual(PhotoAspects.outward(6, from: 2), [2, 3, 1, 4, 0, 5])
        XCTAssertEqual(PhotoAspects.outward(3, from: 9), [2, 1, 0], "a centre past the end starts at the end")
        XCTAssertEqual(PhotoAspects.outward(0, from: 0), [])
    }

    func testAFolderIsReadAndAShownThumbnailHasTheLastWord() async throws {
        let wide = jpeg("wide.jpg", width: 80, height: 40), tall = jpeg("tall.jpg", width: 30, height: 60)
        let aspects = PhotoAspects()
        XCTAssertEqual(aspects.ratio(of: wide), PhotoAspects.fallback, "3:2 until it's known")
        // An edited thumbnail, cropped square, shown before the header is read.
        aspects.note(tall, image: image(width: 50, height: 50))
        aspects.read([PhotoFile(url: wide), PhotoFile(url: tall)], around: tall)
        for _ in 0..<200 where aspects.ratios.count < 2 { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertEqual(aspects.ratio(of: wide), 2, accuracy: 0.001)
        XCTAssertEqual(aspects.ratio(of: tall), 1, accuracy: 0.001, "the header doesn't undo what's shown")
        // A duplicate is read from its photograph's file.
        let duplicate = PhotoFile(url: wide).duplicate("v2", number: 2)
        aspects.read([duplicate], around: nil)
        for _ in 0..<200 where aspects.ratios[duplicate.url] == nil { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertEqual(aspects.ratio(of: duplicate.url), 2, accuracy: 0.001)
    }
}
