import AppKit
import XCTest
@testable import NegsApp

/// The Presets pane as tiles (§4.12): what each tile says, the ring on the
/// preset applied last, sorting by drag, and the delete key on a tile.
@MainActor final class PresetPaneTests: XCTestCase {
    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func waitUntil(_ condition: () -> Bool) async throws {
        for _ in 0..<500 where !condition() { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(condition(), "Timed out")
    }

    private func photo(_ name: String, in folder: URL) throws -> URL {
        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 48, pixelsHigh: 32, bitsPerSample: 8, samplesPerPixel: 3,
                                                    hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 48 * 3, bitsPerPixel: 24))
        for i in 0..<(48 * 32 * 3) { bitmap.bitmapData![i] = UInt8(i % 251) }
        let url = folder.appendingPathComponent(name)
        try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: url)
        return url
    }

    /// Under the name: the film, then "printed on" and the paper, or that
    /// the film is scanned, in the label's words.
    func testWhatATileSays() {
        let film = Stock(.object(["id": .string("kodak_gold_200"), "name": .string("Kodak Gold 200"), "paper": .bool(false)]))
        let paper = Stock(.object(["id": .string("kodak_portra_endura"), "name": .string("Kodak Professional Portra Endura"), "paper": .bool(true)]))
        let printed = PresetStatus.valid(film: "kodak_gold_200", paper: "kodak_portra_endura", scan: false)
        XCTAssertEqual(PresetTileRules.film(printed, films: [film]), "Kodak Gold 200")
        XCTAssertEqual(PresetTileRules.paper(printed, papers: [paper])?.lead, "printed on")
        XCTAssertEqual(PresetTileRules.paper(printed, papers: [paper])?.name, "Kodak Professional Portra Endura")
        let scanned = PresetStatus.valid(film: "kodak_gold_200", paper: "kodak_portra_endura", scan: true)
        XCTAssertEqual(PresetTileRules.paper(scanned, papers: [paper])?.lead, "not printed · film scan")
        XCTAssertEqual(PresetTileRules.paper(scanned, papers: [paper])?.name, "")
        // An id the app doesn't list is shown as it is.
        XCTAssertEqual(PresetTileRules.film(.valid(film: "mystery", paper: "x", scan: false), films: [film]), "mystery")
        XCTAssertNil(PresetTileRules.film(.invalid("Its film isn’t in this version of Negs."), films: [film]))
        XCTAssertNil(PresetTileRules.paper(nil, papers: [paper]))

        XCTAssertEqual(PresetTileRules.foot(dropping: false), "click to apply · drag to sort · drop files to import")
        XCTAssertEqual(PresetTileRules.foot(dropping: true), "drop to import")
    }

    /// A dragged tile carries its own file, marked as the pane's, and lands
    /// on the tile it's over, gaps included.
    func testDraggingFindsTheTileUnderThePointer() throws {
        let file = URL(fileURLWithPath: "/tmp/Presets/Warm Portra.json")
        XCTAssertEqual(PresetsDrop.preset(in: PresetsDrop.marker(file)), file)
        XCTAssertNil(PresetsDrop.preset(in: "/tmp/Presets/Warm Portra.json"), "plain text isn't a dragged tile")

        let left = URL(fileURLWithPath: "/a.json"), right = URL(fileURLWithPath: "/b.json")
        let frames = [left: CGRect(x: 22, y: 18, width: 141, height: 140), right: CGRect(x: 177, y: 18, width: 141, height: 140)]
        XCTAssertEqual(PresetsDrop.tile(at: CGPoint(x: 80, y: 60), in: frames), left)
        XCTAssertEqual(PresetsDrop.tile(at: CGPoint(x: 166, y: 60), in: frames), left, "the gap counts as the nearer tile")
        XCTAssertEqual(PresetsDrop.tile(at: CGPoint(x: 173, y: 60), in: frames), right)
        XCTAssertNil(PresetsDrop.tile(at: CGPoint(x: 80, y: 400), in: frames))
    }

    /// ⌫ on a focused tile deletes the preset, not the photograph: the
    /// menu's Move to Trash steps aside, as it does for a list.
    func testTheDeleteKeyOnATileIsTheTiles() throws {
        _ = NSApplication.shared
        let window = EditorWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled], backing: .buffered, defer: false)
        let content = NSView(frame: window.contentLayoutRect)
        window.contentView = content
        var deleted = 0
        let tile = FocusAnchorView(frame: NSRect(x: 10, y: 10, width: 100, height: 80))
        tile.delete = { deleted += 1 }
        let plain = FocusAnchorView(frame: NSRect(x: 150, y: 10, width: 100, height: 80))
        content.addSubview(tile); content.addSubview(plain)

        XCTAssertTrue(window.makeFirstResponder(tile))
        XCTAssertFalse(MainMenu.takesDeleteKey(window), "Move to Trash leaves ⌫ to the tile")
        let key = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber,
                                                 context: nil, characters: "\u{7f}", charactersIgnoringModifiers: "\u{7f}", isARepeat: false, keyCode: 51))
        tile.keyDown(with: key)
        XCTAssertEqual(deleted, 1)

        XCTAssertTrue(window.makeFirstResponder(plain))
        XCTAssertTrue(MainMenu.takesDeleteKey(window), "a control without its own delete leaves ⌫ to the menu")
    }

    /// The preset applied last to a photograph has the ring; each photograph
    /// has its own, a new preset is the one ringed, and the ring follows a
    /// rename and goes with a delete.
    func testTheRingIsOnThePresetAppliedLastToThisPhotograph() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("negs-preset-pane-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let photos = folder.appendingPathComponent("photos", isDirectory: true)
        try FileManager.default.createDirectory(at: photos, withIntermediateDirectories: true)
        let first = try photo("one.png", in: photos), second = try photo("two.png", in: photos)
        let library = PresetLibrary(folder: folder.appendingPathComponent("Presets", isDirectory: true))
        let model = EditorModel(presetLibrary: library)
        defer { model.shutdown() }
        await model.start(paths: paths())
        XCTAssertTrue(model.hasCatalog, model.error ?? "Engine did not start")
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo

        let text = #"{"simulation": {"film_stock": "kodak_doublex", "print_paper": "kodak_2302"}}"#
        _ = try await library.add(Data(text.utf8), name: "Double-X")
        await model.refreshPresets()
        let doubleX = try XCTUnwrap(model.presets.first)

        model.open(first)
        try await waitUntil { model.image != nil || model.error != nil }
        XCTAssertNil(model.appliedPreset, "nothing applied yet")
        undo.beginUndoGrouping()
        model.applyPreset(doubleX)
        try await waitUntil { model.film == "kodak_doublex" }
        undo.endUndoGrouping()
        XCTAssertEqual(model.appliedPreset, doubleX.url)

        model.open(second)
        try await waitUntil { model.sourceItem == second }
        XCTAssertNil(model.appliedPreset, "another photograph has its own")
        model.open(first)
        try await waitUntil { model.sourceItem == first }
        XCTAssertEqual(model.appliedPreset, doubleX.url, "and coming back keeps it")

        // New: the photograph's settings are the new preset's, so it's ringed.
        model.newPreset()
        try await waitUntil { model.renamingPreset != nil }
        let made = try XCTUnwrap(model.presets.first { $0.url == model.renamingPreset })
        XCTAssertEqual(model.appliedPreset, made.url)
        model.renamePreset(made, to: "Mine")
        try await waitUntil { model.presets.contains { $0.name == "Mine" } }
        let mine = try XCTUnwrap(model.presets.first { $0.name == "Mine" })
        XCTAssertEqual(model.appliedPreset, mine.url, "the ring follows the rename")

        // New, then Escape: the preset goes, and its ring with it.
        model.newPreset()
        try await waitUntil { model.renamingPreset != nil && model.renamingPreset != mine.url }
        let unnamed = try XCTUnwrap(model.presets.first { $0.url == model.renamingPreset })
        XCTAssertEqual(model.appliedPreset, unnamed.url)
        model.cancelRename(unnamed)
        try await waitUntil { !model.presets.contains(unnamed) }
        XCTAssertNil(model.appliedPreset)
    }
}
