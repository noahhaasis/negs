import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

final class PhotoExifTests: XCTestCase {
    private let sample = PhotoExif(make: "NIKON CORPORATION", model: "NIKON D750", lens: "50.0 mm f/1.8", iso: 400,
                                   exposure: 1.0 / 125, aperture: 2.8, focalLength: 50, taken: "2026:11:04 09:30:12")

    /// The camera and lens above, as film edge print is, without saying the make twice.
    func testTheTopLineNamesTheCameraAndLens() {
        XCTAssertEqual(sample.cameraLine, "NIKON D750   50.0 MM F/1.8")
        var canon = sample
        canon.make = "Canon"; canon.model = "EOS R5"; canon.lens = nil
        XCTAssertEqual(canon.cameraLine, "CANON EOS R5")
    }

    /// The exposure and date below.
    func testTheBottomLineGivesTheExposureAndDate() {
        XCTAssertEqual(sample.exposureLine, "ISO 400   1/125   f/2.8   50 MM   2026-11-04")
        XCTAssertEqual(PhotoExif.shutter(2), "2S")
        XCTAssertEqual(PhotoExif.shutter(0.004), "1/250")
    }

    func testMissingDetailsAreLeftOut() {
        var partial = PhotoExif(); partial.iso = 800
        XCTAssertEqual(partial.exposureLine, "ISO 800")
        XCTAssertEqual(partial.cameraLine, "")
        XCTAssertTrue(PhotoExif().isEmpty)
    }

    /// From a real file's metadata, and nothing from a file with none.
    func testReadingAFile() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-exif-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let context = try XCTUnwrap(CGContext(data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0,
                                              space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        func write(_ name: String, _ properties: [CFString: Any]) throws -> URL {
            let url = directory.appendingPathComponent(name)
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(destination, image, properties as CFDictionary)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
            return url
        }
        let tagged = try write("tagged.jpg", [
            kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "NIKON CORPORATION", kCGImagePropertyTIFFModel: "NIKON D750"],
            kCGImagePropertyExifDictionary: [kCGImagePropertyExifISOSpeedRatings: [400], kCGImagePropertyExifExposureTime: 0.008,
                                             kCGImagePropertyExifFNumber: 2.8, kCGImagePropertyExifFocalLength: 50,
                                             kCGImagePropertyExifDateTimeOriginal: "2026:11:04 09:30:12", kCGImagePropertyExifLensModel: "50.0 mm f/1.8"],
        ])
        let read = try XCTUnwrap(PhotoExif.read(tagged))
        XCTAssertEqual(read.cameraLine, "NIKON D750   50.0 MM F/1.8")
        XCTAssertEqual(read.exposureLine, "ISO 400   1/125   f/2.8   50 MM   2026-11-04")
        XCTAssertNil(PhotoExif.read(try write("plain.jpg", [:])))
    }

    /// The font ships with the app's data, and the name it's asked for by is its own.
    func testTheEdgePrintFontIsBundled() throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        EdgePrint.registerFont(data: root.appendingPathComponent("data"))
        XCTAssertEqual(EdgePrint.font(size: 14).fontName, EdgePrint.fontName)
    }
}

