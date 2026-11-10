import SwiftUI

/// The ink drawn round a thumbnail (rounds 20 and 21, `r20-box.html`, B): a
/// pick's cream box and, in the grid, selection's yellow box over it, under
/// the marks' dark halo. Both reach past the picture, over the gaps and the
/// pictures beside it, so the grid and the strip draw this in a layer of its
/// own above every thumbnail. Lay it out at the picture's size: it draws
/// `PickBox.reach` outside it.
struct BoxInk: View {
    let name: String
    /// A pick: the cream box.
    let picked: Bool
    /// Selected or current, in the grid: the yellow box, over the cream one.
    var selected = false
    /// The strip's box: a little closer to the picture, with shorter overshoots.
    var inStrip = false

    var body: some View {
        Canvas { context, size in
            let reach = PickBox.reach
            let picture = CGSize(width: size.width - 2 * reach, height: size.height - 2 * reach)
            context.translateBy(x: reach, y: reach)
            if picked {
                let box = inStrip ? PickBox.strip(in: picture, name: name) : PickBox.grid(in: picture, name: name)
                context.fill(box.path(in: picture), with: .color(Theme.paper))
            }
            if selected {
                context.fill(PickBox.selection(in: picture, name: name).path(in: picture), with: .color(Theme.accent))
            }
        }
        .padding(-PickBox.reach)
        // The halo: drop-shadow 1.5 pt black at 0.95, then 4 pt at 0.45.
        .shadow(color: .black.opacity(0.95), radius: 0.75)
        .shadow(color: .black.opacity(0.45), radius: 2)
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }
}

/// A box drawn in grease pencil round a picture, as an editor boxes a frame
/// on a contact sheet. Each name draws one of three, with its own corners,
/// bow, wobble, weight and tilt, so no two look alike and the same
/// photograph always gets the same one:
///
/// - **Four:** four separate sides, crossing at the corners.
/// - **Loop:** one stroke round, its end running past its start.
/// - **Two:** down and across, then across and up, meeting at the corners.
///
/// Each stroke is a filled outline whose width follows the pencil's
/// pressure. This is a port of the page's `boxPaths`, `ribbon` and
/// `smoothPts`, seeded with the reject cross's hash and random numbers over
/// the name and "·box", so the drawing and the app agree.
enum PickBox {
    enum Kind: Equatable { case four, loop, two }

    struct Box {
        var kind: Kind
        /// Each stroke, before the tilt, in the picture's coordinates.
        var strokes: [Path]
        /// The tilt round the picture's centre, in degrees, clockwise.
        var tilt: CGFloat

        /// The strokes, tilted.
        func path(in size: CGSize) -> Path {
            let transform = CGAffineTransform(translationX: size.width / 2, y: size.height / 2)
                .rotated(by: tilt * .pi / 180)
                .translatedBy(x: -size.width / 2, y: -size.height / 2)
            var path = Path()
            for stroke in strokes { path.addPath(stroke, transform: transform) }
            return path
        }
    }

    /// How far outside the picture a box may draw: the yellow box's 8 pt,
    /// a corner's jitter, an overshoot of up to 12 pt, the stroke's width
    /// and wobble, and the tilt on a wide picture.
    static let reach: CGFloat = 32

    /// A pick in the grid: 3 pt out.
    static func grid(in size: CGSize, name: String) -> Box { box(in: size, outset: 3, name: name) }
    /// A pick in the strip: 2 pt out, the overshoots kept short.
    static func strip(in size: CGSize, name: String) -> Box { box(in: size, outset: 2, name: name, tight: true) }
    /// Selection in the grid: a second box, 8 pt out, in a 4 pt yellow line
    /// (the page's "Yellow line" over its 2.6 pt stroke).
    static func selection(in size: CGSize, name: String) -> Box {
        box(in: size, outset: 8, name: name + "\u{00B7}second", weight: 4 / 2.6)
    }

    static func seed(_ name: String) -> UInt32 { RejectCross.hash(name + "\u{00B7}box") }

