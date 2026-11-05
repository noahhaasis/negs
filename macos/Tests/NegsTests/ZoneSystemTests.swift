import AppKit
import XCTest
@testable import NegsApp

/// The zones: where they sit on the L\* axis and the tones they print as.
final class ZoneSystemTests: XCTestCase {
    /// Adams' eleven zones, evenly spaced in lightness as a print shows them.
    func testTheElevenZonesRunFromPureBlackToPureWhite() throws {
        let zones = ZoneScale.zones
        try XCTSkipUnless(zones.count == 11 || { XCTFail("\(zones.count) zones"); return false }())
        for (index, zone) in zones.enumerated() {
            XCTAssertEqual(zone.index, index)
            XCTAssertEqual(zone.numeral, "\(index)")
            XCTAssertEqual(zone.target, Double(index) * 10)
        }
        XCTAssertEqual(zones[0].name, "Pure black")
        XCTAssertEqual(zones[5].name, "Middle gray")
        XCTAssertEqual(zones[10].name, "Pure white")
    }

    /// Zone 5 is 18 % gray, the point meters are calibrated to.
    func testZoneFiveIsEighteenPercentGray() {
        let y = pow((ZoneScale.zones[5].target + 16) / 116, 3)
        XCTAssertEqual(y, 0.18, accuracy: 0.005)
    }

    func testATargetFallsInTheNearestZone() {
        XCTAssertEqual(ZoneScale.zone(forTarget: 0), 0)
        XCTAssertEqual(ZoneScale.zone(forTarget: 4.9), 0)
        XCTAssertEqual(ZoneScale.zone(forTarget: 5.1), 1)
        XCTAssertEqual(ZoneScale.zone(forTarget: 50), 5)
        XCTAssertEqual(ZoneScale.zone(forTarget: 100), 10)
        XCTAssertEqual(ZoneScale.zone(forTarget: 140), 10)
        XCTAssertEqual(ZoneScale.zone(forTarget: -5), 0)
    }

    /// A tile's tooltip names the zone and says what belongs in it.
    func testATilesTooltipDescribesTheZone() {
        let tip = ZonePanel.tooltip(for: ZoneScale.zones[5])
        XCTAssertTrue(tip.hasPrefix("Zone 5: Middle gray\n"), tip)
        XCTAssertTrue(tip.contains("18% gray"), tip)
        XCTAssertTrue(tip.contains("dark blue sky"), tip)
        for zone in ZoneScale.zones {
            XCTAssertFalse(zone.note.isEmpty, zone.numeral)
            XCTAssertTrue(ZonePanel.tooltip(for: zone).contains(zone.note), zone.numeral)
        }
    }

    func testTileLabelsNameTheZoneAndWhetherItIsPlaced() {
        XCTAssertEqual(ZonePanel.label(for: ZoneScale.zones[5], placed: true), "Zone 5, Middle gray, placed")
        XCTAssertEqual(ZonePanel.label(for: ZoneScale.zones[0], placed: false), "Zone 0, Pure black, not placed")
    }

    /// Z and Delete have no modifier, so they stand down while typing in a field.
    @MainActor func testBareKeysStandDownWhileEditingText() {
        XCTAssertFalse(MainMenu.bareKeysAllowed(firstResponder: NSTextView()))
        XCTAssertTrue(MainMenu.bareKeysAllowed(firstResponder: NSView()))
        XCTAssertTrue(MainMenu.bareKeysAllowed(firstResponder: nil))
    }

    func testGreysRiseFromBlackToPaperWhite() throws {
        let levels = try ZoneScale.zones.map { zone -> CGFloat in
            try XCTUnwrap(ZoneScale.grey(zone.index).usingColorSpace(.sRGB)).redComponent
        }
        XCTAssertEqual(try XCTUnwrap(levels.first), 0, accuracy: 1e-9)
        XCTAssertEqual(try XCTUnwrap(levels.last), 1, accuracy: 1e-9)
        XCTAssertEqual(levels, levels.sorted())
        XCTAssertEqual(Set(levels).count, 11)
    }
}
