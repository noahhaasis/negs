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
    var bookmark: Data?
    /// The edits of a schema 2 session, restored as they were and then
    /// written to the photograph's sidecar. Nil from schema 3 on.
    var state: EditState?
    var inspectorVisible = true
    var fitImage = true
    /// Not saved: the photograph's sidecar, read with the session at launch.
    var sidecar: SidecarRestore?

    private enum CodingKeys: String, CodingKey { case schema, source, bookmark, state, inspectorVisible, fitImage }

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
    var defaultsVersion = SidecarDefaults.latest
    /// Nil when it stores no edits: the photograph opens as a new one.
    var edits: SavedEdits?
    /// Why the sidecar isn't used; it is then never written over.
    var problem: String?

    static func newVariantID() -> String { String(UUID().uuidString.lowercased().prefix(8)) }

    /// Reads `photo`'s sidecar through `store`, merged over its defaults
    /// table in `data`. Small: a few kilobytes, as at launch.
    static func read(photo: URL, store: SidecarStore, data: URL) -> SidecarRestore {
        do {
            guard let sidecar = try store.read(photo: photo) else { return SidecarRestore() }
            var restore = SidecarRestore(exists: true, defaultsVersion: sidecar.defaultsVersion)
            guard let variant = sidecar.variants.first else { return restore }
            restore.variant = variant.id
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
            session.sidecar = SidecarRestore.read(photo: session.source, store: sidecars.store, data: sidecars.data)
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
