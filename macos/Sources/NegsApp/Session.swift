import Foundation

/// Which photograph was open and how the window showed it. Its edits live in
/// the photograph's sidecar (`Sidecar.swift`); pixels, caches and undo
/// history are not saved.
struct EditorSession: Codable, Equatable, Sendable {
    /// 3: no edits; they are in the photograph's sidecar.
    /// 2: the edits of the one photograph, which a schema 3 launch moves to
    /// its sidecar. The DIR gammas and halation baseline are `null` unless
    /// set, and a set value beats the film's preset. Schema 1 stored values
    /// for them that the engine ignored and nothing could set, so `migrate()`
    /// drops them.
    var schema = 3

    mutating func migrate() {
        guard schema == 1 else { return }
        for path in Stock.dirHalationPaths { state?.params[path] = .null }
        schema = 2
    }
    var source: URL
    /// The duplicate open, if one was; nil for the photograph itself.
    var variant: String?
    var bookmark: Data?
    /// The edits of a schema 2 session, restored as they were and then
    /// written to the photograph's sidecar. Nil from schema 3 on.
    var state: EditState?
    var inspectorVisible = true
    var fitImage = true
    /// The photograph's solved Match camera brightness, which a photograph
    /// without edits of its own keeps nowhere else: the next launch then
    /// shows its saved frame and develops it without solving it again.
    var match: SessionMatch?
    /// Not saved: the photograph's sidecar, read with the session at launch.
    var sidecar: SidecarRestore?

    private enum CodingKeys: String, CodingKey { case schema, source, variant, bookmark, state, inspectorVisible, fitImage, match }

    /// With `state`, a schema 2 session, as earlier versions saved.
    init(source: URL, state: EditState? = nil, inspectorVisible: Bool = true, fitImage: Bool = true) {
        self.source = source; self.state = state; self.inspectorVisible = inspectorVisible; self.fitImage = fitImage
        schema = state == nil ? 3 : 2
    }

    func validate() throws {
        guard (2...3).contains(schema), source.isFileURL, schema == 2 || state == nil else {
            throw EngineError(message: "The previous editing session could not be restored.")
        }
        try state?.validate()
        if let match, !match.ev.isFinite || !(-3...3).contains(match.ev) {
            throw EngineError(message: "The previous editing session could not be restored.")
        }
    }
}

/// A solved Match camera brightness, for the file it was solved on (as
/// listed: size and modification date) and the match inputs it was solved
/// for (`EditorModel.matchInputs`). Used for nothing else.
struct SessionMatch: Codable, Equatable, Sendable {
    var ev: Double
    var highlightLimited: Bool
    var inputs: JSONValue
    var size: Int
    var modified: Double

    init?(_ match: BrightnessMatch?, file: URL) {
        guard let match, let ev = match.ev, let inputs = match.inputs, let identity = Self.identity(of: file) else { return nil }
        self.ev = ev; highlightLimited = match.highlightLimited; self.inputs = inputs
        (size, modified) = identity
    }

    /// Whether this was solved on `file` as it is now.
    func solved(on file: URL) -> Bool { Self.identity(of: file).map { $0 == (size, modified) } ?? false }

    /// Read afresh: a URL's resource values are cached with it.
    private static func identity(of file: URL) -> (Int, Double)? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: file.path),
              let size = (attributes[.size] as? NSNumber)?.intValue, let modified = attributes[.modificationDate] as? Date else { return nil }
        return (size, modified.timeIntervalSince1970)
    }
}

extension EditState {
    /// Values a saved edit can't have: a crop outside the photograph, a
    /// rotation that isn't a quarter turn.
    func validate() throws {
        let values = [crop.rect.minX, crop.rect.minY, crop.rect.width, crop.rect.height,
                      crop.custom.width, crop.custom.height, crop.angle]
        guard values.allSatisfy(\.isFinite),
              crop.rect.minX >= 0, crop.rect.minY >= 0, crop.rect.width > 0, crop.rect.height > 0,
              crop.rect.maxX <= 1.000001, crop.rect.maxY <= 1.000001,
              abs(crop.angle) <= CropSettings.maxAngle, crop.custom.width > 0, crop.custom.height > 0,
              case .object = params,
              (0...3).contains(params["io.rotate_cw"].double),
              params["io.rotate_cw"].double.rounded() == params["io.rotate_cw"].double
        else { throw EngineError(message: "The previous editing session could not be restored.") }
    }
}

