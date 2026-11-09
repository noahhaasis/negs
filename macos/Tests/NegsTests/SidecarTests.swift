import AppKit
import CryptoKit
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
private let data = root.appendingPathComponent("data")

private func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-sidecars-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func json(_ text: String) throws -> JSONValue { try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) }

/// The session the next launch reads: the photograph's edits written to its
/// sidecar, then read back with the session, as `SessionStore.read` does.
@MainActor func relaunchSession(of model: EditorModel, store: SidecarStore, data: URL) async -> EditorSession? {
    guard var session = model.session else { return nil }
    await model.flushSidecars()
    session.sidecar = SidecarRestore.read(photo: session.source, store: store, data: data)
    return session
}

/// The format itself: diffs, the frozen tables, unknown keys, writes.
final class SidecarFormatTests: XCTestCase {
    private var table: SidecarDefaults { get throws { try SidecarDefaults.load(version: 1, data: data) } }

    /// Every released table, by hash. A changed default is a new table; an
    /// old one never changes, or photos saved against it would change.
    func testDefaultsTablesAreFrozen() throws {
        let frozen = [1: "d1db8208a62b2b340884911d669ddb453ac7fae8ed59d4ddfec24c70bf81380d"]
        let files = try FileManager.default.contentsOfDirectory(atPath: data.appendingPathComponent("sidecar-defaults").path).filter { $0.hasSuffix(".json") }
        XCTAssertEqual(files.count, SidecarDefaults.latest, "one table per version, and the newest is `SidecarDefaults.latest`")
        for version in 1...SidecarDefaults.latest {
            let bytes = try Data(contentsOf: SidecarDefaults.url(version: version, data: data))
            let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
            XCTAssertEqual(hash, frozen[version], "data/sidecar-defaults/v\(version).json changed. Released tables are frozen: write the next version with scripts/sidecar_defaults.py instead.")
            XCTAssertEqual(try SidecarDefaults.load(version: version, data: data).version, version)
        }
    }

