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

    static func of(_ surface: IOSurface, crop: CropSettings, maxSamples: Int = 1 << 20) -> Histogram {
        ImageScopes.of(surface, crop: crop, maxSamples: maxSamples).histogram
    }

    /// The tallest bin, leaving out pure black and white so clipping doesn't
    /// flatten the rest; their spikes run off the top instead.
    var peak: UInt32 {
        let inner = [red, green, blue].map { $0[1..<255].max() ?? 0 }.max() ?? 0
        return inner > 0 ? inner : [red, green, blue].map { $0.max() ?? 0 }.max() ?? 0
    }
}

/// RGB signal level against horizontal position in the displayed crop.
/// Bins are row-major, with level 0 at row 0 and 255 at row 255.
struct Waveform: Equatable, Sendable {
    let width: Int
    let red: [UInt32]
    let green: [UInt32]
    let blue: [UInt32]
    /// A small precomputed raster keeps drawing independent of photo size.
    let rgba: [UInt8]
    let sampleCount: Int

    init(width: Int = 1, red: [UInt32] = Array(repeating: 0, count: 256),
         green: [UInt32] = Array(repeating: 0, count: 256), blue: [UInt32] = Array(repeating: 0, count: 256)) {
        self.width = width; self.red = red; self.green = green; self.blue = blue
        sampleCount = Int(red.reduce(0, +))
        let peak = max(red.max() ?? 0, green.max() ?? 0, blue.max() ?? 0)
        let normalizer = log1p(Double(peak))
        var pixels = [UInt8](repeating: 0, count: width * 256 * 4)
        if peak > 0 {
            func intensity(_ count: UInt32) -> UInt8 {
                count == 0 ? 0 : UInt8((255 * (0.18 + 0.82 * log1p(Double(count)) / normalizer)).rounded())
            }
            for level in 0..<256 {
                for x in 0..<width {
                    let bin = level * width + x, pixel = ((255 - level) * width + x) * 4
                    pixels[pixel] = intensity(red[bin]); pixels[pixel + 1] = intensity(green[bin]); pixels[pixel + 2] = intensity(blue[bin])
                    pixels[pixel + 3] = max(pixels[pixel], pixels[pixel + 1], pixels[pixel + 2])
                }
            }
        }
        rgba = pixels
    }

    var image: CGImage? {
        guard sampleCount > 0, let provider = CGDataProvider(data: Data(rgba) as CFData) else { return nil }
        return CGImage(width: width, height: 256, bitsPerComponent: 8, bitsPerPixel: 32,
                       bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }
}

/// Both scopes are derived from the same bounded grid and crop transform.
/// A single surface lock/pass avoids competing background scans on every edit.
struct ImageScopes: Equatable, Sendable {
    var histogram = Histogram()
    var waveform = Waveform()

    static func of(_ surface: IOSurface, crop: CropSettings, rotation: Int = 0, maxSamples: Int = 1 << 20) -> ImageScopes {
        let size = CGSize(width: surface.width, height: surface.height)
        let area = crop.pixels(in: size)
        guard area.width >= 1, area.height >= 1, maxSamples > 0 else { return ImageScopes() }
        let width = Int(area.width), height = Int(area.height)
        // This also bounds very thin panoramas: a single common pixel stride
        // can exceed the budget when one of the dimensions is much shorter.
        let columns = min(width, maxSamples, max(1, Int((Double(maxSamples) * Double(width) / Double(height)).squareRoot())))
        let rows = min(height, max(1, maxSamples / columns))
        let turn = (rotation % 4 + 4) % 4
        let shownColumns = turn % 2 == 0 ? columns : rows
        let waveWidth = min(256, shownColumns)
        var red = [UInt32](repeating: 0, count: waveWidth * 256)
        var green = red, blue = red
        var histogram = Histogram()
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        let rowBytes = surface.bytesPerRow
        // A straightened crop is in the turned frame: turn each sample back
        // to the photograph, as CropGeometry.turn(_:by: -angle, about:).
        let (sin, cos) = (Foundation.sin(-crop.angle * .pi / 180), Foundation.cos(-crop.angle * .pi / 180))
        let (cx, cy) = (size.width / 2, size.height / 2)
        for row in 0..<rows {
            for column in 0..<columns {
                let dx = area.minX + (Double(column) + 0.5) * area.width / Double(columns) - cx
                let dy = area.minY + (Double(row) + 0.5) * area.height / Double(rows) - cy
                let x = Int((cx + dx * cos - dy * sin).rounded(.down))
                let y = Int((cy + dx * sin + dy * cos).rounded(.down))
                guard x >= 0, x < surface.width, y >= 0, y < surface.height else { continue }
                let pixel = base + y * rowBytes + x * 4
                let (r, g, b) = (Int(pixel[2]), Int(pixel[1]), Int(pixel[0]))
                histogram.red[r] += 1; histogram.green[g] += 1; histogram.blue[b] += 1
                let shownColumn: Int
                switch turn {
                case 1: shownColumn = rows - 1 - row
                case 2: shownColumn = columns - 1 - column
                case 3: shownColumn = row
                default: shownColumn = column
                }
                let waveX = shownColumn * waveWidth / shownColumns
                red[r * waveWidth + waveX] += 1; green[g * waveWidth + waveX] += 1; blue[b * waveWidth + waveX] += 1
            }
        }
        return ImageScopes(histogram: histogram, waveform: Waveform(width: waveWidth, red: red, green: green, blue: blue))
    }
}

/// Filled channels and fine outlines echo Sawmill's histogram.
struct HistogramView: View {
    let histogram: Histogram?

