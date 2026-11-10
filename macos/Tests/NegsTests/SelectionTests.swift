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

    /// The sheet packs photographs in rows at their own proportions (§4.6).
    func testSheetLayoutPacksRowsAtTheirOwnProportions() {
        let layout = SheetLayout(ratios: [1.5, 1.5, 1.5, 1.5, 0.75], width: 1000, target: 176)
        // 968 pt across: three at 176 leave a gap, four overfill; four at
        // 158 is nearer 176 than three at 212, so the row takes four.
        XCTAssertEqual(layout.rows.map(\.range), [0..<4, 4..<5])
        XCTAssertEqual(layout.rows[0].height, 158)
        XCTAssertEqual(layout.frames[0].minX, SheetLayout.side)
        XCTAssertEqual(layout.frames[1].minX, layout.frames[0].maxX + SheetLayout.gap)
        XCTAssertEqual(layout.frames[3].maxX, 1000 - SheetLayout.side, "a full row ends flush")
        XCTAssertEqual(layout.frames[0].width, 237, "each at its own proportions")
        // The last row isn't stretched.
        XCTAssertEqual(layout.rows[1].height, 176)
        XCTAssertEqual(layout.frames[4].size, CGSize(width: 132, height: 176))
        XCTAssertEqual(layout.rows[1].y, SheetLayout.top + 158 + SheetLayout.gap)
        XCTAssertEqual(layout.height, SheetLayout.top + 158 + SheetLayout.gap + 176 + SheetLayout.bottom)

        // Up and down land on the nearest across, as in Finder.
        XCTAssertEqual(layout.vertical(from: 3, by: 1), 4)
        XCTAssertEqual(layout.vertical(from: 4, by: -1), 0)
        XCTAssertNil(layout.vertical(from: 0, by: -1))
        XCTAssertNil(layout.vertical(from: 4, by: 1))
        XCTAssertEqual(layout.row(of: 3), 0)
        XCTAssertEqual(layout.row(of: 4), 1)
        XCTAssertNil(layout.row(of: 5))

        // The rubber band.
        XCTAssertEqual(layout.indices(in: layout.frames[1].union(layout.frames[2]).insetBy(dx: 10, dy: 10)), [1, 2])
        let gap = CGRect(x: 0, y: layout.frames[0].maxY + 2, width: 1000, height: 2)
        XCTAssertEqual(layout.indices(in: gap), [], "between rows touches none")
        XCTAssertEqual(layout.indices(in: CGRect(x: 0, y: 0, width: 1000, height: 2000)), Array(0..<5))

        // A panorama wider than the sheet has a row to itself, shrunk to fit.
        let wide = SheetLayout(ratios: [10, 1.5], width: 500, target: 176)
        XCTAssertEqual(wide.rows.map(\.range), [0..<1, 1..<2])
        XCTAssertEqual(wide.frames[0].width, 468)
        XCTAssertEqual(wide.rows[0].height, 47)
        let empty = SheetLayout(ratios: [], width: 500, target: 176)
        XCTAssertTrue(empty.rows.isEmpty)
        XCTAssertEqual(empty.height, SheetLayout.top + SheetLayout.bottom)
    }

    /// The sentence over the grid and its foot (§4.6), in the app's words.
    func testTheCurrentAndTheSelectedAreRinged() {
        var selection = LibrarySelection()
        XCTAssertFalse(photos.contains { SheetLayout.ringed($0, selection: selection) })
        selection.select(photos[3])
        selection.toggle(photos[4])
        XCTAssertEqual(photos.indices.filter { SheetLayout.ringed(photos[$0], selection: selection) }, [3, 4])
    }

    func testTheSheetSaysWhatItShows() {
        XCTAssertEqual(SheetWords.words(for: LibraryFilter()), [])
        let filter = LibraryFilter(rating: .atLeast(3), flags: [.pick], editedOnly: true)
        XCTAssertEqual(SheetWords.words(for: filter), [SheetWords.Part(lead: "rated ", word: "★★★ or more"),
                                                       SheetWords.Part(lead: "", word: "picks"),
                                                       SheetWords.Part(lead: "", word: "edited")])
        XCTAssertEqual(SheetWords.words(for: LibraryFilter(rating: .atLeast(5))), [SheetWords.Part(lead: "rated ", word: "★★★★★")])
        XCTAssertEqual(SheetWords.words(for: LibraryFilter(rating: .unrated, flags: [.pick, .reject])),
                       [SheetWords.Part(lead: "", word: "unrated"), SheetWords.Part(lead: "", word: "picks or rejected")])
        XCTAssertEqual(SheetWords.count(shown: 12, total: 12, selected: 1, filtered: false), "12 photographs · 1 selected")
        XCTAssertEqual(SheetWords.count(shown: 4, total: 12, selected: 1, filtered: true), "4 of 12 photographs")
        XCTAssertEqual(SheetWords.count(shown: 1, total: 1, selected: 0, filtered: false), "1 photograph")
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

    /// What a delete does, and the alert before it.
    func testDeletePlanAndQuestion() throws {
        let a = URL(fileURLWithPath: "/photos/a.CR3"), b = URL(fileURLWithPath: "/photos/b.CR3")
        let duplicates = { (photo: URL) in photo == a ? ["a2", "a3"] : ["b2"] }
        let name = { (item: URL) in item.variantID.map { "\(item.lastPathComponent) (\($0))" } ?? item.lastPathComponent }
        let one = DeletePlan([a], duplicates: duplicates)
        XCTAssertEqual(one.files, [a])
        XCTAssertEqual(one.following, 2)
        XCTAssertEqual(one.question(name: name, permanent: false), .init(
            message: "Move “a.CR3” to the Trash?",
            information: "The photograph’s file and its Negs rating and edits are moved to the Trash, with its 2 duplicates. You can put it back with Undo or from the Trash.",
            button: "Move to Trash"))
        // A chosen duplicate of a photograph that goes is counted with it.
        let both = DeletePlan([a, a.variant("a2"), b], duplicates: duplicates)
        XCTAssertEqual(both.items, [a, b])
        XCTAssertEqual(both.duplicates, [])
        XCTAssertEqual(both.following, 2)
        XCTAssertEqual(both.question(name: name, permanent: false).message, "Move 2 photographs to the Trash?")
        let copy = DeletePlan([b.variant("b2")], duplicates: duplicates)
        XCTAssertEqual(copy.question(name: name, permanent: false).message, "Remove the duplicate “b.CR3 (b2)”?")
        XCTAssertEqual(copy.actionName, "Remove Duplicate")
        let mixed = DeletePlan([a, b.variant("b2")], duplicates: duplicates)
        XCTAssertEqual(mixed.question(name: name, permanent: false).message, "Move 1 photograph to the Trash and remove 1 duplicate?")
        XCTAssertEqual(mixed.question(name: name, permanent: true).message, "Delete 1 photograph permanently and remove 1 duplicate?")
        XCTAssertEqual(mixed.withoutFiles.items, [b.variant("b2")])
        XCTAssertEqual(EditorModel.next(after: photos[2], in: photos, skipping: [photos[2], photos[3]]), photos[4])
        XCTAssertEqual(EditorModel.next(after: photos[5], in: photos, skipping: [photos[4], photos[5]]), photos[3], "the last goes back")
        XCTAssertNil(EditorModel.next(after: photos[0], in: [photos[0]], skipping: [photos[0]]))
    }

    func testRemovingADuplicate() throws {
        let raw = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a"}, {"id": "b", "rating": 2}]}"#.utf8))
        XCTAssertNil(try PhotoSidecar.removing(variant: "b", from: raw), "nothing left: the file goes")
        let rated = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "rating": 1}, {"id": "b"}]}"#.utf8))
        XCTAssertEqual(try PhotoSidecar.removing(variant: "b", from: rated)?["variants"].array.count, 1)
    }

    /// Edited thumbnails are drawn like the canvas: cropped, turned, inside
    /// the border, 512 px on the long side.
    @MainActor func testThumbnailComposer() throws {
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let context = try XCTUnwrap(CGContext(data: nil, width: 200, height: 100, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                               bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue))
        context.setFillColor(CGColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: 200, height: 100))
        let red = try XCTUnwrap(context.makeImage())
        let plain = try XCTUnwrap(ThumbnailComposer.compose(red, size: CGSize(width: 2000, height: 1000), crop: CropSettings(), cut: false,
                                                            rotation: 0, border: nil, texture: nil, strip: nil, space: space))
        XCTAssertEqual([plain.width, plain.height], [512, 256])
        let turned = try XCTUnwrap(ThumbnailComposer.compose(red, size: CGSize(width: 2000, height: 1000), crop: CropSettings(), cut: false,
                                                             rotation: 1, border: nil, texture: nil, strip: nil, space: space))
        XCTAssertEqual([turned.width, turned.height], [256, 512])
        var border = BorderSettings(); border.enabled = true; border.style = .solid; border.width = 0.1; border.color = .white
        let framed = try XCTUnwrap(ThumbnailComposer.compose(red, size: CGSize(width: 2000, height: 1000), crop: CropSettings(), cut: false,
                                                             rotation: 0, border: border, texture: nil, strip: nil, space: space))
        XCTAssertEqual(max(framed.width, framed.height), 512)
        func pixel(_ image: CGImage, _ x: Int, _ y: Int) -> [UInt8] {
            let data = CFDataGetBytePtr(image.dataProvider!.data!)!
            let at = y * image.bytesPerRow + x * 4
            return [data[at + 2], data[at + 1], data[at]]
        }
        XCTAssertEqual(pixel(framed, 2, 2), [255, 255, 255], "the mat")
        XCTAssertEqual(pixel(framed, framed.width / 2, framed.height / 2), [255, 0, 0], "the photograph")
    }

    func testEditedThumbnailNames() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("negs-thumbs-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = EditedThumbnailStore(directory: dir)
        let photo = PhotoFile(url: URL(fileURLWithPath: "/photos/a.CR3"), values: nil)
        XCTAssertNil(store.key(for: photo, look: "x"), "not before the engine is known")
        store.engineStamp = "engine"
        let dated = try XCTUnwrap({ () -> PhotoFile? in
            let url = dir.appendingPathComponent("a.png")
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            FileManager.default.createFile(atPath: url.path, contents: Data([1]))
            return PhotoFile(url: url)
        }())
        let key = try XCTUnwrap(store.key(for: dated, look: "x"))
        XCTAssertNotEqual(store.key(for: dated, look: "y"), key)
        XCTAssertNotEqual(store.key(for: dated.duplicate("d", number: 2), look: "x"), key, "a duplicate has its own")
        store.engineStamp = "other"
        XCTAssertNotEqual(store.key(for: dated, look: "x"), key, "a new engine draws them again")
        XCTAssertEqual(PhotoSidecar.look(of: nil, defaultsVersion: 1), nil)
        let edits = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"film": "a", "params": {"prep": {"lift": 1, "gain": 2}}}"#.utf8))
        let reordered = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"params": {"prep": {"gain": 2, "lift": 1}}, "film": "a"}"#.utf8))
        XCTAssertEqual(PhotoSidecar.look(of: edits, defaultsVersion: 1), PhotoSidecar.look(of: reordered, defaultsVersion: 1))
        XCTAssertNotEqual(PhotoSidecar.look(of: edits, defaultsVersion: 1), PhotoSidecar.look(of: edits, defaultsVersion: 2))
    }

    /// Past the limit the least recently shown go first: after the default
    /// film changes, the unedited thumbnails developed with the old one.
    func testThumbnailsPrunedLeastRecentlyShownFirst() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("negs-thumbs-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = EditedThumbnailStore(directory: dir)
        let context = try XCTUnwrap(CGContext(data: nil, width: 64, height: 64, bitsPerComponent: 8, bytesPerRow: 0,
                                              space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        context.setFillColor(red: 0.8, green: 0.3, blue: 0.1, alpha: 1); context.fill(CGRect(x: 0, y: 0, width: 64, height: 64))
        let image = try XCTUnwrap(context.makeImage())
        let before = Date(timeIntervalSinceNow: -3 * 86_400)
        // Before the default changed: two unedited and two edited photographs.
        for key in ["old-film-1", "old-film-2", "edited-1", "edited-2"] {
            try store.save(image, as: key)
            try FileManager.default.setAttributes([.modificationDate: before], ofItemAtPath: store.url(for: key).path)
        }
        // Since: the unedited ones developed again, and the edited ones shown.
        for key in ["new-film-1", "new-film-2"] { try store.save(image, as: key) }
        for key in ["edited-1", "edited-2"] { XCTAssertNotNil(store.image(key, maxPixels: 64)) }
        let size = try XCTUnwrap(store.url(for: "edited-1").resourceValues(forKeys: [.fileSizeKey]).fileSize)
        store.prune(limit: size * 6 + 1)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).count, 6, "nothing under the limit")
        // Just under six: down to three quarters, which two make room for.
        store.prune(limit: size * 16 / 3 + 4)
        for key in ["old-film-1", "old-film-2"] { XCTAssertFalse(store.contains(key), key) }
        for key in ["edited-1", "edited-2", "new-film-1", "new-film-2"] { XCTAssertTrue(store.contains(key), key) }

        // Emptied, but for one being written.
        let writing = dir.appendingPathComponent(".new-film-3-temporary.jpg")
        try Data([1]).write(to: writing)
        XCTAssertEqual(store.size(), size * 4)
        store.removeAll()
        XCTAssertEqual(store.size(), 0)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path), [writing.lastPathComponent])
    }

    /// After an update a photograph's thumbnail and kept develop are no
    /// longer found by name; the last engine's stand in until they're made
    /// again, which the pump still sees is needed.
    func testThumbnailsOfThePreviousEngineStandIn() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("negs-thumbs-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let photoURL = dir.appendingPathComponent("a.jpg")
        try Data([1, 2, 3]).write(to: photoURL)
        let photo = PhotoFile(url: photoURL)
        let store = EditedThumbnailStore(directory: dir.appendingPathComponent("thumbnails"))
        func image(_ red: CGFloat) throws -> CGImage {
            let context = try XCTUnwrap(CGContext(data: nil, width: 64, height: 48, bitsPerComponent: 8, bytesPerRow: 0,
                                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
            context.setFillColor(red: red, green: 0.3, blue: 0.1, alpha: 1); context.fill(CGRect(x: 0, y: 0, width: 64, height: 48))
            return try XCTUnwrap(context.makeImage())
        }
        func red(_ image: CGImage?) throws -> Int {
            let image = try XCTUnwrap(image)
            let context = try XCTUnwrap(CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
            context.draw(image, in: CGRect(x: 0, y: 0, width: 1, height: 1))
            return Int(try XCTUnwrap(context.data).load(as: UInt8.self))
        }
        store.engineStamp = "old"
        XCTAssertNil(store.previousKey(for: photo, look: "edits"), "nothing saved yet")
        let old = try XCTUnwrap(store.key(for: photo, look: "edits"))
        try store.save(image(0.9), as: old, loose: store.looseKey(for: photo, look: "edits"), photoSize: CGSize(width: 6000, height: 4500))
        let other = await PhotoThumbnails.shared.image(for: photo, size: .large, look: "other edits", edited: store)
        XCTAssertNil(other, "only for the same edits")

        store.engineStamp = "new"
        let new = try XCTUnwrap(store.key(for: photo, look: "edits"))
        XCTAssertFalse(store.contains(new), "the pump develops it again")
        XCTAssertEqual(store.previousKey(for: photo, look: "edits"), old)
        XCTAssertEqual(store.render(old)?.photoSize, CGSize(width: 6000, height: 4500))
        let standIn = await PhotoThumbnails.shared.image(for: photo, size: .large, look: "edits", edited: store)
        XCTAssertGreaterThan(try red(standIn), 200)
        // Made again: it replaces the stand-in, in memory too.
        try store.save(image(0.1), as: new, loose: store.looseKey(for: photo, look: "edits"))
        let made = await PhotoThumbnails.shared.image(for: photo, size: .large, look: "edits", edited: store)
        XCTAssertLessThan(try red(made), 60)
        XCTAssertEqual(store.previousKey(for: photo, look: "edits"), new, "the link follows the newest")

        // Links go with what they point at, and count for nothing.
        let sizes = try [old, new].map { try XCTUnwrap(store.url(for: $0).resourceValues(forKeys: [.fileSizeKey]).fileSize) }
        XCTAssertEqual(store.size(), sizes[0] + sizes[1])
        try FileManager.default.removeItem(at: store.url(for: new))
        store.prune(limit: 0)
        XCTAssertNil(store.previousKey(for: photo, look: "edits"))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: store.directory.path), [])
    }

    /// A match solved in the background lands only on the edits it was solved for.
    func testPatchOnlyIfEdits() throws {
        let raw = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "edits": {"film": "x"}}]}"#.utf8))
        let old = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"film": "x"}"#.utf8))
        let new = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"film": "y"}"#.utf8))
        let stale = SidecarPatch(variant: "a", edits: .some(new), onlyIfEdits: .some(new), defaultsVersion: 1, app: "t")
        XCTAssertEqual(try stale.applied(to: raw), raw)
        let current = SidecarPatch(variant: "a", edits: .some(new), onlyIfEdits: .some(old), defaultsVersion: 1, app: "t")
        XCTAssertEqual(try current.applied(to: raw)?["variants"].array.first?["edits.film"], .string("y"))
    }
}
