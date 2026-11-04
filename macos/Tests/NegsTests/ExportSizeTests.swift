import XCTest
@testable import NegsApp

final class ExportSizeTests: XCTestCase {
    func testPixelsAreReadFromDigits() {
        XCTAssertEqual(ExportSize.pixels(from: "2048"), 2048)
        XCTAssertEqual(ExportSize.pixels(from: "2,048"), 2048)
        XCTAssertEqual(ExportSize.pixels(from: " 3000 "), 3000)
    }

    func testBlankOrTextOnlyFieldsUseTheDefault() {
        XCTAssertEqual(ExportSize.pixels(from: ""), ExportSize.full.pixels)
        XCTAssertEqual(ExportSize.pixels(from: "abc"), ExportSize.full.pixels)
    }

    func testOutOfRangeValuesAreClamped() {
        XCTAssertEqual(ExportSize.pixels(from: "5"), ExportSize.pixelRange.lowerBound)
        XCTAssertEqual(ExportSize.pixels(from: "999999"), ExportSize.pixelRange.upperBound)
    }

    func testTheClampedValueReachesTheExporter() {
        let size = ExportSize(edge: .long, pixels: ExportSize.pixels(from: "5"))
        XCTAssertEqual(size.arguments, ["--long-edge", "16"])
        XCTAssertEqual(size.summary, "Fits 16 px on the long edge")
    }
}
