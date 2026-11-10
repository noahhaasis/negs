import SwiftUI
import XCTest
@testable import NegsApp

/// A pick's box and selection's yellow box (rounds 20 and 21): hand-drawn,
/// seeded from the photograph's name, as `r20-box.html` (B) draws them.
final class PickBoxTests: XCTestCase {
    /// Values from the page's own script (node running its `boxPaths` on the
    /// same names): the hash of name + "·box", the kind, the tilt, and for
    /// each stroke its number of outline points, its first point, the point a
    /// third of the way round and its last.
    private struct FromThePage {
        let name: String, size: CGSize, outset: CGFloat, tight: Bool, weight: CGFloat
        let hash: UInt32, kind: PickBox.Kind, tilt: CGFloat
        let strokes: [(count: Int, points: [CGPoint])]
    }

    private let fromThePage: [FromThePage] = [
        FromThePage(name: "DSC_6435.NEF", size: CGSize(width: 264, height: 176), outset: 3, tight: false, weight: 1,
                    hash: 2_056_138_581, kind: .four, tilt: -0.59, strokes: [
                        (206, [CGPoint(x: -8.41, y: -0.79), CGPoint(x: 193.73, y: -2.11), CGPoint(x: -8.74, y: -0.88)]),
                        (138, [CGPoint(x: 268.14, y: -1.58), CGPoint(x: 264.9, y: 136.16), CGPoint(x: 268.24, y: -1.9)]),
                        (202, [CGPoint(x: 269.28, y: 181.45), CGPoint(x: 69.25, y: 177.21), CGPoint(x: 269.6, y: 181.55)]),
                        (136, [CGPoint(x: -4.62, y: 177.63), CGPoint(x: -4.02, y: 43.08), CGPoint(x: -4.71, y: 177.95)]),
                    ]),
        FromThePage(name: "DSC_5784.NEF", size: CGSize(width: 266, height: 176), outset: 3, tight: false, weight: 1,
                    hash: 1_189_942_715, kind: .loop, tilt: 0, strokes: [
                        (668, [CGPoint(x: 70.5, y: -2.14), CGPoint(x: -1.59, y: 174.03), CGPoint(x: 70.19, y: -2.23)]),
                    ]),
        FromThePage(name: "DSC_6448.NEF", size: CGSize(width: 117, height: 176), outset: 3, tight: false, weight: 1,
                    hash: 1_833_535_613, kind: .loop, tilt: 0.56, strokes: [
                        (452, [CGPoint(x: 38.51, y: -3.2), CGPoint(x: 0.39, y: 124.35), CGPoint(x: 38.13, y: -3.32)]),
                    ]),
        FromThePage(name: "DSC_7006.NEF", size: CGSize(width: 264, height: 176), outset: 3, tight: false, weight: 1,
                    hash: 1_275_140_550, kind: .two, tilt: -0.13, strokes: [
                        (318, [CGPoint(x: -4.46, y: 182.24), CGPoint(x: 130.35, y: -0.59), CGPoint(x: -4.55, y: 182.61)]),
                        (316, [CGPoint(x: 267.83, y: 1.74), CGPoint(x: 127.29, y: 176.95), CGPoint(x: 267.92, y: 1.36)]),
                    ]),
        // The strip's: 2 pt out, tight.
        FromThePage(name: "IMG_0001.JPG", size: CGSize(width: 210, height: 140), outset: 2, tight: true, weight: 1,
                    hash: 1_738_235_033, kind: .four, tilt: -0.15, strokes: [
                        (158, [CGPoint(x: -1.63, y: -0.65), CGPoint(x: 152.78, y: 1.22), CGPoint(x: -2.01, y: -0.75)]),
                        (108, [CGPoint(x: 213.05, y: 1.02), CGPoint(x: 210.9, y: 108.06), CGPoint(x: 213.16, y: 0.64)]),
                        (156, [CGPoint(x: 210.36, y: 140.7), CGPoint(x: 56.28, y: 139.43), CGPoint(x: 210.75, y: 140.8)]),
                        (108, [CGPoint(x: -3.16, y: 140.76), CGPoint(x: -0.51, y: 34.3), CGPoint(x: -3.27, y: 141.14)]),
                    ]),
        FromThePage(name: "DSC_7007.NEF", size: CGSize(width: 100, height: 140), outset: 2, tight: true, weight: 1,
                    hash: 969_449_513, kind: .two, tilt: 0.27, strokes: [
                        (182, [CGPoint(x: -0.93, y: 144.85), CGPoint(x: 29.92, y: 0), CGPoint(x: -1.03, y: 145.27)]),
                        (180, [CGPoint(x: 100.66, y: -0.05), CGPoint(x: 65.64, y: 141.42), CGPoint(x: 100.79, y: -0.46)]),
                    ]),
        // Selection's: name + "·second", 8 pt out, 4 / 2.6 the weight.
        FromThePage(name: "DSC_6435.NEF\u{00B7}second", size: CGSize(width: 264, height: 176), outset: 8, tight: false, weight: 4 / 2.6,
                    hash: 4_080_007_402, kind: .loop, tilt: 0.08, strokes: [
                        (688, [CGPoint(x: 96.6, y: -7.38), CGPoint(x: -4.56, y: 138.58), CGPoint(x: 95.97, y: -7.58)]),
                    ]),
        FromThePage(name: "DSC_5784.NEF\u{00B7}second", size: CGSize(width: 266, height: 176), outset: 8, tight: false, weight: 4 / 2.6,
                    hash: 2_258_131_768, kind: .four, tilt: -0.43, strokes: [
                        (206, [CGPoint(x: -13.07, y: -6.6), CGPoint(x: 189.88, y: -7.2), CGPoint(x: -13.69, y: -6.75)]),
                        (144, [CGPoint(x: 271.51, y: -10.8), CGPoint(x: 271.41, y: 131.71), CGPoint(x: 271.65, y: -11.43)]),
                        (208, [CGPoint(x: 278.52, y: 183.19), CGPoint(x: 71.65, y: 183.21), CGPoint(x: 279.15, y: 183.33)]),
                        (144, [CGPoint(x: -8.15, y: 182.6), CGPoint(x: -7.56, y: 40.08), CGPoint(x: -8.31, y: 183.22)]),
                    ]),
    ]

