import SwiftUI

/// A soft light under the print, like a light box under negatives, for when
/// the print's outer edge would sink into the sheet: a film border or a dark
/// mat on the dark sheet (design/proposals/r24-black-border.html, idea 3).
///
/// It comes on only when the edge's contrast with the sheet is too low, so it
/// depends on both colours: a white mat on the dark sheet gets none. On a
/// light sheet (`ViewerSheet`), a light edge gets a soft shade instead.
enum LightTable {
    /// The light comes on below this WCAG contrast between the print's outer
    /// edge and the sheet. A #333333 mat on `bg` is about 1.56, which reads;
    /// #222222 is about 1.23, which doesn't.
    static let minimumContrast = 1.5
    /// How far the light moves the sheet's colour toward `paper` (or the
    /// shade toward black): on `bg` it gives about `blank`, #1c1b18.
    static let strength = 0.07
    /// Points. The light reaches `spread` past the print's edges before it
    /// fades out over `blur`.
    static let spread: CGFloat = 14
    static let blur: CGFloat = 26
    static let cornerRadius: CGFloat = 10

    /// The colour of the print's outermost pixels as `BorderLayers` draws
    /// them, or nil without a border: then the edge is the photograph's own.
    static func edge(of border: BorderSettings?) -> UInt32? {
        guard let border else { return nil }
        switch border.style {
        case .solid, .keyline: return border.color.hex24
        // Instagram's extension puts the film borders on white.
        case .film, .film35: return border.instagram ? 0xffffff : 0x000000
        }
    }

    /// The light's colour under a print whose outer edge is `edge`, on
    /// `sheet`; nil when the edge already stands out from the sheet.
    static func light(sheet: UInt32, edge: UInt32?) -> UInt32? {
        guard let edge, Theme.contrast(sheet, edge) < minimumContrast else { return nil }
        // Away from the edge: lighter under a dark edge, darker under a light one.
        let toward: UInt32 = Theme.luminance(edge) <= Theme.luminance(sheet) ? Theme.Hex.paper : 0x000000
        return mix(sheet, toward, strength)
    }

    static func mix(_ a: UInt32, _ b: UInt32, _ t: Double) -> UInt32 {
        let (a0, a1, a2) = Theme.components(a), (b0, b1, b2) = Theme.components(b)
        func byte(_ x: Double, _ y: Double) -> UInt32 { UInt32(((x + (y - x) * t) * 255).rounded()) }
        return byte(a0, b0) << 16 | byte(a1, b1) << 8 | byte(a2, b2)
    }
}

/// The light, centred under `print` (canvas points).
struct LightTableGlow: View {
    let print: CGRect
    let color: UInt32

    var body: some View {
        RoundedRectangle(cornerRadius: LightTable.cornerRadius)
            .fill(Theme.color(color))
            .frame(width: print.width + 2 * LightTable.spread, height: print.height + 2 * LightTable.spread)
            .blur(radius: LightTable.blur)
            .position(x: print.midX, y: print.midY)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

extension BorderColor {
    var hex24: UInt32 { UInt32(red) << 16 | UInt32(green) << 8 | UInt32(blue) }
}
