import XCTest
@testable import NegsApp

final class BorderTests: XCTestCase {
    private let data = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../../../data").standardizedFileURL

    /// The same sizes as the exporter's tests in `spektrafilm-io::border`.
    func testGeometryMatchesExporter() {
        XCTAssertEqual(BorderSettings(style: .solid).canvas(for: CGSize(width: 200, height: 100)), CGSize(width: 212, height: 112))
        XCTAssertEqual(BorderSettings(style: .keyline).canvas(for: CGSize(width: 1000, height: 500)), CGSize(width: 1068, height: 568))
        XCTAssertEqual(BorderSettings(style: .keyline).keyline(for: CGSize(width: 1000, height: 500)), 4)
        XCTAssertEqual(BorderSettings(style: .film).canvas(for: CGSize(width: 6000, height: 4000)), CGSize(width: 6024, height: 4024))
        XCTAssertEqual(BorderStyle.film(for: CGSize(width: 6000, height: 4000)).pad, 100)
    }

    /// The same sizes as `solid_border_takes_a_width_and_colour`,
    /// `keyline_takes_a_line_width_and_colours` and
    /// `film_border_extends_with_black` in `spektrafilm-io::border`.
    func testWidthsMatchExporter() {
        XCTAssertEqual(BorderSettings(style: .solid, width: 0.1).canvas(for: CGSize(width: 200, height: 100)), CGSize(width: 240, height: 140))
        XCTAssertEqual(BorderSettings(style: .keyline, width: 0.01, line: 0.02).canvas(for: CGSize(width: 1000, height: 500)), CGSize(width: 1060, height: 560))
        XCTAssertEqual(BorderSettings(style: .film, filmWidth: 0.05).canvas(for: CGSize(width: 600, height: 400)), CGSize(width: 662, height: 462))
    }

    /// The same sizes as `instagram_extends_the_short_way_to_3_by_4` in `spektrafilm-io::border`.
    func testInstagramCanvasMatchesExporter() {
        XCTAssertEqual(BorderSettings(style: .keyline).instagramCanvas(for: CGSize(width: 1068, height: 568)), CGSize(width: 1068, height: 1424))
        XCTAssertEqual(BorderSettings(style: .solid).instagramCanvas(for: CGSize(width: 1000, height: 2000)), CGSize(width: 1500, height: 2000))
        XCTAssertEqual(BorderSettings(style: .solid).instagramCanvas(for: CGSize(width: 300, height: 400)), CGSize(width: 300, height: 400))
        XCTAssertEqual(BorderSettings(style: .solid).instagramCanvas(for: CGSize(width: 301, height: 400)), CGSize(width: 301, height: 402))
        XCTAssertEqual(BorderSettings(style: .film).instagramCanvas(for: CGSize(width: 1000, height: 400)), CGSize(width: 1060, height: 1414))
        XCTAssertEqual(BorderSettings(style: .film35).instagramCanvas(for: CGSize(width: 300, height: 400)), CGSize(width: 324, height: 432))
    }

