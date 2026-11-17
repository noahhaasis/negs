import AppKit
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

/// Compare two photographs (ticket 46): the ways in and their words, the
/// arrangement, stepping, leaving, and Original ⌄.
@MainActor final class CompareTests: XCTestCase {
    // MARK: Arrangement

    /// The canvas of a 1291 × 860 window with the panel (340) and status line: ~950 × 794.
    private let withPanel = CGSize(width: 950, height: 794)
    /// The same window in Image Only: the whole width, no status line.
    private let imageOnly = CGSize(width: 1291, height: 822)

    func testTwoLandscapesStackWithThePanelOpen() {
        let layout = CompareArrangement.layout(canvas: withPanel, aspects: (1.5, 1.5))
        XCTAssertEqual(layout.arrangement, .column)
        XCTAssertGreaterThan(layout.frames[0].width, 480, "about 500 wide each, as in the mockup")
        XCTAssertLessThan(CompareArrangement.rowLayout(canvas: withPanel, aspects: (1.5, 1.5)).frames[0].width, 440, "against about 390 in a row")
        XCTAssertLessThan(layout.frames[0].maxY, layout.frames[1].minY, "the first on top")
    }

    func testLandscapesInImageOnlyAndPortraitsGoBesideEachOther() {
        XCTAssertEqual(CompareArrangement.layout(canvas: imageOnly, aspects: (1.5, 1.5)).arrangement, .row)
        let portraits = CompareArrangement.layout(canvas: withPanel, aspects: (2.0 / 3, 2.0 / 3))
        XCTAssertEqual(portraits.arrangement, .row)
        XCTAssertLessThan(portraits.frames[0].maxX, portraits.frames[1].minX, "the first at the left")
    }

    func testMixedProportionsTakeTheLargerTotal() {
        let canvas = withPanel
        let mixed = CompareArrangement.layout(canvas: canvas, aspects: (1.5, 2.0 / 3))
        let row = CompareArrangement.rowLayout(canvas: canvas, aspects: (1.5, 2.0 / 3))
        let column = CompareArrangement.columnLayout(canvas: canvas, aspects: (1.5, 2.0 / 3))
        XCTAssertEqual(mixed.arrangement, row.area >= column.area ? .row : .column)
        XCTAssertEqual(mixed.arrangement, .row)
        // Each keeps its own proportions within its half.
        XCTAssertEqual(mixed.frames[0].width / mixed.frames[0].height, 1.5, accuracy: 0.02)
        XCTAssertEqual(mixed.frames[1].width / mixed.frames[1].height, 2.0 / 3, accuracy: 0.02)
    }

    func testFramesStayInsideTheCanvas() {
        for canvas in [withPanel, imageOnly, CGSize(width: 640, height: 500)] {
            for aspects in [(1.5, 1.5), (0.667, 0.667), (1.5, 0.667), (1, 3)] as [(CGFloat, CGFloat)] {
                let layout = CompareArrangement.layout(canvas: canvas, aspects: aspects)
                for frame in layout.frames {
                    XCTAssertGreaterThanOrEqual(frame.minX, 0); XCTAssertLessThanOrEqual(frame.maxX, canvas.width)
                    XCTAssertGreaterThanOrEqual(frame.minY, CompareArrangement.stackedCaption - 1); XCTAssertLessThanOrEqual(frame.maxY, canvas.height)
                }
                XCTAssertFalse(layout.frames[0].intersects(layout.frames[1]))
            }
        }
    }

    // MARK: Stepping and differences

