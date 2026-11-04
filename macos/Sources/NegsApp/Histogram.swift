import Foundation
import IOSurface
import SwiftUI

/// The developed photograph's tones: 256 bins per channel of the cropped
/// photograph. The border is drawn on the canvas, never in the preview, so
/// it never counts.
struct Histogram: Equatable, Sendable {
    var red = [UInt32](repeating: 0, count: 256)
    var green = [UInt32](repeating: 0, count: 256)
    var blue = [UInt32](repeating: 0, count: 256)

    /// Counts the crop of a BGRA preview. A large crop is sampled on an even
    /// grid of at most `maxSamples` pixels, which keeps the shape and takes a
    /// few milliseconds at any preview size.
    static func of(_ surface: IOSurface, crop: CropSettings, maxSamples: Int = 1 << 20) -> Histogram {
        var histogram = Histogram()
        let size = CGSize(width: surface.width, height: surface.height)
        let area = crop.pixels(in: size)
        guard area.width >= 1, area.height >= 1 else { return histogram }
        let stride = max(1, Int((area.width * area.height / Double(maxSamples)).squareRoot().rounded(.up)))
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
                    func count(_ x: Int, _ y: Int) {
                        let pixel = base + y * rowBytes + x * 4
                        blue[Int(pixel[0])] += 1; green[Int(pixel[1])] += 1; red[Int(pixel[2])] += 1
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
        return histogram
    }

    /// The tallest bin, leaving out pure black and white so clipping doesn't
    /// flatten the rest; their spikes run off the top instead.
    var peak: UInt32 {
        let inner = [red, green, blue].map { $0[1..<255].max() ?? 0 }.max() ?? 0
        return inner > 0 ? inner : [red, green, blue].map { $0.max() ?? 0 }.max() ?? 0
    }
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