    func testBorderSettingsSavedBeforeInstagramRestore() throws {
        let json = Data(#"{"enabled":true,"style":"film","export":false}"#.utf8)
        let border = try JSONDecoder().decode(BorderSettings.self, from: json)
        XCTAssertEqual(border, BorderSettings(enabled: true, style: .film, export: false, instagram: false))
    }

    /// Sessions saved with a white border restore it as Solid, white, 3%.
    func testWhiteBorderRestoresAsSolid() throws {
        let json = Data(#"{"enabled":true,"style":"white","export":true,"instagram":false}"#.utf8)
        let border = try JSONDecoder().decode(BorderSettings.self, from: json)
        XCTAssertEqual(border, BorderSettings(enabled: true, style: .solid))
        XCTAssertEqual(border.width, 0.03)
        XCTAssertEqual(border.color, .white)
        let saved = try JSONDecoder().decode(BorderSettings.self, from: JSONEncoder().encode(BorderSettings(style: .keyline, color: BorderColor(red: 32, green: 64, blue: 96))))
        XCTAssertEqual(saved.color.hex, "#204060")
    }

    func testBorderColorsAreSRGB() {
        XCTAssertEqual(BorderColor(hex: "#204060"), BorderColor(red: 0x20, green: 0x40, blue: 0x60))
        XCTAssertEqual(BorderColor(hex: "FF8000")?.hex, "#ff8000")
        XCTAssertNil(BorderColor(hex: "#ff80"))
        XCTAssertNil(BorderColor(hex: "#gg8000"))
        XCTAssertEqual(BorderColor(BorderColor(red: 12, green: 200, blue: 99).cgColor), BorderColor(red: 12, green: 200, blue: 99))
        // Colours from other spaces convert; grey has one component.
        XCTAssertEqual(BorderColor(CGColor(gray: 1, alpha: 1)), .white)
        let p3Red = CGColor(colorSpace: CGColorSpace(name: CGColorSpace.displayP3)!, components: [1, 0, 0, 1])!
        XCTAssertEqual(BorderColor(p3Red), BorderColor(red: 255, green: 0, blue: 0))
    }

    /// The options the exporter reads; see `BorderOptions` in `spektrafilm-io::border`.
    func testExporterOptions() {
        let keyline = BorderSettings(style: .keyline, instagram: true, width: 0.05, line: 0.01, color: .black, lineColor: .white)
        XCTAssertEqual(keyline.options, ["width": .number(0.05), "line": .number(0.01), "color": .string("#000000"), "line_color": .string("#ffffff")])
        XCTAssertEqual(keyline.arguments, ["--border", "keyline", "--keyline-width", "0.01", "--border-color", "#000000", "--keyline-color", "#ffffff",
                                           "--border-width", "0.05", "--instagram"])
        XCTAssertEqual(BorderSettings(style: .film, filmWidth: 0.02).options["width"], .number(0.02))
        XCTAssertNil(BorderSettings(style: .film35).options["width"])
    }

    func testBorderEditsDoNotChangeTheRenderedState() {
        var state = EditState()
        let rendered = state.rendered
        let photo = CGSize(width: 6000, height: 4000)
        state.border = BorderSettings(enabled: true, style: .film, export: true)
        XCTAssertEqual(state.rendered, rendered)
        XCTAssertEqual(state.border.exported(for: photo)?.style, .film)
        state.border.export = false
        XCTAssertNil(state.border.exported(for: photo))
    }

    func testFilmFormatFollowsTheAspectRatio() {
        XCTAssertEqual(FilmFormat(CGSize(width: 6000, height: 4000)), .full)
        XCTAssertEqual(FilmFormat(CGSize(width: 4000, height: 6000)), .full)
        XCTAssertEqual(FilmFormat(CGSize(width: 7952, height: 5304)), .full)
        XCTAssertEqual(FilmFormat(CGSize(width: 3000, height: 4000)), .half)
        XCTAssertEqual(FilmFormat(CGSize(width: 4032, height: 3024)), .half)
        XCTAssertNil(FilmFormat(CGSize(width: 1520, height: 1000)))
        XCTAssertNil(FilmFormat(CGSize(width: 1920, height: 1080)))
        XCTAssertNil(FilmFormat(.zero))
    }

    func testThe35mmBorderOnlyShowsOnFilmFrames() {
        let border = BorderSettings(enabled: true, style: .film35, export: true)
        XCTAssertEqual(border.shown(for: CGSize(width: 6000, height: 4000))?.style, .film35)
        XCTAssertNil(border.shown(for: CGSize(width: 1920, height: 1080)))
        XCTAssertNil(border.exported(for: CGSize(width: 1920, height: 1080)))
    }

    /// The exporter sizes its canvas with the same formula; see
    /// `film35_strip_surrounds_the_photo` in `spektrafilm-io::border`.
    func testFilmStripCanvas() throws {
        for (size, format) in [(CGSize(width: 600, height: 400), FilmFormat.full), (CGSize(width: 400, height: 600), .full),
                               (CGSize(width: 300, height: 400), .half), (CGSize(width: 400, height: 300), .half)] {
            let strip = try XCTUnwrap(FilmStrip.load(data: data, film: "kodak_portra_400", format: format))
            let canvas = strip.canvas(for: size)
            let full = format == .full
            let alongWidth = full == (size.width > size.height)
            let (along, across) = alongWidth ? (canvas.width / size.width, canvas.height / size.height)
                                             : (canvas.height / size.height, canvas.width / size.width)
            XCTAssertEqual(along, full ? 38.0 / 36 : 19.0 / 18, accuracy: 0.01, "\(size)")
            XCTAssertEqual(across, 35.0 / 24, accuracy: 0.01, "\(size)")
            XCTAssertEqual(BorderSettings(style: .film35).canvas(for: size, strip: strip), canvas)
        }
        let generic = try XCTUnwrap(FilmStrip.load(data: data, film: "not_a_film", format: .full))
        XCTAssertEqual(generic.film, "not_a_film")
        // Until the strip for the photograph's format loads, the canvas is the photograph.
        let half = try XCTUnwrap(FilmStrip.load(data: data, film: "kodak_portra_400", format: .half))
        XCTAssertEqual(BorderSettings(style: .film35).canvas(for: CGSize(width: 600, height: 400), strip: half), CGSize(width: 600, height: 400))
    }
}
