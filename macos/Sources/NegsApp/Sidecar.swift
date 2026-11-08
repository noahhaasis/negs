import Foundation

// Each photo's rating, flag and edits live in a small JSON file beside it,
// named after the photo's full file name: `IMG_0412.CR3.negs`. The format is
// specified in docs/sidecar-format.md, which the Windows and Linux builds
// follow too.

extension JSONValue {
    /// `self` with `changes` laid over it: objects merge key by key, anything
    /// else is replaced whole. Keys `self` lacks are kept, so the engine's
    /// defaults (`restoring`) decide later whether they still mean anything.
    func overlaid(with changes: JSONValue) -> JSONValue {
        guard case .object(var base) = self, case .object(let values) = changes else { return changes }
        for (key, value) in values { base[key] = base[key].map { $0.overlaid(with: value) } ?? value }
        return .object(base)
    }

    /// What `self` changes from `base`, compared exactly so that reading the
    /// change back gives the same values bit for bit: objects recurse, any
    /// other value is kept whole when it differs. Nil when nothing does.
    func changes(from base: JSONValue) -> JSONValue? {
        if case .object(let values) = self, case .object(let defaults) = base {
            var changed: [String: JSONValue] = [:]
            for (key, value) in values {
                if let fallback = defaults[key] {
                    if let change = value.changes(from: fallback) { changed[key] = change }
                } else {
                    changed[key] = value
                }
            }
            return changed.isEmpty ? nil : .object(changed)
        }
        return self == base ? nil : self
    }

    /// The parts of `self`, a stored change, under keys that none of `known`
    /// has at that place: what a newer version stored and this one can't
    /// read, to be written back unchanged.
    func unknown(to known: [JSONValue]) -> JSONValue? {
        guard case .object(let values) = self else { return nil }
        var kept: [String: JSONValue] = [:]
        for (key, value) in values {
            let children = known.compactMap { node -> JSONValue? in
                if case .object(let object) = node { return object[key] }
                return nil
            }
            if children.isEmpty { kept[key] = value } else if let deeper = value.unknown(to: children) { kept[key] = deeper }
        }
        return kept.isEmpty ? nil : .object(kept)
    }

    var object: [String: JSONValue]? { if case .object(let v) = self { return v }; return nil }
    /// A whole number, or nil.
    var integer: Int? {
        guard case .number(let v) = self, v.rounded() == v, abs(v) < 1e15 else { return nil }
        return Int(v)
    }
}

/// The frozen defaults a sidecar's edits are stored against
/// (`data/sidecar-defaults/v<n>.json`). A diff only means something against
/// the defaults it was saved with, so a release that changes a default ships
/// a new table and never edits an old one; `SidecarDefaultsTests` hashes each.
struct SidecarDefaults: Equatable, Sendable {
    /// New sidecars get this version.
    static let latest = 1

    let version: Int
    let film: String
    let paper: String
    let params: JSONValue
    let border: JSONValue
    let crop: JSONValue

    static func url(version: Int, data: URL) -> URL {
        data.appendingPathComponent("sidecar-defaults/v\(version).json")
    }

    private final class Cache: @unchecked Sendable {
        let lock = NSLock()
        var tables: [String: SidecarDefaults] = [:]
    }
    private static let cache = Cache()

    /// Read once per data folder and version.
    static func load(version: Int, data: URL) throws -> SidecarDefaults {
        let file = url(version: version, data: data)
        cache.lock.lock()
        if let table = cache.tables[file.path] { cache.lock.unlock(); return table }
        cache.lock.unlock()
        let table = try SidecarDefaults(json: JSONDecoder().decode(JSONValue.self, from: Data(contentsOf: file)), version: version)
        cache.lock.lock(); cache.tables[file.path] = table; cache.lock.unlock()
        return table
    }

