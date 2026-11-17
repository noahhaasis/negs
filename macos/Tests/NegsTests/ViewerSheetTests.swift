import SwiftUI
import XCTest
@testable import NegsApp

final class ViewerSheetTests: XCTestCase {
    /// Darkroom is today's sheet: the canvas draws with `Theme`'s tokens, to the byte.
    func testDarkroomInksAreTheThemeTokens() {
        let ink = ViewerSheet.darkroom.ink
        XCTAssertEqual(ViewerSheet.darkroom.hex, Theme.Hex.bg)
        XCTAssertEqual(ink, .themes)
        XCTAssertEqual(ViewerSheet.theme.ink, ink, "on Darkroom, the theme's own sheet is Darkroom")
        XCTAssertEqual(ink.hex, SheetInk.Hex(sheet: Theme.Hex.bg, paper: Theme.Hex.paper, label: Theme.Hex.label, dim: Theme.Hex.dim,
                                             track: Theme.Hex.track, line: Theme.Hex.line, accent: Theme.Hex.accent))
        XCTAssertTrue(ink.light)
        XCTAssertEqual(EnvironmentValues().sheetInk, .themes, "outside the canvas, Theme's inks")
    }

    /// Each theme's own sheet draws with that theme's tokens exactly; the
    /// Darkroom sheet keeps Darkroom's inks under a light theme, with the
    /// theme's pencil where it holds 3 : 1.
    @MainActor func testThemesOwnSheetUsesItsTokens() {
        let defaults = UserDefaults(suiteName: "ViewerSheetTests.themes")!
        defer { ThemeStore.shared.choose(.darkroom, defaults: defaults) }
        for theme in AppTheme.allCases {
            ThemeStore.shared.choose(theme, defaults: defaults)
            let t = theme.palette.hex, ink = ViewerSheet.theme.ink
            XCTAssertEqual(ink.hex, SheetInk.Hex(sheet: t.bg, paper: t.paper, label: t.label, dim: t.dim, track: t.track, line: t.line, accent: t.accent), theme.title)
            XCTAssertEqual(ink.light, theme.palette.dark, theme.title)
            XCTAssertEqual(ink.highlighter, theme == .lightTable, theme.title)
            let dark = ViewerSheet.darkroom.ink.hex, d = AppTheme.darkroom.palette.hex
            XCTAssertEqual([dark.paper, dark.label, dark.dim], [d.paper, d.label, d.dim], theme.title)
            XCTAssertGreaterThanOrEqual(Theme.contrast(dark.sheet, dark.accent), 3, theme.title)
        }
    }

    /// The inks design/proposals/r27-sheets.html shows (its `inksFor`).
    func testInksMatchTheProposal() {
        let expected: [ViewerSheet: (light: Bool, inks: [UInt32])] = [
            .black: (true, [0xebe4d6, 0xb9b0a1, 0x8c8578, 0x424039, 0x25241f, 0xffd23f]),
            .charcoal: (true, [0xffffff, 0xd0c7b8, 0xa09a8b, 0x55524a, 0x3b3934, 0xffd23f]),
            .grey22: (true, [0xffffff, 0xd5cdbd, 0xaaa294, 0x636058, 0x4b4944, 0xffd23f]),
            .middleGrey: (false, [0x000000, 0x0b0601, 0x0b0601, 0x605c55, 0x6e6b67, 0xffd23f]),
            .lightGrey: (false, [0x000000, 0x342d21, 0x4c463a, 0x88847d, 0xa19e99, 0xbb2214]),
            .paper: (false, [0x0f0a03, 0x3e382b, 0x615a4d, 0xaba79f, 0xcfccc6, 0xc83122]),
        ]
        for (sheet, want) in expected {
            let ink = sheet.ink.hex
            XCTAssertEqual(sheet.ink.light, want.light, sheet.title)
            XCTAssertEqual([ink.paper, ink.label, ink.dim, ink.track, ink.line, ink.accent].map { String($0, radix: 16) },
                           want.inks.map { String($0, radix: 16) }, sheet.title)
        }
    }

    /// Words reach 4.5 : 1 (middle grey can't quite: as near as it allows);
    /// the pencil 3 : 1; the ramp keeps its order.
    func testEverySheetsInksRead() {
        for sheet in ViewerSheet.allCases {
            let ink = sheet.ink.hex
            let words = sheet == .middleGrey ? 4.4 : 4.5
            for (name, hex) in [("paper", ink.paper), ("label", ink.label), ("dim", ink.dim)] {
                XCTAssertGreaterThanOrEqual(Theme.contrast(sheet.hex, hex), words, "\(sheet.title) \(name)")
            }
            XCTAssertGreaterThanOrEqual(Theme.contrast(sheet.hex, ink.accent), 3, "\(sheet.title) pencil")
            let ramp = [ink.paper, ink.label, ink.dim, ink.track, ink.line].map { Theme.contrast(sheet.hex, $0) }
            XCTAssertEqual(ramp, ramp.sorted(by: >), sheet.title)
            XCTAssertGreaterThan(ramp[4], 1.1, "\(sheet.title) line shows")
        }
    }

    func testStoredChoiceRoundTrips() {
        for sheet in ViewerSheet.allCases { XCTAssertEqual(ViewerSheet(rawValue: sheet.rawValue), sheet) }
        XCTAssertEqual(Set(ViewerSheet.allCases.map(\.title)).count, ViewerSheet.allCases.count)
    }

    func testOKLCHLightness() {
        XCTAssertEqual(OKLCH.lightness(0x000000), 0, accuracy: 1e-6)
        XCTAssertEqual(OKLCH.lightness(0xffffff), 1, accuracy: 1e-4)
        XCTAssertEqual(OKLCH.color(lightness: 1, chroma: 0, hue: 0), 0xffffff)
        XCTAssertEqual(OKLCH.color(lightness: 0, chroma: 0.1, hue: 85), 0x000000)
    }
}