    static func kind(for name: String) -> Kind {
        var random = RejectCross.Random(seed: seed(name))
        return kindOf(random.next())
    }

    private static func kindOf(_ k: CGFloat) -> Kind { k < 0.36 ? .four : k < 0.7 ? .loop : .two }

    private static let profile = RejectCross.Profile(keys: [(0, 1.4), (0.05, 2.9), (0.85, 2.6), (1, 0.9)])

    /// The box round a `size` picture, `outset` outside it. `tight` keeps
    /// the corners and overshoots small (the strip); `weight` scales the
    /// stroke.
    static func box(in size: CGSize, outset o: CGFloat, name: String, tight: Bool = false, weight: CGFloat = 1) -> Box {
        var random = RejectCross.Random(seed: seed(name))
        func jitter(_ m: CGFloat) -> CGFloat { (random.next() - 0.5) * 2 * m }
        let kind = kindOf(random.next())
        let over: CGFloat = tight ? 4 : 12, cj: CGFloat = tight ? 1.5 : 2.6
        let w = size.width, h = size.height
        var c: [CGPoint] = []
        for (x, y) in [(-o, -o), (w + o, -o), (w + o, h + o), (-o, h + o)] {
            let jx = jitter(cj), jy = jitter(cj)
            c.append(CGPoint(x: x + jx, y: y + jy))
        }
        /// `p` moved `d` further from `q`.
        func ext(_ p: CGPoint, _ q: CGPoint, _ d: CGFloat) -> CGPoint {
            let dx = p.x - q.x, dy = p.y - q.y, l = nonZero(hypot(dx, dy))
            return CGPoint(x: p.x + dx / l * d, y: p.y + dy / l * d)
        }
        func at(_ a: CGPoint, _ b: CGPoint, _ t: CGFloat) -> CGPoint { CGPoint(x: lerp(a.x, b.x, t), y: lerp(a.y, b.y, t)) }

        var ways: [[CGPoint]] = []
        switch kind {
        case .four:
            for i in 0..<4 {
                let a = c[i], b = c[(i + 1) % 4]
                let from = ext(a, b, lerp(-2, over * 0.6, random.next()))
                let to = ext(b, a, lerp(-1, over, random.next()))
                ways.append([from, to])
            }
        case .loop:
            let t0 = lerp(0.1, 0.4, random.next())
            var end = at(c[0], c[1], min(0.95, t0 + lerp(0.1, 0.3, random.next())))
            end.y += jitter(2.5)
            ways = [[at(c[0], c[1], t0), c[1], c[2], c[3], c[0], end]]
        case .two:
            let a0 = ext(c[3], c[0], lerp(-2, over * 0.5, random.next()))
            let a2 = ext(c[1], c[0], lerp(0, over, random.next()))
            let b0 = ext(c[1], c[2], lerp(-4, 2, random.next()))
            let b2 = ext(c[3], c[2], lerp(0, over, random.next()))
            ways = [[a0, c[0], a2], [b0, c[2], b2]]
        }
        let scale = lerp(0.85, 1.2, random.next()) * weight
        let tilt = jitter(0.6)
        let wobble = lerp(0.3, 0.9, random.next()), period = lerp(70, 150, random.next()), phase = random.next() * 6.283

        var strokes: [Path] = []
        for way in ways {
            // Each leg in steps of about 3 pt, bowed a little.
            var points: [CGPoint] = []
            for i in 0..<(way.count - 1) {
                let a = way[i], b = way[i + 1], length = nonZero(hypot(b.x - a.x, b.y - a.y))
                let n = max(2, Int((length / 3).rounded(.up)))
                let nx = -(b.y - a.y) / length, ny = (b.x - a.x) / length
                let bow = jitter(min(2.2, length * 0.012))
                for s in 0..<n {
                    let t = CGFloat(s) / CGFloat(n), q = sin(.pi * t) * bow
                    points.append(CGPoint(x: lerp(a.x, b.x, t) + nx * q, y: lerp(a.y, b.y, t) + ny * q))
                }
            }
            points.append(way[way.count - 1])
            points = smoothPoints(points, 2)
            // A slow wobble across the line, by the distance along it.
            var along: CGFloat = 0
            var wobbled: [CGPoint] = []
            wobbled.reserveCapacity(points.count)
            for i in points.indices {
                let p = points[i]
                if i > 0 { along += hypot(p.x - points[i - 1].x, p.y - points[i - 1].y) }
                let q = points[max(0, i - 1)], u = points[min(points.count - 1, i + 1)]
                let tx = u.x - q.x, ty = u.y - q.y, tl = nonZero(hypot(tx, ty))
                let d = wobble * sin(6.283 * along / period + phase)
                wobbled.append(CGPoint(x: p.x - ty / tl * d, y: p.y + tx / tl * d))
            }
            strokes.append(ribbon(wobbled) { profile.width($0) * scale })
        }
        return Box(kind: kind, strokes: strokes, tilt: tilt)
    }

