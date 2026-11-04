import XCTest
@testable import NegsApp

final class ZoneSystemTests: XCTestCase {
    /// A synthetic curve on the helper's sample grid.
    private func curve(
        _ lstar: (Double) -> Double,
        green: (Double) -> Double = { min(max($0 / 16 + 0.6, 0), 1) }
    ) -> ToneCurve {
        let count = 385
        let samples = (0..<count).map { -16.0 + Double($0) * 0.0625 }
        return ToneCurve(
            evMin: -16, evStep: 0.0625,
            lstar: samples.map(lstar), green: samples.map(green)
        )!
    }

    private func lstarForY(_ y: Double) -> Double {
        y > 216.0 / 24389.0 ? 116 * cbrt(y) - 16 : y * 24389.0 / 27.0
    }

    /// The ideal curve: output L\* is exactly the CIE lightness of the scene.
    private func idealCurve() -> ToneCurve {
        curve { lstarForY(0.18 * pow(2, $0)) }
    }

    func testLadderMatchesTheSpecifiedBoundaries() {
        let expected = [0.0, 7.19, 13.18, 20.76, 30.31, 42.35, 57.52, 76.63, 100]
        XCTAssertEqual(ZoneLadder.lstar.count, expected.count)
        for (actual, want) in zip(ZoneLadder.lstar, expected) {
            XCTAssertEqual(actual, want, accuracy: 0.005, "\(actual) vs \(want)")
        }
        XCTAssertEqual(ZoneLadder.names, ["0", "I", "II", "III", "IV", "V", "VI", "VII"])
    }

    func testLStarShapedCurveGivesOneStopSpacing() {
        let readout = ZoneReadout(curve: idealCurve(), pickLStar: 49)
        XCTAssertEqual(readout.state, .ready)
        // Zone I…VII are one stop apart by construction.
        for zone in 1..<7 {
            guard let here = readout.distances[zone], let next = readout.distances[zone + 1] else {
                return XCTFail("zone \(zone) unreachable")
            }
            XCTAssertEqual(next - here, 1, accuracy: 0.02, "zone \(zone)")
        }
    }

    func testGammaCurveGivesTheBriefsSpacing() {
        // An output tone that behaves like a gamma-2.2 display: L* rises as
        // the scene's 1/2.2 power, so the zones crowd toward the highlights.
        let readout = ZoneReadout(curve: curve { min(100, 100 * pow(0.18 * pow(2, $0), 1 / 2.2)) }, pickLStar: 20)
        XCTAssertEqual(readout.state, .ready)
        let spacing = { (zone: Int) in readout.distances[zone + 1]! - readout.distances[zone]! }
        XCTAssertEqual(spacing(1), 1.92, accuracy: 0.05)
        XCTAssertEqual(spacing(6), 0.91, accuracy: 0.05)
    }

    func testZoneZeroIsUnreachableOnPaper() {
        // This stock's floor is L* 3.6, above Zone 0's start at L* 0.
        let readout = ZoneReadout(
            curve: curve { 3.6 + 20 * ($0 + 16) / 24 },
            pickLStar: 20
        )
        XCTAssertEqual(readout.state, .ready)
        XCTAssertNil(readout.distances[0])
        XCTAssertNil(readout.delta(zone: 0, centre: false))
        XCTAssertNil(readout.delta(zone: 0, centre: true))
        XCTAssertNotNil(readout.distances[1])
    }

    func testFallingCurveIsUnavailable() {
        // A direct scan of a negative: L* falls as the scene brightens.
        let measured = [44.7, 38.7, 33.8, 29.0, 24.4]
        let readout = ZoneReadout(
            curve: curve { measured[min(max(Int(($0 + 16) / 24 * 5), 0), 4)] },
            pickLStar: 34
        )
        XCTAssertEqual(readout.state, .unavailable)
        XCTAssertTrue(readout.distances.allSatisfy { $0 == nil })
        XCTAssertNil(readout.delta(zone: 4, centre: false))
    }

    func testPickAboveOrBelowTheRangeIsClipped() {
        let curve = curve { 3.6 + 20 * ($0 + 16) / 24 }
        for pick in [curve.range.lowerBound, curve.range.upperBound, 0.0] {
            let readout = ZoneReadout(curve: curve, pickLStar: pick)
            XCTAssertEqual(readout.state, .clipped, "pick \(pick)")
            XCTAssertTrue(readout.distances.allSatisfy { $0 == nil })
            XCTAssertNil(readout.delta(zone: 5, centre: true))
        }
    }

    func testPlateauInvertsToItsFirstCrossing() {
        // A flat toe with sub-half-L* noise, as a scanner's shadows give.
        let noise = [0.0, 0.1, -0.05, 0.2, -0.1, 0.05, 0.0]
        let curve = curve { ev in
            let rise = max(0, ev + 10)
            return 3.6 + rise + (ev < -10 ? noise[min(max(Int((ev + 16) * 3), 0), 6)] : 0)
        }
        XCTAssertTrue(curve.isInvertible)
        XCTAssertEqual(curve.ev(forLStar: 3.6) ?? .nan, -16, accuracy: 0.08)
    }

    func testCentreIsTheMidpointToTheNextZone() {
        let readout = ZoneReadout(curve: idealCurve(), pickLStar: 49)
        let centre = readout.delta(zone: 5, centre: true)
        XCTAssertEqual(centre ?? .nan, (readout.distances[5]! + readout.distances[6]!) / 2, accuracy: 1e-9)
    }

    func testZoneSevenCentreIsRelativeToThePick() {
        let curve = idealCurve()
        let first = ZoneReadout(curve: curve, pickLStar: curve.lstar(atEV: 0))
        let second = ZoneReadout(curve: curve, pickLStar: curve.lstar(atEV: 1))
        XCTAssertEqual(
            second.delta(zone: 7, centre: true)!,
            first.delta(zone: 7, centre: true)! - 1,
            accuracy: 0.02
        )
    }

    func testZoneSevenCentreUsesTheCurveTopBelowL99() {
        // The print's white stops at L* 92, below the nominal 99.
        let curve = curve { 3.6 + 88.4 * min(max(($0 + 16) / 24, 0), 1) }
        let readout = ZoneReadout(curve: curve, pickLStar: 40)
        XCTAssertEqual(readout.state, .ready)
        let top = curve.ev(forLStar: curve.range.upperBound)!
        let pick = curve.ev(forLStar: 40)!
        XCTAssertEqual(
            readout.delta(zone: 7, centre: true)!,
            (readout.distances[7]! + (top - pick)) / 2,
            accuracy: 1e-9
        )
    }

    func testPickOnAZoneStartReadsZero() {
        let curve = idealCurve()
        for zone in 1...7 {
            let readout = ZoneReadout(curve: curve, pickLStar: ZoneLadder.lstar[zone])
            XCTAssertEqual(readout.state, .ready)
            XCTAssertEqual(readout.delta(zone: zone, centre: false)!, 0, accuracy: 1e-9, "zone \(zone)")
        }
    }

    func testGreenBinFollowsTheCurveAndSkipsPlateaus() {
        // A curve that reaches green 1.0 at -1 EV and stays there.
        let curve = curve({ 20 + $0 }, green: { $0 >= -1 ? 1 : 0.2 + 0.8 * ($0 + 16) / 15 })
        XCTAssertEqual(curve.greenBin(atEV: -8), Int((0.2 + 0.8 * 8 / 15) * 255))
        XCTAssertNil(curve.greenBin(atEV: 2), "a clipped top has no position")
        XCTAssertNil(curve.greenBin(atEV: 0), "inside the plateau")
    }
}
