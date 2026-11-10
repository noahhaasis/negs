import CoreGraphics
import CoreVideo
import Foundation
import IOSurface
import Metal
import MetalPerformanceShaders

/// The fitted photograph, resampled to exactly the physical pixels it covers.
///
/// Core Animation minifies a layer's contents with trilinear mipmaps, on
/// gamma-encoded values. Between two powers of two that loses about a third
/// of MTF50 and half the fine detail of a Lanczos downscale in linear light.
/// So once the fitted view settles, the canvas shows the whole photograph
/// resampled on the GPU (Lanczos, linear light) to round(W·s) × round(H·s)
/// physical pixels, drawn 1:1 on the pixel grid (`PhotographView`).
enum FitDownscale {
    /// The physical pixel size a photograph of `size` layout pixels covers at
    /// `scale` physical pixels per layout pixel; nil from 1:1 up.
    static func targetSize(photo size: CGSize, scale: CGFloat) -> (width: Int, height: Int)? {
        guard scale > 0, scale < 0.999, size.width >= 1, size.height >= 1 else { return nil }
        let width = Int((size.width * scale).rounded()), height = Int((size.height * scale).rounded())
        guard width >= 1, height >= 1 else { return nil }
        return (width, height)
    }

    /// The nearest point on the physical pixel grid, given the maps between
    /// a layer's coordinates and the window's backing pixels.
    static func snapped(_ point: CGPoint, toBacking: (CGPoint) -> CGPoint, fromBacking: (CGPoint) -> CGPoint) -> CGPoint {
        let backing = toBacking(point)
        return fromBacking(CGPoint(x: backing.x.rounded(), y: backing.y.rounded()))
    }

    /// A centre for a half turn (T held) that maps the pixel grid onto
    /// itself: on a whole or half physical pixel.
    static func snappedHalf(_ point: CGPoint, toBacking: (CGPoint) -> CGPoint, fromBacking: (CGPoint) -> CGPoint) -> CGPoint {
        let backing = toBacking(point)
        return fromBacking(CGPoint(x: (backing.x * 2).rounded() / 2, y: (backing.y * 2).rounded() / 2))
    }

    /// The tags of surfaces that hold 8-bit values with the sRGB curve, which
    /// the GPU decodes to linear light through an sRGB texture: the helper's
    /// sRGB and Rec. 2020 (sRGB transfer) previews and the Display P3 original.
    static let sRGBCurveSpaces: Set<String> = [CGColorSpace.sRGB as String, CGColorSpace.displayP3 as String, CGColorSpace.itur_2020_sRGBGamma as String]

    /// True if `surface` can be resampled in linear light.
    static func canResample(_ surface: IOSurface) -> Bool {
        guard surface.pixelFormat == kCVPixelFormatType_32BGRA, surface.planeCount <= 1, surface.width > 0, surface.height > 0,
              let tag = IOSurfaceCopyValue(surface, kIOSurfaceColorSpace),
              let name = CGColorSpace(propertyListPlist: tag as CFPropertyList)?.name else { return false }
        return sRGBCurveSpaces.contains(name as String)
    }
}

/// Lanczos-resamples IOSurfaces on the GPU, with Metal Performance Shaders.
/// Its Metal objects are made on first use, off the main thread, so nothing
/// of it runs before the first frame is on screen.
final class FitResampler: @unchecked Sendable {
    static let shared = FitResampler()

    private let queue = DispatchQueue(label: "dev.negs.fit-downscale", qos: .userInitiated)
    /// Made on `queue`, on first use.
    private var pipeline: (device: MTLDevice, queue: MTLCommandQueue, lanczos: MPSImageLanczosScale)??

    private func makePipeline() -> (device: MTLDevice, queue: MTLCommandQueue, lanczos: MPSImageLanczosScale)? {
        // Compute shaders write sRGB textures on Apple GPUs only.
        // TODO(intel): on an Intel Mac, write half floats and encode in a render pass.
        guard let device = MTLCreateSystemDefaultDevice(), device.supportsFamily(.apple2), MPSSupportsMTLDevice(device),
              let queue = device.makeCommandQueue() else { return nil }
        queue.label = "Fit downscale"
        return (device, queue, MPSImageLanczosScale(device: device))
    }

    /// `source` resampled to `width` × `height`, tagged with its colour space,
    /// or nil if the GPU can't. Calls `done` on the main queue.
    func resample(_ source: IOSurface, width: Int, height: Int, done: @escaping @MainActor @Sendable (IOSurface?) -> Void) {
        queue.async { [self] in
            let result = resampleNow(source, width: width, height: height)
            DispatchQueue.main.async { MainActor.assumeIsolated { done(result) } }
        }
    }

    private let lock = NSLock()

    /// Synchronous, on the caller's thread; for tests and measurements.
    func resampleNow(_ source: IOSurface, width: Int, height: Int) -> IOSurface? {
        lock.lock(); defer { lock.unlock() }
        if pipeline == nil { pipeline = .some(makePipeline()) }
        guard let made = pipeline, let (device, commands, lanczos) = made,
              width > 0, height > 0, FitDownscale.canResample(source),
              let tag = IOSurfaceCopyValue(source, kIOSurfaceColorSpace),
              let target = IOSurface(properties: [
                  .width: width, .height: height, .bytesPerElement: 4,
                  .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
                  .pixelFormat: kCVPixelFormatType_32BGRA
              ]) else { return nil }
        // sRGB formats: the GPU decodes reads to linear light and encodes writes.
        let input = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm_srgb, width: source.width, height: source.height, mipmapped: false)
        input.usage = .shaderRead
        input.storageMode = .shared
        let output = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm_srgb, width: width, height: height, mipmapped: false)
        output.usage = [.shaderRead, .shaderWrite]
        output.storageMode = .shared
        guard let from = device.makeTexture(descriptor: input, iosurface: source, plane: 0),
              let to = device.makeTexture(descriptor: output, iosurface: target, plane: 0),
              let buffer = commands.makeCommandBuffer() else { return nil }
        // No scale transform: the whole source fills the whole target.
        lanczos.edgeMode = .clamp
        lanczos.encode(commandBuffer: buffer, sourceTexture: from, destinationTexture: to)
        buffer.commit()
        buffer.waitUntilCompleted()
        guard buffer.status == .completed else { return nil }
        IOSurfaceSetValue(target, kIOSurfaceColorSpace, tag)
        return target
    }
}
