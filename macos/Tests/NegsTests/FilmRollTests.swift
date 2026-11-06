import AppKit
import XCTest
@testable import NegsApp

final class FilmRollTests: XCTestCase {
    private let frame = CGSize(width: FilmRollLayout.frameLength(stripHeight: 120), height: 120)

    /// A 3:2 photograph and its gap, in whole points.
    func testFramesAreAWholeNumberOfPointsLong() {
        XCTAssertEqual(FilmRollLayout.frameLength(stripHeight: 120), (114 * 1.5).rounded() + 6)
        XCTAssertEqual(FilmRollLayout.frameLength(stripHeight: 121), FilmRollLayout.frameLength(stripHeight: 121).rounded())
    }

    /// The photograph has black all round it.
    func testThePhotographHasAGapAllRound() {
        let gate = FilmRollLayout.gate(frame: frame)
        XCTAssertEqual(gate.minX, 3)
        XCTAssertEqual(gate.minY, 3)
        XCTAssertEqual(gate.maxX, frame.width - 3)
        XCTAssertEqual(gate.maxY, frame.height - 3)
    }

    /// The strip stays where it's been put while the open frame shows at all.
    /// Wholly out of view, the frame comes just far enough in, at the nearer edge.
    func testTheStripMovesOnlyToRevealAFrameOutOfView() {
        func reveal(_ index: Int, offset: CGFloat) -> CGFloat? {
            FilmRollLayout.revealOffset(frame: index, frameLength: 195, offset: offset, viewport: 1000, maxOffset: 5000)
        }
        XCTAssertNil(reveal(3, offset: 0), "In view")
        XCTAssertNil(reveal(5, offset: 0), "Partly in view: 975 to 1170")
        XCTAssertNil(reveal(0, offset: 190), "Partly in view at the other end")
        XCTAssertEqual(reveal(10, offset: 0), 1950 + 195 - 1000, "Past the right edge")
        XCTAssertEqual(reveal(2, offset: 2000), 390, "Past the left edge")
        XCTAssertEqual(FilmRollLayout.revealOffset(frame: 40, frameLength: 195, offset: 0, viewport: 1000, maxOffset: 500), 500, "Within the film")
    }

    /// The open photograph stands out by being brighter, not by a mark.
    func testOnlyTheOtherFramesAreDimmed() {
        XCTAssertEqual(FilmRollLayout.photographOpacity(selected: true), 1)
        XCTAssertEqual(FilmRollLayout.photographOpacity(selected: false), 0.5)
    }

    /// A frame is 38 mm of a 35 mm wide film, in whole points so nothing shows
    /// A double-click finds the photograph under it, on the strip past any
    /// scrolling, and only within its gate.
    func testFindingThePhotographUnderAPoint() {
        let frame = CGSize(width: 180, height: 120)
        let gate = FilmRollLayout.gate(frame: frame)
        func photo(_ x: CGFloat, _ y: CGFloat, photos: Int = 10) -> Int? {
            FilmRollLayout.photo(at: CGPoint(x: x, y: y), frame: frame, gate: gate, photos: photos)
        }
        XCTAssertEqual(photo(90, 60), 0)
        XCTAssertEqual(photo(180 * 3 + 90, 60), 3)
        XCTAssertNil(photo(90, 1), "The black above it")
        XCTAssertNil(photo(90, 119), "And below")
        XCTAssertNil(photo(180, 60), "The gap between frames")
        XCTAssertNil(photo(180 * 3 + 90, 60, photos: 2), "Past the last photograph")
        XCTAssertNil(photo(-20, 60))
    }

    /// RAW only keeps RAW files, however their extensions are written, in order.
    func testTheRawFilterKeepsOnlyRawFiles() {
        let files = ["a.NEF", "b.jpg", "c.cr3", "d.tif", "e.png", "f.dng"].map { PhotoFile(url: URL(fileURLWithPath: "/roll/\($0)")) }
        XCTAssertEqual(PhotoFolder.shown(files, rawOnly: true).map(\.name), ["a.NEF", "c.cr3", "f.dng"])
        XCTAssertEqual(PhotoFolder.shown(files, rawOnly: false), files)
    }

