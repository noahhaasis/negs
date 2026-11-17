import AppKit
import Observation
import SwiftUI

/// The built-in themes (round 28, design/proposals/IMPLEMENTATION-THEMES.md).
/// One is chosen in Settings › Look; Darkroom, the original dark design, is
/// the default. The theme doesn't follow the system's light or dark mode.
enum AppTheme: String, CaseIterable, Identifiable, Sendable {
    case darkroom, contactSheet, lightTable, daylight, plain

    /// The choice in the user defaults.
    static let key = "appTheme"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .darkroom: "Darkroom"
        case .contactSheet: "Contact sheet"
        case .lightTable: "Light table"
        case .daylight: "Daylight"
        case .plain: "Plain"
        }
    }

    /// The line under the swatches in Settings › Look.
    var hint: String {
        switch self {
        case .darkroom: "Warm black, cream ink and the yellow pencil. The default."
        case .contactSheet: "Ivory paper and the red grease pencil; the strip stays black film."
        case .lightTable: "Lit white, graphite type and a yellow highlighter."
        case .daylight: "Soft grey with an amber pencil, the calmest of the light ones."
        case .plain: "Black on light grey in the system font, with plain blue marks. The easiest to read."
        }
    }

    var palette: Palette { Palette.all[self]! }

    /// The stored choice, Darkroom when there's none or it's unknown.
    static func stored(_ defaults: UserDefaults = .standard) -> AppTheme {
        defaults.string(forKey: key).flatMap(AppTheme.init(rawValue:)) ?? .darkroom
    }
}

/// One theme's colours, as hex for the contrast tests and as ready-made
/// SwiftUI and AppKit colours.
struct Palette: Sendable {
    /// 0xRRGGBB, sRGB.
    struct Hex: Equatable, Sendable {
        /// The sheet the photograph lies on, and the title band.
        var bg: UInt32
        /// The inspector, Settings and the alert.
        var panel: UInt32
        /// The strip, the tray and the leader.
        var film: UInt32
        /// Behind the grid.
        var grid: UInt32
        var line, track, dim, label, paper: UInt32
        /// The pencil's thin marks: rings, dots, fills, lines.
        var accent: UInt32
        /// The Export and Done buttons, and the text on them.
        var fill, onFill: UInt32
        var warn: UInt32
        /// A tile not yet developed.
        var blank: UInt32
    }

    struct Colors: Sendable {
        let bg, panel, film, grid, line, track, dim, label, paper, accent, fill, onFill, warn, blank: Color
        init(_ h: Hex) {
            func c(_ v: UInt32) -> Color { Theme.color(v) }
            (bg, panel, film, grid, line, track) = (c(h.bg), c(h.panel), c(h.film), c(h.grid), c(h.line), c(h.track))
            (dim, label, paper, accent, fill, onFill) = (c(h.dim), c(h.label), c(h.paper), c(h.accent), c(h.fill), c(h.onFill))
            (warn, blank) = (c(h.warn), c(h.blank))
        }
    }

    struct NSColors: @unchecked Sendable {
        let bg, panel, film, line, track, dim, label, paper, accent, onFill, warn, blank: NSColor
        init(_ h: Hex) {
            func c(_ v: UInt32) -> NSColor { Theme.nsColor(v) }
            (bg, panel, film, line, track, dim) = (c(h.bg), c(h.panel), c(h.film), c(h.line), c(h.track), c(h.dim))
            (label, paper, accent, onFill, warn, blank) = (c(h.label), c(h.paper), c(h.accent), c(h.onFill), c(h.warn), c(h.blank))
        }
    }

    let hex: Hex
    /// Light ink on dark: AppKit draws menus and pop-overs dark.
    let dark: Bool
    /// The pencil under a word is a highlighter, swiped behind its lower
    /// half (Light table); strikes stay a pencil line in `accent`.
    let highlighter: Bool
    /// The system font for the serif and straight, upright marks for the
    /// pencil's (Plain): `Theme.serifFont`, `PencilStroke`, `PencilRing`.
    let plain: Bool
    let colors: Colors
    let ns: NSColors
    /// The words and marks on the film (strip, tray, leader): this palette's
    /// own, except where the film is dark under light panels (Contact sheet).
    let filmHex: Hex
    let filmColors: Colors
    let filmNS: NSColors

    init(_ hex: Hex, dark: Bool, highlighter: Bool = false, plain: Bool = false, filmInk: Hex? = nil) {
        self.hex = hex
        self.dark = dark
        self.highlighter = highlighter
        self.plain = plain
        filmHex = filmInk ?? hex
        colors = Colors(hex)
        ns = NSColors(hex)
        filmColors = Colors(filmHex)
        filmNS = NSColors(filmHex)
    }

    private static let darkroomHex = Hex(bg: 0x0c0b0a, panel: 0x161513, film: 0x000000, grid: 0x000000,
                                         line: 0x2b2925, track: 0x47443d, dim: 0x918a7c, label: 0xbeb6a7, paper: 0xf2ebdd,
                                         accent: 0xffd23f, fill: 0xffd23f, onFill: 0x14120c, warn: 0xff8a66, blank: 0x1c1b18)

