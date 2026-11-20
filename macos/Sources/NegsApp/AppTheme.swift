import AppKit
import CNegsCore
import Observation
import SwiftUI

/// The built-in themes (round 28, design/proposals/IMPLEMENTATION-THEMES.md).
/// One is chosen in Settings › Look; Darkroom, the original dark design, is
/// the default. The theme doesn't follow the system's light or dark mode.
enum AppTheme: String, CaseIterable, Identifiable, Sendable {
    case darkroom, contactSheet, lightTable, daylight, plain, plainDark

    /// The choice in the user defaults.
    static let key = "appTheme"

    var id: String { rawValue }

    /// The core's id (`NEGS_THEME_*`): the cases are in the core's order.
    var coreId: UInt32 { UInt32(Self.allCases.firstIndex(of: self)!) }

    var title: String { String(cString: Palette.core(coreId).title) }

    /// The line under the swatches in Settings › Look.
    var hint: String { String(cString: negs_theme_hint(coreId, NEGS_PLATFORM_MACOS)) }

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

    /// The themes' tokens, from the shared core (core/src/theme).
    static let all: [AppTheme: Palette] = Dictionary(uniqueKeysWithValues: AppTheme.allCases.map { theme in
        let t = core(theme.coreId)
        return (theme, Palette(Hex(t.hex), dark: t.dark != 0, highlighter: t.highlighter != 0, plain: t.plain != 0,
                               filmInk: Hex(t.film)))
    })

    static func core(_ id: UInt32) -> negs_theme_t {
        var t = negs_theme_t()
        precondition(negs_theme_get(id, &t) != 0, "the core has no theme \(id)")
        return t
    }

    static let constants: negs_theme_constants_t = {
        var c = negs_theme_constants_t()
        negs_theme_constants(&c)
        return c
    }()

    /// The highlighter's yellow and strength (Light table).
    static let highlighterInk: UInt32 = constants.highlighter_ink
    static let highlighterOpacity = constants.highlighter_opacity
}

extension Palette.Hex {
    init(_ h: negs_theme_hex_t) {
        self.init(bg: h.bg, panel: h.panel, film: h.film, grid: h.grid, line: h.line, track: h.track, dim: h.dim,
                  label: h.label, paper: h.paper, accent: h.accent, fill: h.fill, onFill: h.on_fill, warn: h.warn, blank: h.blank)
    }
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
        let c = Palette.constants, d = palette.dark ? c.dimming_dark : c.dimming_light
        return NSColor(srgbRed: d.0, green: d.1, blue: d.2, alpha: d.3)
    }
    /// Words on a dark badge laid over a photograph: Darkroom's `paper` in
    /// every theme, since the badge stays dark.
    static var onPhoto: Color { AppTheme.darkroom.palette.colors.paper }
    /// The colour of shadows under alerts and tags: black, softer on a light theme.
    static var shadowStrength: Double { palette.dark ? Palette.constants.shadow_dark : Palette.constants.shadow_light }

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
