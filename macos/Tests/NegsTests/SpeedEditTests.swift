import XCTest
@testable import NegsApp

/// Speed edit: the keys, their steps, and the sliders they move.
final class SpeedEditTests: XCTestCase {
    func testKeys() {
        let keys: [String: SpeedEditTarget] = ["q": .exposure, "w": .contrast, "e": .saturation, "a": .highlights,
                                               "s": .shadows, "d": .gain, "f": .lift, "z": .temperature, "x": .tint]
        for (key, target) in keys { XCTAssertEqual(SpeedEditTarget.key(key), target, key) }
        XCTAssertNil(SpeedEditTarget.key("t"))
        XCTAssertNil(SpeedEditTarget.key("1"))
    }

    /// Every Prep target moves a slider the inspector has, at its step.
    func testPrepTargetsUseTheInspectorsSliders() {
        for target in SpeedEditTarget.allCases where target != .exposure && target != .temperature {
            guard let slider = target.slider else { return XCTFail("\(target) has no slider") }
            let fine = target.amount(fine: true, coarse: false)
            // The finest notch is at least one step, so it always moves
            // (temperature moves in mireds; see testTemperatureMovesInMireds).
            XCTAssertGreaterThanOrEqual(fine, slider.step, "\(target)")
        }
        XCTAssertEqual(SpeedEditTarget.highlights.slider?.range, -180...100)
        XCTAssertEqual(SpeedEditTarget.contrast.slider?.step, 1)
    }

    func testFineNormalCoarse() {
        let t = SpeedEditTarget.contrast
        XCTAssertLessThan(t.amount(fine: true, coarse: false), t.amount(fine: false, coarse: false))
        XCTAssertLessThan(t.amount(fine: false, coarse: false), t.amount(fine: false, coarse: true))
        XCTAssertEqual(SpeedEditTarget.exposure.amount(fine: false, coarse: false), 0.1)
    }

    /// Up is warmer: a higher temperature, in even mired steps.
    func testTemperatureMovesInMireds() {
        let t = SpeedEditTarget.temperature
        let warmer = t.moved(5000, steps: 1, amount: 3)
        XCTAssertGreaterThan(warmer, 5000)
        XCTAssertEqual(1e6 / 5000 - 1e6 / warmer, 3, accuracy: 1e-9)
        XCTAssertLessThan(t.moved(5000, steps: -1, amount: 3), 5000)
        // A notch is bigger in kelvin when warm-balanced, the same in mireds.
        XCTAssertGreaterThan(t.moved(9000, steps: 1, amount: 3) - 9000, warmer - 5000)
    }

    func testReadout() {
        XCTAssertEqual(SpeedEditTarget.exposure.formatted(0.35), "+0.35 EV")
        XCTAssertEqual(SpeedEditTarget.contrast.formatted(12), "+12")
        XCTAssertEqual(SpeedEditTarget.contrast.formatted(-4), "-4")
        XCTAssertEqual(SpeedEditTarget.contrast.formatted(0), "0")
        XCTAssertEqual(SpeedEditTarget.temperature.formatted(5212.4), "5212 K")
        XCTAssertEqual(SpeedEditTarget.tint.formatted(1.04), "1.04")
    }
}
