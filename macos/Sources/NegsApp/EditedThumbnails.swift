import Accelerate
import AppKit
import CryptoKit
import ImageIO
import QuartzCore
import UniformTypeIdentifiers

/// Edited photographs' thumbnails, developed with their edits, in Caches.
/// Never needed for correctness: without one, the strip and grid show the
/// embedded preview. Each is named by everything it depends on, so a
/// changed one is never found and the oldest are removed past `limit`.
final class EditedThumbnailStore: @unchecked Sendable {
    /// The thumbnail's long side, border included.
    static let longSide: CGFloat = 512
    static let limit = 500 * 1024 * 1024
    /// The app's; under XCTest a temporary folder, never the user's Caches.
    static let shared = NSClassFromString("XCTestCase") != nil
        ? EditedThumbnailStore(directory: FileManager.default.temporaryDirectory.appendingPathComponent("negs-thumbnails-\(UUID().uuidString)", isDirectory: true))
        : EditedThumbnailStore()

    let directory: URL
    private let lock = NSLock()
    private var engine: String?
    private var writes = 0

    init(directory: URL = AppDataLocations.cachesDirectory.appendingPathComponent("Thumbnails", isDirectory: true)) {
        self.directory = directory
    }

    /// The helper and its data, which every thumbnail depends on. Until it's
    /// known no thumbnail is looked up.
    var engineStamp: String? {
        get { lock.withLock { engine } }
        set { lock.withLock { engine = newValue } }
    }

    /// Identifies the helper and every file of its data, as the rendered
    /// preview cache does. Read once a session, off the main actor.
    static func stamp(_ paths: EnginePaths) -> String? {
        func stamp(_ url: URL) throws -> String {
            let a = try FileManager.default.attributesOfItem(atPath: url.path)
            let modified = (a[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0
            return "\(url.standardizedFileURL.path)|\(a[.size] ?? "")|\(modified)"
        }
        do {
            var files = try [stamp(paths.helper)]
            guard let items = FileManager.default.enumerator(at: paths.data, includingPropertiesForKeys: [.isRegularFileKey]) else { return nil }
            for case let url as URL in items where try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true {
                files.append(try stamp(url))
            }
            return digest(Data(files.sorted().joined(separator: "\n").utf8))
        } catch { return nil }
    }

    private static func digest(_ data: Data) -> String {
        SHA256.hash(data: data).prefix(16).map { String(format: "%02x", $0) }.joined()
    }

    /// The name of `item`'s thumbnail with stored edits `look`: the file as
    /// listed (path, size, date), the duplicate, the edits and the engine.
    func key(for file: PhotoFile, look: String) -> String? {
        guard let engine = engineStamp, let modified = file.modified else { return nil }
        let photo = file.url.photoFile.standardizedFileURL.path
        return Self.digest(Data("1|\(photo)|\(file.size ?? -1)|\(modified.timeIntervalSince1970)|\(file.url.variantID ?? "")|\(look)|\(engine)".utf8))
    }

    func url(for key: String) -> URL { directory.appendingPathComponent(key + ".jpg") }

    func contains(_ key: String) -> Bool { FileManager.default.fileExists(atPath: url(for: key).path) }

    /// The thumbnail at most `maxPixels` on its long side; nil if there's none.
    func image(_ key: String, maxPixels: Int) -> CGImage? {
        let file = url(for: key)
        guard let source = CGImageSourceCreateWithURL(file as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
              let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                  kCGImageSourceCreateThumbnailFromImageAlways: true,
                  kCGImageSourceThumbnailMaxPixelSize: maxPixels,
                  kCGImageSourceShouldCacheImmediately: true,
              ] as CFDictionary) else { return nil }
        // Recently used: the last to be removed. At most once a day.
        let modified = (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        if let modified, modified.timeIntervalSinceNow < -86_400 {
            try? FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: file.path)
        }
        return image
    }

    /// Writes `image` as `key`, through a temporary file and a rename.
    func save(_ image: CGImage, as key: String) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let file = url(for: key), temporary = directory.appendingPathComponent(".\(key)-\(UUID().uuidString).jpg")
        guard let destination = CGImageDestinationCreateWithURL(temporary as CFURL, UTType.jpeg.identifier as CFString, 1, nil) else {
            throw EngineError(message: "The thumbnail couldn’t be written.")
        }
        CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary)
        guard CGImageDestinationFinalize(destination), rename(temporary.path, file.path) == 0 else {
            try? FileManager.default.removeItem(at: temporary)
            throw EngineError(message: "The thumbnail couldn’t be written.")
        }
        let count = lock.withLock { writes += 1; return writes }
        if count % 200 == 0 { prune() }
    }

    /// Removes the least recently used thumbnails while there are more than
    /// `limit` bytes, down to three quarters of it.
    func prune(limit: Int = EditedThumbnailStore.limit) {
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey]
        guard let files = try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles]) else { return }
        var entries = files.compactMap { file -> (url: URL, size: Int, date: Date)? in
            guard let values = try? file.resourceValues(forKeys: Set(keys)) else { return nil }
            return (file, values.fileSize ?? 0, values.contentModificationDate ?? .distantPast)
        }
        var total = entries.reduce(0) { $0 + $1.size }
        guard total > limit else { return }
        entries.sort { $0.date < $1.date }
        for entry in entries where total > limit * 3 / 4 {
            try? FileManager.default.removeItem(at: entry.url)
            total -= entry.size
        }
    }
}

