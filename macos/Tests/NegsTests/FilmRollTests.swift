import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

final class FilmRollTests: XCTestCase {
    private let texture = CGSize(width: 3800, height: 3500)
    private let window = CGRect(x: 0.026316, y: 0.157143, width: 0.947368, height: 0.685714)

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
    /// between frames.
    func testFramesAreAWholeNumberOfPointsLong() {
        XCTAssertEqual(FilmRollLayout.frameLength(stripHeight: 120, texture: texture), 130)
    }

    /// The roll runs across, as the artwork does, so its gate is the artwork's.
    func testTheGateLiesAlongTheRoll() {
        let gate = FilmRollLayout.gate(window: window, frame: CGSize(width: 130, height: 120))
        XCTAssertEqual(gate.minX, window.minX * 130, accuracy: 0.001)
        XCTAssertEqual(gate.minY, window.minY * 120, accuracy: 0.001)
        XCTAssertEqual(gate.width, window.width * 130, accuracy: 0.001)
        XCTAssertEqual(gate.height, window.height * 120, accuracy: 0.001)
        XCTAssertGreaterThan(gate.width, gate.height)
    }

    /// As on a real roll, landscape frames sit upright and portrait ones lie on their side.
    func testPortraitFramesLieOnTheirSide() {
        XCTAssertFalse(FilmRollLayout.liesOnItsSide(CGSize(width: 600, height: 400)))
        XCTAssertTrue(FilmRollLayout.liesOnItsSide(CGSize(width: 400, height: 600)))
    }

    /// Blank frames carry a short roll past the strip's end, and a long one needs none.
    func testBlankFramesFillTheStrip() {
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 2, viewport: 700, frameLength: 130), 4)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 0, viewport: 700, frameLength: 130), 6)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 30, viewport: 700, frameLength: 130), 0)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 2, viewport: 0, frameLength: 130), 0)
    }

    /// A double-click finds the photograph under it, on the film past any
    /// scrolling, and only within its gate.
    func testFindingThePhotographUnderAPoint() {
        let frame = CGSize(width: 130, height: 120)
        let gate = FilmRollLayout.gate(window: window, frame: frame)
        func photo(_ x: CGFloat, _ y: CGFloat, photos: Int = 10) -> Int? {
            FilmRollLayout.photo(at: CGPoint(x: x, y: y), frame: frame, gate: gate, photos: photos)
        }
        XCTAssertEqual(photo(65, 60), 0)
        XCTAssertEqual(photo(130 * 3 + 65, 60), 3)
        XCTAssertNil(photo(65, 6), "The sprockets along the top")
        XCTAssertNil(photo(65, 114), "And the bottom")
        XCTAssertNil(photo(130, 60), "The film between frames")
        XCTAssertNil(photo(130 * 3 + 65, 60, photos: 2), "A blank frame")
        XCTAssertNil(photo(-20, 60))
    }

    /// Still films print frame numbers; cine and print films' edges have their
    /// own text there, which must not be painted over.
    func testOnlyStillFilmsAreRenumbered() {
        XCTAssertTrue(FilmRollLayout.printsFrameNumbers(film: "kodak_portra_400"))
        XCTAssertTrue(FilmRollLayout.printsFrameNumbers(film: "fujifilm_c200"))
        XCTAssertTrue(FilmRollLayout.printsFrameNumbers(film: "some_other_film"), "The generic strip has them")
        for film in ["kodak_doublex", "kodak_2302", "kodak_2383", "kodak_2393", "kodak_verita_200d",
                     "kodak_vision3_50d", "kodak_vision3_200t", "kodak_vision3_250d", "kodak_vision3_500t"] {
            XCTAssertFalse(FilmRollLayout.printsFrameNumbers(film: film), film)
        }
    }

    /// The strip for the roll comes small, since it's only ever a thumbnail.
    func testTheStripLoadsAtTheSizeAsked() throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let strip = try XCTUnwrap(FilmStrip.load(data: root.appendingPathComponent("data"), film: "kodak_portra_400", format: .full, maxPixelSize: 400))
        XCTAssertEqual(max(strip.image.width, strip.image.height), 400)
        XCTAssertEqual(strip.pixelSize, texture, "It still knows the file's proportions")
    }

    /// An empty slot is black behind the gate, not whatever's behind the window.
    @MainActor func testAnEmptySlotIsBlack() throws {
        _ = NSApplication.shared
        let host = NSHostingView(rootView: BlankFilmFrame(number: 3, strip: nil, film: "kodak_portra_400"))
        host.frame = NSRect(x: 0, y: 0, width: 195, height: 180)
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let colour = try XCTUnwrap(bitmap.colorAt(x: bitmap.pixelsWide / 2, y: bitmap.pixelsHigh / 2)?.usingColorSpace(.sRGB))
        XCTAssertEqual(colour.alphaComponent, 1, accuracy: 0.01)
        XCTAssertEqual(colour.redComponent, 0, accuracy: 0.01)
    }

    /// Nothing behind the roll shows through a frame: on Proof's white, the edge
    /// of the gate stays dark all the way round, with the artwork loaded.
    @MainActor func testNoLightShowsAroundTheGate() throws {
        _ = NSApplication.shared
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let strip = try XCTUnwrap(FilmStrip.load(data: root.appendingPathComponent("data"), film: "kodak_portra_400", format: .full, maxPixelSize: 400))
        let file = PhotoFile(url: URL(fileURLWithPath: "/nonexistent/frame.png"))
        let frame = FilmFrame(file: file, number: 1, selected: false, strip: strip, film: "kodak_portra_400") {}
        let host = NSHostingView(rootView: frame.background(Color.white))
        let size = CGSize(width: FilmRollLayout.frameLength(stripHeight: FilmRollLayout.stripHeight, texture: strip.pixelSize), height: FilmRollLayout.stripHeight)
        host.frame = NSRect(origin: .zero, size: size)
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let scale = CGFloat(bitmap.pixelsWide) / size.width
        func brightness(_ x: CGFloat, _ y: CGFloat) throws -> CGFloat {
            try XCTUnwrap(bitmap.colorAt(x: Int(x * scale), y: Int(y * scale))?.usingColorSpace(.sRGB)).redComponent
        }
        let gate = FilmRollLayout.gate(window: strip.window, frame: size)
        // Rings just inside, on and just outside each edge of the gate.
        for offset: CGFloat in [-1, -0.5, 0, 0.5, 1] {
            for step in stride(from: gate.minY + 8, to: gate.maxY - 8, by: 8) {
                XCTAssertLessThan(try brightness(gate.minX + offset, step), 0.25, "left \(offset), \(step)")
                XCTAssertLessThan(try brightness(gate.maxX + offset, step), 0.25, "right \(offset), \(step)")
            }
            for step in stride(from: gate.minX + 8, to: gate.maxX - 8, by: 8) {
                XCTAssertLessThan(try brightness(step, gate.minY + offset), 0.25, "top \(offset), \(step)")
                XCTAssertLessThan(try brightness(step, gate.maxY + offset), 0.25, "bottom \(offset), \(step)")
            }
        }
        // The gate's rounded corners too.
        XCTAssertLessThan(try brightness(gate.minX + 0.5, gate.minY + 0.5), 0.25)
        XCTAssertLessThan(try brightness(gate.maxX - 0.5, gate.maxY - 0.5), 0.25)
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
