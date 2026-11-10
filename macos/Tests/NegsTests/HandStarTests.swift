import SwiftUI
import XCTest
@testable import NegsApp

/// Rating stars drawn by hand (round 22, A · asterisk): three strokes each,
/// seeded from the photograph's name and the star's place, as
/// `r22-edge.html` draws them.
final class HandStarTests: XCTestCase {
    /// Values from the page's own script (node running its `handStar("A", …)`
    /// on name + "·star" + index): the hash, and for each stroke its number of
    /// outline points, its first point, the point a third of the way round
    /// and its last.
    private struct FromThePage {
        let name: String, index: Int, hash: UInt32
        let strokes: [(count: Int, points: [CGPoint])]
    }

    private let fromThePage: [FromThePage] = [
        FromThePage(name: "DSC_6435.NEF", index: 0, hash: 3_315_392_284, strokes: [
            (28, [CGPoint(x: -1.99, y: -4.77), CGPoint(x: -0.09, y: 3.89), CGPoint(x: -1.98, y: -4.98)]),
            (28, [CGPoint(x: -5.62, y: -3.29), CGPoint(x: 4.14, y: 3.07), CGPoint(x: -5.77, y: -3.44)]),
            (28, [CGPoint(x: 4.47, y: -3.68), CGPoint(x: -5.1, y: 3.21), CGPoint(x: 4.67, y: -3.76)]),
        ]),
        FromThePage(name: "DSC_6435.NEF", index: 1, hash: 3_332_169_903, strokes: [
            (28, [CGPoint(x: 0.02, y: -5.01), CGPoint(x: -1.06, y: 4.73), CGPoint(x: 0.09, y: -5.22)]),
            (28, [CGPoint(x: -3.96, y: -2.24), CGPoint(x: 3.76, y: 3.89), CGPoint(x: -4.08, y: -2.42)]),
            (28, [CGPoint(x: 5.12, y: -2.12), CGPoint(x: -5.19, y: 2.22), CGPoint(x: 5.34, y: -2.16)]),
        ]),
        FromThePage(name: "DSC_6435.NEF", index: 4, hash: 3_248_281_808, strokes: [
            (28, [CGPoint(x: 0.18, y: -5.59), CGPoint(x: -0.78, y: 4.84), CGPoint(x: 0.27, y: -5.78)]),
            (28, [CGPoint(x: -4.36, y: -2.06), CGPoint(x: 4.13, y: 2.18), CGPoint(x: -4.53, y: -2.19)]),
            (28, [CGPoint(x: 4.23, y: -2.86), CGPoint(x: -3.94, y: 1.73), CGPoint(x: 4.44, y: -2.91)]),
        ]),
        FromThePage(name: "IMG_0001.JPG", index: 0, hash: 948_015_264, strokes: [
            (28, [CGPoint(x: -0.13, y: -4.8), CGPoint(x: -0.51, y: 5.99), CGPoint(x: -0.04, y: -5.03)]),
            (28, [CGPoint(x: -4.51, y: -2.73), CGPoint(x: 3.9, y: 4.59), CGPoint(x: -4.63, y: -2.95)]),
            (28, [CGPoint(x: 4.02, y: -2.84), CGPoint(x: -4.55, y: 2.54), CGPoint(x: 4.26, y: -2.92)]),
        ]),
        FromThePage(name: "IMG_0001.JPG", index: 2, hash: 981_570_502, strokes: [
            (28, [CGPoint(x: 0.07, y: -4.46), CGPoint(x: 0.67, y: 5.6), CGPoint(x: 0.13, y: -4.69)]),
            (28, [CGPoint(x: -3.9, y: -1.28), CGPoint(x: 4.56, y: 2.95), CGPoint(x: -4.06, y: -1.45)]),
            (28, [CGPoint(x: 4.36, y: -2.93), CGPoint(x: -3.96, y: 3.07), CGPoint(x: 4.58, y: -3.01)]),
        ]),
        FromThePage(name: "DSC_7041.NEF", index: 3, hash: 1_708_573_339, strokes: [
            (28, [CGPoint(x: -1.51, y: -5.29), CGPoint(x: 1.08, y: 6.06), CGPoint(x: -1.48, y: -5.53)]),
            (28, [CGPoint(x: -5.01, y: -1.3), CGPoint(x: 4.69, y: 1.77), CGPoint(x: -5.21, y: -1.44)]),
            (28, [CGPoint(x: 4.05, y: -3.18), CGPoint(x: -4.47, y: 2.1), CGPoint(x: 4.28, y: -3.25)]),
        ]),
        FromThePage(name: "f03.jpg", index: 0, hash: 2_243_906_042, strokes: [
            (28, [CGPoint(x: 0.02, y: -5.24), CGPoint(x: -0.79, y: 5.83), CGPoint(x: 0.11, y: -5.46)]),
            (28, [CGPoint(x: -5.49, y: -2.1), CGPoint(x: 4.59, y: 3.45), CGPoint(x: -5.65, y: -2.26)]),
            (28, [CGPoint(x: 4.93, y: -2.24), CGPoint(x: -5.38, y: 2.27), CGPoint(x: 5.17, y: -2.27)]),
        ]),
        // A name past ASCII: the hash runs over UTF-16, as the page's does.
        FromThePage(name: "Ålesund 2024.dng", index: 1, hash: 124_155_722, strokes: [
            (28, [CGPoint(x: 0.2, y: -5.65), CGPoint(x: -0.5, y: 5.5), CGPoint(x: 0.28, y: -5.89)]),
            (28, [CGPoint(x: -4.49, y: -1.42), CGPoint(x: 5.21, y: 2.24), CGPoint(x: -4.69, y: -1.57)]),
            (28, [CGPoint(x: 3.82, y: -3.61), CGPoint(x: -4.21, y: 2.26), CGPoint(x: 4.07, y: -3.68)]),
        ]),
    ]

