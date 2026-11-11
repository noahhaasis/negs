import XCTest
@testable import NegsApp

/// The list under a tab's count lists what the count counts, and Reset on the
/// tab clears both in one undo step.
@MainActor final class TabEditsTests: XCTestCase {
    func testTheListHasAsManyEditsAsTheCount() {
        let model = EditorModel()
        for tab in InspectorTab.allCases {
            XCTAssertEqual(InspectorEdits.edits(tab, model).count, InspectorEdits.count(tab, model), "\(tab)")
        }
        model.set("prep.contrast", .number(12))
        model.set("prep.shadows", .number(-30))
        model.set("film_render.halation.strength", .number(2))
        for tab in InspectorTab.allCases {
            XCTAssertEqual(InspectorEdits.edits(tab, model).count, InspectorEdits.count(tab, model), "\(tab)")
        }
    }

    func testResettingATabLeavesTheOthers() {
        let model = EditorModel()
        model.set("prep.contrast", .number(12))
        model.set("film_render.halation.strength", .number(2))
        let lab = InspectorEdits.count(.lab, model)
        XCTAssertGreaterThan(InspectorEdits.count(.prep, model), 0)
        model.resetEdits(tab: .prep)
        XCTAssertEqual(InspectorEdits.count(.prep, model), 0)
        XCTAssertEqual(InspectorEdits.count(.lab, model), lab)
    }

    func testOneEditResetsAlone() {
        let model = EditorModel()
        model.set("prep.contrast", .number(12))
        model.set("prep.shadows", .number(-30))
        model.resetEdits([.param("prep.contrast")], action: "Reset Contrast")
        XCTAssertFalse(model.isModified("prep.contrast"))
        XCTAssertTrue(model.isModified("prep.shadows"))
    }
}
