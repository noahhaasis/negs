import XCTest
@testable import SpektrafilmApp

final class BorderTests: XCTestCase {
    /// The same sizes as the exporter's tests in `spektrafilm-io::border`.
    func testGeometryMatchesExporter() {
        XCTAssertEqual(BorderStyle.white.inset(for: CGSize(width: 200, height: 100)), 6)
        XCTAssertEqual(BorderStyle.keyline.inset(for: CGSize(width: 1000, height: 500)), 34)
        XCTAssertEqual(BorderStyle.keyline(for: CGSize(width: 1000, height: 500)), 4)
        XCTAssertEqual(BorderStyle.film.inset(for: CGSize(width: 6000, height: 4000)), 12)
        XCTAssertEqual(BorderStyle.film(for: CGSize(width: 6000, height: 4000)).pad, 100)
    }

    func testBorderEditsDoNotChangeTheRenderedState() {
        var state = EditState()
        let rendered = state.rendered
        state.border = BorderSettings(enabled: true, style: .film, export: true)
        XCTAssertEqual(state.rendered, rendered)
        XCTAssertEqual(state.border.exported, .film)
        state.border.export = false
        XCTAssertNil(state.border.exported)
    }
}
