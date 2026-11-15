import AppKit
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

/// The small states (§4.14) and the behaviour they bring (§6.1, §6.10):
/// folders open, and a photograph that can't be developed says so on the
/// canvas instead of in an alert.
@MainActor final class SmallStatesTests: XCTestCase {
    private func folder() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-states-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func photo(in dir: URL, _ name: String) throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        for i in 0..<(96 * 64 * 3) { bitmap.bitmapData![i] = UInt8(i % 251) }
        let url = dir.appendingPathComponent(name)
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !predicate() && Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate())
    }

    private func model() async throws -> EditorModel {
        let paths = EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                                exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages", "sidecars"].map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(false, forKey: "autoPlaceNewImages")
        defaults.set(false, forKey: "sidecars")
        let model = EditorModel(sidecars: SidecarStore())
        addTeardownBlock { @MainActor in model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        return model
    }

    // MARK: Folders (§6.1)

    func testTheOpenPanelTakesFoldersAsWellAsPhotographs() {
        let panel = EditorModel.makeOpenPanel()
        XCTAssertTrue(panel.canChooseDirectories)
        XCTAssertTrue(panel.canChooseFiles)
        XCTAssertFalse(panel.allowsMultipleSelection)
    }

    func testAFoldersFirstPhotographIsTheFirstByName() throws {
        let dir = try folder()
        _ = try photo(in: dir, "img_10.png")
        let first = try photo(in: dir, "img_2.png")
        try Data("not a photograph".utf8).write(to: dir.appendingPathComponent("a-notes.txt"))
        try FileManager.default.createDirectory(at: dir.appendingPathComponent("a-subfolder"), withIntermediateDirectories: true)
        XCTAssertEqual(EditorModel.firstPhotograph(in: dir)?.lastPathComponent, first.lastPathComponent, "names sort as Finder does; other files and folders don't count")
        XCTAssertNil(EditorModel.firstPhotograph(in: dir.appendingPathComponent("a-subfolder")))
    }

    func testOpeningAFolderOpensItsFirstPhotographInTheGrid() async throws {
        let dir = try folder()
        let first = try photo(in: dir, "a.png")
        _ = try photo(in: dir, "b.png")
        let model = try await model()
        model.open(dir)
        XCTAssertEqual(model.source?.lastPathComponent, first.lastPathComponent)
        XCTAssertEqual(model.mode, .grid)
        XCTAssertNil(model.error)
    }

    func testOpeningAFolderWithoutPhotographsSaysSo() async throws {
        let dir = try folder()
        let model = try await model()
        model.open(dir)
        XCTAssertNil(model.source)
        XCTAssertEqual(model.error, "“\(dir.lastPathComponent)” has no photographs Negs can open.")
    }

    // MARK: Can't develop (§6.10)

    func testAPhotographThatCantBeDevelopedSaysSoWithoutAnAlert() async throws {
        let dir = try folder()
        let broken = dir.appendingPathComponent("DSC_0001.NEF")
        try Data((0..<200_000).map { UInt8(truncatingIfNeeded: $0 &* 2_654_435_761 >> 13) }).write(to: broken)
        let good = try photo(in: dir, "good.png")
        let model = try await model()
        model.open(broken)
        try await waitUntil { model.renderFailed }
        XCTAssertNil(model.error, "said on the canvas, not in an alert")
        XCTAssertFalse(model.developFailure?.isEmpty ?? true, "with the app's reason")
        XCTAssertNil(DevelopTagTitle.title(source: true, failed: model.renderFailed, developed: model.image != nil, slow: model.showsActivity, loadingOriginal: false))

        // Try Again develops it again, and it fails again the same way.
        model.retryDevelop()
        XCTAssertFalse(model.renderFailed, "cleared while it tries")
        try await waitUntil { model.renderFailed }
        XCTAssertNil(model.error)

        // Another photograph develops as usual.
        model.open(good)
        XCTAssertFalse(model.renderFailed)
        try await waitUntil { model.previewCurrent }
        XCTAssertNil(model.developFailure)
    }

    // MARK: Words and places

    func testTheTagSaysDevelopingThenUpdatingOnlyWhenSlow() {
        XCTAssertNil(DevelopTagTitle.title(source: false, failed: false, developed: false, slow: false, loadingOriginal: false), "nothing open")
        XCTAssertEqual(DevelopTagTitle.title(source: true, failed: false, developed: false, slow: false, loadingOriginal: false), "Developing")
        XCTAssertNil(DevelopTagTitle.title(source: true, failed: false, developed: true, slow: false, loadingOriginal: false))
        XCTAssertEqual(DevelopTagTitle.title(source: true, failed: false, developed: true, slow: true, loadingOriginal: false), "Updating")
        XCTAssertEqual(DevelopTagTitle.title(source: true, failed: false, developed: true, slow: true, loadingOriginal: true), "Loading original",
                       "a held original still decoding says so over an update")
        XCTAssertNil(DevelopTagTitle.title(source: true, failed: true, developed: true, slow: true, loadingOriginal: false), "a failure says itself")
    }

    func testTrimMarksReachTwentySixPointsOutsideTheCorners() {
        let print = CGRect(x: 100, y: 50, width: 300, height: 200)
        let marks = TrimMarks.marks(beside: print)
        XCTAssertEqual(marks.count, 4)
        for mark in marks {
            XCTAssertEqual(mark.size, CGSize(width: 18, height: 1.5))
            XCTAssertTrue(mark.minX == print.minX - 26 || mark.maxX == print.maxX + 26)
            XCTAssertTrue(mark.minY == print.minY || mark.maxY == print.maxY, "level with the print's edges")
        }
    }

    func testTheEmptyPrintIsALandscapeBetweenTheBands() {
        let size = CGSize(width: 900, height: 700)
        let place = EmptyPrint.place(in: size)
        XCTAssertEqual(place.width / place.height, 1.5, accuracy: 0.01)
        XCTAssertGreaterThanOrEqual(place.minX, ImageScrollView.fitInset)
        XCTAssertGreaterThanOrEqual(place.minY, CanvasArea.captionBand)
        XCTAssertLessThanOrEqual(place.maxY, size.height - CanvasArea.labelBand)
        XCTAssertEqual(place.midX, size.width / 2, accuracy: 1)
    }
}
