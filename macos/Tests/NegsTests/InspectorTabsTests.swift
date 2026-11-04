import XCTest
@testable import NegsApp

final class InspectorTabsTests: XCTestCase {
    func testWorkflowOrder() {
        XCTAssertEqual(InspectorTab.allCases.map(\.title), ["Camera", "Film", "Print", "Scan", "Finish", "Export"])
    }

    func testPrintIsSkippedOnlyWhileScanningFilmDirectly() {
        XCTAssertTrue(InspectorTab.print.isSkipped(scanning: true))
        for tab in InspectorTab.allCases where tab != .print {
            XCTAssertFalse(tab.isSkipped(scanning: true))
        }
        XCTAssertFalse(InspectorTab.print.isSkipped(scanning: false))
    }

    func testTabsHaveDistinctSymbols() {
        XCTAssertEqual(Set(InspectorTab.allCases.map(\.symbol)).count, InspectorTab.allCases.count)
    }
}
