import XCTest
@testable import NegsApp

/// Every inspector control and group explains itself in a tooltip.
final class ControlHelpTests: XCTestCase {
    func testEveryControlAndGroupHasATooltip() {
        let controls = adjustmentGroups.flatMap(\.controls) + prepToneControls + prepColorControls + prepDetailControls
        var missing: [String] = []
        for control in controls {
            switch control {
            case .slider(_, let path, _, _), .toggle(_, let path), .choice(_, let path, _), .stockSlider(_, let path, _, _, _, _):
                if ControlHelp.text(path) == nil { missing.append(path) }
            case .balancePrint(_, let help), .filterLink(let help):
                XCTAssertFalse(help.isEmpty)
            }
        }
        for path in ["io.raw_white_balance.mode", "io.raw_white_balance.temperature", "io.raw_white_balance.tint",
                     "camera.auto_exposure_method", "camera.exposure_compensation_ev", "camera.film_format_mm",
                     "camera.lens_blur_um", "io.input_color_space", "io.input_cctf_decoding", "io.input_interpretation.linear"]
        where ControlHelp.text(path) == nil {
            missing.append(path)
        }
        XCTAssertEqual(missing, [], "Controls without a tooltip")
        for group in adjustmentGroups.map(\.title) + ["Input Colour", "Output"] {
            XCTAssertNotNil(ControlHelp.group(group), group)
        }
    }
}