    /// Four columns fill the width, with 3:2 boxes and room for two lines.
    func testTheGridsLayout() {
        let width = PhotoGridLayout.cellWidth(available: 1000)
        XCTAssertEqual(width, 220)
        XCTAssertEqual(width * 4 + PhotoGridLayout.columnGap * 3 + PhotoGridLayout.margin * 2, 1000)
        XCTAssertEqual(PhotoGridLayout.boxHeight(cellWidth: 220), 147)
        XCTAssertEqual(PhotoGridLayout.cellHeight(cellWidth: 220), 147 + 40)
    }

    /// A photograph fits its box, centred, whatever its shape.
    func testAPhotographFitsItsBox() {
        let box = CGSize(width: 300, height: 200)
        XCTAssertEqual(PhotoGridLayout.photoRect(image: CGSize(width: 600, height: 400), box: box), CGRect(x: 0, y: 0, width: 300, height: 200))
        XCTAssertEqual(PhotoGridLayout.photoRect(image: CGSize(width: 400, height: 600), box: box).size.height, 200, accuracy: 0.01)
        XCTAssertEqual(PhotoGridLayout.photoRect(image: CGSize(width: 400, height: 600), box: box).minX, (300 - 200 * 400 / 600) / 2, accuracy: 0.01)
        XCTAssertEqual(PhotoGridLayout.photoRect(image: CGSize(width: 800, height: 200), box: box).minY, (200 - 75) / 2, accuracy: 0.01)
    }

    func testTheMiddleOfTheCellsOnScreen() {
        XCTAssertEqual(PhotoGridLayout.middle(of: [4, 2, 3, 8, 5]), 4)
        XCTAssertNil(PhotoGridLayout.middle(of: []))
    }

    /// Frames are turned as their photograph is.
    func testAFrameTurnsWithItsPhotograph() throws {
        let context = try XCTUnwrap(CGContext(data: nil, width: 40, height: 20, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        context.setFillColor(CGColor(gray: 0, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 40, height: 20))
        // White on the left half.
        context.setFillColor(CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 20, height: 20))
        let image = try XCTUnwrap(context.makeImage())
        let turned = try XCTUnwrap(RollDeveloper.turned(image, quarterTurns: 1))
        XCTAssertEqual(turned.width, 20)
        XCTAssertEqual(turned.height, 40)
        let bitmap = NSBitmapImageRep(cgImage: turned)
        // A quarter clockwise puts the left half on top.
        XCTAssertGreaterThan(try XCTUnwrap(bitmap.colorAt(x: 10, y: 5)?.usingColorSpace(.sRGB)).redComponent, 0.9)
        XCTAssertLessThan(try XCTUnwrap(bitmap.colorAt(x: 10, y: 35)?.usingColorSpace(.sRGB)).redComponent, 0.1)
        XCTAssertTrue(RollDeveloper.turned(image, quarterTurns: 4) === image)
    }

    /// Past its limit, the cache gives up its least recently used frames.
    func testTheFrameCachePrunesTheOldest() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("negs-prune-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        for (index, name) in ["old", "middle", "new"].enumerated() {
            let file = dir.appendingPathComponent(name)
            try Data(count: 100).write(to: file)
            try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSince1970: Double(1_000_000 + index))], ofItemAtPath: file.path)
        }
        RollDeveloper.prune(dir, limit: 250)
        XCTAssertEqual(Set(try FileManager.default.contentsOfDirectory(atPath: dir.path)), ["middle", "new"])
    }
}

