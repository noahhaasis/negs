import XCTest
@testable import NegsApp

/// The status line's words (§4.7).
@MainActor final class StatusLineTests: XCTestCase {
    func testExportingCountsTheOneUnderWayFromOne() {
        XCTAssertEqual(StatusText.exporting(0, of: 12), "Exporting 1 of 12 photographs")
        XCTAssertEqual(StatusText.exporting(4, of: 12), "Exporting 5 of 12 photographs")
        // The last one finishing doesn't count past the total.
        XCTAssertEqual(StatusText.exporting(12, of: 12), "Exporting 12 of 12 photographs")
        XCTAssertEqual(StatusText.exporting(0, of: 1), "Exporting 1 of 1 photograph")
    }

    func testCroppingSaysTheRatioAndTheExportedSize() {
        XCTAssertEqual(StatusText.cropping("3:2", size: CGSize(width: 3935.6, height: 2624.2)), "Cropping · 3:2 · 3936 × 2624 px")
        XCTAssertEqual(StatusText.cropping("Free", size: CGSize(width: 800, height: 600)), "Cropping · Free · 800 × 600 px")
    }

    func testANoteIsSaidInSentenceCase() {
        XCTAssertEqual(StatusText.action("Edits Pasted"), "Edits pasted")
        XCTAssertEqual(StatusText.action("Edits Not Copied"), "Edits not copied")
        XCTAssertEqual(StatusText.action("Rejected"), "Rejected")
        XCTAssertEqual(StatusText.action("★★★"), "★★★")
    }

    func testANoteSaysHowManyAndWhy() {
        typealias Note = EditorModel.LibraryNote
        XCTAssertEqual(StatusText.detail(Note(text: "★★", serial: 1)), "")
        XCTAssertEqual(StatusText.detail(Note(text: "★★", count: 4, serial: 1)), "4 photographs")
        XCTAssertEqual(StatusText.detail(Note(text: "Not Duplicated", detail: "Settings say not to", serial: 1)), "Settings say not to")
        XCTAssertEqual(StatusText.detail(Note(text: "Picked", detail: "Not saved", count: 3, serial: 1)), "3 photographs · Not saved")
    }
}
