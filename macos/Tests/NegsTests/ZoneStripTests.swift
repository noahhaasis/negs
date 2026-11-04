import AppKit
import XCTest
@testable import NegsApp

final class ZoneStripTests: XCTestCase {
    private let width: CGFloat = 272

    /// A rising, unclipped curve on the histogram's grid: green climbs evenly
    /// with the scene, and L\* spans the print's floor to its white.
    private func risingCurve() -> ToneCurve {
        ToneCurve(
            evMin: -16, evStep: 0.0625,
            lstar: (0..<385).map { 3.6 + 88.4 * Double($0) / 384 },
            green: (0..<385).map { 0.05 + 0.9 * Double($0) / 384 }
        )!
    }

    func testBandsAlternateFromZoneI() {
        // I, III, V and VII are lit; 0, II, IV and VI are dark.
        XCTAssertEqual((0..<8).map(ZoneGeometry.bandLit), [false, true, false, true, false, true, false, true])
    }

    func testLabelFormatting() {
        XCTAssertEqual(ZoneLabels.distance(1.24), "+1.2")
        XCTAssertEqual(ZoneLabels.distance(-0.44), "−0.4")
        XCTAssertEqual(ZoneLabels.distance(0), "+0.0")
        XCTAssertEqual(ZoneLabels.distance(nil), "—")
        let sample = ToneSample(red: 128, green: 120, blue: 110, lstar: 49.24)
        let curve = risingCurve()
        let ready = ZoneReadout(curve: curve, pickLStar: 40)
        XCTAssertEqual(ready.state, .ready)
        XCTAssertEqual(ZoneLabels.headline(ready, sample: sample), "L* 49.2 · 128 120 110")
        XCTAssertEqual(ZoneLabels.headline(nil, sample: nil), "⌃-click a tone to read its zones")
        // A clipped pick and a backwards print each explain themselves.
        let clipped = ZoneReadout(curve: curve, pickLStar: curve.range.upperBound)
        XCTAssertEqual(clipped.state, .clipped)
        XCTAssertTrue(ZoneLabels.headline(clipped, sample: sample).hasPrefix("Clipped"))
        let falling = ToneCurve(
            evMin: -16, evStep: 0.0625,
            lstar: (0..<385).map { 44.7 - 20 * Double($0) / 384 },
            green: (0..<385).map { 0.9 - 0.8 * Double($0) / 384 }
        )!
        let unavailable = ZoneReadout(curve: falling, pickLStar: 34)
        XCTAssertEqual(unavailable.state, .unavailable)
        XCTAssertTrue(ZoneLabels.headline(unavailable, sample: sample).contains("backwards"))
    }

    func testEvTicksAreLongEveryStop() {
        let curve = risingCurve()
        let ticks = ZoneGeometry.evTicks(curve: curve, width: width)
        XCTAssertEqual(ticks.count, 49, "n 0…48, 1/3 EV apart")
        for (n, tick) in ticks.enumerated() {
            XCTAssertEqual(tick.ev, -Double(n) / 3, accuracy: 1e-12)
            XCTAssertEqual(tick.long, n % 3 == 0, "tick \(n)")
            XCTAssertEqual(tick.midtone, n == 9)
        }
        // A plateau leaves no tick: every stop there would pile on a code.
        let plateau = ToneCurve(
            evMin: -16, evStep: 0.0625,
            lstar: (0..<385).map { 3.6 + 20 * Double($0) / 384 },
            green: (0..<385).map { $0 < 200 ? 0.5 : 0.05 + 0.9 * Double($0) / 384 }
        )!
        XCTAssertLessThan(ZoneGeometry.evTicks(curve: plateau, width: width).count, 49)
    }

    func testMidToneMarkIsAtTwelveAndAHalfPercent() {
        let curve = risingCurve()
        let marks = ZoneGeometry.evTicks(curve: curve, width: width).filter(\.midtone)
        XCTAssertEqual(marks.count, 1)
        XCTAssertEqual(marks[0].ev, -3, accuracy: 1e-12, "12.5 % is 2^-3")
        XCTAssertTrue(marks[0].long, "it falls on a stop")
        XCTAssertEqual(marks[0].x, ZoneGeometry.x(greenBin: curve.greenBin(atEV: -3)!, width: width))
    }

