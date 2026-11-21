@testable import NegsApp
import XCTest

/// Round 31, C: a RAW with the camera's picture beside it shows "+JPG".
final class PairBadgeTests: XCTestCase {
    func testTheBadgeNamesTheCamerasPicture() {
        let url = { (name: String) in URL(fileURLWithPath: "/roll/" + name) }
        XCTAssertEqual(PairBadge.word(for: url("DSCF5759.JPG"), short: false), "+JPG")
        XCTAssertEqual(PairBadge.word(for: url("dscf5759.jpeg"), short: false), "+JPG")
        XCTAssertEqual(PairBadge.word(for: url("IMG_0001.HIF"), short: false), "+HEIF")
        XCTAssertEqual(PairBadge.word(for: url("IMG_0001.heic"), short: false), "+HEIF")
        XCTAssertEqual(PairBadge.word(for: url("DSCF5759.JPG"), short: true), "+J")
        XCTAssertEqual(PairBadge.word(for: url("IMG_0001.HEIF"), short: true), "+H")
    }

    func testVoiceOverStillHearsIt() {
        let label = PhotoMarks.accessibilityLabel("DSCF5759.RAF", summary: LibrarySummary(),
                                                  cameraJPEG: URL(fileURLWithPath: "/roll/DSCF5759.JPG"))
        XCTAssertEqual(label, "DSCF5759.RAF, with camera JPEG")
    }
}
