import CryptoKit
import Foundation

/// Solved Match camera brightness values, kept across photographs and
/// launches (ticket 39): a photograph without edits has no sidecar, so going
/// A → B → A solved A's match again. Each value is named by everything its
/// solve read: the file as it is on disk (path, size, modification date),
/// the exact request the helper solved (film, paper and every parameter,
/// which include the match inputs), and the engine (app version and helper
/// binary). A hit is what solving again
/// would give. In Caches, never beside the photographs; the least recently
/// used go past `limit`.
final class BrightnessMatchCache: @unchecked Sendable {
    struct Solved: Codable, Equatable, Sendable {
        var ev: Double
        var limited: Bool
        /// When it was last solved or used, for the LRU.
        var used: Double
    }

    static let limit = 2000
    /// The app's; under XCTest a temporary folder's, never the user's Caches.
    static let shared = NSClassFromString("XCTestCase") != nil
        ? BrightnessMatchCache(url: FileManager.default.temporaryDirectory
            .appendingPathComponent("negs-matches-\(UUID().uuidString)", isDirectory: true).appendingPathComponent("BrightnessMatches.json"))
        : BrightnessMatchCache(url: AppDataLocations.cachesDirectory.appendingPathComponent("BrightnessMatches.json"))

    let url: URL
    let limit: Int
    private let lock = NSLock()
    private var entries: [String: Solved]?
    private var dirty = false
    private var writeScheduled = false
    private let queue = DispatchQueue(label: "dev.negs.match-cache", qos: .utility)

    init(url: URL, limit: Int = BrightnessMatchCache.limit) {
        self.url = url
        self.limit = limit
    }

    /// The name of a solve of `request` on `file`, or nil if the file can't
    /// be read (nothing is then looked up or kept).
    static func key(file: URL, request: EditState, engine: String) -> String? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: file.path),
              let size = (attributes[.size] as? NSNumber)?.intValue, let modified = attributes[.modificationDate] as? Date else { return nil }
        let solved: JSONValue = .object(["film": .string(request.film), "paper": .string(request.paper), "params": request.params])
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        guard let json = try? encoder.encode(solved) else { return nil }
        var text = Data("1|\(file.standardizedFileURL.path)|\(size)|\(modified.timeIntervalSince1970)|\(engine)|".utf8)
        text.append(json)
        return SHA256.hash(data: text).map { String(format: "%02x", $0) }.joined()
    }

    /// The engine a solve ran in: the app's version and the helper binary as
    /// it is on disk, so a new build or release solves afresh. Its data
    /// ships with it.
    static func engineStamp(_ paths: EnginePaths) -> String {
        let helper = paths.helper.standardizedFileURL
        let attributes = try? FileManager.default.attributesOfItem(atPath: helper.path)
        let size = (attributes?[.size] as? NSNumber)?.intValue ?? -1
        let modified = (attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0
        return "\(EditorModel.appVersion)|\(helper.path)|\(size)|\(modified)|\(paths.data.standardizedFileURL.path)"
    }

    /// The solve kept for `key`, marked as used.
    func solved(_ key: String) -> (ev: Double, limited: Bool)? {
        lock.withLock {
            loadLocked()
            guard var entry = entries?[key] else { return nil }
            entry.used = Date().timeIntervalSince1970
            entries?[key] = entry
            return (entry.ev, entry.limited)
        }
    }

    /// Keeps a solve made on a whole decode.
    func store(_ key: String, ev: Double, limited: Bool) {
        lock.withLock {
            loadLocked()
            entries?[key] = Solved(ev: ev, limited: limited, used: Date().timeIntervalSince1970)
            if let count = entries?.count, count > limit, let oldest = entries?.sorted(by: { $0.value.used < $1.value.used }) {
                // A tenth more than needed, so it isn't sorted on every store.
                for (key, _) in oldest.prefix(count - limit + limit / 10) { entries?[key] = nil }
            }
            dirty = true
            guard !writeScheduled else { return }
            writeScheduled = true
            queue.asyncAfter(deadline: .now() + 1) { [weak self] in self?.write() }
        }
    }

    /// Reads the file now, off the main actor, so the first lookup needn't.
    func preload() { lock.withLock { loadLocked() } }

    /// Writes what's not on disk yet (at quit).
    func flush() { queue.sync { write() } }

    var count: Int { lock.withLock { loadLocked(); return entries?.count ?? 0 } }

    private func loadLocked() {
        guard entries == nil else { return }
        entries = (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode([String: Solved].self, from: $0) } ?? [:]
    }

    private func write() {
        let data: Data? = lock.withLock {
            writeScheduled = false
            guard dirty, let entries else { return nil }
            dirty = false
            return try? JSONEncoder().encode(entries)
        }
        guard let data else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }
}
