import XCTest
@testable import NegsApp

final class PresetNameTests: XCTestCase {
    func testNamesEveryPlatformAccepts() {
        for name in ["Portra 400 warm", "Kodachrome – 1970s", "Trix (push 2)", "Ünïcødé", "v1.2"] {
            XCTAssertNil(PresetName.problem(name), name)
        }
        for name in ["", "   ", "a/b", "a\\b", "a:b", "what?", "a*", "\"quoted\"", "<x>", "a|b", ".hidden", "trailing.",
                     "trailing ", " leading", "tab\there", "CON", "con", "Lpt1", "NUL.json", "COM9.backup", String(repeating: "x", count: 121)] {
            XCTAssertNotNil(PresetName.problem(name), name)
        }
    }

    func testNamesCompareWithoutCaseOrUnicodeForm() {
        let composed = "Ektachrome caf\u{E9}", decomposed = "EKTACHROME CAFE\u{301}"
        XCTAssertEqual(PresetName.key(composed), PresetName.key(decomposed))
        let taken: Set = [PresetName.key("Portra"), PresetName.key("portra 2")]
        XCTAssertEqual(PresetName.unique("PORTRA", taken: taken), "PORTRA 3")
        XCTAssertEqual(PresetName.unique("Gold", taken: taken), "Gold")
    }

    func testFileNamesBecomePresetNames() {
        XCTAssertEqual(PresetName.from(fileName: "05 P400 natural"), "05 P400 natural")
        XCTAssertEqual(PresetName.from(fileName: "a:b|c"), "a b c")
        XCTAssertEqual(PresetName.from(fileName: "..."), "Preset")
        XCTAssertEqual(PresetName.from(fileName: "CON"), "Preset")
    }
}

final class PresetLibraryTests: XCTestCase {
    private var folder: URL!

    override func setUpWithError() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("negs-presets-\(UUID().uuidString)", isDirectory: true)
    }
    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: folder) }

    func testAddingNeverReplacesAndRenamingChecksNames() async throws {
        let library = PresetLibrary(folder: folder)
        let first = try await library.add(Data("1".utf8), name: "Portra")
        let second = try await library.add(Data("2".utf8), name: "portra")
        XCTAssertEqual(second.name, "portra 2")
        XCTAssertEqual(try Data(contentsOf: first.url), Data("1".utf8))
        // Hidden temporary files and other files aren't presets.
        try Data().write(to: folder.appendingPathComponent(".partial.tmp"))
        try Data().write(to: folder.appendingPathComponent("notes.txt"))
        let listed = try await library.list()
        XCTAssertEqual(listed.map(\.name), ["Portra", "portra 2"])

        do { _ = try await library.rename(second, to: "PORTRA"); XCTFail("Renamed onto another preset") }
        catch { XCTAssertTrue(error.localizedDescription.contains("already exists"), error.localizedDescription) }
        do { _ = try await library.rename(second, to: "a/b"); XCTFail("Accepted an invalid name") } catch {}
        // Another case of its own name is the same file.
        let recased = try await library.rename(first, to: "PORTRA")
        let relisted = try await library.list()
        XCTAssertEqual(relisted.map(\.name), ["PORTRA", "portra 2"])
        let renamed = try await library.rename(recased, to: "Portra warm")
        let bytes = try await library.read(renamed)
        XCTAssertEqual(bytes, Data("1".utf8))
    }

    func testTheUsersOrderIsRememberedAndAddedPresetsGoLast() async throws {
        let library = PresetLibrary(folder: folder)
        for name in ["Zeta", "Alpha", "Mid"] { _ = try await library.add(Data(name.utf8), name: name) }
        var listed = try await library.list()
        XCTAssertEqual(listed.map(\.name), ["Zeta", "Alpha", "Mid"])
        try await library.setOrder([listed[2], listed[0], listed[1]])
        // Copied in by hand: after the ordered ones, in name order.
        try Data("x".utf8).write(to: folder.appendingPathComponent("Copied B.json"))
        try Data("x".utf8).write(to: folder.appendingPathComponent("Copied A.json"))
        listed = try await library.list()
        XCTAssertEqual(listed.map(\.name), ["Mid", "Zeta", "Alpha", "Copied A", "Copied B"])
        // Renaming keeps the place; deleting forgets it; adding goes last.
        _ = try await library.rename(listed[1], to: "Omega")
        try await library.delete(listed[0])
        _ = try await library.add(Data("n".utf8), name: "Newest")
        listed = try await library.list()
        XCTAssertEqual(listed.map(\.name), ["Omega", "Alpha", "Copied A", "Copied B", "Newest"])
        // A fresh library reads the same order from the folder.
        let reopened = try await PresetLibrary(folder: folder).list()
        XCTAssertEqual(reopened.map(\.name), listed.map(\.name))
    }

    func testFilesChangedOnDiskAreLeftAlone() async throws {
        let library = PresetLibrary(folder: folder)
        let preset = try await library.add(Data("original".utf8), name: "Gold")
        // Edited by hand after it was listed.
        try Data("edited elsewhere".utf8).write(to: preset.url)
        do { try await library.delete(preset); XCTFail("Deleted a changed file") }
        catch { XCTAssertTrue(error.localizedDescription.contains("changed on disk"), error.localizedDescription) }
        do { _ = try await library.rename(preset, to: "Other"); XCTFail("Renamed a changed file") } catch {}
        XCTAssertTrue(FileManager.default.fileExists(atPath: preset.url.path))
        let listed = try await library.list()
        let current = try XCTUnwrap(listed.first)
        try await library.delete(current)
        let remaining = try await library.list()
        XCTAssertEqual(remaining, [])
    }
}