/// Draws an edited thumbnail as the edit view shows the photograph: cropped,
/// straightened, turned and inside its border, with the canvas's own layers.
enum ThumbnailComposer {
    /// `image` is a photograph of `size` pixels, or with `cut` already its
    /// crop, straightened. The result is `longSide` on its long side, in
    /// `space`.
    @MainActor static func compose(_ image: CGImage, size: CGSize, crop: CropSettings, cut: Bool, rotation: Int, border: BorderSettings?,
                                   texture: CGImage?, strip: FilmStrip?, longSide: CGFloat = EditedThumbnailStore.longSide,
                                   space: CGColorSpace) -> CGImage? {
        let cropped = crop.size(of: size)
        guard cropped.width > 0, cropped.height > 0 else { return nil }
        let shown = rotation % 2 != 0 ? CGSize(width: cropped.height, height: cropped.width) : cropped
        let border = border?.shown(for: cropped)
        let document = border.map { border in
            let canvas = border.canvas(for: shown, strip: strip)
            return border.instagram ? border.instagramCanvas(for: canvas) : canvas
        } ?? shown
        let scale = longSide / max(document.width, document.height)
        let (width, height) = (max(1, Int((document.width * scale).rounded())), max(1, Int((document.height * scale).rounded())))
        guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                      bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue) else { return nil }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let host = CALayer(), clip = CALayer(), photo = CALayer()
        host.bounds = CGRect(origin: .zero, size: document)
        host.position = CGPoint(x: document.width / 2, y: document.height / 2)
        photo.contents = image
        photo.contentsGravity = .resize
        photo.isOpaque = true
        clip.masksToBounds = true
        clip.addSublayer(photo)
        host.addSublayer(clip)
        let layers = BorderLayers(in: host, around: clip)
        PhotographView.layout(clip: clip, photo: photo, size: size, crop: crop, rotation: rotation,
                              center: CGPoint(x: document.width / 2, y: document.height / 2), cut: cut)
        layers.layout(border, photo: shown, host: host, texture: texture, filmStrip: strip)
        context.interpolationQuality = .high
        context.scaleBy(x: CGFloat(width) / document.width, y: CGFloat(height) / document.height)
        host.render(in: context)
        return context.makeImage()
    }

    /// The preview on screen, reduced to at most `maxPixels` on its long side
    /// and tagged with its colour space.
    nonisolated static func image(of surface: IOSurface, maxPixels: Int = 1024) -> CGImage? {
        let (width, height) = (surface.width, surface.height)
        guard width > 0, height > 0, let space = EditorModel.displayColorSpace(EditorModel.displaySpace(of: surface)) else { return nil }
        let scale = min(1, Double(maxPixels) / Double(max(width, height)))
        let (w, h) = (max(1, Int((Double(width) * scale).rounded())), max(1, Int((Double(height) * scale).rounded())))
        guard let context = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                      bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
              let destination = context.data else { return nil }
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        var source = vImage_Buffer(data: surface.baseAddress, height: vImagePixelCount(height), width: vImagePixelCount(width), rowBytes: surface.bytesPerRow)
        var target = vImage_Buffer(data: destination, height: vImagePixelCount(h), width: vImagePixelCount(w), rowBytes: context.bytesPerRow)
        guard vImageScale_ARGB8888(&source, &target, nil, vImage_Flags(kvImageHighQualityResampling)) == kvImageNoError else { return nil }
        return context.makeImage()
    }
}

/// What a strip or grid cell needs to show an edited thumbnail: the item's
/// stored edits, and a count that moves when a thumbnail for them is made.
struct EditedThumbnail: Hashable, Sendable {
    let look: String
    let version: Int
    /// The engine's identity, part of every thumbnail's name.
    let stamp: String
    let store: EditedThumbnailStore

    static func == (a: Self, b: Self) -> Bool { a.look == b.look && a.version == b.version && a.stamp == b.stamp && a.store === b.store }
    func hash(into hasher: inout Hasher) { hasher.combine(look); hasher.combine(version) }
}
