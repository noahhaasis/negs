import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// A camera's embedded preview, never a new RAW development. The small,
/// explicitly tagged sRGB image is only a brightness reference, not a look.
enum CameraBrightnessReference {
    static func png(from url: URL, expectedSize: CGSize) throws -> Data {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
            [kCGImageSourceShouldCache: false] as CFDictionary),
              let thumbnail = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: false,
                kCGImageSourceCreateThumbnailFromImageIfAbsent: false,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: 512,
                kCGImageSourceShouldCacheImmediately: true
              ] as CFDictionary) else {
            throw EngineError(message: "This RAW has no embedded camera preview. Adjust Exposure manually.")
        }
        guard thumbnail.width >= 64, thumbnail.height >= 64,
              expectedSize.width > 0, expectedSize.height > 0 else {
            throw EngineError(message: "The camera preview is too small to match brightness reliably.")
        }
        let aspect = Double(thumbnail.width) / Double(thumbnail.height)
        let expected = expectedSize.width / expectedSize.height
        guard max(aspect, expected) / min(aspect, expected) <= 1.02 else {
            throw EngineError(message: "The camera preview has a different crop. Adjust Exposure manually.")
        }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        let exif = properties?[kCGImagePropertyExifDictionary] as? [CFString: Any]
        let sRGBDeclared = (exif?[kCGImagePropertyExifColorSpace] as? NSNumber)?.intValue == 1
        let profile = (properties?[kCGImagePropertyProfileName] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let namedProfile = !profile.isEmpty && !["device rgb", "generic rgb"].contains(profile.lowercased())
        // An ImageIO-assigned default color space is not evidence that an
        // untagged preview has known colorimetry. Require camera metadata too.
        guard let sourceSpace = thumbnail.colorSpace,
              sRGBDeclared || namedProfile,
              sourceSpace.model == .rgb || sourceSpace.model == .monochrome,
              sourceSpace.copyICCData() != nil else {
            throw EngineError(message: "The camera preview has no usable color profile. Adjust Exposure manually.")
        }
        let scale = min(1, 512.0 / Double(max(thumbnail.width, thumbnail.height)))
        let width = max(1, Int((Double(thumbnail.width) * scale).rounded()))
        let height = max(1, Int((Double(thumbnail.height) * scale).rounded()))
        guard let sRGB = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: sRGB,
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue) else {
            throw EngineError(message: "The camera preview could not be prepared.")
        }
        context.interpolationQuality = .high
        context.draw(thumbnail, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let image = context.makeImage() else {
            throw EngineError(message: "The camera preview could not be prepared.")
        }
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else {
            throw EngineError(message: "The camera preview could not be prepared.")
        }
        CGImageDestinationAddImage(destination, image, [kCGImagePropertyOrientation: 1] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw EngineError(message: "The camera preview could not be prepared.")
        }
        return data as Data
    }
}

/// A fresh selection receives at most one automatic attempt. Restoration and
/// user edits never become eligible again when a render finishes or is retried.
struct ExposurePlacementGate {
    private(set) var automaticGeneration: Int?

    mutating func opened(generation: Int, restored: Bool, raw: Bool, enabled: Bool) {
        automaticGeneration = !restored && raw && enabled ? generation : nil
    }
    mutating func edited() { automaticGeneration = nil }
    mutating func takeAutomatic(generation: Int, raw: Bool, enabled: Bool) -> Bool {
        guard automaticGeneration == generation else { return false }
        automaticGeneration = nil
        return raw && enabled
    }
}
