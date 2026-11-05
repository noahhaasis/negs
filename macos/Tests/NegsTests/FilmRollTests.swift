import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

final class FilmRollTests: XCTestCase {
    private let texture = CGSize(width: 3800, height: 3500)
    private let window = CGRect(x: 0.026316, y: 0.157143, width: 0.947368, height: 0.685714)

    /// The open photograph stands out by being brighter, not by a mark.
    func testOnlyTheOtherFramesAreDimmed() {
        XCTAssertEqual(FilmRollLayout.photographOpacity(selected: true), 1)
        XCTAssertEqual(FilmRollLayout.photographOpacity(selected: false), 0.3)
    }

    /// The column is the film, with no margin around it.
    func testTheColumnIsJustTheFilm() {
        XCTAssertEqual(FilmRollLayout.columnWidth, FilmRollLayout.stripWidth)
    }

    /// Blank frames carry a short roll past the column's bottom, and a long one needs none.
    func testBlankFramesFillTheColumn() {
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 2, viewport: 700, frameHeight: 163), 3)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 0, viewport: 700, frameHeight: 163), 5)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 5, viewport: 700, frameHeight: 163), 0)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 30, viewport: 700, frameHeight: 163), 0)
        XCTAssertEqual(FilmRollLayout.blankFrames(photos: 2, viewport: 0, frameHeight: 163), 0)
    }

    /// A frame is 38 mm of a 35 mm wide film, in whole points so no light shows
    /// between frames.
    func testFramesAreAWholeNumberOfPointsLong() {
        XCTAssertEqual(FilmRollLayout.frameHeight(stripWidth: 150, texture: texture), 163)
    }

    /// The artwork's strip runs across; the roll runs down, turned a quarter
    /// clockwise, so its gate stands upright.
    func testTheGateStandsUprightOnTheRoll() {
        let gate = FilmRollLayout.gate(window: window, frame: CGSize(width: 150, height: 163))
        XCTAssertEqual(gate.minX, (1 - window.maxY) * 150, accuracy: 0.001)
        XCTAssertEqual(gate.minY, window.minX * 163, accuracy: 0.001)
        XCTAssertEqual(gate.width, window.height * 150, accuracy: 0.001)
        XCTAssertEqual(gate.height, window.width * 163, accuracy: 0.001)
        XCTAssertGreaterThan(gate.height, gate.width)
    }

    /// As on a real roll held up, landscape frames lie on their side and
    /// portrait ones stand up.
    func testLandscapeFramesLieOnTheirSide() {
        XCTAssertTrue(FilmRollLayout.liesOnItsSide(CGSize(width: 600, height: 400)))
        XCTAssertFalse(FilmRollLayout.liesOnItsSide(CGSize(width: 400, height: 600)))
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
        host.frame = NSRect(x: 0, y: 0, width: 150, height: 163)
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
        let size = CGSize(width: 150, height: FilmRollLayout.frameHeight(stripWidth: 150, texture: strip.pixelSize))
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
