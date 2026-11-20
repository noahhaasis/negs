import AppKit
import CNegsCore
import CoreText
import os
import SwiftUI

/// The T2 · Label design's tokens: colours, type and sizes, the pencil
/// stroke and rings. Every view reads from here, so the design changes in
/// one place. The values come from design/proposals/IMPLEMENTATION.md §2
/// (and final.css, the mockup's tokens). The colours belong to the chosen
/// theme (`AppTheme`, round 28: IMPLEMENTATION-THEMES.md); every colour
/// below reads the live one, so SwiftUI views redraw when it changes.
enum Theme {
    // MARK: Colour (§2.1), sRGB

    /// The chosen theme's palette.
    static var palette: Palette { ThemeStore.shared.palette }

    /// The colour tokens as 0xRRGGBB, for the contrast tests and the hex
    /// lookups below. The chosen theme's.
    enum Hex {
        /// The sheet the photograph lies on (canvas, title band).
        static var bg: UInt32 { palette.hex.bg }
        /// The inspector, a shade off the sheet; also the alert sheet.
        static var panel: UInt32 { palette.hex.panel }
        /// The strip, the tray, the leader.
        static var film: UInt32 { palette.hex.film }
        /// Behind the grid.
        static var grid: UInt32 { palette.hex.grid }
        /// Rules between sections and modules.
        static var line: UInt32 { palette.hex.line }
        /// Slider track, outlines, separators, outlined buttons. Only on
        /// things whose words or shape already say what they are.
        static var track: UInt32 { palette.hex.track }
        /// Values at their default, hints, secondary words.
        static var dim: UInt32 { palette.hex.dim }
        /// Control labels.
        static var label: UInt32 { palette.hex.label }
        /// What's being read: names, changed values, the focus box.
        static var paper: UInt32 { palette.hex.paper }
        /// The pencil. Only ever a mark, never a text colour.
        static var accent: UInt32 { palette.hex.accent }
        /// The Export and Done buttons' fill.
        static var fill: UInt32 { palette.hex.fill }
        /// Errors and warnings, always as words.
        static var warn: UInt32 { palette.hex.warn }
        /// The text on the Export and Done buttons.
        static var onAccent: UInt32 { palette.hex.onFill }
        /// A tile not yet developed.
        static var blank: UInt32 { palette.hex.blank }
    }

    static var bg: Color { palette.colors.bg }
    static var panel: Color { palette.colors.panel }
    static var film: Color { palette.colors.film }
    static var grid: Color { palette.colors.grid }
    static var line: Color { palette.colors.line }
    static var track: Color { palette.colors.track }
    static var dim: Color { palette.colors.dim }
    static var label: Color { palette.colors.label }
    static var paper: Color { palette.colors.paper }
    static var accent: Color { palette.colors.accent }
    static var fill: Color { palette.colors.fill }
    static var warn: Color { palette.colors.warn }
    static var onAccent: Color { palette.colors.onFill }
    static var blank: Color { palette.colors.blank }

    /// The same tokens for AppKit and Core Animation. Colours set once on a
    /// view or layer must be set again on `Theme.didChange`.
    enum NS {
        static var bg: NSColor { palette.ns.bg }
        static var panel: NSColor { palette.ns.panel }
        static var film: NSColor { palette.ns.film }
        static var line: NSColor { palette.ns.line }
        static var track: NSColor { palette.ns.track }
        static var dim: NSColor { palette.ns.dim }
        static var label: NSColor { palette.ns.label }
        static var paper: NSColor { palette.ns.paper }
        static var accent: NSColor { palette.ns.accent }
        static var warn: NSColor { palette.ns.warn }
        static var onAccent: NSColor { palette.ns.onFill }
        static var blank: NSColor { palette.ns.blank }
    }

    /// The inks on the film (the strip, the tray, the leader). The same as
    /// the theme's, except in Contact sheet, where the film stays black.
    enum Film {
        static var paper: Color { palette.filmColors.paper }
        static var label: Color { palette.filmColors.label }
        static var dim: Color { palette.filmColors.dim }
        static var track: Color { palette.filmColors.track }
        static var line: Color { palette.filmColors.line }
        static var accent: Color { palette.filmColors.accent }
        static var blank: Color { palette.filmColors.blank }
        static var nsPaper: NSColor { palette.filmNS.paper }
        static var nsDim: NSColor { palette.filmNS.dim }
    }

