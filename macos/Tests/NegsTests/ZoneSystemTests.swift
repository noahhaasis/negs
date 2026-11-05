import AppKit
import XCTest
@testable import NegsApp

/// The zones: where they sit on the L\* axis and the tones they print as.
final class ZoneSystemTests: XCTestCase {
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

    func testTileLabelsNameTheZoneAndWhetherItIsPlaced() {
        XCTAssertEqual(ZonePanel.label(for: ZoneScale.zones[5], placed: true), "Zone V, Middle grey, placed")
        XCTAssertEqual(ZonePanel.label(for: ZoneScale.zones[0], placed: false), "Zone 0, Pure black, not placed")
    }

    func testGreysRiseFromBlackToPaperWhite() throws {
        let levels = try ZoneScale.zones.map { zone -> CGFloat in
            try XCTUnwrap(ZoneScale.grey(zone.index).usingColorSpace(.sRGB)).redComponent
        }
        XCTAssertEqual(try XCTUnwrap(levels.first), 0, accuracy: 1e-9)
        XCTAssertEqual(try XCTUnwrap(levels.last), 1, accuracy: 1e-9)
        XCTAssertEqual(levels, levels.sorted())
        XCTAssertEqual(Set(levels).count, 9)
    }
}
