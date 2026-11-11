import Foundation
import SwiftUI

/// The sheet the photograph lies on in the viewer, chosen in Settings ›
/// Photographs › Viewer from a few (design/proposals/r27-sheets.html).
/// Darkroom is `bg`. The title band, the inspector, the strip and the grid
/// stay dark whichever is chosen.
enum ViewerSheet: String, CaseIterable, Identifiable, Sendable {
    case darkroom, black, charcoal, grey22, middleGrey, lightGrey, paper

    /// The choice in the user defaults.
    static let key = "viewerSheet"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .darkroom: "Darkroom"
        case .black: "Black"
        case .charcoal: "Charcoal"
        case .grey22: "22% grey"
        case .middleGrey: "Middle grey"
        case .lightGrey: "Light grey"
        case .paper: "Paper"
        }
    }

    /// 0xRRGGBB, sRGB.
    var hex: UInt32 {
        switch self {
        case .darkroom: Theme.Hex.bg
        case .black: 0x000000
        // Lightroom's and Photoshop's Dark Gray, about.
        case .charcoal: 0x242424
        // 22% in Lightroom's readout.
        case .grey22: 0x383838
        // The 18% grey card, L* 50: ISO 3664's surround for judging prints.
        case .middleGrey: 0x777777
        // Lightroom's Light Gray, about.
        case .lightGrey: 0xb4b4b4
        case .paper: Theme.Hex.paper
        }
    }

    var color: Color { Theme.color(hex) }

    /// The canvas's inks on this sheet, worked out once.
    var ink: SheetInk { Self.inks[self] ?? .darkroom }
    private static let inks = Dictionary(uniqueKeysWithValues: allCases.map { ($0, SheetInk(sheet: $0.hex)) })
}

/// The inks the canvas draws with on a sheet: the caption, the label, the
/// trim marks, the empty window's words, crop's dial, and the pencil. The
/// views on the canvas read them from the environment (`sheetInk`); the
/// inspector, the title band and everything else keep `Theme`'s, which the
/// default is.
///
/// Each role keeps its warm hue and chroma in OKLCH and moves only in
/// lightness, to the contrast with the sheet that it has on `bg`. A sheet
/// that can't reach `paper`'s 16.6 : 1 squeezes the ramp in log-contrast,
/// with words held at 4.5 : 1 where the sheet allows it. Light ink when
/// white stands out from the sheet more than black does. The pencil stays
/// yellow while it holds 3 : 1; on a light sheet it turns the red of a
/// grease pencil on a contact sheet. On `bg` the inks are `Theme`'s exactly.
struct SheetInk: Equatable, Sendable {
    /// 0xRRGGBB.
    struct Hex: Equatable, Sendable {
        var sheet, paper, label, dim, track, line, accent: UInt32
    }

    let hex: Hex
    /// Light ink, on a dark sheet.
    let light: Bool

    var sheet: Color { Theme.color(hex.sheet) }
    var paper: Color { Theme.color(hex.paper) }
    var label: Color { Theme.color(hex.label) }
    var dim: Color { Theme.color(hex.dim) }
    var track: Color { Theme.color(hex.track) }
    var line: Color { Theme.color(hex.line) }
    var accent: Color { Theme.color(hex.accent) }

    static let darkroom = SheetInk(sheet: Theme.Hex.bg)

    /// Each role's contrast with `bg`, and its OKLCH chroma and hue.
    private static let roles: [(ink: WritableKeyPath<Hex, UInt32>, contrast: Double, chroma: Double, hue: Double, words: Bool)] = [
        (\.paper, 16.58, 0.020, 85, true),
        (\.label, 9.78, 0.023, 83, true),
        (\.dim, 5.74, 0.022, 85, true),
        (\.track, 2.02, 0.012, 88, false),
        (\.line, 1.35, 0.008, 85, false),
    ]
    /// The grease pencil's red.
    static let grease: UInt32 = 0xc83122
    /// Words must reach this contrast where the sheet allows it; marks this.
    static let wordsFloor = 4.5, markFloor = 3.0

