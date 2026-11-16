import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

/// Open in Lightroom (ticket 48): names, Lightroom's date folders, and the
/// cleanup that deletes a TIFF once Lightroom has its copy, or after a week.
final class LightroomTests: XCTestCase {
    private var dir: URL!
    private let fm = FileManager.default

    override func setUpWithError() throws {
        dir = fm.temporaryDirectory.appendingPathComponent("negs-lightroom-\(UUID().uuidString)")
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
    }
    override func tearDownWithError() throws { try? fm.removeItem(at: dir) }

    /// A small TIFF, with an EXIF capture date unless nil.
    @discardableResult
    private func tiff(_ url: URL, captured: String?, width: Int = 8) throws -> URL {
        try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let context = try XCTUnwrap(CGContext(data: nil, width: width, height: 4, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.tiff.identifier as CFString, 1, nil))
        var properties: [CFString: Any] = [:]
        if let captured { properties[kCGImagePropertyExifDictionary] = [kCGImagePropertyExifDateTimeOriginal: captured] }
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return url
    }
    private func size(_ url: URL) throws -> Int { try XCTUnwrap(fm.attributesOfItem(atPath: url.path)[.size] as? Int) }
    /// `Pictures/Lightroom Library.lrlibrary/<catalog>/originals`.
    private func library(_ catalog: String = "e429c73fbb1f419c825671791bb7a120") throws -> URL {
        let originals = dir.appendingPathComponent("Pictures/Lightroom Library.lrlibrary/\(catalog)/originals")
        try fm.createDirectory(at: originals, withIntermediateDirectories: true)
        return originals
    }

    func testNamesAreTheOriginalsWithNegsAfterThem() {
        var options = BatchOptions()
        options.format = OutputFormat.tiff.rawValue
        options.naming = .custom
        options.custom = Lightroom.naming
        XCTAssertEqual(options.fileName(name: "IMG_0412", film: "kodak_gold_200"), "IMG_0412-Negs.tiff", "an ordinary TIFF export as before")
        options.lightroom = .cloud
        XCTAssertEqual(options.fileName(name: "IMG_0412", film: "kodak_gold_200"), "IMG_0412-Negs.tif")
        XCTAssertEqual(options.fileName(name: "IMG_0412 (2)", film: "kodak_gold_200"), "IMG_0412 (2)-Negs.tif", "a duplicate keeps its number")
        XCTAssertEqual(BatchNames.numbered("IMG_0412-Negs.tif") { $0 == "IMG_0412-Negs.tif" }, "IMG_0412-Negs 2.tif")
        XCTAssertEqual(BatchNames.unique(["DSC_1-Negs.tif", "DSC_1-Negs.tif"]), ["DSC_1-Negs.tif", "DSC_1-Negs 2.tif"], "a RAW+JPEG pair sent twice")
    }

    func testAJournalFromBeforeLightroomStillReads() throws {
        // Written by 0.x before `lightroom` existed.
        let old = #"{"format":"jpeg","engine":"reference","edge":"full","pixels":2048,"colorSpace":0,"jpegQuality":95,"naming":"nameFilm","custom":"{name}_{film}","existing":"number","lowMemory":false}"#
        let options = try JSONDecoder().decode(BatchOptions.self, from: Data(old.utf8))
        XCTAssertNil(options.lightroom)
        var sent = options
        sent.lightroom = .cloud
        XCTAssertEqual(try JSONDecoder().decode(BatchOptions.self, from: JSONEncoder().encode(sent)).lightroom, .cloud)
    }

    func testTheStatusLineSaysWhereTheyGo() {
        XCTAssertEqual(StatusText.exporting(1, of: 3), "Exporting 2 of 3 photographs")
        XCTAssertEqual(StatusText.exporting(0, of: 3, to: .cloud), "Sending 1 of 3 photographs to Lightroom")
        XCTAssertEqual(StatusText.exporting(0, of: 1, to: .classic), "Sending 1 of 1 photograph to Lightroom Classic")
        XCTAssertEqual(EditorModel.lightroomTitle(.cloud, count: 1), "Open in Lightroom")
        XCTAssertEqual(EditorModel.lightroomTitle(.cloud, count: 12), "Open 12 Photographs in Lightroom")
    }

