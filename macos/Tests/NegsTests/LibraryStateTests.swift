import AppKit
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
private let data = root.appendingPathComponent("data")
private let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: data)

/// Ratings, flags, edits and duplicates kept in sidecars, with undo, against
/// seeded random sequences of user actions: switching photographs, slider
/// edits and drags, Paste and Reset Edits on several photographs, Duplicate,
/// Delete, Undo and Redo, and changes another app makes to the files. Each
/// time the user stops, what the app shows must be what is on disk, and an
/// action undone at once must leave the files and the library as they were.
@MainActor final class LibraryStateTests: XCTestCase {
    private var restoreDefaults: [() -> Void] = []
    private var cleanup: [() -> Void] = []

    override func tearDown() async throws {
        for step in cleanup.reversed() { step() }
        cleanup = []
        for restore in restoreDefaults.reversed() { restore() }
        restoreDefaults = []
    }

    private func fixture(in dir: URL, _ name: String, seed: Int) throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        for i in 0..<(96 * 64 * 3) { bitmap.bitmapData![i] = UInt8((i * (seed + 1)) % 251) }
        let url = dir.appendingPathComponent(name)
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    private func temporaryFolder(_ name: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-library-state-\(name)-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        cleanup.append { try? FileManager.default.removeItem(at: url) }
        return url
    }

    /// `relaunch`: the same Mac's settings, as the previous model left them.
    private func makeModel(store: SidecarStore, bin: URL, relaunch: Bool = false) async throws -> EditorModel {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        if !relaunch {
            let defaults = UserDefaults.standard
            let cleared = ["advanceAfterRating", LibraryFilter.defaultsKey, EditorModel.newPhotoFilmKey, EditorModel.newPhotoPaperKey]
            let saved = (["autoPlaceNewImages", "sidecars", "recentFiles"] + cleared).map { ($0, defaults.object(forKey: $0)) }
            restoreDefaults.append { for (key, value) in saved { defaults.set(value, forKey: key) } }
            defaults.set(false, forKey: "autoPlaceNewImages")
            defaults.set(true, forKey: "sidecars")
            for key in cleared { defaults.removeObject(forKey: key) }
        }
        let cache = try temporaryFolder("cache")
        let model = EditorModel(previewCache: RenderedPreviewCache(file: cache.appendingPathComponent("preview.cache")), sidecars: store)
        cleanup.append { model.shutdown() }
        // Every item's thumbnail is developed, not only those on screen (none here).
        model.lowPowerMode = false
        model.editedThumbnails = EditedThumbnailStore(directory: cache.appendingPathComponent("thumbnails", isDirectory: true), limit: 1 << 30)
        model.renderArchive = EditedThumbnailStore(directory: cache.appendingPathComponent("renders", isDirectory: true), limit: 1 << 30)
        // Never the user's own Trash.
        model.trash = PhotoTrash(available: { _ in true }, trash: { file in
            let landed = bin.appendingPathComponent(UUID().uuidString + "-" + file.lastPathComponent)
            try FileManager.default.moveItem(at: file, to: landed)
            return landed
        }, remove: { try FileManager.default.removeItem(at: $0) })
        await model.start(paths: paths, restoration: Task { nil })
        return model
    }

    // MARK: What is on disk

    /// A sidecar as stored, without the `app` that wrote it last or the id
    /// of its first variant, which the app makes up for a photograph that
    /// has none and nothing names.
    private static func stored(_ photo: URL) -> JSONValue? {
        guard case .object(var top)? = try? SidecarStore.readRaw(photo: photo) else { return nil }
        top["app"] = nil
        if case .array(var variants)? = top["variants"], case .object(var first)? = variants.first {
            first["id"] = .string("first")
            variants[0] = .object(first)
            top["variants"] = .array(variants)
        }
        return .object(top)
    }

