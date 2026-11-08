import Foundation
import Observation

// The opened photograph's folder is the library: its photographs, and the
// ratings and flags in their sidecars (`Sidecar.swift`). There is no catalog.

/// A duplicate (a sidecar's second or later variant) is a library item of its
/// own, addressed as its photograph's URL with the variant in the fragment:
/// `IMG_0412.CR3#variant=a1b2c3d4`. The first variant is the plain URL.
/// File access always goes through `photoFile`.
extension URL {
    private static let variantPrefix = "variant="

    func variant(_ id: String) -> URL {
        var components = URLComponents(url: photoFile, resolvingAgainstBaseURL: false)
        components?.fragment = Self.variantPrefix + id
        return components?.url ?? self
    }

    /// The duplicate's variant id; nil for a photograph itself.
    var variantID: String? {
        guard let fragment, fragment.hasPrefix(Self.variantPrefix) else { return nil }
        return String(fragment.dropFirst(Self.variantPrefix.count))
    }

    /// The photograph's own file.
    var photoFile: URL {
        guard fragment != nil, var components = URLComponents(url: self, resolvingAgainstBaseURL: false) else { return self }
        components.fragment = nil
        return components.url ?? self
    }
}

/// What the film strip shows of a photograph or duplicate: its variant's
/// rating and flag, and whether it has edits.
struct LibrarySummary: Equatable, Sendable {
    var rating = 0
    var flag: PhotoSidecar.Flag?
    var edited = false
    /// Why its sidecar isn't used (unreadable, from a newer Negs).
    var problem: String?
}

extension PhotoSidecar {
    var summary: LibrarySummary { variants.first.map(Self.summary(of:)) ?? LibrarySummary() }

    static func summary(of variant: Variant) -> LibrarySummary {
        LibrarySummary(rating: variant.rating, flag: variant.flag, edited: variant.edits != nil)
    }

    /// Each library item of the photograph: itself (the first variant) and its duplicates.
    func items(of photo: URL) -> [(item: URL, summary: LibrarySummary)] {
        guard !variants.isEmpty else { return [(photo, LibrarySummary())] }
        return variants.enumerated().map { index, variant in
            (index == 0 ? photo : photo.variant(variant.id), Self.summary(of: variant))
        }
    }
}

/// Which photographs the film strip shows. Remembered for the app, not per
/// folder.
struct LibraryFilter: Codable, Equatable, Sendable {
    enum Rating: Codable, Hashable, Sendable {
        case any
        /// At least this many stars, 1 to 5.
        case atLeast(Int)
        case unrated
    }
    enum Flag: String, Codable, CaseIterable, Sendable { case pick, unflagged, reject }

    var rating: Rating = .any
    /// The flags shown; empty shows every one.
    var flags: Set<Flag> = []
    var editedOnly = false

    var active: Bool { self != LibraryFilter() }

    func includes(_ summary: LibrarySummary) -> Bool {
        switch rating {
        case .any: break
        case .atLeast(let stars): if summary.rating < stars { return false }
        case .unrated: if summary.rating != 0 { return false }
        }
        if !flags.isEmpty {
            let flag: Flag = switch summary.flag { case .pick: .pick; case .reject: .reject; case nil: .unflagged }
            if !flags.contains(flag) { return false }
        }
        return !editedOnly || summary.edited
    }

    static let defaultsKey = "libraryFilter"

    static func load(_ defaults: UserDefaults = .standard) -> LibraryFilter {
        defaults.data(forKey: defaultsKey).flatMap { try? JSONDecoder().decode(LibraryFilter.self, from: $0) } ?? LibraryFilter()
    }

    func save(_ defaults: UserDefaults = .standard) {
        if active, let data = try? JSONEncoder().encode(self) { defaults.set(data, forKey: Self.defaultsKey) }
        else { defaults.removeObject(forKey: Self.defaultsKey) }
    }
}

/// The open folder's photographs and their ratings. The listing comes first
/// and the sidecars are read after it, off the main actor, so thumbnails
/// never wait for them; only an active filter does. Later listings re-read
/// only the sidecars whose modification date or size changed.
@MainActor @Observable final class PhotoLibrary {
    private(set) var directory: URL?
    private(set) var files: [PhotoFile] = []
    /// Each photograph's duplicates, by variant id, in the sidecar's order.
    private(set) var duplicates: [URL: [String]] = [:]
    private(set) var loading = false
    private(set) var failed = false
    /// Every sidecar of the listing has been read.
    private(set) var summariesLoaded = false
    private(set) var summaries: [URL: LibrarySummary] = [:]
    /// Ratings and edits can't be saved in the folder (a locked card, a
    /// read-only share).
    private(set) var readOnly = false
    var filter: LibraryFilter {
        didSet { if filter != oldValue { filter.save(defaults); filterChanged?() } }
    }

