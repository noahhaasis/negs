import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

/// The alert sheet (§4.14, §6.5): the app's questions and errors in its own
/// sheet instead of `NSAlert`. Return is the default button, Esc the Cancel one.
@MainActor final class AlertSheetTests: XCTestCase {
    private func folder() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-alert-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate())
    }

    private let question = AppAlert(title: "Delete it?", detail: "For good.",
                                    buttons: [.init(title: "Delete", role: .destructive), .init(title: "Cancel", role: .cancel)])

    // MARK: The alert

    func testReturnIsTheFirstButtonAndEscTheCancelOne() {
        XCTAssertEqual(question.defaultIndex, 0)
        XCTAssertEqual(question.cancelIndex, 1)
        // The only button takes both.
        let said = AppAlert(title: "Done")
        XCTAssertEqual(said.buttons.map(\.title), ["OK"])
        XCTAssertEqual(said.cancelIndex, 0)
        // A question with no Cancel waits for an answer.
        let resume = AppAlert(title: "Resume?", buttons: [.init(title: "Resume"), .init(title: "Discard")])
        XCTAssertNil(resume.cancelIndex)
        // The default can be the safe one too: Continue Exporting.
        let quit = AppAlert(title: "Quit?", buttons: [.init(title: "Continue Exporting", role: .cancel), .init(title: "Quit")])
        XCTAssertEqual(quit.defaultIndex, 0)
        XCTAssertEqual(quit.cancelIndex, 0)
    }

    func testReturnsButtonSaysSo() {
        // Round 16, C: Return's button is heavier and ends in ↩; the others don't.
        XCTAssertEqual(OutlinedButtonStyle(prominent: true).mark, "↩")
        XCTAssertEqual(OutlinedButtonStyle(prominent: true).weight, 650)
        XCTAssertNil(OutlinedButtonStyle().mark)
        XCTAssertEqual(OutlinedButtonStyle().weight, 500)
    }

    func testAnErrorsFirstSentenceIsItsHeading() {
        XCTAssertEqual(AppAlert.error("Could not save the editing session. Negs stayed open. Disk full."),
                       AppAlert(title: "Could not save the editing session", detail: "Negs stayed open. Disk full."))
        XCTAssertEqual(AppAlert.error("Nothing was deleted: the folder “x” is read-only."),
                       AppAlert(title: "Nothing was deleted: the folder “x” is read-only."))
        XCTAssertEqual(AppAlert.error("“x.json” isn’t a preset. "), AppAlert(title: "“x.json” isn’t a preset."))
    }

    // MARK: The queue

    func testQuestionsWaitTheirTurnAndAnswerWithTheirTickBoxes() async throws {
        let model = EditorModel()
        defer { model.shutdown() }
        XCTAssertNil(model.alert)
        var answers: [String: AlertAnswer] = [:]
        let first = Task { answers["first"] = await model.ask(question) }
        let choosing = AppAlert(title: "Copy Edits", buttons: [.init(title: "Copy"), .init(title: "Cancel", role: .cancel)],
                                choices: [.init(title: "Lab", on: true), .init(title: "Border", on: false)])
        let second = Task { answers["second"] = await model.ask(choosing) }
        try await waitUntil { model.alert?.alert == self.question }

        // Esc takes Cancel; the next question shows.
        XCTAssertTrue(model.dismissAlert())
        await first.value
        XCTAssertEqual(answers["first"], AlertAnswer(button: 1))
        try await waitUntil { model.alert?.alert == choosing }
        let shown = try XCTUnwrap(model.alert)
        XCTAssertEqual(shown.choices.map(\.on), [true, false])
        shown.choices[0].on = false
        shown.choices[1].on = true
        // Return takes the default.
        model.confirmAlert()
        await second.value
        XCTAssertEqual(answers["second"], AlertAnswer(button: 0, choices: [false, true]))
        XCTAssertNil(model.alert)
    }

    func testEscLeavesAQuestionWithoutCancelOpen() async throws {
        let model = EditorModel()
        defer { model.shutdown() }
        let asking = Task { await model.ask(AppAlert(title: "Resume?", buttons: [.init(title: "Resume"), .init(title: "Discard")])) }
        try await waitUntil { model.alert != nil }
        XCTAssertFalse(model.dismissAlert())
        XCTAssertNotNil(model.alert)
        model.answerAlert(1)
        let answer = await asking.value
        XCTAssertEqual(answer.button, 1)
    }

    func testAnErrorShowsInTheSheetAfterAnyQuestionAndClosesWithOK() async throws {
        let model = EditorModel()
        defer { model.shutdown() }
        model.error = "The folder is read-only. Edits stay in Negs."
        XCTAssertEqual(model.alert?.alert, AppAlert(title: "The folder is read-only", detail: "Edits stay in Negs."))
        // A question comes first; the error waits under it.
        let asking = Task { await model.ask(question) }
        try await waitUntil { model.alert?.alert == self.question }
        model.confirmAlert()
        _ = await asking.value
        XCTAssertEqual(model.alert?.alert.title, "The folder is read-only")
        // Esc closes an error, as OK does.
        XCTAssertTrue(model.dismissAlert())
        XCTAssertNil(model.error)
        XCTAssertNil(model.alert)
        model.error = "Again."
        model.confirmAlert()
        XCTAssertNil(model.error)
    }

    // MARK: The questions

    func testDeletingAPhotographAsksInTheSheet() async throws {
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "sidecars"].map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(false, forKey: "autoPlaceNewImages")
        defaults.set(false, forKey: "sidecars")
        let dir = try folder(), bin = try folder()
        let photo = dir.appendingPathComponent("a.png")
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        try bitmap.representation(using: .png, properties: [:])!.write(to: photo)
        let model = EditorModel(sidecars: SidecarStore())
        defer { model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        // Never the user's own Trash.
        model.trash = PhotoTrash(available: { _ in true }, trash: { file in
            let landed = bin.appendingPathComponent(file.lastPathComponent)
            try FileManager.default.moveItem(at: file, to: landed)
            return landed
        }, remove: { try FileManager.default.removeItem(at: $0) })
        model.open(photo)
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil { model.library.items.count == 1 }

        model.requestDelete(asking: true)
        try await waitUntil { model.alert != nil }
        let asked = try XCTUnwrap(model.alert?.alert)
        XCTAssertEqual(asked.title, "Move “a.png” to the Trash?")
        XCTAssertEqual(asked.buttons.map(\.title), ["Move to Trash", "Cancel"])
        XCTAssertTrue(model.dismissAlert())
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertTrue(FileManager.default.fileExists(atPath: photo.path), "Esc deletes nothing")

        model.requestDelete(asking: true)
        try await waitUntil { model.alert != nil }
        model.confirmAlert()
        try await waitUntil { !FileManager.default.fileExists(atPath: photo.path) }
        XCTAssertTrue(FileManager.default.fileExists(atPath: bin.appendingPathComponent("a.png").path))
    }

    func testCopyEditsChoosingTicksTheLastChoiceAndRemembersTheNewOne() async throws {
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: "copyEditGroups")
        addTeardownBlock { defaults.set(saved, forKey: "copyEditGroups") }
        let model = EditorModel()
        defer { model.shutdown() }
        model.copyGroups = [.lab]
        model.copyEditsChoosing()
        try await waitUntil { model.alert != nil }
        let shown = try XCTUnwrap(model.alert)
        XCTAssertEqual(shown.alert.buttons.map(\.title), ["Copy", "Cancel"])
        XCTAssertEqual(shown.choices.map(\.title), EditGroup.allCases.map(\.title))
        XCTAssertEqual(shown.choices.filter(\.on).map(\.title), [EditGroup.lab.title])
        // Cancel changes nothing.
        model.dismissAlert()
        try await Task.sleep(for: .milliseconds(50))
        XCTAssertEqual(model.copyGroups, [.lab])

        model.copyEditsChoosing()
        try await waitUntil { model.alert != nil }
        let again = try XCTUnwrap(model.alert)
        let border = try XCTUnwrap(EditGroup.allCases.firstIndex(of: .border))
        again.choices[border].on = true
        model.confirmAlert()
        try await waitUntil { model.copyGroups == [.lab, .border] }
    }

    func testDeletingAPresetAsksFirst() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("negs-presets-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let library = PresetLibrary(folder: folder)
        let preset = try await library.add(Data("{}".utf8), name: "Portra")
        let model = EditorModel(presetLibrary: library)
        defer { model.shutdown() }

        model.deletePreset(preset)
        try await waitUntil { model.alert != nil }
        XCTAssertEqual(model.alert?.alert.title, "Delete the preset “Portra”?")
        XCTAssertEqual(model.alert?.alert.buttons.map(\.title), ["Delete", "Cancel"])
        model.dismissAlert()
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertTrue(FileManager.default.fileExists(atPath: preset.url.path), "Cancel keeps it")

        model.deletePreset(preset)
        try await waitUntil { model.alert != nil }
        model.confirmAlert()
        try await waitUntil { !FileManager.default.fileExists(atPath: preset.url.path) }
    }

    // MARK: On the window

    /// It hangs from under the title bar, centred, over everything, with
    /// the window dimmed below the title bar; it takes the keys while it
    /// shows and gives them back.
    func testTheSheetHangsFromTheTitleBarOverADimmedWindow() async throws {
        let model = EditorModel()
        defer { model.shutdown() }
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 800),
                              styleMask: [.titled, .closable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
        // The columns' container, under the 38 pt title band, as in the editor.
        let frame = NSView(frame: NSRect(x: 0, y: 0, width: 1200, height: 800))
        window.contentView = frame
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 1200, height: 800 - Theme.Size.titleBar))
        frame.addSubview(content)
        let before = NSTextView(frame: NSRect(x: 0, y: 0, width: 100, height: 20))
        content.addSubview(before)
        window.orderFront(nil)
        defer { window.orderOut(nil) }
        XCTAssertTrue(window.makeFirstResponder(before))
        let presenter = AlertPresenter(model: model, window: window, root: content)
        // The window isn't key (here nothing is; in the app, Settings may
        // be, when it asks): it's given the keys, so Return and Esc answer.
        var keyed: [NSWindow] = []
        presenter.takeKeys = { keyed.append($0) }
        let asking = Task { await model.ask(question) }
        try await waitUntil { model.alert != nil }
        presenter.show(model.alert)
        XCTAssertTrue(presenter.onWindow)
        let cover = try XCTUnwrap(content.subviews.last as? AlertCover)
        let below = content.bounds
        XCTAssertEqual(cover.dim.frame, below, "dimmed below the title bar")
        XCTAssertTrue(cover.clipsToBounds, "it slides out from under the title bar, not over it")
        let sheet = try XCTUnwrap(cover.subviews.last)
        try await Task.sleep(for: .milliseconds(400))
        XCTAssertEqual(sheet.frame.maxY, below.maxY, accuracy: 0.5, "flush under the title bar")
        XCTAssertEqual(sheet.frame.midX, cover.bounds.midX, accuracy: 1)
        XCTAssertEqual(sheet.frame.width, AlertSheet.width + AlertSheet.margin.leading + AlertSheet.margin.trailing, accuracy: 0.5)
        XCTAssertTrue(window.firstResponder === sheet, "the keys go to the sheet")
        XCTAssertEqual(keyed.count, 1)
        XCTAssertTrue(keyed.first === window, "asked from another window, it takes the keys")
        // A click under it lands on the cover, not what's below.
        XCTAssertTrue(content.hitTest(NSPoint(x: 20, y: 20)) === cover)

        model.confirmAlert()
        _ = await asking.value
        presenter.show(model.alert)
        XCTAssertTrue(window.firstResponder === before, "the keys come back")
        try await waitUntil { cover.superview == nil }
        XCTAssertFalse(presenter.onWindow)
    }
}