    func testEvScaleMovesWithConversionParameters() {
        let base = risingCurve()
        let brighter = ToneCurve(
            evMin: base.evMin, evStep: base.evStep,
            lstar: base.lstar, green: base.green.map { min($0 * 1.3, 1) }
        )!
        let a = ZoneGeometry.evTicks(curve: base, width: width)
        let b = ZoneGeometry.evTicks(curve: brighter, width: width)
        XCTAssertEqual(a.count, b.count)
        XCTAssertEqual(a[20].ev, b[20].ev)
        XCTAssertGreaterThan(b[20].x, a[20].x, "a brighter curve puts the same stop higher")
    }

    func testLStripeStepsEveryTenWithValuesAbove() {
        let steps = ZoneGeometry.lStripe(width: width)
        XCTAssertEqual(steps.map(\.value), Array(stride(from: 0, through: 100, by: 10)))
        for step in steps {
            XCTAssertEqual(step.x, ZoneGeometry.x(lstar: Double(step.value), width: width), accuracy: 1e-9)
        }
    }

    func testChannelTicksGateBarsAndPickerLines() {
        XCTAssertEqual(HistogramView.drawnChannels(.all), [.red, .green, .blue])
        XCTAssertEqual(HistogramView.drawnPickers(.all), [.red, .green, .blue, .lightness])
        let noLightness = HistogramChannels.all.subtracting(.lightness)
        XCTAssertEqual(HistogramView.drawnChannels(noLightness), [.red, .green, .blue])
        XCTAssertEqual(HistogramView.drawnPickers(noLightness), [.red, .green, .blue])
        XCTAssertEqual(HistogramView.drawnChannels(.red), [.red])
        XCTAssertEqual(HistogramView.drawnPickers(.red), [.red])
    }

    func testNumbersAppearOnlyAfterAPick() {
        XCTAssertFalse(ZoneStrip.showsNumbers(nil))
        let curve = risingCurve()
        XCTAssertTrue(ZoneStrip.showsNumbers(ZoneReadout(curve: curve, pickLStar: 15)))
        XCTAssertFalse(ZoneStrip.showsNumbers(ZoneReadout(curve: curve, pickLStar: curve.range.upperBound)))
    }

    func testNumbersAreAtZoneStarts() {
        for (zone, boundary) in ZoneLadder.lstar.dropLast().enumerated() {
            XCTAssertEqual(
                ZoneGeometry.x(lstar: boundary, width: width),
                width * CGFloat(boundary * 2.55 / 255),
                accuracy: 1e-9,
                "zone \(zone)"
            )
        }
        // Two alternating rows, so the shadow zones' labels don't collide.
        XCTAssertEqual((0..<8).map(ZoneGeometry.numberRow), [0, 1, 0, 1, 0, 1, 0, 1])
    }

    func testUnreachableZoneIsDisabled() {
        XCTAssertTrue(ZoneStrip.isEnabled(canPlace: true, distance: 1.2))
        XCTAssertFalse(ZoneStrip.isEnabled(canPlace: true, distance: nil))
        XCTAssertFalse(ZoneStrip.isEnabled(canPlace: false, distance: 1.2))
        // On paper, Zone 0's start is below the print's floor.
        let readout = ZoneReadout(curve: risingCurve(), pickLStar: 12)
        XCTAssertEqual(readout.state, .ready)
        XCTAssertNil(readout.delta(zone: 0, centre: false))
    }

    func testStripIsTheSameHeightWithAndWithoutAPick() {
        // The strip reserves two rows in its frame whether or not numbers are
        // drawn, so a pick never moves the graph below it.
        XCTAssertEqual(ZoneGeometry.stripHeight, 22)
        XCTAssertEqual(ZoneGeometry.stripHeight, 2 * 11)
        XCTAssertFalse(ZoneStrip.showsNumbers(nil))
    }
}
