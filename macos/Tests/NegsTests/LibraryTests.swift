import AppKit
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
private let data = root.appendingPathComponent("data")

private func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-library-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func json(_ text: String) throws -> JSONValue { try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) }

private func write(_ text: String, sidecarOf photo: URL) throws {
    try Data(text.utf8).write(to: PhotoSidecar.url(for: photo))
}

/// Ratings and flags in the sidecar format, and the filter's rules.
final class LibraryFormatTests: XCTestCase {
    private func patch(rating: Int? = nil, flag: PhotoSidecar.Flag?? = nil, variant: String = "v1", primary: Bool = false) -> SidecarPatch {
        SidecarPatch(variant: variant, primary: primary, rating: rating, flag: flag, defaultsVersion: 1, app: "test")
    }

    func testRatingsAndFlagsChangeOnlyTheirOwnFields() throws {
        let file = try json(#"{"negs": 1, "defaultsVersion": 1, "app": "old", "future": {"x": 1}, "variants": [{"id": "v1", "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "newer": true}, "tags": ["a"]}]}"#)
        let rated = try XCTUnwrap(patch(rating: 3).applied(to: file))
        XCTAssertEqual(rated["variants"].array.first?["rating"], .number(3))
        XCTAssertEqual(rated["variants"].array.first?["edits"], file["variants"].array.first?["edits"], "edits, unknown keys included, are untouched")
        XCTAssertEqual(rated["variants"].array.first?["tags"], file["variants"].array.first?["tags"])
        XCTAssertEqual(rated["future"], file["future"])
        XCTAssertEqual(rated["app"], .string("test"))
        let flagged = try XCTUnwrap(patch(flag: .some(.reject)).applied(to: rated))
        XCTAssertEqual(flagged["variants"].array.first?["flag"], .string("reject"))
        XCTAssertEqual(flagged["variants"].array.first?["rating"], .number(3), "a 3-star reject: stars and flags are independent")
        let cleared = try XCTUnwrap(patch(rating: 0, flag: .some(nil)).applied(to: flagged))
        XCTAssertNil(cleared["variants"].array.first?.object?["rating"])
        XCTAssertNil(cleared["variants"].array.first?.object?["flag"])
        XCTAssertNotNil(cleared["variants"].array.first?.object?["edits"])
        // Read back as the library sees it.
        XCTAssertEqual(try PhotoSidecar(raw: flagged).summary, LibrarySummary(rating: 3, flag: .reject, edited: true))
    }

    func testARatingAloneMakesAFileAndClearingItRemovesIt() throws {
        XCTAssertNil(try patch(rating: 0).applied(to: nil), "nothing to hold, no file")
        XCTAssertNil(try patch(flag: .some(nil)).applied(to: nil))
        let made = try XCTUnwrap(patch(rating: 2).applied(to: nil))
        XCTAssertEqual(made["negs"], .number(1))
        XCTAssertEqual(made["defaultsVersion"], .number(1))
        XCTAssertEqual(made["variants"], .array([.object(["id": .string("v1"), "rating": .number(2)])]))
        XCTAssertNil(try patch(rating: 0).applied(to: made), "the last rating gone, the file goes")
    }

    /// An edit written after another app rated the photograph keeps the
    /// rating; a rating written over a file from another table version keeps
    /// its edits, which don't depend on it.
    func testEditsAndRatingsMergeWithTheFileAsItIsNow() throws {
        let rated = try json(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "v1", "rating": 4, "flag": "pick"}]}"#)
        let edits = SidecarPatch(variant: "v1", edits: try json(#"{"film": "kodak_gold_200", "paper": "kodak_portra_endura"}"#),
                                 known: [:], defaultsVersion: 1, app: "test")
        let both = try XCTUnwrap(edits.applied(to: rated))
        XCTAssertEqual(try PhotoSidecar(raw: both).summary, LibrarySummary(rating: 4, flag: .pick, edited: true))
        let removed = try XCTUnwrap(SidecarPatch(variant: "v1", edits: .some(nil), defaultsVersion: 1, app: "test").applied(to: both))
        XCTAssertEqual(try PhotoSidecar(raw: removed).summary, LibrarySummary(rating: 4, flag: .pick, edited: false), "edits removed, the rating stays")
    }

