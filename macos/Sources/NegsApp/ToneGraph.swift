import SwiftUI

// Tone graph for the print curves (ticket 20, prototype). The paper's real
// response to a ramp of greys, with the photograph's film exposure behind it;
// dragging it sets Print exposure and the print curves' gammas. Adapted from
// SpektraMac's Print tab "Contrast" graph (GPL-3.0).

enum ToneGraphPaths {
    static let printExposure = "enlarger.print_exposure"
    static let morph = "print_render.density_curves_morph"
    static let active = "\(morph).active"
    static let gamma = "\(morph).gamma_factor"
    static let fast = "\(morph).gamma_factor_fast"
    static let slow = "\(morph).gamma_factor_slow"
    static let exposureRange = 0.1...5.0
    static let gammaRange = 0.5...2.0
}

/// The controls the graph moves, as they act: the gammas are 1 while the
/// print curves are off.
struct ToneControls: Equatable {
    var printExposure = 1.0, gamma = 1.0, fast = 1.0, slow = 1.0

    init(printExposure: Double = 1, gamma: Double = 1, fast: Double = 1, slow: Double = 1) {
        (self.printExposure, self.gamma, self.fast, self.slow) = (printExposure, gamma, fast, slow)
    }

    init(_ value: (String) -> JSONValue) {
        func positive(_ path: String) -> Double { let v = value(path).double; return v > 0 ? v : 1 }
        printExposure = positive(ToneGraphPaths.printExposure)
        if value(ToneGraphPaths.active).bool {
            gamma = positive(ToneGraphPaths.gamma); fast = positive(ToneGraphPaths.fast); slow = positive(ToneGraphPaths.slow)
        }
    }
}

/// The helper's `tone_curve`: per grey, the film's exposure in stops, the
/// paper's log10 exposure and the scan, per channel.
struct ToneCurveData: Equatable {
    /// What it was computed for: film, paper and params.
    var request: JSONValue
    var controls: ToneControls
    /// Mean film exposure per grey, in stops (the x axis).
    var stops: [Double]
    /// Per channel, per grey.
    var paper: [[Double]]?
    var rgb: [[Double]]

    init?(_ result: JSONValue, request: JSONValue, controls: ToneControls) {
        let film = result["film"].array.map { $0.array.map(\.double) }
        let rgb = result["rgb"].array.map { $0.array.map(\.double) }
        guard film.count >= 2, film.count == rgb.count, film.allSatisfy({ $0.count == 3 }), rgb.allSatisfy({ $0.count == 3 }) else { return nil }
        self.request = request
        self.controls = controls
        stops = film.map { ($0[0] + $0[1] + $0[2]) / 3 }
        self.rgb = (0..<3).map { c in rgb.map { $0[c] } }
        let paper = result["paper"].array.map { $0.array.map(\.double) }
        self.paper = paper.count == film.count && paper.allSatisfy({ $0.count == 3 }) ? (0..<3).map { c in paper.map { $0[c] } } : nil
    }

    static func luma(_ r: Double, _ g: Double, _ b: Double) -> Double { 0.2126 * r + 0.7152 * g + 0.0722 * b }

    /// The curve's brightness per grey for `channel` (nil: neutral, the luma).
    func brightness(_ channel: Int?) -> [Double] {
        if let channel { return rgb[channel] }
        return stops.indices.map { Self.luma(rgb[0][$0], rgb[1][$0], rgb[2][$0]) }
    }

    /// Paper exposure per grey for `channel` (nil: the layers' mean).
    func paperExposure(_ channel: Int?) -> [Double]? {
        guard let paper else { return nil }
        if let channel { return paper[channel] }
        return stops.indices.map { (paper[0][$0] + paper[1][$0] + paper[2][$0]) / 3 }
    }

