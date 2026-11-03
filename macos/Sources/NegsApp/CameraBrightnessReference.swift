// From yosoyelchris's Negs fork (`prep-raw-scopes`, d2a144d); see docs/prep.md.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Why a RAW's embedded preview can't serve as the reference. Retrying the
/// same photograph gives the same answer, so the app stops offering it.
struct CameraPreviewUnusable: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

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
            throw CameraPreviewUnusable(message: "This RAW has no camera preview to match.")
        }
        guard thumbnail.width >= 64, thumbnail.height >= 64,
              expectedSize.width > 0, expectedSize.height > 0 else {
            throw CameraPreviewUnusable(message: "The camera preview is too small to match.")
        }
        let expected = expectedSize.width / expectedSize.height
        var preview = thumbnail
        if !sameShape(Double(thumbnail.width) / Double(thumbnail.height), expected) {
            // Only previews of another shape pay for the bar search.
            guard let inside = trimmingBars(thumbnail, toAspect: expected) else {
                throw CameraPreviewUnusable(message: "The camera preview is framed differently from the photo.")
            }
            preview = inside
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
            throw CameraPreviewUnusable(message: "The camera preview has no color profile.")
        }
        return try encoded(preview, unusable: "The camera preview couldn’t be read.")
    }

    /// A JPEG, PNG or TIFF as its own brightness reference: the file is what
    /// the camera (or the editor before Negs) made, so the developed photo
    /// should be about as bright. Read through ImageIO with its colour
    /// profile, untagged as sRGB, as the engine's Automatic reading does.
    /// Transparent and floating-point files are refused: the engine drops
    /// alpha and reads untagged floats as linear, which ImageIO doesn't.
    static func filePNG(from url: URL, expectedSize: CGSize) throws -> Data {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary) else {
            throw CameraPreviewUnusable(message: "The photograph couldn’t be read for matching.")
        }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        if (properties?[kCGImagePropertyHasAlpha] as? NSNumber)?.boolValue == true {
            throw CameraPreviewUnusable(message: "Transparent images can’t be matched.")
        }
        if (properties?[kCGImagePropertyIsFloat] as? NSNumber)?.boolValue == true {
            throw CameraPreviewUnusable(message: "Floating-point images can’t be matched.")
        }
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: 512,
                kCGImageSourceShouldCacheImmediately: true
              ] as CFDictionary),
              image.width >= 2, image.height >= 2, expectedSize.width > 0, expectedSize.height > 0 else {
            throw CameraPreviewUnusable(message: "The photograph couldn’t be read for matching.")
        }
        guard sameShape(Double(image.width) / Double(image.height), expectedSize.width / expectedSize.height) else {
            throw CameraPreviewUnusable(message: "The photograph’s orientation couldn’t be matched.")
        }
        return try encoded(image, unusable: "The photograph couldn’t be read for matching.")
    }

    /// At most 512 px, 8-bit sRGB PNG, as the helper's reference.
    private static func encoded(_ preview: CGImage, unusable: String) throws -> Data {
        let scale = min(1, 512.0 / Double(max(preview.width, preview.height)))
        let width = max(1, Int((Double(preview.width) * scale).rounded()))
        let height = max(1, Int((Double(preview.height) * scale).rounded()))
        guard let sRGB = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: sRGB,
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue) else {
            throw CameraPreviewUnusable(message: unusable)
        }
        context.interpolationQuality = .high
        context.draw(preview, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let image = context.makeImage() else {
            throw CameraPreviewUnusable(message: unusable)
        }
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else {
            throw CameraPreviewUnusable(message: unusable)
        }
        CGImageDestinationAddImage(destination, image, [kCGImagePropertyOrientation: 1] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw CameraPreviewUnusable(message: unusable)
        }
        return data as Data
    }

    /// The note under the button for a failed match, and whether it holds for
    /// the photograph (the camera preview is the problem) or until the next
    /// edit. The engine's reasons are worded for logs, so they're matched on
    /// phrases from `exposure_placement.rs` and never shown as they are.
    static func failure(_ error: Error) -> (message: String, forPhoto: Bool) {
        if let unusable = error as? CameraPreviewUnusable { return (unusable.message, true) }
        let reason = error.localizedDescription
        if reason.contains("midtone quantiles") {
            return ("The camera preview is too dark or too bright to match.", true)
        }
        if reason.contains("±3 EV") { return ("Matching would take more than ±3 EV.", false) }
        return ("No reliable brightness match with these settings.", false)
    }

    static func sameShape(_ aspect: Double, _ expected: Double) -> Bool {
        max(aspect, expected) / min(aspect, expected) <= 1.02
    }

    /// The picture inside black bars, when that has the photo's shape. Some
    /// cameras letterbox a 4:3 preview of their 3:2 frame: the Leica M8 with 13
    /// rows top and bottom (320×240), the M (Typ 240) and Monochrom (Typ 246)
    /// with 14 rows at the top only (160×120); also the Sony A100, Minolta
    /// Dynax 7D and Nikon D1. Each edge is searched on its own, so one-sided
    /// bars work; a dark scene edge that isn't a bar leaves the wrong shape and
    /// is refused like any other crop.
    static func trimmingBars(_ image: CGImage, toAspect expected: Double) -> CGImage? {
        let width = image.width, height = image.height
        guard width > 0, height > 0,
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width,
                                      space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return nil }
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let data = context.data else { return nil }
        // Row 0 of the bitmap is the image's top row. Bars measure 0–4 of 255;
        // 24 leaves room for JPEG noise without taking in shadows with detail.
        let pixels = data.bindMemory(to: UInt8.self, capacity: width * height)
        let black: UInt8 = 24
        func barRow(_ y: Int) -> Bool { (0..<width).allSatisfy { pixels[y * width + $0] <= black } }
        var top = 0, bottom = height
        while top < bottom, barRow(top) { top += 1 }
        while bottom > top, barRow(bottom - 1) { bottom -= 1 }
        func barColumn(_ x: Int) -> Bool { (top..<bottom).allSatisfy { pixels[$0 * width + x] <= black } }
        var left = 0, right = width
        while left < right, barColumn(left) { left += 1 }
        while right > left, barColumn(right - 1) { right -= 1 }
        let inside = (width: right - left, height: bottom - top)
        guard inside.width < width || inside.height < height, inside.width >= 64, inside.height >= 64,
              sameShape(Double(inside.width) / Double(inside.height), expected) else { return nil }
        return image.cropping(to: CGRect(x: left, y: top, width: inside.width, height: inside.height))
    }
}