    func testCaptureDatesAreReadAsWritten() throws {
        XCTAssertEqual(Lightroom.exifDate("2026:11:15 23:59:58"), DateComponents(year: 2026, month: 11, day: 15))
        XCTAssertNil(Lightroom.exifDate("0000:00:00 00:00:00"))
        XCTAssertNil(Lightroom.exifDate("    :  :     :  :  "))
        XCTAssertNil(Lightroom.exifDate("2026-11"))
        let file = try tiff(dir.appendingPathComponent("a.tif"), captured: "2026:11:15 09:31:00")
        XCTAssertEqual(Lightroom.captureDate(of: file), DateComponents(year: 2026, month: 11, day: 15))
        XCTAssertNil(Lightroom.captureDate(of: try tiff(dir.appendingPathComponent("b.tif"), captured: nil)))
    }

    func testDateFoldersAreTheCaptureDayAndTheDaysEitherSide() {
        XCTAssertEqual(Lightroom.dateFolders(capture: DateComponents(year: 2026, month: 11, day: 15)),
                       ["2026/2026-11-15", "2026/2026-11-14", "2026/2026-11-16"])
        XCTAssertEqual(Lightroom.dateFolders(capture: DateComponents(year: 2027, month: 1, day: 1)),
                       ["2027/2027-01-01", "2026/2026-12-31", "2027/2027-01-02"], "across a year")
        XCTAssertEqual(Lightroom.dateFolders(capture: DateComponents(year: 2028, month: 3, day: 1)).last, "2028/2028-03-02")
        XCTAssertEqual(Lightroom.dateFolders(capture: DateComponents(year: 2028, month: 3, day: 1))[1], "2028/2028-02-29", "a leap day")
        XCTAssertEqual(Lightroom.dateFolders(capture: nil), ["0000/0000-00-00"])
    }

    func testLibrariesAreFoundByGlobbingNotByCatalogID() throws {
        let one = try library("aaa"), two = try library("bbb")
        try fm.createDirectory(at: dir.appendingPathComponent("Pictures/Other.lrlibrary/ccc"), withIntermediateDirectories: true)  // no originals
        try fm.createDirectory(at: dir.appendingPathComponent("Pictures/Photos Library.photoslibrary/ddd/originals"), withIntermediateDirectories: true)
        let found = Lightroom.libraries(pictures: dir.appendingPathComponent("Pictures")).map(\.standardizedFileURL.path).sorted()
        XCTAssertEqual(found, [one, two].map(\.standardizedFileURL.path).sorted())
        XCTAssertEqual(Lightroom.libraries(pictures: dir.appendingPathComponent("Missing")), [])
    }

    func testACopyIsTheSameNameAndSizeInACaptureDateFolder() throws {
        let originals = try library()
        let sent = try tiff(dir.appendingPathComponent("cache/DSC_6945-Negs.tif"), captured: "2026:11:15 23:30:00")
        let bytes = try size(sent)
        let capture = Lightroom.captureDate(of: sent)
        func copied() -> Bool { Lightroom.copied(name: sent.lastPathComponent, size: bytes, capture: capture, libraries: [originals]) }
        XCTAssertFalse(copied(), "nothing there yet")
        // Same name, other size: another photograph (or a copy still being written).
        let other = try tiff(originals.appendingPathComponent("2026/2026-11-15/DSC_6945-Negs.tif"), captured: "2026:11:15 23:30:00", width: 9)
        XCTAssertNotEqual(try size(other), bytes)
        XCTAssertFalse(copied())
        try fm.removeItem(at: other)
        // Two days off: not ours.
        try fm.createDirectory(at: originals.appendingPathComponent("2026/2026-11-17"), withIntermediateDirectories: true)
        try fm.copyItem(at: sent, to: originals.appendingPathComponent("2026/2026-11-17/DSC_6945-Negs.tif"))
        XCTAssertFalse(copied())
        // The next day, as a time zone puts it.
        try fm.createDirectory(at: originals.appendingPathComponent("2026/2026-11-16"), withIntermediateDirectories: true)
        try fm.copyItem(at: sent, to: originals.appendingPathComponent("2026/2026-11-16/DSC_6945-Negs.tif"))
        XCTAssertTrue(copied())
        // Another name in the right folder doesn't count.
        XCTAssertFalse(Lightroom.copied(name: "DSC_6946-Negs.tif", size: bytes, capture: capture, libraries: [originals]))
    }

