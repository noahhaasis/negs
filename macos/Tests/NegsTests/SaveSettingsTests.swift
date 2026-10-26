import XCTest
@testable import NegsApp

final class SaveSettingsTests: XCTestCase {
    func testProPhotoNeedsSixteenBits() {
        XCTAssertEqual(SaveSettings.effectiveSpace(SaveSettings.prophoto, bitDepth: 16), SaveSettings.prophoto)
        XCTAssertEqual(SaveSettings.effectiveSpace(SaveSettings.prophoto, bitDepth: 8), SaveSettings.prophotoFallback)
        XCTAssertEqual(SaveSettings.effectiveSpace(1, bitDepth: 8), 1)
        XCTAssertEqual(SaveSettings.effectiveSpace(99, bitDepth: 16), SaveSettings.prophoto)
    }

    /// As `spektrafilm-io::save::SaveOptions` and the CLI take them.
    func testSettingsReachTheEngine() {
        let save = SaveSettings(bitDepth: 16, colorSpace: 1, jpegQuality: 80)
        XCTAssertEqual(save.json, .object(["jpeg_quality": .number(80), "bit_depth": .number(16), "color_space": .string("Display P3")]))
        XCTAssertEqual(save.arguments, ["--jpeg-quality", "80", "--color-space", "Display P3", "--bit-depth", "16"])
        XCTAssertFalse(save.isSRGB)
        // The defaults leave the bit depth to the format and save sRGB.
        XCTAssertEqual(SaveSettings().arguments, ["--jpeg-quality", "95", "--color-space", "sRGB"])
        XCTAssertNil(SaveSettings().summary(export: true))
    }
}
