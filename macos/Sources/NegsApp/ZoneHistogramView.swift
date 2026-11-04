import SwiftUI

/// One zone of the print, as a band on the L\* axis.
struct Zone: Identifiable, Hashable, Sendable {
    let index: Int
    let numeral: String
    let name: String
    /// The band's edges on the 0…100 L\* axis.
    let lStarRange: ClosedRange<Double>
    var id: Int { index }
    var centre: Double { (lStarRange.lowerBound + lStarRange.upperBound) / 2 }
    /// Where a pin on this band lands: black for zone 0, paper white for the
    /// last zone, and the middle of the band in between.
    var target: Double {
        if lStarRange.lowerBound <= 0 { return 0 }
        if lStarRange.upperBound >= 100 { return 100 }
        return centre
    }
}

/// The zones on the L\* axis, from the same ladder the engine's tone curve
/// uses (`ZoneLadder`): zones 0–VII one stop apart from 18 % grey, then
/// zone VIII, paper white.
enum ZoneScale {
    static let names = [
        "Pure black", "Near black", "First texture", "Dark detail",
        "Open shadow", "Middle grey", "Light midtone", "Light texture", "Paper white"
    ]

    static let zones: [Zone] = {
        var out: [Zone] = []
        for index in ZoneLadder.names.indices {
            let lower = ZoneLadder.lstar[index]
            guard lower < 100 else { break }
            let upper = min(100, ZoneLadder.lstar[min(index + 1, ZoneLadder.lstar.count - 1)])
            out.append(Zone(
                index: index, numeral: ZoneLadder.names[index], name: names[index],
                lStarRange: lower...upper
            ))
        }
        return out
    }()

    /// The band containing a tone, L\* 0…100.
    static func zone(forTarget target: Double) -> Int {
        zones.last { $0.lStarRange.lowerBound <= target }?.index ?? 0
    }
}

/// The histogram on the L\* axis: four arrays of 256 raw counts. Bin `i` is
/// plotted at L\* `i / 255 × 100`.
struct ToneHistogram: Sendable {
    var red: [Double]
    var green: [Double]
    var blue: [Double]
    /// The photograph's own CIE L\*.
    var luminance: [Double]
    /// The tallest bar, leaving out the clipped ends so their spikes don't
    /// flatten the rest. Lightness never sets the scale.
    let peak: Double

    static let empty = ToneHistogram(
        red: Array(repeating: 0, count: 256), green: Array(repeating: 0, count: 256),
        blue: Array(repeating: 0, count: 256), luminance: Array(repeating: 0, count: 256)
    )

    init(red: [Double], green: [Double], blue: [Double], luminance: [Double]) {
        self.red = red; self.green = green; self.blue = blue; self.luminance = luminance
        let channels = [red, green, blue]
        let inner = channels.map { $0.count > 250 ? $0[5...250].max() ?? 0 : 0 }.max() ?? 0
        peak = inner > 0 ? inner : channels.map { $0.max() ?? 0 }.max() ?? 0
    }

    init(_ histogram: Histogram) {
        self.init(
            red: histogram.red.map(Double.init), green: histogram.green.map(Double.init),
            blue: histogram.blue.map(Double.init), luminance: histogram.lightness.map(Double.init)
        )
    }

    /// Fraction (0…1) of the photograph's lightness bins inside a zone.
    func share(of zone: Zone) -> Double {
        let count = luminance.count
        guard count > 1 else { return 0 }
        let total = luminance.reduce(0, +)
        guard total > 0 else { return 0 }
        let isLast = zone.index == ZoneScale.zones.last?.index
        var sum = 0.0
        for index in 0..<count {
            let value = Double(index) / Double(count - 1) * 100
            if value >= zone.lStarRange.lowerBound
                && (value < zone.lStarRange.upperBound || (isLast && value <= 100)) {
                sum += luminance[index]
            }
        }
        return sum / total
    }
}

