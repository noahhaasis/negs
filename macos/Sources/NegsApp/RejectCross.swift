import CNegsCore
import SwiftUI

/// A reject's cross (round 14, `r14-reject.html`): each photograph gets one
/// of two hand-drawn crosses, chosen from its file name, with its own bow,
/// wobble, weight and retrace, so no two look alike and the same photograph
/// always gets the same one.
///
/// - **Loose:** drawn fast. Each stroke starts short of one corner, runs past
///   the other and ends in a flick; the second is a little lighter.
/// - **Gone over twice:** a thinner line traced again just beside itself.
///
/// Each stroke is a filled outline whose width follows the pencil's
/// pressure. The geometry is the shared core's (core/src/marks, a port of
/// the page's script with the same hash and random numbers in the same
/// order), so the drawing and every app agree.
struct CrossMark: View {
    let name: String

    var body: some View {
        Canvas { context, size in
            for stroke in RejectCross.strokes(in: size, name: name) {
                context.fill(stroke.path, with: .color(Theme.paper.opacity(stroke.opacity)))
            }
        }
    }
}

enum RejectCross {
    struct Stroke {
        var path: Path
        var opacity: Double
    }

    enum Kind: Equatable { case loose, twice }

    /// FNV-1a over the name's UTF-16 units, as the page hashes it.
    static func hash(_ name: String) -> UInt32 { CoreMarks.withName(name) { negs_marks_hash($0, $1) } }

    /// mulberry32, as the page's `rng`.
    struct Random {
        var state: UInt32
        init(seed: UInt32) { state = seed }
        mutating func next() -> CGFloat { CGFloat(negs_marks_random_next(&state)) }
    }

    static func kind(for name: String) -> Kind {
        CoreMarks.withName(name) { negs_marks_cross_kind($0, $1) } == NEGS_CROSS_LOOSE ? .loose : .twice
    }

    /// The strokes for a picture of this size: top left to bottom right,
    /// then top right to bottom left, as a hand draws an X.
    static func strokes(in size: CGSize, name: String) -> [Stroke] {
        let block = CoreMarks.take(CoreMarks.withName(name) { negs_marks_cross(size.width, size.height, $0, $1) })
        return zip(block.paths, block.opacities).map { Stroke(path: $0, opacity: $1) }
    }
}