    /// `item`'s variant on disk, or nil when its photograph has no sidecar
    /// (or the variant is gone: `gone` then).
    private static func variant(_ item: URL) throws -> (variant: PhotoSidecar.Variant?, version: Int, gone: Bool) {
        guard let raw = try SidecarStore.readRaw(photo: item.photoFile) else { return (nil, SidecarDefaults.latest, item.variantID != nil) }
        let sidecar = try PhotoSidecar(raw: raw)
        let variant = item.variantID.map { id in sidecar.variants.first { $0.id == id } } ?? sidecar.variants.first
        return (variant, sidecar.defaultsVersion, item.variantID != nil && variant == nil)
    }

    /// What opening `item` shows, as `userEdits`, from its sidecar on disk;
    /// nil if its variant is gone.
    private static func savedEdits(_ item: URL, model: EditorModel) throws -> EditState? {
        let (variant, version, gone) = try variant(item)
        guard !gone else { return nil }
        guard let edits = variant?.edits else { return EditorModel.userEdits(model.plainState) }
        let saved = try PhotoSidecar.decodeEdits(edits, table: SidecarDefaults.load(version: version, data: data))
        return EditorModel.userEdits(model.restoredState(saved.state, legacy: false, matchSolved: saved.matchSolved))
    }

    /// The files and the library as the user would find them.
    private struct Snapshot: Equatable {
        var photos: [String: JSONValue?]
        var items: [String]
        var summaries: [String: String]

        func difference(from other: Snapshot) -> String {
            var lines: [String] = []
            for name in Set(photos.keys).union(other.photos.keys).sorted() where photos[name] != other.photos[name] {
                lines.append("\(name): \(describe(other.photos[name])) → \(describe(photos[name]))")
            }
            if items != other.items { lines.append("items \(other.items) → \(items)") }
            for name in Set(summaries.keys).union(other.summaries.keys).sorted() where summaries[name] != other.summaries[name] {
                lines.append("\(name): \(other.summaries[name] ?? "none") → \(summaries[name] ?? "none")")
            }
            return lines.joined(separator: "\n    ")
        }
        private func describe(_ value: JSONValue??) -> String {
            switch value {
            case .none: return "absent"
            case .some(.none): return "no sidecar"
            case .some(.some(let json)): return (try? String(data: PhotoSidecar.encode(json), encoding: .utf8)?.replacingOccurrences(of: "\n", with: " ")) ?? "?"
            }
        }
    }

