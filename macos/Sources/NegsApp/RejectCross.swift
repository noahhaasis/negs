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
/// pressure. This is a port of the page's script, with the same hash and the
/// same random numbers in the same order, so the drawing and the app agree.
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
    static func hash(_ name: String) -> UInt32 {
        var h: UInt32 = 2_166_136_261
        for unit in name.utf16 { h = (h ^ UInt32(unit)) &* 16_777_619 }
        return h
    }

    /// mulberry32, as the page's `rng`.
    struct Random {
        var state: UInt32
        init(seed: UInt32) { state = seed }
        mutating func next() -> CGFloat {
            state = state &+ 0x6D2B_79F5
            var t = (state ^ (state >> 15)) &* (1 | state)
            t = (t &+ ((t ^ (t >> 7)) &* (61 | t))) ^ t
            return CGFloat(t ^ (t >> 14)) / 4_294_967_296
        }
    }

    /// Width along a stroke, from keyframes `(t, width)` eased between.
    struct Profile {
        let keys: [(CGFloat, CGFloat)]
        func width(_ t: CGFloat) -> CGFloat {
            for i in 1..<keys.count where t <= keys[i].0 {
                let (t0, w0) = keys[i - 1], (t1, w1) = keys[i]
                return lerp(w0, w1, smooth((t - t0) / (t1 - t0)))
            }
            return keys[keys.count - 1].1
        }
    }

    /// One way of drawing the cross. Lengths are in points.
    struct Style {
        var inset: CGFloat
        var jitter: CGFloat
        /// How far short of its first corner a stroke starts (+), as a range.
        var start: (CGFloat, CGFloat)?
        /// How far short of its last corner a stroke ends: − runs past it.
        var end: (CGFloat, CGFloat)?
        /// The bow at the middle, as a share of the stroke's length.
        var bow: CGFloat = 0
        /// The chance a stroke bows the other way.
        var bowFlip: CGFloat = 0.3
        var wobble: CGFloat = 0
        var passes = 1
        var retrace: CGFloat = 0
        var passOpacity: Double = 1
        var profile: Profile
        var scale: CGFloat = 1
        var opacity: Double
        /// The second stroke's width scale and opacity, if they differ.
        var second: (scale: CGFloat, opacity: Double)?
    }

    static let loose = Style(inset: 12, jitter: 3, start: (10, 24), end: (-14, -5), bowFlip: 0.5,
                             profile: Profile(keys: [(0, 2.2), (0.1, 3), (0.7, 2.4), (1, 0.2)]), opacity: 0.9)
    static let twice = Style(inset: 8, jitter: 2, start: nil, end: nil, bow: 0.02, wobble: 0.45, passes: 2,
                             profile: Profile(keys: [(0, 0.7), (0.12, 1.7), (0.8, 1.5), (1, 0.4)]), opacity: 0.95)

    /// The style a name draws, with its own spread (the page's `mixed`).
    static func style(for name: String, random: inout Random) -> (Kind, Style) {
        if random.next() < 0.5 {
            var style = loose
            style.scale = lerp(0.85, 1.15, random.next())
            style.bow = lerp(0.025, 0.05, random.next())
            style.wobble = lerp(0.5, 1, random.next())
            style.second = (lerp(0.8, 1, random.next()), 0.8)
            return (.loose, style)
        }
        var style = twice
        style.scale = lerp(0.85, 1.2, random.next())
        style.retrace = lerp(1.2, 2.3, random.next())
        style.passOpacity = Double(lerp(0.55, 0.8, random.next()))
        return (.twice, style)
    }

    static func kind(for name: String) -> Kind {
        var random = Random(seed: hash(name))
        return style(for: name, random: &random).0
    }

    /// The strokes for a picture of this size: top left to bottom right,
    /// then top right to bottom left, as a hand draws an X.
    static func strokes(in size: CGSize, name: String) -> [Stroke] {
        guard size.width > 0, size.height > 0 else { return [] }
        var random = Random(seed: hash(name))
        let style = style(for: name, random: &random).1
        let m = style.inset, w = size.width, h = size.height
        func jitter() -> CGFloat { (random.next() - 0.5) * 2 * style.jitter }
        let corners = [(CGPoint(x: m, y: m), CGPoint(x: w - m, y: h - m)), (CGPoint(x: w - m, y: m), CGPoint(x: m, y: h - m))]
        var strokes: [Stroke] = []
        for (k, (a, b)) in corners.enumerated() {
            let dx = b.x - a.x, dy = b.y - a.y, length = hypot(dx, dy)
            guard length > 0 else { continue }
            let ux = dx / length, uy = dy / length
            let s = style.start.map { lerp($0.0, $0.1, random.next()) } ?? 0
            let e = style.end.map { lerp($0.0, $0.1, random.next()) } ?? 0
            let ax = a.x + ux * s + jitter(), ay = a.y + uy * s + jitter()
            let bx = b.x - ux * e + jitter(), by = b.y - uy * e + jitter()
            var scale = style.scale, opacity = style.opacity
            if k == 1, let second = style.second { (scale, opacity) = second }
            for pass in 0..<style.passes {
                let off = pass > 0 ? style.retrace * (random.next() < 0.5 ? -1 : 1) : 0
                let nx = -uy * off, ny = ux * off
                let from = CGPoint(x: ax + nx + jitter() * 0.4, y: ay + ny)
                let to = CGPoint(x: bx + nx, y: by + ny + jitter() * 0.4)
                let path = outline(from: from, to: to, style: style, scale: scale, random: &random)
                strokes.append(Stroke(path: path, opacity: opacity * (pass > 0 ? style.passOpacity : 1)))
            }
        }
        return strokes
    }

    /// One stroke as a filled outline: a bowed, slightly wobbling line from
    /// `a` to `b`, its width following the profile, with round ends.
    static func outline(from a: CGPoint, to b: CGPoint, style: Style, scale: CGFloat, random: inout Random) -> Path {
        let dx = b.x - a.x, dy = b.y - a.y, length = hypot(dx, dy)
        let nx = -dy / length, ny = dx / length
        let bowSize = style.bow * length * (0.6 + 0.8 * random.next())
        let bow = bowSize * (random.next() < style.bowFlip ? -1 : 1)
        let cx = (a.x + b.x) / 2 + nx * bow * 2, cy = (a.y + b.y) / 2 + ny * bow * 2
        let amp = style.wobble * (0.5 + random.next())
        let frequency = 1.2 + 1.6 * random.next()
        let phase = random.next() * 6.28
        let n = 56
        var centre: [(CGPoint, CGFloat)] = []
        for i in 0...n {
            let t = CGFloat(i) / CGFloat(n), u = 1 - t
            let wave = sin(.pi * t) * amp * sin(6.283 * frequency * t + phase)
            let x = u * u * a.x + 2 * u * t * cx + t * t * b.x + nx * wave
            let y = u * u * a.y + 2 * u * t * cy + t * t * b.y + ny * wave
            centre.append((CGPoint(x: x, y: y), style.profile.width(t) * scale))
        }
        var left: [CGPoint] = [], right: [CGPoint] = []
        for i in 0...n {
            let (p, width) = centre[i], q = centre[max(0, i - 1)].0, s = centre[min(n, i + 1)].0
            let tx = s.x - q.x, ty = s.y - q.y, tl = hypot(tx, ty) == 0 ? 1 : hypot(tx, ty)
            let half = width / 2
            left.append(CGPoint(x: p.x - ty / tl * half, y: p.y + tx / tl * half))
            right.append(CGPoint(x: p.x + ty / tl * half, y: p.y - tx / tl * half))
        }
        /// Half a circle round `p`, from one side of the stroke to the other.
        func cap(_ p: (CGPoint, CGFloat), from: CGPoint, to: CGPoint) -> [CGPoint] {
            let a0 = atan2(from.y - p.0.y, from.x - p.0.x), half = p.1 / 2
            var a1 = atan2(to.y - p.0.y, to.x - p.0.x)
            while a1 < a0 { a1 += 6.283 }
            if a1 - a0 > 3.3 { a1 -= 6.283 }
            return (1..<6).map { k in
                let angle = lerp(a0, a1, CGFloat(k) / 6)
                return CGPoint(x: p.0.x + cos(angle) * half, y: p.0.y + sin(angle) * half)
            }
        }
        let points = left + cap(centre[n], from: left[n], to: right[n]) + right.reversed() + cap(centre[0], from: right[0], to: left[0])
        var path = Path()
        path.addLines(points)
        path.closeSubpath()
        return path
    }

    private static func lerp(_ a: CGFloat, _ b: CGFloat, _ t: CGFloat) -> CGFloat { a + (b - a) * t }
    private static func smooth(_ t: CGFloat) -> CGFloat { t * t * (3 - 2 * t) }
}
