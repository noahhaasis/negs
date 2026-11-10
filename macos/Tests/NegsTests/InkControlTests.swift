import AppKit
import XCTest
@testable import NegsApp

/// The inspector's T2 · Label behaviour (IMPLEMENTATION.md §6): the slider's
/// keys, typed numbers past either end, Tab through the inspector, the
/// keyboard-only focus box, what the Lab keeps behind "N more", and the
/// summary over the Export button.
@MainActor final class InkControlTests: XCTestCase {
    // MARK: Typed numbers past either end

    func testClampNoteNamesTheEndANumberWasSetTo() {
        XCTAssertEqual(LabeledSlider.clampNote(typed: 250, range: -100...100, step: 1),
                       "100 is the most; it was set to 100.")
        let low = (-2.0).formatted(.number.precision(.fractionLength(0...2)))
        XCTAssertEqual(LabeledSlider.clampNote(typed: -7, range: -2...2, step: 0.01, unit: " EV"),
                       "\(low) EV is the least; it was set to \(low) EV.")
        // In the reader's locale: 2.25 or 2,25.
        let end = 2.25.formatted(.number.precision(.fractionLength(0...2)))
        XCTAssertEqual(LabeledSlider.clampNote(typed: 3.5, range: 0...2.25, step: 0.05),
                       "\(end) is the most; it was set to \(end).", "The end keeps the slider's decimals")
        XCTAssertNil(LabeledSlider.clampNote(typed: 100, range: -100...100, step: 1), "An end itself is no clamp")
        XCTAssertNil(LabeledSlider.clampNote(typed: 0.5, range: 0...1, step: 0.01))
    }

    func testDecimalsFollowTheStep() {
        XCTAssertEqual(LabeledSlider.decimals(1), 0)
        XCTAssertEqual(LabeledSlider.decimals(0.1), 1)
        XCTAssertEqual(LabeledSlider.decimals(0.05), 2)
        XCTAssertEqual(LabeledSlider.decimals(0.001), 3)
    }

    // MARK: The slider's keys and clicks