    private static func snapshot(_ model: EditorModel, folder: URL) -> Snapshot {
        let names = ((try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []).filter { $0.hasSuffix(".png") }
        var photos: [String: JSONValue?] = [:]
        for name in names { photos[name] = .some(stored(folder.appendingPathComponent(name))) }
        let items = model.library.items.map { model.library.name(of: $0.url) }
        var summaries: [String: String] = [:]
        for item in model.library.items {
            let summary = model.library.summary(for: item.url)
            summaries[model.library.name(of: item.url)] = "★\(summary.rating) \(summary.flag.map { "\($0)" } ?? "-")\(summary.edited ? " edited" : "")"
        }
        return Snapshot(photos: photos, items: items, summaries: summaries)
    }

    // MARK: The invariants

    /// Once everything is written: the library shows each item's rating,
    /// flag, edits and duplicates as on disk, the open photograph shows the
    /// edits its sidecar keeps, and each photograph left since keeps the
    /// edits it showed (`left`).
    private static func violations(_ model: EditorModel, folder: URL, left: [URL: EditState]) -> [String] {
        var found: [String] = []
        let library = model.library
        let onDisk = ((try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []).filter { $0.hasSuffix(".png") }.sorted()
        let listed = library.files.map(\.url.lastPathComponent).sorted()
        if listed != onDisk { found.append("Listing: \(listed) but the folder has \(onDisk)") }
        for file in library.files {
            let photo = file.url
            let raw: JSONValue?
            do { raw = try SidecarStore.readRaw(photo: photo) } catch { found.append("Readable: \(photo.lastPathComponent) \(error)"); continue }
            let sidecar = try? raw.map(PhotoSidecar.init(raw:))
            let ids = (sidecar?.variants ?? []).dropFirst().map(\.id)
            if (library.duplicates[photo] ?? []) != Array(ids) {
                found.append("Duplicates: \(photo.lastPathComponent) lists \(library.duplicates[photo] ?? []) but its sidecar has \(Array(ids))")
            }
            let variants = sidecar?.variants ?? []
            for (index, variant) in (variants.isEmpty ? [nil] : variants.map { $0 }).enumerated() {
                let item = index == 0 ? photo : photo.variant(variant!.id)
                let name = library.name(of: item)
                let summary = library.summary(for: item)
                if summary.rating != (variant?.rating ?? 0) || summary.flag != variant?.flag {
                    found.append("Summary: \(name) shows ★\(summary.rating) \(String(describing: summary.flag)) but its sidecar has ★\(variant?.rating ?? 0) \(String(describing: variant?.flag))")
                }
                if summary.edited != (variant?.edits != nil) {
                    found.append("Edited: \(name) shows edited \(summary.edited) but its sidecar \(variant?.edits != nil ? "has" : "has no") edits")
                }
                let look = variant?.edits.flatMap { PhotoSidecar.look(of: $0, defaultsVersion: sidecar?.defaultsVersion ?? SidecarDefaults.latest) }
                if library.looks[item] != look {
                    found.append("Look: \(name)'s thumbnail look is \(library.looks[item] ?? "none") but its sidecar's is \(look ?? "none")")
                }
            }
        }
        if let open = model.sourceItem, FileManager.default.fileExists(atPath: open.photoFile.path), model.hasCatalog {
            do {
                if let saved = try savedEdits(open, model: model), saved != EditorModel.userEdits(model.state) {
                    found.append("ShownIsSaved: \(library.name(of: open)) shows edits its sidecar doesn't have: \(editDifference(EditorModel.userEdits(model.state), saved))")
                }
            } catch { found.append("ShownIsSaved: \(open.lastPathComponent)'s sidecar can't be read: \(error)") }
        }
        for (item, edits) in left where item != model.sourceItem && FileManager.default.fileExists(atPath: item.photoFile.path) {
            if let saved = try? savedEdits(item, model: model), saved != edits {
                found.append("LeftIsSaved: \(library.name(of: item)) was left showing edits its sidecar doesn't have: \(editDifference(edits, saved))")
            }
        }
        return found
    }

    private static func editDifference(_ shown: EditState, _ saved: EditState) -> String {
        var parts: [String] = []
        if shown.film != saved.film { parts.append("film \(shown.film) vs \(saved.film)") }
        if shown.paper != saved.paper { parts.append("paper \(shown.paper) vs \(saved.paper)") }
        if shown.crop != saved.crop { parts.append("crop") }
        if shown.border != saved.border { parts.append("border") }
        for path in ["prep.contrast", "prep.highlights", "prep.shadows", "io.rotate_cw"] where shown.params[path] != saved.params[path] {
            parts.append("\(path) \(shown.params[path]) vs \(saved.params[path])")
        }
        if parts.isEmpty, shown.params != saved.params { parts.append("other parameters") }
        if parts.isEmpty { parts.append("\(shown) vs \(saved)") }
        return parts.joined(separator: ", ")
    }

    // MARK: Regressions

    /// A photograph whose sidecar can't be written (its folder became
    /// read-only) keeps its saved edits; leaving it filed the preview of the
    /// unsaved ones as the saved edits' thumbnail and kept develop, so the
    /// grid showed edits that opening it doesn't.
    func testUnsavedEditsAreNeverFiledAsTheSavedEditsThumbnail() async throws {
        let folder = try temporaryFolder("photos"), bin = try temporaryFolder("trash")
        let a = try fixture(in: folder, "a.png", seed: 0), b = try fixture(in: folder, "b.png", seed: 1)
        let model = try await makeModel(store: SidecarStore(), bin: bin)
        var saved: [(item: URL, look: String, edits: EditState, kind: String)] = []
        model.thumbnailSaved = { saved.append(($0, $1, $2, $3)) }
        model.open(a)
        let watching = Task { await model.library.watch(folder) }
        cleanup.append { watching.cancel() }
        func waitUntil(_ predicate: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(30)
            while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
            XCTAssertTrue(predicate())
        }
        // Thumbnails are filed once the engine's identity is read.
        try await waitUntil { model.hasCatalog && model.library.summariesLoaded && model.previewCurrent && model.thumbnailStamp != nil }
        model.set("prep.contrast", .number(20))
        await model.flushSidecars()
        try await waitUntil { model.library.looks[a] != nil && model.previewCurrent }
        let look = try XCTUnwrap(model.library.looks[a])

        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: folder.path)
        cleanup.append { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path) }
        model.set("prep.contrast", .number(-30))
        await model.flushSidecars()
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(try SidecarStore.readRaw(photo: a)?["variants"].array.first?["edits"]["params"]["prep"]["contrast"], .number(20), "not saved")
        model.open(b)
        try await waitUntil { model.previewCurrent }
        XCTAssertEqual(model.library.looks[a], look, "the library shows the edits its sidecar keeps")
        let filed = saved.filter { $0.item == a && $0.look == look }
        XCTAssertTrue(filed.allSatisfy { $0.edits.params["prep.contrast"] == .number(20) },
                      "filed under the saved edits' look: \(filed.map { "\($0.kind) with contrast \($0.edits.params["prep.contrast"])" })")
    }