    /// The appearance for menus, pop-overs and panels AppKit draws itself.
    static var appearance: NSAppearance? { NSAppearance(named: palette.dark ? .darkAqua : .aqua) }
    /// The same for SwiftUI.
    static var colorScheme: ColorScheme { palette.dark ? .dark : .light }
    /// Posted on the main thread after the theme changes, for AppKit views
    /// and layers that set their colours once.
    static let didChange = Notification.Name("NegsThemeDidChange")

    /// Text selection: the pencil at 0.32, never system blue (§4.9).
    static var selection: Color { accent.opacity(Palette.constants.selection_opacity) }
    /// Outside the crop box the photograph darkens with this (§4.10).
    static let cropShade = color(Palette.constants.crop_shade, opacity: Palette.constants.crop_shade_opacity)
    /// Disabled controls fade to this opacity (§2.1: 0.35–0.38).
    static let disabledOpacity = Palette.constants.disabled_opacity
    /// Strip frames other than the current one (§4.4).
    static let stripDim = Palette.constants.strip_dim

    static func color(_ hex: UInt32, opacity: Double = 1) -> Color {
        let (r, g, b) = components(hex)
        return Color(.sRGB, red: r, green: g, blue: b, opacity: opacity)
    }
    static func nsColor(_ hex: UInt32, alpha: CGFloat = 1) -> NSColor {
        let (r, g, b) = components(hex)
        return NSColor(srgbRed: r, green: g, blue: b, alpha: alpha)
    }
    static func components(_ hex: UInt32) -> (Double, Double, Double) {
        (Double((hex >> 16) & 0xff) / 255, Double((hex >> 8) & 0xff) / 255, Double(hex & 0xff) / 255)
    }

    /// The WCAG 2 contrast ratio between two sRGB colours, 1…21.
    static func contrast(_ a: UInt32, _ b: UInt32) -> Double { negs_theme_contrast(a, b) }
    /// The WCAG 2 relative luminance of an sRGB colour, 0…1.
    static func luminance(_ hex: UInt32) -> Double { negs_theme_luminance(hex) }

    // MARK: Layout (§2.3), points

    enum Size {
        static let titleBar: CGFloat = 38
        static let inspector: CGFloat = 404
        static let inspectorSmallest: CGFloat = 340
        static let gutter: CGFloat = 22
        static let row: CGFloat = 29
        /// Fitted to the longest slider label (“Developer exhaustion”, 122 pt)
        /// and the widest value in bold, so the track gets the rest (round 26);
        /// `InspectorLayoutTests` checks every slider still fits.
        static let sliderLabel: CGFloat = 132
        static let valueWidth: CGFloat = 46
        static let strip: CGFloat = 158
        static let stripTopPadding: CGFloat = 6
        static let stripRebate: CGFloat = 12
        static let stripGap: CGFloat = 5
        static let statusLine: CGFloat = 28
        static let tray: CGFloat = strip + statusLine
        static let gridGap: CGFloat = 6
        static let gridThumbnail: CGFloat = 176
        /// Space round the photograph: the caption above, two label rows below, always.
        static let stageHorizontal: CGFloat = 48
        static let stageTop: CGFloat = 54
        static let stageBottom: CGFloat = 68
        static let smallestWindow = CGSize(width: 980, height: 560)
        static let sectionHead: CGFloat = 34
        static let tabRow: CGFloat = 50
        static let paneHeader: CGFloat = 42
        static let moduleRow: CGFloat = 37
        /// The keyboard focus box: 1.5 pt `paper`, 5 pt outside, radius 3 (§4.13).
        static let focusOutset: CGFloat = 5
        static let focusLine: CGFloat = 1.5
        static let focusRadius: CGFloat = 3
    }

    // MARK: Type (§2.2)