    init(sheet: UInt32) {
        if sheet == Theme.Hex.bg {
            hex = Hex(sheet: sheet, paper: Theme.Hex.paper, label: Theme.Hex.label, dim: Theme.Hex.dim,
                      track: Theme.Hex.track, line: Theme.Hex.line, accent: Theme.Hex.accent)
            light = true
            return
        }
        let light = Theme.contrast(sheet, 0xffffff) >= Theme.contrast(sheet, 0x000000)
        let reach = Theme.contrast(sheet, light ? 0xffffff : 0x000000)
        let squeeze = min(1, log(reach) / log(Self.roles[0].contrast))
        var hex = Hex(sheet: sheet, paper: 0, label: 0, dim: 0, track: 0, line: 0, accent: 0)
        for role in Self.roles {
            var target = exp(log(role.contrast) * squeeze)
            if role.words { target = max(target, min(Self.wordsFloor, reach * 0.995)) }
            hex[keyPath: role.ink] = OKLCH.solve(sheet: sheet, contrast: target, chroma: role.chroma, hue: role.hue, light: light)
        }
        if Theme.contrast(sheet, Theme.Hex.accent) >= Self.markFloor {
            hex.accent = Theme.Hex.accent
        } else if light {
            hex.accent = OKLCH.solve(sheet: sheet, contrast: Self.markFloor, chroma: 0.162, hue: 91, light: true)
        } else {
            hex.accent = Theme.contrast(sheet, Self.grease) >= Self.markFloor
                ? Self.grease : OKLCH.solve(sheet: sheet, contrast: Self.markFloor, chroma: 0.19, hue: 30, light: false)
        }
        self.hex = hex
        self.light = light
    }
}

extension EnvironmentValues {
    /// The canvas's inks; `Theme`'s outside the canvas.
    @Entry var sheetInk = SheetInk.darkroom
}

/// Björn Ottosson's OKLab in polar form, for inks that keep their hue.
enum OKLCH {
    static func linear(_ c: Double) -> Double { c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
    static func encoded(_ c: Double) -> Double { c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1 / 2.4) - 0.055 }

    /// OKLab's lightness of an sRGB colour, 0…1.
    static func lightness(_ hex: UInt32) -> Double {
        let (r0, g0, b0) = Theme.components(hex)
        let (r, g, b) = (linear(r0), linear(g0), linear(b0))
        let l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        let m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        let s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        return 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s
    }

    /// Linear sRGB, or nil outside its gamut.
    static func linearRGB(lightness: Double, chroma: Double, hue: Double) -> (Double, Double, Double)? {
        let a = chroma * cos(hue * .pi / 180), b = chroma * sin(hue * .pi / 180)
        func cube(_ x: Double) -> Double { x * x * x }
        let l = cube(lightness + 0.3963377774 * a + 0.2158037573 * b)
        let m = cube(lightness - 0.1055613458 * a - 0.0638541728 * b)
        let s = cube(lightness - 0.0894841775 * a - 1.2914855480 * b)
        let rgb = (4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                   -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                   -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
        let inside = [rgb.0, rgb.1, rgb.2].allSatisfy { $0 > -1e-4 && $0 < 1 + 1e-4 }
        return inside ? rgb : nil
    }

    /// The colour at `lightness` and `hue`, with `chroma` or as much of it as sRGB holds.
    static func color(lightness: Double, chroma: Double, hue: Double) -> UInt32 {
        func byte(_ c: Double) -> UInt32 { UInt32((min(1, max(0, encoded(c))) * 255).rounded()) }
        var c = chroma
        while true {
            if let (r, g, b) = linearRGB(lightness: lightness, chroma: c, hue: hue) { return byte(r) << 16 | byte(g) << 8 | byte(b) }
            if c < 1e-4 {
                let grey = byte(lightness * lightness * lightness)
                return grey << 16 | grey << 8 | grey
            }
            c *= 0.85
        }
    }

    /// The colour nearest the sheet in lightness, lighter or darker, that
    /// reaches `contrast` with it.
    static func solve(sheet: UInt32, contrast target: Double, chroma: Double, hue: Double, light: Bool) -> UInt32 {
        var (low, high) = light ? (lightness(sheet), 1.0) : (0.0, lightness(sheet))
        for _ in 0..<40 {
            let mid = (low + high) / 2
            let reaches = Theme.contrast(sheet, color(lightness: mid, chroma: chroma, hue: hue)) >= target
            if light { if reaches { high = mid } else { low = mid } } else { if reaches { low = mid } else { high = mid } }
        }
        return color(lightness: light ? high : low, chroma: chroma, hue: hue)
    }
}
