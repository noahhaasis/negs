import AppKit
import XCTest
@testable import NegsApp

private let photos = (0..<6).map { URL(fileURLWithPath: "/photos/\($0).png") }

/// The selection's rules: plain, ⌘ and ⇧ clicks, the keys, and the filter.
final class SelectionTests: XCTestCase {
    func testClicks() {
        var selection = LibrarySelection()
        selection.select(photos[1])
        XCTAssertEqual(selection.current, photos[1])
        XCTAssertEqual(selection.selected, [photos[1]])
        // ⌘ adds and removes, leaving the current photograph.
        selection.toggle(photos[3])
        XCTAssertEqual(selection.selected, [photos[1], photos[3]])
        XCTAssertEqual(selection.current, photos[1])
        selection.toggle(photos[1])
        XCTAssertEqual(selection.selected, [photos[1], photos[3]], "the current photograph stays selected")
        // ⇧ adds the range from the last click to what was selected then.
        selection.extend(to: photos[5], in: photos)
        XCTAssertEqual(selection.selected, Set([1, 3, 4, 5].map { photos[$0] }))
        selection.extend(to: photos[2], in: photos)
        XCTAssertEqual(selection.selected, Set([1, 2, 3].map { photos[$0] }), "a second ⇧-click moves the range's end")
        XCTAssertEqual(selection.ordered(photos), [1, 2, 3].map { photos[$0] })
        // A plain click starts over.
        selection.select(photos[4])
        XCTAssertEqual(selection.selected, [photos[4]])
    }

    func testKeys() {
        var selection = LibrarySelection()
        selection.select(photos[2])
        selection.extend(by: 1, in: photos)
        selection.extend(by: 1, in: photos)
        XCTAssertEqual(selection.selected, Set([2, 3, 4].map { photos[$0] }))
        selection.extend(by: -3, in: photos)
        XCTAssertEqual(selection.selected, Set([1, 2].map { photos[$0] }), "back past the anchor, the range turns")
        XCTAssertEqual(selection.current, photos[2])
        selection.extend(by: -10, in: photos)
        XCTAssertEqual(selection.selected, Set([0, 1, 2].map { photos[$0] }), "stops at the first")
        selection.selectAll(photos)
        XCTAssertEqual(selection.selected, Set(photos))
        selection.deselectAll()
        XCTAssertEqual(selection.selected, [photos[2]])
        XCTAssertEqual(selection.current, photos[2])
    }

    func testTheRubberBandAddsToWhatItStartedWith() {
        var selection = LibrarySelection()
        selection.select(photos[0])
        selection.band([photos[3], photos[4]], adding: [])
        XCTAssertEqual(selection.selected, [photos[0], photos[3], photos[4]], "the current photograph stays")
        selection.band([photos[4]], adding: [photos[0], photos[1]])
        XCTAssertEqual(selection.selected, [photos[0], photos[1], photos[4]])
    }

    /// Photographs a rating changed stay shown until the selection moves;
    /// a filter change drops the hidden ones from the selection.
    func testKeptAndPruned() {
        var selection = LibrarySelection()
        selection.select(photos[0])
        selection.extend(to: photos[2], in: photos)
        selection.keep([photos[1], photos[2]])
        XCTAssertEqual(selection.visible, Set([0, 1, 2].map { photos[$0] }))
        selection.toggle(photos[5])
        XCTAssertEqual(selection.visible, [photos[0]], "kept until the selection changes")
        selection.prune(to: [photos[2]])
        XCTAssertEqual(selection.selected, [photos[0], photos[2]], "the current photograph is never pruned")
        XCTAssertEqual(selection.current, photos[0])
    }

    func testGridMetrics() {
        let metrics = GridMetrics(width: 1000, cell: 180)
        // (1000 - 40 + 14) / 194 = 5 columns, centred.
        XCTAssertEqual(metrics.columns, 5)
        XCTAssertEqual(metrics.inset, (1000 - (5 * 180 + 4 * 14)) / 2)
        XCTAssertEqual(metrics.rows(11), 3)
        XCTAssertEqual(metrics.frame(6).minX, metrics.frame(1).minX)
        XCTAssertEqual(metrics.frame(6).minY, GridMetrics.padding + metrics.rowHeight + GridMetrics.spacing)
        // A band over the second row's first two cells, and the gap below it.
        let second = metrics.frame(5).union(metrics.frame(6)).insetBy(dx: 10, dy: 10)
        XCTAssertEqual(metrics.indices(in: second, count: 11), [5, 6])
        let gap = CGRect(x: 0, y: metrics.frame(0).maxY + 2, width: 1000, height: 4)
        XCTAssertEqual(metrics.indices(in: gap, count: 11), [], "between rows touches none")
        XCTAssertEqual(metrics.indices(in: CGRect(x: 0, y: 0, width: 1000, height: 2000), count: 11), Array(0..<11))
        XCTAssertEqual(GridMetrics(width: 50, cell: 360).columns, 1, "always at least one column")
    }

    /// The edit view's tools leave the toolbar in the grid; Open, the
    /// switch and Filter lead in both, so only the rest is replaced.
    @MainActor func testToolbarItemsPerMode() {
        let photo = EditorWindowController.itemIdentifiers(for: .photo), grid = EditorWindowController.itemIdentifiers(for: .grid)
        XCTAssertEqual(Array(photo.prefix(3)), Array(grid.prefix(3)))
        XCTAssertEqual(Array(photo.prefix(4)), [.open, .mode, .filter, .filmstrip])
        XCTAssertTrue(grid.contains(.gridSize) && grid.contains(.export))
        for id in [NSToolbarItem.Identifier.filmstrip, .zoom, .edit, .presets, .adjustments] { XCTAssertFalse(grid.contains(id)) }
    }
}
