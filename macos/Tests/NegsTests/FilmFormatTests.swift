import XCTest
@testable import NegsApp

final class FilmFormatTests: XCTestCase {
    func testFormatsReadBackFromTheLongEdge() {
        XCTAssertEqual(FilmFormatPicker.format(mm: 35), "35 mm (135)", "The engine default is 135")
        XCTAssertEqual(FilmFormatPicker.format(mm: 56), "6×4.5 / 6×6")
        XCTAssertEqual(FilmFormatPicker.format(mm: 120), "4×5")
        // Rust defaults arrive as widened f32 values.
        XCTAssertEqual(FilmFormatPicker.format(mm: Double(Float(24))), "Half frame")
        XCTAssertNil(FilmFormatPicker.format(mm: 45), "Other values read as Custom")
        XCTAssertNil(FilmFormatPicker.format(mm: 36))
    }

    func testEveryFormatFitsTheCustomSlider() {
        for format in FilmFormatPicker.formats {
            XCTAssertTrue(FilmFormatPicker.range.contains(format.mm), format.name)
        }
        XCTAssertEqual(Set(FilmFormatPicker.formats.map(\.mm)).count, FilmFormatPicker.formats.count, "Each long edge names one format")
    }
}
