import SwiftUI
import XCTest
@testable import NegsApp

/// A pick's mark (round 15): the small waving flag, B in `r15-flag.html`.
final class PickFlagTests: XCTestCase {
    func testTheFlagFillsItsBoxAsDrawn() {
        let path = FlagMark().path(in: CGRect(x: 0, y: 0, width: 16, height: 16))
        let box = path.boundingRect
        // The drawing's 20 pt box at 16: the pole from 2.6 to 17.4 down,
        // the cloth out to 16.5 across.
        XCTAssertEqual(box.minX, 4.35 * 0.8, accuracy: 0.05)
        XCTAssertEqual(box.maxX, 16.6 * 0.8, accuracy: 0.15)
        XCTAssertEqual(box.minY, 2.4 * 0.8, accuracy: 0.2)
        XCTAssertEqual(box.maxY, 17.9 * 0.8, accuracy: 0.2)
        // Two filled pieces, the pole and the cloth: a shape, not a colour.
        var pieces = 0
        path.forEach { if case .move = $0 { pieces += 1 } }
        XCTAssertEqual(pieces, 2)
        XCTAssertTrue(path.contains(CGPoint(x: 4 * 0.8 + 1, y: 15 * 0.8)), "the pole")
        XCTAssertTrue(path.contains(CGPoint(x: 11 * 0.8, y: 7 * 0.8)), "the cloth")
        XCTAssertFalse(path.contains(CGPoint(x: 11 * 0.8, y: 15 * 0.8)), "nothing under the cloth")
    }

    func testStillSaysPick() {
        var summary = LibrarySummary()
        summary.flag = .pick
        summary.rating = 3
        XCTAssertEqual(PhotoMarks.accessibilityLabel("DSC_6435.NEF", summary: summary), "DSC_6435.NEF, 3 stars, Pick")
    }
}