    /// A folder of `count` photographs, the first open, everything read.
    private func openFolder(_ count: Int) async throws -> (model: EditorModel, photos: [URL]) {
        let folder = try temporaryFolder("photos"), bin = try temporaryFolder("trash")
        let photos = try (0..<count).map { try fixture(in: folder, "\(Character(UnicodeScalar(97 + $0)!)).png", seed: $0) }
        let model = try await makeModel(store: SidecarStore(), bin: bin)
        model.open(photos[0])
        let watching = Task { await model.library.watch(folder) }
        cleanup.append { watching.cancel() }
        let deadline = Date().addingTimeInterval(30)
        while !(model.hasCatalog && model.library.summariesLoaded && model.library.items.count == count), Date() < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(model.hasCatalog)
        return (model, photos)
    }

    /// Random sequences: a photograph edited when the new photographs' film
    /// changed kept its film, as it should, but undoing its edits then took
    /// it for unedited and removed them, so it opened with the new film.
    func testUndoingEveryEditAfterANewDefaultFilmKeepsTheFilmShown() async throws {
        let (model, photos) = try await openFolder(1)
        let film = model.film
        let other = try XCTUnwrap(["kodak_portra_400", "kodak_ektar_100"].first { $0 != film })
        model.set("prep.contrast", .number(20))
        model.setNewPhotoFilm(other)
        XCTAssertEqual(model.film, film, "an edited photograph doesn't follow")
        model.set("prep.contrast", model.defaultValue("prep.contrast"))
        await model.flushSidecars()
        XCTAssertEqual(try Self.savedEdits(photos[0], model: model), EditorModel.userEdits(model.state), "what it shows is what opening it shows")
        XCTAssertEqual(model.film, film)
    }

    /// Random sequences: Paste Edits from a photograph without edits leaves
    /// the open one without edits, but it showed the film the copy had, the
    /// new photographs' film then, though it opens with today's.
    func testPastingFromAnUneditedPhotographShowsWhatItOpensWith() async throws {
        let (model, photos) = try await openFolder(2)
        let film = model.film
        let other = try XCTUnwrap(["kodak_portra_400", "kodak_ektar_100"].first { $0 != film })
        model.copyEdits(from: photos[1])
        model.set("prep.contrast", .number(20))
        model.setNewPhotoFilm(other)
        model.pasteEdits(onto: [photos[0]])
        await model.flushSidecars()
        XCTAssertNil(try SidecarStore.readRaw(photo: photos[0]), "without edits")
        XCTAssertEqual(model.film, other, "as it opens")
        XCTAssertEqual(try Self.savedEdits(photos[0], model: model), EditorModel.userEdits(model.state))
    }