    /// Newsreader, bundled (macos/Fonts): names, tabs, values, hints,
    /// headings, the caption and buttons. Optical size follows the point
    /// size, as the mockup's `font-optical-sizing: auto` does, and figures
    /// are tabular and lining. Weights run 400–700 (the subset's range).
    static func serif(_ size: CGFloat, weight: CGFloat = 400, italic: Bool = false) -> Font {
        Font(serifFont(size, weight: weight, italic: italic))
    }
    /// The same face as a Core Text font, for AppKit text and layers.
    /// Plain sets SF instead, upright, at 0.95 of the size: the same cap
    /// height as Newsreader's (its x-height still a fifth taller), and
    /// near the same width, so the layouts made for Newsreader still fit.
    static func serifFont(_ size: CGFloat, weight: CGFloat = 400, italic: Bool = false) -> CTFont {
        let key = FontKey(size: size, weight: weight, italic: italic, plain: palette.plain)
        if let font = fontCache.withLock({ $0[key] }) { return font }
        if key.plain {
            let font = NSFont.monospacedDigitSystemFont(ofSize: size * 0.95, weight: weight >= 650 ? .semibold : weight >= 500 ? .medium : .regular) as CTFont
            fontCache.withLock { $0[key] = font }
            return font
        }
        let font = Fonts.make(key)
        // A fallback isn't kept, so Newsreader replaces it once it's read.
        if Fonts.loaded { fontCache.withLock { $0[key] = font } }
        return font
    }
    /// SF, the system font: control labels, the status line, small UI text.
    static func ui(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight).monospacedDigit()
    }

    struct FontKey: Hashable { let size: CGFloat, weight: CGFloat, italic: Bool; var plain = false }
    private static let fontCache = OSAllocatedUnfairLock(initialState: [FontKey: CTFont]())

    /// Finding and reading the bundled fonts.
    enum Fonts {
        static let upright = "Newsreader-Negs.ttf", italic = "Newsreader-Negs-Italic.ttf"

        /// The folder holding the fonts: Contents/Resources/Fonts in the app,
        /// else macos/Fonts under SPEKTRAFILM_ROOT or the working directory
        /// (`swift run`), else beside this source file (tests).
        static func directory(environment env: [String: String] = ProcessInfo.processInfo.environment,
                              resourceDir: URL? = Bundle.main.resourceURL,
                              workingDir: URL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)) -> URL? {
            let root = env["SPEKTRAFILM_ROOT"].map { URL(fileURLWithPath: $0) } ?? workingDir
            let source = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../Fonts").standardizedFileURL
            let candidates = [resourceDir?.appendingPathComponent("Fonts"), root.appendingPathComponent("macos/Fonts"),
                              root.appendingPathComponent("Fonts"), source].compactMap { $0 }
            return candidates.first { FileManager.default.fileExists(atPath: $0.appendingPathComponent(upright).path) }
        }

        private static let descriptors = OSAllocatedUnfairLock<(upright: CTFontDescriptor, italic: CTFontDescriptor)?>(initialState: nil)
        private static let loading = DispatchGroup()
        private static let started = OSAllocatedUnfairLock(initialState: false)

        /// Starts reading Newsreader on another thread, at launch
        /// (applicationWillFinishLaunching). The fonts are made from the
        /// files' own descriptors, which needs no registration: registering
        /// for the process cost ≈10 ms of the main thread at launch, reading
        /// the descriptors ≈3 ms, and that now happens off it. The first font
        /// asked for waits for it (`make`).
        static func prepare(in directory: URL? = directory()) {
            guard started.withLock({ let was = $0; $0 = true; return !was }) else { return }
            loading.enter()
            DispatchQueue.global(qos: .userInteractive).async {
                load(in: directory)
                loading.leave()
            }
        }

        /// Reads both faces now. Returns whether both were found; without
        /// them the serif falls back to the system's serif (New York).
        @discardableResult
        static func load(in directory: URL? = directory()) -> Bool {
            guard let directory else { return false }
            let found = [upright, italic].compactMap {
                (CTFontManagerCreateFontDescriptorsFromURL(directory.appendingPathComponent($0) as CFURL) as? [CTFontDescriptor])?.first
            }
            guard found.count == 2 else { return false }
            let pair = (upright: found[0], italic: found[1])
            descriptors.withLock { $0 = pair }
            return true
        }
        static var loaded: Bool { descriptors.withLock { $0 != nil } }

        static func make(_ key: FontKey) -> CTFont {
            // A slow disk never holds a view up for long: past 0.2 s the
            // fallback draws, and later fonts are Newsreader once it's read.
            if started.withLock({ $0 }), !loaded { _ = loading.wait(timeout: .now() + 0.2) }
            let base = descriptors.withLock { $0.map { key.italic ? $0.italic : $0.upright } }
            guard let base else { return fallback(key) }
            let variation: [NSNumber: NSNumber] = [
                NSNumber(value: axis("wght")): NSNumber(value: Double(min(max(key.weight, 400), 700))),
                NSNumber(value: axis("opsz")): NSNumber(value: Double(min(max(key.size, 6), 72))),
            ]
            let features: [[CFString: Any]] = ["tnum", "lnum"].map { [kCTFontOpenTypeFeatureTag: $0, kCTFontOpenTypeFeatureValue: 1] }
            let descriptor = CTFontDescriptorCreateCopyWithAttributes(base, [
                kCTFontVariationAttribute: variation,
                kCTFontFeatureSettingsAttribute: features,
            ] as CFDictionary)
            return CTFontCreateWithFontDescriptor(descriptor, key.size, nil)
        }
        private static func fallback(_ key: FontKey) -> CTFont {
            let weight: NSFont.Weight = key.weight >= 650 ? .bold : key.weight >= 550 ? .semibold : key.weight >= 450 ? .medium : .regular
            var descriptor = NSFont.systemFont(ofSize: key.size, weight: weight).fontDescriptor
            descriptor = descriptor.withDesign(.serif) ?? descriptor
            if key.italic { descriptor = descriptor.withSymbolicTraits(.italic) }
            return (NSFont(descriptor: descriptor, size: key.size) ?? .systemFont(ofSize: key.size)) as CTFont
        }
        static func axis(_ tag: String) -> UInt32 { tag.utf8.reduce(0) { $0 << 8 | UInt32($1) } }
    }

    // MARK: Marks (§2.1)

    /// The pencil stroke's width: 2.1 pt, not scaled with the stroke.
    static let pencilWidth = CGFloat(CoreMarks.constants.pencil_width)
    /// The pencil's height under a word: drawn 5–6 pt tall under the baseline.
    /// The highlighter's width (Light table): r28-light.html's 11 pt band, less its round ends.
    static let highlighterWidth = CGFloat(CoreMarks.constants.highlighter_width)
    static let pencilHeight = CGFloat(CoreMarks.constants.pencil_height)
}

