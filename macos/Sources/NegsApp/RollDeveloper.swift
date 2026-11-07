import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import Observation
import os
import UniformTypeIdentifiers

/// Develops the photographs near the view behind the scenes, for the grid and for
/// the next to open, each with its own final edit, nearest the centre first, and
/// keeps them in a cache on disk so they never have to be developed twice. Only
/// those within `reach` of the centre: never a whole folder for its own sake. It
/// has a helper of its own: the
/// main one holds the open photograph's decoded source, which a run through other
/// files would throw away.
@MainActor @Observable final class RollDeveloper {
    /// The developed frames near the centre, ready to draw at once.
    private(set) var frames: [URL: CGImage] = [:]
    /// Frames finished of those within reach, for the status bar.
    private(set) var finished = 0
    private(set) var total = 0
    /// A frame is going through the film now, rather than being read from the cache.
    private(set) var isDeveloping = false
    /// How many had to be developed rather than read from the disk cache.
    @ObservationIgnored private(set) var developedCount = 0

    /// How far either side of the centre to develop: the neighbours the arrow keys
    /// reach next, or what the grid has on screen and just beyond it.
    nonisolated static let openReach = 4
    nonisolated static let gridReach = 32
    var reach = RollDeveloper.openReach {
        didSet { if reach != oldValue { wake() } }
    }
    /// Frames this far from the middle of the screen stay decoded, so scrolling never waits.
    static let keptRadius = 32
    private static let unavailable = "unavailable"
    nonisolated static let cacheLimit: Int64 = 2 << 30

    @ObservationIgnored private let client: EngineClient
    nonisolated static var defaultCache: URL { AppDataLocations.cachesDirectory.appendingPathComponent("RollFrames", isDirectory: true) }
    @ObservationIgnored private let cache: URL
    /// The helper request for a photograph's final edit, and its quarter turns.
    @ObservationIgnored private let request: (URL) -> (command: [String: JSONValue], turns: Int)?
    @ObservationIgnored private var files: [PhotoFile] = []
    @ObservationIgnored private var centre = 0
    /// What each file was last developed or looked up as.
    @ObservationIgnored private var completed: [URL: String] = [:]
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var generation = 1.0
    /// While true, nothing new starts: something more urgent wants the machine.
    @ObservationIgnored var waits: () -> Bool = { false }

    init(paths: EnginePaths, cache: URL = RollDeveloper.defaultCache,
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

    /// The folder's frames, developing out from `centre`.
    func show(files: [PhotoFile], centre: Int? = nil) {
        if let centre { self.centre = centre }
        guard files != self.files else { wake(); return }
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

    /// The next frame to look at within reach, nearest the centre. One that couldn't
    /// be developed isn't tried again until its edit or the file changes.
    private func next() -> PhotoFile? {
        for distance in 0...max(reach, 0) {
            for index in distance == 0 ? [centre] : [centre - distance, centre + distance] where files.indices.contains(index) {
                let url = files[index].url
                let done = completed[url]
                if done == nil || (done != Self.unavailable && frames[url] == nil) { return files[index] }
            }
        }
        return nil
    }

    private func updateProgress() {
        let reachable = files.indices.filter { abs($0 - centre) <= reach }
        total = reachable.count
        finished = reachable.filter { completed[files[$0].url] != nil }.count
    }

    private func wake() {
        updateProgress()
        guard worker == nil else { return }
        worker = Task { [weak self] in
            while let self, !Task.isCancelled, let file = self.next() {
                if self.waits() {
                    try? await Task.sleep(for: .milliseconds(100))
                    continue
                }
                await self.process(file)
                self.updateProgress()
            }
            self?.worker = nil
        }
    }

    private func process(_ file: PhotoFile) async {
        guard let (command, turns) = request(file.url) else { completed[file.url] = Self.unavailable; return }
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
        completed[file.url] = image == nil ? Self.unavailable : key
        if let image, nearURLs().contains(file.url) { frames[file.url] = image }
    }

    private func develop(_ file: PhotoFile, command: [String: JSONValue], turns: Int, to path: URL) async -> CGImage? {
        developedCount += 1
        generation += 1
        isDeveloping = true
        defer { isDeveloping = false }
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
        var hash = SHA256()
        hash.update(data: Data("\(file.url.path)|\(file.modified?.timeIntervalSince1970 ?? 0)|\(file.size ?? 0)|\(contentsDigest(of: helper))|".utf8))
        hash.update(data: edit)
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }

    /// The helper's contents rather than its date, which every install changes:
    /// frames only go stale when the engine itself is different. Read once per build.
    nonisolated private static func contentsDigest(of helper: URL) -> String {
        // Not resourceValues, which a URL keeps for a while once it's read them.
        let attributes = try? FileManager.default.attributesOfItem(atPath: helper.path)
        let modified = (attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0
        let stamp = "\(helper.path)|\(modified)|\(attributes?[.size] ?? 0)"
        if let known = helperDigests.withLock({ $0[stamp] }) { return known }
        guard let data = try? Data(contentsOf: helper, options: .mappedIfSafe) else { return "" }
        let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        helperDigests.withLock { $0[stamp] = digest }
        return digest
    }
    nonisolated private static let helperDigests = OSAllocatedUnfairLock(initialState: [String: String]())

    /// A frame already developed with this edit, if it's in the cache.
    nonisolated static func cachedFrame(file: PhotoFile, command: [String: JSONValue], helper: URL, cache: URL = defaultCache) -> CGImage? {
        read(cache.appendingPathComponent(key(file: file, command: command, helper: helper)).appendingPathExtension("jpg"))
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