    init(json: JSONValue, version: Int) throws {
        guard json["defaultsVersion"].integer == version, case .string(let film) = json["film"], case .string(let paper) = json["paper"],
              case .object = json["params"], case .object = json["border"], case .object = json["crop"] else {
            throw EngineError(message: "The defaults table v\(version) is damaged. Reinstall Negs.")
        }
        self.version = version
        self.film = film; self.paper = paper
        params = json["params"]; border = json["border"]; crop = json["crop"]
    }
}

/// A photo's edits as saved: the parameters are the defaults table with the
/// stored changes laid over it, before the engine's own defaults fill in
/// what the table lacks (`EditorModel` does that).
struct SavedEdits: Equatable, Sendable {
    var state: EditState
    /// The stored brightness match was solved for these edits, so opening
    /// the photo doesn't solve it again.
    var matchSolved: Bool
}

/// One sidecar file, decoded. `raw` is the whole file, kept so that keys this
/// version doesn't know are written back unchanged.
struct PhotoSidecar: Equatable, Sendable {
    static let schema = 1
    static let fileExtension = "negs"
    static let maximumSize = 1_048_576

    enum Flag: String, Sendable { case pick, reject }

    struct Variant: Equatable, Sendable {
        var id: String
        /// 0 (unrated) to 5 stars.
        var rating: Int
        var flag: Flag?
        /// The stored edits, as in the file; nil when none.
        var edits: JSONValue?
    }

    var raw: JSONValue
    var defaultsVersion: Int
    var variants: [Variant]

    /// Why a sidecar isn't used. It is then never written over either.
    enum Problem: Error, Equatable, Sendable {
        case unreadable(String), newer, tooLarge

        func message(for photo: URL) -> String {
            let name = photo.lastPathComponent
            switch self {
            case .unreadable(let reason): return "The edits saved for \(name) couldn’t be read (\(reason)), so it opens with the defaults. Its .negs file is left as it is."
            case .newer: return "\(name) was edited in a newer version of Negs, so it opens with the defaults. Update Negs to see its edits; its .negs file is left as it is."
            case .tooLarge: return "The .negs file of \(name) is too large to be one of Negs’s, so it opens with the defaults and the file is left as it is."
            }
        }
    }

    /// `IMG_0412.CR3` → `IMG_0412.CR3.negs`.
    static func url(for photo: URL) -> URL { photo.appendingPathExtension(fileExtension) }
    /// The hidden file a write goes through, which folder scans skip.
    static func temporaryURL(for photo: URL) -> URL {
        photo.deletingLastPathComponent().appendingPathComponent(".\(url(for: photo).lastPathComponent).tmp")
    }

    init(raw: JSONValue) throws {
        guard case .object(let top) = raw else { throw Problem.unreadable("not a JSON object") }
        guard let schema = top["negs"]?.integer else { throw Problem.unreadable("no schema") }
        guard schema <= Self.schema else { throw Problem.newer }
        guard schema >= 1 else { throw Problem.unreadable("schema \(schema)") }
        guard let version = top["defaultsVersion"]?.integer, version >= 1 else { throw Problem.unreadable("no defaults version") }
        guard version <= SidecarDefaults.latest else { throw Problem.newer }
        var variants: [Variant] = []
        for entry in top["variants"]?.array ?? [] {
            guard case .object(let fields) = entry, case .string(let id)? = fields["id"], !id.isEmpty,
                  !variants.contains(where: { $0.id == id }) else { throw Problem.unreadable("a variant without its own id") }
            let rating = fields["rating"]?.integer ?? 0
            guard (0...5).contains(rating) else { throw Problem.unreadable("rating \(rating)") }
            var flag: Flag?
            if case .string(let name)? = fields["flag"] {
                guard let known = Flag(rawValue: name) else { throw Problem.unreadable("flag “\(name)”") }
                flag = known
            }
            let edits = fields["edits"]
            if let edits, edits.object == nil { throw Problem.unreadable("edits that aren’t an object") }
            variants.append(Variant(id: id, rating: rating, flag: flag, edits: edits))
        }
        self.raw = raw
        defaultsVersion = version
        self.variants = variants
    }

