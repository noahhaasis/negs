import Foundation
import IOSurface
@testable import NegsApp

/// The tone a circle of a preview shows, for tests to check what printed. The
/// app measures pins in the helper, before the curve.
enum TonePick: Equatable, Sendable {
    case circle(centre: CGPoint, radius: Double)

    /// The average L\* of the pixels inside, or nil when none of the
    /// photograph falls inside the circle.
    func sample(_ surface: IOSurface, maxSamples: Int = 1 << 16) -> ToneSample? {
        let size = CGSize(width: surface.width, height: surface.height)
        guard size.width >= 1, size.height >= 1, case .circle(let centre, let radius) = self else { return nil }
        let shorter = min(size.width, size.height)
        let radiusPixels = CGFloat(radius) * shorter
        let reach = max(radiusPixels, 0.5)
        let area = CGRect(
            x: centre.x * size.width - reach, y: centre.y * size.height - reach,
            width: reach * 2, height: reach * 2
        ).intersection(CGRect(origin: .zero, size: size))
        let stride = max(1, Int((area.width * area.height / Double(maxSamples)).squareRoot().rounded(.up)))
        let (cx, cy) = (centre.x * size.width, centre.y * size.height)
        var pixels: [(x: Int, y: Int)] = []
        if radiusPixels <= 0 {
            // No radius: the pixel the point falls in, as the engine reads it.
            guard (0...1).contains(centre.x), (0...1).contains(centre.y) else { return nil }
            pixels = [(min(Int(cx), surface.width - 1), min(Int(cy), surface.height - 1))]
        } else {
        for row in Swift.stride(from: 0, to: Int(area.height), by: stride) {
            for column in Swift.stride(from: 0, to: Int(area.width), by: stride) {
                let x = Int(area.minX) + column, y = Int(area.minY) + row
                guard x < surface.width, y < surface.height else { continue }
                let dx = Double(x) + 0.5 - cx, dy = Double(y) + 0.5 - cy
                if radiusPixels > 0 && dx * dx + dy * dy > Double(radiusPixels * radiusPixels) { continue }
                pixels.append((x, y))
            }
        }
        }
        guard !pixels.isEmpty else { return nil }
        let weights = luminanceWeights(EditorModel.displaySpace(of: surface))
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        var lightness = 0.0
        for (x, y) in pixels {
            let pixel = base + y * surface.bytesPerRow + x * 4
            let linear = SIMD3(
                srgbDecode(Float(pixel[2]) / 255),
                srgbDecode(Float(pixel[1]) / 255),
                srgbDecode(Float(pixel[0]) / 255)
            )
            let y = weights.x * linear.x + weights.y * linear.y + weights.z * linear.z
            lightness += lstar(y)
        }
        return ToneSample(lstar: lightness / Double(pixels.count))
    }
}

/// A reference's tone: CIE L\* 0…100.
struct ToneSample: Equatable, Sendable {
    var lstar: Double
}