    func testCleanupDeletesWhatLightroomCopiedAndWhatIsAWeekOld() throws {
        let originals = try library(), cache = dir.appendingPathComponent("cache")
        let added = try tiff(cache.appendingPathComponent("added-Negs.tif"), captured: "2026:11:15 10:00:00")
        let cancelled = try tiff(cache.appendingPathComponent("cancelled-Negs.tif"), captured: "2026:11:15 10:00:00")
        let undated = try tiff(cache.appendingPathComponent("undated-Negs.tif"), captured: nil)
        let old = try tiff(cache.appendingPathComponent("old-Negs.tif"), captured: "2026:11:01 10:00:00")
        let now = Date()
        // Lightroom's copies: one in the capture date's folder, one undated.
        try fm.createDirectory(at: originals.appendingPathComponent("2026/2026-11-15"), withIntermediateDirectories: true)
        try fm.copyItem(at: added, to: originals.appendingPathComponent("2026/2026-11-15/added-Negs.tif"))
        try fm.createDirectory(at: originals.appendingPathComponent("0000/0000-00-00"), withIntermediateDirectories: true)
        try fm.copyItem(at: undated, to: originals.appendingPathComponent("0000/0000-00-00/undated-Negs.tif"))
        // Sent eight days ago (a faked date) and never added.
        try fm.setAttributes([.modificationDate: now.addingTimeInterval(-8 * 24 * 3600)], ofItemAtPath: old.path)
        // Six days: still waits.
        try fm.setAttributes([.modificationDate: now.addingTimeInterval(-6 * 24 * 3600)], ofItemAtPath: cancelled.path)

        let result = Lightroom.cleanUp(folder: cache, libraries: [originals], now: now)
        XCTAssertEqual(result, Lightroom.Cleanup(copied: 2, expired: 1, waiting: 1))
        XCTAssertEqual(try fm.contentsOfDirectory(atPath: cache.path), ["cancelled-Negs.tif"])
        XCTAssertTrue(fm.fileExists(atPath: originals.appendingPathComponent("2026/2026-11-15/added-Negs.tif").path), "Lightroom's copy is never touched")
        // A day later it's a week old too.
        XCTAssertEqual(Lightroom.cleanUp(folder: cache, libraries: [originals], now: now.addingTimeInterval(1.5 * 24 * 3600)),
                       Lightroom.Cleanup(copied: 0, expired: 1, waiting: 0))
        XCTAssertEqual(Lightroom.cleanUp(folder: dir.appendingPathComponent("missing"), libraries: [originals]), Lightroom.Cleanup())
    }

    func testASentFileGetsANameLightroomDoesNotHaveYet() throws {
        let originals = try library(), cache = dir.appendingPathComponent("cache")
        let sent = try tiff(cache.appendingPathComponent("DSC_1-Negs.tif"), captured: "2026:11:15 10:00:00")
        XCTAssertEqual(Lightroom.uniqueInLibrary(sent, libraries: [originals]), sent, "a new name stays")
        // Sent before and added: the same photograph sent again is a new one.
        try tiff(originals.appendingPathComponent("2026/2026-11-14/DSC_1-Negs.tif"), captured: "2026:11:15 10:00:00")
        try tiff(cache.appendingPathComponent("DSC_1-Negs 2.tif"), captured: "2026:11:15 10:00:00")  // waiting from another send
        let renamed = Lightroom.uniqueInLibrary(sent, libraries: [originals])
        XCTAssertEqual(renamed.lastPathComponent, "DSC_1-Negs 3.tif")
        XCTAssertTrue(fm.fileExists(atPath: renamed.path))
        XCTAssertFalse(fm.fileExists(atPath: sent.path))
    }

    func testSizesAndDeletingTheSentCopies() throws {
        let cache = dir.appendingPathComponent("cache")
        XCTAssertNil(Lightroom.size(of: cache))
        try tiff(cache.appendingPathComponent("a-Negs.tif"), captured: nil)
        try Data("keep".utf8).write(to: cache.appendingPathComponent("notes.txt"))
        XCTAssertGreaterThan(try XCTUnwrap(Lightroom.size(of: cache)), 0)
        Lightroom.deleteSent(in: cache)
        XCTAssertEqual(try fm.contentsOfDirectory(atPath: cache.path), ["notes.txt"], "only TIFFs are Negs'")
    }

    func testFoldersAreInCachesAndPictures() {
        XCTAssertTrue(Lightroom.cloudFolder.path.contains("/Library/Caches/"))
        XCTAssertEqual(Lightroom.cloudFolder.lastPathComponent, "Lightroom")
        XCTAssertEqual(Lightroom.classicFolder.lastPathComponent, "Negs for Lightroom")
        XCTAssertEqual(Lightroom.classicFolder.deletingLastPathComponent().lastPathComponent, "Pictures")
    }
}
