import CNegsCore
import SwiftUI

/// The pencil's geometry comes from the shared core (core/src/marks), so a
/// photograph draws the same star, cross and box on every platform. These
/// turn its results into paths.
enum CoreMarks {
    static let constants: negs_marks_constants_t = {
        var c = negs_marks_constants_t()
        negs_marks_constants(&c)
        return c
    }()

    /// Calls `body` with the name as UTF-8 and its length in bytes (NULL
    /// and 0 for the empty name).
    static func withName<T>(_ name: String, _ body: (UnsafePointer<CChar>?, Int) -> T) -> T {
        var name = name
        return name.withUTF8 { bytes in
            bytes.withMemoryRebound(to: CChar.self) { body($0.baseAddress, $0.count) }
        }
    }

    /// Each stroke of a block from the core as a closed outline; frees it.
    static func take(_ strokes: negs_strokes_t) -> (paths: [Path], opacities: [Double], kind: UInt32, tilt: Double) {
        var strokes = strokes
        defer { negs_strokes_free(&strokes) }
        var paths: [Path] = [], opacities: [Double] = [], at = 0
        for i in 0..<strokes.stroke_count {
            let count = Int(strokes.counts[i])
            let points = (at..<at + count).map { CGPoint(x: strokes.points[$0].x, y: strokes.points[$0].y) }
            var path = Path()
            path.addLines(points)
            path.closeSubpath()
            paths.append(path)
            opacities.append(strokes.opacities[i])
            at += count
        }
        return (paths, opacities, strokes.kind, strokes.tilt)
    }

    /// Segments from the core as a `CGPath`.
    static func cgPath(_ segs: ArraySlice<negs_seg_t>) -> CGPath {
        let path = CGMutablePath()
        func p(_ q: negs_pt_t) -> CGPoint { CGPoint(x: q.x, y: q.y) }
        for s in segs {
            switch s.kind {
            case NEGS_SEG_MOVE: path.move(to: p(s.p.0))
            case NEGS_SEG_LINE: path.addLine(to: p(s.p.0))
            case NEGS_SEG_CUBIC: path.addCurve(to: p(s.p.2), control1: p(s.p.0), control2: p(s.p.1))
            default: path.closeSubpath()
            }
        }
        return path
    }

    static func rect(_ r: CGRect) -> negs_rect_t { negs_rect_t(x: r.minX, y: r.minY, w: r.width, h: r.height) }
}
