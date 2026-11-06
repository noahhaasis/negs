import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import Observation
import UniformTypeIdentifiers

/// Develops a folder's photographs in the background for the big film strip, each
/// with its own final edit, nearest the centre first, and keeps them in a cache on
/// disk so they never have to be developed twice. It has a helper of its own: the
/// main one holds the open photograph's decoded source, which a run through other
/// files would throw away.
@MainActor @Observable final class RollDeveloper {
    /// The developed frames near the centre, ready to draw at once.
    private(set) var frames: [URL: CGImage] = [:]
    /// Frames finished of the folder's, for the status bar.
    private(set) var finished = 0
    private(set) var total = 0
    /// How many had to be developed rather than read from the disk cache.
    @ObservationIgnored private(set) var developedCount = 0

    /// Frames this far from the centre stay decoded, so flicking never waits.
    static let keptRadius = 12
    nonisolated static let cacheLimit: Int64 = 2 << 30

    @ObservationIgnored private let client: EngineClient
    @ObservationIgnored private let cache: URL
    /// The helper request for a photograph's final edit, and its quarter turns.
    @ObservationIgnored private let request: (URL) -> (command: [String: JSONValue], turns: Int)?
    @ObservationIgnored private var files: [PhotoFile] = []
    @ObservationIgnored private var centre = 0
    /// What each file was last developed or looked up as.
    @ObservationIgnored private var completed: [URL: String] = [:]
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var generation = 1.0

    init(paths: EnginePaths, cache: URL = AppDataLocations.cachesDirectory.appendingPathComponent("RollFrames", isDirectory: true),
         request: @escaping (URL) -> (command: [String: JSONValue], turns: Int)?) throws {
        client = try EngineClient(paths: paths)
        self.cache = cache
        self.request = request
        try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
        Task.detached(priority: .background) { Self.prune(cache, limit: Self.cacheLimit) }
    }

    func stop() {
        worker?.cancel()
        worker = nil
        client.stop()
    }

    func show(files: [PhotoFile]) {
        guard files != self.files else { return }
        self.files = files
        let present = Set(files.map(\.url))
        completed = completed.filter { present.contains($0.key) }
        frames = frames.filter { present.contains($0.key) }
        wake()
    }

    func setCentre(_ index: Int) {
        guard index != centre else { return }
        centre = index
        let near = nearURLs()
        for url in frames.keys where !near.contains(url) { frames[url] = nil }
        wake()
    }

    /// A photograph's edit changed: look at it again. Its old frame shows until then.
    func invalidate(_ url: URL) {
        completed[url] = nil
        wake()
    }

    /// The look of the whole folder changed.
    func invalidateAll() {
        completed = [:]
        wake()
    }

    private func nearURLs() -> Set<URL> {
        guard !files.isEmpty else { return [] }
        let low = max(0, centre - Self.keptRadius), high = min(files.count - 1, centre + Self.keptRadius)
        guard low <= high else { return [] }
        return Set(files[low...high].map(\.url))
    }

    /// The next frame to look at, nearest the centre.
    private func next() -> PhotoFile? {
        let near = nearURLs()
        return files.indices
            .sorted { abs($0 - centre) < abs($1 - centre) }
            .lazy.map { self.files[$0] }
            .first { self.completed[$0.url] == nil || (near.contains($0.url) && self.frames[$0.url] == nil) }
    }

    private func wake() {
        total = files.count
        finished = completed.count
        guard worker == nil else { return }
        worker = Task { [weak self] in
            while let self, !Task.isCancelled, let file = self.next() {
                await self.process(file)
                self.finished = self.completed.count
            }
            self?.worker = nil
        }
    }

