import AppKit
import CoreText
import XCTest
@testable import NegsApp

/// The T2 · Label tokens (Theme.swift): the bundled Newsreader registers and
/// keeps its axes, every text colour meets the contrast rules, and the pencil
/// marks stay inside the boxes they're drawn in.
final class ThemeTests: XCTestCase {
    func testNewsreaderRegistersWithBothAxes() throws {
        let directory = try XCTUnwrap(Theme.Fonts.directory(), "macos/Fonts holds the subset fonts")
        for name in [Theme.Fonts.upright, Theme.Fonts.italic, "OFL-Newsreader.txt"] {
            XCTAssertTrue(FileManager.default.fileExists(atPath: directory.appendingPathComponent(name).path), name)
        }
        XCTAssertTrue(Theme.Fonts.register(in: directory))
        XCTAssertTrue(Theme.Fonts.register(in: directory), "Registering twice is harmless")
        XCTAssertTrue(Theme.Fonts.registered)

        let font = Theme.serifFont(24, weight: 600)
        XCTAssertTrue((CTFontCopyFamilyName(font) as String).hasPrefix("Newsreader"))
        let axes = (CTFontCopyVariationAxes(font) as? [[String: Any]] ?? [])
            .compactMap { $0[kCTFontVariationAxisIdentifierKey as String] as? Int }
        XCTAssertEqual(Set(axes), [Int(Theme.Fonts.axis("wght")), Int(Theme.Fonts.axis("opsz"))])
        let variation = CTFontCopyVariation(font) as? [NSNumber: NSNumber] ?? [:]
        XCTAssertEqual(variation[NSNumber(value: Theme.Fonts.axis("wght"))]?.doubleValue, 600)
        XCTAssertEqual(variation[NSNumber(value: Theme.Fonts.axis("opsz"))]?.doubleValue, 24, "Optical size follows the point size")
        XCTAssertEqual(CTFontGetSize(font), 24)

        let italic = Theme.serifFont(13, italic: true)
        XCTAssertTrue(CTFontGetSymbolicTraits(italic).contains(.traitItalic))
        XCTAssertTrue(Theme.serifFont(13, italic: true) === italic, "Fonts are cached")
        // Weights outside the subset's 400–700 are clamped, not left to Core Text.
        let heavy = CTFontCopyVariation(Theme.serifFont(14, weight: 800)) as? [NSNumber: NSNumber] ?? [:]
        XCTAssertEqual(heavy[NSNumber(value: Theme.Fonts.axis("wght"))]?.doubleValue, 700)
    }

    func testSubsetCoversEuropeanFileNames() throws {
        XCTAssertTrue(Theme.Fonts.register())
        let font = Theme.serifFont(15)
        for text in ["Łódź", "Ağrı", "Říčany", "Ærøskøbing", "f/2,8 · 1/1250 · −0,65 € …", "“Istanbul 2026”"] {
            var characters = Array(text.utf16)
            var glyphs = [CGGlyph](repeating: 0, count: characters.count)
            XCTAssertTrue(CTFontGetGlyphsForCharacters(font, &characters, &glyphs, characters.count), text)
        }
    }

    func testFiguresAreTabular() throws {
        XCTAssertTrue(Theme.Fonts.register())
        let font = Theme.serifFont(14.5)
        let settings = CTFontDescriptorCopyAttribute(CTFontCopyFontDescriptor(font), kCTFontFeatureSettingsAttribute) as? [[String: Any]] ?? []
        let tags = settings.compactMap { $0[kCTFontOpenTypeFeatureTag as String] as? String }
        // Newsreader's figures are lining only, so Core Text drops `lnum`
        // (and would drop `onum`) as a feature the face doesn't have.
        XCTAssertTrue(tags.contains("tnum"), "\(tags)")
        func width(_ text: String) -> CGFloat { (text as NSString).size(withAttributes: [.font: font as NSFont]).width }
        XCTAssertEqual(width("1111"), width("0000"), accuracy: 0.01)
        XCTAssertEqual(width("1984"), width("3656"), accuracy: 0.01)
    }

    /// §1: body text at least 4.5:1 on what it's read against.
    func testTextColoursMeetContrast() {
        for text in [Theme.Hex.dim, Theme.Hex.label, Theme.Hex.paper, Theme.Hex.warn] {
            for background in [Theme.Hex.panel, Theme.Hex.bg, Theme.Hex.film] {
                XCTAssertGreaterThanOrEqual(Theme.contrast(text, background), 4.5, String(format: "%06x on %06x", text, background))
            }
        }
        XCTAssertGreaterThanOrEqual(Theme.contrast(Theme.Hex.onAccent, Theme.Hex.accent), 4.5, "Export's words on the pencil")
        // The spec's table (§2.1): dim 5.3 on the panel, paper 16.6 on the sheet.
        XCTAssertEqual(Theme.contrast(Theme.Hex.dim, Theme.Hex.panel), 5.3, accuracy: 0.1)
        XCTAssertEqual(Theme.contrast(Theme.Hex.paper, Theme.Hex.bg), 16.6, accuracy: 0.1)
        XCTAssertEqual(Theme.contrast(Theme.Hex.warn, Theme.Hex.panel), 7.9, accuracy: 0.1)
    }

    func testPencilStrokeSpansItsWord() {
        let rect = CGRect(x: 10, y: 20, width: 80, height: 6)
        let bounds = PencilStroke.cgPath(in: rect).boundingBoxOfPath
        XCTAssertEqual(bounds.minX, 10 + 0.8, accuracy: 0.01)
        XCTAssertEqual(bounds.maxX, 10 + 79.2, accuracy: 0.01)
        XCTAssertGreaterThanOrEqual(bounds.minY, rect.minY)
        XCTAssertLessThanOrEqual(bounds.maxY, rect.maxY)
    }

    func testRingScalesItsCornersLikeCSS() {
        // Big enough: the radii are used as given.
        let large = PencilRing.cgPath(in: CGRect(x: 0, y: 0, width: 200, height: 120)).boundingBoxOfPath
        XCTAssertEqual(large, CGRect(x: 0, y: 0, width: 200, height: 120))
        // A 40 pt tall box can't fit 34 + 28 down its left side, so every
        // radius shrinks by the same factor and the shape still closes.
        let small = CGRect(x: 0, y: 0, width: 60, height: 40)
        let path = PencilRing.cgPath(in: small)
        XCTAssertEqual(path.boundingBoxOfPath.width, small.width, accuracy: 0.01)
        XCTAssertEqual(path.boundingBoxOfPath.height, small.height, accuracy: 0.01)
        XCTAssertFalse(path.contains(CGPoint(x: 0.5, y: 0.5)), "The corner is cut away")
        XCTAssertTrue(path.contains(CGPoint(x: 30, y: 20)))
    }
}