@MainActor final class PresetApplyTests: XCTestCase {
    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func waitUntil(_ condition: () -> Bool) async throws {
        for _ in 0..<500 where !condition() { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(condition(), "Timed out")
    }

    func testApplyingIsOneUndoStepAndKeepsThePhotographsSettings() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("negs-presets-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let library = PresetLibrary(folder: folder)
        let model = EditorModel(presetLibrary: library)
        defer { model.shutdown() }
        await model.start(paths: paths())
        XCTAssertTrue(model.hasCatalog, model.error ?? "Engine did not start")
        // The photograph's own settings, which a preset never changes.
        model.setLongestSide(3000)
        model.set("io.rotate_cw", .number(1))
        model.set("post.contrast", .number(20))
        model.set("settings.use_cat16", .bool(false))
        model.set("camera.exposure_compensation_ev", .number(1.5))
        let before = model.state
        // Only now: an undo registered outside a group aborts the test.
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo

        let text = #"{"simulation": {"film_stock": "kodak_doublex", "print_paper": "kodak_2302"},"#
            + #""film_chemistry": {"development_time": 8.0}, "couplers": {"langmuir_donor_k_rgb": [Infinity, 1.0, 1.0]},"#
            + #""camera": {"exposure_compensation_ev": -0.5}, "load_raw": {"white_balance": "daylight"}}"#
        _ = try await library.add(Data(text.utf8), name: "Double-X")
        await model.refreshPresets()
        let preset = try XCTUnwrap(model.presets.first)
        guard case .valid(let film, let paper, false) = model.presetStatus[preset] else { return XCTFail("\(String(describing: model.presetStatus[preset]))") }
        XCTAssertEqual(film, "kodak_doublex"); XCTAssertEqual(paper, "kodak_2302")

        undo.beginUndoGrouping()
        model.applyPreset(preset)
        try await waitUntil { model.film == "kodak_doublex" }
        undo.endUndoGrouping()
        XCTAssertEqual(model.paper, "kodak_2302")
        XCTAssertEqual(model.value("camera.exposure_compensation_ev"), .number(-0.5))
        // Listed by Double-X, so the Development picker shows it.
        XCTAssertEqual(model.value("film_render.development_time"), .number(9))
        XCTAssertEqual(model.value("film_render.dir_couplers.langmuir_donor_k_rgb.0"), .null)
        XCTAssertEqual(model.value("io.raw_white_balance.mode"), .string("daylight"))
        for path in [ImageSize.targetPath, "io.rotate_cw", "post.contrast", "settings.use_cat16"] {
            XCTAssertEqual(model.value(path), before.params[path], path)
        }
        XCTAssertEqual(undo.undoActionName, "Apply Preset “Double-X”")
        undo.undo()
        XCTAssertEqual(model.state, before)
        undo.redo()
        XCTAssertEqual(model.film, "kodak_doublex")

        // New Preset saves the current settings; they read back the same.
        model.newPreset()
        try await waitUntil { model.renamingPreset != nil }
        let saved = try XCTUnwrap(model.presets.first { $0.name == "Kodak Double-X 5222" } ?? model.presets.first { $0.name != "Double-X" })
        XCTAssertEqual(model.renamingPreset, saved.url)
        guard case .valid("kodak_doublex", "kodak_2302", false) = model.presetStatus[saved] else { return XCTFail("\(String(describing: model.presetStatus[saved]))") }
        // Added at the end; Escape while naming it removes it again.
        XCTAssertEqual(model.presets.last, saved)
        model.cancelRename(saved)
        try await waitUntil { model.presets.count == 1 }
        XCTAssertEqual(model.presets.map(\.name), ["Double-X"])

        // Escape on an existing preset only stops renaming.
        model.newPreset()
        try await waitUntil { model.renamingPreset != nil }
        let kept = try XCTUnwrap(model.presets.last)
        model.renamePreset(kept, to: "Mine")
        try await waitUntil { model.presets.contains { $0.name == "Mine" } }
        let mine = try XCTUnwrap(model.presets.first { $0.name == "Mine" })
        model.renamingPreset = mine.url
        model.cancelRename(mine)
        XCTAssertNil(model.renamingPreset)
        // Dragging the last row to the top.
        model.movePresets(from: [1], to: 0)
        XCTAssertEqual(model.presets.map(\.name), ["Mine", "Double-X"])
        let orderFile = folder.appendingPathComponent(".order.json")
        // Saved in the background; wait for the new order on disk.
        try await waitUntil {
            guard let saved = try? String(contentsOf: orderFile, encoding: .utf8),
                  let mine = saved.range(of: "Mine"), let other = saved.range(of: "Double-X") else { return false }
            return mine.lowerBound < other.lowerBound
        }
        await model.refreshPresets()
        XCTAssertEqual(model.presets.map(\.name), ["Mine", "Double-X"])
    }

    func testFilesTheEngineRefusesAreListedWithTheReason() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("negs-presets-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let library = PresetLibrary(folder: folder)
        _ = try await library.add(Data(#"{"output_gamut_compress": {"algorithm": "jzazbz"}}"#.utf8), name: "Exotic")
        let model = EditorModel(presetLibrary: library)
        defer { model.shutdown() }
        await model.start(paths: paths())
        await model.refreshPresets()
        let preset = try XCTUnwrap(model.presets.first)
        guard case .invalid(let reason) = model.presetStatus[preset] else { return XCTFail("Accepted") }
        XCTAssertTrue(reason.contains("jzazbz"), reason)
        // Importing refuses it too, and copies nothing.
        let outside = folder.deletingLastPathComponent().appendingPathComponent("refused-\(UUID().uuidString).json")
        try Data(#"{"simulation": {"film_stock": "kodak_unknown"}}"#.utf8).write(to: outside)
        defer { try? FileManager.default.removeItem(at: outside) }
        model.importPresets([outside])
        try await waitUntil { model.error != nil }
        XCTAssertTrue(model.error?.contains("kodak_unknown") ?? false, model.error ?? "")
        XCTAssertEqual(model.presets.count, 1)
    }
}