    func testANewPhotographsPatchesGoToTheFirstVariantAnotherAppMade() throws {
        let other = try json(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "abc", "edits": {"film": "kodak_gold_200"}}]}"#)
        let rated = try XCTUnwrap(patch(rating: 5, variant: "new00001", primary: true).applied(to: other))
        XCTAssertEqual(rated["variants"].array.count, 1)
        XCTAssertEqual(rated["variants"].array.first?["id"], .string("abc"))
        XCTAssertEqual(rated["variants"].array.first?["rating"], .number(5))
        let fresh = try XCTUnwrap(patch(rating: 1, variant: "new00001", primary: true).applied(to: nil))
        XCTAssertEqual(fresh["variants"].array.first?["id"], .string("new00001"))
    }

    func testARatingNeverWritesOverANewerFile() throws {
        let newer = try json(#"{"negs": 2, "defaultsVersion": 1, "variants": []}"#)
        XCTAssertThrowsError(try patch(rating: 3).applied(to: newer))
        let unknownTable = try json(#"{"negs": 1, "defaultsVersion": 99, "variants": []}"#)
        XCTAssertThrowsError(try patch(rating: 3).applied(to: unknownTable))
    }

    func testFilterRules() {
        let unrated = LibrarySummary(), three = LibrarySummary(rating: 3), fivePick = LibrarySummary(rating: 5, flag: .pick)
        let reject = LibrarySummary(rating: 4, flag: .reject), edited = LibrarySummary(edited: true)
        var filter = LibraryFilter()
        XCTAssertFalse(filter.active)
        XCTAssertTrue([unrated, three, fivePick, reject, edited].allSatisfy(filter.includes), "everything shows by default, rejects too")
        filter.rating = .atLeast(3)
        XCTAssertEqual([unrated, three, fivePick, reject, edited].map(filter.includes), [false, true, true, true, false])
        filter.rating = .atLeast(5)
        XCTAssertEqual([unrated, three, fivePick, reject, edited].map(filter.includes), [false, false, true, false, false])
        filter.rating = .unrated
        XCTAssertEqual([unrated, three, fivePick, reject, edited].map(filter.includes), [true, false, false, false, true])
        filter = LibraryFilter(flags: [.pick, .unflagged])
        XCTAssertEqual([unrated, three, fivePick, reject, edited].map(filter.includes), [true, true, true, false, true])
        filter = LibraryFilter(editedOnly: true)
        XCTAssertEqual([unrated, three, fivePick, reject, edited].map(filter.includes), [false, false, false, false, true])
    }

    func testTheFilterIsRememberedForTheApp() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "negs-library-tests-\(UUID().uuidString)"))
        XCTAssertEqual(LibraryFilter.load(defaults), LibraryFilter())
        let filter = LibraryFilter(rating: .atLeast(2), flags: [.reject], editedOnly: true)
        filter.save(defaults)
        XCTAssertEqual(LibraryFilter.load(defaults), filter)
        LibraryFilter().save(defaults)
        XCTAssertNil(defaults.object(forKey: LibraryFilter.defaultsKey))
    }

    func testTheListingFindsSidecarsButNotTheirTemporaryFiles() throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        for name in ["b.jpg", "a10.png", "a2.png", "notes.txt", "a2.png.negs", "B.JPG.NEGS", ".a10.png.negs.tmp", "orphan.cr3.negs"] {
            try Data("x".utf8).write(to: dir.appendingPathComponent(name))
        }
        let listing = try PhotoFolder.listing(in: dir)
        XCTAssertEqual(listing.photos.map(\.name), ["a2.png", "a10.png", "b.jpg"])
        XCTAssertEqual(Set(listing.sidecars.keys), ["a2.png", "B.JPG", "orphan.cr3"])
        XCTAssertEqual(try PhotoFolder.snapshot(in: dir).map(\.name), ["a2.png", "a10.png", "b.jpg"])
    }

    @MainActor func testRatingKeysAreIgnoredWhileTyping() {
        let window = EditorWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 200), styleMask: [.titled], backing: .buffered, defer: false)
        let field = NSTextField(frame: NSRect(x: 10, y: 10, width: 200, height: 24))
        window.contentView?.addSubview(field)
        XCTAssertTrue(MainMenu.takesRatingKeys(window))
        XCTAssertTrue(window.makeFirstResponder(field))
        XCTAssertTrue(window.firstResponder is NSText, "the field editor has the keys")
        XCTAssertFalse(MainMenu.takesRatingKeys(window))
        window.makeFirstResponder(nil)
        XCTAssertTrue(MainMenu.takesRatingKeys(window))
        XCTAssertFalse(MainMenu.takesRatingKeys(NSWindow()), "only the editing window's photograph is rated")
        XCTAssertFalse(MainMenu.takesRatingKeys(nil))
    }
}

