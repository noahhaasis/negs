import XCTest
@testable import NegsApp

/// Enlarger › Shifts move: the other shift follows by the same change, so
/// nothing jumps on switching, and both stop at either end.
final class FilterLinkTests: XCTestCase {
    let m = FilterLink.magenta, y = FilterLink.yellow

    func testSeparatelyMovesOnlyTheSlider() {
        let r = FilterLink.separate.shifts(moving: m, to: 15, magenta: 10, yellow: -5)
        XCTAssertEqual(r.magenta, 15); XCTAssertEqual(r.yellow, -5)
    }

    func testTogetherKeepsTheGap() {
        let r = FilterLink.together.shifts(moving: m, to: 15, magenta: 10, yellow: -5)
        XCTAssertEqual(r.magenta, 15); XCTAssertEqual(r.yellow, 0)
        let s = FilterLink.together.shifts(moving: y, to: -8, magenta: 10, yellow: -5)
        XCTAssertEqual(s.magenta, 7); XCTAssertEqual(s.yellow, -8)
    }

    func testOppositeKeepsTheSum() {
        let r = FilterLink.opposite.shifts(moving: m, to: 15, magenta: 10, yellow: -5)
        XCTAssertEqual(r.magenta, 15); XCTAssertEqual(r.yellow, -10)
    }

    func testBothStopAtEitherEnd() {
        // Yellow reaches 50 after 10 of the 30 asked.
        let r = FilterLink.together.shifts(moving: m, to: 30, magenta: 0, yellow: 40)
        XCTAssertEqual(r.magenta, 10); XCTAssertEqual(r.yellow, 50)
        let s = FilterLink.opposite.shifts(moving: y, to: 30, magenta: -45, yellow: 0)
        XCTAssertEqual(s.magenta, -50); XCTAssertEqual(s.yellow, 5)
        // Moving the other way is never held back.
        let t = FilterLink.together.shifts(moving: m, to: -10, magenta: 0, yellow: 50)
        XCTAssertEqual(t.magenta, -10); XCTAssertEqual(t.yellow, 40)
    }

    func testAnUnsteppedBalanceKeepsItsGapExactly() {
        // Balance Print writes unrounded shifts; the follower isn't snapped.
        let r = FilterLink.together.shifts(moving: m, to: 4, magenta: 3.4567, yellow: -1.2345)
        XCTAssertEqual(r.magenta - r.yellow, 3.4567 + 1.2345, accuracy: 1e-9)
    }
}

/// Resetting linked shifts: both reset, as they move, by any route; a
/// double-click's first click (which moved both) is one undo step with the
/// reset. Separately, only the clicked one resets and the other stays put.
@MainActor final class FilterLinkResetTests: XCTestCase {
    func testLinkedShiftsResetTogether() async throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
                                data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-link-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let saved = UserDefaults.standard.string(forKey: FilterLink.key)
        defer { UserDefaults.standard.set(saved, forKey: FilterLink.key) }
        let model = EditorModel(previewCache: RenderedPreviewCache(file: directory.appendingPathComponent("preview.cache")))
        defer { model.shutdown() }
        await model.start(paths: paths)
        XCTAssertTrue(model.ready, model.error ?? "No engine")

        // Each block stands for one event: the app's undo manager groups by
        // event, closing the group as the run loop's pass ends.
        let undo = UndoManager(); model.undoManager = undo
        func event(_ body: () -> Void) {
            body()
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
            XCTAssertEqual(undo.groupingLevel, 0)
        }
        let (m, y) = (FilterLink.magenta, FilterLink.yellow)
        func shifts() -> [Double] { [model.value(m).double, model.value(y).double] }

        let zero = [model.defaultValue(m).double, model.defaultValue(y).double]
        model.filterLink = .together
        event { model.set(m, .number(3.4567)) }
        event { model.set(y, .number(-1.2345)) }
        let before = model.state

        // The ↺ button, the label, the menu, ⌥-click.
        event { model.resetFilterShift(y) }
        XCTAssertEqual(shifts(), zero, "Together resets both")
        event { undo.undo() }
        XCTAssertEqual(model.state, before)
        model.filterLink = .opposite
        event { model.resetFilterShift(m) }
        XCTAssertEqual(shifts(), zero, "Opposite ways resets both")
        event { undo.undo() }
        XCTAssertEqual(model.state, before)

        // A double-click on the track.
        model.filterLink = .together
        event { model.beginInteraction(); model.filterShift(m, step: 0.1).wrappedValue = 20; model.endInteraction() }
        XCTAssertEqual(shifts()[1], -1.2345 + (20 - 3.4567), accuracy: 1e-9, "The first click moves yellow along")
        event { model.resetFilterShiftAfterClick(m) }
        XCTAssertEqual(shifts(), zero)
        event { undo.undo() }
        XCTAssertEqual(model.state, before, "One undo goes back to before the double-click")

        // Separately: the first click moved only magenta; yellow stays put.
        model.filterLink = .separate
        event { model.beginInteraction(); model.filterShift(m, step: 0.1).wrappedValue = 20; model.endInteraction() }
        event { model.resetFilterShiftAfterClick(m) }
        XCTAssertEqual(shifts(), [zero[0], -1.2345])
        event { model.resetFilterShift(y) }
        XCTAssertEqual(shifts(), zero)
    }
}
