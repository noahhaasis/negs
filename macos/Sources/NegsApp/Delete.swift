import Foundation

/// Where deleted photographs go: the Trash, keeping where each file landed
/// so Undo can put it back. Tests use a folder of their own.
struct PhotoTrash: Sendable {
    /// Whether `folder`'s volume has a Trash. Without one, a delete is permanent.
    var available: @Sendable (_ folder: URL) -> Bool
    /// Moves `file` to the Trash and returns where it went.
    var trash: @Sendable (_ file: URL) throws -> URL
    /// Deletes `file` at once: on a volume without a Trash.
    var remove: @Sendable (_ file: URL) throws -> Void

    static let system = PhotoTrash(
        available: { folder in
            (try? FileManager.default.url(for: .trashDirectory, in: .userDomainMask, appropriateFor: folder, create: true)) != nil
        },
        trash: { file in
            var landed: NSURL?
            try FileManager.default.trashItem(at: file, resultingItemURL: &landed)
            return landed as URL? ?? file
        },
        remove: { try FileManager.default.removeItem(at: $0) })
}

/// What a delete does. Photographs go to the Trash with their sidecars, and
/// so with all their duplicates. A duplicate of a photograph that stays is
/// removed from the sidecar, and the file stays.
struct DeletePlan: Equatable, Sendable {
    /// Everything chosen, in order: what Undo selects again.
    private(set) var items: [URL] = []
    private(set) var files: [URL] = []
    private(set) var duplicates: [URL] = []
    /// Duplicates that go with their photographs without being chosen.
    private(set) var following = 0

    var isEmpty: Bool { items.isEmpty }

    /// `duplicates` gives a photograph's duplicates by variant id.
    init(_ chosen: [URL], duplicates ids: (URL) -> [String] = { _ in [] }) {
        var seen = Set<URL>()
        let chosen = chosen.filter { seen.insert($0).inserted }
        files = chosen.filter { $0.variantID == nil }
        let trashed = Set(files)
        duplicates = chosen.filter { $0.variantID != nil && !trashed.contains($0.photoFile) }
        items = chosen.filter { $0.variantID == nil || !trashed.contains($0.photoFile) }
        following = files.reduce(0) { count, file in count + ids(file).filter { !seen.contains(file.variant($0)) }.count }
    }

    /// The same plan without its files: after a permanent delete, only the
    /// duplicates can come back and go again.
    var withoutFiles: DeletePlan {
        var plan = self
        plan.files = []; plan.following = 0; plan.items = duplicates
        return plan
    }

    /// The Undo menu's name for it.
    var actionName: String {
        guard files.isEmpty else { return "Move to Trash" }
        return duplicates.count > 1 ? "Remove Duplicates" : "Remove Duplicate"
    }

    struct Question: Equatable {
        var message: String
        var information: String
        var button: String
    }

    /// The alert before it. A single photograph is named rather than counted.
    func question(name: (URL) -> String, permanent: Bool) -> Question {
        let photos = files.count, copies = duplicates.count
        func count(_ n: Int, _ noun: String) -> String { n == 1 ? "1 \(noun)" : "\(n) \(noun)s" }
        let with = following == 0 ? "" : photos == 1
            ? (following == 1 ? ", with its duplicate" : ", with its \(following) duplicates")
            : ", with \(count(following, "duplicate"))"
        let filesGo = photos == 1 ? "the photograph’s file and its Negs rating and edits"
            : "the photograph files and their Negs ratings and edits"
        let copiesGo = copies == 1 ? "The duplicate’s rating and edits are removed from its .negs file; the photograph stays."
            : "The duplicates’ ratings and edits are removed from their .negs files; the photographs stay."
        if photos == 0 {
            return Question(message: copies == 1 ? "Remove the duplicate “\(name(duplicates[0]))”?" : "Remove \(copies) duplicates?",
                            information: copiesGo + " You can put \(copies == 1 ? "it" : "them") back with Undo.", button: "Remove")
        }
        let what = photos == 1 && copies == 0 ? "“\(name(files[0]))”" : count(photos, "photograph")
        let andCopies = copies == 0 ? "" : " and remove \(count(copies, "duplicate"))"
        if permanent {
            let back = copies == 0 ? "" : " The duplicates can be put back with Undo."
            return Question(message: "Delete \(what) permanently\(andCopies)?",
                            information: "This volume has no Trash, so \(filesGo) are deleted at once\(with). This can’t be undone."
                                + (copies == 0 ? "" : " " + copiesGo) + back,
                            button: "Delete")
        }
        let back = photos == 1 && copies == 0 ? "You can put it back with Undo or from the Trash." : "You can put them back with Undo or from the Trash."
        return Question(message: "Move \(what) to the Trash\(andCopies)?",
                        information: "\(filesGo.prefix(1).uppercased() + filesGo.dropFirst()) are moved to the Trash\(with). " + (copies == 0 ? "" : copiesGo + " ") + back,
                        button: copies == 0 ? "Move to Trash" : "Delete")
    }
}

/// What a delete moved, filled in once the files have gone; Undo waits for it.
@MainActor final class TrashRecord {
    var moves: [(original: URL, trashed: URL)] = []
    var task: Task<Void, Never>?
}

extension PhotoSidecar {
    /// `raw` without the variant `id`; nil when nothing is left in it.
    static func removing(variant id: String, from raw: JSONValue) throws -> JSONValue? {
        // Decoding checks the schema: a newer file is never written over.
        _ = try PhotoSidecar(raw: raw)
        guard var top = raw.object, var variants = top["variants"]?.array else { return raw }
        variants.removeAll { $0["id"] == .string(id) }
        top["variants"] = .array(variants)
        let plain = variants.isEmpty || variants.count == 1 && Set(variants[0].object.map { Array($0.keys) } ?? []) == ["id"]
        return plain && Set(top.keys).isSubset(of: ["negs", "app", "defaultsVersion", "variants"]) ? nil : .object(top)
    }
}
