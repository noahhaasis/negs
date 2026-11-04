import XCTest
@testable import NegsApp

/// Fitting shows the whole photograph, but never past 1:1 (one photograph
/// pixel per screen pixel, half a point on Retina).
@MainActor final class FitZoomTests: XCTestCase {
    private let viewport = CGSize(width: 1000, height: 750)

    func testLargePhotographsFitTheViewport() {
        let amount = ImageScrollView.magnification(fitting: true, document: CGSize(width: 6000, height: 4000), viewport: viewport, backingScale: 2)
        XCTAssertEqual(amount, 1000.0 / 6000, accuracy: 1e-9)
    }

    func testSmallPhotographsStopAtOneToOne() {
        let retina = ImageScrollView.magnification(fitting: true, document: CGSize(width: 1600, height: 1067), viewport: viewport, backingScale: 2)
        XCTAssertEqual(retina, 0.5, accuracy: 1e-9, "Fit would enlarge to 0.625; 1:1 on Retina is 0.5")
        let standard = ImageScrollView.magnification(fitting: true, document: CGSize(width: 800, height: 600), viewport: viewport, backingScale: 1)
        XCTAssertEqual(standard, 1, accuracy: 1e-9)
    }

    func testActualSizeIgnoresTheViewport() {
        let amount = ImageScrollView.magnification(fitting: false, document: CGSize(width: 6000, height: 4000), viewport: viewport, backingScale: 2)
        XCTAssertEqual(amount, 0.5, accuracy: 1e-9)
    }
}
