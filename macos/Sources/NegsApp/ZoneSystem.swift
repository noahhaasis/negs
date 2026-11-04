import AppKit

/// The zone ladder: where each zone begins, in CIE Lab lightness.
enum ZoneLadder {
    /// Zone 0, then I…VIII. Zone VIII is paper white: a reflection print
    /// cannot hold a stop above it, so Ansel Adams' zones IX and X (paper
    /// white without texture, and the specular light source) have no place on
    /// this axis.
    static let names = ["0", "I", "II", "III", "IV", "V", "VI", "VII", "VIII"]

    /// Each zone's tone in relative luminance: 18 % × 2^(n − 5) for 0…VII,
    /// then paper white for VIII.
    static let centres: [Double] =
        (0..<8).map { 0.18 / pow(2, 5) * pow(2, Double($0)) } + [0.9]

    /// The ten boundaries, L\* 0…100: black, the geometric midpoint between
    /// neighbouring zones (a half stop apart through the midtones), and paper
    /// white. Zone VII ends at L\* 92.6 and VIII runs to 100.
    static let lstar: [Double] =
        [0] + (1..<centres.count).map { lstar(sqrt(centres[$0 - 1] * centres[$0])) } + [100]

    /// CIE Lab lightness of a relative luminance.
    static func lstar(_ y: Double) -> Double {
        y > 216.0 / 24389.0 ? 116 * cbrt(y) - 16 : y * 24389.0 / 27.0
    }
}

/// One zone of the print, as a band on the L\* axis.
struct Zone: Identifiable, Hashable, Sendable {
    let index: Int
    let numeral: String
    let name: String
    /// The band's edges on the 0…100 L\* axis.
    let lStarRange: ClosedRange<Double>
    var id: Int { index }
    var centre: Double { (lStarRange.lowerBound + lStarRange.upperBound) / 2 }
    /// Where a pin on this band lands: black for zone 0, paper white for the
    /// last zone, and the middle of the band in between.
    var target: Double {
        if lStarRange.lowerBound <= 0 { return 0 }
        if lStarRange.upperBound >= 100 { return 100 }
        return centre
    }
}

/// The zones on the L\* axis, from the same ladder the engine's tone curve
/// uses (`ZoneLadder`): zones 0–VII one stop apart from 18 % grey, then
/// zone VIII, paper white.
enum ZoneScale {
    static let names = [
        "Pure black", "Near black", "First texture", "Dark detail",
        "Open shadow", "Middle grey", "Light midtone", "Light texture", "Paper white"
    ]

    static let zones: [Zone] = {
        var out: [Zone] = []
        for index in ZoneLadder.names.indices {
            let lower = ZoneLadder.lstar[index]
            guard lower < 100 else { break }
            let upper = min(100, ZoneLadder.lstar[min(index + 1, ZoneLadder.lstar.count - 1)])
            out.append(Zone(
                index: index, numeral: ZoneLadder.names[index], name: names[index],
                lStarRange: lower...upper
            ))
        }
        return out
    }()

    /// The band containing a tone, L\* 0…100.
    static func zone(forTarget target: Double) -> Int {
        zones.last { $0.lStarRange.lowerBound <= target }?.index ?? 0
    }

    /// The grey a zone prints as, on its tile and its pins.
    static func grey(_ zone: Int) -> NSColor { grey(lstar: zones[zone].target) }

    /// The sRGB grey of a tone, L\* 0…100.
    static func grey(lstar: Double) -> NSColor {
        let y = lstar > 8 ? pow((lstar + 16) / 116, 3) : lstar * 27.0 / 24389
        let encoded = y <= 0.003_130_8 ? 12.92 * y : 1.055 * pow(y, 1 / 2.4) - 0.055
        let v = min(max(encoded, 0), 1)
        return NSColor(srgbRed: v, green: v, blue: v, alpha: 1)
    }
}

/// A pinned tone: a measured L\* held where it is, or aimed at a zone, and
/// the circle on the photograph it averaged.
struct TonePin: Equatable, Sendable {
    var input: Double
    var target: Double
    var zone: Int
    /// 1-based, for the numbered circle on the photograph.
    var number: Int
    /// Where the tone was measured; nil when the app recorded no position.
    var source: PinSource?
}

/// The circle a pin averaged, in fractions of the photograph.
struct PinSource: Equatable, Sendable {
    var centre: CGPoint
    /// As a fraction of the shorter side.
    var radius: Double
}

/// A pin the canvas outlines.
struct TonePinMarker: Equatable, Sendable {
    let index: Int
    /// The number in the circle.
    let number: Int
    /// The measured L\*, which colours the circle.
    let lstar: Double
    let source: PinSource
    let selected: Bool
}
