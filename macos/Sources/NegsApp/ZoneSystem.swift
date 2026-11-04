import Foundation

/// The zone ladder: where each zone begins, in CIE Lab lightness.
enum ZoneLadder {
    /// Zone 0, then I…VII.
    static let names = ["0", "I", "II", "III", "IV", "V", "VI", "VII"]

    /// The nine boundaries, L\* 0…100.
    ///
    /// Zone I begins at the scene level 18 % × 2^(1 − 5.5); each next zone is
    /// one stop whiter, so the ladder is `Y = 18 / 2^5.5 · 2^i`. Zone 0 begins
    /// at black, and Zone VII ends at paper white (the last level is a little
    /// beyond 18 %, so it is held at 100).
    static let lstar: [Double] = (0...8).map { index -> Double in
        guard index > 0 else { return 0 }
        let y = 0.18 / pow(2, 5.5) * pow(2, Double(index))
        let lstar = y > 216.0 / 24389.0 ? 116 * cbrt(y) - 16 : y * 24389.0 / 27.0
        return min(lstar, 100)
    }
}

/// The film-and-print curve the helper measures, as output L\* against scene
/// EV. The zone readout inverts it: how many stops of `camera.placement_ev`
/// land a picked tone on a zone.
struct ToneCurve: Equatable, Sendable {
    let evMin: Double
    let evStep: Double
    let lstar: [Double]
    /// The output-encoded green channel at each sample, in [0, 1]: the EV
    /// scale places its ticks by the same values as the histogram's bars.
    let green: [Double]

    /// Nil for an empty or malformed curve; the zones then stay unavailable.
    init?(evMin: Double, evStep: Double, lstar: [Double], green: [Double]) {
        guard !lstar.isEmpty, lstar.count == green.count,
              evMin.isFinite, evStep.isFinite, evStep > 0,
              lstar.allSatisfy(\.isFinite), green.allSatisfy(\.isFinite)
        else { return nil }
        self.evMin = evMin
        self.evStep = evStep
        self.lstar = lstar
        self.green = green
    }

    var evMax: Double { evMin + Double(lstar.count - 1) * evStep }

    /// The green channel's output code at `ev`, or nil on a plateau: every
    /// EV there would share one code, so a tick would not sit anywhere.
    func greenBin(atEV ev: Double) -> Int? {
        guard let (index, fraction) = sample(atEV: ev) else {
            return Int(min(max((green.first ?? 0) * 255, 0), 255))
        }
        guard green[index] != green[index + 1] else { return nil }
        let value = green[index] * (1 - fraction) + green[index + 1] * fraction
        return Int(min(max(value, 0), 1) * 255)
    }

    /// Whether the curve can be inverted: a real rise, and no fall beyond
    /// noise (a scanner's toed-in shadows wobble by a few hundredths).
    var isInvertible: Bool {
        guard lstar.count > 1 else { return false }
        let envelope = runningMaximum
        guard envelope[envelope.count - 1] - envelope[0] >= 10 else { return false }
        return zip(lstar, lstar.dropFirst()).allSatisfy { $1 - $0 >= -0.5 }
    }

    /// The range of the running-maximum envelope. A pick within half an L\*
    /// of either end has no single scene value.
    var range: ClosedRange<Double> {
        let envelope = runningMaximum
        return envelope[0]...envelope[envelope.count - 1]
    }

    /// The scene EV at which the curve reaches `target`, or nil outside
    /// `range`. The search runs on the running-maximum envelope and
    /// interpolates linearly between samples, so a flat toe inverts to its
    /// first crossing.
    func ev(forLStar target: Double) -> Double? {
        guard isInvertible else { return nil }
        let envelope = runningMaximum
        guard target >= envelope[0], target <= envelope[envelope.count - 1] else { return nil }
        var low = 0, high = envelope.count - 1
        while low < high {
            let middle = (low + high) / 2
            if envelope[middle] < target { low = middle + 1 } else { high = middle }
        }
        let index = max(low, 1)
        let (lower, upper) = (envelope[index - 1], envelope[index])
        let fraction = upper > lower ? (target - lower) / (upper - lower) : 0
        return evMin + (Double(index - 1) + fraction) * evStep
    }

    /// The curve's L\* at `ev`, clamped to the measured range.
    func lstar(atEV ev: Double) -> Double {
        guard let (index, fraction) = sample(atEV: ev) else { return lstar[0] }
        return lstar[index] * (1 - fraction) + lstar[index + 1] * fraction
    }

    /// The sample index and fraction between it and the next under `ev`,
    /// clamped to the curve. Nil for a one-sample curve.
    private func sample(atEV ev: Double) -> (Int, Double)? {
        guard lstar.count > 1 else { return nil }
        let x = min(max((ev - evMin) / evStep, 0), Double(lstar.count - 1))
        let index = min(Int(x.rounded(.down)), lstar.count - 2)
        return (index, x - Double(index))
    }

    private var runningMaximum: [Double] {
        var result = lstar
        for index in 1..<result.count { result[index] = max(result[index], result[index - 1]) }
        return result
    }
}

/// How far `camera.placement_ev` must move so the picked tone lands on each
/// zone, or why it can't be known.
struct ZoneReadout: Equatable, Sendable {
    enum State: Equatable {
        /// The print runs backwards, as a direct scan of the negative does.
        case unavailable
        /// The pick is at (or beyond) an end of what the print can reach.
        case clipped
        case ready
    }

    let state: State
    /// Eight entries, one per zone: the EV to the start of that zone, or nil
    /// where the print can't reach it (Zone 0 on paper).
    let distances: [Double?]

    private let curve: ToneCurve?
    private let pickEV: Double?

    init(curve: ToneCurve, pickLStar: Double) {
        let unreachable = Array(repeating: Double?.none, count: ZoneLadder.names.count)
        guard curve.isInvertible else {
            state = .unavailable
            distances = unreachable
            pickEV = nil
            self.curve = nil
            return
        }
        let range = curve.range
        guard pickLStar >= range.lowerBound + 0.5, pickLStar <= range.upperBound - 0.5,
              let picked = curve.ev(forLStar: pickLStar)
        else {
            state = .clipped
            distances = unreachable
            pickEV = nil
            self.curve = nil
            return
        }
        state = .ready
        pickEV = picked
        self.curve = curve
        // Zone VII's start is the eighth boundary; the ninth is paper white,
        // which its centre uses instead.
        distances = ZoneLadder.lstar.dropLast().map { boundary in
            curve.ev(forLStar: boundary).map { $0 - picked }
        }
    }

    /// The shift for one zone: to its start, or with `centre` to the midpoint
    /// of this zone's distance and the next's. Zone VII has no next zone, so
    /// its centre is the midpoint to L\* `min(99, the curve's top)`.
    func delta(zone: Int, centre: Bool) -> Double? {
        guard state == .ready, let curve, let pickEV,
              zone >= 0, zone < distances.count
        else { return nil }
        if centre {
            if zone == distances.count - 1 {
                guard let start = distances[zone],
                      let top = curve.ev(forLStar: min(99, curve.range.upperBound))
                else { return nil }
                return (start + (top - pickEV)) / 2
            }
            guard let start = distances[zone], let next = distances[zone + 1] else { return nil }
            return (start + next) / 2
        }
        return curve.ev(forLStar: ZoneLadder.lstar[zone]).map { $0 - pickEV }
    }
}