    private func key(_ code: UInt16, _ flags: NSEvent.ModifierFlags = [], window: NSWindow? = nil) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
                                       windowNumber: window?.windowNumber ?? 0, context: nil, characters: "", charactersIgnoringModifiers: "",
                                       isARepeat: false, keyCode: code))
    }

    func testArrowsStepAndReturnTypes() throws {
        let slider = InkSliderView(frame: NSRect(x: 0, y: 0, width: 200, height: Theme.Size.row))
        slider.range = -100...100
        slider.step = 1
        var values: [Double] = [], typed = 0
        slider.onChange = { values.append($0); slider.value = $0 }
        slider.onType = { typed += 1 }

        slider.keyDown(with: try key(124))
        XCTAssertEqual(values.last, 1, "→ steps once")
        slider.keyDown(with: try key(124, .shift))
        XCTAssertEqual(values.last, 11, "⇧→ steps ten times")
        slider.keyDown(with: try key(123))
        XCTAssertEqual(values.last, 10, "← steps back")
        slider.keyDown(with: try key(36))
        slider.keyDown(with: try key(76))
        XCTAssertEqual(typed, 2, "Return and Enter start typing")
        XCTAssertEqual(values.count, 3, "Return doesn't change the value")
    }

    func testDoubleClickAndOptionClickReset() throws {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        let slider = InkSliderView(frame: NSRect(x: 20, y: 20, width: 200, height: Theme.Size.row))
        window.contentView?.addSubview(slider)
        slider.range = 0...10
        var resets = 0, changes = 0, editing: [Bool] = []
        slider.onReset = { resets += 1 }
        slider.onChange = { _ in changes += 1 }
        slider.onEditing = { editing.append($0) }
        func mouse(_ type: NSEvent.EventType, clicks: Int, _ flags: NSEvent.ModifierFlags = []) throws -> NSEvent {
            try XCTUnwrap(NSEvent.mouseEvent(with: type, location: NSPoint(x: 120, y: 30), modifierFlags: flags,
                                             timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                             context: nil, eventNumber: 0, clickCount: clicks, pressure: 1))
        }
        slider.mouseDown(with: try mouse(.leftMouseDown, clicks: 1))
        slider.mouseUp(with: try mouse(.leftMouseUp, clicks: 1))
        XCTAssertEqual(changes, 1, "A click moves the knob to it")
        XCTAssertEqual(editing, [true, false])
        slider.mouseDown(with: try mouse(.leftMouseDown, clicks: 2))
        slider.mouseDown(with: try mouse(.leftMouseDown, clicks: 1, .option))
        XCTAssertEqual(resets, 2)
        XCTAssertEqual(changes, 1, "Resetting doesn't drag")

        slider.enabled = false
        slider.mouseDown(with: try mouse(.leftMouseDown, clicks: 2))
        XCTAssertEqual(resets, 2, "A dimmed slider ignores clicks")
        XCTAssertFalse(slider.takesKeyboardFocus)
        XCTAssertFalse(slider.acceptsFirstResponder)
    }

    // MARK: Tab through the inspector

    private func inspectorWindow() -> (NSWindow, NSView, [InkSliderView]) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled], backing: .buffered, defer: false)
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        root.identifier = InkKeyLoop.inspectorIdentifier
        window.contentView = root
        // Added out of order: the loop goes by position, top to bottom, then left to right.
        let frames = [NSRect(x: 20, y: 100, width: 150, height: 29),  // second row
                      NSRect(x: 20, y: 200, width: 150, height: 29),  // first row, left
                      NSRect(x: 200, y: 202, width: 150, height: 29), // first row, right (2 pt off)
                      NSRect(x: 20, y: 20, width: 150, height: 29)]   // last row
        let sliders = frames.map { InkSliderView(frame: $0) }
        let holder = NSView(frame: root.bounds)
        root.addSubview(holder)
        for slider in sliders { holder.addSubview(slider) }
        return (window, root, sliders)
    }

    func testKeyLoopGoesInReadingOrderAndSkipsWhatCantTakeFocus() {
        let (window, root, sliders) = inspectorWindow()
        _ = window
        XCTAssertEqual(InkKeyLoop.targets(in: root), [sliders[1], sliders[2], sliders[0], sliders[3]])
        XCTAssertTrue(InkKeyLoop.root(of: sliders[0]) === root)
        XCTAssertNil(InkKeyLoop.root(of: NSView()))

        sliders[2].enabled = false
        sliders[3].isHidden = true
        XCTAssertEqual(InkKeyLoop.targets(in: root), [sliders[1], sliders[0]], "Dimmed and hidden controls are skipped")
        sliders[3].isHidden = false
        sliders[3].superview?.alphaValue = 0
        XCTAssertEqual(InkKeyLoop.targets(in: root), [], "Nothing under a transparent view")
    }

    func testTabArrowsAndShiftTabWrapRoundTheLoop() throws {
        let (window, root, sliders) = inspectorWindow()
        let order = [sliders[1], sliders[2], sliders[0], sliders[3]]
        XCTAssertTrue(InkKeyLoop.advance(in: root, from: nil, forward: true))
        XCTAssertTrue(window.firstResponder === order[0], "Tab from outside starts at the top")
        order[0].keyDown(with: try key(48, window: window))
        XCTAssertTrue(window.firstResponder === order[1])
        order[1].keyDown(with: try key(125, window: window))
        XCTAssertTrue(window.firstResponder === order[2], "↓ moves on like Tab")
        order[2].keyDown(with: try key(126, window: window))
        XCTAssertTrue(window.firstResponder === order[1], "↑ moves back")
        order[1].keyDown(with: try key(48, .shift, window: window))
        XCTAssertTrue(window.firstResponder === order[0], "⇧Tab moves back")
        order[0].keyDown(with: try key(48, .shift, window: window))
        XCTAssertTrue(window.firstResponder === order[3], "⇧Tab from the first wraps to the last")
        order[3].keyDown(with: try key(48, window: window))
        XCTAssertTrue(window.firstResponder === order[0], "Tab from the last wraps to the first")
        XCTAssertTrue(InkKeyLoop.advance(in: root, from: nil, forward: false))
        XCTAssertTrue(window.firstResponder === order[3], "⇧Tab from outside starts at the bottom")
    }

    // MARK: The focus box is for the keyboard only

    func testFocusBoxFollowsTheLastInput() throws {
        let mode = InputMode.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        func click() throws -> NSEvent {
            try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseDown, location: .zero, modifierFlags: [], timestamp: 0,
                                             windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        }
        mode.note(try click())
        XCTAssertFalse(mode.keyboard)
        mode.note(try key(0, window: window))
        XCTAssertFalse(mode.keyboard, "Typing a letter isn't moving focus")
        mode.note(try key(48, window: window))
        XCTAssertTrue(mode.keyboard, "Tab shows the box")
        mode.note(try click())
        XCTAssertFalse(mode.keyboard, "A click hides it")
        for code: UInt16 in [123, 124, 125, 126] {
            mode.note(try key(code, window: window))
            XCTAssertTrue(mode.keyboard, "Arrow \(code)")
            mode.note(try click())
        }
        XCTAssertFalse(mode.keyboard)
    }

    // MARK: What the Lab keeps back

    private func group(_ title: String) throws -> AdjustmentGroup { try XCTUnwrap(adjustmentGroups.first { $0.title == title }) }

    func testModulesShowTheirBasicsUntilMoreIsClicked() throws {
        let grain = try group("Grain")
        let none = InspectorReveal(all: false)
        XCTAssertEqual(none.controls(grain) { _ in false }.map(\.id), grain.controls.prefix(grain.basic).map(\.id))
        XCTAssertGreaterThan(grain.controls.count, grain.basic, "Grain keeps some back")

        // A changed control past the basics shows anyway.
        let changed = grain.controls[grain.basic]
        XCTAssertEqual(none.controls(grain) { $0.id == changed.id }.map(\.id), grain.controls.prefix(grain.basic).map(\.id) + [changed.id])

        let stored = none.revealing("Grain")
        XCTAssertEqual(stored, "Grain")
        XCTAssertEqual(InspectorReveal(all: false, stored: stored).controls(grain) { _ in false }.count, grain.controls.count)
        XCTAssertEqual(InspectorReveal(all: true).controls(grain) { _ in false }.count, grain.controls.count, "Show All Controls shows them all")
    }

    func testSectionsKeepTheirExpertModulesBehindMore() throws {
        let scanner = try group("Scanner"), enlarger = try group("Enlarger")
        XCTAssertEqual(scanner.basic, 0)
        let none = InspectorReveal(all: false)
        XCTAssertFalse(none.shows(scanner, edits: 0))
        XCTAssertTrue(none.shows(scanner, edits: 1), "A changed module always shows")
        XCTAssertTrue(none.shows(enlarger, edits: 0), "A module with basics always shows")
        XCTAssertFalse(none.shows(section: .scan))

        let key = InspectorReveal.sectionKey(.scan)
        XCTAssertEqual(key, "section:Scan & Color")
        let opened = InspectorReveal(all: false, stored: InspectorReveal(all: false, revealed: ["Grain"]).revealing(key))
        XCTAssertEqual(opened.revealed, ["Grain", key], "Revealing keeps what was revealed before")
        XCTAssertTrue(opened.shows(section: .scan))
        XCTAssertTrue(opened.shows(scanner, edits: 0))
        XCTAssertFalse(opened.shows(section: .print))
        XCTAssertTrue(InspectorReveal(all: true).shows(section: .print))
    }

    func testMoreLineText() {
        XCTAssertNil(InspectorReveal.moreText([]))
        XCTAssertEqual(InspectorReveal.moreText(["Lens blur"]), "1 more: Lens blur")
        XCTAssertEqual(InspectorReveal.moreText(["Scanner", "Color Management"]), "2 more: Scanner, Color Management")
    }

    func testShowAllControlsMenuTogglesTheDefault() throws {
        let suite = "negs-ink-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        XCTAssertFalse(defaults.bool(forKey: AdjustmentInspector.allControlsKey))
        XCTAssertTrue(AdjustmentInspector.toggleAllControls(in: defaults))
        XCTAssertTrue(defaults.bool(forKey: AdjustmentInspector.allControlsKey))
        XCTAssertFalse(AdjustmentInspector.toggleAllControls(in: defaults))
        XCTAssertFalse(defaults.bool(forKey: AdjustmentInspector.allControlsKey))
    }

    // MARK: Over the Export button

    func testExportSummary() {
        let photo = CGSize(width: 6048, height: 4024)
        XCTAssertEqual(ExportFoot.summary(photo: photo, format: .jpeg, quality: 92, chosenSpace: 0), "6048 × 4024 · JPEG 92 · sRGB")
        XCTAssertEqual(ExportFoot.summary(photo: photo, format: .jpeg, quality: 95, chosenSpace: SaveSettings.prophoto),
                       "6048 × 4024 · JPEG 95 · Adobe RGB (1998)", "JPEG is 8-bit, so ProPhoto gives way to Adobe RGB")
        XCTAssertEqual(ExportFoot.summary(photo: .zero, format: .jpeg, quality: 80, chosenSpace: 1), "JPEG 80 · Display P3",
                       "No size before a photograph opens")
        let summary = ExportFoot.summary(photo: photo, format: .tiff, chosenSpace: 0)
        XCTAssertTrue(summary == "6048 × 4024 · TIFF 16-bit · sRGB" || summary == "6048 × 4024 · TIFF 8-bit · sRGB", summary)
    }

    // MARK: Counting edits

    /// The counts on the tabs, sections and modules, and the header's "N edits".
    func testEditCountsFollowChangedControls() async throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-edits-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        await model.start(paths: paths)
        XCTAssertTrue(model.ready, model.error ?? "No engine")

        let enlarger = try group("Enlarger")
        let baseline = InspectorTab.allCases.map { InspectorEdits.count($0, model) }
        XCTAssertEqual(InspectorEdits.count(enlarger, model), 0)

        model.set("enlarger.m_filter_shift", .number(-5))
        XCTAssertEqual(InspectorEdits.count(enlarger, model), 1)
        XCTAssertEqual(InspectorEdits.count(.print, model), 1)
        XCTAssertEqual(InspectorEdits.count(.lab, model), baseline[1] + 1)

        model.set("prep.highlights", .number(-40))
        model.set("prep.shadows", .number(20))
        XCTAssertEqual(InspectorEdits.count(.prep, model), baseline[0] + 2)

        model.border(\.enabled).wrappedValue = true
        XCTAssertEqual(InspectorEdits.count(.output, model), baseline[2] + 1)

        model.resetValue("enlarger.m_filter_shift")
        model.resetValue("prep.shadows")
        XCTAssertEqual(InspectorEdits.count(enlarger, model), 0)
        XCTAssertEqual(InspectorTab.allCases.map { InspectorEdits.count($0, model) }, [baseline[0] + 1, baseline[1], baseline[2] + 1],
                       "A control set back to its default stops counting")
    }
}
