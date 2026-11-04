import AppKit
import SwiftUI

/// Where the zones and scales sit on the histogram's 0…255 axis.
enum ZoneGeometry {
    /// L\* 0…100 across the graph, as the helper's lightness bins read.
    static func x(lstar: Double, width: CGFloat) -> CGFloat {
        CGFloat(min(max(lstar, 0), 100) * 2.55 / 255) * width
    }
    /// A green code (0…255) across the graph, as the EV scale places itself.
    static func x(greenBin: Int, width: CGFloat) -> CGFloat {
        width * CGFloat(min(max(greenBin, 0), 255)) / 255
    }
    /// Bands alternate from Zone I, which is lit; Zone 0 and the even zones
    /// are dark, so each band reads next to its neighbours.
    static func bandLit(_ zone: Int) -> Bool { zone % 2 == 1 }
    /// The gold numbers sit in two rows so the narrow shadow zones' labels
    /// don't collide; even zones above, odd below.
    static func numberRow(_ zone: Int) -> Int { zone % 2 }
    /// The rows' height: two rows of numbers, reserved whether or not a pick
    /// exists, so the graph never shifts.
    static let stripHeight: CGFloat = 22

    /// The L\* stripe's eleven steps, 0, 10, … 100, and their x.
    static func lStripe(width: CGFloat) -> [(value: Int, x: CGFloat)] {
        (0...10).map { step in
            let value = step * 10
            return (value, x(lstar: Double(value), width: width))
        }
    }
}

/// A green code, as the helper's ramp measures it.
/// One tick of the EV scale under the graph.
struct EvTick: Equatable {
    /// The scene's EV from 18 % grey.
    let ev: Double
    let x: CGFloat
    /// Whole stops are long; 1/3-EV ticks are short.
    let long: Bool
    /// The meter-calibration point at 12.5 %: taller and thicker.
    let midtone: Bool
}

extension ZoneGeometry {
    /// The scene values 2^(-n/3), n 0…48, positioned by the measured curve's
    /// green channel. Ticks on a plateau are left out: they would pile on one
    /// code and read as a single mark.
    static func evTicks(curve: ToneCurve?, width: CGFloat) -> [EvTick] {
        guard let curve, width > 0 else { return [] }
        return (0...48).compactMap { n in
            let ev = -Double(n) / 3
            guard let bin = curve.greenBin(atEV: ev) else { return nil }
            return EvTick(
                ev: ev, x: x(greenBin: bin, width: width),
                long: n % 3 == 0, midtone: n == 9
            )
        }
    }
}

/// How the readout's numbers and one-line caption read.
enum ZoneLabels {
    /// +1.2, −0.4, or an em dash where the print can't reach the zone.
    static func distance(_ value: Double?) -> String {
        guard let value else { return "—" }
        let magnitude = String(format: "%.1f", abs(value))
        return (value < 0 ? "−" : "+") + magnitude
    }

    /// The line beneath the graph: the picked tone and its codes, a hint, or
    /// why there are no numbers.
    static func headline(_ readout: ZoneReadout?, sample: ToneSample?) -> String {
        guard let sample else { return "⌃-click a tone to read its zones" }
        switch readout?.state {
        case .clipped: return "Clipped: pick a tone inside what the print can reach"
        case .unavailable:
            return "This print runs backwards, so the zones can't be placed"
        case .ready:
            return String(
                format: "L* %.1f · %.0f %.0f %.0f",
                sample.lstar, sample.red, sample.green, sample.blue
            )
        case nil:
            return "Measuring the print's curve…"
        }
    }

    /// A zone button's accessibility name, with its distance.
    static func zoneLabel(_ zone: Int, distance: Double?) -> String {
        "Zone \(ZoneLadder.names[zone]), \(distanceText(distance))"
    }

    static func distanceText(_ distance: Double?) -> String {
        guard let distance else { return "unreachable" }
        return distance == 0 ? "already on it" : "\(self.distance(distance)) EV"
    }
}

/// The gold numbers over the graph: one per zone, at the zone's starting
/// boundary, after a ⌃ pick. ⇧-click aims for the zone's centre. The numbers
/// take the clicks; the histogram image never does.
struct ZoneStrip: View {
    let model: EditorModel

    /// Numbers appear only when the readout is ready: a clipped pick or a
    /// print that runs backwards leaves the strip empty and the caption
    /// explaining why.
    static func showsNumbers(_ readout: ZoneReadout?) -> Bool {
        readout?.state == .ready
    }

    /// A zone is clickable only when it can be reached and the edit is
    /// settled, so a click never moves by a stale curve's distance.
    static func isEnabled(canPlace: Bool, distance: Double?) -> Bool {
        canPlace && distance != nil
    }

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .topLeading) {
                if Self.showsNumbers(model.zoneReadout) {
                    ForEach(0..<ZoneLadder.names.count, id: \.self) { zone in
                        button(zone: zone, width: geometry.size.width)
                    }
                }
            }
        }
        .frame(height: ZoneGeometry.stripHeight)
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private func button(zone: Int, width: CGFloat) -> some View {
        let distance = model.zoneReadout?.delta(zone: zone, centre: false)
        let enabled = Self.isEnabled(canPlace: model.canPlaceZone, distance: distance)
        Button {
            // ⇧ aims for the zone's centre.
            let centre = NSEvent.modifierFlags.contains(.shift)
            model.placeInZone(zone, centre: centre)
        } label: {
            Text(ZoneLabels.distance(distance))
                .font(.system(size: 9, weight: .semibold))
                .monospacedDigit()
                .foregroundStyle(enabled ? Color(red: 0.85, green: 0.66, blue: 0.22) : Color.secondary)
                .padding(.horizontal, 1)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .offset(
            x: ZoneGeometry.x(lstar: ZoneLadder.lstar[zone], width: width),
            y: CGFloat(ZoneGeometry.numberRow(zone)) * 11
        )
        .help(enabled
            ? "Move the picked tone to the start of Zone \(ZoneLadder.names[zone]); hold ⇧ for its centre."
            : "Zone \(ZoneLadder.names[zone]) can't be reached from this tone.")
        .accessibilityLabel(ZoneLabels.zoneLabel(zone, distance: distance))
    }
}
