import XCTest
@testable import NegsApp

/// The chart's model: the zone ladder on the L\* axis and the share of the
/// photograph's tones inside each band.
final class ZoneHistogramTests: XCTestCase {
    func testTheZonesTileTheAxisFromTheLadder() {
        let zones = ZoneScale.zones
        XCTAssertEqual(zones.count, 9)
        XCTAssertEqual(zones.first?.lStarRange.lowerBound, 0)
        XCTAssertEqual(zones.last?.lStarRange.upperBound, 100)
        for (index, zone) in zones.enumerated() {
            XCTAssertEqual(zone.index, index)
            XCTAssertEqual(zone.numeral, ZoneLadder.names[index])
            XCTAssertEqual(zone.lStarRange.lowerBound, ZoneLadder.lstar[index], accuracy: 1e-12)
            if index > 0 {
                // No gaps, no overlaps.
                XCTAssertEqual(zone.lStarRange.lowerBound, zones[index - 1].lStarRange.upperBound, accuracy: 1e-12)
            }
        }
        XCTAssertEqual(zones[5].name, "Middle grey")
        XCTAssertEqual(zones[5].lStarRange.lowerBound, 42.35, accuracy: 0.01)
        XCTAssertEqual(zones[5].centre, (42.35 + 57.52) / 2, accuracy: 0.01)
        // Zone VIII is paper white, not a stop above VII.
        XCTAssertEqual(zones[8].numeral, "VIII")
        XCTAssertEqual(zones[8].name, "Paper white")
        XCTAssertEqual(zones[8].lStarRange.lowerBound, 91.91, accuracy: 0.05)
        XCTAssertEqual(zones[8].lStarRange.upperBound, 100)
        XCTAssertEqual(zones[7].lStarRange.upperBound, zones[8].lStarRange.lowerBound, accuracy: 1e-12)
    }

    /// The extreme bands are the black and white points; the rest pin to
    /// their middles.
    func testZoneTargetsSetTheBlackAndWhitePoints() {
        let zones = ZoneScale.zones
        XCTAssertEqual(zones[0].target, 0)
        XCTAssertEqual(zones[8].target, 100)
        XCTAssertEqual(zones[5].target, zones[5].centre)
        XCTAssertEqual(zones[4].target, zones[4].centre)
        XCTAssertEqual(ZoneScale.zone(forTarget: 0), 0)
        XCTAssertEqual(ZoneScale.zone(forTarget: 50), 5)
        XCTAssertEqual(ZoneScale.zone(forTarget: 100), 8)
    }

    func testShareIsTheFractionInsideTheBand() {
        // The chart's 256 bins are evenly spaced in L*.
        var histogram = ToneHistogram.empty
        histogram.luminance[128] = 10
        let zone = ZoneScale.zones[5]
        XCTAssertEqual(histogram.share(of: zone), 1, accuracy: 1e-12)
        XCTAssertEqual(histogram.share(of: ZoneScale.zones[0]), 0, accuracy: 1e-12)
        // A uniform histogram gives each zone its width's share.
        histogram.luminance = Array(repeating: 1, count: 256)
        let width = zone.lStarRange.upperBound - zone.lStarRange.lowerBound
        XCTAssertEqual(histogram.share(of: zone), width / 100, accuracy: 0.02)
    }

    func testTheLastZoneIncludesPaperWhite() {
        var histogram = ToneHistogram.empty
        histogram.luminance[255] = 1
        XCTAssertEqual(histogram.share(of: ZoneScale.zones[8]), 1, accuracy: 1e-12)
        XCTAssertEqual(histogram.share(of: ZoneScale.zones[7]), 0, accuracy: 1e-12)
    }

    /// A clipped spike can't flatten the chart: it runs off the top.
    func testClippedEndsDoNotSetTheScale() {
        var histogram = Histogram()
        histogram.red[255] = 90_000; histogram.blue[0] = 70_000; histogram.green[128] = 40
        XCTAssertEqual(ToneHistogram(histogram).peak, 40)
    }

    /// A click only promises what it does: without a selected pin it reads.
    func testOnlyASelectedPinOffersAClick() {
        XCTAssertEqual(ZoneHistogramView.clickHint(pinSelected: true), "Click to move here")
        XCTAssertNil(ZoneHistogramView.clickHint(pinSelected: false))
    }

    func testEmptyHistogramHasNoShare() {
        let empty = ToneHistogram.empty
        XCTAssertEqual(empty.share(of: ZoneScale.zones[5]), 0)
        XCTAssertEqual(empty.red.count, 256)
    }

    func testTheChartsArraysComeFromTheCountedTones() {
        var counted = Histogram()
        counted.lightness[100] = 7
        counted.red[200] = 3
        let tones = ToneHistogram(counted)
        XCTAssertEqual(tones.luminance[100], 7)
        XCTAssertEqual(tones.red[200], 3)
        XCTAssertEqual(tones.green.reduce(0, +), 0)
    }
}
