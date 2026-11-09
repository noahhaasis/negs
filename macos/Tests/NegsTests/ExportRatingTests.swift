import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

/// `xmp:Rating` goes into an export without touching its pixels.
final class ExportRatingTests: XCTestCase {
    private func image(bits: Int) throws -> CGImage {
        let space = CGColorSpace(name: CGColorSpace.displayP3)!
        let (width, height) = (64, 48)
        let info = bits == 16 ? CGImageAlphaInfo.noneSkipLast.rawValue | CGBitmapInfo.byteOrder16Little.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue
        let context = try XCTUnwrap(CGContext(data: nil, width: width, height: height, bitsPerComponent: bits, bytesPerRow: 0, space: space, bitmapInfo: info))
        for y in 0..<height { for x in 0..<width {
            context.setFillColor(CGColor(colorSpace: space, components: [CGFloat(x) / 64, CGFloat(y) / 48, CGFloat((x * y) % 97) / 97, 1])!)
            context.fill(CGRect(x: x, y: y, width: 1, height: 1))
        } }
        return try XCTUnwrap(context.makeImage())
    }

    private func pixels(_ url: URL) throws -> (Data, Data?) {
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        return (try XCTUnwrap(image.dataProvider?.data as Data?), image.colorSpace?.copyICCData() as Data?)
    }

    func testRatingIsAddedWithoutChangingPixels() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("negs-rating-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        for (type, bits, ext) in [(UTType.jpeg, 8, "jpg"), (.png, 8, "png"), (.png, 16, "png"), (.tiff, 16, "tif"), (.tiff, 8, "tif")] {
            let url = dir.appendingPathComponent("\(bits)-\(UUID().uuidString).\(ext)")
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, try image(bits: bits), nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
            let before = try pixels(url)
            XCTAssertNil(ExportRating.read(url))
            try ExportRating.embed(4, in: url)
            XCTAssertEqual(ExportRating.read(url), 4, "\(ext) \(bits)-bit")
            let after = try pixels(url)
            XCTAssertEqual(after.0, before.0, "\(ext) \(bits)-bit pixels")
            XCTAssertEqual(after.1, before.1, "\(ext) \(bits)-bit profile")
            try ExportRating.embed(-1, in: url)
            XCTAssertEqual(ExportRating.read(url), -1, "replaced")
            XCTAssertEqual(try pixels(url).0, before.0)
        }
        XCTAssertEqual(ExportRating.crc32(Data("IEND".utf8)), 0xAE42_6082)
        XCTAssertEqual(ExportRating.value(LibrarySummary(rating: 3)), 3)
        XCTAssertEqual(ExportRating.value(LibrarySummary(rating: 3, flag: .reject)), -1)
        XCTAssertNil(ExportRating.value(LibrarySummary(flag: .pick)))
    }

    func testBatchNames() {
        var options = BatchOptions()
        XCTAssertEqual(options.fileName(name: "IMG_0412 (2)", film: "kodak_gold_200"), "IMG_0412 (2)_kodak_gold_200.jpeg")
        options.naming = .name; options.format = OutputFormat.tiff.rawValue
        XCTAssertEqual(options.fileName(name: "IMG_0412", film: "x"), "IMG_0412.tiff")
        options.naming = .custom; options.custom = "trip/{film}-{name}"
        XCTAssertEqual(options.fileName(name: "A", film: "f"), "trip-f-A.tiff")
        XCTAssertEqual(BatchNames.unique(["a.jpg", "b.jpg", "A.jpg", "a.jpg"]), ["a.jpg", "b.jpg", "A 2.jpg", "a 3.jpg"])
        XCTAssertEqual(BatchNames.numbered("a.jpg") { ["a.jpg", "a 2.jpg"].contains($0) }, "a 3.jpg")
    }
}
