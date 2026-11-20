import CNegsCore
import SwiftUI

/// A rating star drawn by hand (round 22, `r22-edge.html`, A): the quick
/// asterisk an editor puts beside a frame, three strokes through the
/// middle, the upright one first. Each star is seeded from the file name and
/// its place in the row, so a row of four is four different stars, and the
/// same every time.
///
/// Drawn in a 12 × 12 pt box round (0, 0); a stroke may reach a little past
/// it, as on the page. Each stroke is a filled outline whose width follows
/// the pencil's pressure. The geometry is the shared core's (core/src/marks,
/// a port of the page's `handStar("A", …)` and `stroke`).
enum HandStar {
    /// The box each star is drawn in, and the gap between two in a row.
    static let size = CGFloat(CoreMarks.constants.star_size)
    static let gap = CGFloat(CoreMarks.constants.star_gap)

    /// The page's seed: the name, "·star" and the star's place, from 0.
    static func seed(_ name: String, index: Int) -> UInt32 {
        CoreMarks.withName(name) { negs_marks_star_seed($0, $1, UInt32(index)) }
    }

    /// The three strokes of the `index`th star in `name`'s row, round (0, 0).
    static func strokes(name: String, index: Int) -> [Path] {
        CoreMarks.take(CoreMarks.withName(name) { negs_marks_star($0, $1, UInt32(index)) }).paths
    }

    /// The star's strokes as one path round (0, 0).
    static func path(name: String, index: Int) -> Path {
        var path = Path()
        for stroke in strokes(name: name, index: index) { path.addPath(stroke) }
        return path
    }
}

/// One hand-drawn star, centred in the frame it's given (lay it out at
/// `HandStar.size`). A shape, not a canvas, so a stroke that reaches past
/// its box isn't cut off.
struct HandStarShape: Shape {
    let name: String
    let index: Int

    func path(in rect: CGRect) -> Path {
        HandStar.path(name: name, index: index).offsetBy(dx: rect.midX, dy: rect.midY)
    }
}
