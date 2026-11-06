import AppKit
import SwiftUI

/// The curve the pins make, to draw while it's being edited. The same rule as
/// the engine's `AnchorCurve`, which is what the photograph develops with:
/// `testTheCurveMatchesTheEngine` holds the two to the same numbers.
struct ToneCurve: Equatable {
    private let black: Double, white: Double
    private let points: [(input: Double, target: Double)]
    private let tangents: [Double]

    static func == (a: ToneCurve, b: ToneCurve) -> Bool {
        a.black == b.black && a.white == b.white && a.points.elementsEqual(b.points) { $0 == $1 }
    }

    init(anchors: [(input: Double, target: Double)]) {
        // Pins on one target are one reading of that tone.
        var zones: [Int: (sum: Double, count: Int)] = [:]
        for anchor in anchors where anchor.input.isFinite && anchor.target.isFinite && (0..<100).contains(anchor.input) {
            let key = Int((min(max(anchor.target, 0), 100) * 2).rounded())
            zones[key, default: (0, 0)].sum += anchor.input
            zones[key, default: (0, 0)].count += 1
        }
        var (black, white) = (0.0, 100.0)
        var interior: [(input: Double, target: Double)] = []
        for (key, zone) in zones {
            let (input, target) = (zone.sum / Double(zone.count), Double(key) / 2)
            if target <= 0 { black = input } else if target >= 100 { white = input } else { interior.append((input, target)) }
        }
        if white <= black + 0.5 { white = 100 }
        interior.sort { ($0.input, $0.target) < ($1.input, $1.target) }
        // The curve must rise, so a zone that would turn it back has no effect.
        var kept: [(input: Double, target: Double)] = []
        for point in interior where point.input > black + 0.5 && point.input < white - 0.5 {
            if let last = kept.last, point.input - last.input < 0.5 || point.target - last.target < 0.5 { continue }
            kept.append(point)
        }
        self.black = black; self.white = white; points = kept
        tangents = Self.tangents(black: black, white: white, points: kept)
    }

    private static func tangents(black: Double, white: Double, points: [(input: Double, target: Double)]) -> [Double] {
        let knots = [(input: black, target: 0.0)] + points + [(input: white, target: 100.0)]
        let widths = zip(knots, knots.dropFirst()).map { $1.input - $0.input }
        let slopes = zip(zip(knots, knots.dropFirst()), widths).map { ($0.1.target - $0.0.target) / $1 }
        func end(_ h0: Double, _ h1: Double, _ s0: Double, _ s1: Double) -> Double {
            min(max(((2 * h0 + h1) * s0 - h0 * s1) / (h0 + h1), 0), 3 * s0)
        }
        let last = slopes.count - 1
        var result = [Double](repeating: 0, count: knots.count)
        if slopes.count == 1 {
            result = [slopes[0], slopes[0]]
        } else {
            result[0] = end(widths[0], widths[1], slopes[0], slopes[1])
            result[last + 1] = end(widths[last], widths[last - 1], slopes[last], slopes[last - 1])
            for i in 1...last {
                let (h0, h1) = (widths[i - 1], widths[i])
                let (w0, w1) = (2 * h1 + h0, h1 + 2 * h0)
                result[i] = (w0 + w1) / (w0 / slopes[i - 1] + w1 / slopes[i])
            }
        }
        if black > 0 { result[0] = 0 }
        if white < 100 { result[last + 1] = 0 }
        return result
    }

    /// The printed L\* of a measured one.
    func map(_ value: Double) -> Double {
        if black <= 0, white >= 100, points.isEmpty { return value }
        if !value.isFinite || value <= black { return 0 }
        if value >= white { return 100 }
        let index = points.filter { $0.input <= value }.count
        func knot(_ i: Int) -> (input: Double, target: Double) {
            i == 0 ? (black, 0) : i > points.count ? (white, 100) : points[i - 1]
        }
        let (a, b) = (knot(index), knot(index + 1))
        let h = b.input - a.input, t = (value - a.input) / h
        let (t2, t3) = (t * t, t * t * t)
        let y = (2 * t3 - 3 * t2 + 1) * a.target + (t3 - 2 * t2 + t) * h * tangents[index]
            + (-2 * t3 + 3 * t2) * b.target + (t3 - t2) * h * tangents[index + 1]
        return min(max(y, a.target), b.target)
    }
}

/// A point on the tone curve: a zone's pins, read as one, or a point set by hand.
struct CurvePoint: Identifiable, Equatable {
    var id: Double { target }
    let input: Double
    let target: Double
    /// The anchors behind it.
    let anchors: [Int]
    /// Measured from the photograph, rather than set here.
    let pinned: Bool
    let ignored: Bool
}

/// The whole curve over a faint histogram. Drag a point to change it, which
/// turns a pin into a plain point; click to add one; drag one out to remove it.
struct ToneCurveBox: View {
    let model: EditorModel
    @State private var dragging: [Int]?
    @State private var moved = false
    @State private var outside = false
    @State private var start = CGPoint.zero

    /// How far past the box a point has to be let go to come off.
    static let removeDistance: CGFloat = 24
    /// How close a click has to be to take hold of a point.
    static let reach: CGFloat = 10