    /// Where sidecars are read, pending writes included; nil with sidecars off.
    var store: SidecarStore? {
        didSet {
            guard store !== oldValue else { return }
            summaries = [:]; sidecarFiles = [:]; summariesLoaded = false
            refreshNow()
        }
    }
    /// Told of each photograph whose sidecar changed on disk since it was
    /// last read here.
    @ObservationIgnored var sidecarChanged: ((URL) -> Void)?
    /// Told when the filter changes, so the selection can follow it.
    @ObservationIgnored var filterChanged: (() -> Void)?

    @ObservationIgnored private let defaults: UserDefaults
    /// Each photograph's sidecar file as last read; nil, it had none.
    @ObservationIgnored private var sidecarFiles: [URL: PhotoFile?] = [:]
    /// Counts the app's own changes to a summary, so a read that started
    /// before one doesn't undo it.
    @ObservationIgnored private var revisions: [URL: Int] = [:]
    @ObservationIgnored private var rescans: [UUID: AsyncStream<Void>.Continuation] = [:]
    /// Photographs being deleted: never listed, even while their files are
    /// still on their way to the Trash.
    @ObservationIgnored private var hidden: Set<URL> = []

    init(store: SidecarStore? = nil, defaults: UserDefaults = .standard) {
        self.store = store
        self.defaults = defaults
        filter = LibraryFilter.load(defaults)
    }

    func summary(for photo: URL) -> LibrarySummary { summaries[photo] ?? LibrarySummary() }

    /// The photographs, each followed by its duplicates.
    var items: [PhotoFile] {
        guard !duplicates.isEmpty else { return files }
        var items: [PhotoFile] = []
        items.reserveCapacity(files.count + duplicates.count)
        for file in files {
            items.append(file)
            for (index, id) in (duplicates[file.url] ?? []).enumerated() { items.append(file.duplicate(id, number: index + 2)) }
        }
        return items
    }

    /// The item's name: `IMG_0412.CR3`, or `IMG_0412.CR3 (2)` for its first duplicate.
    func name(of item: URL) -> String {
        let name = item.lastPathComponent
        guard let id = item.variantID, let index = duplicates[item.photoFile]?.firstIndex(of: id) else { return name }
        return "\(name) (\(index + 2))"
    }

    /// Reads `photo`'s sidecar again now, pending writes included: after the
    /// app changed it in ways the summaries can't follow (duplicates, edits).
    func reload(_ photo: URL) {
        guard let store else { return }
        let read: [(item: URL, summary: LibrarySummary)]
        do { read = try store.read(photo: photo)?.items(of: photo) ?? [(photo, LibrarySummary())] }
        catch let problem as PhotoSidecar.Problem { read = [(photo, LibrarySummary(problem: problem.message(for: photo)))] }
        catch { read = [(photo, LibrarySummary(problem: error.localizedDescription))] }
        var updated = summaries
        apply(read, for: photo, to: &updated)
        for item in read.map(\.item) { revisions[item, default: 0] += 1 }
        if updated != summaries { summaries = updated }
    }

    /// Sets the summaries and duplicates of `photo` from its sidecar as read.
    private func apply(_ read: [(item: URL, summary: LibrarySummary)], for photo: URL, to summaries: inout [URL: LibrarySummary]) {
        let ids = read.compactMap { $0.item.variantID }
        for gone in duplicates[photo] ?? [] where !ids.contains(gone) { summaries[photo.variant(gone)] = nil }
        if (duplicates[photo] ?? []) != ids { duplicates[photo] = ids.isEmpty ? nil : ids }
        for (item, summary) in read { summaries[item] = summary }
    }

    /// The app's own change, shown at once; the sidecar is written separately.
    func set(_ summary: LibrarySummary, for photo: URL) {
        revisions[photo, default: 0] += 1
        if summaries[photo] != summary { summaries[photo] = summary }
    }

    /// What the opened photograph's sidecar held, unless a summary is known
    /// already (one that couldn't be saved, say).
    func opened(_ photo: URL, summary: LibrarySummary) {
        if summaries[photo] == nil { summaries[photo] = summary }
    }

    /// Stops listing `photos`, with their duplicates, before their files go.
    func remove(_ photos: [URL]) {
        guard !photos.isEmpty else { return }
        hidden.formUnion(photos)
        let gone = Set(photos)
        files.removeAll { gone.contains($0.url) }
        for photo in photos {
            for id in duplicates[photo] ?? [] { summaries[photo.variant(id)] = nil }
            duplicates[photo] = nil; summaries[photo] = nil; sidecarFiles[photo] = nil
        }
    }

    /// Lists `photos` again if they're there: once their files have gone,
    /// or come back.
    func unhide(_ photos: [URL]) {
        hidden.subtract(photos)
        refreshNow()
    }

    /// The photographs the strip shows: those the filter lets through, and
    /// always `current`, which never vanishes under the cursor.
    func shown(current: URL?) -> [PhotoFile] { shown(keeping: current.map { [$0] } ?? []) }

