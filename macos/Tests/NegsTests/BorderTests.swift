import XCTest
@testable import NegsApp

final class BorderTests: XCTestCase {
    private let data = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../../../data").standardizedFileURL

    /// The same sizes as the exporter's tests in `spektrafilm-io::border`.
    func testGeometryMatchesExporter() {
        XCTAssertEqual(BorderStyle.white.canvas(for: CGSize(width: 200, height: 100)), CGSize(width: 212, height: 112))
        XCTAssertEqual(BorderStyle.keyline.canvas(for: CGSize(width: 1000, height: 500)), CGSize(width: 1068, height: 568))
        XCTAssertEqual(BorderStyle.keyline(for: CGSize(width: 1000, height: 500)), 4)
        XCTAssertEqual(BorderStyle.film.canvas(for: CGSize(width: 6000, height: 4000)), CGSize(width: 6024, height: 4024))
        XCTAssertEqual(BorderStyle.film(for: CGSize(width: 6000, height: 4000)).pad, 100)
    }

    func testBorderEditsDoNotChangeTheRenderedState() {
        var state = EditState()
        let rendered = state.rendered
        let photo = CGSize(width: 6000, height: 4000)
        state.border = BorderSettings(enabled: true, style: .film, export: true)
        XCTAssertEqual(state.rendered, rendered)
        XCTAssertEqual(state.border.exported(for: photo), .film)
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
        XCTAssertEqual(border.shown(for: CGSize(width: 6000, height: 4000)), .film35)
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
            XCTAssertEqual(BorderStyle.film35.canvas(for: size, strip: strip), canvas)
        }
        let generic = try XCTUnwrap(FilmStrip.load(data: data, film: "not_a_film", format: .full))
        XCTAssertEqual(generic.film, "not_a_film")
        // Until the strip for the photograph's format loads, the canvas is the photograph.
        let half = try XCTUnwrap(FilmStrip.load(data: data, film: "kodak_portra_400", format: .half))
        XCTAssertEqual(BorderStyle.film35.canvas(for: CGSize(width: 600, height: 400), strip: half), CGSize(width: 600, height: 400))
    }
}
