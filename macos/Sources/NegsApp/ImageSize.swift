import Foundation

/// Image Size: the resolution the film develops at. The source is resized
/// before the simulation, so grain and halation form at that size. A Longest
/// side target (`io.photo_long_side_px`) describes the cropped photograph,
/// before borders and export reductions; the crop still trims the developed
/// frame, so only the crop's extent sets the working size, never its position.
/// Older edits may instead carry a legacy factor (`io.upscale_factor`).
///
/// The helper resolves sizes with `spektrafilm-io::image_size`; this projects
/// the same frame for readouts and crop drafts without asking it. Keep the
/// formula and the limit in sync.
enum ImageSize {
    static let targetPath = "io.photo_long_side_px"
    static let factorPath = "io.upscale_factor"
    static let range = 16...65_535
    /// The largest working frame an enlargement may request, in pixels.
    static let maxWorkingPixels = 128_000_000.0

    enum Mode: Hashable {
        case original, longestSide
        /// A working-resolution factor other than 1 from an older edit.
        case legacy(Double)
    }

    static func mode(of params: JSONValue) -> Mode {
        if case .number = params[targetPath] { return .longestSide }
        let factor = params[factorPath]
        if case .number(let value) = factor, abs(value - 1) >= 1e-6 { return .legacy(value) }
        return .original
    }

    static func target(of params: JSONValue) -> Int? {
        if case .number(let value) = params[targetPath] { return Int(value) }
        return nil
    }

    /// The whole frame the film develops at, for a `source` of that size.
    /// Matches `image_size::resolve`, including the legacy factor's f32 rounding.
    static func working(source: CGSize, params: JSONValue, crop: CropSettings) -> CGSize {
        switch mode(of: params) {
        case .original: return source
        case .legacy(let factor):
            let f = Float(factor)
            return CGSize(width: Double(max((Float(source.width) * f).rounded(), 1)), height: Double(max((Float(source.height) * f).rounded(), 1)))
        case .longestSide: return working(source: source, target: target(of: params) ?? range.lowerBound, crop: crop)
        }
    }

    /// The frame that gives `crop` a longest side of `target`: scaled from the
    /// crop's extent only.
    static func working(source: CGSize, target: Int, crop: CropSettings) -> CGSize {
        let extent = crop.cropped ? crop.rect.size : CGSize(width: 1, height: 1)
        let scale = Double(target) / max(extent.width * source.width, extent.height * source.height)
        return CGSize(width: max((source.width * scale).rounded(), 1), height: max((source.height * scale).rounded(), 1))
    }

    /// An enlargement past the working limit, which the helper refuses.
    static func exceedsLimit(working: CGSize, source: CGSize) -> Bool {
        let pixels = working.width * working.height
        return pixels > source.width * source.height && pixels > maxWorkingPixels
    }

    static func megapixels(_ size: CGSize) -> String {
        let mp = size.width * size.height / 1e6
        return mp.formatted(FloatingPointFormatStyle<Double>.number.precision(.fractionLength(mp < 10 ? 1 : 0)))
    }

    static func limitMessage(working: CGSize) -> String {
        "Too large to develop: needs \(Int(working.width)) × \(Int(working.height)) px (\(megapixels(working)) MP), more than \(Int(maxWorkingPixels / 1e6)) MP. Choose a smaller longest side or a larger crop."
    }

    /// The last Longest side the user chose, offered when switching to it again.
    /// Remembered separately from the edit, and never switches the mode itself.
    static var remembered: Int? {
        get {
            let value = UserDefaults.standard.integer(forKey: "imageSizeLongestSide")
            return range.contains(value) ? value : nil
        }
        set { UserDefaults.standard.set(newValue, forKey: "imageSizeLongestSide") }
    }
}
