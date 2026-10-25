import XCTest
@testable import NegsApp

/// The app's projection must agree with `spektrafilm-io::image_size`, whose
/// tests use the same examples.
final class ImageSizeTests: XCTestCase {
    private func params(target: Int? = nil) -> JSONValue {
        .object(["io": .object(["photo_long_side_px": target.map { .number(Double($0)) } ?? .null])])
    }

    func testModesFromParameters() {
        XCTAssertEqual(ImageSize.mode(of: params()), .original)
        XCTAssertEqual(ImageSize.mode(of: params(target: 6000)), .longestSide)
        // An old session without the field is Original.
        XCTAssertEqual(ImageSize.mode(of: .object(["io": .object([:])])), .original)
    }

    func testProjectionMatchesTheEngine() {
        let centre = CropSettings(rect: CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5))
        let source = CGSize(width: 6000, height: 4000)
        let working = ImageSize.working(source: source, params: params(target: 6000), crop: centre)
        XCTAssertEqual(working, CGSize(width: 12000, height: 8000))
        XCTAssertEqual(centre.size(of: working), CGSize(width: 6000, height: 4000))
        // Panning keeps the frame; the crop's pixels stay within one of the target.
        let panned = CropSettings(rect: CGRect(x: 0.137, y: 0.41, width: 0.5, height: 0.5))
        XCTAssertEqual(ImageSize.working(source: source, params: params(target: 6000), crop: panned), working)
        XCTAssertLessThanOrEqual(abs(panned.size(of: working).width - 6000), 1)
        // Original is the source, whatever the crop.
        XCTAssertEqual(ImageSize.working(source: source, params: params(), crop: centre), source)
        // Half-pixel edges, as in the engine's test.
        let half = CropSettings(rect: CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.25))
        XCTAssertEqual(ImageSize.working(source: CGSize(width: 100, height: 80), target: 25, crop: half), CGSize(width: 50, height: 40))
    }

    func testWorkingLimitBoundsEnlargementsOnly() {
        let source = CGSize(width: 8000, height: 4000)
        XCTAssertFalse(ImageSize.exceedsLimit(working: ImageSize.working(source: source, target: 16_000, crop: CropSettings()), source: source))
        XCTAssertTrue(ImageSize.exceedsLimit(working: ImageSize.working(source: source, target: 16_002, crop: CropSettings()), source: source))
        // A larger source still develops at its own size or smaller.
        let large = CGSize(width: 16_000, height: 10_000)
        XCTAssertFalse(ImageSize.exceedsLimit(working: ImageSize.working(source: large, target: 15_000, crop: CropSettings()), source: large))
        let small = CropSettings(rect: CGRect(x: 0.4, y: 0.4, width: 0.2, height: 0.2))
        let working = ImageSize.working(source: CGSize(width: 4000, height: 3000), target: 6000, crop: small)
        XCTAssertEqual(working, CGSize(width: 30000, height: 22500))
        XCTAssertTrue(ImageSize.exceedsLimit(working: working, source: CGSize(width: 4000, height: 3000)))
        XCTAssertTrue(ImageSize.limitMessage(working: working).contains("675 MP"))
    }

    /// The crop's extent is the render's identity only with a target.
    func testRenderedStateKeepsTheCropExtentOnlyForLongestSide() {
        var state = EditState()
        state.params = params()
        let original = state.rendered
        state.crop = CropSettings(rect: CGRect(x: 0.1, y: 0.2, width: 0.5, height: 0.5), aspect: .square, angle: 3)
        XCTAssertEqual(state.rendered, original, "Original never renders for a crop")
        state.params = params(target: 4000)
        let sized = state.rendered
        state.crop.rect.origin = CGPoint(x: 0.3, y: 0.1)
        state.crop.angle = -2; state.crop.aspect = .original
        XCTAssertEqual(state.rendered, sized, "Panning, straightening and the ratio choice reuse the render")
        state.crop.rect.size = CGSize(width: 0.6, height: 0.5)
        XCTAssertNotEqual(state.rendered, sized, "A different extent needs a different working size")
        state.crop = CropSettings()
        state.params = params(target: 4000)
        XCTAssertEqual(state.rendered.crop, CropSettings())
    }
}
