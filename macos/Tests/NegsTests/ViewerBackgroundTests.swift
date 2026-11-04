import SwiftUI
import XCTest
@testable import NegsApp

final class ViewerBackgroundTests: XCTestCase {
    func testHexParsing() throws {
        let colour = try XCTUnwrap(ViewerBackground.components(hex: "#336699"))
        XCTAssertEqual(colour.red, 0.2, accuracy: 1e-9)
        XCTAssertEqual(colour.green, 0.4, accuracy: 1e-9)
        XCTAssertEqual(colour.blue, 0.6, accuracy: 1e-9)
        XCTAssertNotNil(ViewerBackground.components(hex: "336699"))
        XCTAssertNotNil(ViewerBackground.components(hex: "  #336699\n"))
        XCTAssertNil(ViewerBackground.components(hex: "#369"))
        XCTAssertNil(ViewerBackground.components(hex: "hello!"))
    }

    func testHexWriteClampsAndFormats() {
        XCTAssertEqual(ViewerBackground.hex(red: 0.2, green: 0.4, blue: 0.6), "#336699")
        XCTAssertEqual(ViewerBackground.hex(red: -1, green: 2, blue: 0.5), "#00FF80")
    }

    func testColourRoundTripsThroughHex() {
        let colour = Color(.sRGB, red: 0.2, green: 0.4, blue: 0.6, opacity: 1)
        let hex = ViewerBackground.hex(colour)
        XCTAssertEqual(hex, "#336699")
        XCTAssertNotNil(ViewerBackground.color(hex: hex))
    }
}