/// The lines sit outside a fitted photograph, on the canvas.
@MainActor final class ExifLinesTests: XCTestCase {
    private func canvas() throws -> (ImageScrollView, PhotographView) {
        _ = NSApplication.shared
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: 1000, height: 600))
        scroll.contentView = CenteringClipView()
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        let photo = PhotographView(frame: NSRect(x: 0, y: 0, width: 4000, height: 4000))
        scroll.documentView = photo
        scroll.tile()
        scroll.applyZoom()
        return (scroll, photo)
    }

    func testAFittedPhotographLeavesRoomForTheLines() throws {
        let (scroll, photo) = try canvas()
        XCTAssertEqual(scroll.magnification, 600.0 / 4000, accuracy: 0.001, "No details, no room kept")
        scroll.exif = PhotoExif(model: "D750", iso: 400)
        let margin = PinOverlayView.exifMargin
        XCTAssertEqual(scroll.magnification, (600 - margin * 2) / 4000, accuracy: 0.001)
        let overlay = scroll.overlay
        overlay.redraw()
        let rect = overlay.convert(photo.bounds, from: photo)
        let lines = try XCTUnwrap(overlay.exifFrames)
        XCTAssertLessThanOrEqual(lines.exposure.maxY, rect.minY, "Below it")
        XCTAssertGreaterThanOrEqual(lines.exposure.minY, 0, "On the canvas")
        XCTAssertEqual(lines.exposure.minX, rect.minX, accuracy: 1, "The exposure at its left")
        XCTAssertEqual(lines.camera.minY, lines.exposure.minY, accuracy: 0.5, "The camera on the same row")
        XCTAssertEqual(lines.camera.maxX, rect.maxX, accuracy: 1, "At its right")
        XCTAssertEqual(overlay.exifLines?.camera, "D750")
        XCTAssertEqual(overlay.exifLines?.exposure, "ISO 400")
    }

    /// When the canvas changes size, as when the film strip shows or hides, the
    /// lines stay against the photograph in that same pass, with nothing else moving.
    func testTheLinesFollowWhenTheCanvasResizes() throws {
        let (scroll, photo) = try canvas()
        scroll.exif = PhotoExif(model: "D750", iso: 400)
        scroll.overlay.redraw()
        for height: CGFloat in [420, 600, 300, 600] {
            scroll.setFrameSize(NSSize(width: 1000, height: height))
            scroll.layout()
            let overlay = scroll.overlay
            let rect = overlay.convert(photo.bounds, from: photo)
            let lines = try XCTUnwrap(overlay.exifFrames)
            XCTAssertEqual(overlay.bounds.height, height, accuracy: 0.5)
            XCTAssertEqual(lines.camera.maxY, rect.minY - PinOverlayView.exifGap, accuracy: 0.5, "Camera, at \(height)")
            XCTAssertEqual(lines.exposure.maxY, rect.minY - PinOverlayView.exifGap, accuracy: 0.5, "Below, at \(height)")
            XCTAssertEqual(lines.exposure.minX, rect.minX, accuracy: 0.5, "At its left, at \(height)")
        }
    }

    /// A photograph held at 1:1 doesn't change zoom as the canvas resizes, and only
    /// moves. The lines have to move with it, whether it's fitted or set by hand.
    func testTheLinesFollowAPhotographHeldAtOneToOne() throws {
        for fitted in [true, false] {
            _ = NSApplication.shared
            let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: 1000, height: 600))
            scroll.contentView = CenteringClipView()
            scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
            let photo = PhotographView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
            scroll.documentView = photo
            scroll.tile()
            scroll.applyZoom()
            scroll.exif = PhotoExif(model: "D750", iso: 400)
            if !fitted { scroll.setFitting(false); scroll.applyZoom() }
            scroll.overlay.redraw()
            for height: CGFloat in [420, 600, 340, 600] {
                scroll.setFrameSize(NSSize(width: 1000, height: height))
                scroll.layout()
                let overlay = scroll.overlay
                let rect = overlay.convert(photo.bounds, from: photo)
                let lines = try XCTUnwrap(overlay.exifFrames)
                XCTAssertEqual(rect.midY, height / 2, accuracy: 1, "Still centred, fitted \(fitted), at \(height)")
                XCTAssertEqual(lines.camera.maxY, rect.minY - PinOverlayView.exifGap, accuracy: 0.5, "Above, fitted \(fitted), at \(height)")
                XCTAssertEqual(lines.exposure.maxY, rect.minY - PinOverlayView.exifGap, accuracy: 0.5, "Below, fitted \(fitted), at \(height)")
            }
        }
    }

    func testZoomedInTheLinesStayOnTheCanvas() throws {
        let (scroll, _) = try canvas()
        scroll.exif = PhotoExif(model: "D750", iso: 400)
        scroll.setFitting(false)
        scroll.magnification = 1
        scroll.overlay.redraw()
        let lines = try XCTUnwrap(scroll.overlay.exifFrames)
        XCTAssertLessThanOrEqual(lines.camera.maxY, scroll.overlay.bounds.height)
        XCTAssertGreaterThanOrEqual(lines.exposure.minY, 0)
    }
}