    // MARK: Random sequences

    /// SplitMix64, as in `FramePipelineTests`.
    private struct Random {
        var state: UInt64
        mutating func next() -> UInt64 {
            state &+= 0x9E37_79B9_7F4A_7C15
            var z = state
            z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
            z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
            return z ^ (z >> 31)
        }
        mutating func below(_ n: Int) -> Int { Int(next() % UInt64(n)) }
        mutating func chance(_ p: Double) -> Bool { Double(next() % 10_000) / 10_000 < p }
        mutating func pick<T>(_ items: [T]) -> T { items[below(items.count)] }
        mutating func subset<T>(_ items: [T]) -> [T] {
            let some = items.filter { _ in chance(0.5) }
            return some.isEmpty ? [pick(items)] : some
        }
    }

    /// NEGS_LIBRARY_SEEDS (default 3) runs, NEGS_LIBRARY_STEPS (default 60)
    /// actions each, NEGS_LIBRARY_SEED a single seed.
    func testRandomActionSequencesKeepSidecarsAndTheLibraryInStep() async throws {
        let environment = ProcessInfo.processInfo.environment
        let seeds: [UInt64] = environment["NEGS_LIBRARY_SEED"].flatMap(UInt64.init).map { [$0] }
            ?? Array(1...UInt64(environment["NEGS_LIBRARY_SEEDS"].flatMap(Int.init) ?? 3))
        let steps = environment["NEGS_LIBRARY_STEPS"].flatMap(Int.init) ?? 60
        for seed in seeds {
            try await runSequence(seed: seed, steps: steps)
            for step in cleanup.reversed() { step() }
            cleanup = []
        }
    }