/// The one hand-drawn stroke reused for every underline mark: the SVG path
/// `M1 3.6 C 22 1.2, 47 5.2, 70 2.6 S 93 2.2, 99 3.4` in a 100 × 6 box,
/// stretched to the rect. Stroke it with `Theme.pencilWidth`, round caps,
/// in `accent` (`.pencil()` below).
struct PencilStroke: Shape {
    /// A straight line through the middle instead (Plain).
    var plain = Theme.palette.plain

    func path(in rect: CGRect) -> Path { Path(Self.cgPath(in: rect, plain: plain)) }

    /// For `CAShapeLayer`s: the same path, stretched to `rect`.
    static func cgPath(in rect: CGRect, plain: Bool = Theme.palette.plain) -> CGPath {
        var segs = [negs_seg_t](repeating: negs_seg_t(), count: 3)
        let n = negs_marks_pencil(CoreMarks.rect(rect), plain ? 1 : 0, &segs, segs.count)
        return CoreMarks.cgPath(segs[0..<min(n, segs.count)])
    }
}

extension Shape {
    /// The mark under a word: the pencil, 2.1 pt in `accent` (the film's on
    /// the strip and tray), round caps. With no colour given, on a theme with
    /// a highlighter (Light table) it's a yellow swipe behind the lower half
    /// of the word instead, lifted `lift` pt from where the pencil would be.
    @ViewBuilder
    func pencil(_ color: Color? = nil, onFilm: Bool = false, highlighter: Bool? = nil, lift: CGFloat = 5,
                width: CGFloat = Theme.pencilWidth) -> some View {
        if color == nil, highlighter ?? Theme.palette.highlighter {
            stroke(Theme.color(Palette.highlighterInk, opacity: Palette.highlighterOpacity),
                   style: StrokeStyle(lineWidth: Theme.highlighterWidth, lineCap: .round, lineJoin: .round))
                .offset(y: -lift)
                .blendMode(.multiply)
        } else {
            stroke(color ?? (onFilm ? Theme.Film.accent : Theme.accent), style: StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round))
        }
    }

    /// A line struck through words (skipped, not printed): always a thin
    /// pencil line, never the highlighter.
    func strike(_ color: Color? = nil) -> some View {
        stroke(color ?? Theme.accent, style: StrokeStyle(lineWidth: Theme.pencilWidth, lineCap: .round, lineJoin: .round))
    }
}