/// Fixed dark values so the chart matches the inspector panel.
private enum ZonePalette {
    static let bandEven = Color(hex: 0x1C1C1E)
    static let bandOdd = Color(hex: 0x202022)
    static let bandHover = Color(hex: 0x2E2E31)
    static let bandSelected = Color(hex: 0x0A84FF).opacity(0.28)
    static let divider = Color.white.opacity(0.045)
    static let frame = Color.white.opacity(0.06)
    static let textPrimary = Color(hex: 0xF2F2F7)
    static let textSecondary = Color(hex: 0x8E8E93)
    static let accent = Color(hex: 0x0A84FF)
    static let accentText = Color(hex: 0x64B0FF)
    static let red = Color(hex: 0xFF453A)
    static let green = Color(hex: 0x32D74B)
    static let blue = Color(hex: 0x0A84FF)
    static let blueStroke = Color(hex: 0x409CFF)
}

private extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1
        )
    }
}

/// The tone histogram drawn over the print's zones. Each band is a button:
/// with a pin selected it aims the pin there, and otherwise it holds that
/// band's reading in the caption. The curves never take the pointer.
struct ZoneHistogramView: View {
    let histogram: ToneHistogram
    /// A tone can be measured on the photograph right now.
    var canPin = false
    var pins: [TonePin] = []
    var selectedPin: Int?
    /// A band was clicked with a pin selected.
    var onAim: (Int) -> Void = { _ in }
    var chartHeight: CGFloat = 124

    @State private var hoveredZone: Int?
    /// The band a click is holding in the caption.
    @State private var readZone: Int?

    private var zones: [Zone] { ZoneScale.zones }

    private var pin: TonePin? {
        guard let selectedPin, pins.indices.contains(selectedPin) else { return nil }
        return pins[selectedPin]
    }

    /// The lit band: the selected pin's, else the one held for reading.
    private var highlightedZone: Int? { pin?.zone ?? readZone }

    /// Hover wins over the highlight for the caption.
    private var activeZone: Zone? {
        guard let index = hoveredZone ?? highlightedZone else { return nil }
        return zones.first { $0.index == index }
    }

