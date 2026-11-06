import AppKit
import XCTest
@testable import NegsApp

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

    /// One export pixel to one screen pixel, whatever resolution the preview
    /// has and whatever the window's size. The test canvas has no window, so a
    /// point is two pixels.
    func testProofIsOneExportPixelToAScreenPixel() {
        for viewport: CGFloat in [300, 3000] {
            let scroll = canvas(viewport: viewport)
            scroll.proofPixels = CGSize(width: 1600, height: 1200)
            XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001, "\(viewport)")
        }
        let reduced = canvas(photo: CGSize(width: 400, height: 300))
        reduced.proofPixels = CGSize(width: 1600, height: 1200)
        XCTAssertEqual(reduced.magnification, 2, accuracy: 0.001)
    }

    /// Changing the export changes the photograph; changing the window doesn't.
    func testOnlyTheExportSizeChangesTheProof() {
        let scroll = canvas(viewport: 1000)
        scroll.proofPixels = CGSize(width: 1600, height: 1200)
        scroll.setFrameSize(NSSize(width: 350, height: 700))
        scroll.layout()
        XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001)
        scroll.proofPixels = CGSize(width: 800, height: 600)
        XCTAssertEqual(scroll.magnification, 0.5, accuracy: 0.001)
    }

    /// While a photograph of another orientation arrives, the long edge still sets the scale.
    func testTheLongEdgeSetsTheScaleInEitherOrientation() {
        let scroll = canvas(photo: CGSize(width: 600, height: 800))
        scroll.proofPixels = CGSize(width: 1600, height: 1200)
        XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001)
    }

    func testNothingZoomsAProof() {
        let scroll = canvas()
        scroll.proofPixels = CGSize(width: 1600, height: 1200)
        scroll.zoom(by: 2)
        XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001)
        scroll.toggleZoom(about: .zero)
        XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001)
    }

    /// A border adds to the canvas, not to the photograph's size.
    func testABorderDoesntChangeThePhotographsSize() {
        let scroll = canvas(photo: CGSize(width: 960, height: 720))
        let photo = scroll.documentView as! PhotographView
        photo.show(NSImage(size: CGSize(width: 800, height: 600)), size: CGSize(width: 800, height: 600), rotation: 0)
        scroll.proofPixels = CGSize(width: 1600, height: 1200)
        XCTAssertEqual(scroll.magnification, 1, accuracy: 0.001)
    }

    /// It pans only when it's larger than the window, to reach the rest of it.
    func testAProofPansOnlyWhenItOverflows() {
        let small = canvas(viewport: 300)
        small.proofPixels = CGSize(width: 1600, height: 1200)
        XCTAssertTrue(small.canScroll)
        let large = canvas(viewport: 2000)
        large.proofPixels = CGSize(width: 1600, height: 1200)
        XCTAssertFalse(large.canScroll)
    }

    /// Entering and leaving a proof doesn't move the photograph: it's centred
    /// in both, in the same place.
    func testAProofIsCentredWhereTheNormalViewIs() throws {
        let scroll = canvas(viewport: 1200, photo: CGSize(width: 1000, height: 1000))
        func centre() throws -> CGPoint {
            let photo = try XCTUnwrap(scroll.documentView)
            return scroll.overlay.convert(CGPoint(x: 500, y: 500), from: photo)
        }
        let normal = try centre()
        scroll.proofPixels = CGSize(width: 2000, height: 2000)
        let proof = try centre()
        XCTAssertEqual(proof.x, normal.x, accuracy: 0.5)
        XCTAssertEqual(proof.y, normal.y, accuracy: 0.5)
        scroll.proofPixels = nil
        XCTAssertEqual(try centre().y, normal.y, accuracy: 0.5)
    }

    func testLeavingProofGoesBackToTheNormalFit() throws {
        let scroll = canvas(viewport: 1000, photo: CGSize(width: 100, height: 100))
        scroll.proofPixels = CGSize(width: 1600, height: 1600)
        scroll.proofPixels = nil
        XCTAssertLessThan(scroll.magnification, 2)
        let photo = try XCTUnwrap(scroll.documentView)
        let overlay = scroll.overlay
        let (low, high) = (overlay.convert(CGPoint.zero, from: photo).y, overlay.convert(CGPoint(x: 0, y: 100), from: photo).y)
        XCTAssertEqual(min(low, high), overlay.bounds.height - max(low, high), accuracy: 1)
    }
}

/// What a proof shows: the file an export would write.
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

    /// In a proof, the white surround replaces the border, so only the photograph sets the size.
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
}
