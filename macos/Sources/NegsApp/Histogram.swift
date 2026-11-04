import CoreGraphics
import Foundation
import IOSurface
import SwiftUI

/// The developed photograph as a tone histogram: 256 bins across CIE L\*
/// 0…100 for red, green, blue and the photograph's lightness.
///
/// Every bin is an L\* bin, so the bars line up with the zone bands: bin `i`
/// sits at L\* `i / 255 × 100`. The channels are their own values read as
/// tones — a saturated red's red channel is bright, its blue channel dark —
/// and the lightness is the pixel's luminance, which is what the zones mean.
struct Histogram: Equatable, Sendable {
    var red = [UInt32](repeating: 0, count: 256)
    var green = [UInt32](repeating: 0, count: 256)
    var blue = [UInt32](repeating: 0, count: 256)
    /// The pixel's CIE L\*: CIE Lab lightness of its luminance.
    var lightness = [UInt32](repeating: 0, count: 256)

    /// Counts the crop of a BGRA preview. A large crop is sampled on an even
    /// grid of at most `maxSamples` pixels, which keeps the shape and takes a
    /// few milliseconds at any preview size.
    static func of(
        _ surface: IOSurface,
        crop: CropSettings,
        maxSamples: Int = 1 << 20
    ) -> Histogram {
        var histogram = Histogram()
        let size = CGSize(width: surface.width, height: surface.height)
        let area = crop.pixels(in: size)
        guard area.width >= 1, area.height >= 1 else { return histogram }
        let stride = max(1, Int((area.width * area.height / Double(maxSamples)).squareRoot().rounded(.up)))
        // The proxy carries the sRGB curve in either display space, so one
        // code → L\* table serves the channels.
        let weights = luminanceWeights(EditorModel.displaySpace(of: surface))
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        let (width, height, rowBytes) = (surface.width, surface.height, surface.bytesPerRow)
        // A straightened crop is in the turned frame: turn each sample back
        // to the photograph, as `CropGeometry.turn(_:by: -angle, about:)`.
        let (sin, cos) = (Foundation.sin(-crop.angle * .pi / 180), Foundation.cos(-crop.angle * .pi / 180))
        let (cx, cy) = (size.width / 2, size.height / 2)
        let codeBins = Histogram.codeBins
        histogram.red.withUnsafeMutableBufferPointer { red in
            histogram.green.withUnsafeMutableBufferPointer { green in
                histogram.blue.withUnsafeMutableBufferPointer { blue in
                    histogram.lightness.withUnsafeMutableBufferPointer { lightness in
                        func count(_ x: Int, _ y: Int) {
                            let pixel = base + y * rowBytes + x * 4
                            red[Int(codeBins[Int(pixel[2])])] += 1
                            green[Int(codeBins[Int(pixel[1])])] += 1
                            blue[Int(codeBins[Int(pixel[0])])] += 1
                            let linear = SIMD3(
                                Histogram.codeLinear[Int(pixel[2])],
                                Histogram.codeLinear[Int(pixel[1])],
                                Histogram.codeLinear[Int(pixel[0])]
                            )
                            let luminance = weights.x * linear.x + weights.y * linear.y + weights.z * linear.z
                            lightness[Int(Histogram.bin(forLuminance: luminance))] += 1
                        }
                        for row in Swift.stride(from: 0, to: Int(area.height), by: stride) {
                            for column in Swift.stride(from: 0, to: Int(area.width), by: stride) {
                                let (dx, dy) = (area.minX + CGFloat(column) + 0.5 - cx, area.minY + CGFloat(row) + 0.5 - cy)
                                let (x, y) = (Int((cx + dx * cos - dy * sin).rounded(.down)), Int((cy + dx * sin + dy * cos).rounded(.down)))
                                if x >= 0, x < width, y >= 0, y < height { count(x, y) }
                            }
                        }
                    }
                }
            }
        }
        return histogram
    }

    /// Each code's linear value, so a count doesn't call `pow` per pixel.
    private static let codeLinear: [Float] = (0...255).map { srgbDecode(Float($0) / 255) }

    /// Every L\* code's bin, for a channel value read as a tone.
    private static let codeBins: [UInt8] = codeLinear.map(lstarBin)

    /// Bins for a linear luminance, at 2048 √Y steps so the shadows get the
    /// resolution the eye does.
    private static let luminanceBins: [UInt8] = (0..<2048).map { index in
        let y = Float(index) / 2047
        return lstarBin(y * y)
    }

    private static func bin(forLuminance y: Float) -> UInt8 {
        luminanceBins[Int(min(max(y, 0).squareRoot() * 2047, 2047))]
    }
}

/// The proxy's luminance weights: sRGB's, or Rec. 2020's for a wide render.
func luminanceWeights(_ displaySpace: String) -> SIMD3<Float> {
    displaySpace == "Rec. 2020 (sRGB transfer)"
        ? SIMD3(0.2627, 0.6780, 0.0593)
        : SIMD3(0.2126, 0.7152, 0.0722)
}

/// The sRGB transfer function, the proxy's encoding in either space.
func srgbDecode(_ v: Float) -> Float {
    v <= 0.040_45 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
}

/// CIE Lab lightness 0…100 of a linear luminance.
func lstar(_ y: Float) -> Double {
    let y = Double(max(y, 0))
    return y > 216.0 / 24389.0 ? 116 * cbrt(y) - 16 : y * 24389.0 / 27.0
}

/// The same lightness, binned into 0…255.
private func lstarBin(_ y: Float) -> UInt8 {
    UInt8(min(max(lstar(y) / 100, 0), 1) * 255)
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