    /// The curve `controls` would give, predicted from this one: print
    /// exposure shifts the paper exposure by log10(new/old); the gammas
    /// scale it about 0 (slow above, fast below), and the brightness is
    /// this curve's at the scaled exposure. Exact for print exposure and
    /// overall gamma, close for fast and slow; the engine's next curve
    /// corrects it.
    func predicted(_ next: ToneControls) -> ToneDisplay {
        guard let paper, next != controls else {
            return ToneDisplay(stops: stops, rgb: rgb, paper: self.paper)
        }
        let shift = log10(next.printExposure / controls.printExposure)
        let gamma = next.gamma / controls.gamma
        let (slow, fast) = (next.slow / controls.slow, next.fast / controls.fast)
        var outRGB = rgb, outPaper = paper
        for c in 0..<3 {
            for i in stops.indices {
                let x = paper[c][i] + shift
                let k = gamma * (x >= 0 ? slow : fast)
                outPaper[c][i] = x
                outRGB[c][i] = Self.interpolate(rgb[c], at: Self.index(ofDecreasing: paper[c], k * x))
            }
        }
        return ToneDisplay(stops: stops, rgb: outRGB, paper: outPaper)
    }

    /// The fractional index where a decreasing series reaches `value`, clamped to the ends.
    static func index(ofDecreasing series: [Double], _ value: Double) -> Double {
        guard let first = series.first, let last = series.last else { return 0 }
        if value >= first { return 0 }
        if value <= last { return Double(series.count - 1) }
        var (lo, hi) = (0, series.count - 1)
        while hi - lo > 1 {
            let mid = (lo + hi) / 2
            if series[mid] > value { lo = mid } else { hi = mid }
        }
        let span = series[lo] - series[hi]
        return Double(lo) + (span > 0 ? (series[lo] - value) / span : 0)
    }

    static func index(ofIncreasing series: [Double], _ value: Double) -> Double {
        index(ofDecreasing: series.map { -$0 }, -value)
    }

    static func interpolate(_ series: [Double], at index: Double) -> Double {
        guard !series.isEmpty else { return 0 }
        let i = min(max(index, 0), Double(series.count - 1))
        let lo = Int(i.rounded(.down)), hi = min(lo + 1, series.count - 1)
        return series[lo] + (series[hi] - series[lo]) * (i - Double(lo))
    }
}

/// A curve as drawn: per channel brightness and paper exposure per grey.
struct ToneDisplay {
    var stops: [Double]
    var rgb: [[Double]]
    var paper: [[Double]]?

    func brightness(_ channel: Int?) -> [Double] {
        if let channel { return rgb[channel] }
        return stops.indices.map { ToneCurveData.luma(rgb[0][$0], rgb[1][$0], rgb[2][$0]) }
    }

    /// The handles' tones: where the paper's exposure is 0, and 2 stops
    /// either side. Nil if the paper exposure never crosses 0.
    func handles(_ channel: Int?) -> (shadows: Double, middle: Double, highlights: Double)? {
        guard let paper else { return nil }
        let series = channel.map { paper[$0] } ?? stops.indices.map { (paper[0][$0] + paper[1][$0] + paper[2][$0]) / 3 }
        guard let first = series.first, let last = series.last, first > 0, last < 0 else { return nil }
        let middle = ToneCurveData.interpolate(stops, at: ToneCurveData.index(ofDecreasing: series, 0))
        return (middle - 2, middle, middle + 2)
    }

    func value(_ series: [Double], atStops t: Double) -> Double {
        ToneCurveData.interpolate(series, at: ToneCurveData.index(ofIncreasing: stops, t))
    }
}

/// The photograph's film exposure: counts per layer over `stops`.
struct FilmHistogram: Equatable {
    var range: ClosedRange<Double>
    var counts: [[Double]]

    init?(_ value: JSONValue) {
        let counts = value["counts"].array.map { $0.array.map(\.double) }
        let stops = value["stops"].array.map(\.double)
        guard counts.count == 3, stops.count == 2, stops[1] > stops[0], let bins = counts.first?.count, bins > 1,
              counts.allSatisfy({ $0.count == bins }) else { return nil }
        range = stops[0]...stops[1]
        self.counts = counts
    }

    func bins(_ channel: Int?) -> [Double] {
        if let channel { return counts[channel] }
        return counts[0].indices.map { (counts[0][$0] + counts[1][$0] + counts[2][$0]) / 3 }
    }
}