    /// What clicking a band will do, when it does more than hold the reading.
    static func clickHint(pinSelected: Bool) -> String? {
        pinSelected ? "Click to move here" : nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    bands(width: geometry.size.width)
                    HistogramCurves(histogram: histogram)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                }
            }
            .frame(height: chartHeight)
            .clipped()
            .overlay(Rectangle().strokeBorder(ZonePalette.frame, lineWidth: 1).allowsHitTesting(false))

            caption.frame(height: 18)
        }
    }

    private func bands(width: CGFloat) -> some View {
        HStack(spacing: 0) {
            ForEach(zones) { zone in
                let span = zone.lStarRange.upperBound - zone.lStarRange.lowerBound
                ZoneBand(
                    zone: zone,
                    share: histogram.share(of: zone),
                    isHovered: hoveredZone == zone.index,
                    isSelected: highlightedZone == zone.index
                ) {
                    if pin != nil {
                        onAim(zone.index)
                    } else {
                        readZone = readZone == zone.index ? nil : zone.index
                    }
                }
                .frame(width: width * span / 100)
                .onHover { inside in
                    if inside {
                        hoveredZone = zone.index
                    } else if hoveredZone == zone.index {
                        hoveredZone = nil
                    }
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Exposure zones")
    }

    @ViewBuilder private var caption: some View {
        HStack(spacing: 6) {
            if let pin, hoveredZone == nil {
                Text("Pin \(pin.number)").fontWeight(.semibold).foregroundStyle(ZonePalette.textPrimary)
                Text("L* \(pin.input.formatted(.number.precision(.fractionLength(1)))) → Zone \(ZoneLadder.names[pin.zone])")
                    .foregroundStyle(ZonePalette.textSecondary)
                Spacer(minLength: 0)
                Text("⌫ deletes").foregroundStyle(ZonePalette.textSecondary)
            } else if let zone = activeZone {
                Text("Zone \(zone.numeral)").fontWeight(.semibold).foregroundStyle(ZonePalette.textPrimary)
                Text(zone.name).foregroundStyle(ZonePalette.textSecondary)
                Spacer(minLength: 0)
                // The hint replaces the share: both don't fit the narrowest inspector.
                if let hint = Self.clickHint(pinSelected: pin != nil) {
                    Text(hint).foregroundStyle(ZonePalette.accentText)
                } else {
                    Text(histogram.share(of: zone), format: .percent.precision(.fractionLength(1)))
                        .monospacedDigit()
                        .foregroundStyle(ZonePalette.textPrimary)
                }
            } else {
                Text(canPin ? "⌘-click or ⌘-drag a tone to pin it" : "Hover a zone to read it")
                    .foregroundStyle(ZonePalette.textSecondary)
                Spacer(minLength: 0)
            }
        }
        .font(.system(size: 12))
        .lineLimit(1)
        .accessibilityElement(children: .combine)
    }
}

/// A zone band: background, hover/selection bar, and the numeral.
private struct ZoneBand: View {
    let zone: Zone
    let share: Double
    let isHovered: Bool
    let isSelected: Bool
    let action: () -> Void

    private var background: Color {
        if isSelected { return ZonePalette.bandSelected }
        if isHovered { return ZonePalette.bandHover }
        return zone.index.isMultiple(of: 2) ? ZonePalette.bandEven : ZonePalette.bandOdd
    }
    private var barColor: Color {
        if isSelected { return ZonePalette.accent }
        if isHovered { return ZonePalette.textPrimary.opacity(0.55) }
        return .clear
    }
    private var labelColor: Color {
        if isSelected { return ZonePalette.accentText }
        if isHovered { return ZonePalette.textPrimary }
        return ZonePalette.textSecondary
    }

    var body: some View {
        Button(action: action) {
            ZStack(alignment: .top) {
                Rectangle().fill(background)
                Rectangle().fill(barColor).frame(height: 2)
                Text(zone.numeral)
                    .font(.system(size: 10, weight: .semibold))
                    .tracking(0.2)
                    .foregroundStyle(labelColor)
                    .fixedSize()
                    .padding(.top, 6)
            }
            .overlay(alignment: .trailing) {
                Rectangle().fill(ZonePalette.divider).frame(width: 1)
            }
            .clipped()
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .animation(.easeOut(duration: 0.12), value: isHovered)
        .animation(.easeOut(duration: 0.12), value: isSelected)
        .accessibilityLabel(
            "Zone \(zone.numeral), \(zone.name), \(share.formatted(.percent.precision(.fractionLength(1)))) of pixels"
        )
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// R, G and B as screened fills with thin strokes, the photograph's L\* as an
/// open line over them.
private struct HistogramCurves: View {
    let histogram: ToneHistogram

    var body: some View {
        Canvas { context, size in
            let peak = max(histogram.peak, .leastNonzeroMagnitude)
            let channels: [([Double], Color, Double, Color, Double)] = [
                (histogram.red, ZonePalette.red, 0.32, ZonePalette.red, 0.80),
                (histogram.green, ZonePalette.green, 0.32, ZonePalette.green, 0.80),
                (histogram.blue, ZonePalette.blue, 0.36, ZonePalette.blueStroke, 0.85)
            ]
            context.blendMode = .screen
            for (values, fill, fillOpacity, stroke, strokeOpacity) in channels {
                let path = Self.path(values, peak: peak, size: size, closed: true)
                context.fill(path, with: .color(fill.opacity(fillOpacity)))
                context.stroke(path, with: .color(stroke.opacity(strokeOpacity)), lineWidth: 0.75)
            }
            context.blendMode = .normal
            context.stroke(
                Self.path(histogram.luminance, peak: peak, size: size, closed: false),
                with: .color(ZonePalette.textPrimary.opacity(0.9)), lineWidth: 1
            )
        }
    }

    /// The baseline sits 2pt below the visible area so zero-value strokes are
    /// clipped away; the top 22pt stays clear for the numerals.
    static func path(_ values: [Double], peak: Double, size: CGSize, closed: Bool) -> Path {
        var path = Path()
        guard values.count > 1 else { return path }
        let base = size.height + 2
        let top: CGFloat = 22
        func point(_ index: Int) -> CGPoint {
            let x = CGFloat(index) / CGFloat(values.count - 1) * size.width
            let y = base - CGFloat(min(1, values[index] / peak)) * (base - top)
            return CGPoint(x: x, y: y)
        }
        if closed {
            path.move(to: CGPoint(x: 0, y: base))
            for index in values.indices { path.addLine(to: point(index)) }
            path.addLine(to: CGPoint(x: size.width, y: base))
            path.closeSubpath()
        } else {
            path.move(to: point(0))
            for index in 1..<values.count { path.addLine(to: point(index)) }
        }
        return path
    }
}