    private func runSequence(seed: UInt64, steps: Int) async throws {
        var random = Random(state: seed)
        let folder = try temporaryFolder("photos"), bin = try temporaryFolder("trash")
        let photos = try ["a.png", "b.png", "c.png", "d.png"].enumerated().map { try fixture(in: folder, $0.element, seed: $0.offset) }
        let store = SidecarStore()
        let model = try await makeModel(store: store, bin: bin)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        model.open(photos[0])
        let watching = Task { await model.library.watch(folder) }
        cleanup.append { watching.cancel() }
        let deadline = Date().addingTimeInterval(30)
        while !(model.library.summariesLoaded && model.library.items.count == 4 && model.hasCatalog), Date() < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertTrue(model.hasCatalog && model.library.items.count == 4)

        var log: [String] = []
        var failures: [String] = []
        /// The edits each photograph showed when it was left, until
        /// something else changes them.
        var left: [URL: EditState] = [:]
        /// Every thumbnail and kept develop saved, with the edits it shows,
        /// and the edits each look was seen to stand for on disk.
        var thumbnails: [(item: URL, look: String, edits: EditState, kind: String, after: String)] = []
        var looks: [URL: [String: EditState]] = [:]
        model.thumbnailSaved = { item, look, edits, kind in thumbnails.append((item, look, edits, kind, log.last ?? "")) }
        func forget(_ items: [URL]) { for item in items { left[item] = nil; left[item.photoFile] = nil } }
        func forgetDuplicates(of items: [URL]) { for key in left.keys where items.contains(key.photoFile) { left[key] = nil } }
        func leaving() { if let open = model.sourceItem, model.hasCatalog { left[open] = EditorModel.userEdits(model.state) } }
        func name(_ item: URL) -> String { model.library.name(of: item) }

        /// Everything written, and the folder listed again.
        func writeAndList() async throws {
            await model.flushSidecars()
            model.library.refreshNow()
            try await Task.sleep(for: .milliseconds(30))
        }
        /// The user stops: within a few seconds everything is in step.
        func settle(_ when: String) async throws {
            var found: [String] = []
            for _ in 0..<60 {
                try await writeAndList()
                found = Self.violations(model, folder: folder, left: left)
                if found.isEmpty {
                    // A thumbnail filed under a look shows the edits that look stands for.
                    for item in model.library.items.map(\.url) {
                        if let look = model.library.looks[item] ?? model.uneditedLook, let edits = try? Self.savedEdits(item, model: model) {
                            looks[item, default: [:]][look] = edits
                        }
                    }
                    for saved in thumbnails {
                        let expected = saved.look == model.uneditedLook ? EditorModel.userEdits(model.plainState) : looks[saved.item]?[saved.look]
                        if let expected, expected != saved.edits {
                            found.append("ThumbnailShowsItsLook: \(saved.kind) of \(saved.item.lastPathComponent) shows \(Self.editDifference(saved.edits, expected)) (saved after \(saved.after))")
                        }
                    }
                    thumbnails.removeAll { saved in saved.look == model.uneditedLook || looks[saved.item]?[saved.look] != nil }
                    if found.isEmpty { return }
                    break
                }
                try await Task.sleep(for: .milliseconds(50))
            }
            if failures.count < 3 {
                failures.append("seed \(seed): \(when)\n    " + found.joined(separator: "\n    ") + "\n  after \(log.suffix(12).joined(separator: "; "))")
            }
        }
        /// An action, as one undo group.
        func act(_ name: String, _ action: () -> Void) {
            log.append(name)
            undo.beginUndoGrouping(); action(); undo.endUndoGrouping()
        }
        /// Undo straight after an undoable action puts the files and the
        /// library back as they were, and Redo brings the change back.
        func undoRedoCheck(_ before: Snapshot, _ name: String) async throws {
            guard failures.isEmpty, undo.canUndo, !undo.undoActionName.isEmpty else { return }
            try await settle("after \(name)")
            let after = Self.snapshot(model, folder: folder)
            guard after != before else { return }
            for (when, expected, step) in [("undo", before, { undo.undo() }), ("redo", after, { undo.redo() })] as [(String, Snapshot, () -> Void)] {
                log.append("\(when) \(name)")
                step()
                forget(Array(left.keys))
                var now = Self.snapshot(model, folder: folder)
                for _ in 0..<60 where now != expected {
                    try await writeAndList()
                    now = Self.snapshot(model, folder: folder)
                    if now != expected { try await Task.sleep(for: .milliseconds(50)) }
                }
                if now != expected, failures.count < 3 {
                    failures.append("seed \(seed): \(when) of \(name) left\n    \(now.difference(from: expected))\n  after \(log.suffix(12).joined(separator: "; "))")
                    return
                }
                try await settle("after \(when) of \(name)")
            }
        }

        let films = ["kodak_portra_400", "kodak_ektar_100", "kodak_gold_200"].filter { id in model.films.contains { $0.id == id } }
        let sliders = ["prep.contrast", "prep.highlights", "prep.shadows"]
        for _ in 0..<steps where failures.isEmpty {
            // An undo check starts from everything written.
            let checkUndo = random.chance(0.35)
            if checkUndo { log.append("settle"); try await settle("on settling") }
            let items = model.library.items.map(\.url)
            let open = model.sourceItem
            let before = Self.snapshot(model, folder: folder)
            var undoable: String?
            switch random.below(21) {
            case 0, 1, 2:
                guard open != nil else { continue }
                let path = random.pick(sliders)
                let value: JSONValue = random.chance(0.25) ? model.defaultValue(path) : .number(Double(random.below(9) - 4) * 10)
                act("\(path) \(value)") { model.set(path, value) }
                undoable = "\(path) \(value)"
            case 3:
                guard open != nil else { continue }
                let film = random.pick(films)
                act("film \(film)") { model.chooseFilm(film) }
                undoable = "film \(film)"
            case 4:
                guard open != nil else { continue }
                let path = random.pick(sliders), moves = random.below(4)
                log.append("drag \(path) ×\(moves)")
                model.beginInteraction()
                for _ in 0..<moves {
                    model.set(path, .number(Double(random.below(9) - 4) * 10))
                    try await Task.sleep(for: .milliseconds(random.below(300)))
                }
                undo.beginUndoGrouping(); model.endInteraction(); undo.endUndoGrouping()
            case 5, 6:
                let item = random.pick(items)
                leaving()
                log.append("open \(name(item))")
                model.open(item)
            case 7, 8:
                let targets = random.chance(0.5) || open == nil ? random.subset(items) : [open!]
                if random.chance(0.6) {
                    let stars = random.below(6)
                    act("rate \(targets.map(name)) \(stars)") { model.setRating(stars, targets: targets) }
                    undoable = "rating"
                } else {
                    let flag = random.pick([nil, .pick, .reject] as [PhotoSidecar.Flag?])
                    act("flag \(targets.map(name)) \(String(describing: flag))") { model.setFlag(flag, targets: targets) }
                    undoable = "flag"
                }
            case 9:
                let item = random.pick(items)
                log.append("copy \(name(item))")
                model.copyEdits(from: item, groups: random.chance(0.7) ? EditGroup.standard : Set(EditGroup.allCases))
            case 10, 11:
                guard model.copiedEdits != nil else { continue }
                let targets = random.subset(items)
                leaving()
                act("paste onto \(targets.map(name))") { model.pasteEdits(onto: targets) }
                forget(targets)
                undoable = "paste"
            case 12:
                guard model.previousEdits != nil else { continue }
                let targets = random.subset(items)
                leaving()
                act("paste previous onto \(targets.map(name))") { model.pasteFromPrevious(onto: targets) }
                forget(targets)
                undoable = "paste previous"
            case 13:
                let targets = random.subset(items)
                leaving()
                act("reset \(targets.map(name))") { model.resetEdits(of: targets) }
                forget(targets)
                undoable = "reset"
            case 14:
                guard items.count < 9 else { continue }
                let targets = random.subset(items)
                leaving()
                act("duplicate \(targets.map(name))") { model.duplicate(targets) }
                undoable = "duplicate"
            case 15:
                let targets = random.subset(items)
                guard Set(targets.map(\.photoFile)).count < model.library.files.count - 1 else { continue }
                leaving()
                act("delete \(targets.map(name))") { model.delete(model.deletePlan(targets)) }
                forget(targets); forgetDuplicates(of: targets.filter { $0.variantID == nil })
                undoable = "delete"
            case 16:
                if random.chance(0.5) {
                    guard undo.canUndo else { continue }
                    log.append("undo \(undo.undoActionName)"); undo.undo()
                } else {
                    guard undo.canRedo else { continue }
                    log.append("redo \(undo.redoActionName)"); undo.redo()
                }
                forget(Array(left.keys))
            case 17:
                if model.mode == .grid { log.append("edit view"); model.editCurrent() } else { leaving(); log.append("grid"); model.showGrid() }
            case 18:
                // Another app or Mac changes a sidecar, while nothing of ours is being written.
                let photo = random.pick(model.library.files.map(\.url))
                await store.flush()
                let raw = try? SidecarStore.readRaw(photo: photo)
                let version = (try? raw.map(PhotoSidecar.init(raw:)))??.defaultsVersion ?? SidecarDefaults.latest
                var patch = SidecarPatch(variant: "other", primary: true, defaultsVersion: version, app: "other")
                if random.chance(0.5) {
                    patch.rating = random.below(6)
                    log.append("another app rates \(photo.lastPathComponent) \(patch.rating!)")
                } else {
                    let path = random.pick(sliders), value = Double(random.below(9) - 4) * 10
                    var edits = ["film": JSONValue.string(random.pick(films)), "paper": .string(model.paper)]
                    edits["params"] = .object([String(path.split(separator: ".")[0]): .object([String(path.split(separator: ".")[1]): .number(value)])])
                    patch.edits = .some(.object(edits))
                    log.append("another app sets \(photo.lastPathComponent) \(path) \(value)")
                    forget([photo]); forgetDuplicates(of: [photo])
                }
                if let changed = try? patch.applied(to: raw) {
                    try PhotoSidecar.encode(changed).write(to: PhotoSidecar.url(for: photo), options: .atomic)
                }
                model.library.refreshNow()
            case 19:
                // Settings › New photographs: every photograph without edits
                // opens, and develops its thumbnail, with the new film.
                let film = random.pick(films.filter { $0 != model.newPhotoFilm })
                log.append("new photographs' film \(film)")
                model.setNewPhotoFilm(film)
                forget(Array(left.keys))
            default:
                log.append("quit and relaunch")
                try await relaunchCheck(model: model, store: store, folder: folder, seed: seed, log: log, failures: &failures)
            }
            if let undoable, checkUndo { try await undoRedoCheck(before, undoable) }
            // The pause before the next action: none, a moment, past the
            // sidecar's one-second write, or until everything is written.
            switch random.below(5) {
            case 0: await Task.yield()
            case 1: try await Task.sleep(for: .milliseconds(random.below(150)))
            case 2: try await Task.sleep(for: .milliseconds(900 + random.below(300)))
            default: log.append("settle"); try await settle("on settling")
            }
        }
        if failures.isEmpty { try await settle("at the end") }
        if failures.isEmpty { try await relaunchCheck(model: model, store: store, folder: folder, seed: seed, log: log, failures: &failures) }
        XCTAssertTrue(failures.isEmpty, failures.joined(separator: "\n"))
        print("LIBRARY-STATE seed \(seed): \(log.count) actions\(failures.isEmpty ? "" : ", FAILED")")
    }