    func testSteppingSkipsTheFirstAndStopsAtTheEnds() {
        let order = ["a", "b", "c", "d", "e"].map { URL(fileURLWithPath: "/roll/\($0).png") }
        let (a, b, c, d, e) = (order[0], order[1], order[2], order[3], order[4])
        XCTAssertEqual(EditorModel.compareStep(from: b, skipping: c, in: order, by: 1), d, "c is the first: skipped")
        XCTAssertEqual(EditorModel.compareStep(from: d, skipping: c, in: order, by: -1), b)
        XCTAssertEqual(EditorModel.compareStep(from: e, skipping: c, in: order, by: 1), nil, "the end stops it")
        XCTAssertEqual(EditorModel.compareStep(from: b, skipping: a, in: order, by: -1), nil, "a is the first, so b is the start")
        // In the shown order, which is what's passed: a filter that hides c.
        XCTAssertEqual(EditorModel.compareStep(from: b, skipping: a, in: [a, b, d, e], by: 1), d)
        // One no longer shown steps on from the first.
        XCTAssertEqual(EditorModel.compareStep(from: c, skipping: a, in: [a, b, d], by: 1), b)
    }

    func testDifferencesAreNamedAsTheStatusLineSaysThem() {
        let a = PhotoFacts(camera: "Nikon Z 6", iso: 100, exposure: 1.0 / 1250, aperture: 2.8, focalLength: 28)
        var b = a
        XCTAssertTrue(CompareDifferences(a, b, film: ("x", "x"), paper: ("p", "p")).isEmpty)
        b.exposure = 1.0 / 640
        XCTAssertEqual(CompareDifferences(a, b, film: ("x", "x"), paper: ("p", "p")).words, "shutter")
        b.iso = 200
        XCTAssertEqual(CompareDifferences(a, b, film: ("x", "y"), paper: ("p", "p")).words, "ISO, shutter and film")
        let duplicate = CompareDifferences(a, a, film: ("kodak_gold_200", "kodak_portra_400"), paper: ("p", "q"))
        XCTAssertEqual(duplicate.words, "film and paper")
        XCTAssertEqual(duplicate.facts, [])
    }

    // MARK: The model

    private var paths: EnginePaths {
        EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                    exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func waitUntil(_ seconds: TimeInterval = 30, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !predicate() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate())
    }

