import CoreImage
import CoreVideo
import Foundation
import ImageIO
import IOSurface

/// The photograph before developing, for the press-and-hold comparison:
/// a RAW file as Apple's RAW developer shows it by default, anything else as
/// the file itself. Lines up with the developed image, which the canvas
/// stretches it over.
enum OriginalImage {
    // TODO(intel): RAW decoding time and memory were measured on Apple
    // silicon (~0.5 s cold, ~1 GB transient). Re-measure on Intel/AMD GPUs.
    private static let context = CIContext(options: [.cacheIntermediates: false])
    private static let colorSpace = CGColorSpace(name: CGColorSpace.displayP3)!

    /// Decodes `url` with its long edge at most `maxPixels`, upright like the
    /// engine's loader, into a Display P3 IOSurface.
    nonisolated static func load(_ url: URL, raw: Bool, maxPixels: Int) -> IOSurface? {
        // Releases the decoder's full-size buffers before returning.
        autoreleasepool { render(url, raw: raw, maxPixels: maxPixels) }
    }

    private static func render(_ url: URL, raw: Bool, maxPixels: Int) -> IOSurface? {
        guard let image = (raw ? developed(url, maxPixels: maxPixels) : nil) ?? decoded(url, maxPixels: maxPixels) else { return nil }
        let extent = image.extent.integral
        let width = Int(extent.width), height = Int(extent.height)
        guard width > 0, height > 0, let surface = IOSurface(properties: [
            .width: width, .height: height, .bytesPerElement: 4,
            .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
            .pixelFormat: kCVPixelFormatType_32BGRA
        ]) else { return nil }
        context.render(image.transformed(by: CGAffineTransform(translationX: -extent.minX, y: -extent.minY)),
                       to: surface, bounds: CGRect(x: 0, y: 0, width: width, height: height), colorSpace: colorSpace)
        // Core Image otherwise keeps over a gigabyte of full-size RAW
        // intermediates for the next render.
        context.clearCaches()
        IOSurfaceSetValue(surface, kIOSurfaceColorSpace, colorSpace.copyPropertyList()!)
        return surface
    }

    /// Apple's default look, without lens correction: the engine's decode
    /// leaves it off, so the framing matches the developed image.
    private static func developed(_ url: URL, maxPixels: Int) -> CIImage? {
        guard let filter = CIRAWFilter(imageURL: url) else { return nil }
        if filter.isLensCorrectionSupported { filter.isLensCorrectionEnabled = false }
        guard let full = filter.outputImage, !full.extent.isEmpty else { return nil }
        let scale = min(1, CGFloat(maxPixels) / max(full.extent.width, full.extent.height))
        guard scale < 1 else { return full }
        filter.scaleFactor = Float(scale)
        return filter.outputImage
    }

    /// The file as any viewer shows it, with its own color profile. A RAW file
    /// Apple can't develop falls back to its embedded preview.
    private static func decoded(_ url: URL, maxPixels: Int) -> CIImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
              let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                  kCGImageSourceCreateThumbnailFromImageAlways: true,
                  kCGImageSourceCreateThumbnailWithTransform: true,
                  kCGImageSourceThumbnailMaxPixelSize: maxPixels
              ] as CFDictionary) else { return nil }
        return CIImage(cgImage: image)
    }
}

extension IOSurface {
    /// Lets macOS discard the pixels under memory pressure. Volatile memory
    /// doesn't count toward the app's footprint.
    func makeVolatile() {
        var old = IOSurfacePurgeabilityState.purgeableKeepCurrent
        setPurgeable(.purgeableVolatile, oldState: &old)
    }
    /// Keeps the pixels again ([] is non-volatile). False if macOS discarded them meanwhile.
    func makeNonVolatile() -> Bool {
        var old = IOSurfacePurgeabilityState.purgeableKeepCurrent
        setPurgeable([], oldState: &old)
        return old != .purgeableEmpty
    }
}