    init(data: Data) throws {
        guard data.count <= Self.maximumSize else { throw Problem.tooLarge }
        guard let raw = try? JSONDecoder().decode(JSONValue.self, from: data) else { throw Problem.unreadable("not JSON") }
        try self.init(raw: raw)
    }

    /// The file's bytes: sorted keys, so a file written twice with the same
    /// content is the same, which keeps sync services quiet.
    static func encode(_ raw: JSONValue) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        var data = try encoder.encode(raw)
        data.append(10)
        return data
    }

    // MARK: Edits

    /// The edits keys this version reads and writes; others are kept as found.
    static let editKeys = ["film", "paper", "params", "crop", "border", "brightnessMatch"]

    /// The stored form of `state`: the film and paper always, and only what
    /// differs from `table` otherwise. The brightness match keeps its solved
    /// exposure, not what it was solved for: that is the edits themselves,
    /// so reading them back rebuilds it.
    static func encodeEdits(_ state: EditState, matchSolved: Bool, table: SidecarDefaults) -> JSONValue {
        var edits: [String: JSONValue] = ["film": .string(state.film), "paper": .string(state.paper)]
        if let params = state.params.changes(from: table.params) { edits["params"] = params }
        if let crop = json(state.crop)?.changes(from: table.crop) { edits["crop"] = crop }
        if let border = json(state.border)?.changes(from: table.border) { edits["border"] = border }
        if let match = state.brightnessMatch {
            var stored: [String: JSONValue] = ["highlightLimited": .bool(match.highlightLimited)]
            if let ev = match.ev {
                stored["ev"] = .number(ev)
                if !matchSolved { stored["solved"] = .bool(false) }
            }
            edits["brightnessMatch"] = .object(stored)
        }
        return .object(edits)
    }

    static func decodeEdits(_ edits: JSONValue, table: SidecarDefaults) throws -> SavedEdits {
        guard case .object(let fields) = edits else { throw Problem.unreadable("edits that aren’t an object") }
        var state = EditState()
        func string(_ key: String, _ fallback: String) throws -> String {
            guard let value = fields[key] else { return fallback }
            guard case .string(let text) = value, !text.isEmpty else { throw Problem.unreadable("\(key) isn’t a name") }
            return text
        }
        state.film = try string("film", table.film)
        state.paper = try string("paper", table.paper)
        state.params = table.params.overlaid(with: fields["params"] ?? .object([:]))
        guard let crop = decode(CropSettings.self, table.crop.overlaid(with: fields["crop"] ?? .object([:]))) else { throw Problem.unreadable("crop") }
        guard let border = decode(BorderSettings.self, table.border.overlaid(with: fields["border"] ?? .object([:]))) else { throw Problem.unreadable("border") }
        state.crop = crop
        state.border = border
        var solved = false
        if let stored = fields["brightnessMatch"] {
            guard case .object(let match) = stored else { throw Problem.unreadable("brightness match") }
            var ev: Double?
            if let value = match["ev"] {
                guard case .number(let number) = value, number.isFinite, (-3...3).contains(number) else { throw Problem.unreadable("brightness match") }
                ev = number
            }
            state.brightnessMatch = BrightnessMatch(ev: ev, inputs: nil, highlightLimited: match["highlightLimited"]?.bool ?? false)
            solved = ev != nil && match["solved"] != .bool(false)
        }
        do { try state.validate() } catch { throw Problem.unreadable("edits out of range") }
        return SavedEdits(state: state, matchSolved: solved)
    }

    private static func json<T: Encodable>(_ value: T) -> JSONValue? {
        (try? JSONEncoder().encode(value)).flatMap { try? JSONDecoder().decode(JSONValue.self, from: $0) }
    }
    private static func decode<T: Decodable>(_ type: T.Type, _ value: JSONValue) -> T? {
        (try? JSONEncoder().encode(value)).flatMap { try? JSONDecoder().decode(type, from: $0) }
    }
}

