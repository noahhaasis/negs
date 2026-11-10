import CoreGraphics
import Foundation
import ImageIO

/// Each photograph's shape (width ÷ height, as shown), so the strip and the
/// grid lay frames out at their own proportions before any thumbnail is
/// decoded (§4.4, §4.6). A folder's are read from the files' headers, nearest
/// the open photograph first, at utility priority: about 5 ms a RAW. A
/// thumbnail once shown is the final word, so an edited photograph's crop
/// and rotation change its frame.
@MainActor @Observable final class PhotoAspects {
    /// Until a photograph's own is known: 3:2, the commonest.
    nonisolated static let fallback: CGFloat = 1.5

    private(set) var ratios: [URL: CGFloat] = [:]
    /// Learnt from a thumbnail shown, which a header read never replaces.
    @ObservationIgnored private var seen: Set<URL> = []
    @ObservationIgnored private var reading: Task<Void, Never>?

    func ratio(of item: URL) -> CGFloat { ratios[item] ?? Self.fallback }

    /// A thumbnail of `item` was decoded: its shape is what's shown.
    func note(_ item: URL, image: CGImage) {
        guard image.width > 0, image.height > 0 else { return }
        let ratio = CGFloat(image.width) / CGFloat(image.height)
        seen.insert(item)
        if let known = ratios[item], abs(known - ratio) < 0.005 { return }
        ratios[item] = ratio
    }

    /// Reads the headers of the items not known yet, outward from
    /// `current`, publishing them a batch at a time. A newer call replaces
    /// the reading.
    func read(_ items: [PhotoFile], around current: URL?) {
        reading?.cancel()
        var missing: [PhotoFile] = []
        // Nearest the current photograph: the first missing at or after it.
        var centre: Int?, passed = false
        for file in items {
            if file.url == current { passed = true }
            guard ratios[file.url] == nil else { continue }
            if passed, centre == nil { centre = missing.count }
            missing.append(file)
        }
        guard !missing.isEmpty else { reading = nil; return }
        let order = Self.outward(missing.count, from: centre ?? (current == nil ? 0 : missing.count - 1)).map { missing[$0] }
        reading = Task { [weak self] in
            var start = 0
            while start < order.count {
                let batch = Array(order[start..<min(start + Self.batch, order.count)])
                start += Self.batch
                let read = await Task.detached(priority: .utility) { () -> [(URL, CGFloat)] in
                    batch.compactMap { file in
                        guard !Task.isCancelled else { return nil }
                        return Self.header(of: file.photo.url).map { (file.url, $0) }
                    }
                }.value
                guard let self, !Task.isCancelled else { return }
                var ratios = self.ratios
                for (url, ratio) in read where !self.seen.contains(url) { ratios[url] = ratio }
                if ratios != self.ratios { self.ratios = ratios }
            }
            self?.reading = nil
        }
    }

    nonisolated static let batch = 48

    /// Indices `0..<count` nearest `centre` first, alternating after, before.
    nonisolated static func outward(_ count: Int, from centre: Int) -> [Int] {
        guard count > 0 else { return [] }
        let centre = min(max(centre, 0), count - 1)
        var order = [centre]
        order.reserveCapacity(count)
        var step = 1
        while order.count < count {
            if centre + step < count { order.append(centre + step) }
            if centre - step >= 0 { order.append(centre - step) }
            step += 1
        }
        return order
    }

    /// Width ÷ height as the photograph is shown upright: the header's
    /// pixel size, turned by its EXIF orientation. No pixels are read.
    nonisolated static func header(of url: URL) -> CGFloat? {
        let options = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(url as CFURL, options),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, options) as? [CFString: Any],
              let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.doubleValue,
              let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.doubleValue,
              width > 0, height > 0 else { return nil }
        let orientation = (properties[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1
        return CGFloat((5...8).contains(orientation) ? height / width : width / height)
    }
}
