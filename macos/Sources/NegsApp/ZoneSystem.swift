import Foundation

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

