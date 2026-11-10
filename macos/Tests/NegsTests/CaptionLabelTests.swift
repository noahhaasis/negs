import AppKit
import ImageIO
import XCTest
@testable import NegsApp

/// The caption over the photograph and the label under it (IMPLEMENTATION.md
/// §4.1, §4.2): the camera's facts as written, the label's one-or-two-rows
/// rule, and the room the canvas keeps for them.
@MainActor final class CaptionLabelTests: XCTestCase {
    // MARK: Caption

    func testCameraNameDropsTheRepeatedMaker() {
        XCTAssertEqual(PhotoFacts.camera(make: "NIKON CORPORATION", model: "NIKON Z f"), "Nikon Z f")
        XCTAssertEqual(PhotoFacts.camera(make: "NIKON CORPORATION", model: "NIKON Z 6"), "Nikon Z 6")
        XCTAssertEqual(PhotoFacts.camera(make: "Canon", model: "Canon EOS R5"), "Canon EOS R5")
        XCTAssertEqual(PhotoFacts.camera(make: "SONY", model: "ILCE-7M3"), "Sony ILCE-7M3")
        XCTAssertEqual(PhotoFacts.camera(make: "FUJIFILM", model: "X-T4"), "Fujifilm X-T4")
        XCTAssertEqual(PhotoFacts.camera(make: "LEICA CAMERA AG", model: "LEICA Q2"), "Leica Q2")
        XCTAssertEqual(PhotoFacts.camera(make: nil, model: "iPhone 15 Pro"), "iPhone 15 Pro")
        XCTAssertEqual(PhotoFacts.camera(make: "Apple", model: nil), "Apple")
        XCTAssertNil(PhotoFacts.camera(make: nil, model: nil))
        XCTAssertNil(PhotoFacts.camera(make: nil, model: "  "))
    }

    func testShutterSpeeds() {
        XCTAssertEqual(PhotoFacts.shutter(1.0 / 1250), "1/1250")
        XCTAssertEqual(PhotoFacts.shutter(1.0 / 640), "1/640")
        XCTAssertEqual(PhotoFacts.shutter(0.5), "1/2", "A whole fraction stays a fraction")
        let decimal = 1.3.formatted(.number.precision(.fractionLength(0...1)))
        XCTAssertEqual(PhotoFacts.shutter(1.3), "\(decimal) s")
        XCTAssertEqual(PhotoFacts.shutter(2), "2 s")
        XCTAssertEqual(PhotoFacts.shutter(30), "30 s")
    }

