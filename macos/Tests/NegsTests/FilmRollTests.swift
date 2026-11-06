import AppKit
import ImageIO
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
            FilmRollLayout.revealOffset(start: CGFloat(index) * 195, end: CGFloat(index + 1) * 195, offset: offset, viewport: 1000, maxOffset: 5000)
        }
        XCTAssertNil(reveal(3, offset: 0), "In view")
        XCTAssertNil(reveal(5, offset: 0), "Partly in view: 975 to 1170")
        XCTAssertNil(reveal(0, offset: 190), "Partly in view at the other end")
        XCTAssertEqual(reveal(10, offset: 0), 1950 + 195 - 1000, "Past the right edge")
        XCTAssertEqual(reveal(2, offset: 2000), 390, "Past the left edge")
        XCTAssertEqual(FilmRollLayout.revealOffset(start: 40 * 195, end: 41 * 195, offset: 0, viewport: 1000, maxOffset: 500), 500, "Within the film")
    }

    /// The open photograph stands out by being brighter, not by a mark.
    func testOnlyTheOtherFramesAreDimmed() {
        XCTAssertEqual(FilmRollLayout.photographOpacity(selected: true), 1)
        XCTAssertEqual(FilmRollLayout.photographOpacity(selected: false), 0.5)
    }

    /// A frame is 38 mm of a 35 mm wide film, in whole points so nothing shows
    /// Each frame is as long as its photograph needs, with only the gap around it.
    func testAFrameFitsItsPhotographsShape() {
        let landscape = FilmRollLayout.frameLength(stripHeight: 186, aspect: 3.0 / 2)
        let portrait = FilmRollLayout.frameLength(stripHeight: 186, aspect: 2.0 / 3)
        XCTAssertEqual(landscape, 270 + 6)
        XCTAssertEqual(portrait, 120 + 6)
        XCTAssertEqual(FilmRollLayout.frameEdges(lengths: [10, 20, 5]), [0, 10, 30, 35])
    }

    /// A double-click finds the photograph under it, whatever the frames' shapes,
    /// and only on the photograph.
    func testFindingThePhotographUnderAPoint() {
        // Landscape, portrait, landscape.
        let edges = FilmRollLayout.frameEdges(lengths: [180, 80, 180])
        func photo(_ x: CGFloat, _ y: CGFloat) -> Int? { FilmRollLayout.photo(at: CGPoint(x: x, y: y), edges: edges, height: 120) }
        XCTAssertEqual(photo(90, 60), 0)
        XCTAssertEqual(photo(220, 60), 1)
        XCTAssertEqual(photo(350, 60), 2)
        XCTAssertNil(photo(90, 1), "The black above it")
        XCTAssertNil(photo(90, 119), "And below")
        XCTAssertNil(photo(180, 60), "The gap between frames")
        XCTAssertNil(photo(441, 60), "Past the last photograph")
        XCTAssertNil(photo(-20, 60))
    }

    /// A photograph's shape comes from its metadata, turned by its orientation.
    func testReadingAPhotographsShape() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("negs-aspect-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let context = try XCTUnwrap(CGContext(data: nil, width: 60, height: 40, bitsPerComponent: 8, bytesPerRow: 0,
                                              space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        func write(_ name: String, orientation: Int) throws -> URL {
            let url = directory.appendingPathComponent(name)
            let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString, 1, nil))
            CGImageDestinationAddImage(destination, image, [kCGImagePropertyOrientation: orientation] as CFDictionary)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
            return url
        }
        XCTAssertEqual(try XCTUnwrap(PhotoFolder.aspect(of: try write("up.jpg", orientation: 1))), 1.5, accuracy: 0.001)
        XCTAssertEqual(try XCTUnwrap(PhotoFolder.aspect(of: try write("turned.jpg", orientation: 6))), 2.0 / 3, accuracy: 0.001)
        XCTAssertNil(PhotoFolder.aspect(of: directory.appendingPathComponent("missing.jpg")))
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

    /// Only the frames near the view are built, whatever their lengths, and any
    /// frame that could be on screen is among them.
    func testOnlyFramesNearTheViewAreBuilt() {
        let lengths = (0..<1000).map { CGFloat($0 % 2 == 0 ? 267 : 124) }
        let edges = FilmRollLayout.frameEdges(lengths: lengths)
        XCTAssertEqual(FilmRollLayout.framesInView(edges: [0], band: 0), 0..<0)
        XCTAssertEqual(FilmRollLayout.framesInView(edges: edges, band: 0).lowerBound, 0)
        for offset in stride(from: CGFloat(0), to: edges[1000] - 3000, by: 777) {
            let frames = FilmRollLayout.framesInView(edges: edges, band: FilmRollLayout.band(offset: offset))
            XCTAssertLessThan(frames.count, 100)
            XCTAssertLessThanOrEqual(edges[frames.lowerBound], offset)
            XCTAssertGreaterThanOrEqual(edges[frames.upperBound], offset + 3000, "A wide window stays covered")
        }
        let end = FilmRollLayout.framesInView(edges: edges, band: FilmRollLayout.band(offset: edges[1000]))
        XCTAssertEqual(end.upperBound, 1000)
    }
}
