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
    nonisolated static func load(_ url: URL, raw: Bool, lensCorrection: Bool, maxPixels: Int) -> IOSurface? {
        // Releases the decoder's full-size buffers before returning.
        autoreleasepool { render(url, raw: raw, lensCorrection: lensCorrection, maxPixels: maxPixels) }
    }

    /// The camera's own JPEG of a RAW (RAW+JPEG shooting), laid over the
    /// RAW's `frame` so it lines up with the developed image: the size the
    /// camera saved it at doesn't matter, and a JPEG the camera cropped to
    /// another shape (1:1, 16:9) sits centred on black, where it came from.
    nonisolated static func loadCameraPicture(_ url: URL, frame: CGSize, maxPixels: Int) -> IOSurface? {
        autoreleasepool {
            guard frame.width > 0, frame.height > 0, let picture = decoded(url, maxPixels: maxPixels),
                  !picture.extent.isEmpty else { return nil }
            let long = CGFloat(maxPixels), aspect = frame.width / frame.height
            let out = aspect >= 1 ? CGSize(width: long, height: (long / aspect).rounded())
                : CGSize(width: (long * aspect).rounded(), height: long)
            let size = picture.extent.size
            // Within 1% it's the same shape, stretched to the last pixel.
            let same = abs(size.width / size.height / aspect - 1) < 0.01
            let sx = same ? out.width / size.width : min(out.width / size.width, out.height / size.height)
            let sy = same ? out.height / size.height : sx
            let placed = picture.transformed(by: CGAffineTransform(translationX: -picture.extent.minX, y: -picture.extent.minY)
                .concatenating(CGAffineTransform(scaleX: sx, y: sy))
                .concatenating(CGAffineTransform(translationX: ((out.width - size.width * sx) / 2).rounded(),
                                                 y: ((out.height - size.height * sy) / 2).rounded())))
            let black = CIImage(color: .black).cropped(to: CGRect(origin: .zero, size: out))
            return render(placed.composited(over: black).cropped(to: CGRect(origin: .zero, size: out)))
        }
    }

    private static func render(_ url: URL, raw: Bool, lensCorrection: Bool, maxPixels: Int) -> IOSurface? {
        guard let image = (raw ? developed(url, lensCorrection: lensCorrection, maxPixels: maxPixels) : nil)
                ?? decoded(url, maxPixels: maxPixels) else { return nil }
        return render(image)
    }

    private static func render(_ image: CIImage) -> IOSurface? {
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

    /// Apple's default look, with lens correction as the engine's decode has
    /// it, so the framing matches the developed image.
    private static func developed(_ url: URL, lensCorrection: Bool, maxPixels: Int) -> CIImage? {
        guard let filter = CIRAWFilter(imageURL: url) else { return nil }
        if filter.isLensCorrectionSupported { filter.isLensCorrectionEnabled = lensCorrection }
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