    /// A started model with a folder of `count` photographs, the first open.
    private func model(_ count: Int = 4) async throws -> (EditorModel, [URL]) {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-compare-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        var photos: [URL] = []
        for index in 0..<count {
            let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3,
                                                        hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24))
            for i in 0..<(96 * 64 * 3) { bitmap.bitmapData![i] = UInt8((i * (index + 1)) % 251) }
            let url = directory.appendingPathComponent("DSC_000\(index + 1).png")
            try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: url)
            photos.append(url)
        }
        let defaults = UserDefaults.standard
        let keys = ["autoPlaceNewImages", "sidecars", "advanceAfterRating", LibraryFilter.defaultsKey, OriginalMode.key, "filmstripVisible"]
        let saved = keys.map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(false, forKey: "autoPlaceNewImages")
        defaults.set(true, forKey: "sidecars")
        defaults.removeObject(forKey: LibraryFilter.defaultsKey)
        defaults.removeObject(forKey: "advanceAfterRating")
        let model = EditorModel(sidecars: SidecarStore())
        addTeardownBlock { @MainActor in model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        model.open(photos[0])
        let watching = Task { await model.library.watch(directory) }
        addTeardownBlock { watching.cancel() }
        try await waitUntil { model.library.summariesLoaded && model.library.files.count == count }
        return (model, photos)
    }

    func testTheMenuItemIsWordedByItsTargets() async throws {
        let (model, photos) = try await model()
        // The open photograph alone: against its original, once developed.
        XCTAssertEqual(model.compareOffer(for: photos[0]).title, "Compare with Original")
        try await waitUntil { model.image != nil }
        XCTAssertTrue(model.compareOffer(for: photos[0]).enabled)
        XCTAssertTrue(model.compareOffer(for: photos[0]).withOriginal)
        // One that isn't selected: with the current one, which goes first.
        let with = model.compareOffer(for: photos[2])
        XCTAssertEqual(with.title, "Compare with 1")
        XCTAssertTrue(with.enabled)
        XCTAssertEqual(with.pair?.0, photos[0]); XCTAssertEqual(with.pair?.1, photos[2])
        // Two selected.
        model.click(photos[1], command: true)
        let two = model.compareOffer(for: photos[1])
        XCTAssertEqual(two.title, "Compare 1 and 2")
        XCTAssertTrue(two.enabled)
        // Three or more: there, but off.
        model.click(photos[3], command: true)
        let three = model.compareOffer(for: photos[3])
        XCTAssertEqual(three.title, "Compare Two Photographs")
        XCTAssertFalse(three.enabled)
    }

    /// Grid · Photo · Compare in the title bar, in every view: the one
    /// shown chosen. Leaving compare for the grid hands the grid's sheet
    /// the two photographs' places, to fly them back from.
    func testTheViewSwitchFollowsTheViewAndLeavingHandsOverThePlaces() async throws {
        let (model, photos) = try await model()
        XCTAssertEqual(ViewSwitch.place(of: model), .photo)
        model.showGrid()
        XCTAssertEqual(ViewSwitch.place(of: model), .grid)
        model.click(photos[1], command: true)
        model.toggleCompare()
        XCTAssertEqual(ViewSwitch.place(of: model), .compare)
        let rect = CGRect(x: 10, y: 20, width: 300, height: 200)
        model.places.compare[photos[1]] = ThumbnailPlaces.Place(rect: rect, image: nil)
        model.showGrid()
        XCTAssertNil(model.comparison)
        XCTAssertEqual(ViewSwitch.place(of: model), .grid)
        XCTAssertEqual(model.places.leftCompare[photos[1]], rect)
        // Leaving for the photo view: nothing to fly.
        model.places.leftCompare = [:]
        model.toggleCompare()
        model.places.compare[photos[1]] = ThumbnailPlaces.Place(rect: rect, image: nil)
        model.leaveCompare(toGrid: false)
        XCTAssertEqual(ViewSwitch.place(of: model), .photo)
        XCTAssertTrue(model.places.leftCompare.isEmpty)
    }

    func testTheCompareWordActsWithExactlyTwo() async throws {
        let (model, photos) = try await model()
        model.showGrid()
        XCTAssertFalse(model.compareWord.enabled)
        XCTAssertEqual(model.compareWord.help, "Select a second photograph to compare (⌘-click)")
        XCTAssertFalse(model.canToggleCompare)
        model.click(photos[2], command: true)
        XCTAssertTrue(model.compareWord.enabled)
        XCTAssertEqual(model.compareWord.help, "Compare 1 and 3 (C)")
        XCTAssertTrue(model.canToggleCompare)
        model.click(photos[3], command: true)
        XCTAssertFalse(model.compareWord.enabled)
        XCTAssertEqual(model.compareWord.help, "Compare takes two photographs. Three or more can't be compared.")
    }

    /// From the grid: the two in shown order, the current one edited; the
    /// second steps with ← →, skipping the first; leaving goes back to the
    /// grid with the selection as it was.
    func testComparingFromTheGridStepsAndLeavesAsItWas() async throws {
        let (model, photos) = try await model()
        model.showGrid()
        model.click(photos[2])
        model.click(photos[0], command: true)
        let before = model.selection
        XCTAssertEqual(before.current, photos[2])
        model.toggleCompare()
        let comparison = try XCTUnwrap(model.comparison)
        XCTAssertEqual(comparison.first, .photo(photos[0]), "shown order: 1 before 3")
        XCTAssertEqual(comparison.second, .photo(photos[2]))
        XCTAssertFalse(comparison.activeFirst, "the current photograph is the edited one")
        XCTAssertEqual(model.mode, .photo)
        XCTAssertEqual(model.sourceItem, photos[2])
        XCTAssertNotNil(model.compareSides[.photo(photos[0])], "the other gets its stand-ins and develop")

        // ← → step the second: the edited one here, so it opens once the steps stop.
        model.stepCompared(by: 1)
        XCTAssertEqual(model.comparison?.second, .photo(photos[3]))
        XCTAssertEqual(model.sourceItem, photos[2], "not yet: stand-ins while stepping")
        try await waitUntil(5) { model.sourceItem == photos[3] }
        XCTAssertEqual(model.comparison?.activeFirst, false, "the edited side stays the edited side")
        model.stepCompared(by: 1)
        XCTAssertEqual(model.comparison?.second, .photo(photos[3]), "the end stops it")
        model.stepCompared(by: -1); model.stepCompared(by: -1)
        XCTAssertEqual(model.comparison?.second, .photo(photos[1]), "1 is the first: skipped")
        model.stepCompared(by: -1)
        XCTAssertEqual(model.comparison?.second, .photo(photos[1]))

        // Swap: places change, the same photograph stays edited.
        try await waitUntil(5) { model.sourceItem == photos[1] }
        model.swapCompared()
        XCTAssertEqual(model.comparison?.first, .photo(photos[1]))
        XCTAssertEqual(model.comparison?.activeFirst, true)
        XCTAssertEqual(model.sourceItem, photos[1])

        model.toggleCompare()
        XCTAssertNil(model.comparison)
        XCTAssertEqual(model.mode, .grid, "back where it came from")
        XCTAssertEqual(model.selection.selected, before.selected, "the selection as it was")
        XCTAssertFalse(model.compareEngineRunning, "its helper stops")
    }

    /// A click on the other photograph edits it; the one edited until now
    /// keeps its developed frame on its side. From the strip, leaving goes
    /// back to the photo view.
    func testClickingTheOtherEditsItAndKeepsTheFrame() async throws {
        let (model, photos) = try await model()
        try await waitUntil { model.image != nil && model.previewCurrent }
        let developed = try XCTUnwrap(model.image)
        model.compare(photos[0], photos[1])
        XCTAssertEqual(model.comparison?.activeFirst, true)
        model.activateCompared(first: false)
        XCTAssertEqual(model.sourceItem, photos[1])
        let kept = try XCTUnwrap(model.compareSides[.photo(photos[0])])
        XCTAssertTrue(kept.image === developed, "its last developed frame stays on its side")
        XCTAssertEqual(kept.shown, .developed)
        model.leaveCompare()
        XCTAssertEqual(model.mode, .photo)
        XCTAssertEqual(model.sourceItem, photos[1], "the edited photo open")
    }

    /// The other photograph develops in a helper of its own, after the open one.
    func testTheOtherPhotographDevelops() async throws {
        let (model, photos) = try await model()
        model.compare(photos[0], photos[1])
        let other = try XCTUnwrap(model.compareSides[.photo(photos[1])])
        try await waitUntil { other.shown == .developed }
        XCTAssertTrue(model.previewCurrent, "the edited one went first")
        XCTAssertTrue(model.compareEngineRunning)
        XCTAssertNotNil(model.comparePicture(.photo(photos[1])))
        XCTAssertEqual(model.compareSummary, "Comparing 1 and 2")
        model.leaveCompare()
        XCTAssertFalse(model.compareEngineRunning)
        XCTAssertTrue(model.compareSides.isEmpty)
    }

    /// A flipped photograph shows flipped in Compare, as in the photo view,
    /// and so does its original.
    func testComparedPhotographsShowTheirFlip() async throws {
        let (model, photos) = try await model()
        try await waitUntil { model.image != nil && model.previewCurrent }
        model.flip()
        model.endCrop(apply: true)
        XCTAssertTrue(model.imageFlip)
        model.compare(photos[0], photos[1])
        XCTAssertEqual(model.comparePicture(.photo(photos[0]))?.flip, true)
        XCTAssertEqual(model.openPicture?.flip, true)
        model.leaveCompare()
        model.originalMode = .sideBySide
        model.clickOriginal()
        let comparison = try XCTUnwrap(model.comparison)
        try await waitUntil { model.compareSides[comparison.second]?.shown == .original }
        XCTAssertEqual(model.comparePicture(comparison.second)?.flip, true, "the original, framed as the photograph")
    }

    func testAgainstTheOriginal() async throws {
        let (model, _) = try await model()
        try await waitUntil { model.image != nil }
        model.originalMode = .sideBySide
        model.clickOriginal()
        let comparison = try XCTUnwrap(model.comparison)
        XCTAssertTrue(comparison.againstOriginal)
        XCTAssertEqual(comparison.layout, .sideBySide)
        XCTAssertEqual(model.compareSummary, "Comparing 1 with its original")
        try await waitUntil { model.compareSides[comparison.second]?.shown == .original }
        model.activateCompared(first: false)
        XCTAssertEqual(model.comparison?.originalActive, true, "the panel dims; the developed photo stays open")
        model.stepCompared(by: 1)
        XCTAssertEqual(model.comparison?.second, comparison.second, "← → don't step an original")
    }

    func testOriginalModeIsRememberedAndSplitToggles() async throws {
        let (model, _) = try await model()
        model.originalMode = .split
        XCTAssertEqual(OriginalMode.load(), .split)
        XCTAssertEqual(EditorModel().originalMode, .split, "a new launch reads it")
        try await waitUntil { model.image != nil }
        model.clickOriginal()
        XCTAssertTrue(model.splitting)
        XCTAssertTrue(model.loadingOriginal, "the tag says Loading original until the line's original is in")
        try await waitUntil { model.splitOriginal != nil }
        XCTAssertFalse(model.loadingOriginal)
        model.clickOriginal()
        XCTAssertFalse(model.splitting)
        XCTAssertNil(model.splitOriginal)
        model.originalMode = .hold
        model.clickOriginal()
        XCTAssertFalse(model.splitting, "hold is the press itself")
    }

    /// Esc closes the tray first, then leaves Image Only, then compare.
    func testEscapeClosesTheTrayThenImageOnlyThenCompare() async throws {
        let (model, photos) = try await model()
        _ = NSApplication.shared
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        controller.editorFrame.split.splitView.autosaveName = nil
        window.setFrameAutosaveName("")
        window.orderFront(nil)
        addTeardownBlock { @MainActor in window.orderOut(nil) }
        func escape() throws -> Bool {
            controller.handleKey(try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                                                                windowNumber: window.windowNumber, context: nil, characters: "\u{1b}",
                                                                charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53)))
        }
        try await waitUntil { model.image != nil }
        model.compare(photos[0], photos[1])
        model.toggleImageOnly()
        model.openTray(paper: false)
        let trayOpened = model.tray != nil
        if trayOpened {
            XCTAssertTrue(try escape())
            XCTAssertNil(model.tray)
            XCTAssertNotNil(model.comparison)
        }
        XCTAssertTrue(model.imageOnly)
        XCTAssertTrue(try escape())
        XCTAssertFalse(model.imageOnly)
        XCTAssertNotNil(model.comparison)
        XCTAssertTrue(try escape())
        XCTAssertNil(model.comparison)
        XCTAssertEqual(model.mode, .photo)
    }

    func testTrashingOneOfTheTwoLeaves() async throws {
        let (model, photos) = try await model()
        model.trash = PhotoTrash(available: { _ in true }, trash: { url in url }, remove: { _ in })
        model.compare(photos[0], photos[1])
        XCTAssertNotNil(model.comparison)
        model.delete(model.deletePlan([photos[1]]))
        XCTAssertNil(model.comparison)
    }

    func testFrameNumbersCountTheRoll() async throws {
        let (model, photos) = try await model(3)
        XCTAssertEqual(model.library.frameNumber(of: photos[0]), "1")
        XCTAssertEqual(model.library.frameNumber(of: photos[2]), "3")
        model.duplicate([photos[1]])
        try await waitUntil { model.library.items.count == 4 }
        let duplicate = try XCTUnwrap(model.library.items.first { $0.url.variantID != nil }?.url)
        XCTAssertEqual(model.library.frameNumber(of: duplicate), "2a")
    }
}