    func testMatchesTheDrawing() {
        for photo in fromThePage {
            XCTAssertEqual(PickBox.seed(photo.name), photo.hash, photo.name)
            XCTAssertEqual(PickBox.kind(for: photo.name), photo.kind, photo.name)
            let box = PickBox.box(in: photo.size, outset: photo.outset, name: photo.name, tight: photo.tight, weight: photo.weight)
            XCTAssertEqual(box.kind, photo.kind, photo.name)
            // The page writes its numbers to two places.
            XCTAssertEqual(box.tilt, photo.tilt, accuracy: 0.006, photo.name)
            XCTAssertEqual(box.strokes.count, photo.strokes.count, photo.name)
            for (stroke, expected) in zip(box.strokes, photo.strokes) {
                let points = outline(stroke)
                XCTAssertEqual(points.count, expected.count, photo.name)
                guard points.count == expected.count else { continue }
                let n = points.count
                for (point, want) in zip([points[0], points[n / 3], points[n - 1]], expected.points) {
                    XCTAssertEqual(point.x, want.x, accuracy: 0.006, photo.name)
                    XCTAssertEqual(point.y, want.y, accuracy: 0.006, photo.name)
                }
            }
        }
    }

    func testTheGridStripAndSelectionBoxesAreThePagesSettings() {
        let size = CGSize(width: 264, height: 176)
        XCTAssertEqual(PickBox.grid(in: size, name: "DSC_6435.NEF").strokes.map(\.description),
                       PickBox.box(in: size, outset: 3, name: "DSC_6435.NEF").strokes.map(\.description))
        XCTAssertEqual(PickBox.strip(in: size, name: "DSC_6435.NEF").strokes.map(\.description),
                       PickBox.box(in: size, outset: 2, name: "DSC_6435.NEF", tight: true).strokes.map(\.description))
        XCTAssertEqual(PickBox.selection(in: size, name: "DSC_6435.NEF").strokes.map(\.description),
                       PickBox.box(in: size, outset: 8, name: "DSC_6435.NEF·second", weight: 4 / 2.6).strokes.map(\.description))
    }

    func testTheSamePhotographAlwaysGetsTheSameBox() {
        let size = CGSize(width: 214, height: 140)
        let a = PickBox.grid(in: size, name: "DSC_6435.NEF"), b = PickBox.grid(in: size, name: "DSC_6435.NEF")
        XCTAssertEqual(a.path(in: size).description, b.path(in: size).description)
        XCTAssertNotEqual(a.path(in: size).description, PickBox.grid(in: size, name: "DSC_6436.NEF").path(in: size).description)
        // Selection's box is its own, not the pick's drawn again.
        XCTAssertNotEqual(a.path(in: size).description, PickBox.selection(in: size, name: "DSC_6435.NEF").path(in: size).description)
    }

    func testAllThreeBoxesTurnUp() {
        let names = (0..<40).map { "DSC_\(7000 + $0).NEF" }
        let kinds = names.map(PickBox.kind(for:))
        // The page draws 16 four, 10 loop and 14 two of these.
        XCTAssertEqual(kinds.filter { $0 == .four }.count, 16)
        XCTAssertEqual(kinds.filter { $0 == .loop }.count, 10)
        XCTAssertEqual(kinds.filter { $0 == .two }.count, 14)
        // Four sides, one loop, two strokes.
        let size = CGSize(width: 200, height: 140)
        for (name, kind) in zip(names, kinds) {
            XCTAssertEqual(PickBox.grid(in: size, name: name).strokes.count, kind == .four ? 4 : kind == .loop ? 1 : 2, name)
        }
    }

    func testStaysWithinItsReach() {
        for name in (0..<40).map({ "DSC_\(7000 + $0).NEF" }) {
            for size in [CGSize(width: 86, height: 140), CGSize(width: 264, height: 176), CGSize(width: 540, height: 360)] {
                let picture = CGRect(origin: .zero, size: size)
                for box in [PickBox.grid(in: size, name: name), PickBox.strip(in: size, name: name), PickBox.selection(in: size, name: name)] {
                    let bounds = box.path(in: size).boundingRect
                    XCTAssertFalse(bounds.isNull || bounds.minX.isNaN, name)
                    // Round the picture, never into it, and inside what the layer draws.
                    XCTAssert(bounds.contains(picture.insetBy(dx: 4, dy: 4)), "\(name) \(size) \(bounds)")
                    // Leave room for the halo's blur.
                    XCTAssert(picture.insetBy(dx: -PickBox.reach + 4, dy: -PickBox.reach + 4).contains(bounds), "\(name) \(size) \(bounds)")
                }
            }
        }
        XCTAssertTrue(PickBox.grid(in: .zero, name: "DSC_6435.NEF").path(in: .zero).boundingRect.width.isFinite)
    }

    func testStillSaysPick() {
        var summary = LibrarySummary()
        summary.flag = .pick
        summary.rating = 3
        XCTAssertEqual(PhotoMarks.accessibilityLabel("DSC_6435.NEF", summary: summary), "DSC_6435.NEF, 3 stars, Pick")
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