/// A loose ellipse drawn in pencil: a rounded rectangle whose four corners
/// each have their own elliptical radii, as CSS `border-radius` draws them
/// (radii that don't fit are all scaled down together, as CSS does). Draw it
/// 2 pt wide in `accent`, rotated a little (`PencilRing.tilt`).
struct PencilRing: Shape {
    /// Horizontal and vertical radii per corner, clockwise from the top left.
    struct Radii: Equatable {
        var horizontal: (CGFloat, CGFloat, CGFloat, CGFloat)
        var vertical: (CGFloat, CGFloat, CGFloat, CGFloat)
        /// Whether the values are fractions of the width and height (CSS %)
        /// rather than points.
        var relative = false

        static func == (a: Radii, b: Radii) -> Bool {
            a.horizontal == b.horizontal && a.vertical == b.vertical && a.relative == b.relative
        }
        /// Round a picked frame, tile or thumbnail, 6–7 pt outside the
        /// picture: `30 40 28 44 / 34 26 40 28`.
        static let box = Radii(horizontal: (30, 40, 28, 44), vertical: (34, 26, 40, 28))
        /// Round a number: `47% 53% 50% 50% / 56% 44% 58% 42%`.
        static let number = Radii(horizontal: (0.47, 0.53, 0.50, 0.50), vertical: (0.56, 0.44, 0.58, 0.42), relative: true)
    }
    var radii: Radii = .box
    /// A plain rounded rectangle, or an ellipse round a number, drawn
    /// upright (Plain).
    var plain = Theme.palette.plain

    /// The rotation a ring is drawn at: −0.8° round thumbnails, −1° round
    /// tiles; none in Plain.
    static var tilt: Angle { tilt(-0.8) }
    static func tilt(_ degrees: Double) -> Angle { .degrees(negs_marks_ring_tilt(degrees, Theme.palette.plain ? 1 : 0)) }
    static let lineWidth = CGFloat(CoreMarks.constants.ring_line_width)
    /// How far a ring sits outside the picture it rings.
    static let outset = CGSize(width: CoreMarks.constants.ring_outset_w, height: CoreMarks.constants.ring_outset_h)

    func path(in rect: CGRect) -> Path { Path(Self.cgPath(in: rect, radii: radii, plain: plain)) }

    static func cgPath(in rect: CGRect, radii: Radii = .box, plain: Bool = Theme.palette.plain) -> CGPath {
        var r = negs_radii_t()
        let (h, v) = (radii.horizontal, radii.vertical)
        r.horizontal = (Double(h.0), Double(h.1), Double(h.2), Double(h.3))
        r.vertical = (Double(v.0), Double(v.1), Double(v.2), Double(v.3))
        r.relative = radii.relative ? 1 : 0
        var segs = [negs_seg_t](repeating: negs_seg_t(), count: 10)
        let n = negs_marks_ring(CoreMarks.rect(rect), &r, plain ? 1 : 0, &segs, segs.count)
        return CoreMarks.cgPath(segs[0..<min(n, segs.count)])
    }
}
