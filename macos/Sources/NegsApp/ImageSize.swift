import Foundation

/// Image Size: the resolution the film develops at. The source is resized
/// before the simulation, so grain and halation form at that size. A Longest
/// side target (`io.photo_long_side_px`) describes the cropped photograph,
/// before borders and export reductions; the crop still trims the developed
/// frame, so only the crop's extent sets the working size, never its position.
/// The engine's older working-resolution factor (`io.upscale_factor`) stays
/// at 1 in the app; restoring a session resets it.
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

    enum Mode { case original, longestSide }

    static func mode(of params: JSONValue) -> Mode { target(of: params) == nil ? .original : .longestSide }

    static func target(of params: JSONValue) -> Int? {
        if case .number(let value) = params[targetPath] { return Int(value) }
        return nil
    }

    /// The whole frame the film develops at, for a `source` of that size.
    static func working(source: CGSize, params: JSONValue, crop: CropSettings) -> CGSize {
        target(of: params).map { working(source: source, target: $0, crop: crop) } ?? source
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