    /// Quitting writes everything: a new launch opening the same photograph
    /// shows the same edits, rating and flag.
    private func relaunchCheck(model: EditorModel, store: SidecarStore, folder: URL, seed: UInt64, log: [String], failures: inout [String]) async throws {
        await model.flushSidecars()
        guard let open = model.sourceItem, model.hasCatalog, FileManager.default.fileExists(atPath: open.photoFile.path) else { return }
        let shown = EditorModel.userEdits(model.state), summary = model.library.summary(for: open)
        let next = try await makeModel(store: SidecarStore(), bin: try temporaryFolder("trash"), relaunch: true)
        next.open(open)
        let watching = Task { await next.library.watch(folder) }
        defer { watching.cancel(); next.shutdown() }
        let deadline = Date().addingTimeInterval(30)
        while !next.hasCatalog || !next.library.summariesLoaded || next.library.loading, Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        let reopened = EditorModel.userEdits(next.state), nextSummary = next.library.summary(for: open)
        if reopened != shown, failures.count < 3 {
            failures.append("seed \(seed): relaunch shows \(model.library.name(of: open)) with other edits: \(Self.editDifference(shown, reopened)); relaunched new film \(next.newPhotoFilm), defaults \(UserDefaults.standard.string(forKey: EditorModel.newPhotoFilmKey) ?? "none"), main new film \(model.newPhotoFilm), catalog \(next.hasCatalog), open \(String(describing: next.sourceItem)), disk \(String(describing: Self.stored(open.photoFile)))\n  after \(log.suffix(12).joined(separator: "; "))")
        }
        if (nextSummary.rating, nextSummary.flag) != (summary.rating, summary.flag), failures.count < 3 {
            failures.append("seed \(seed): relaunch shows \(model.library.name(of: open)) ★\(nextSummary.rating) \(String(describing: nextSummary.flag)), was ★\(summary.rating) \(String(describing: summary.flag)); disk \(String(describing: Self.stored(open.photoFile))); relaunched open \(String(describing: next.sourceItem)), loaded \(next.library.summariesLoaded), loading \(next.library.loading), items \(next.library.items.count), summaries \(next.library.summaries.count)\n  after \(log.suffix(12).joined(separator: "; "))")
        }
    }
}