enum ToneHandle: Equatable {
    case middle, shadows, highlights
    /// Anywhere else on the curve, at this tone in stops.
    case curve(Double)

    var title: String {
        switch self {
        case .middle: "Print exposure"
        case .shadows: "Shadow contrast"
        case .highlights: "Highlight contrast"
        case .curve: "Contrast"
        }
    }
    var path: String {
        switch self {
        case .middle: ToneGraphPaths.printExposure
        case .shadows: ToneGraphPaths.slow
        case .highlights: ToneGraphPaths.fast
        case .curve: ToneGraphPaths.gamma
        }
    }
    var hint: String {
        switch self {
        case .middle: "drag sideways"
        default: "drag up or down"
        }
    }
}

struct ToneGraphView: View {
    let model: EditorModel
    @AppStorage("toneGraphChannel") private var channelTag = "rgb"
    @State private var hovered: ToneHandle?
    @State private var drag: Drag?
    @State private var size = CGSize.zero

    private struct Drag {
        var handle: ToneHandle
        /// The engine curve and controls when the drag began; the drag's
        /// values are solved on it, so a newer curve mid-drag doesn't jitter.
        var base: ToneCurveData
        var start: ToneControls
        /// The tone the drag acts at, in stops.
        var tone: Double
        var started = false
    }