    /// A stroke of varying width along any centreline, as a filled outline
    /// with round ends: `width(t)` at the share `t` of the way along.
    static func ribbon(_ p: [CGPoint], width: (CGFloat) -> CGFloat) -> Path {
        let n = p.count
        guard n > 1 else { return Path() }
        var acc: [CGFloat] = [0]
        for i in 1..<n { acc.append(acc[i - 1] + hypot(p[i].x - p[i - 1].x, p[i].y - p[i - 1].y)) }
        let total = nonZero(acc[n - 1])
        var left: [CGPoint] = [], right: [CGPoint] = [], tangent: [CGPoint] = [], half: [CGFloat] = []
        for i in 0..<n {
            let q = p[max(0, i - 1)], s = p[min(n - 1, i + 1)]
            let tx = s.x - q.x, ty = s.y - q.y, tl = nonZero(hypot(tx, ty))
            let h = width(acc[i] / total) / 2
            tangent.append(CGPoint(x: tx / tl, y: ty / tl))
            half.append(h)
            left.append(CGPoint(x: p[i].x - ty / tl * h, y: p[i].y + tx / tl * h))
            right.append(CGPoint(x: p[i].x + ty / tl * h, y: p[i].y - tx / tl * h))
        }
        /// Half a circle round `p`, from its left side to its right, bulging along `t` × `d`.
        func cap(_ p: CGPoint, _ t: CGPoint, _ h: CGFloat, _ d: CGFloat) -> [CGPoint] {
            (1..<6).map { k in
                let a = CGFloat(k) / 6 * .pi, c = cos(a), s = sin(a)
                return CGPoint(x: p.x + (-t.y * c * d) * h + t.x * s * h * d, y: p.y + (t.x * c * d) * h + t.y * s * h * d)
            }
        }
        let points = left + cap(p[n - 1], tangent[n - 1], half[n - 1], 1) + right.reversed() + cap(p[0], tangent[0], half[0], -1)
        var path = Path()
        path.addLines(points)
        path.closeSubpath()
        return path
    }

    /// Each point but the ends moved to the mean of the `k` either side of it.
    static func smoothPoints(_ p: [CGPoint], _ k: Int) -> [CGPoint] {
        p.indices.map { i in
            if i == 0 || i == p.count - 1 { return p[i] }
            var x: CGFloat = 0, y: CGFloat = 0, count: CGFloat = 0
            for j in max(0, i - k)...min(p.count - 1, i + k) {
                x += p[j].x
                y += p[j].y
                count += 1
            }
            return CGPoint(x: x / count, y: y / count)
        }
    }

    /// The page's `x || 1`.
    private static func nonZero(_ x: CGFloat) -> CGFloat { x == 0 ? 1 : x }
    private static func lerp(_ a: CGFloat, _ b: CGFloat, _ t: CGFloat) -> CGFloat { RejectCross.lerp(a, b, t) }
}