/// A change to one variant of a sidecar, applied to the file as it is on
/// disk when written, so a rating set by another app or Mac meanwhile stays.
/// Only the fields it sets change.
struct SidecarPatch: Equatable, Sendable {
    var variant: String
    /// The file's first variant, whatever its id, and a new one with
    /// `variant`'s id only if it has none: for a photograph that had no
    /// sidecar when it opened, whose file another app may have made since.
    var primary = false
    /// The variant's new stored edits (`.some(nil)` removes them); nil leaves them.
    var edits: JSONValue?? = nil
    /// The new rating, 0 to 5; nil leaves it.
    var rating: Int? = nil
    /// The new flag (`.some(nil)` unflags); nil leaves it.
    var flag: PhotoSidecar.Flag?? = nil
    /// What the writer's own version knows of each stored group (`params`,
    /// `crop`, `border`): the defaults table, and for the parameters also the
    /// engine's defaults. Stored values outside them are a newer version's
    /// and stay.
    var known: [String: [JSONValue]] = [:]
    /// The table `edits` is stored against; a new file gets it.
    var defaultsVersion: Int
    var app: String

    /// Whether the change gives a variant something to hold.
    private var adds: Bool { (edits ?? nil) != nil || (rating ?? 0) > 0 || (flag ?? nil) != nil }

    /// The file after this change, or nil to remove it: a single variant with
    /// nothing in it and nothing else in the file means the photo has no
    /// rating, flag or edits left.
    func applied(to raw: JSONValue?) throws -> JSONValue? {
        var top = raw?.object ?? ["negs": .number(Double(PhotoSidecar.schema)), "defaultsVersion": .number(Double(defaultsVersion))]
        if raw != nil {
            // Decoding checks the schema and versions: a newer file is never written over.
            let sidecar = try PhotoSidecar(raw: .object(top))
            // Ratings and flags don't depend on the table; edits are stored against it.
            if edits != nil, sidecar.defaultsVersion != defaultsVersion {
                throw PhotoSidecar.Problem.unreadable("its defaults version changed while it was open")
            }
        }
        var variants = top["variants"]?.array ?? []
        var index = primary && !variants.isEmpty ? 0 : variants.firstIndex { $0["id"] == .string(variant) }
        if index == nil {
            guard adds else { return raw }
            variants.append(.object(["id": .string(variant)]))
            index = variants.count - 1
        }
        top["app"] = .string(app)
        var fields = variants[index!].object ?? [:]
        if let edits {
            let previous = fields["edits"]?.object ?? [:]
            // Keep what this version doesn't know, then lay what it does over it.
            var stored = previous.filter { !PhotoSidecar.editKeys.contains($0.key) }
            for (group, known) in known {
                if let unknown = previous[group]?.unknown(to: known) { stored[group] = unknown }
            }
            for (key, value) in edits?.object ?? [:] {
                stored[key] = stored[key].map { $0.overlaid(with: value) } ?? value
            }
            fields["edits"] = stored.isEmpty ? nil : .object(stored)
        }
        if let rating { fields["rating"] = rating > 0 ? .number(Double(min(rating, 5))) : nil }
        if let flag { fields["flag"] = flag.map { .string($0.rawValue) } }
        variants[index!] = .object(fields)
        top["variants"] = .array(variants)
        let plain = variants.count == 1 && Set(fields.keys) == ["id"] && Set(top.keys).isSubset(of: ["negs", "app", "defaultsVersion", "variants"])
        return plain ? nil : .object(top)
    }
}

/// Reads and writes photos' sidecars. Writes run one at a time, away from
/// the main actor; a read sees the changes still waiting to be written.
final class SidecarStore: Sendable {
    /// Off for benchmarks, which must not leave files beside their photo.
    let writes: Bool
    private let queue = Queue()

    init(writes: Bool = true) { self.writes = writes }