/// The drag and its coast, on a plain scroll view 2000 points of film long.
@MainActor final class FilmRollScrollerTests: XCTestCase {
    private func makeScroller() -> (FilmRollScroller, NSScrollView) {
        _ = NSApplication.shared
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 500, height: 120))
        scroll.documentView = NSView(frame: NSRect(x: 0, y: 0, width: 2000, height: 120))
        let scroller = FilmRollScroller()
        scroller.scroll = scroll
        return (scroller, scroll)
    }

    /// Dragging left moves the film left, under the pointer.
    func testTheFilmFollowsThePointer() {
        let (scroller, _) = makeScroller()
        scroller.offset = 300
        scroller.press()
        scroller.drag(translation: -100, time: 0)
        XCTAssertEqual(scroller.offset, 400)
        scroller.drag(translation: -160, time: 0.05)
        XCTAssertEqual(scroller.offset, 460)
        scroller.drag(translation: 40, time: 0.1)
        XCTAssertEqual(scroller.offset, 260)
    }

    /// A flick keeps its speed, and it falls away like macOS's.
    func testAFlickCoastsAndSlowsDown() {
        let (scroller, _) = makeScroller()
        scroller.offset = 300
        scroller.press()
        scroller.drag(translation: 0, time: 1.0)
        scroller.drag(translation: -50, time: 1.05)
        scroller.release(time: 1.06, animate: false)
        XCTAssertEqual(scroller.velocity, 1000, accuracy: 0.01)
        let start = scroller.offset
        scroller.advance(seconds: 0.1)
        XCTAssertEqual(scroller.offset, start + 100, accuracy: 0.01)
        XCTAssertEqual(scroller.velocity, 1000 * pow(0.998, 100), accuracy: 0.01)
        for _ in 0..<2000 where scroller.isCoasting { scroller.advance(seconds: 1.0 / 120) }
        XCTAssertFalse(scroller.isCoasting, "It comes to rest")
        XCTAssertGreaterThan(scroller.offset, start + 300, "After travelling")
    }

    /// Holding still before letting go means no flick.
    func testAPauseBeforeReleaseDoesNotCoast() {
        let (scroller, _) = makeScroller()
        scroller.offset = 300
        scroller.press()
        scroller.drag(translation: 0, time: 1.0)
        scroller.drag(translation: -50, time: 1.05)
        scroller.release(time: 1.3, animate: false)
        XCTAssertFalse(scroller.isCoasting)
    }

    /// Only the last moments of a drag set its speed.
    func testOnlyTheLastMomentsSetTheSpeed() {
        let (scroller, _) = makeScroller()
        scroller.offset = 300
        scroller.press()
        scroller.drag(translation: 0, time: 1.0)
        scroller.drag(translation: -400, time: 1.1)
        // Then slow, for longer than the window it looks back over.
        for step in 1...8 { scroller.drag(translation: -400 - CGFloat(step) * 2, time: 1.1 + Double(step) * 0.02) }
        scroller.release(time: 1.27, animate: false)
        XCTAssertEqual(scroller.velocity, 100, accuracy: 1)
    }

    /// The ends of the roll stop it, with nothing past them.
    func testTheEndsOfTheRollStopIt() {
        let (scroller, _) = makeScroller()
        scroller.offset = 1400
        scroller.press()
        scroller.drag(translation: 0, time: 1.0)
        scroller.drag(translation: -100, time: 1.02)
        scroller.release(time: 1.03, animate: false)
        for _ in 0..<600 where scroller.isCoasting { scroller.advance(seconds: 1.0 / 120) }
        XCTAssertEqual(scroller.offset, 1500, "The last of the film")
        XCTAssertFalse(scroller.isCoasting)
        scroller.press()
        scroller.drag(translation: 0, time: 2)
        scroller.drag(translation: 9000, time: 2.02)
        XCTAssertEqual(scroller.offset, 0)
    }

    /// A press stops a coast dead.
    func testAPressStopsIt() {
        let (scroller, _) = makeScroller()
        scroller.offset = 300
        scroller.press()
        scroller.drag(translation: 0, time: 1)
        scroller.drag(translation: -50, time: 1.05)
        scroller.release(time: 1.06, animate: false)
        XCTAssertTrue(scroller.isCoasting)
        scroller.press()
        XCTAssertFalse(scroller.isCoasting)
    }
}
