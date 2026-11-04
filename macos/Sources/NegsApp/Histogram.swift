import CoreGraphics
import Foundation
import IOSurface
import SwiftUI

/// How a proxy's bytes become the output space's code values: the matrix
/// from the proxy's linear RGB to the output space's linear RGB, and a
/// 1024-entry table on √linear giving the output-encoded value.
///
/// The helper sends both with every render; `.identity` describes an sRGB
/// proxy whose output is sRGB, as in tests and before a first render.
struct OutputMapping: Equatable, Sendable {
    /// One row per output channel.
    private let matrix: [SIMD3<Float>]
    /// `encode[i]` is the output-encoded value for `linear = (i / 1023)^2`.
    private let encode: [Float]
    /// The proxy is already the output: the bytes are the codes.
    private let isIdentity: Bool

    static let identity = OutputMapping(
        matrix: [SIMD3(1, 0, 0), SIMD3(0, 1, 0), SIMD3(0, 0, 1)],
        encode: OutputMapping.srgbTable,
        isIdentity: true
    )

    init?(matrix: JSONValue, encode: JSONValue) {
        let rows = matrix.array
        let table = encode.array.map(\.double)
        guard rows.count == 3, rows.allSatisfy({ $0.array.count == 3 }),
              table.count > 1, table.allSatisfy(\.isFinite)
        else { return nil }
        let rows3 = rows.map {
            SIMD3(Float($0.array[0].double), Float($0.array[1].double), Float($0.array[2].double))
        }
        guard rows3.allSatisfy({ $0.x.isFinite && $0.y.isFinite && $0.z.isFinite }) else { return nil }
        self.init(
            matrix: rows3, encode: table.map(Float.init),
            isIdentity: Self.isIdentity(rows3, table)
        )
    }

    private init(matrix: [SIMD3<Float>], encode: [Float], isIdentity: Bool) {
        self.matrix = matrix
        self.encode = encode
        self.isIdentity = isIdentity
    }

    /// The output-encoded code values (0…1) of one proxy pixel's linear RGB.
    /// With an identity mapping the proxy's own transfer is exact; otherwise
    /// the table's square-root spacing puts its resolution near black.
    /// Colours a wide proxy clipped stay clipped.
    func encoded(linear: SIMD3<Float>) -> SIMD3<Float> {
        if isIdentity {
            return SIMD3(
                Self.srgbEncode(linear.x), Self.srgbEncode(linear.y), Self.srgbEncode(linear.z)
            )
        }
        var out = SIMD3<Float>(0, 0, 0)
        for channel in 0..<3 {
            let row = matrix[channel]
            let value = row.x * linear.x + row.y * linear.y + row.z * linear.z
            out[channel] = Self.lookup(encode, sqrt(max(value, 0)))
        }
        return out
    }

    private static func lookup(_ table: [Float], _ x: Float) -> Float {
        let t = min(max(x, 0), 1) * Float(table.count - 1)
        let index = min(Int(t), table.count - 2)
        let fraction = t - Float(index)
        return table[index] + (table[index + 1] - table[index]) * fraction
    }

    private static func isIdentity(_ matrix: [SIMD3<Float>], _ table: [Double]) -> Bool {
        guard matrix == [SIMD3(1, 0, 0), SIMD3(0, 1, 0), SIMD3(0, 0, 1)],
              table.count == 1024
        else { return false }
        return table.indices.allSatisfy { index in
            let linear = pow(Double(index) / 1023, 2)
            return abs(table[index] - srgbEncode(linear)) < 1e-5
        }
    }

    /// The sRGB transfer function, the proxy's own encoding.
    static func srgbEncode(_ x: Double) -> Double {
        x <= 0.003_130_8 ? 12.92 * x : 1.055 * pow(x, 1 / 2.4) - 0.055
    }
    static func srgbEncode(_ x: Float) -> Float {
        x <= 0.003_130_8 ? 12.92 * x : 1.055 * pow(x, 1 / 2.4) - 0.055
    }
    static func srgbDecode(_ v: Float) -> Float {
        v <= 0.040_45 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
    }

    private static let srgbTable: [Float] = (0..<1024).map { index in
        Float(srgbEncode(pow(Double(index) / 1023, 2)))
    }
}

/// The developed photograph's tones: 256 bins per channel of the cropped
/// photograph, and 256 bins of CIE Lab lightness. The border is drawn on the
/// canvas, never in the preview, so it never counts.
struct Histogram: Equatable, Sendable {
    var red = [UInt32](repeating: 0, count: 256)
    var green = [UInt32](repeating: 0, count: 256)
    var blue = [UInt32](repeating: 0, count: 256)
    /// L\* × 2.55, so the lightness shares the graph's 0…255 axis.
    var lightness = [UInt32](repeating: 0, count: 256)

