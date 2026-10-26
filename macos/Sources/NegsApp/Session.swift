import Foundation

/// Small, versioned editing state. Pixels, caches and undo history are not saved.
struct EditorSession: Codable, Equatable, Sendable {
    /// 2: the DIR gammas and halation baseline are `null` unless set, and a
    /// set value beats the film's preset. Schema 1 stored values for them
    /// that the engine ignored and nothing could set, so `migrate()` drops them.
    var schema = 2

    mutating func migrate() {
        guard schema == 1 else { return }
        for path in Stock.dirHalationPaths { state.params[path] = .null }
        schema = 2
    }
    var source: URL
    var bookmark: Data?
    var state: EditState
    var inspectorVisible = true
    var fitImage = true

    func validate() throws {
        let crop = state.crop
        let values = [crop.rect.minX, crop.rect.minY, crop.rect.width, crop.rect.height,
                      crop.custom.width, crop.custom.height, crop.angle]
        guard schema == 2, source.isFileURL, values.allSatisfy(\.isFinite),
              crop.rect.minX >= 0, crop.rect.minY >= 0, crop.rect.width > 0, crop.rect.height > 0,
              crop.rect.maxX <= 1.000001, crop.rect.maxY <= 1.000001,
              abs(crop.angle) <= CropSettings.maxAngle, crop.custom.width > 0, crop.custom.height > 0,
              case .object = state.params,
              (0...3).contains(state.params["io.rotate_cw"].double),
              state.params["io.rotate_cw"].double.rounded() == state.params["io.rotate_cw"].double
        else { throw EngineError(message: "The previous editing session could not be restored.") }
    }
}

/// All file I/O, JSON coding and bookmark resolution run away from the main actor.
actor SessionStore {
    let file: URL

    init(file: URL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("dev.negs.app/session.json")) {
        self.file = file
    }

    func load() throws -> EditorSession? {
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
        return session
    }

    func save(_ session: EditorSession) throws {
        try session.validate()
        var session = session
        session.bookmark = try? session.source.bookmarkData(options: .minimalBookmark, includingResourceValuesForKeys: nil, relativeTo: nil)
        let data = try JSONEncoder().encode(session)
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: file, options: .atomic)
    }

    /// An actor barrier: earlier session reads/writes finish before cleanup.
    func finishPendingIO() {}
}