    /// The photographs the strip and grid show: those the filter lets
    /// through, and always `kept` (the current photograph, those just rated).
    func shown(keeping kept: Set<URL>) -> [PhotoFile] {
        let items = items
        guard filter.active else { return items }
        return items.filter { kept.contains($0.url) || filter.includes(summary(for: $0.url)) }
    }

    /// An active filter is waiting for sidecars still being read.
    var filtering: Bool { filter.active && (loading || !summariesLoaded) }

    /// Lists the folder again, such as when the window becomes key: file
    /// events on network shares get lost.
    func refreshNow() {
        for rescan in rescans.values { rescan.yield(()) }
    }

    /// Lists `directory` and keeps the listing current until cancelled.
    func watch(_ directory: URL) async {
        if self.directory != directory {
            self.directory = directory
            files = []; sidecarFiles = [:]; summariesLoaded = false; failed = false
            duplicates = [:]
            // The open photograph's summary stays.
            summaries = summaries.filter { $0.key.photoFile.deletingLastPathComponent().path == directory.path }
        }
        loading = files.isEmpty
        let id = UUID()
        let (requests, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        rescans[id] = continuation
        // Armed before the first listing, so arrivals during it aren't missed.
        let watcher = PhotoFolderWatcher(directory: directory)
        let forward = watcher.map { watcher in Task { for await _ in watcher.changes { continuation.yield(()) } } }
        defer {
            watcher?.stop(); forward?.cancel(); continuation.finish()
            rescans[id] = nil
        }
        await refresh(directory)
        for await _ in requests {
            guard !Task.isCancelled else { return }
            await refresh(directory)
        }
    }

    private func refresh(_ directory: URL) async {
        let scan = Task.detached(priority: .utility) {
            (try PhotoFolder.listing(in: directory), !PhotoLibrary.canWrite(in: directory))
        }
        let listing: (photos: [PhotoFile], sidecars: [String: PhotoFile]), readOnly: Bool
        do {
            (listing, readOnly) = try await withTaskCancellationHandler { try await scan.value } onCancel: { scan.cancel() }
            try Task.checkCancellation()
        } catch {
            guard !Task.isCancelled, self.directory == directory else { return }
            failed = true; loading = false
            return
        }
        guard self.directory == directory else { return }
        let photos = hidden.isEmpty ? listing.photos : listing.photos.filter { !hidden.contains($0.url) }
        if files != photos { files = photos }
        if self.readOnly != readOnly { self.readOnly = readOnly }
        failed = false; loading = false
        guard let store else { summariesLoaded = true; return }
        // The sidecars that are new, changed or gone since last read.
        var changed: [URL: PhotoFile?] = [:]
        for photo in photos {
            let sidecar = listing.sidecars[photo.name]
            if sidecarFiles[photo.url] != .some(sidecar) { changed[photo.url] = sidecar }
        }
        let present = Set(photos.map(\.url))
        for gone in sidecarFiles.keys where !present.contains(gone) { sidecarFiles[gone] = nil }
        for gone in duplicates.keys where !present.contains(gone) { duplicates[gone] = nil }
        guard !changed.isEmpty else { summariesLoaded = true; return }
        let started = revisions
        let urls = Array(changed.keys)
        let read = await Task.detached(priority: .utility) { () -> [URL: [(item: URL, summary: LibrarySummary)]] in
            var read: [URL: [(item: URL, summary: LibrarySummary)]] = [:]
            for photo in urls {
                guard !Task.isCancelled else { break }
                do { read[photo] = try store.read(photo: photo)?.items(of: photo) ?? [(photo, LibrarySummary())] }
                catch let problem as PhotoSidecar.Problem { read[photo] = [(photo, LibrarySummary(problem: problem.message(for: photo)))] }
                catch { read[photo] = [(photo, LibrarySummary(problem: error.localizedDescription))] }
            }
            return read
        }.value
        guard self.directory == directory, self.store === store, !Task.isCancelled else { return }
        var updated = summaries
        var notify: [URL] = []
        for (photo, items) in read {
            let known = sidecarFiles.keys.contains(photo)
            sidecarFiles[photo] = .some(changed[photo] ?? nil)
            if known { notify.append(photo) }
            // The app changed it meanwhile; its own write brings the file back here.
            guard items.allSatisfy({ revisions[$0.item] == started[$0.item] }), revisions[photo] == started[photo] else { continue }
            apply(items, for: photo, to: &updated)
        }
        if updated != summaries { summaries = updated }
        summariesLoaded = true
        for photo in notify { sidecarChanged?(photo) }
    }

    /// Whether sidecars can be written beside `photo`.
    nonisolated static func canWrite(beside photo: URL) -> Bool { canWrite(in: photo.deletingLastPathComponent()) }
    nonisolated static func canWrite(in folder: URL) -> Bool { Darwin.access(folder.path, W_OK) == 0 }
}
