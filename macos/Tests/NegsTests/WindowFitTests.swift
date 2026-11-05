import AppKit
import XCTest
@testable import NegsApp

final class WindowFitTests: XCTestCase {
    private let chrome = CGSize(width: 400, height: 200)
    private let limit = CGSize(width: 3000, height: 1600)

    private func fit(_ canvas: CGSize) -> CGSize { WindowFit.contentSize(canvas: canvas, chrome: chrome, limit: limit) }

    func testAnExportThatFitsIsShownAtItsSize() {
        XCTAssertEqual(fit(CGSize(width: 1000, height: 700)), CGSize(width: 1400, height: 900))
    }

    /// Too big for the screen, it scales down in the same shape.
    func testAnExportTooBigForTheScreenKeepsItsShape() {
        let size = fit(CGSize(width: 2457, height: 1775))
        XCTAssertEqual(size.height, limit.height, accuracy: 0.5)
        XCTAssertEqual((size.width - chrome.width) / (size.height - chrome.height), 2457.0 / 1775, accuracy: 0.001)
    }

    func testASmallExportStillGetsAUsableCanvas() {
        let size = fit(CGSize(width: 32, height: 24))
        XCTAssertEqual(size.height - chrome.height, WindowFit.minimumCanvas, accuracy: 0.5)
        XCTAssertEqual((size.width - chrome.width) / (size.height - chrome.height), 32.0 / 24, accuracy: 0.001)
    }

    /// Two exports of one size give one window, so changing photographs doesn't move it.
    func testTheSameExportSizeGivesTheSameWindow() {
        XCTAssertEqual(fit(CGSize(width: 1228, height: 887)), fit(CGSize(width: 1228, height: 887)))
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

    /// Locked, nothing pans: a press can't scroll, and neither can the wheel.
    func testLockedCanvasDoesNotPan() throws {
        let scroll = canvas(viewport: 200)
        scroll.locksToFit = true
        scroll.magnification = 3
        XCTAssertFalse(scroll.canScroll)
        let origin = scroll.contentView.bounds.origin
        let wheel = try XCTUnwrap(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: -40, wheel2: 0, wheel3: 0))
        scroll.scrollWheel(with: try XCTUnwrap(NSEvent(cgEvent: wheel)))
        XCTAssertEqual(scroll.contentView.bounds.origin, origin)
    }

    /// Unlocked, fit is back to never enlarging past 1:1.
    func testUnlockingGoesBackToTheNormalFit() {
        let scroll = canvas(viewport: 1000, photo: CGSize(width: 100, height: 100))
        scroll.locksToFit = true
        scroll.locksToFit = false
        XCTAssertLessThan(scroll.magnification, 2)
    }
}

/// What locking the window shows: the file an export would write.
@MainActor final class ExportPixelsTests: XCTestCase {
    /// The saved file measured at 4914 × 3549: 4096 on the long side and a 10 % border.
    func testAPhotographWithABorderMatchesTheSavedFile() {
        XCTAssertEqual(ExportPixels.size(photograph: CGSize(width: 4096, height: 2731), rotated: false, withBorder: { CGSize(width: $0.width + 818, height: $0.height + 818) }, fit: .full),
                       CGSize(width: 4914, height: 3549))
    }

    func testATurnedPhotographSwapsItsSides() {
        XCTAssertEqual(ExportPixels.size(photograph: CGSize(width: 400, height: 300), rotated: true, withBorder: nil, fit: .full), CGSize(width: 300, height: 400))
    }

    /// An export limit only ever reduces, as the exporter's does.
    func testAnExportLimitReducesButNeverEnlarges() {
        let photo = CGSize(width: 4000, height: 2000)
        XCTAssertEqual(ExportPixels.size(photograph: photo, rotated: false, withBorder: nil, fit: ExportSize(edge: .long, pixels: 1000)), CGSize(width: 1000, height: 500))
        XCTAssertEqual(ExportPixels.size(photograph: photo, rotated: false, withBorder: nil, fit: ExportSize(edge: .short, pixels: 1000)), CGSize(width: 2000, height: 1000))
        XCTAssertEqual(ExportPixels.size(photograph: photo, rotated: false, withBorder: nil, fit: ExportSize(edge: .long, pixels: 9000)), photo)
    }

    func testPTogglesTheLockOutsideTextFields() {
        let p: UInt16 = 35
        XCTAssertTrue(EditorWindowController.togglesWindowLock(keyCode: p, modifiers: [], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesWindowLock(keyCode: p, modifiers: [], firstResponder: NSTextView()))
        XCTAssertFalse(EditorWindowController.togglesWindowLock(keyCode: p, modifiers: [.command], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesWindowLock(keyCode: 48, modifiers: [], firstResponder: nil))
    }
}
