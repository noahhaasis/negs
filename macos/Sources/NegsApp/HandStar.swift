import SwiftUI

/// A rating star drawn by hand (round 22, `r22-edge.html`, A): the quick
/// asterisk an editor puts beside a frame, three strokes through the
/// middle, the upright one first. Each star is seeded from the file name and
/// its place in the row, so a row of four is four different stars, and the
/// same every time.
///
/// Drawn in a 12 × 12 pt box round (0, 0); a stroke may reach a little past
/// it, as on the page. Each stroke is a filled outline whose width follows
/// the pencil's pressure. This is a port of the page's `handStar("A", …)`
/// and `stroke`, with the reject cross's hash and random numbers in the same
/// order and the pick box's `ribbon`, so the drawing and the app agree.
enum HandStar {
    /// The box each star is drawn in, and the gap between two in a row.
    static let size: CGFloat = 12
    static let gap: CGFloat = 2.5

    /// The page's seed: the name, "·star" and the star's place, from 0.
    static func seed(_ name: String, index: Int) -> UInt32 {
        RejectCross.hash(name + "\u{00B7}star\(index)")
    }

    private static let profile = RejectCross.Profile(keys: [(0, 0.9), (0.15, 1.75), (0.85, 1.55), (1, 0.7)])

    /// The three strokes of the `index`th star in `name`'s row, round (0, 0).
    static func strokes(name: String, index: Int) -> [Path] {
        var random = RejectCross.Random(seed: seed(name, index: index))
        func jitter(_ m: CGFloat) -> CGFloat { (random.next() - 0.5) * 2 * m }
        let scale = lerp(0.9, 1.15, random.next())
        let tilt = jitter(9), cx = jitter(0.5), cy = jitter(0.5)
        var strokes: [Path] = []
        for a0: CGFloat in [90, 30, 150] {
            let a = (a0 + tilt + jitter(8)) * .pi / 180
            let length = lerp(4.6, 5.8, random.next()), off = jitter(0.7)
            let ca = cos(a), sa = sin(a)
            let from = CGPoint(x: cx + off - ca * length, y: cy - sa * length)
            let toX = cx + ca * length * lerp(0.85, 1.05, random.next())
            let toY = cy + sa * length * lerp(0.85, 1.05, random.next()) + off
            strokes.append(stroke(from, CGPoint(x: toX, y: toY), random: &random) { profile.width($0) * scale })
        }
        return strokes
    }

    /// The star's strokes as one path round (0, 0).
    static func path(name: String, index: Int) -> Path {
        var path = Path()
        for stroke in strokes(name: name, index: index) { path.addPath(stroke) }
        return path
    }

    /// A line from `a` to `b` in 8 steps, bowed a little to one side.
    private static func stroke(_ a: CGPoint, _ b: CGPoint, bow: CGFloat = 0.25, random: inout RejectCross.Random,
                               width: (CGFloat) -> CGFloat) -> Path {
        let length = hypot(b.x - a.x, b.y - a.y), l = length == 0 ? 1 : length
        let nx = -(b.y - a.y) / l, ny = (b.x - a.x) / l, q = (random.next() - 0.5) * 2 * bow
        let points = (0...8).map { s -> CGPoint in
            let t = CGFloat(s) / 8, d = sin(.pi * t) * q
            return CGPoint(x: lerp(a.x, b.x, t) + nx * d, y: lerp(a.y, b.y, t) + ny * d)
        }
        return PickBox.ribbon(points, width: width)
    }

    private static func lerp(_ a: CGFloat, _ b: CGFloat, _ t: CGFloat) -> CGFloat { RejectCross.lerp(a, b, t) }
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
