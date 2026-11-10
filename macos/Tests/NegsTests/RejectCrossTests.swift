import SwiftUI
import XCTest
@testable import NegsApp

/// The reject's cross (round 14): one of two hand-drawn crosses per
/// photograph, seeded from its name, as `r14-reject.html` draws it.
final class RejectCrossTests: XCTestCase {
    /// Values from the page's own script (node, `mixed(200, 140, name)`):
    /// the hash, the first two random numbers, where each stroke starts, and
    /// each stroke's opacity.
    private let fromThePage: [(name: String, hash: UInt32, first: [CGFloat], starts: [CGPoint], opacities: [Double])] = [
        ("DSC_6435.NEF", 2_972_515_683, [0.212829355, 0.331041164], [CGPoint(x: 24.84, y: 19.71), CGPoint(x: 173.45, y: 18.11)], [0.9, 0.8]),
        ("DSC_5784.NEF", 1_338_732_189, [0.090616154, 0.541751701], [CGPoint(x: 22.24, y: 24.10), CGPoint(x: 172.57, y: 17.38)], [0.9, 0.8]),
        ("IMG_0001.JPG", 2_515_027_847, [0.705942326, 0.111862622],
         [CGPoint(x: 5.90, y: 10.17), CGPoint(x: 5.79, y: 11.72), CGPoint(x: 191.77, y: 5.74), CGPoint(x: 191.37, y: 4.19)], [0.95, 0.69, 0.95, 0.69]),
        ("DSC_8200.NEF", 2_362_156_491, [0.952154420, 0.436344665],
         [CGPoint(x: 6.83, y: 7.86), CGPoint(x: 5.85, y: 9.28), CGPoint(x: 192.66, y: 8.28), CGPoint(x: 192.06, y: 6.88)], [0.95, 0.66, 0.95, 0.66]),
    ]

    func testMatchesTheDrawing() {
        for photo in fromThePage {
            XCTAssertEqual(RejectCross.hash(photo.name), photo.hash, photo.name)
            var random = RejectCross.Random(seed: photo.hash)
            for value in photo.first { XCTAssertEqual(random.next(), value, accuracy: 1e-9, photo.name) }
            let strokes = RejectCross.strokes(in: CGSize(width: 200, height: 140), name: photo.name)
            XCTAssertEqual(strokes.count, photo.starts.count, photo.name)
            for (stroke, start) in zip(strokes, photo.starts) {
                let first = firstPoint(stroke.path)
                XCTAssertEqual(first.x, start.x, accuracy: 0.006, photo.name)
                XCTAssertEqual(first.y, start.y, accuracy: 0.006, photo.name)
            }
            for (stroke, opacity) in zip(strokes, photo.opacities) { XCTAssertEqual(stroke.opacity, opacity, accuracy: 0.006, photo.name) }
        }
    }

    func testTheSamePhotographAlwaysGetsTheSameCross() {
        let size = CGSize(width: 214, height: 140)
        let a = RejectCross.strokes(in: size, name: "DSC_6435.NEF"), b = RejectCross.strokes(in: size, name: "DSC_6435.NEF")
        XCTAssertEqual(a.map(\.path.description), b.map(\.path.description))
        XCTAssertNotEqual(a.map(\.path.description), RejectCross.strokes(in: size, name: "DSC_6436.NEF").map(\.path.description))
    }

    func testBothCrossesTurnUp() {
        let kinds = (0..<40).map { RejectCross.kind(for: "DSC_\(7000 + $0).NEF") }
        let loose = kinds.filter { $0 == .loose }.count
        XCTAssert((8...32).contains(loose), "\(loose) of 40 loose")
        // Loose draws two strokes; gone over twice draws each one twice.
        XCTAssertEqual(RejectCross.strokes(in: CGSize(width: 200, height: 140), name: "DSC_6435.NEF").count, 2)
        XCTAssertEqual(RejectCross.strokes(in: CGSize(width: 200, height: 140), name: "IMG_0001.JPG").count, 4)
    }

    func testDrawsInsideTheSmallestFrames() {
        for name in (0..<40).map({ "DSC_\(7000 + $0).NEF" }) {
            for size in [CGSize(width: 86, height: 140), CGSize(width: 264, height: 176), .zero] {
                for stroke in RejectCross.strokes(in: size, name: name) {
                    let box = stroke.path.boundingRect
                    XCTAssertFalse(box.isNull || box.minX.isNaN || box.minY.isNaN, name)
                    // A loose stroke may run a few points past its corner.
                    XCTAssert(box.insetBy(dx: 6, dy: 6).intersection(CGRect(origin: .zero, size: size)) == box.insetBy(dx: 6, dy: 6), "\(name) \(size) \(box)")
                }
            }
        }
    }

    func testStillSaysRejected() {
        var summary = LibrarySummary()
        summary.flag = .reject
        XCTAssertEqual(PhotoMarks.accessibilityLabel("DSC_6435.NEF", summary: summary), "DSC_6435.NEF, Rejected")
    }

    private func firstPoint(_ path: Path) -> CGPoint {
        var first: CGPoint?
        path.forEach { element in
            if first == nil, case .move(let point) = element { first = point }
        }
        return first ?? .zero
    }
}
