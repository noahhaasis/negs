import ImageIO
import SwiftUI

/// What the camera wrote down: the caption over the photograph reads
/// camera, ISO, shutter, aperture and focal length (IMPLEMENTATION.md §4.1).
struct PhotoFacts: Equatable, Sendable {
    var camera: String?
    var iso: Double?
    /// Seconds.
    var exposure: Double?
    var aperture: Double?
    /// Millimetres.
    var focalLength: Double?

    /// Reads the file's EXIF and TIFF tags (≈1 ms; RAW files included). Off the main actor.
    nonisolated static func read(_ url: URL) -> PhotoFacts? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] else { return nil }
        return PhotoFacts(properties: properties)
    }

    init(camera: String? = nil, iso: Double? = nil, exposure: Double? = nil, aperture: Double? = nil, focalLength: Double? = nil) {
        self.camera = camera; self.iso = iso; self.exposure = exposure; self.aperture = aperture; self.focalLength = focalLength
    }

    init(properties: [CFString: Any]) {
        let exif = properties[kCGImagePropertyExifDictionary] as? [CFString: Any] ?? [:]
        let tiff = properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any] ?? [:]
        func number(_ value: Any?) -> Double? {
            guard let value = (value as? [Any])?.first ?? value else { return nil }
            guard let number = (value as? NSNumber)?.doubleValue, number > 0, number.isFinite else { return nil }
            return number
        }
        camera = Self.camera(make: tiff[kCGImagePropertyTIFFMake] as? String, model: tiff[kCGImagePropertyTIFFModel] as? String)
        iso = number(exif[kCGImagePropertyExifISOSpeedRatings])
        exposure = number(exif[kCGImagePropertyExifExposureTime])
        aperture = number(exif[kCGImagePropertyExifFNumber])
        focalLength = number(exif[kCGImagePropertyExifFocalLength])
    }

    /// "Nikon Z f" from "NIKON CORPORATION" and "NIKON Z f"; "Sony ILCE-7M3"
    /// from "SONY" and "ILCE-7M3". The maker's first word, in title case if
    /// it's shouted, then the model without the maker repeated.
    static func camera(make: String?, model: String?) -> String? {
        let model = model?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty
        guard let brand = make?.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: " ").first.map(String.init) else { return model }
        let name = brand == brand.uppercased() && brand.count > 2 ? brand.prefix(1) + brand.dropFirst().lowercased() : brand
        guard var model else { return String(name) }
        if model.lowercased().hasPrefix(brand.lowercased()) {
            model = model.dropFirst(brand.count).trimmingCharacters(in: .whitespaces)
        }
        return model.isEmpty ? String(name) : "\(name) \(model)"
    }

    /// "1/1250" under a second (and any whole fraction), else "2 s" or "1,3 s".
    static func shutter(_ seconds: Double) -> String {
        if seconds < 1 {
            let denominator = 1 / seconds
            if seconds < 0.3 || abs(denominator - denominator.rounded()) < 0.05 { return "1/\(Int(denominator.rounded()))" }
        }
        return "\(seconds.formatted(.number.precision(.fractionLength(0...1)))) s"
    }

    /// The caption's items, in order.
    var items: [String] {
        var items: [String] = []
        if let camera { items.append(camera) }
        if let iso { items.append("ISO \(Int(iso.rounded()))") }
        if let exposure { items.append(Self.shutter(exposure)) }
        if let aperture { items.append("f/\(aperture.formatted(.number.precision(.fractionLength(0...1))))") }
        if let focalLength { items.append("\(focalLength.formatted(.number.precision(.fractionLength(0...1)))) mm") }
        return items
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}

/// The caption 10 pt above the photograph: the camera's facts in italic
/// `label`, `·` between them in `track`, and the file name at the right in
/// `dim`. The file name goes first when it doesn't fit (the title bar has
/// it); a portrait's caption is centred and has none.
struct PhotoCaption: View {
    let facts: PhotoFacts?
    let fileName: String
    let portrait: Bool

    var body: some View {
        let items = facts?.items ?? []
        if portrait {
            line(items).frame(maxWidth: .infinity, alignment: .center)
        } else {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    line(items)
                    Spacer(minLength: 9)
                    Text(fileName).font(Theme.serif(15)).foregroundStyle(Theme.dim).lineLimit(1).fixedSize()
                }
                line(items).frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func line(_ items: [String]) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 9) {
            ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                if index > 0 { Text("·").font(Theme.serif(15)).foregroundStyle(Theme.track) }
                Text(item).font(Theme.serif(15, italic: true)).foregroundStyle(Theme.label).lineLimit(1).fixedSize()
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(items.joined(separator: ", "))
        .accessibilityAddTraits(.isStaticText)
    }
}