    /// What a new photograph starts with besides the engine's parameters is
    /// the newest table's; the engine's own are checked with the helper
    /// (`SidecarModelTests`).
    func testLatestTableMatchesTheAppsOwnDefaults() throws {
        let table = try SidecarDefaults.load(version: SidecarDefaults.latest, data: data)
        XCTAssertEqual(table.film, EditState().film)
        XCTAssertEqual(table.paper, EditState().paper)
        XCTAssertEqual(table.border, try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(BorderSettings())))
        XCTAssertEqual(table.crop, try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(CropSettings())))
        XCTAssertEqual(table.params[EditorModel.lensCorrectionPath], .bool(true))
    }

    func testChangesKeepOnlyWhatDiffersAndReadBackExactly() throws {
        let base = try json(#"{"a": {"x": 0.10000000149011612, "y": [1, 2, 3], "z": null}, "b": true, "c": "auto"}"#)
        XCTAssertNil(base.changes(from: base))
        var edited = base
        edited["a.x"] = .number(0.1)            // the slider's decimal beside a widened f32 default: kept
        edited["a.y"] = .array([.number(1), .number(2), .number(4)])
        edited["a.z"] = .array([.number(0.5)])
        edited["d"] = .number(7)                // a key the base lacks: kept
        let changes = try XCTUnwrap(edited.changes(from: base))
        XCTAssertEqual(changes, try json(#"{"a": {"x": 0.1, "y": [1, 2, 4], "z": [0.5]}, "d": 7}"#))
        XCTAssertEqual(base.overlaid(with: changes), edited)
        // A null set over a value is a change, and reads back as null.
        var cleared = base; cleared["c"] = .null
        XCTAssertEqual(base.overlaid(with: try XCTUnwrap(cleared.changes(from: base))), cleared)
    }

    func testEditsRoundTripThroughTheStoredForm() throws {
        let table = try table
        var state = EditState()
        state.film = "kodak_portra_400"
        state.params = table.params
        state.params["prep.highlights"] = .number(-40)
        state.params["io.rotate_cw"] = .number(3)
        state.params["film_render.grain.rms_granularity"] = .array([.number(12), .number(13), .number(12)])
        state.crop.rect = CGRect(x: 0.1, y: 0.2, width: 0.7, height: 0.6)
        state.crop.angle = 2.5
        state.crop.aspect = .threeTwo
        state.border = BorderSettings(enabled: true, style: .keyline, export: false)
        state.brightnessMatch = BrightnessMatch(ev: 0.42, inputs: .object([:]), highlightLimited: true)
        let stored = PhotoSidecar.encodeEdits(state, matchSolved: true, table: table)
        XCTAssertEqual(stored["params"], try json(#"{"prep": {"highlights": -40}, "io": {"rotate_cw": 3}, "film_render": {"grain": {"rms_granularity": [12, 13, 12]}}}"#))
        XCTAssertEqual(stored["brightnessMatch"], try json(#"{"ev": 0.42, "highlightLimited": true}"#), "the match's inputs are the edits; they aren't stored")
        XCTAssertEqual(stored["crop"]["angle"], .number(2.5))
        XCTAssertNil(stored.object?["crop"]?.object?["custom"], "only what differs from the table")
        let read = try PhotoSidecar.decodeEdits(stored, table: table)
        var expected = state; expected.brightnessMatch?.inputs = nil
        XCTAssertEqual(read.state, expected)
        XCTAssertTrue(read.matchSolved)
        // A match still waiting for its solve is solved again on opening.
        let stale = PhotoSidecar.encodeEdits(state, matchSolved: false, table: table)
        XCTAssertEqual(stale["brightnessMatch"]["solved"], .bool(false))
        XCTAssertFalse(try PhotoSidecar.decodeEdits(stale, table: table).matchSolved)
        // A new photograph's edits store only the film and paper.
        var plain = EditState(); plain.params = table.params
        XCTAssertEqual(PhotoSidecar.encodeEdits(plain, matchSolved: false, table: table), try json(#"{"film": "kodak_gold_200", "paper": "kodak_portra_endura"}"#))
    }

    func testDamagedNewerAndOversizedSidecarsAreRefused() throws {
        XCTAssertThrowsError(try PhotoSidecar(data: Data("not json".utf8)))
        XCTAssertThrowsError(try PhotoSidecar(data: Data(#"{"negs": 2, "defaultsVersion": 1, "variants": []}"#.utf8))) { XCTAssertEqual($0 as? PhotoSidecar.Problem, .newer) }
        XCTAssertThrowsError(try PhotoSidecar(data: Data(#"{"negs": 1, "defaultsVersion": 99, "variants": []}"#.utf8))) { XCTAssertEqual($0 as? PhotoSidecar.Problem, .newer) }
        XCTAssertThrowsError(try PhotoSidecar(data: Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "rating": 9}]}"#.utf8)))
        XCTAssertThrowsError(try PhotoSidecar(data: Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a"}, {"id": "a"}]}"#.utf8)))
        XCTAssertThrowsError(try PhotoSidecar(data: Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "flag": "maybe"}]}"#.utf8)))
        XCTAssertThrowsError(try PhotoSidecar(data: Data(count: PhotoSidecar.maximumSize + 1))) { XCTAssertEqual($0 as? PhotoSidecar.Problem, .tooLarge) }
        let table = try table
        XCTAssertThrowsError(try PhotoSidecar.decodeEdits(try json(#"{"crop": {"rect": [[0.5, 0], [0.9, 1]]}}"#), table: table), "a crop outside the photograph")
        XCTAssertThrowsError(try PhotoSidecar.decodeEdits(try json(#"{"params": {"io": {"rotate_cw": 1.5}}}"#), table: table))
        XCTAssertThrowsError(try PhotoSidecar.decodeEdits(try json(#"{"film": 3}"#), table: table))
    }

    /// The fixtures in docs/sidecar-fixtures, which the Windows and Linux
    /// readers are checked against too.
    func testFixtures() throws {
        let fixtures = root.appendingPathComponent("docs/sidecar-fixtures")
        func read(_ name: String) throws -> PhotoSidecar { try PhotoSidecar(data: Data(contentsOf: fixtures.appendingPathComponent(name))) }
        let table = try table

        let minimal = try read("minimal.negs")
        XCTAssertEqual(minimal.variants.map(\.id), ["3f9c2a1b"])
        XCTAssertEqual(minimal.variants[0].rating, 0)
        XCTAssertNil(minimal.variants[0].flag)
        let plain = try PhotoSidecar.decodeEdits(try XCTUnwrap(minimal.variants[0].edits), table: table)
        XCTAssertEqual(plain.state.film, "kodak_portra_400")
        XCTAssertEqual(plain.state.params, table.params)
        XCTAssertEqual(plain.state.crop, CropSettings())
        XCTAssertNil(plain.state.brightnessMatch)

        let edited = try read("edited.negs")
        XCTAssertEqual(edited.variants[0].rating, 3)
        XCTAssertEqual(edited.variants[0].flag, .pick)
        let saved = try PhotoSidecar.decodeEdits(try XCTUnwrap(edited.variants[0].edits), table: table).state
        XCTAssertEqual(saved.params["prep.highlights"], .number(-40))
        XCTAssertEqual(saved.params["prep.exposure_ev"], .number(0.65))
        XCTAssertEqual(saved.params["prep.shadows"], table.params["prep.shadows"])
        XCTAssertEqual(saved.params["io.rotate_cw"], .number(1))
        XCTAssertEqual(ImageSize.target(of: saved.params), 4000)
        XCTAssertEqual(saved.crop.rect, CGRect(x: 0.1, y: 0.05, width: 0.8, height: 0.9))
        XCTAssertEqual(saved.crop.aspect, .freeform)
        XCTAssertEqual(saved.border.style, .keyline)
        XCTAssertTrue(saved.border.enabled)
        XCTAssertEqual(saved.border.color.hex, "#f4efe6")
        XCTAssertEqual(saved.border.width, BorderSettings.defaultWidth)
        XCTAssertEqual(saved.brightnessMatch, BrightnessMatch(ev: 0.45, inputs: nil, highlightLimited: false))

        let reject = try read("rated-reject.negs")
        XCTAssertEqual(reject.variants[0].rating, 4)
        XCTAssertEqual(reject.variants[0].flag, .reject)
        XCTAssertNil(reject.variants[0].edits)

        let two = try read("two-variants.negs")
        XCTAssertEqual(two.variants.map(\.id), ["a1", "b7"])
        XCTAssertEqual(two.variants.map(\.rating), [2, 5])
        XCTAssertEqual(two.variants.map(\.flag), [nil, .pick])
        let copy = try PhotoSidecar.decodeEdits(try XCTUnwrap(two.variants[1].edits), table: table)
        XCTAssertEqual(copy.state.film, "kodak_trix")
        XCTAssertFalse(copy.matchSolved)
        XCTAssertEqual(copy.state.brightnessMatch?.highlightLimited, true)

        let unknown = try read("unknown-keys.negs")
        let future = try PhotoSidecar.decodeEdits(try XCTUnwrap(unknown.variants[0].edits), table: table).state
        XCTAssertEqual(future.params["prep.highlights"], .number(-20))
        XCTAssertEqual(future.crop.rect.height, 0.8)

        XCTAssertThrowsError(try read("newer-schema.negs")) { XCTAssertEqual($0 as? PhotoSidecar.Problem, .newer) }
        XCTAssertThrowsError(try read("newer-defaults.negs")) { XCTAssertEqual($0 as? PhotoSidecar.Problem, .newer) }
    }

    /// Writing this version's edits keeps everything it doesn't know, and
    /// what another app or Mac set meanwhile.
    func testAWriteKeepsUnknownKeysAndOtherFields() throws {
        let table = try table
        let file = try Data(contentsOf: root.appendingPathComponent("docs/sidecar-fixtures/unknown-keys.negs"))
        var raw = try PhotoSidecar(data: file).raw
        raw["variants"] = .array([raw["variants"].array[0].overlaid(with: try json(#"{"rating": 4, "flag": "reject"}"#))])
        var state = EditState(); state.film = "kodak_gold_200"; state.params = table.params
        state.params["prep.shadows"] = .number(15)
        let patch = SidecarPatch(variant: "e5", edits: PhotoSidecar.encodeEdits(state, matchSolved: false, table: table),
                                 known: ["params": [table.params], "crop": [table.crop], "border": [table.border]],
                                 defaultsVersion: 1, app: "0.15.0")
        let written = try XCTUnwrap(try patch.applied(to: raw))
        XCTAssertEqual(written["sync.device"], .string("Studio iMac"))
        XCTAssertEqual(written["app"], .string("0.15.0"))
        let variant = written["variants"].array[0]
        XCTAssertEqual(variant["label"], .string("red"))
        XCTAssertEqual(variant["tags"], .array([.string("street"), .string("night")]))
        XCTAssertEqual(variant["rating"], .number(4))
        XCTAssertEqual(variant["flag"], .string("reject"))
        let edits = variant["edits"]
        XCTAssertEqual(edits["futureGroup.amount"], .number(1))
        XCTAssertEqual(edits["params.future_stage.strength"], .number(2))
        XCTAssertEqual(edits["params.prep.future_knob"], .number(3))
        XCTAssertEqual(edits["crop.futureField"], .bool(true))
        // What this version knows is its own: the old highlights and crop are gone.
        XCTAssertEqual(edits["params.prep.highlights"], .null)
        XCTAssertEqual(edits["params.prep.shadows"], .number(15))
        XCTAssertEqual(edits["crop.rect"], .null)
        XCTAssertEqual(edits["film"], .string("kodak_gold_200"))
        // A newer file is never written over.
        var newer = raw; newer["negs"] = .number(2)
        XCTAssertThrowsError(try patch.applied(to: newer))
    }

    func testStoreWritesAtomicallyQueuesReadsAndRemovesEmptySidecars() async throws {
        let table = try table
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = dir.appendingPathComponent("IMG_0412.CR3")
        try Data("raw".utf8).write(to: photo)
        let store = SidecarStore()
        XCTAssertNil(try store.read(photo: photo))
        var state = EditState(); state.params = table.params; state.params["prep.contrast"] = .number(20)
        let known: [String: [JSONValue]] = ["params": [table.params], "crop": [table.crop], "border": [table.border]]
        let patch = SidecarPatch(variant: "v1", edits: PhotoSidecar.encodeEdits(state, matchSolved: false, table: table),
                                 known: known, defaultsVersion: 1, app: "test")
        store.enqueue(patch, for: photo)
        // Queued, not yet written: reads already see it.
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: photo).path))
        XCTAssertEqual(try store.read(photo: photo)?.variants.first?.edits?["params.prep.contrast"], .number(20))
        let failures = await store.flush()
        XCTAssertEqual(failures, [:])
        XCTAssertEqual(PhotoSidecar.url(for: photo).lastPathComponent, "IMG_0412.CR3.negs")
        let names = try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted()
        XCTAssertEqual(names, ["IMG_0412.CR3", "IMG_0412.CR3.negs"], "no temporary file left behind")
        let written = try XCTUnwrap(try store.read(photo: photo))
        XCTAssertEqual(written.defaultsVersion, 1)
        XCTAssertEqual(written.raw["negs"], .number(1))
        XCTAssertEqual(written.variants.map(\.id), ["v1"])
        // The same content gives the same bytes.
        let bytes = try Data(contentsOf: PhotoSidecar.url(for: photo))
        XCTAssertEqual(try PhotoSidecar.encode(written.raw), bytes)
        // Removing the only variant's edits leaves nothing: the file goes.
        store.enqueue(SidecarPatch(variant: "v1", edits: .some(nil), known: known, defaultsVersion: 1, app: "test"), for: photo)
        await store.flush()
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: photo).path))
        XCTAssertNil(try store.read(photo: photo))
    }

    func testUnwritableAndNewerSidecarsFailWithoutChangingAnything() async throws {
        let table = try table
        let dir = try folder()
        defer {
            try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path)
            try? FileManager.default.removeItem(at: dir)
        }
        let photo = dir.appendingPathComponent("a.jpg"), newer = dir.appendingPathComponent("b.jpg")
        try Data().write(to: photo); try Data().write(to: newer)
        let future = Data(#"{"negs": 2, "defaultsVersion": 1, "variants": [{"id": "x"}]}"#.utf8)
        try future.write(to: PhotoSidecar.url(for: newer))
        let store = SidecarStore()
        let known: [String: [JSONValue]] = ["params": [table.params], "crop": [table.crop], "border": [table.border]]
        let edits = PhotoSidecar.encodeEdits(EditState(), matchSolved: false, table: table)
        store.enqueue(SidecarPatch(variant: "x", edits: edits, known: known, defaultsVersion: 1, app: "test"), for: newer)
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: dir.path)
        store.enqueue(SidecarPatch(variant: "v", edits: edits, known: known, defaultsVersion: 1, app: "test"), for: photo)
        let failures = await store.flush()
        XCTAssertEqual(Set(failures.keys), [photo.standardizedFileURL, newer.standardizedFileURL])
        XCTAssertEqual(try Data(contentsOf: PhotoSidecar.url(for: newer)), future)
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: photo).path))
        XCTAssertNil(try store.read(photo: photo), "failed changes are dropped")
    }
}

/// The app with sidecars: per-photograph edits, migration, launch.
@MainActor final class SidecarModelTests: XCTestCase {
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

    /// Settings this suite changes, restored afterwards.
    private func preferences(match: Bool) {
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "sidecars"].map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(match, forKey: "autoPlaceNewImages")
        defaults.set(true, forKey: "sidecars")
    }

    private func model(store: SidecarStore) async -> EditorModel {
        let model = EditorModel(sidecars: store)
        addTeardownBlock { @MainActor in model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        return model
    }

    func testTheEnginesDefaultsAreTheNewestTable() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let client = try EngineClient(paths: paths)
        defer { client.stop() }
        var defaults = try await client.request(["command": .string("catalog")]).1["defaults"]
        defaults[EditorModel.lensCorrectionPath] = .bool(true)
        let table = try SidecarDefaults.load(version: SidecarDefaults.latest, data: data)
        XCTAssertEqual(defaults, table.params, "The engine's defaults changed. Write the next table with scripts/sidecar_defaults.py and raise SidecarDefaults.latest; old photographs keep theirs.")
    }

    func testEachPhotographKeepsItsOwnEdits() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: false)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let first = try fixture(in: dir, "first.png"), second = try fixture(in: dir, "second.png")
        let store = SidecarStore()
        let model = await model(store: store)
        model.open(first)
        model.chooseFilm("kodak_portra_400")
        model.set("prep.highlights", .number(-80))
        model.set("io.rotate_cw", .number(1))
        model.border(\.enabled).wrappedValue = true
        // Switching photographs writes the first one's edits.
        model.open(second)
        await store.flush()
        let saved = try XCTUnwrap(try store.read(photo: first))
        XCTAssertEqual(saved.variants.count, 1)
        XCTAssertEqual(saved.variants[0].edits?["film"], .string("kodak_portra_400"))
        XCTAssertEqual(saved.variants[0].edits?["params.prep.highlights"], .number(-80))
        XCTAssertEqual(saved.variants[0].edits?["border.enabled"], .bool(true))
        // A photograph without a sidecar keeps the film, not the edits.
        XCTAssertEqual(model.film, "kodak_portra_400")
        XCTAssertEqual(model.value("prep.highlights"), model.defaultValue("prep.highlights"))
        XCTAssertEqual(model.imageRotation, 0)
        XCTAssertFalse(model.border.enabled)
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: second).path), "opening isn't editing")
        // Its own edits, then back to the first: each has its own.
        model.chooseFilm("kodak_gold_200")
        model.set("prep.shadows", .number(30))
        model.open(first)
        XCTAssertEqual(model.film, "kodak_portra_400")
        XCTAssertEqual(model.value("prep.highlights"), .number(-80))
        XCTAssertEqual(model.value("prep.shadows"), model.defaultValue("prep.shadows"))
        XCTAssertEqual(model.imageRotation, 1)
        XCTAssertTrue(model.border.enabled)
        model.open(second)
        XCTAssertEqual(model.film, "kodak_gold_200")
        XCTAssertEqual(model.value("prep.shadows"), .number(30))
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
    }

    func testEditsAreWrittenASecondAfterTheLastOneAndRemovedWhenUndone() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: false)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png")
        let store = SidecarStore()
        let model = await model(store: store)
        model.open(photo)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        undo.beginUndoGrouping()
        model.set("prep.contrast", .number(25))
        undo.endUndoGrouping()
        let file = PhotoSidecar.url(for: photo)
        XCTAssertFalse(FileManager.default.fileExists(atPath: file.path), "not before a second has passed")
        try await waitUntil { FileManager.default.fileExists(atPath: file.path) }
        XCTAssertEqual(try store.read(photo: photo)?.variants.first?.edits?["params.prep.contrast"], .number(25))
        undo.undo()
        try await waitUntil { !FileManager.default.fileExists(atPath: file.path) }
        model.undoManager = nil
    }

    /// The automatic brightness match is no edit: opening a photograph and
    /// letting it solve writes nothing. Once the photograph has a sidecar,
    /// the solved exposure is stored, so opening it again doesn't solve.
    func testTheMatchAloneWritesNothingAndIsStoredOnceEdited() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: true)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png"), other = try fixture(in: dir, "other.png")
        let store = SidecarStore()
        let model = await model(store: store)
        model.open(photo)
        try await waitUntil { model.error != nil || model.previewCurrent && !model.brightnessMatchPending && !model.matchingBrightness }
        XCTAssertNil(model.error)
        let ev = try XCTUnwrap(model.state.brightnessMatch?.ev)
        await model.flushSidecars()
        XCTAssertFalse(FileManager.default.fileExists(atPath: PhotoSidecar.url(for: photo).path))
        model.set("prep.saturation", .number(10))
        model.open(other)
        await store.flush()
        let stored = try XCTUnwrap(try store.read(photo: photo)?.variants.first?.edits)
        XCTAssertEqual(stored["brightnessMatch.ev"], .number(ev))
        XCTAssertNil(stored["brightnessMatch"].object?["solved"])
        model.open(photo)
        XCTAssertFalse(model.brightnessMatchPending, "a stored match isn't solved again")
        XCTAssertEqual(model.state.brightnessMatch?.ev, ev)
        XCTAssertEqual(model.value("prep.saturation"), .number(10))
    }

    func testAFilmThisVersionLacksFallsBackAndIsKept() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: false)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png"), other = try fixture(in: dir, "other.png")
        try Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "d1", "edits": {"film": "dyeworks_imaginary", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 5}}}}]}"#.utf8)
            .write(to: PhotoSidecar.url(for: photo))
        let store = SidecarStore()
        let model = await model(store: store)
        model.open(photo)
        XCTAssertNotNil(model.error)
        model.error = nil
        XCTAssertEqual(model.film, "kodak_gold_200")
        XCTAssertEqual(model.value("prep.lift"), .number(5))
        model.set("prep.lift", .number(6))
        model.open(other)
        await store.flush()
        let edits = try XCTUnwrap(try store.read(photo: photo)?.variants.first?.edits)
        XCTAssertEqual(edits["film"], .string("dyeworks_imaginary"), "the saved film stays until another is picked")
        XCTAssertEqual(edits["params.prep.lift"], .number(6))
    }

    func testANewerSidecarOpensWithTheDefaultsAndIsLeftAlone() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: false)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png"), other = try fixture(in: dir, "other.png")
        let future = try Data(contentsOf: root.appendingPathComponent("docs/sidecar-fixtures/newer-schema.negs"))
        try future.write(to: PhotoSidecar.url(for: photo))
        let store = SidecarStore()
        let model = await model(store: store)
        model.open(photo)
        XCTAssertNotNil(model.error)
        model.error = nil
        model.set("prep.lift", .number(6))
        model.open(other)
        await store.flush()
        XCTAssertEqual(try Data(contentsOf: PhotoSidecar.url(for: photo)), future)
    }

    func testSidecarsOffRememberNothing() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: false)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png"), other = try fixture(in: dir, "other.png")
        try Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "d1", "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"lift": 5}}}}]}"#.utf8)
            .write(to: PhotoSidecar.url(for: photo))
        let store = SidecarStore()
        let model = await model(store: store)
        model.sidecarsEnabled = false
        model.open(photo)
        XCTAssertEqual(model.value("prep.lift"), model.defaultValue("prep.lift"))
        model.set("prep.lift", .number(9))
        model.open(other)
        await model.flushSidecars()
        XCTAssertEqual(try store.read(photo: photo)?.variants.first?.edits?["params.prep.lift"], .number(5), "existing sidecars stay untouched")
        // Back on, they are used again.
        model.sidecarsEnabled = true
        model.open(photo)
        XCTAssertEqual(model.value("prep.lift"), .number(5))
    }

    /// A session saved by an earlier version holds the last photograph's
    /// edits: they restore as before and move to its sidecar once launch is
    /// over; the session is saved without them from then on.
    func testASchema2SessionMovesItsEditsToTheSidecar() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        preferences(match: false)
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let photo = try fixture(in: dir, "photo.png")
        let table = try SidecarDefaults.load(version: 1, data: data)
        var saved = EditState()
        saved.film = "kodak_portra_400"
        saved.params = table.params
        saved.params["prep.highlights"] = .number(-65)
        saved.crop.rect = CGRect(x: 0.1, y: 0.1, width: 0.8, height: 0.8)
        let sessionFile = dir.appendingPathComponent("session.json")
        try JSONEncoder().encode(EditorSession(source: photo, state: saved, inspectorVisible: false)).write(to: sessionFile)
        let sessions = SessionStore(file: sessionFile)
        let store = SidecarStore()
        let model = EditorModel(sidecars: store)
        defer { model.shutdown() }
        let lookup = SidecarLookup(store: store, data: data)
        await model.start(paths: paths, restoration: Task { try await sessions.load(sidecars: lookup) })
        XCTAssertEqual(model.value("prep.highlights"), .number(-65))
        XCTAssertFalse(model.inspectorVisible)
        await store.flush()
        let edits = try XCTUnwrap(try store.read(photo: photo)?.variants.first?.edits)
        XCTAssertEqual(edits["film"], .string("kodak_portra_400"))
        XCTAssertEqual(edits["params.prep.highlights"], .number(-65))
        XCTAssertEqual(edits["crop.rect"], .array([.array([.number(0.1), .number(0.1)]), .array([.number(0.8), .number(0.8)])]))
        try await sessions.save(try XCTUnwrap(model.session))
        let text = try String(contentsOf: sessionFile, encoding: .utf8)
        XCTAssertFalse(text.contains("highlights"))
        // The next launch reads the edits from the sidecar, with the session.
        let next = try XCTUnwrap(try sessions.read(sidecars: lookup))
        XCTAssertEqual(next.schema, 3)
        XCTAssertNil(next.state)
        XCTAssertEqual(next.sidecar?.edits?.state.params["prep.highlights"], .number(-65))
        let relaunched = EditorModel(sidecars: store)
        defer { relaunched.shutdown() }
        await relaunched.start(paths: paths, restoration: Task { next })
        XCTAssertEqual(relaunched.source?.resolvingSymlinksInPath(), photo.resolvingSymlinksInPath())
        XCTAssertEqual(relaunched.value("prep.highlights"), .number(-65))
        XCTAssertEqual(relaunched.crop.rect, saved.crop.rect)
        XCTAssertFalse(relaunched.inspectorVisible)
    }
}