    var body: some View {
        let points = model.curvePoints
        VStack(spacing: 8) {
            GeometryReader { geometry in
                ZStack {
                    HistogramView(histogram: model.histogram).opacity(0.3)
                    Canvas { context, size in draw(&context, size: size, points: points) }
                }
                // AppKit, since SwiftUI doesn't pass mouse drags through the Form's scrolling.
                .overlay {
                    CurveMouseSurface { event in
                        // It sits outside SwiftUI's disabling.
                        guard model.source != nil, !model.cropping else { return }
                        switch event {
                        case .down(let point): start = point; changed(at: point, size: geometry.size)
                        case .dragged(let point): changed(at: point, size: geometry.size)
                        case .up: ended()
                        }
                    }
                }
            }
            .frame(height: 170)
            .accessibilityElement()
            .accessibilityLabel("Tone Curve")
            .accessibilityValue(points.isEmpty ? "Straight" : "\(points.count) points")
            HStack {
                Button("Copy") { model.copyToneCurve() }.disabled(points.isEmpty)
                Button("Paste") { model.pasteToneCurve() }.disabled(!model.canPasteToneCurve)
                Spacer()
                Button("Reset") { model.resetToneCurve() }.disabled(points.isEmpty)
            }
            .buttonStyle(.borderless)
        }
        .disabled(model.source == nil || model.cropping)
    }

    static func position(input: Double, target: Double, size: CGSize) -> CGPoint {
        CGPoint(x: size.width * input / 100, y: size.height * (1 - target / 100))
    }

    private func draw(_ context: inout GraphicsContext, size: CGSize, points: [CurvePoint]) {
        var grid = Path()
        for step in 1..<4 {
            let (x, y) = (size.width * CGFloat(step) / 4, size.height * CGFloat(step) / 4)
            grid.move(to: CGPoint(x: x, y: 0)); grid.addLine(to: CGPoint(x: x, y: size.height))
            grid.move(to: CGPoint(x: 0, y: y)); grid.addLine(to: CGPoint(x: size.width, y: y))
        }
        context.stroke(grid, with: .color(EdgePrint.amber.opacity(0.12)), lineWidth: 1)
        let curve = model.toneCurve
        var line = Path()
        let samples = max(Int(size.width), 2)
        for sample in 0...samples {
            let input = 100 * Double(sample) / Double(samples)
            let point = Self.position(input: input, target: curve.map(input), size: size)
            if sample == 0 { line.move(to: point) } else { line.addLine(to: point) }
        }
        context.stroke(line, with: .color(EdgePrint.amber), lineWidth: 1.5)
        for point in points {
            let centre = Self.position(input: point.input, target: point.target, size: size)
            let selected = model.selectedPin.map(point.anchors.contains) ?? false
            let radius: CGFloat = selected ? 5.5 : 4
            let dot = Path(ellipseIn: CGRect(x: centre.x - radius, y: centre.y - radius, width: radius * 2, height: radius * 2))
            let colour = point.ignored ? Color.red : EdgePrint.amber
            context.opacity = outside && dragging == point.anchors ? 0.3 : 1
            context.fill(dot, with: .color(point.pinned ? colour : .black))
            context.stroke(dot, with: .color(colour), lineWidth: 1.5)
            context.opacity = 1
        }
    }

    private func changed(at location: CGPoint, size: CGSize) {
        let input = 100 * location.x / max(size.width, 1)
        let target = 100 * (1 - location.y / max(size.height, 1))
        guard let held = dragging else {
            let near = model.curvePoints.map { point -> (CurvePoint, CGFloat) in
                let centre = Self.position(input: point.input, target: point.target, size: size)
                return (point, hypot(centre.x - location.x, centre.y - location.y))
            }.filter { $0.1 <= Self.reach }.min { $0.1 < $1.1 }?.0
            if let near {
                dragging = near.anchors
                model.selectPin(near.anchors.first)
            } else {
                model.beginInteraction()
                moved = true
                dragging = model.addCurvePoint(input: input, target: target)
            }
            return
        }
        if !moved {
            guard hypot(location.x - start.x, location.y - start.y) > 1 else { return }
            model.beginInteraction()
            moved = true
        }
        let r = Self.removeDistance
        outside = !CGRect(origin: .zero, size: size).insetBy(dx: -r, dy: -r).contains(location)
        if !outside { dragging = model.moveCurvePoint(held, input: input, target: target) }
    }

    private func ended() {
        if outside, let dragging { model.removeCurvePoint(dragging) }
        if moved { model.endInteraction(action: "Edit Tone Curve") }
        dragging = nil; moved = false; outside = false
    }
}

/// The mouse over the curve, in points from the box's top left.
private struct CurveMouseSurface: NSViewRepresentable {
    enum Event { case down(CGPoint), dragged(CGPoint), up }
    let handle: (Event) -> Void

    final class Surface: NSView {
        var handle: (Event) -> Void = { _ in }
        override var isFlipped: Bool { true }
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        private func point(_ event: NSEvent) -> CGPoint { convert(event.locationInWindow, from: nil) }
        override func mouseDown(with event: NSEvent) { handle(.down(point(event))) }
        override func mouseDragged(with event: NSEvent) { handle(.dragged(point(event))) }
        override func mouseUp(with event: NSEvent) { handle(.up) }
    }

    func makeNSView(context: Context) -> Surface { Surface() }
    func updateNSView(_ view: Surface, context: Context) { view.handle = handle }
}
