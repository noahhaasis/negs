import AppKit
import CoreText
import XCTest
@testable import NegsApp

/// The T2 · Label tokens (Theme.swift): the bundled Newsreader loads and
/// keeps its axes, every text colour meets the contrast rules, and the pencil
/// marks stay inside the boxes they're drawn in.
final class ThemeTests: XCTestCase {
    func testNewsreaderLoadsWithBothAxes() throws {
        let directory = try XCTUnwrap(Theme.Fonts.directory(), "macos/Fonts holds the subset fonts")
        for name in [Theme.Fonts.upright, Theme.Fonts.italic, "OFL-Newsreader.txt"] {
            XCTAssertTrue(FileManager.default.fileExists(atPath: directory.appendingPathComponent(name).path), name)
        }
        XCTAssertTrue(Theme.Fonts.load(in: directory))
        XCTAssertTrue(Theme.Fonts.load(in: directory), "Loading twice is harmless")
        XCTAssertTrue(Theme.Fonts.loaded)

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

    /// Launch reads the fonts on another thread; the first font asked for
    /// waits for them rather than falling back.
    func testFirstFontWaitsForLaunchRead() {
        Theme.Fonts.prepare()
        Theme.Fonts.prepare()
        XCTAssertTrue((CTFontCopyFamilyName(Theme.serifFont(31, weight: 500)) as String).hasPrefix("Newsreader"))
        XCTAssertTrue(Theme.Fonts.loaded)
    }

    func testSubsetCoversEuropeanFileNames() throws {
        XCTAssertTrue(Theme.Fonts.load())
        let font = Theme.serifFont(15)
        for text in ["Łódź", "Ağrı", "Říčany", "Ærøskøbing", "f/2,8 · 1/1250 · −0,65 € …", "“Istanbul 2026”"] {
            var characters = Array(text.utf16)
            var glyphs = [CGGlyph](repeating: 0, count: characters.count)
            XCTAssertTrue(CTFontGetGlyphsForCharacters(font, &characters, &glyphs, characters.count), text)
        }
    }

    func testFiguresAreTabular() throws {
        XCTAssertTrue(Theme.Fonts.load())
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

    /// Round 28: in every theme, words reach 4.5 : 1 and marks 3 : 1 on
    /// what they're read against: the panel, the sheet, the grid and (with
    /// the film's own inks) the film; Export's words on its fill.
    func testEveryThemeMeetsContrast() {
        for theme in AppTheme.allCases {
            let h = theme.palette.hex, f = theme.palette.filmHex
            func check(_ ink: UInt32, on bg: UInt32, _ floor: Double, _ what: String) {
                XCTAssertGreaterThanOrEqual(Theme.contrast(ink, bg), floor, String(format: "\(theme.title): \(what) %06x on %06x", ink, bg))
            }
            for (name, ink) in [("dim", h.dim), ("label", h.label), ("paper", h.paper), ("warn", h.warn)] {
                for bg in [h.panel, h.bg, h.grid] { check(ink, on: bg, 4.5, name) }
            }
            for (name, ink) in [("dim", f.dim), ("label", f.label), ("paper", f.paper)] { check(ink, on: h.film, 4.5, "film " + name) }
            for bg in [h.panel, h.bg, h.grid] { check(h.accent, on: bg, 3, "pencil") }
            check(f.accent, on: h.film, 3, "film pencil")
            check(h.onFill, on: h.fill, 4.5, "Export's words")
            XCTAssertNotEqual(h.panel, h.bg, "\(theme.title): the panel stands apart from the sheet")
            if !theme.palette.dark {
                XCTAssertLessThan(Theme.luminance(h.panel), Theme.luminance(h.bg), "\(theme.title): the panel is a tone darker than the sheet (round 28)")
            }
        }
        XCTAssertEqual(AppTheme.darkroom.palette.hex, AppTheme.darkroom.palette.filmHex, "Darkroom's film inks are its own")
    }

    /// The stored choice: Darkroom by default and for anything unknown;
    /// choosing stores it and tells AppKit's views.
    @MainActor func testChoosingAThemeStoresItAndNotifies() {
        let defaults = UserDefaults(suiteName: "ThemeTests.choose")!
        defaults.removePersistentDomain(forName: "ThemeTests.choose")
        XCTAssertEqual(AppTheme.stored(defaults), .darkroom)
        defaults.set("neon", forKey: AppTheme.key)
        XCTAssertEqual(AppTheme.stored(defaults), .darkroom)
        let posted = expectation(forNotification: Theme.didChange, object: nil)
        ThemeStore.shared.choose(.daylight, defaults: defaults)
        wait(for: [posted], timeout: 1)
        XCTAssertEqual(AppTheme.stored(defaults), .daylight)
        XCTAssertEqual(Theme.Hex.panel, 0xd8d5cf)
        XCTAssertEqual(Theme.colorScheme, .light)
        ThemeStore.shared.choose(.darkroom, defaults: defaults)
        XCTAssertEqual(Theme.Hex.panel, 0x161513)
        XCTAssertEqual(Theme.colorScheme, .dark)
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