    /// The app's store; none under XCTest, whose models must not write
    /// beside the user's photos (tests that check sidecars make their own).
    static let forApp: SidecarStore? = NSClassFromString("XCTestCase") != nil ? nil
        : SidecarStore(writes: ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] == nil)

    private final class Queue: @unchecked Sendable {
        private let lock = NSLock()
        private var pending: [URL: [SidecarPatch]] = [:]
        private var running: Task<Void, Never>?

        func patches(for photo: URL) -> [SidecarPatch] { lock.withLock { pending[photo] ?? [] } }
        func add(_ patch: SidecarPatch, for photo: URL) { lock.withLock { pending[photo, default: []].append(patch) } }
        var all: [URL: [SidecarPatch]] { lock.withLock { pending } }
        /// Drops the first `count` patches of `photo`, which have been written.
        func written(_ count: Int, for photo: URL) {
            lock.withLock {
                guard let waiting = pending[photo] else { return }
                pending[photo] = waiting.count > count ? Array(waiting.dropFirst(count)) : nil
            }
        }
        /// Runs `work` after every earlier flush.
        func chain(_ work: @escaping @Sendable () async -> [URL: String]) -> Task<[URL: String], Never> {
            lock.withLock {
                let previous = running
                let task = Task.detached(priority: .utility) { () -> [URL: String] in
                    await previous?.value
                    return await work()
                }
                running = Task { _ = await task.value }
                return task
            }
        }
    }

    /// The sidecar as it will be once queued changes are written. Nil when
    /// the photo has none. Small enough to read on the main actor.
    func read(photo: URL) throws -> PhotoSidecar? {
        let patches = queue.patches(for: photo.standardizedFileURL)
        var raw = try Self.readRaw(photo: photo)
        for patch in patches { raw = try patch.applied(to: raw) }
        guard let raw else { return nil }
        return try PhotoSidecar(raw: raw)
    }

    /// The file's JSON, or nil if there's none.
    static func readRaw(photo: URL) throws -> JSONValue? {
        let file = PhotoSidecar.url(for: photo)
        let data: Data
        do {
            let handle = try FileHandle(forReadingFrom: file)
            defer { try? handle.close() }
            data = try handle.read(upToCount: PhotoSidecar.maximumSize + 1) ?? Data()
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile || error.code == .fileNoSuchFile {
            return nil
        } catch let error as NSError where error.domain == NSPOSIXErrorDomain && error.code == Int(ENOENT) {
            return nil
        } catch {
            throw PhotoSidecar.Problem.unreadable(error.localizedDescription)
        }
        return try PhotoSidecar(data: data).raw
    }

    /// Queues `patch`; `flush` writes it.
    func enqueue(_ patch: SidecarPatch, for photo: URL) {
        queue.add(patch, for: photo.standardizedFileURL)
    }

    /// Writes every queued change, oldest first, and returns the photos whose
    /// sidecar couldn't be written, with the reason. Their changes are dropped.
    @discardableResult func flush() async -> [URL: String] {
        await queue.chain { [queue] in
            var failures: [URL: String] = [:]
            for (photo, patches) in queue.all {
                do { try Self.write(patches, photo: photo) } catch { failures[photo] = error.localizedDescription }
                queue.written(patches.count, for: photo)
            }
            return failures
        }.value
    }

    /// Applies `patches` to the file on disk and writes it through a hidden
    /// temporary file and a rename, or removes it once nothing is left.
    private static func write(_ patches: [SidecarPatch], photo: URL) throws {
        var raw: JSONValue?
        do {
            raw = try readRaw(photo: photo)
            for patch in patches { raw = try patch.applied(to: raw) }
        } catch let problem as PhotoSidecar.Problem {
            throw EngineError(message: problem.message(for: photo))
        }
        let file = PhotoSidecar.url(for: photo)
        guard let raw else {
            do { try FileManager.default.removeItem(at: file) } catch CocoaError.fileNoSuchFile {}
            return
        }
        let temporary = PhotoSidecar.temporaryURL(for: photo)
        do {
            try PhotoSidecar.encode(raw).write(to: temporary)
            guard rename(temporary.path, file.path) == 0 else {
                throw EngineError(message: String(cString: strerror(errno)))
            }
        } catch {
            try? FileManager.default.removeItem(at: temporary)
            throw error
        }
    }
}