    func testMatchesTheDrawing() {
        for star in fromThePage {
            let label = "\(star.name) #\(star.index)"
            XCTAssertEqual(HandStar.seed(star.name, index: star.index), star.hash, label)
            let strokes = HandStar.strokes(name: star.name, index: star.index)
            XCTAssertEqual(strokes.count, star.strokes.count, label)
            for (stroke, expected) in zip(strokes, star.strokes) {
                let points = outline(stroke)
                XCTAssertEqual(points.count, expected.count, label)
                guard points.count == expected.count else { continue }
                let n = points.count
                // The page writes its numbers to two places.
                for (point, want) in zip([points[0], points[n / 3], points[n - 1]], expected.points) {
                    XCTAssertEqual(point.x, want.x, accuracy: 0.006, label)
                    XCTAssertEqual(point.y, want.y, accuracy: 0.006, label)
                }
            }
        }
    }

    func testTheSameStarEveryTimeAndEachOneItsOwn() {
        let first = HandStar.path(name: "DSC_6435.NEF", index: 2).description
        XCTAssertEqual(first, HandStar.path(name: "DSC_6435.NEF", index: 2).description)
        // Another place in the row, or another photograph, draws another star.
        let row = (0..<5).map { HandStar.path(name: "DSC_6435.NEF", index: $0).description }
        XCTAssertEqual(Set(row).count, 5)
        XCTAssertNotEqual(first, HandStar.path(name: "DSC_6436.NEF", index: 2).description)
    }

    func testStaysInItsBox() {
        // Each star is drawn in a 12 × 12 box round (0, 0). A stroke's end may
        // reach a little past it, as on the page (the page lets it), but never
        // as far as the next star's middle, 14.5 pt along.
        let box = CGRect(x: -6, y: -6, width: 12, height: 12)
        for name in (0..<200).map({ "DSC_\(7000 + $0).NEF" }) {
            for index in 0..<5 {
                let bounds = HandStar.path(name: name, index: index).boundingRect
                XCTAssertFalse(bounds.isNull || bounds.minX.isNaN, name)
                XCTAssert(box.insetBy(dx: -2, dy: -2).contains(bounds), "\(name) #\(index) \(bounds)")
                // Three strokes through the middle: it covers most of the box.
                XCTAssertGreaterThan(bounds.width, 7, name)
                XCTAssertGreaterThan(bounds.height, 7, name)
                XCTAssert(bounds.contains(CGPoint(x: 0, y: 0)), name)
            }
        }
    }

    func testTheShapeIsCentredInItsFrame() {
        let star = HandStarShape(name: "DSC_6435.NEF", index: 0)
        let at = star.path(in: CGRect(x: 20, y: 30, width: HandStar.size, height: HandStar.size)).boundingRect
        let round = HandStar.path(name: "DSC_6435.NEF", index: 0).boundingRect
        XCTAssertEqual(at.midX - round.midX, 26, accuracy: 0.0001)
        XCTAssertEqual(at.midY - round.midY, 36, accuracy: 0.0001)
    }

    func testTheRowsMeasuresAreThePages() {
        XCTAssertEqual(HandStar.size, 12)
        XCTAssertEqual(HandStar.gap, 2.5)
    }

    func testStillSaysHowManyStars() {
        var summary = LibrarySummary()
        summary.rating = 1
        XCTAssertEqual(PhotoMarks.accessibilityLabel("DSC_6435.NEF", summary: summary), "DSC_6435.NEF, 1 star")
        summary.rating = 4
        XCTAssertEqual(PhotoMarks.accessibilityLabel("DSC_6435.NEF", summary: summary), "DSC_6435.NEF, 4 stars")
    }

    /// The outline's points, in order.
    private func outline(_ path: Path) -> [CGPoint] {
        var points: [CGPoint] = []
        path.forEach { element in
            switch element {
            case .move(let point), .line(let point): points.append(point)
            default: break
            }
        }
        return points
    }
}
