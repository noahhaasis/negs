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

/// What a delete does (`delete.rs`). A photograph goes to the Trash with
/// its sidecar, and a RAW with its camera JPEG, only with all its
/// duplicates. Otherwise what is chosen leaves the sidecar and the file
/// stays: a duplicate, or the photograph itself, whose first duplicate left
/// then takes its place.
struct DeletePlan: Equatable, Sendable {
    /// Everything chosen, in order: what Undo selects again.
    private(set) var items: [URL] = []
    private(set) var files: [URL] = []
    /// What leaves the sidecar of a photograph that stays: duplicates, and
    /// photographs whose duplicates don't all go.
    private(set) var duplicates: [URL] = []
    /// The duplicates that go with `files`.
    private(set) var following = 0
    /// The camera JPEGs that go with `files`, by file.
    private(set) var companions: [URL: URL] = [:]
    /// Each duplicate that becomes its photograph, its photograph removed.
    private(set) var renamed: [URL: URL] = [:]

    var isEmpty: Bool { items.isEmpty }

    /// `duplicates` gives a photograph's duplicates by variant id.
    init(_ chosen: [URL], duplicates ids: (URL) -> [String] = { _ in [] }, companion: (URL) -> URL? = { _ in nil }) {
        var seen = Set<URL>()
        let chosen = chosen.filter { seen.insert($0).inserted }
        // A photograph's file goes only with every one of its duplicates.
        files = chosen.filter { photo in photo.variantID == nil && ids(photo).allSatisfy { seen.contains(photo.variant($0)) } }
        let trashed = Set(files)
        duplicates = chosen.filter { !trashed.contains($0.photoFile) }
        items = chosen.filter { $0.variantID == nil || !trashed.contains($0.photoFile) }
        following = files.reduce(0) { $0 + ids($1).count }
        for file in files { if let picture = companion(file) { companions[file] = picture } }
        for photo in duplicates where photo.variantID == nil {
            if let first = ids(photo).first(where: { !seen.contains(photo.variant($0)) }) { renamed[photo.variant(first)] = photo }
        }
    }

    /// What `item` is called once the plan is done: a duplicate that takes
    /// its photograph's place becomes the photograph.
    func after(_ item: URL) -> URL { renamed[item] ?? item }

    /// The photographs among `duplicates`, kept for their duplicates.
    private var keptPhotographs: Int { duplicates.filter { $0.variantID == nil }.count }

    /// The same plan without its files: after a permanent delete, only the
    /// duplicates can come back and go again.
    var withoutFiles: DeletePlan {
        var plan = self
        plan.files = []; plan.following = 0; plan.items = duplicates; plan.companions = [:]
        return plan
    }

    /// The Undo menu's name for it.
    var actionName: String {
        guard files.isEmpty else { return "Move to Trash" }
        if keptPhotographs > 0 { return duplicates.count > 1 ? "Remove Photographs" : "Remove Photograph" }
        return duplicates.count > 1 ? "Remove Duplicates" : "Remove Duplicate"
    }

    struct Question: Equatable {
        var message: String
        var information: String
        var button: String
    }

    /// The alert before it. A single photograph is named rather than counted.
    func question(name: (URL) -> String, permanent: Bool) -> Question {
        let photos = files.count, copies = duplicates.count, kept = keptPhotographs
        func count(_ n: Int, _ noun: String) -> String { n == 1 ? "1 \(noun)" : "\(n) \(noun)s" }
        let with = following == 0 ? "" : photos == 1
            ? (following == 1 ? ", with its duplicate" : ", with its \(following) duplicates")
            : ", with \(count(following, "duplicate"))"
        let filesGo = companions.isEmpty
            ? (photos == 1 ? "the photograph’s file and its Negs rating and edits" : "the photograph files and their Negs ratings and edits")
            : (photos == 1 ? "the photograph’s file, its camera JPEG and its Negs rating and edits"
                : "the photograph files, their camera JPEGs and their Negs ratings and edits")
        let copiesGo: String
        switch (kept, copies) {
        case (0, 1): copiesGo = "The duplicate’s rating and edits are removed from its .negs file; the photograph stays."
        case (0, _): copiesGo = "The duplicates’ ratings and edits are removed from their .negs files; the photographs stay."
        case (_, 1):
            copiesGo = renamed.keys.first.map { "Its rating and edits are removed from its .negs file; the file stays, and its duplicate “\(name($0))” takes its place." }
                ?? "Its rating and edits are removed from its .negs file; the file stays for its duplicates."
        default: copiesGo = "Their ratings and edits are removed from their .negs files; the files stay, and the first duplicate left takes each photograph’s place."
        }
        // What leaves the sidecars, counted: "1 photograph and 2 duplicates".
        let removals = kept == 0 ? count(copies, "duplicate")
            : kept == copies ? count(kept, "photograph") : "\(count(kept, "photograph")) and \(count(copies - kept, "duplicate"))"
        if photos == 0 {
            let message = copies == 1
                ? (kept == 0 ? "Remove the duplicate “\(name(duplicates[0]))”?" : "Remove “\(name(duplicates[0]))”?")
                : (kept == 0 ? "Remove \(copies) duplicates?" : "Remove \(removals)?")
            return Question(message: message, information: copiesGo + " You can put \(copies == 1 ? "it" : "them") back with Undo.", button: "Remove")
        }
        let what = photos == 1 && copies == 0 ? "“\(name(files[0]))”" : count(photos, "photograph")
        let andCopies = copies == 0 ? "" : " and remove \(removals)"
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