    func testCaptionItemsInOrderSkippingWhatsMissing() {
        let aperture = 2.8.formatted(.number.precision(.fractionLength(0...1)))
        let facts = PhotoFacts(properties: [
            kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "NIKON CORPORATION", kCGImagePropertyTIFFModel: "NIKON Z f"] as [CFString: Any],
            kCGImagePropertyExifDictionary: [kCGImagePropertyExifISOSpeedRatings: [160], kCGImagePropertyExifExposureTime: 0.0008,
                                             kCGImagePropertyExifFNumber: 2.8, kCGImagePropertyExifFocalLength: 28] as [CFString: Any]
        ])
        XCTAssertEqual(facts.items, ["Nikon Z f", "ISO 160", "1/1250", "f/\(aperture)", "28 mm"])
        XCTAssertEqual(PhotoFacts(properties: [:]).items, [], "A PNG without EXIF has only its file name")
        XCTAssertEqual(PhotoFacts(properties: [kCGImagePropertyExifDictionary: [kCGImagePropertyExifFNumber: 8, kCGImagePropertyExifFocalLength: 0]
                                               as [CFString: Any]]).items, ["f/8"], "A zero focal length is missing, not 0 mm")
    }

    func testFactsAreReadFromTheFile() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-caption-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("tagged.jpg")
        let context = try XCTUnwrap(CGContext(data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0,
                                              space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, [
            kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "FUJIFILM", kCGImagePropertyTIFFModel: "X100V"],
            kCGImagePropertyExifDictionary: [kCGImagePropertyExifISOSpeedRatings: [400], kCGImagePropertyExifExposureTime: 0.004]
        ] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let facts = try XCTUnwrap(PhotoFacts.read(url))
        XCTAssertEqual(facts.items, ["Fujifilm X100V", "ISO 400", "1/250"])
        XCTAssertNil(PhotoFacts.read(directory.appendingPathComponent("missing.jpg")))
    }

    // MARK: Label

    func testLabelWrapsWhenTheNamesDontFitUnderThePhotograph() throws {
        let directory = try XCTUnwrap(Theme.Fonts.directory())
        XCTAssertTrue(Theme.Fonts.load(in: directory))
        let kind = PrintLabelLayout.kind(scanning: false)
        let gold = "Kodak Gold 200", endura = "Kodak Professional Portra Endura"
        let pair = PrintLabelLayout.filmWidth(gold) + PrintLabelLayout.gap + PrintLabelLayout.paperWidth(endura, kind: kind)
        XCTAssertTrue(PrintLabelLayout.fitsOneRow(film: gold, paper: endura, kind: kind, width: 760), "\(pair) pt")
        XCTAssertFalse(PrintLabelLayout.fitsOneRow(film: gold, paper: endura, kind: kind, width: pair - 1))
        XCTAssertTrue(PrintLabelLayout.fitsOneRow(film: gold, paper: endura, kind: kind, width: pair))

        // The longest real pair (the spec estimated about 1060 pt from the
        // mockup; it measures about 860 here). At 980 × 560 the canvas is 640
        // wide, so it takes two rows, and each row fits inside the canvas's margins.
        let doubleX = "Kodak Double-X Negative Film 5222", grade2 = "Ilford Multigrade RC Deluxe Grade 2 (experimental)"
        let longest = PrintLabelLayout.filmWidth(doubleX) + PrintLabelLayout.gap + PrintLabelLayout.paperWidth(grade2, kind: kind)
        XCTAssertGreaterThan(longest, 800)
        let canvas: CGFloat = 980 - Theme.Size.inspectorSmallest
        let photo = canvas - ImageScrollView.fitInset * 2
        XCTAssertFalse(PrintLabelLayout.fitsOneRow(film: doubleX, paper: grade2, kind: kind, width: photo))
        XCTAssertLessThan(PrintLabelLayout.filmWidth(doubleX), photo, "The film's row fits under the photograph")
        XCTAssertLessThan(PrintLabelLayout.paperWidth(grade2, kind: kind), canvas - ImageScrollView.fitInset * 2, "So does the paper's, uncut")

        // Scanning swaps "printed on" for the longer reason.
        let scanning = PrintLabelLayout.kind(scanning: true)
        XCTAssertEqual(scanning, "not printed — scanning the film")
        XCTAssertGreaterThan(PrintLabelLayout.paperWidth(endura, kind: scanning), PrintLabelLayout.paperWidth(endura, kind: kind))
    }

    func testTheBandUnderThePhotographHoldsTwoRows() throws {
        let directory = try XCTUnwrap(Theme.Fonts.directory())
        XCTAssertTrue(Theme.Fonts.load(in: directory))
        // Padding 8, the film's line (24 × 1.22 + 3) and the paper's (17.5 × 1.12 + 5).
        let film = CTFontGetAscent(Theme.serifFont(24, weight: 600)) + CTFontGetDescent(Theme.serifFont(24, weight: 600))
        let paper = CTFontGetAscent(Theme.serifFont(17.5)) + CTFontGetDescent(Theme.serifFont(17.5))
        XCTAssertLessThanOrEqual(8 + film + 5 + paper + 5, PrintLabelLayout.band)
        XCTAssertEqual(CanvasArea.captionBand, 54)
        XCTAssertEqual(CanvasArea.labelBand, 68)
    }

    // MARK: The canvas keeps room for them

    func testFittingLeavesTheMarginsBesideThePhotograph() {
        let viewport = CGSize(width: 1000, height: 600)
        XCTAssertEqual(ImageScrollView.fitViewport(viewport), CGSize(width: 904, height: 600))
        let amount = ImageScrollView.magnification(fitting: true, document: CGSize(width: 6000, height: 4000),
                                                   viewport: ImageScrollView.fitViewport(viewport), backingScale: 2)
        XCTAssertEqual(amount * 6000, 900, accuracy: 0.5, "Height-limited: 600 tall, so 900 wide, inside the 904")
        let wide = ImageScrollView.magnification(fitting: true, document: CGSize(width: 6000, height: 2000),
                                                 viewport: ImageScrollView.fitViewport(viewport), backingScale: 2)
        XCTAssertEqual(wide * 6000, 904, accuracy: 0.5, "Width-limited: 48 pt each side")
    }

    func testGeometryReportsThePrintOnScreen() throws {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 600), styleMask: [.titled], backing: .buffered, defer: false)
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: 1000, height: 600))
        scroll.contentView = CenteringClipView()
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        scroll.documentView = NSView(frame: NSRect(x: 0, y: 0, width: 3000, height: 1000))
        window.contentView = scroll
        let geometry = CanvasGeometry()
        scroll.geometry = geometry
        scroll.fitting = true
        scroll.applyZoom()
        scroll.layoutSubtreeIfNeeded()
        scroll.reportGeometry()
        // 3000 × 1000 fitted into 904 × 600: 904 × 301, centred.
        XCTAssertEqual(geometry.print.width, 904, accuracy: 1.5)
        XCTAssertEqual(geometry.print.height, 302, accuracy: 1.5)
        XCTAssertEqual(geometry.print.minX, 48, accuracy: 1.5)
        XCTAssertEqual(geometry.print.midY, 300, accuracy: 1.5)
        XCTAssertFalse(geometry.zoomed)

        // Zoomed in, the print fills the viewport and stays put while it scrolls.
        scroll.fitting = false
        scroll.setMagnification(1, centeredAt: NSPoint(x: 1500, y: 500))
        scroll.reportGeometry()
        XCTAssertTrue(geometry.zoomed)
        XCTAssertEqual(geometry.print, CGRect(x: 0, y: 0, width: 1000, height: 600))
        scroll.contentView.scroll(to: NSPoint(x: 200, y: 100))
        scroll.reflectScrolledClipView(scroll.contentView)
        XCTAssertEqual(geometry.print, CGRect(x: 0, y: 0, width: 1000, height: 600))
    }

    // MARK: Development time, moved from beside the stocks to the Lab

    func testDevelopmentTimeMenu() {
        let stock = Stock(.object(["id": .string("kodak_gold_200"), "name": .string("Kodak Gold 200"),
                                   "development_times": .array([.number(3.25), .number(4), .number(5.5)])]))
        let items = InspectorEdits.developmentItems(stock)
        XCTAssertEqual(items.count, 4)
        XCTAssertEqual(items.first, ChoiceItem.option("Profile default", tag: ""))
        XCTAssertEqual(InspectorEdits.developmentTag(.null), "", "Unset is the profile's default")
        XCTAssertEqual(InspectorEdits.developmentTag(.number(0)), "")
        let tag = InspectorEdits.developmentTag(.number(4))
        XCTAssertTrue(items.contains { if case .option(_, let t) = $0 { return t == tag }; return false }, "A chosen time is ticked in the menu")
        XCTAssertEqual(Double(tag), 4)
    }
}
