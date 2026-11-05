import AppKit
import XCTest
@testable import NegsApp

final class WindowFitTests: XCTestCase {
    private let chrome = CGSize(width: 400, height: 200)
    private let limit = CGSize(width: 2000, height: 1200)

    private func canvas(_ content: CGSize) -> CGSize { CGSize(width: content.width - chrome.width, height: content.height - chrome.height) }

    func testDraggingTheWidthSetsTheHeight() {
        let size = WindowFit.resized(document: CGSize(width: 3, height: 2), chrome: chrome, proposed: CGSize(width: 1000, height: 500), current: CGSize(width: 900, height: 500), limit: limit)
        XCTAssertEqual(canvas(size).width, 600, accuracy: 1)
        XCTAssertEqual(canvas(size).height, 400, accuracy: 1)
    }

    func testDraggingTheHeightSetsTheWidth() {
        let size = WindowFit.resized(document: CGSize(width: 3, height: 2), chrome: chrome, proposed: CGSize(width: 1000, height: 800), current: CGSize(width: 1000, height: 600), limit: limit)
        XCTAssertEqual(canvas(size).height, 600, accuracy: 1)
        XCTAssertEqual(canvas(size).width, 900, accuracy: 1)
    }

    func testItStaysOnTheScreenAndKeepsTheShape() {
        let size = WindowFit.resized(document: CGSize(width: 2, height: 3), chrome: chrome, proposed: CGSize(width: 1900, height: 900), current: CGSize(width: 1000, height: 900), limit: limit)
        XCTAssertLessThanOrEqual(size.height, limit.height)
        XCTAssertLessThanOrEqual(size.width, limit.width)
        XCTAssertEqual(canvas(size).width / canvas(size).height, 2.0 / 3, accuracy: 0.01)
    }

    func testAPanoramaCanStillBeMadeUsable() {
        let size = WindowFit.resized(document: CGSize(width: 6, height: 1), chrome: chrome, proposed: CGSize(width: 500, height: 300), current: CGSize(width: 600, height: 300), limit: limit)
        XCTAssertEqual(canvas(size).height, WindowFit.minimumCanvas, accuracy: 1, "The shorter side keeps a usable size")
        XCTAssertEqual(canvas(size).width / canvas(size).height, 6, accuracy: 0.05)
    }

    /// A new shape keeps about the same canvas area, so turning a photograph
    /// neither collapses the window nor grows it.
    func testRefittingKeepsTheAreaAndTakesTheShape() {
        let current = CGSize(width: 1000, height: 600)
        let size = WindowFit.refitted(document: CGSize(width: 1, height: 1), chrome: chrome, current: current, limit: limit)
        XCTAssertEqual(canvas(size).width, canvas(size).height, accuracy: 1)
        XCTAssertEqual(canvas(size).width * canvas(size).height, 600 * 400, accuracy: 2000)
    }

    /// A window already the right shape stays put, so refitting can't loop.
    func testRefittingAWindowThatFitsChangesNothing() {
        let current = CGSize(width: 1000, height: 600)
        let size = WindowFit.refitted(document: CGSize(width: 3, height: 2), chrome: chrome, current: current, limit: limit)
        XCTAssertEqual(size.width, current.width, accuracy: 1.5)
        XCTAssertEqual(size.height, current.height, accuracy: 1.5)
    }
}

@MainActor final class CanvasZoomTests: XCTestCase {
    private func canvas(viewport: CGFloat = 400, photo: CGSize = CGSize(width: 800, height: 600)) -> ImageScrollView {
        _ = NSApplication.shared
        let scroll = ImageScrollView(frame: NSRect(x: 0, y: 0, width: viewport, height: viewport))
        scroll.contentView = CenteringClipView()
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        scroll.documentView = PhotographView(frame: CGRect(origin: .zero, size: photo))
        scroll.tile()
        return scroll
    }

    func testZoomingInAndOutStepsAndStopsFitting() {
        let scroll = canvas()
        scroll.magnification = 1
        scroll.zoom(by: ImageScrollView.zoomStep)
        XCTAssertEqual(scroll.magnification, ImageScrollView.zoomStep, accuracy: 0.001)
        XCTAssertFalse(scroll.fitting)
        scroll.zoom(by: 1 / ImageScrollView.zoomStep)
        XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001)
    }

    func testZoomStaysWithinItsLimits() {
        let scroll = canvas()
        scroll.magnification = 15
        scroll.zoom(by: ImageScrollView.zoomStep)
        XCTAssertEqual(scroll.magnification, 16, accuracy: 0.001)
    }

    /// Locked, the photograph fills the canvas even above 1:1, and nothing zooms it.
    func testLockedFitFillsTheCanvasAboveOneToOne() {
        let scroll = canvas(viewport: 1000, photo: CGSize(width: 100, height: 100))
        scroll.locksToFit = true
        XCTAssertEqual(scroll.magnification, 10, accuracy: 0.01)
        scroll.zoom(by: 2)
        XCTAssertEqual(scroll.magnification, 10, accuracy: 0.01)
        scroll.toggleZoom(about: .zero)
        XCTAssertEqual(scroll.magnification, 10, accuracy: 0.01)
        XCTAssertTrue(scroll.fitting)
    }

    /// Unlocked, fit is back to never enlarging past 1:1.
    func testUnlockingGoesBackToTheNormalFit() {
        let scroll = canvas(viewport: 1000, photo: CGSize(width: 100, height: 100))
        scroll.locksToFit = true
        scroll.locksToFit = false
        XCTAssertLessThan(scroll.magnification, 2)
    }
}