/// The folder as the library: listing, reading sidecars, outside changes.
@MainActor final class PhotoLibraryTests: XCTestCase {
    private func waitUntil(_ message: String = "", _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(20)
        while !predicate() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), message)
    }

    private func library() throws -> PhotoLibrary {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "negs-library-tests-\(UUID().uuidString)"))
        return PhotoLibrary(store: SidecarStore(), defaults: defaults)
    }

    func testRatingsArriveAfterTheListingAndFollowOutsideChanges() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let a = dir.appendingPathComponent("a.png"), b = dir.appendingPathComponent("b.png"), c = dir.appendingPathComponent("c.png")
        for photo in [a, b, c] { try Data("x".utf8).write(to: photo) }
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "1", "rating": 4, "flag": "pick"}]}"#, sidecarOf: a)
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "2", "flag": "reject", "edits": {"film": "kodak_gold_200"}}]}"#, sidecarOf: b)
        try write(#"{"negs": 9}"#, sidecarOf: c)
        let library = try library()
        var changed: [URL] = []
        library.sidecarChanged = { changed.append($0) }
        let watching = Task { await library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { library.summariesLoaded && !library.loading }
        XCTAssertEqual(library.files.map(\.name), ["a.png", "b.png", "c.png"])
        XCTAssertEqual(library.summary(for: a), LibrarySummary(rating: 4, flag: .pick))
        XCTAssertEqual(library.summary(for: b), LibrarySummary(flag: .reject, edited: true))
        XCTAssertNotNil(library.summary(for: c).problem, "a newer file is reported, not read")
        XCTAssertFalse(library.readOnly)
        XCTAssertTrue(changed.isEmpty, "the first read is no change")

        // Another app rates b and clears a.
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "2", "rating": 2, "flag": "reject", "edits": {"film": "kodak_gold_200"}}]}"#, sidecarOf: b)
        try FileManager.default.removeItem(at: PhotoSidecar.url(for: a))
        try await waitUntil("the watcher sees sidecar writes") { library.summary(for: b).rating == 2 && library.summary(for: a) == LibrarySummary() }
        XCTAssertEqual(Set(changed), [a, b])

        // Listing again on request (window became key) reads nothing unchanged.
        changed = []
        library.refreshNow()
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertTrue(changed.isEmpty)
    }

    func testAFilterHidesPhotographsButNeverTheCurrentOne() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photos = ["a.png", "b.png", "c.png"].map { dir.appendingPathComponent($0) }
        for photo in photos { try Data("x".utf8).write(to: photo) }
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "1", "flag": "pick"}]}"#, sidecarOf: photos[1])
        let library = try library()
        library.filter = LibraryFilter(flags: [.pick])
        let watching = Task { await library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { !library.filtering }
        XCTAssertEqual(library.shown(current: nil).map(\.name), ["b.png"])
        XCTAssertEqual(library.shown(current: photos[0]).map(\.name), ["a.png", "b.png"], "the photograph under the cursor stays")
        // Rejecting the pick it's on keeps it until another is opened.
        library.set(LibrarySummary(flag: .reject), for: photos[1])
        XCTAssertEqual(library.shown(current: photos[1]).map(\.name), ["b.png"])
        XCTAssertEqual(library.shown(current: photos[2]).map(\.name), ["c.png"])
    }

    func testAReadOnlyFolderIsReported() async throws {
        let dir = try folder()
        try Data("x".utf8).write(to: dir.appendingPathComponent("a.png"))
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: dir.path)
        defer {
            try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path)
            try? FileManager.default.removeItem(at: dir)
        }
        let library = try library()
        let watching = Task { await library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { library.summariesLoaded }
        XCTAssertTrue(library.readOnly)
        XCTAssertFalse(PhotoLibrary.canWrite(beside: dir.appendingPathComponent("a.png")))
    }
}