/// What opening a photograph needs from its sidecar.
struct SidecarRestore: Equatable, Sendable {
    /// The sidecar exists.
    var exists = false
    /// The variant shown (the first), and the defaults table its edits use.
    var variant = SidecarRestore.newVariantID()
    /// `variant` is made up: the file has none, or there's no file.
    var newVariant = true
    var defaultsVersion = SidecarDefaults.latest
    /// The variant's rating and flag.
    var rating = 0
    var flag: PhotoSidecar.Flag?
    /// Nil when it stores no edits: the photograph opens as a new one.
    var edits: SavedEdits?
    /// Why the sidecar isn't used; it is then never written over.
    var problem: String?

    static func newVariantID() -> String { String(UUID().uuidString.lowercased().prefix(8)) }

    /// Reads `photo`'s sidecar through `store`, merged over its defaults
    /// table in `data`. Small: a few kilobytes, as at launch. `variant`
    /// picks a duplicate; without it, or if it's gone, the first variant.
    static func read(photo: URL, variant id: String? = nil, store: SidecarStore, data: URL) -> SidecarRestore {
        do {
            guard let sidecar = try store.read(photo: photo) else { return SidecarRestore() }
            var restore = SidecarRestore(exists: true, defaultsVersion: sidecar.defaultsVersion)
            guard let variant = sidecar.variants.first(where: { $0.id == id }) ?? sidecar.variants.first else { return restore }
            restore.variant = variant.id; restore.newVariant = false
            restore.rating = variant.rating; restore.flag = variant.flag
            if let edits = variant.edits {
                let table = try SidecarDefaults.load(version: sidecar.defaultsVersion, data: data)
                restore.edits = try PhotoSidecar.decodeEdits(edits, table: table)
            }
            return restore
        } catch let problem as PhotoSidecar.Problem {
            return SidecarRestore(exists: true, problem: problem.message(for: photo))
        } catch {
            return SidecarRestore(exists: true, problem: error.localizedDescription)
        }
    }
}

/// Where the session finds the last photograph's sidecar at launch; nil
/// with sidecars off.
struct SidecarLookup: Sendable {
    let store: SidecarStore
    let data: URL
}

/// File I/O, JSON coding and bookmark resolution run away from the main
/// actor, except the one small read at launch (`read()`).
actor SessionStore {
    let file: URL

    init(file: URL = AppDataLocations.supportDirectory.appendingPathComponent("session.json")) {
        self.file = file
    }

    func load(sidecars: SidecarLookup? = nil) throws -> EditorSession? { try read(sidecars: sidecars) }

    /// `load()` without the actor, for launch, before the window is built: a
    /// file of a few kilobytes, the photograph's sidecar of a few more, and a
    /// bookmark resolution that never mounts a volume or shows UI.
    nonisolated func read(sidecars: SidecarLookup? = nil) throws -> EditorSession? {
        guard FileManager.default.fileExists(atPath: file.path) else { return nil }
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        guard (attributes[.size] as? Int ?? Int.max) <= 1_048_576 else {
            throw EngineError(message: "The previous editing session is too large to restore.")
        }
        var session = try JSONDecoder().decode(EditorSession.self, from: Data(contentsOf: file))
        session.migrate()
        try session.validate()
        if let bookmark = session.bookmark {
            var stale = false
            // Do not mount an offline volume or display access dialogs at launch.
            if let resolved = try? URL(resolvingBookmarkData: bookmark, options: [.withoutUI, .withoutMounting],
                                       relativeTo: nil, bookmarkDataIsStale: &stale) {
                session.source = resolved
            }
        }
        guard FileManager.default.isReadableFile(atPath: session.source.path) else {
            throw EngineError(message: "The previous photograph is unavailable. Open a photograph to continue.")
        }
        if session.state == nil, let sidecars {
            session.sidecar = SidecarRestore.read(photo: session.source, variant: session.variant, store: sidecars.store, data: sidecars.data)
        }
        return session
    }

    /// Saves the photograph and window; its edits are in its sidecar.
    func save(_ session: EditorSession) throws {
        var session = session
        session.schema = 3; session.state = nil
        try session.validate()
        session.bookmark = try? session.source.bookmarkData(options: .minimalBookmark, includingResourceValuesForKeys: nil, relativeTo: nil)
        let data = try JSONEncoder().encode(session)
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: file, options: .atomic)
    }

    /// An actor barrier: earlier session reads/writes finish before cleanup.
    func finishPendingIO() {}
}
