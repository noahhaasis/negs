import AppKit

/// One of Ansel Adams' zones, as a tone of the print.
struct Zone: Identifiable, Hashable, Sendable {
    let index: Int
    let name: String
    /// What typically belongs in this zone.
    let note: String
    var id: Int { index }
    var numeral: String { "\(index)" }
    /// Where a pin on this zone prints, in L\* 0…100: the middle of its band.
    /// The two end bands are half as wide, stopping at black and white, so a
    /// pin on either leaves the ends of the curve alone rather than clipping.
    var target: Double { min(max(Double(index) * 10, 2.5), 97.5) }
}

/// The eleven zones, evenly spaced in L\*, which puts zone 5 at 18 % gray
/// (L\* 49.5). On the negative a zone is a stop, but 18 % × 2⁵ is far past
/// white: the print compresses that range onto paper, and these are the tones
/// it prints.
enum ZoneScale {
    static let zones: [Zone] = [
        ("Pure black", "No tone at all."),
        ("Near black", "A slight tonality, but no detail."),
        ("Dark black", "Slight detail in the shadows."),
        ("Very dark gray", "Shadows with distinct, visible texture."),
        ("Medium dark gray", "Slightly darker Black skin, dark foliage, landscape shadows."),
        ("Middle gray", "18% gray: darker tan white skin, lighter Black skin, light foliage, dark blue sky."),
        ("Middle light gray", "Average white skin, light stone, shadow areas on snow."),
        ("Light gray", "Pale white skin, concrete or gray asphalt in sunlight."),
        ("Gray-white", "Detail in highlights: white walls in sunlight, bright surfaces."),
        ("Bright white", "Slight detail in highlights: white paper, snow, white water."),
        ("Pure white", "No detail: light sources, specular highlights.")
    ].enumerated().map { Zone(index: $0.offset, name: $0.element.0, note: $0.element.1) }

    /// The zone nearest a tone, L\* 0…100.
    static func zone(forTarget target: Double) -> Int {
        min(max(Int((target / 10).rounded()), 0), zones.count - 1)
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

/// A pin: a measured tone that prints as its zone.
struct TonePin: Equatable, Sendable {
    /// The tone before the pins' curve.
    var input: Double
    var target: Double
    var zone: Int
    /// Where the tone was measured; nil when the app recorded no position.
    var source: PinSource?
}

/// The circle a pin averages, in fractions of the uncropped photograph.
struct PinSource: Equatable, Sendable {
    var centre: CGPoint
    /// As a fraction of the shorter side.
    var radius: Double
}

/// A pin the canvas draws.
struct TonePinMarker: Equatable, Sendable {
    let index: Int
    let zone: Int
    let source: PinSource
    let selected: Bool
    /// The tone curve can't honour it, so it has no effect.
    var ignored = false
}

/// A zone being dragged from the panel.
struct ZoneDrag: Equatable, Sendable {
    let zone: Int
    /// The pointer in canvas points, from the top left.
    let point: CGPoint
}
