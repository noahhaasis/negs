import XCTest
@testable import NegsApp

final class LightTableTests: XCTestCase {
    private let sheet = Theme.Hex.bg

    private func border(_ style: BorderStyle, color: BorderColor = .white, instagram: Bool = false) -> BorderSettings {
        BorderSettings(enabled: true, style: style, instagram: instagram, color: color)
    }

    func testFilmBordersOnTheDarkSheetGetALight() throws {
        for style in [BorderStyle.film, .film35] {
            let light = try XCTUnwrap(LightTable.light(sheet: sheet, edge: LightTable.edge(of: border(style))), "\(style)")
            XCTAssertGreaterThan(Theme.luminance(light), Theme.luminance(sheet), "a light, not a shade, under a black edge")
        }
    }

    /// On `bg` the light is about `blank`, as in the r24 mockup.
    func testLightOnTheDarkSheetIsAboutBlank() throws {
        let light = try XCTUnwrap(LightTable.light(sheet: sheet, edge: 0x000000))
        XCTAssertLessThan(Theme.contrast(light, Theme.Hex.blank), 1.05)
    }

    func testMatsFollowTheirColour() {
        XCTAssertNil(LightTable.light(sheet: sheet, edge: LightTable.edge(of: border(.solid))), "white mat")
        XCTAssertNil(LightTable.light(sheet: sheet, edge: LightTable.edge(of: border(.keyline, color: BorderColor(red: 0x33, green: 0x33, blue: 0x33)))))
        XCTAssertNotNil(LightTable.light(sheet: sheet, edge: LightTable.edge(of: border(.solid, color: .black))))
        XCTAssertNotNil(LightTable.light(sheet: sheet, edge: LightTable.edge(of: border(.keyline, color: BorderColor(red: 0x22, green: 0x22, blue: 0x22)))))
    }

    /// Instagram's extension puts the film borders on white, which stands out.
    func testInstagramFilmBorderNeedsNoLight() {
        XCTAssertEqual(LightTable.edge(of: border(.film, instagram: true)), 0xffffff)
        XCTAssertNil(LightTable.light(sheet: sheet, edge: LightTable.edge(of: border(.film, instagram: true))))
    }

    func testNoBorderNoLight() {
        XCTAssertNil(LightTable.edge(of: nil))
        XCTAssertNil(LightTable.light(sheet: sheet, edge: nil))
    }

    /// For when the sheet's colour can be chosen again.
    func testLightSheetsShadeALightEdgeAndLeaveADarkOne() throws {
        let paperSheet: UInt32 = 0xece5d8
        let shade = try XCTUnwrap(LightTable.light(sheet: paperSheet, edge: 0xffffff))
        XCTAssertLessThan(Theme.luminance(shade), Theme.luminance(paperSheet))
        XCTAssertNil(LightTable.light(sheet: paperSheet, edge: 0x000000))
        XCTAssertNil(LightTable.light(sheet: 0x808080, edge: 0x000000), "black on mid grey reads")
    }

    func testMix() {
        XCTAssertEqual(LightTable.mix(0x000000, 0xffffff, 0), 0x000000)
        XCTAssertEqual(LightTable.mix(0x000000, 0xffffff, 1), 0xffffff)
        XCTAssertEqual(LightTable.mix(0x204060, 0x204060, 0.5), 0x204060)
        XCTAssertEqual(BorderColor(red: 0x20, green: 0x40, blue: 0x60).hex24, 0x204060)
    }
}
