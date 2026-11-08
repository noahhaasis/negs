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

    /// Copy Edits' groups: the standard ones leave crop, rotation and how
    /// the file was read; a group not chosen leaves the target as it was.
    func testCopyGroupsMerge() {
        var source = EditState()
        source.film = "kodak_portra_400"; source.paper = "fujifilm_crystal_archive_typeii"
        source.params = .object([
            "prep": .object(["lift": .number(9)]),
            "io": .object(["raw_white_balance": .object(["mode": .string("daylight")]), "rotate_cw": .number(1),
                           "photo_long_side_px": .number(2000), "lens_correction": .object(["enabled": .bool(false)])]),
            "film_render": .object(["grain": .object(["active": .bool(false)])]),
        ])
        source.crop.angle = 3
        source.brightnessMatch = BrightnessMatch(ev: 1.2)
        var target = EditState()
        target.params = .object([
            "prep": .object(["lift": .number(1), "gain": .number(4)]),
            "io": .object(["rotate_cw": .number(2), "lens_correction": .object(["enabled": .bool(true)])]),
        ])
        let merged = EditGroup.merge(EditGroup.standard, from: source, into: target)
        XCTAssertEqual(merged.film, "kodak_portra_400")
        XCTAssertEqual(merged.params["prep.lift"], .number(9))
        XCTAssertEqual(merged.params["prep.gain"], .number(4))
        XCTAssertEqual(merged.params["io.raw_white_balance.mode"], .string("daylight"))
        XCTAssertEqual(merged.params["io.photo_long_side_px"], .number(2000))
        XCTAssertEqual(merged.params["film_render.grain.active"], .bool(false))
        XCTAssertEqual(merged.params["io.rotate_cw"], .number(2), "crop and rotation aren't copied by default")
        XCTAssertEqual(merged.params["io.lens_correction.enabled"], .bool(true), "nor the file's lens correction")
        XCTAssertEqual(merged.crop, target.crop)
        XCTAssertEqual(merged.brightnessMatch, BrightnessMatch(), "on, and solved again for the photograph")

        let everything = EditGroup.merge(Set(EditGroup.allCases), from: source, into: target)
        XCTAssertEqual(everything.params["io.rotate_cw"], .number(1))
        XCTAssertEqual(everything.params["io.lens_correction.enabled"], .bool(false))
        XCTAssertEqual(everything.crop, source.crop)

        let film = EditGroup.merge([.filmAndPaper], from: source, into: target)
        XCTAssertEqual(film.paper, source.paper)
        XCTAssertEqual(film.params, target.params)
        XCTAssertNil(film.brightnessMatch)
    }

    func testDuplicateURLs() {
        let photo = URL(fileURLWithPath: "/photos/IMG_0412.CR3")
        let copy = photo.variant("a1b2c3d4")
        XCTAssertEqual(copy.variantID, "a1b2c3d4")
        XCTAssertNil(photo.variantID)
        XCTAssertEqual(copy.photoFile, photo)
        XCTAssertEqual(copy.path, photo.path)
        XCTAssertEqual(copy.lastPathComponent, "IMG_0412.CR3")
        XCTAssertNotEqual(copy, photo)
        XCTAssertEqual(PhotoSidecar.url(for: copy.photoFile).lastPathComponent, "IMG_0412.CR3.negs")
        XCTAssertEqual(copy.variant("ff").variantID, "ff")
    }
}