    private static let shown = -6.0...6.0
    private var channel: Int? { ["r": 0, "g": 1, "b": 2][channelTag] }
    private var controls: ToneControls { ToneControls { model.value($0) } }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(label).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                Picker("Channel", selection: $channelTag) {
                    Text("RGB").tag("rgb"); Text("R").tag("r"); Text("G").tag("g"); Text("B").tag("b")
                }
                .pickerStyle(.segmented).labelsHidden().controlSize(.mini).fixedSize()
            }
            graph.frame(height: 170)
        }
        .onAppear { model.toneGraphAppeared(true) }
        .onDisappear { model.toneGraphAppeared(false) }
    }

    private var label: String {
        let active = drag?.handle ?? hovered
        guard let active else {
            return model.toneCurve == nil ? "Tone graph" : "Drag the curve or a handle"
        }
        let value = model.value(active.path).double
        return "\(active.title) \(value.formatted(.number.precision(.fractionLength(2))))\(drag == nil ? " · \(active.hint)" : "")"
    }

    private var display: ToneDisplay? { model.toneCurve?.predicted(controls) }

    private var graph: some View {
        let display = display, channel = channel, histogram = model.filmHistogram
        return GeometryReader { geometry in
            Canvas { context, size in
                draw(&context, size: size, display: display, histogram: histogram, channel: channel)
            }
            .contentShape(Rectangle())
            .onAppear { size = geometry.size }
            .onChange(of: geometry.size) { size = $1 }
            .onContinuousHover { phase in
                guard drag == nil else { return }
                switch phase {
                case .active(let point): hovered = hit(point, display: display)
                case .ended: hovered = nil
                }
            }
            .gesture(DragGesture(minimumDistance: 1).onChanged { dragged($0, display: display) }.onEnded { _ in endDrag() })
            .onTapGesture(count: 2, coordinateSpace: .local) { point in
                guard let handle = hit(point, display: display) else { return }
                model.resetValue(handle.path)
            }
        }
        .background(Color(white: 0.1), in: RoundedRectangle(cornerRadius: 6))
        .accessibilityElement()
        .accessibilityLabel("Tone graph")
        .accessibilityValue("The print's brightness for each film exposure. Drag the middle handle sideways for print exposure, the side handles or the curve up or down for contrast.")
    }

    // MARK: Geometry

    private func x(_ stops: Double, _ size: CGSize) -> CGFloat {
        CGFloat((stops - Self.shown.lowerBound) / (Self.shown.upperBound - Self.shown.lowerBound)) * size.width
    }
    private func stops(_ x: CGFloat, _ size: CGSize) -> Double {
        Self.shown.lowerBound + Double(x / max(size.width, 1)) * (Self.shown.upperBound - Self.shown.lowerBound)
    }
    private static let inset: CGFloat = 8
    private func y(_ brightness: Double, _ size: CGSize) -> CGFloat {
        Self.inset + CGFloat(1 - min(max(brightness, 0), 1)) * (size.height - 2 * Self.inset)
    }
    private func brightness(_ y: CGFloat, _ size: CGSize) -> Double {
        min(max(1 - Double((y - Self.inset) / max(size.height - 2 * Self.inset, 1)), 0.001), 0.999)
    }

    private func handlePoints(_ display: ToneDisplay, channel: Int?) -> [(ToneHandle, CGPoint)] {
        guard let handles = display.handles(channel) else { return [] }
        let curve = display.brightness(channel)
        return [(.shadows, handles.shadows), (.middle, handles.middle), (.highlights, handles.highlights)].map { handle, t in
            (handle, CGPoint(x: x(t, size), y: y(display.value(curve, atStops: t), size)))
        }
    }

    private func hit(_ point: CGPoint, display: ToneDisplay?) -> ToneHandle? {
        guard let display else { return nil }
        if let (handle, _) = handlePoints(display, channel: channel)
            .map({ ($0.0, hypot($0.1.x - point.x, $0.1.y - point.y)) })
            .filter({ $0.1 < 10 }).min(by: { $0.1 < $1.1 }) {
            return handle
        }
        guard display.paper != nil else { return nil }
        let t = stops(point.x, size)
        let curveY = y(display.value(display.brightness(channel), atStops: t), size)
        return abs(curveY - point.y) < 12 ? .curve(t) : nil
    }

    // MARK: Dragging

    private func dragged(_ value: DragGesture.Value, display: ToneDisplay?) {
        if drag == nil {
            guard let base = model.toneCurve, let handle = hit(value.startLocation, display: display) else { return }
            let tone: Double
            switch handle {
            case .curve(let t): tone = t
            default:
                guard let handles = display?.handles(channel) else { return }
                tone = handle == .shadows ? handles.shadows : handle == .highlights ? handles.highlights : handles.middle
            }
            drag = Drag(handle: handle, base: base, start: controls, tone: tone)
            hovered = nil
            model.beginInteraction()
            if handle != .middle, !model.value(ToneGraphPaths.active).bool {
                model.set(ToneGraphPaths.active, .bool(true))
            }
        }
        guard let drag else { return }
        let base = drag.base, current = controls
        guard let paper = base.paperExposure(channel) else { return }
        let curve = base.brightness(channel)
        switch drag.handle {
        case .middle:
            // Paper exposure 0 under the pointer: new = base · 10^(−paper(t)).
            let t = stops(value.location.x, size)
            let p = ToneCurveData.interpolate(paper, at: ToneCurveData.index(ofIncreasing: base.stops, t))
            set(ToneGraphPaths.printExposure, base.controls.printExposure * pow(10, -p), range: ToneGraphPaths.exposureRange)
        case .shadows, .highlights, .curve:
            // The tone on the base curve that already has the pointer's
            // brightness; the scale is its paper exposure over the handle's.
            let target = brightness(value.location.y, size)
            let reached = ToneCurveData.interpolate(paper, at: ToneCurveData.index(ofIncreasing: curve, target))
            let shift = log10(current.printExposure / base.controls.printExposure)
            let at = ToneCurveData.interpolate(paper, at: ToneCurveData.index(ofIncreasing: base.stops, drag.tone)) + shift
            guard abs(at) > 0.03 else { return }
            let k = max(reached / at, 0.01)
            let side = at >= 0 ? (current.slow / base.controls.slow) : (current.fast / base.controls.fast)
            switch drag.handle {
            case .shadows:
                set(ToneGraphPaths.slow, base.controls.slow * k / (current.gamma / base.controls.gamma), range: ToneGraphPaths.gammaRange)
            case .highlights:
                set(ToneGraphPaths.fast, base.controls.fast * k / (current.gamma / base.controls.gamma), range: ToneGraphPaths.gammaRange)
            default:
                set(ToneGraphPaths.gamma, base.controls.gamma * k / side, range: ToneGraphPaths.gammaRange)
            }
        }
    }

    private func set(_ path: String, _ value: Double, range: ClosedRange<Double>) {
        guard value.isFinite else { return }
        model.set(path, .number(EditorModel.snap(value, to: range, step: 0.01)))
    }

    private func endDrag() {
        guard drag != nil else { return }
        drag = nil
        model.endInteraction(action: "Change Tone Curve")
    }

    // MARK: Drawing

    private func draw(_ context: inout GraphicsContext, size: CGSize, display: ToneDisplay?, histogram: FilmHistogram?, channel: Int?) {
        let colors: [Color] = [Color(red: 1, green: 0.35, blue: 0.35), Color(red: 0.35, green: 0.9, blue: 0.35), Color(red: 0.4, green: 0.55, blue: 1)]
        // Grid: every 2 stops, mid-grey dashed; brightness quarters.
        for stop in stride(from: -4.0, through: 4.0, by: 2.0) {
            var line = Path(); line.move(to: CGPoint(x: x(stop, size), y: 0)); line.addLine(to: CGPoint(x: x(stop, size), y: size.height))
            context.stroke(line, with: .color(.white.opacity(stop == 0 ? 0.18 : 0.07)), style: StrokeStyle(lineWidth: 1, dash: stop == 0 ? [3, 3] : []))
        }
        for level in [0.25, 0.5, 0.75] {
            var line = Path(); line.move(to: CGPoint(x: 0, y: y(level, size))); line.addLine(to: CGPoint(x: size.width, y: y(level, size)))
            context.stroke(line, with: .color(.white.opacity(0.07)), lineWidth: 1)
        }
        // Histogram of the photograph's film exposure, behind the curve.
        if let histogram {
            let bins = histogram.bins(channel), peak = bins.max() ?? 0
            if peak > 0 {
                var area = Path()
                let width = (histogram.range.upperBound - histogram.range.lowerBound) / Double(bins.count)
                area.move(to: CGPoint(x: x(histogram.range.lowerBound, size), y: size.height))
                for (i, count) in bins.enumerated() {
                    let t = histogram.range.lowerBound + (Double(i) + 0.5) * width
                    area.addLine(to: CGPoint(x: x(t, size), y: size.height - CGFloat(min(count / peak, 1)) * size.height * 0.55))
                }
                area.addLine(to: CGPoint(x: x(histogram.range.upperBound, size), y: size.height))
                area.closeSubpath()
                context.fill(area, with: .color((channel.map { colors[$0] } ?? .white).opacity(0.16)))
            }
        }
        guard let display else { return }
        func path(_ series: [Double]) -> Path {
            var path = Path()
            for (i, t) in display.stops.enumerated() {
                let point = CGPoint(x: x(t, size), y: y(series[i], size))
                if i == 0 { path.move(to: point) } else { path.addLine(to: point) }
            }
            return path
        }
        for c in 0..<3 {
            let strong = channel == c
            context.stroke(path(display.rgb[c]), with: .color(colors[c].opacity(strong ? 1 : 0.35)), lineWidth: strong ? 2 : 1)
        }
        if channel == nil {
            context.stroke(path(display.brightness(nil)), with: .color(.white), lineWidth: 2)
        }
        if case .curve(let t) = drag?.handle ?? hovered {
            let point = CGPoint(x: x(t, size), y: y(display.value(display.brightness(channel), atStops: drag?.tone ?? t), size))
            context.fill(Path(ellipseIn: CGRect(x: point.x - 3, y: point.y - 3, width: 6, height: 6)), with: .color(.white.opacity(0.6)))
        }
        for (handle, point) in handlePoints(display, channel: channel) {
            let active = handle == (drag?.handle ?? hovered)
            let r: CGFloat = active ? 6 : 4.5
            let rect = CGRect(x: point.x - r, y: point.y - r, width: 2 * r, height: 2 * r)
            let shape = handle == .middle ? Path(roundedRect: rect, cornerRadius: 1.5) : Path(ellipseIn: rect)
            context.fill(shape, with: .color(handle == .middle ? .yellow : .white))
            context.stroke(shape, with: .color(.black.opacity(0.8)), lineWidth: 1)
        }
    }
}