    var body: some View {
        Canvas { context, size in
            scopeGrid(context: &context, size: size)
            guard let histogram, histogram.peak > 0 else { return }
            let peak = Double(histogram.peak)
            context.blendMode = .plusLighter
            for (bins, color) in [(histogram.red, Color(red: 1, green: 0.42, blue: 0.42)),
                                  (histogram.green, Color(red: 0.42, green: 0.8, blue: 0.47)),
                                  (histogram.blue, Color(red: 0.3, green: 0.59, blue: 1))] {
                var line = Path()
                for (index, count) in bins.enumerated() {
                    let point = CGPoint(x: size.width * CGFloat(index) / CGFloat(bins.count - 1),
                                        y: size.height * (1 - min(Double(count) / peak, 1)))
                    if index == 0 { line.move(to: point) } else { line.addLine(to: point) }
                }
                var fill = line
                fill.addLine(to: CGPoint(x: size.width, y: size.height))
                fill.addLine(to: CGPoint(x: 0, y: size.height))
                fill.closeSubpath()
                context.fill(fill, with: .color(color.opacity(0.4)))
                context.stroke(line, with: .color(color.opacity(0.9)), lineWidth: 1)
            }
        }
        .padding(.horizontal, 4).padding(.top, 6)
        .background(Color(white: 0.075), in: RoundedRectangle(cornerRadius: 6))
        .accessibilityElement()
        .accessibilityLabel("Histogram")
        .accessibilityValue(histogram == nil ? "No developed photograph" : "Red, green, and blue tones of the developed photograph")
    }
}

struct WaveformView: View {
    let waveform: Waveform?

    var body: some View {
        HStack(spacing: 4) {
            VStack {
                ForEach([100, 75, 50, 25, 0], id: \.self) { level in
                    Text("\(level)").font(.system(size: 9, design: .monospaced)).foregroundStyle(Color.white.opacity(0.6))
                    if level != 0 { Spacer(minLength: 0) }
                }
            }
            .frame(width: 22)
            Canvas { context, size in
                scopeGrid(context: &context, size: size)
                if let image = waveform?.image {
                    context.draw(Image(decorative: image, scale: 1), in: CGRect(origin: .zero, size: size))
                }
            }
        }
        .padding(6)
        .background(Color(white: 0.075), in: RoundedRectangle(cornerRadius: 6))
        .accessibilityElement()
        .accessibilityLabel("RGB waveform")
        .accessibilityValue(waveform == nil ? "No developed photograph" : "Red, green, and blue signal levels from zero to one hundred percent across the developed photograph")
    }
}

private func scopeGrid(context: inout GraphicsContext, size: CGSize) {
    var grid = Path()
    for fraction in [0.25, 0.5, 0.75] {
        grid.move(to: CGPoint(x: 0, y: size.height * fraction))
        grid.addLine(to: CGPoint(x: size.width, y: size.height * fraction))
    }
    context.stroke(grid, with: .color(.white.opacity(0.09)), lineWidth: 0.5)
}

enum ScopeMode: String, CaseIterable {
    case histogram = "Histogram", waveform = "Waveform"
}

struct ScopePanelView: View {
    let histogram: Histogram?
    let waveform: Waveform?
    @AppStorage("scopeMode") private var mode = ScopeMode.histogram

    var body: some View {
        VStack(spacing: 6) {
            HStack {
                Picker("Scope", selection: $mode) {
                    ForEach(ScopeMode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.menu).labelsHidden().fixedSize().controlSize(.small)
                .accessibilityLabel("Scope view")
                Spacer()
                Text("RGB").font(.caption2).foregroundStyle(.secondary)
            }
            Group {
                switch mode {
                case .histogram: HistogramView(histogram: histogram)
                case .waveform: WaveformView(waveform: waveform)
                }
            }
            .frame(height: 190)
            .overlay {
                if histogram == nil {
                    Text("No developed photograph").font(.caption).foregroundStyle(Color.white.opacity(0.6))
                }
            }
        }
    }
}