    private func process(_ file: PhotoFile) async {
        guard let (command, turns) = request(file.url) else { completed[file.url] = "unavailable"; return }
        let key = Self.key(file: file, command: command, helper: client.paths.helper)
        let path = cache.appendingPathComponent(key).appendingPathExtension("jpg")
        var image = await Task.detached(priority: .utility) { Self.read(path) }.value
        if image == nil {
            image = await develop(file, command: command, turns: turns, to: path)
        } else {
            // Recently used, so pruning takes it last.
            try? FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: path.path)
        }
        guard !Task.isCancelled else { return }
        completed[file.url] = key
        if let image, nearURLs().contains(file.url) { frames[file.url] = image }
    }

    private func develop(_ file: PhotoFile, command: [String: JSONValue], turns: Int, to path: URL) async -> CGImage? {
        developedCount += 1
        generation += 1
        let buffer = client.directory.appendingPathComponent("roll-\(UUID().uuidString).rgba")
        var command = command
        command["preview_path"] = .string(buffer.path)
        command["source_generation"] = .number(generation)
        do {
            let (_, result) = try await client.request(command)
            let (width, height) = (Int(result["width"].double), Int(result["height"].double))
            let space = result["display_color_space"].string
            return await Task.detached(priority: .utility) { () -> CGImage? in
                defer { try? FileManager.default.removeItem(at: buffer) }
                guard let developed = try? EditorModel.thumbnailImage(Data(contentsOf: buffer), width: width, height: height, displaySpace: space),
                      let turned = Self.turned(developed, quarterTurns: turns) else { return nil }
                Self.write(turned, to: path)
                return turned
            }.value
        } catch {
            try? FileManager.default.removeItem(at: buffer)
            return nil
        }
    }

    /// Everything the finished frame depends on: the file as it is now, its edit
    /// and the helper that developed it.
    nonisolated static func key(file: PhotoFile, command: [String: JSONValue], helper: URL) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        let edit = (try? encoder.encode(JSONValue.object(command))) ?? Data()
        let built = (try? helper.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)?.timeIntervalSince1970 ?? 0
        var hash = SHA256()
        hash.update(data: Data("\(file.url.path)|\(file.modified?.timeIntervalSince1970 ?? 0)|\(file.size ?? 0)|\(built)|".utf8))
        hash.update(data: edit)
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }

    nonisolated static func turned(_ image: CGImage, quarterTurns: Int) -> CGImage? {
        let turns = ((quarterTurns % 4) + 4) % 4
        guard turns != 0 else { return image }
        let swap = turns % 2 == 1
        let (width, height) = swap ? (image.height, image.width) : (image.width, image.height)
        guard let space = image.colorSpace,
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                      bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        context.translateBy(x: CGFloat(width) / 2, y: CGFloat(height) / 2)
        context.rotate(by: -CGFloat(turns) * .pi / 2)
        context.draw(image, in: CGRect(x: -CGFloat(image.width) / 2, y: -CGFloat(image.height) / 2,
                                       width: CGFloat(image.width), height: CGFloat(image.height)))
        return context.makeImage()
    }

    nonisolated private static func read(_ path: URL) -> CGImage? {
        guard let source = CGImageSourceCreateWithURL(path as CFURL, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
    }

    /// Atomically, so a frame on disk is always a whole one.
    nonisolated private static func write(_ image: CGImage, to path: URL) {
        let staging = path.deletingLastPathComponent().appendingPathComponent(".\(UUID().uuidString).jpg")
        guard let destination = CGImageDestinationCreateWithURL(staging as CFURL, UTType.jpeg.identifier as CFString, 1, nil) else { return }
        CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.92] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { try? FileManager.default.removeItem(at: staging); return }
        if rename(staging.path, path.path) != 0 { try? FileManager.default.removeItem(at: staging) }
    }

    /// Removes the least recently used frames once the cache passes `limit`.
    nonisolated static func prune(_ cache: URL, limit: Int64) {
        let keys: Set<URLResourceKey> = [.fileSizeKey, .contentModificationDateKey]
        guard let files = try? FileManager.default.contentsOfDirectory(at: cache, includingPropertiesForKeys: Array(keys)) else { return }
        let entries = files.compactMap { url -> (url: URL, size: Int64, date: Date)? in
            guard let values = try? url.resourceValues(forKeys: keys), let size = values.fileSize else { return nil }
            return (url, Int64(size), values.contentModificationDate ?? .distantPast)
        }.sorted { $0.date > $1.date }
        var used: Int64 = 0
        for entry in entries {
            used += entry.size
            if used > limit { try? FileManager.default.removeItem(at: entry.url) }
        }
    }
}
