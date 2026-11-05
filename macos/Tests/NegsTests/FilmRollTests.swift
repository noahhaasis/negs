import XCTest
@testable import NegsApp

final class FilmRollTests: XCTestCase {
    private let texture = CGSize(width: 3800, height: 3500)
    private let window = CGRect(x: 0.026316, y: 0.157143, width: 0.947368, height: 0.685714)

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
}