/// Rating and flagging the open photograph.
@MainActor final class LibraryModelTests: XCTestCase {
    private var paths: EnginePaths {
        EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                    exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: data)
    }

    private func fixture(in dir: URL, _ name: String) throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        for i in 0..<(96 * 64 * 3) { bitmap.bitmapData![i] = UInt8(i % 251) }
        let url = dir.appendingPathComponent(name)
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate())
    }

    private func model(store: SidecarStore) async throws -> EditorModel {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "sidecars", "advanceAfterRating", LibraryFilter.defaultsKey].map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(false, forKey: "autoPlaceNewImages")
        defaults.set(true, forKey: "sidecars")
        defaults.removeObject(forKey: "advanceAfterRating")
        defaults.removeObject(forKey: LibraryFilter.defaultsKey)
        let model = EditorModel(sidecars: store)
        addTeardownBlock { @MainActor in model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        return model
    }

    func testRatingsAndFlagsAreSavedBesideThePhotograph() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png"), other = try fixture(in: dir, "other.png")
        let store = SidecarStore()
        let model = try await model(store: store)
        model.open(photo)
        model.setRating(3)
        XCTAssertEqual(model.libraryNote?.text, "★★★")
        XCTAssertNil(model.libraryNote?.detail)
        model.setFlag(.reject)
        XCTAssertEqual(model.libraryNote?.text, "Rejected")
        model.changeRating(by: 1)
        await store.flush()
        let saved = try XCTUnwrap(try store.read(photo: photo))
        XCTAssertEqual(saved.summary, LibrarySummary(rating: 4, flag: .reject))
        XCTAssertNil(saved.variants.first?.edits, "rating isn't editing")
        // Edits join the same variant; the rating stays.
        model.set("prep.lift", .number(7))
        model.open(other)
        await store.flush()
        let both = try XCTUnwrap(try store.read(photo: photo))
        XCTAssertEqual(both.variants.count, 1)
        XCTAssertEqual(both.summary, LibrarySummary(rating: 4, flag: .reject, edited: true))
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: other).path))

        // Another launch reads them back when the photograph opens.
        let relaunched = try await self.model(store: store)
        relaunched.open(photo)
        XCTAssertEqual(relaunched.currentSummary, LibrarySummary(rating: 4, flag: .reject, edited: true))
        XCTAssertEqual(relaunched.value("prep.lift"), .number(7))
        relaunched.changeRating(by: 5)
        XCTAssertEqual(relaunched.currentSummary?.rating, 5, "at most five stars")
        relaunched.setRating(0); relaunched.setFlag(nil)
        XCTAssertEqual(relaunched.libraryNote?.text, "Unflagged")
        await relaunched.flushSidecars()
        XCTAssertEqual(try store.read(photo: photo)?.summary, LibrarySummary(edited: true))
        // A flag alone makes a file, and goes with it.
        relaunched.open(other)
        relaunched.setFlag(.pick)
        await relaunched.flushSidecars()
        XCTAssertEqual(try store.read(photo: other)?.summary, LibrarySummary(flag: .pick))
        relaunched.setFlag(nil)
        await relaunched.flushSidecars()
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: other).path), "nothing left, no file")
    }

    func testAReadOnlyFolderKeepsRatingsForTheSessionAndSaysSo() async throws {
        let dir = try folder()
        let photo = try fixture(in: dir, "photo.png"), other = try fixture(in: dir, "other.png")
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: dir.path)
        defer {
            try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path)
            try? FileManager.default.removeItem(at: dir)
        }
        let store = SidecarStore()
        let model = try await model(store: store)
        model.open(photo)
        model.setRating(2)
        XCTAssertEqual(model.libraryNote?.detail, "Not saved: the folder is read-only")
        model.open(other)
        model.open(photo)
        XCTAssertEqual(model.currentSummary?.rating, 2, "kept for this session")
        await model.flushSidecars()
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: photo).path))
    }

    /// Another app or Mac changed the open photograph's sidecar: with no
    /// unsaved edits the newer file wins; with some, ours are written over it.
    func testOutsideChangesToTheOpenPhotograph() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png")
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "d1", "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 5}}}}]}"#, sidecarOf: photo)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        model.open(photo)
        XCTAssertEqual(model.value("prep.lift"), .number(5))
        // Our own edit, written: the file read back is ours, nothing reloads.
        undo.beginUndoGrouping(); model.set("prep.lift", .number(6)); undo.endUndoGrouping()
        await model.flushSidecars()
        model.sidecarChangedOnDisk(photo)
        XCTAssertEqual(model.value("prep.lift"), .number(6))
        XCTAssertTrue(undo.canUndo)

        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "d1", "rating": 1, "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 12}}}}]}"#, sidecarOf: photo)
        model.sidecarChangedOnDisk(photo)
        XCTAssertEqual(model.value("prep.lift"), .number(12), "no unsaved edits: the newer file wins")
        XCTAssertFalse(undo.canUndo, "its history is of edits that are gone")

        // Unsaved edits win, and are written over the newer file.
        undo.beginUndoGrouping(); model.set("prep.lift", .number(20)); undo.endUndoGrouping()
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "d1", "rating": 3, "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 30}}}}]}"#, sidecarOf: photo)
        model.sidecarChangedOnDisk(photo)
        XCTAssertEqual(model.value("prep.lift"), .number(20))
        await model.flushSidecars()
        let saved = try XCTUnwrap(try store.read(photo: photo)?.variants.first)
        XCTAssertEqual(saved.edits?["params.prep.lift"], .number(20))
        XCTAssertEqual(saved.rating, 3, "the newer rating stays")
    }

    func testAdvanceAfterRatingFollowsTheFilteredStrip() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let a = try fixture(in: dir, "a.png"), b = try fixture(in: dir, "b.png"), c = try fixture(in: dir, "c.png")
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "1", "rating": 1}]}"#, sidecarOf: b)
        let store = SidecarStore()
        let model = try await model(store: store)
        model.open(a)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.files.count == 3 }
        model.step(by: 1)
        XCTAssertEqual(model.source, b)
        model.step(by: -1)
        XCTAssertEqual(model.source, a)
        model.advanceAfterRating = true
        model.library.filter = LibraryFilter(rating: .unrated)
        model.setRating(2)
        XCTAssertEqual(model.source, c, "b is rated, so the unrated strip skips it")
        XCTAssertEqual(model.library.shown(current: c).map(\.name), ["c.png"], "a, rated now, is gone once left")
        model.step(by: -1)
        XCTAssertEqual(model.source, c)
        await model.flushSidecars()
        XCTAssertEqual(try store.read(photo: a)?.summary.rating, 2)
    }

    /// In the grid clicks and keys move the current photograph without
    /// opening it, ratings apply to the whole selection, and leaving opens
    /// the current one. In the edit view ratings stay on the open photograph.
    func testTheGridSelectsWithoutDevelopingAndRatesTheSelection() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let a = try fixture(in: dir, "a.png"), b = try fixture(in: dir, "b.png"), c = try fixture(in: dir, "c.png")
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "b1", "rating": 1}]}"#, sidecarOf: b)
        let store = SidecarStore()
        let model = try await model(store: store)
        model.open(a)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.files.count == 3 }

        model.showGrid()
        XCTAssertEqual(model.mode, .grid)
        model.click(b)
        XCTAssertEqual(model.selection.current, b)
        XCTAssertEqual(model.source, a, "nothing develops in the grid")
        model.click(c, shift: true)
        XCTAssertEqual(model.ratingTargets, [b, c])
        model.setRating(3)
        XCTAssertEqual(model.libraryNote?.text, "★★★")
        XCTAssertEqual(model.libraryNote?.count, 2)
        model.changeRating(by: 1)
        XCTAssertEqual(model.libraryNote?.text, "Rating Raised")
        await store.flush()
        XCTAssertEqual(try store.read(photo: b)?.variants.map(\.id), ["b1"], "its own variant")
        XCTAssertEqual(try store.read(photo: b)?.summary.rating, 4)
        XCTAssertEqual(try store.read(photo: c)?.summary.rating, 4, "a new file")
        XCTAssertNil(try store.read(photo: a), "the open photograph wasn't selected")

        // Rated out of the filter, they stay until the selection moves.
        model.library.filter = LibraryFilter(rating: .atLeast(4))
        XCTAssertEqual(model.selection.selected, [b, c])
        model.setRating(0)
        XCTAssertEqual(model.shownPhotos.map(\.url), [b, c])
        model.click(b)
        XCTAssertEqual(model.shownPhotos.map(\.url), [b])
        model.library.filter = LibraryFilter()

        // Arrows: a row is two columns here; down onto a short row lands on its end.
        model.gridColumns = 2
        model.click(a)
        model.move(by: 2, extending: false)
        XCTAssertEqual(model.selection.current, c)
        model.move(by: -2, extending: false)
        XCTAssertEqual(model.selection.current, a)
        model.move(by: 1, extending: false)
        model.move(by: 2, extending: false)
        XCTAssertEqual(model.selection.current, c)
        model.move(by: -1, extending: true)
        XCTAssertEqual(model.selection.selected, [b, c])
        XCTAssertEqual(model.source, a)
        model.selectAll()
        XCTAssertEqual(model.ratingTargets, [a, b, c])
        model.deselectAll()
        XCTAssertEqual(model.ratingTargets, [c])

        // Leaving opens the current photograph and keeps the selection.
        model.click(b, command: true)
        model.editCurrent()
        XCTAssertEqual(model.mode, .photo)
        XCTAssertEqual(model.source, c)
        XCTAssertEqual(model.selection.selected, [b, c])
        // In the edit view a rating is the open photograph's alone.
        model.setRating(5)
        XCTAssertEqual(model.libraryNote?.count, 1)
        XCTAssertEqual(model.library.summary(for: c).rating, 5)
        XCTAssertEqual(model.library.summary(for: b).rating, 0)
        // Opening another photograph selects it alone; ⌘ and ⇧ never open.
        model.click(a, command: true)
        XCTAssertEqual(model.source, c)
        model.click(a)
        XCTAssertEqual(model.source, a)
        XCTAssertEqual(model.selection.selected, [a])
        await model.flushSidecars()
    }

    /// Paste, Reset and Duplicate change several photographs' sidecars as
    /// one undo step each, which survives opening another photograph.
    func testPasteResetAndDuplicateAreOneUndoStepEach() async throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let a = try fixture(in: dir, "a.png"), b = try fixture(in: dir, "b.png"), c = try fixture(in: dir, "c.png")
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "b1", "rating": 2, "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 5}}}}]}"#, sidecarOf: b)
        let store = SidecarStore()
        let model = try await model(store: store)
        let strip = UserDefaults.standard.object(forKey: "filmstripVisible")
        addTeardownBlock { UserDefaults.standard.set(strip, forKey: "filmstripVisible") }
        model.filmstripVisible = true
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        func step(_ body: () -> Void) { undo.beginUndoGrouping(); body(); undo.endUndoGrouping() }
        func variants(_ photo: URL) throws -> [PhotoSidecar.Variant] { try store.read(photo: photo)?.variants ?? [] }
        model.open(a)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.files.count == 3 }

        step { model.set("prep.lift", .number(30)) }
        model.copyEdits()
        XCTAssertEqual(model.copiedEdits?.state.params["prep.lift"], .number(30))
        XCTAssertEqual(model.copiedEdits?.groups, EditGroup.standard)
        model.showGrid()
        model.click(b); model.click(c, shift: true)
        step { model.pasteEdits() }
        XCTAssertEqual(model.libraryNote?.count, 2)
        await model.flushSidecars()
        XCTAssertEqual(try variants(b).map(\.id), ["b1"])
        XCTAssertEqual(try variants(b).first?.rating, 2, "the rating stays")
        XCTAssertEqual(try variants(b).first?.edits?["params.prep.lift"], .number(30))
        XCTAssertEqual(try variants(b).first?.edits?["film"], .string(EditState().film), "film and paper are copied")
        XCTAssertEqual(try variants(c).first?.edits?["params.prep.lift"], .number(30))
        XCTAssertTrue(model.library.summary(for: c).edited)

        // One undo puts both back, the photograph without a file without one.
        undo.undo()
        await model.flushSidecars()
        XCTAssertEqual(try variants(b).first?.edits?["params.prep.lift"], .number(5))
        XCTAssertEqual(try variants(b).first?.edits?["film"], .string("kodak_portra_400"))
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: c).path))
        XCTAssertFalse(model.library.summary(for: c).edited)
        undo.redo()
        await model.flushSidecars()
        XCTAssertEqual(try variants(c).first?.edits?["params.prep.lift"], .number(30))

        // Reset removes the edits and keeps the rating.
        step { model.resetEdits(of: [b]) }
        await model.flushSidecars()
        XCTAssertEqual(try variants(b).first?.rating, 2)
        XCTAssertNil(try variants(b).first?.edits)
        XCTAssertEqual(model.library.summary(for: b), LibrarySummary(rating: 2))

        // A duplicate is a second variant with the same edits, rating and flag, listed and selected after its photograph.
        model.setFlag(.pick, targets: [c])
        step { model.duplicate([c]) }
        await model.flushSidecars()
        let copy = try XCTUnwrap(model.selection.current)
        XCTAssertEqual(copy.photoFile, c)
        XCTAssertEqual(copy.variantID, try variants(c).last?.id)
        XCTAssertEqual(try variants(c).count, 2)
        XCTAssertEqual(try variants(c)[1].edits, try variants(c)[0].edits)
        XCTAssertEqual(try variants(c).map(\.flag), [.pick, .pick])
        XCTAssertEqual(model.library.items.map(\.name), ["a.png", "b.png", "c.png", "c.png (2)"])
        model.setRating(4)
        await model.flushSidecars()
        XCTAssertEqual(try variants(c).map(\.rating), [0, 4], "only the duplicate is rated")

        // Opening it develops c with the duplicate's own edits.
        model.editCurrent()
        XCTAssertEqual(model.source, c)
        XCTAssertEqual(model.sourceItem, copy)
        XCTAssertEqual(model.value("prep.lift"), .number(30))
        step { model.set("prep.lift", .number(-10)) }
        model.open(a)
        await model.flushSidecars()
        XCTAssertEqual(try variants(c).map { $0.edits?["params.prep.lift"] }, [.number(30), .number(-10)])
        XCTAssertEqual(model.previousEdits?.params["prep.lift"], .number(-10), "the last photograph left with edits")

        // Paste from Previous onto the open photograph, then undo it and the duplicate.
        step { model.pasteFromPrevious() }
        XCTAssertEqual(model.value("prep.lift"), .number(-10))
        undo.undo()
        XCTAssertEqual(model.value("prep.lift"), .number(30))
        undo.undo()
        await model.flushSidecars()
        XCTAssertEqual(try variants(c).count, 1, "the duplicate is gone")
        XCTAssertEqual(model.library.items.map(\.name), ["a.png", "b.png", "c.png"])

        // Resetting the open photograph keeps its film and removes its stored edits.
        step { model.set("film", .string("kodak_portra_400")) }
        step { model.resetEdits(of: [a]) }
        await model.flushSidecars()
        XCTAssertEqual(model.value("prep.lift"), .number(0))
        XCTAssertNil(try variants(a).first?.edits)
    }

    /// Delete moves the photograph and its sidecar to the Trash, a duplicate
    /// leaves its sidecar, and one undo step puts either back.
    func testDeleteMovesToTheTrashAndUndoPutsItBack() async throws {
        let dir = try folder(), bin = try folder()
        defer { try? FileManager.default.removeItem(at: dir); try? FileManager.default.removeItem(at: bin) }
        let a = try fixture(in: dir, "a.png"), b = try fixture(in: dir, "b.png"), c = try fixture(in: dir, "c.png"), d = try fixture(in: dir, "d.png")
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "b1", "rating": 3}, {"id": "b2", "rating": 1}]}"#, sidecarOf: b)
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "c1", "flag": "reject"}, {"id": "c2", "rating": 2}]}"#, sidecarOf: c)
        let store = SidecarStore()
        let model = try await model(store: store)
        // Never the user's own Trash.
        model.trash = PhotoTrash(available: { _ in true }, trash: { file in
            let landed = bin.appendingPathComponent(file.lastPathComponent)
            try FileManager.default.moveItem(at: file, to: landed)
            return landed
        }, remove: { try FileManager.default.removeItem(at: $0) })
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        func step(_ body: () -> Void) { undo.beginUndoGrouping(); body(); undo.endUndoGrouping() }
        func exists(_ url: URL) -> Bool { FileManager.default.fileExists(atPath: url.path) }
        func variants(_ photo: URL) throws -> [PhotoSidecar.Variant] { try store.read(photo: photo)?.variants ?? [] }
        model.open(b)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.items.count == 6 }

        // In the edit view it's the open photograph alone, with its duplicate; the next one opens.
        XCTAssertEqual(model.deleteTargets, [b])
        let plan = model.deletePlan(model.deleteTargets)
        XCTAssertEqual(plan.following, 1)
        step { model.delete(plan) }
        XCTAssertEqual(model.source, c)
        XCTAssertEqual(model.library.items.map(\.name), ["a.png", "c.png", "c.png (2)", "d.png"], "gone at once")
        try await waitUntil { !exists(b) && exists(bin.appendingPathComponent("b.png")) && exists(bin.appendingPathComponent("b.png.negs")) }
        XCTAssertEqual(undo.undoActionName, "Move to Trash")

        undo.undo()
        try await waitUntil { exists(b) && model.source == b && model.library.items.count == 6 }
        XCTAssertEqual(try variants(b).map(\.rating), [3, 1], "its sidecar is back with it")
        XCTAssertFalse(exists(bin.appendingPathComponent("b.png")))
        undo.redo()
        try await waitUntil { !exists(b) && model.library.items.count == 4 }
        undo.undo()
        try await waitUntil { exists(b) && model.library.items.count == 6 }

        // A duplicate alone leaves its sidecar; the photograph stays.
        model.showGrid()
        model.click(c.variant("c2"))
        step { model.requestDelete(asking: false) }
        await model.flushSidecars()
        XCTAssertEqual(try variants(c).map(\.id), ["c1"])
        XCTAssertTrue(exists(c))
        XCTAssertEqual(model.selection.current, d, "the next photograph is current")
        XCTAssertEqual(undo.undoActionName, "Remove Duplicate")
        undo.undo()
        try await waitUntil { model.selection.selected == [c.variant("c2")] }
        await model.flushSidecars()
        XCTAssertEqual(try variants(c).map(\.id), ["c1", "c2"])

        // Delete Rejected Photographs: c, and its duplicate goes with it.
        XCTAssertEqual(model.rejected, [c])
        step { model.delete(model.deletePlan(model.rejected)) }
        try await waitUntil { !exists(c) && !exists(PhotoSidecar.url(for: c)) }
        XCTAssertEqual(model.library.items.map(\.name), ["a.png", "b.png", "b.png (2)", "d.png"])

        // With everything gone there's no photograph; undo opens one again.
        model.selectAll()
        step { model.delete(model.deletePlan(model.deleteTargets)) }
        XCTAssertNil(model.source)
        XCTAssertEqual(model.mode, .photo)
        try await waitUntil { [a, b, d].allSatisfy { !exists($0) } }
        undo.undo()
        try await waitUntil { [a, b, d].allSatisfy(exists) && model.source != nil }
        await model.flushSidecars()
    }

    /// Edited photographs get thumbnails developed with their edits: the
    /// open one from its preview on leaving it, the others in a background
    /// helper, which also solves a pasted match and stores it.
    func testEditedThumbnailsOnLeavingAndInTheBackground() async throws {
        let dir = try folder(), thumbnails = try folder()
        defer { try? FileManager.default.removeItem(at: dir); try? FileManager.default.removeItem(at: thumbnails) }
        let a = try fixture(in: dir, "a.png"), b = try fixture(in: dir, "b.png"), c = try fixture(in: dir, "c.png")
        // b as a paste leaves it: the match on, not solved for these edits.
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "b1", "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 5}}, "brightnessMatch": {"highlightLimited": false}}}]}"#, sidecarOf: b)
        let store = SidecarStore()
        let model = try await model(store: store)
        let edited = EditedThumbnailStore(directory: thumbnails)
        model.editedThumbnails = edited
        func file(_ url: URL) -> PhotoFile? { model.library.items.first { $0.url == url } }
        func made(_ url: URL) -> Bool {
            guard let file = file(url), let look = model.library.looks[url], let key = edited.key(for: file, look: look) else { return false }
            return edited.contains(key)
        }
        model.open(a)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.files.count == 3 && model.previewCurrent }
        XCTAssertNil(model.library.looks[a], "unedited: the embedded preview")
        XCTAssertNotNil(model.library.looks[b])

        // b in the background, its match solved and stored first.
        try await waitUntil { made(b) }
        let match = try store.read(photo: b)?.variants.first?.edits?["brightnessMatch"]
        XCTAssertEqual(match?["ev"].double.isFinite, true, "solved: \(String(describing: match))")
        XCTAssertEqual(match?["solved"], .null, "stored as solved")
        XCTAssertEqual(try store.read(photo: b)?.variants.first?.edits?["params.prep.lift"], .number(5), "the edits stay")
        let key = try XCTUnwrap(file(b).flatMap { edited.key(for: $0, look: model.library.looks[b]!) })
        let image = try XCTUnwrap(edited.image(key, maxPixels: 1024))
        XCTAssertEqual(max(image.width, image.height), 512)
        XCTAssertNotNil(model.editedThumbnail(for: b))

        // a from its preview, once edited and left.
        model.set("prep.lift", .number(20))
        try await waitUntil { model.previewCurrent }
        model.open(c)
        try await waitUntil { made(a) }
        XCTAssertGreaterThan(model.thumbnailVersions[a] ?? 0, 0)
        await model.flushSidecars()
    }

    func testBatchExportsEachPhotographWithItsOwnEdits() async throws {
        let dir = try folder(), out = try folder(), alone = try folder()
        defer { for url in [dir, out, alone] { try? FileManager.default.removeItem(at: url) } }
        let a = try fixture(in: dir, "a.png"), b = try fixture(in: dir, "b.png"), c = try fixture(in: dir, "c.png")
        // b edited and rated, its match on but not solved for these edits; c never edited.
        try write(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "b1", "rating": 4, "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 5}}, "brightnessMatch": {"highlightLimited": false}}}]}"#, sidecarOf: b)
        let store = SidecarStore()
        let model = try await model(store: store)
        let journal = FileManager.default.temporaryDirectory.appendingPathComponent("negs-batch-\(UUID().uuidString)/active-export.json")
        defer { try? FileManager.default.removeItem(at: journal.deletingLastPathComponent()) }
        model.batchJournalURL = journal
        model.autoPlaceNewImages = true
        model.open(a)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.files.count == 3 && model.previewCurrent && !model.brightnessMatchPending }
        // a: another film than the default, and 3 stars.
        let film = try XCTUnwrap(model.films.first { $0.id != model.state.film && !$0.positive }?.id)
        model.chooseFilm(film)
        try await waitUntil { model.previewCurrent && !model.brightnessMatchPending && !model.matchingBrightness }
        model.setRating(3)

        model.showGrid()
        model.selectAll()
        XCTAssertEqual(model.exportTitle, "Export 3 Photographs…")
        XCTAssertTrue(model.canExport)
        var options = BatchOptions()
        options.format = OutputFormat.png.rawValue
        let started = await model.exportBatch(model.exportTargets, to: out, options: options)
        let run = try XCTUnwrap(started)
        XCTAssertEqual(run.items.map(\.fileName), ["a_\(film).png", "b_kodak_portra_400.png", "c_\(film).png"], "c follows the open photograph's film")
        XCTAssertEqual(run.items.map(\.needsMatch), [false, true, true])
        XCTAssertTrue(model.batchRunning)
        XCTAssertFalse(model.canExport, "one batch at a time")
        try await waitUntil { FileManager.default.fileExists(atPath: journal.path) }
        await run.task?.value
        XCTAssertEqual(run.items.map(\.status), [.done, .done, .done], "\(run.items.map { $0.reason ?? "" })")
        XCTAssertFalse(FileManager.default.fileExists(atPath: journal.path), "the journal goes when the batch ends")
        XCTAssertNil(model.batch, "a batch without failures is forgotten")

        let files = ["a_\(film).png", "b_kodak_portra_400.png", "c_\(film).png"].map { out.appendingPathComponent($0) }
        XCTAssertEqual(files.map(ExportRating.read), [3, 4, nil])
        // b's match solved and stored with its edits; c gets no sidecar.
        let match = try store.read(photo: b)?.variants.first?.edits?["brightnessMatch"]
        XCTAssertEqual(match?["ev"].double.isFinite, true, "solved: \(String(describing: match))")
        XCTAssertEqual(try store.read(photo: b)?.variants.first?.edits?["params.prep.lift"], .number(5))
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: c).path))
        // Each exported alone, as today, gives the same bytes: a open as it
        // was, and c opened after it, so with a's film and its match solved.
        // (Not c against a: a kept an exposure solved before its film
        // changed, as a change under 0.05 EV does.)
        for (photo, file) in [(a, files[0]), (c, files[2])] {
            model.open(photo)
            try await waitUntil { model.previewCurrent && !model.brightnessMatchPending && !model.matchingBrightness }
            XCTAssertEqual(model.state.film, film)
            let single = alone.appendingPathComponent(file.lastPathComponent)
            await model.export(to: single, engine: .reference, size: options.size, save: options.save)
            XCTAssertEqual(try Data(contentsOf: single), try Data(contentsOf: file), file.lastPathComponent)
        }

        // Again into the same folder: Add a Number, then Skip.
        model.showGrid()
        model.selectAll()
        let numbered = await model.exportBatch([b], to: out, options: options)
        let again = try XCTUnwrap(numbered)
        await again.task?.value
        XCTAssertEqual(again.items.first?.written?.lastPathComponent, "b_kodak_portra_400 2.png")
        options.existing = .skip
        let skipping = await model.exportBatch([b], to: out, options: options)
        let skipped = try XCTUnwrap(skipping)
        await skipped.task?.value
        XCTAssertEqual(skipped.items.first?.status, .skipped)
        await model.flushSidecars()
    }

    func testBatchJournalRoundTrip() throws {
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("j/active-export.json")
        var state = EditState(params: .object(["prep": .object(["lift": .number(5)])]))
        state.brightnessMatch = BrightnessMatch()
        let photo = dir.appendingPathComponent("IMG.CR3")
        let journal = BatchJournal(destination: dir, bookmark: try dir.bookmarkData(), options: BatchOptions(),
                                   items: [BatchItem(item: photo.variant("v2"), title: "IMG.CR3 (2)", state: state, needsMatch: true, storesMatch: true,
                                                     rating: -1, fileName: "IMG (2)_kodak_gold_200.jpeg", size: CGSize(width: 6000, height: 4000), look: "abc")])
        try journal.write(to: url)
        XCTAssertEqual(BatchJournal.read(from: url), journal)
        XCTAssertEqual(journal.remaining, 1)
        XCTAssertEqual(EditorModel.batchName(photo.variant("v2"), title: "IMG.CR3 (2)"), "IMG (2)")
        BatchJournal.remove(at: url)
        XCTAssertNil(BatchJournal.read(from: url))
    }

}