    /// Counts the crop of a BGRA preview. A large crop is sampled on an even
    /// grid of at most `maxSamples` pixels, which keeps the shape and takes a
    /// few milliseconds at any preview size.
    ///
    /// R, G and B are code values of `output`'s space: an sRGB proxy is read
    /// directly, and a wide one maps back through the render's matrix. L\* is
    /// CIE Lab lightness of the proxy's own luminance, so it is the tone of
    /// the photograph rather than of one output space.
    static func of(
        _ surface: IOSurface,
        crop: CropSettings,
        output: OutputMapping = .identity,
        maxSamples: Int = 1 << 20
    ) -> Histogram {
        var histogram = Histogram()
        let size = CGSize(width: surface.width, height: surface.height)
        let area = crop.pixels(in: size)
        guard area.width >= 1, area.height >= 1 else { return histogram }
        let stride = max(1, Int((area.width * area.height / Double(maxSamples)).squareRoot().rounded(.up)))
        let space = EditorModel.displaySpace(of: surface)
        let weights = luminanceWeights(space)
        let maps = space != "sRGB"
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        let (width, height, rowBytes) = (surface.width, surface.height, surface.bytesPerRow)
        // A straightened crop is in the turned frame: turn each sample back
        // to the photograph, as `CropGeometry.turn(_:by: -angle, about:)`.
        let (sin, cos) = (Foundation.sin(-crop.angle * .pi / 180), Foundation.cos(-crop.angle * .pi / 180))
        let (cx, cy) = (size.width / 2, size.height / 2)
        histogram.red.withUnsafeMutableBufferPointer { red in
            histogram.green.withUnsafeMutableBufferPointer { green in
                histogram.blue.withUnsafeMutableBufferPointer { blue in
                    histogram.lightness.withUnsafeMutableBufferPointer { lightness in
                        func count(_ x: Int, _ y: Int) {
                            let pixel = base + y * rowBytes + x * 4
                            let linear = SIMD3(
                                OutputMapping.srgbDecode(Float(pixel[2]) / 255),
                                OutputMapping.srgbDecode(Float(pixel[1]) / 255),
                                OutputMapping.srgbDecode(Float(pixel[0]) / 255)
                            )
                            let codes = maps
                                ? output.encoded(linear: linear) * 255
                                : SIMD3(Float(pixel[2]), Float(pixel[1]), Float(pixel[0]))
                            red[Int(codes.x.rounded())] += 1
                            green[Int(codes.y.rounded())] += 1
                            blue[Int(codes.z.rounded())] += 1
                            let y = weights.x * linear.x + weights.y * linear.y + weights.z * linear.z
                            lightness[Int(min(max(y.lstar * 2.55, 0), 255))] += 1
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

    /// The tallest bin, leaving out the clipped ends so they don't flatten
    /// the rest; their spikes run off the top instead. Lightness never sets
    /// the scale.
    var peak: UInt32 {
        let inner = [red, green, blue].map { $0[5...250].max() ?? 0 }.max() ?? 0
        return inner > 0 ? inner : [red, green, blue].map { $0.max() ?? 0 }.max() ?? 0
    }
}

/// The proxy's luminance weights: sRGB's, or Rec. 2020's for a wide render.
private func luminanceWeights(_ displaySpace: String) -> SIMD3<Float> {
    displaySpace == "Rec. 2020 (sRGB transfer)"
        ? SIMD3(0.2627, 0.6780, 0.0593)
        : SIMD3(0.2126, 0.7152, 0.0722)
}

private extension Float {
    /// CIE Lab lightness of a linear Y, on the 216/24389 breakpoint.
    var lstar: Float { Float(lstarValue(Double(self))) }
}

private func lstarValue(_ y: Double) -> Double {
    y > 216.0 / 24389.0 ? 116 * cbrt(y) - 16 : y * 24389.0 / 27.0
}


/// A control-click on the canvas: a 3×3 or 7×7 sample about a point (unit
/// coordinates of the uncropped photograph, top left), or the whole dragged
/// region (as the histogram's crop).
enum TonePick: Equatable, Sendable {
    case small(CGPoint)
    case large(CGPoint)
    case region(CropSettings)

    /// The average of the picked pixels, in output-space code values and CIE
    /// L\*. Nil when no pixel of the photograph falls inside the pick.
    func sample(_ surface: IOSurface, output: OutputMapping, maxSamples: Int = 1 << 16) -> ToneSample? {
        let size = CGSize(width: surface.width, height: surface.height)
        guard size.width >= 1, size.height >= 1 else { return nil }
        let pixels: [(x: Int, y: Int)]
        switch self {
        case .small(let point):
            pixels = Self.window(around: point, radius: 1, size: size)
        case .large(let point):
            pixels = Self.window(around: point, radius: 3, size: size)
        case .region(let crop):
            pixels = Self.region(crop, size: size, maxSamples: maxSamples)
        }
        guard !pixels.isEmpty else { return nil }
        let space = EditorModel.displaySpace(of: surface)
        let weights = luminanceWeights(space)
        let maps = space != "sRGB"
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        var codes = SIMD3<Double>(0, 0, 0)
        var lightness = 0.0
        for (x, y) in pixels {
            let pixel = base + y * surface.bytesPerRow + x * 4
            let linear = SIMD3(
                OutputMapping.srgbDecode(Float(pixel[2]) / 255),
                OutputMapping.srgbDecode(Float(pixel[1]) / 255),
                OutputMapping.srgbDecode(Float(pixel[0]) / 255)
            )
            let outputCodes = maps
                ? output.encoded(linear: linear) * 255
                : SIMD3(Float(pixel[2]), Float(pixel[1]), Float(pixel[0]))
            codes += SIMD3(Double(outputCodes.x), Double(outputCodes.y), Double(outputCodes.z))
            let y = weights.x * linear.x + weights.y * linear.y + weights.z * linear.z
            lightness += lstarValue(Double(y))
        }
        let count = Double(pixels.count)
        return ToneSample(
            red: codes.x / count, green: codes.y / count, blue: codes.z / count,
            lstar: lightness / count
        )
    }

    /// The in-image pixels of a square window about `point`.
    private static func window(around point: CGPoint, radius: Int, size: CGSize) -> [(x: Int, y: Int)] {
        let (width, height) = (Int(size.width), Int(size.height))
        let (cx, cy) = (Int((point.x * size.width).rounded(.down)), Int((point.y * size.height).rounded(.down)))
        guard (0..<width).contains(cx), (0..<height).contains(cy) else { return [] }
        var pixels: [(Int, Int)] = []
        for y in (cy - radius)...(cy + radius) where (0..<height).contains(y) {
            for x in (cx - radius)...(cx + radius) where (0..<width).contains(x) {
                pixels.append((x, y))
            }
        }
        return pixels
    }

    /// The pixels a region covers, sampled evenly like the histogram's count
    /// so a full frame stays cheap. The region is in the turned frame, as
    /// `CropSettings` and the histogram read it.
    private static func region(_ crop: CropSettings, size: CGSize, maxSamples: Int) -> [(x: Int, y: Int)] {
        let area = crop.pixels(in: size)
        guard area.width >= 1, area.height >= 1 else { return [] }
        let stride = max(1, Int((area.width * area.height / Double(maxSamples)).squareRoot().rounded(.up)))
        let (sin, cos) = (Foundation.sin(-crop.angle * .pi / 180), Foundation.cos(-crop.angle * .pi / 180))
        let (cx, cy) = (size.width / 2, size.height / 2)
        let (width, height) = (Int(size.width), Int(size.height))
        var pixels: [(Int, Int)] = []
        for row in Swift.stride(from: 0, to: Int(area.height), by: stride) {
            for column in Swift.stride(from: 0, to: Int(area.width), by: stride) {
                let (dx, dy) = (area.minX + CGFloat(column) + 0.5 - cx, area.minY + CGFloat(row) + 0.5 - cy)
                let (x, y) = (Int((cx + dx * cos - dy * sin).rounded(.down)), Int((cy + dx * sin + dy * cos).rounded(.down)))
                if x >= 0, x < width, y >= 0, y < height { pixels.append((x, y)) }
            }
        }
        return pixels
    }
}

/// A picked tone: output-space code values 0–255, and CIE L\* 0–100.
struct ToneSample: Equatable, Sendable {
    var red: Double
    var green: Double
    var blue: Double
    var lstar: Double
}


/// Red, green, and blue added on a dark well, as in photo editors: where
/// channels overlap they mix to cyan, magenta, yellow, and white.
struct HistogramView: View {
    let histogram: Histogram?

    var body: some View {
        Canvas { context, size in
            guard let histogram, histogram.peak > 0 else { return }
            let peak = Double(histogram.peak)
            context.blendMode = .plusLighter
            for (bins, color) in [(histogram.red, Color(red: 1, green: 0.2, blue: 0.2)),
                                  (histogram.green, Color(red: 0.2, green: 1, blue: 0.2)),
                                  (histogram.blue, Color(red: 0.25, green: 0.35, blue: 1))] {
                var path = Path()
                path.move(to: CGPoint(x: 0, y: size.height))
                for (index, count) in bins.enumerated() {
                    let x = size.width * CGFloat(index) / CGFloat(bins.count - 1)
                    path.addLine(to: CGPoint(x: x, y: size.height * (1 - min(Double(count) / peak, 1))))
                }
                path.addLine(to: CGPoint(x: size.width, y: size.height))
                path.closeSubpath()
                context.fill(path, with: .color(color.opacity(0.75)))
            }
        }
        .padding(.horizontal, 4).padding(.top, 6)
        .background(Color(white: 0.1), in: RoundedRectangle(cornerRadius: 6))
        .accessibilityElement()
        .accessibilityLabel("Histogram")
        .accessibilityValue(histogram == nil ? "No developed photograph" : "Red, green, and blue tones of the developed photograph")
    }
}

