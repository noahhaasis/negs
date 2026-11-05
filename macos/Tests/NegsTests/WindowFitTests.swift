import AppKit
import XCTest
@testable import NegsApp

final class WindowFitTests: XCTestCase {
    private let chrome = CGSize(width: 400, height: 200)
    private let limit = CGSize(width: 3000, height: 1600)

    private func fit(_ canvas: CGSize) -> CGSize { WindowFit.contentSize(canvas: canvas, chrome: chrome, limit: limit) }

    /// The mat is 8 % of the long edge on the sides and top, 1.2 × that below.
    func testTheMatScalesWithThePhotographAndWeightsTheBottom() {
        let mat = WindowFit.mat(forLongEdge: 1000)
        XCTAssertEqual(mat.side, 80, accuracy: 0.001)
        XCTAssertEqual(mat.top, 80, accuracy: 0.001)
        XCTAssertEqual(mat.bottom, 96, accuracy: 0.001)
    }

    /// The export keeps its size, with the mat around it.
    func testAnExportThatFitsIsShownAtItsSize() {
        let size = fit(CGSize(width: 1000, height: 700))
        XCTAssertEqual(size.width, 1000 + 160 + chrome.width, accuracy: 0.01)
        XCTAssertEqual(size.height, 700 + 80 + 96 + chrome.height, accuracy: 0.01)
    }

    /// Too big for the screen, photograph and mat scale down together.
    func testAnExportTooBigForTheScreenKeepsItsShape() {
        let size = fit(CGSize(width: 2457, height: 1775))
        let mat = WindowFit.mat(forLongEdge: 2457)
        XCTAssertEqual(size.height, limit.height, accuracy: 0.5)
        XCTAssertEqual((size.width - chrome.width) / (size.height - chrome.height),
                       (2457 + mat.side * 2) / (1775 + mat.top + mat.bottom), accuracy: 0.001)
    }

    func testASmallExportStillGetsAUsableCanvas() {
        let size = fit(CGSize(width: 32, height: 24))
        // Scaled up to 320 × 240, with the mat of that size.
        XCTAssertEqual(size.height - chrome.height, 240 + 25.6 + 30.72, accuracy: 0.01)
        XCTAssertEqual(size.width - chrome.width, 320 + 51.2, accuracy: 0.01)
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
        // Photograph and mat fill the canvas: 100 + 8 + 9.6 tall sets the scale.
        let filled: CGFloat = 1000 / 117.6
        XCTAssertEqual(scroll.magnification, filled, accuracy: 0.01)
        scroll.zoom(by: 2)
        XCTAssertEqual(scroll.magnification, filled, accuracy: 0.01)
        scroll.toggleZoom(about: .zero)
        XCTAssertEqual(scroll.magnification, filled, accuracy: 0.01)
        XCTAssertTrue(scroll.fitting)
    }

    /// The photograph sits above centre, with 1.2 × the mat below as above it.
    func testTheLockedPhotographSitsAboveCentre() throws {
        let scroll = canvas(viewport: 1000, photo: CGSize(width: 100, height: 100))
        scroll.locksToFit = true
        let photo = try XCTUnwrap(scroll.documentView)
        let overlay = scroll.overlay
        let (low, high) = (overlay.convert(CGPoint.zero, from: photo).y, overlay.convert(CGPoint(x: 0, y: 100), from: photo).y)
        let (bottom, top) = (min(low, high), overlay.bounds.height - max(low, high))
        XCTAssertEqual(bottom / top, 1.2, accuracy: 0.02, "top \(top) bottom \(bottom)")
        XCTAssertEqual(top, 8 * 1000 / 117.6, accuracy: 1)
    }

    /// Unlocked it's centred, as ever.
    func testAnUnlockedPhotographIsCentred() throws {
        let scroll = canvas(viewport: 1000, photo: CGSize(width: 100, height: 100))
        scroll.locksToFit = true
        scroll.locksToFit = false
        let photo = try XCTUnwrap(scroll.documentView)
        let overlay = scroll.overlay
        let (low, high) = (overlay.convert(CGPoint.zero, from: photo).y, overlay.convert(CGPoint(x: 0, y: 100), from: photo).y)
        XCTAssertEqual(min(low, high), overlay.bounds.height - max(low, high), accuracy: 1)
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

    /// Locked, the mat replaces the border, so only the photograph sets the size.
    func testThePhotographsOwnPixelsLeaveOutTheBorder() {
        let photo = CGSize(width: 4096, height: 2731)
        let framed: (CGSize) -> CGSize = { CGSize(width: $0.width + 818, height: $0.height + 818) }
        XCTAssertEqual(ExportPixels.photograph(photo, rotated: false, withBorder: framed, fit: .full), photo)
        XCTAssertEqual(ExportPixels.photograph(photo, rotated: true, withBorder: framed, fit: .full), CGSize(width: 2731, height: 4096))
    }

    /// An export limit fits the framed file, so the photograph inside it is smaller.
    func testAnExportLimitScalesThePhotographInsideTheFrame() {
        let size = ExportPixels.photograph(CGSize(width: 800, height: 400), rotated: false, withBorder: { CGSize(width: $0.width + 200, height: $0.height + 200) }, fit: ExportSize(edge: .long, pixels: 500))
        XCTAssertEqual(size, CGSize(width: 400, height: 200))
    }

    func testTheCanvasDropsTheBorderWhileLocked() {
        let model = EditorModel()
        let saved = UserDefaults.standard.object(forKey: "lockWindowToPhotograph")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "lockWindowToPhotograph")
        }
        model.border(\.enabled).wrappedValue = true
        let photo = CGSize(width: 800, height: 600)
        model.windowLockedToPhotograph = false
        XCTAssertNotNil(model.canvasBorder(for: photo))
        model.windowLockedToPhotograph = true
        XCTAssertNil(model.canvasBorder(for: photo))
        XCTAssertTrue(model.border.enabled, "The setting stays for when it's unlocked")
    }

    func testTheBorderCantBeEditedWhileLocked() {
        let model = EditorModel()
        let saved = UserDefaults.standard.object(forKey: "lockWindowToPhotograph")
        defer {
            model.shutdown()
            UserDefaults.standard.set(saved, forKey: "lockWindowToPhotograph")
        }
        model.windowLockedToPhotograph = false
        XCTAssertTrue(model.canEditBorder)
        model.windowLockedToPhotograph = true
        XCTAssertFalse(model.canEditBorder)
    }

    func testPTogglesTheLockOutsideTextFields() {
        let p: UInt16 = 35
        XCTAssertTrue(EditorWindowController.togglesWindowLock(keyCode: p, modifiers: [], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesWindowLock(keyCode: p, modifiers: [], firstResponder: NSTextView()))
        XCTAssertFalse(EditorWindowController.togglesWindowLock(keyCode: p, modifiers: [.command], firstResponder: nil))
        XCTAssertFalse(EditorWindowController.togglesWindowLock(keyCode: 48, modifiers: [], firstResponder: nil))
    }
}