    static let all: [AppTheme: Palette] = [
        // The T2 · Label tokens (IMPLEMENTATION.md §2.1), unchanged.
        .darkroom: Palette(darkroomHex, dark: true),
        // Ivory paper and the red grease pencil of a Magnum contact sheet; the
        // strip stays black film, its inks Darkroom's with a brighter red.
        .contactSheet: Palette(Hex(bg: 0xf7f3ea, panel: 0xe9e2d3, film: 0x0c0b0a, grid: 0xf7f3ea,
                                   line: 0xd6cdbb, track: 0xb3aa99, dim: 0x655c4e, label: 0x463f35, paper: 0x1d1a15,
                                   accent: 0xc63a26, fill: 0xc63a26, onFill: 0xfff8ee, warn: 0x9a2f18, blank: 0xe2dccd),
                               dark: false, filmInk: { var h = darkroomHex; h.accent = 0xe2492f; return h }()),
        // A lit white table, graphite type; the yellow stays as a highlighter
        // and in the Export button, ochre where it's a thin line.
        .lightTable: Palette(Hex(bg: 0xffffff, panel: 0xededeb, film: 0xf3f3f1, grid: 0xf3f3f1,
                                 line: 0xdcdcd9, track: 0xbdbdba, dim: 0x666663, label: 0x41413f, paper: 0x151514,
                                 accent: 0xad7b00, fill: 0xffd23f, onFill: 0x14120c, warn: 0xb4381e, blank: 0xe4e4e1),
                             dark: false, highlighter: true),
        // Soft warm grey with an amber pencil.
        .daylight: Palette(Hex(bg: 0xe8e6e1, panel: 0xd8d5cf, film: 0xc9c6bf, grid: 0xc9c6bf,
                               line: 0xc6c2bb, track: 0xa29e96, dim: 0x57524b, label: 0x3a3631, paper: 0x161411,
                               accent: 0xa35000, fill: 0xa35000, onFill: 0xffffff, warn: 0x912a16, blank: 0xd0cdc7), dark: false),
        // Near native: neutral greys, near-black words (7 : 1 and up on every
        // ground), a plain blue mark and the system font.
        .plain: Palette(Hex(bg: 0xf7f7f7, panel: 0xececec, film: 0xe2e2e2, grid: 0xf7f7f7,
                            line: 0xd2d2d2, track: 0xa6a6a6, dim: 0x4a4a4a, label: 0x1d1d1d, paper: 0x000000,
                            accent: 0x0058d0, fill: 0x0062d9, onFill: 0xffffff, warn: 0xb0160a, blank: 0xe0e0e0),
                        dark: false, plain: true),
    ]

    /// The highlighter's yellow and strength (Light table).
    static let highlighterInk: UInt32 = 0xffd23f
    static let highlighterOpacity = 0.85
}

/// The chosen theme, observed: SwiftUI views that read a `Theme` colour in
/// their body redraw when it changes.
@Observable
final class ThemeStore: @unchecked Sendable {
    static let shared = ThemeStore()

    private(set) var theme: AppTheme
    private(set) var palette: Palette

    init(theme: AppTheme = AppTheme.stored()) {
        self.theme = theme
        palette = theme.palette
    }

    /// Chooses a theme, stores it and tells AppKit's views (`Theme.didChange`).
    @MainActor
    func choose(_ theme: AppTheme, defaults: UserDefaults = .standard) {
        defaults.set(theme.rawValue, forKey: AppTheme.key)
        guard theme != self.theme else { return }
        self.theme = theme
        palette = theme.palette
        NotificationCenter.default.post(name: Theme.didChange, object: nil)
    }
}

extension Theme {
    /// What dims the window under an alert: black at 0.28 on a dark theme, a
    /// warm brown at 0.12 on a light one (r28-light.html).
    static var dimming: NSColor {
        palette.dark ? NSColor.black.withAlphaComponent(0.28) : NSColor(srgbRed: 0.24, green: 0.16, blue: 0.08, alpha: 0.12)
    }
    /// Words on a dark badge laid over a photograph: Darkroom's `paper` in
    /// every theme, since the badge stays dark.
    static var onPhoto: Color { AppTheme.darkroom.palette.colors.paper }
    /// The colour of shadows under alerts and tags: black, softer on a light theme.
    static var shadowStrength: Double { palette.dark ? 1 : 0.3 }

    /// Calls `apply` now and again after every theme change, for as long as
    /// `owner` lives: for AppKit views and layers that set their colours once.
    @MainActor static func follow(_ owner: AnyObject, _ apply: @escaping @MainActor () -> Void) {
        apply()
        let token = NotificationCenter.default.addObserver(forName: didChange, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { apply() }
        }
        let box = ThemeFollower(token)
        objc_setAssociatedObject(owner, Unmanaged.passUnretained(box).toOpaque(), box, .OBJC_ASSOCIATION_RETAIN)
    }
}

/// Removes its observer when its owner goes.
private final class ThemeFollower {
    let token: NSObjectProtocol
    init(_ token: NSObjectProtocol) { self.token = token }
    deinit { NotificationCenter.default.removeObserver(token) }
}
