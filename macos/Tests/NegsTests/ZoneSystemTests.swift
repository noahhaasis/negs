import AppKit
import SwiftUI
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
            if (1...9).contains(index) { XCTAssertEqual(zone.target, Double(index) * 10) }
        }
        // The end zones land in the middle of their bands, like the rest.
        XCTAssertEqual(zones[0].target, 2.5)
        XCTAssertEqual(zones[10].target, 97.5)
        XCTAssertEqual(ZoneScale.zone(forTarget: zones[0].target), 0)
        XCTAssertEqual(ZoneScale.zone(forTarget: zones[10].target), 10)
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

    /// The column is one scale, with no lines between its tiles, and a line down
    /// its right side that shows against a white mat as well as the dark tiles.
    @MainActor func testTheZoneColumnHasARightEdgeAndNoRowLines() throws {
        _ = NSApplication.shared
        let model = EditorModel()
        defer { model.shutdown() }
        let height = 550
        let host = NSHostingView(rootView: ZonePanel(model: model).frame(height: CGFloat(height)).background(Color.white))
        host.appearance = NSAppearance(named: .darkAqua)
        host.frame = NSRect(x: 0, y: 0, width: ZoneGeometry.panelWidth, height: CGFloat(height))
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let scale = bitmap.pixelsHigh / height
        func grey(_ x: Int, _ y: Int) throws -> CGFloat {
            try XCTUnwrap(bitmap.colorAt(x: x, y: y * scale)?.usingColorSpace(.sRGB)).redComponent * 255
        }
        // Down a column clear of the numerals, the tone only ever falls: a row
        // line would show as a dip darker than the tile below it.
        var previous: CGFloat = 256
        for y in 0..<height {
            let value = try grey(4 * scale, y)
            XCTAssertLessThanOrEqual(value, previous + 0.5, "A line at row \(y)")
            previous = value
        }
        // Translucent grey: only slightly darker than the white tile under it.
        let right = bitmap.pixelsWide - 1
        let edge = try grey(right, 25)
        XCTAssertLessThan(edge, try grey(right - 3 * scale, 25) - 20, "It shows on white")
        XCTAssertGreaterThan(edge, 190, "It's very slight")
    }

    /// The same line over a dark tile comes out lighter than the tile.
    @MainActor func testTheZoneEdgeIsLighterThanADarkTone() throws {
        let host = NSHostingView(rootView: Color.black.overlay { Rectangle().fill(ZonePanel.edgeColour) })
        host.frame = NSRect(x: 0, y: 0, width: 20, height: 20)
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let colour = try XCTUnwrap(bitmap.colorAt(x: bitmap.pixelsWide / 2, y: bitmap.pixelsHigh / 2)?.usingColorSpace(.sRGB))
        XCTAssertGreaterThan(colour.redComponent * 255, 20)
    }

    /// Tab shows and hides the pins, and stands down while typing in a field.
    @MainActor func testTabTogglesThePinsOutsideTextFields() {
        let tab: UInt16 = 48
        XCTAssertTrue(EditorWindowController.togglesPins(keyCode: tab, modifiers: [], firstResponder: NSView()))
        XCTAssertTrue(EditorWindowController.togglesPins(keyCode: tab, modifiers: [], firstResponder: nil))
        XCTAssertTrue(EditorWindowController.togglesPins(keyCode: tab, modifiers: [.capsLock], firstResponder: nil), "Caps Lock doesn't matter")
        XCTAssertFalse(EditorWindowController.togglesPins(keyCode: tab, modifiers: [], firstResponder: NSTextView()))
        XCTAssertFalse(EditorWindowController.togglesPins(keyCode: tab, modifiers: [.shift], firstResponder: nil), "Shift-Tab keeps its meaning")
        XCTAssertFalse(EditorWindowController.togglesPins(keyCode: tab, modifiers: [.control], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesPins(keyCode: 6, modifiers: [], firstResponder: nil), "Z no longer does")
    }

    /// Delete has no modifier, so it stands down while typing in a field.
    @MainActor func testBareKeysStandDownWhileEditingText() {
        XCTAssertFalse(MainMenu.bareKeysAllowed(firstResponder: NSTextView()))
        XCTAssertTrue(MainMenu.bareKeysAllowed(firstResponder: NSView()))
        XCTAssertTrue(MainMenu.bareKeysAllowed(firstResponder: nil))
    }

    @MainActor func testBareFShowsOnlyThePhotograph() {
        XCTAssertTrue(EditorWindowController.togglesImageOnly(keyCode: 3, modifiers: [], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesImageOnly(keyCode: 3, modifiers: [.command, .option], firstResponder: nil), "Hide Film Strip")
        XCTAssertFalse(EditorWindowController.togglesImageOnly(keyCode: 3, modifiers: [], firstResponder: NSText()))
        XCTAssertFalse(EditorWindowController.togglesImageOnly(keyCode: 5, modifiers: [], firstResponder: nil))
    }

    func testGreysRiseFromBlackToPaperWhite() throws {
        let levels = try ZoneScale.zones.map { zone -> CGFloat in
            try XCTUnwrap(ZoneScale.grey(zone.index).usingColorSpace(.sRGB)).redComponent
        }
        XCTAssertEqual(try XCTUnwrap(levels.first), 0.03, accuracy: 0.02)
        XCTAssertEqual(try XCTUnwrap(levels.last), 0.97, accuracy: 0.02)
        XCTAssertEqual(levels, levels.sorted())
        XCTAssertEqual(Set(levels).count, 11)
    }
}
