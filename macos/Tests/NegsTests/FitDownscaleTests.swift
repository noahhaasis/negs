import AppKit
import IOSurface
import ScreenCaptureKit
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

/// The fitted photograph is resampled to exactly the physical pixels it
/// covers and drawn 1:1 on the pixel grid (`FitDownscale`).
@MainActor final class FitDownscaleTests: XCTestCase {
    func testTheTargetIsThePhysicalPixelsThePhotographCovers() {
        XCTAssert(FitDownscale.targetSize(photo: CGSize(width: 6000, height: 4000), scale: 2200.0 / 6000)! == (2200, 1467))
        // The Windows report: 4896 × 3264 in 1035 physical pixels of height.
        XCTAssert(FitDownscale.targetSize(photo: CGSize(width: 4896, height: 3264), scale: 1035.0 / 3264)! == (1553, 1035))
        // Odd sizes round to the nearest pixel, as the layer's corners do.
        XCTAssert(FitDownscale.targetSize(photo: CGSize(width: 6001, height: 3999), scale: 0.25)! == (1500, 1000))
        XCTAssert(FitDownscale.targetSize(photo: CGSize(width: 333, height: 777), scale: 0.5)! == (167, 389))
        XCTAssertNil(FitDownscale.targetSize(photo: CGSize(width: 6000, height: 4000), scale: 1), "1:1 shows the photograph's own pixels")
        XCTAssertNil(FitDownscale.targetSize(photo: CGSize(width: 6000, height: 4000), scale: 1.5))
        XCTAssertNil(FitDownscale.targetSize(photo: CGSize(width: 6000, height: 4000), scale: 0.9995), "as good as 1:1")
        XCTAssertNil(FitDownscale.targetSize(photo: CGSize(width: 6000, height: 4000), scale: 0))
        XCTAssertNil(FitDownscale.targetSize(photo: CGSize(width: 1, height: 1), scale: 0.1), "nothing left")
    }

    /// The corner lands on a whole physical pixel, at most half a pixel from
    /// where it was, through a fractional offset, the zoom, a quarter turn,
    /// on Retina and standard displays.
    func testTheCornerSnapsToThePhysicalPixelGrid() {
        for backingScale in [1.0, 2.0] as [CGFloat] {
            for turns in 0..<4 {
                for (magnification, offset) in [(0.1833, CGPoint(x: 37.26, y: 11.71)), (0.25, CGPoint(x: 0.5, y: 0.25)), (0.3121, CGPoint(x: -3.333, y: 7.019))] as [(CGFloat, CGPoint)] {
                    // Layer coordinates → backing pixels, as the canvas's layers compose.
                    let toBacking = CGAffineTransform(rotationAngle: .pi / 2 * CGFloat(turns))
                        .concatenating(CGAffineTransform(scaleX: magnification * backingScale, y: magnification * backingScale))
                        .concatenating(CGAffineTransform(translationX: offset.x * backingScale, y: offset.y * backingScale))
                    let fromBacking = toBacking.inverted()
                    for corner in [CGPoint(x: -1234.5678, y: 91.3), CGPoint(x: 0, y: 0), CGPoint(x: 17.17, y: -4000.4)] {
                        let snapped = FitDownscale.snapped(corner, toBacking: { $0.applying(toBacking) }, fromBacking: { $0.applying(fromBacking) })
                        let pixel = snapped.applying(toBacking), before = corner.applying(toBacking)
                        XCTAssertEqual(pixel.x, pixel.x.rounded(), accuracy: 1e-6)
                        XCTAssertEqual(pixel.y, pixel.y.rounded(), accuracy: 1e-6)
                        XCTAssertLessThanOrEqual(abs(pixel.x - before.x), 0.5 + 1e-9)
                        XCTAssertLessThanOrEqual(abs(pixel.y - before.y), 0.5 + 1e-9)
                        // A half turn about a centre on a half pixel keeps the grid.
                        let centre = FitDownscale.snappedHalf(CGPoint(x: 501.37, y: 333.33), toBacking: { $0.applying(toBacking) }, fromBacking: { $0.applying(fromBacking) })
                        let c = centre.applying(toBacking)
                        XCTAssertEqual(c.x * 2, (c.x * 2).rounded(), accuracy: 1e-6)
                        XCTAssertEqual((2 * c.x - pixel.x), (2 * c.x - pixel.x).rounded(), accuracy: 1e-6)
                        XCTAssertEqual((2 * c.y - pixel.y), (2 * c.y - pixel.y).rounded(), accuracy: 1e-6)
                    }
                }
            }
        }
    }

    func testOnlySurfacesWithTheSRGBCurveResample() throws {
        XCTAssertTrue(FitDownscale.canResample(try surface(width: 8, height: 8, space: CGColorSpace.sRGB)))
        XCTAssertTrue(FitDownscale.canResample(try surface(width: 8, height: 8, space: CGColorSpace.itur_2020_sRGBGamma)))
        XCTAssertTrue(FitDownscale.canResample(try surface(width: 8, height: 8, space: CGColorSpace.displayP3)))
        XCTAssertFalse(FitDownscale.canResample(try surface(width: 8, height: 8, space: CGColorSpace.linearSRGB)))
        XCTAssertFalse(FitDownscale.canResample(try surface(width: 8, height: 8, space: nil)))
    }

    /// Lanczos in linear light: a fine black and white grating averages to
    /// linear mid-grey (188 in sRGB), not to the gamma average (128).
    func testTheResampleIsInLinearLight() throws {
        let source = try surface(width: 400, height: 300, space: CGColorSpace.sRGB) { x, _ in x % 2 == 0 ? 255 : 0 }
        guard let small = FitResampler.shared.resampleNow(source, width: 147, height: 110) else { throw XCTSkip("No Apple GPU") }
        XCTAssertEqual(small.width, 147); XCTAssertEqual(small.height, 110)
        XCTAssertEqual(EditorModel.displaySpace(of: small), "sRGB")
        let value = pixel(small, x: 70, y: 55)
        XCTAssertEqual(Double(value), 188, accuracy: 2)
        let wide = try surface(width: 400, height: 300, space: CGColorSpace.itur_2020_sRGBGamma) { _, _ in 90 }
        let tagged = try XCTUnwrap(FitResampler.shared.resampleNow(wide, width: 100, height: 75))
        XCTAssertEqual(EditorModel.displaySpace(of: tagged), "Rec. 2020 (sRGB transfer)", "tagged like its source")
        XCTAssertEqual(Double(pixel(tagged, x: 50, y: 30)), 90, accuracy: 1)
    }

    /// In a window, the fitted canvas swaps in the resample, 1:1 and on the
    /// pixel grid, and drops it at once for anything it doesn't match.
    func testTheFittedCanvasShowsTheResampleOnThePixelGrid() async throws {
        for (content, rotation, crop) in [(NSSize(width: 731, height: 517), 0, CropSettings()),
                                          (NSSize(width: 640, height: 481), 1, CropSettings()),
                                          (NSSize(width: 803, height: 600), 0, CropSettings(rect: CGRect(x: 0.1, y: 0.2, width: 0.63, height: 0.5)))] {
            let (window, scroll, view) = canvas(content)
            defer { window.close() }
            let image = try surface(width: 3000, height: 2000, space: CGColorSpace.sRGB) { x, y in UInt8((x * 7 + y * 3) % 256) }
            show(image, in: view, scroll: scroll, rotation: rotation, crop: crop)
            XCTAssertTrue(view.sharp.isHidden, "nothing until the GPU is done")
            let appeared = try await waitForSharp(view)
            XCTAssertTrue(appeared, "resampled")
            let scale = scroll.magnification * window.backingScaleFactor
            let target = try XCTUnwrap(FitDownscale.targetSize(photo: CGSize(width: 3000, height: 2000), scale: scale))
            let shown = try XCTUnwrap(view.sharp.contents as AnyObject?) as! IOSurface
            XCTAssertEqual(shown.width, target.width); XCTAssertEqual(shown.height, target.height)
            let pixels = backingRect(of: view.sharp, in: view)
            for value in [pixels.minX, pixels.minY, pixels.width, pixels.height] {
                XCTAssertEqual(value, value.rounded(), accuracy: 1e-3, "on the pixel grid at \(content), rotation \(rotation)")
            }
            XCTAssertEqual(Set([pixels.width.rounded(), pixels.height.rounded()]), Set([CGFloat(target.width), CGFloat(target.height)]), "1:1")
            // Within half a pixel of the photograph Core Animation would scale.
            let photo = backingRect(of: try XCTUnwrap(view.sharp.superlayer?.sublayers?.first), in: view)
            XCTAssertEqual(pixels.minX, photo.minX, accuracy: 0.5 + 1e-6); XCTAssertEqual(pixels.minY, photo.minY, accuracy: 0.5 + 1e-6)
            XCTAssertEqual(pixels.width, photo.width, accuracy: 0.5 + 1e-6); XCTAssertEqual(pixels.height, photo.height, accuracy: 0.5 + 1e-6)

            // Held upside down, it stays on the grid.
            view.turned = true
            let turned = backingRect(of: view.sharp, in: view, throughTurn: true)
            for value in [turned.minX, turned.minY] { XCTAssertEqual(value, value.rounded(), accuracy: 1e-3, "turned") }
            view.turned = false

            // Zoomed: Core Animation's own scaling, at once.
            scroll.fitting = false
            scroll.magnification *= 1.3
            XCTAssertTrue(view.sharp.isHidden, "zoomed")
            scroll.fitting = true; scroll.applyZoom()
            XCTAssertFalse(view.sharp.isHidden, "back to the fit: the kept resample")

            // A new picture never shows the old one's resample.
            let next = try surface(width: 3000, height: 2000, space: CGColorSpace.sRGB) { x, _ in UInt8(x % 256) }
            show(next, in: view, scroll: scroll, rotation: rotation, crop: crop)
            XCTAssertTrue(view.sharp.isHidden, "new picture")
            let resampled = try await waitForSharp(view)
            XCTAssertTrue(resampled)
            XCTAssertFalse((view.sharp.contents as AnyObject?) === shown)
            // Back to the first (Before and After): its resample is kept.
            show(image, in: view, scroll: scroll, rotation: rotation, crop: crop)
            XCTAssertFalse(view.sharp.isHidden, "kept")
            XCTAssertTrue((view.sharp.contents as AnyObject?) === shown)

            // A frame rendered at canvas size, or straightened: as before.
            view.settled = false
            XCTAssertTrue(view.sharp.isHidden, "drag frame")
            view.settled = true
            XCTAssertFalse(view.sharp.isHidden)
            var straightened = crop; straightened.angle = 2
            show(image, in: view, scroll: scroll, rotation: rotation, crop: straightened)
            XCTAssertTrue(view.sharp.isHidden, "straightened")
        }
    }

    /// The photograph's own layer is untouched: the canvas's other readers
    /// (the window snapshot tests, the crop and Original) still find it.
    func testThePhotographLayerKeepsItsImageAndFrame() async throws {
        let (window, scroll, view) = canvas(NSSize(width: 700, height: 500))
        defer { window.close() }
        let image = try surface(width: 3000, height: 2000, space: CGColorSpace.sRGB) { x, _ in UInt8(x % 256) }
        show(image, in: view, scroll: scroll, rotation: 0, crop: CropSettings())
        let photo = try XCTUnwrap(view.layer?.descendants.first { $0.contents as AnyObject === image })
        let frame = photo.convert(photo.bounds, to: view.layer)
        let appeared = try await waitForSharp(view)
        XCTAssertTrue(appeared, "resampled")
        XCTAssertTrue(photo.contents as AnyObject === image)
        XCTAssertEqual(photo.convert(photo.bounds, to: view.layer), frame)
        XCTAssertTrue(view.layer?.descendants.first { $0.contentsGravity == .resize && $0.contents != nil } === photo)
    }

    /// On screen: captures the real window before (Core Animation's trilinear
    /// scaling) and after (the resample) for an outside comparison with a
    /// Lanczos reference. Set NEGS_FIT_PROOF to a folder and
    /// NEGS_FIT_PROOF_IMAGE to an 8-bit sRGB photograph.
    func testCaptureFittedCanvasBeforeAndAfter() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let folder = environment["NEGS_FIT_PROOF"], let path = environment["NEGS_FIT_PROOF_IMAGE"] else {
            throw XCTSkip("Set NEGS_FIT_PROOF and NEGS_FIT_PROOF_IMAGE to capture the fitted canvas")
        }
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil))
        let cgImage = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        let image = try surface(width: cgImage.width, height: cgImage.height, space: CGColorSpace.sRGB)
        image.lock(options: [], seed: nil)
        let context = try XCTUnwrap(CGContext(data: image.baseAddress, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.bytesPerRow,
                                              space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                              bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue))
        context.interpolationQuality = .none
        context.draw(cgImage, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        image.unlock(options: [], seed: nil)
        var report: [[String: Any]] = []
        // Content sizes (points): the fitted width is 96 pt narrower.
        var cases: [(NSSize, Int, CropSettings)] = [(NSSize(width: 1196, height: 800), 0), (NSSize(width: 1001, height: 677), 0), (NSSize(width: 853, height: 611), 0),
                                      (NSSize(width: 846, height: 700), 0), (NSSize(width: 1240, height: 783), 0), (NSSize(width: 777, height: 1001), 1),
                                      (NSSize(width: 1500, height: 701), 0), (NSSize(width: 1333, height: 677), 0), (NSSize(width: 1100, height: 1001), 3)].map { ($0.0, $0.1, CropSettings()) }
        // Crops put the photograph's corner off the pixel grid.
        cases += [(NSSize(width: 1196, height: 800), 0, CropSettings(rect: CGRect(x: 0.1037, y: 0.0711, width: 0.8, height: 0.85))),
                  (NSSize(width: 1001, height: 900), 1, CropSettings(rect: CGRect(x: 0.2313, y: 0.1499, width: 0.6, height: 0.7)))]
        for (index, (content, rotation, crop)) in cases.enumerated() {
            let window = NSWindow(contentRect: NSRect(origin: CGPoint(x: 60, y: 60), size: content), styleMask: [.titled], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.colorSpace = .sRGB
            let scroll = ImageScrollView(frame: NSRect(origin: .zero, size: content))
            scroll.contentView = CenteringClipView()
            scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
            scroll.drawsBackground = true; scroll.backgroundColor = .black
            let view = PhotographView()
            scroll.documentView = view
            window.contentView = scroll
            window.makeKeyAndOrderFront(nil)
            show(image, in: view, scroll: scroll, rotation: rotation, crop: crop)
            let started = Date()
            let appeared = try await waitForSharp(view)
            XCTAssertTrue(appeared)
            let latency = Date().timeIntervalSince(started)
            try await Task.sleep(nanoseconds: 500_000_000)
            let name = "case\(index)"
            try await capture(window, to: "\(folder)/\(name)-after.png")
            let sharpRect = backingRect(of: view.sharp, in: view)
            let photoRect = backingRect(of: try XCTUnwrap(view.sharp.superlayer?.sublayers?.first), in: view)
            let clipRect = backingRect(of: try XCTUnwrap(view.sharp.superlayer), in: view)
            let target = (view.sharp.contents as AnyObject?).map { $0 as! IOSurface }
            view.settled = false
            XCTAssertTrue(view.sharp.isHidden)
            try await Task.sleep(nanoseconds: 500_000_000)
            try await capture(window, to: "\(folder)/\(name)-before.png")
            let frame = window.convertToBacking(CGRect(origin: .zero, size: window.frame.size))
            report.append(["name": name, "content": [content.width, content.height], "rotation": rotation, "backingScale": window.backingScaleFactor,
                           "magnification": scroll.magnification, "windowPixels": [frame.width, frame.height],
                           "sharp": [sharpRect.minX, sharpRect.minY, sharpRect.width, sharpRect.height],
                           "photo": [photoRect.minX, photoRect.minY, photoRect.width, photoRect.height],
                           "clip": [clipRect.minX, clipRect.minY, clipRect.width, clipRect.height],
                           "target": [target?.width ?? 0, target?.height ?? 0], "firstResampleSeconds": latency])
            window.close()
        }
        let json = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted])
        try json.write(to: URL(fileURLWithPath: "\(folder)/capture.json"))
    }

    /// The window's own pixels, wherever it is on screen (Stage Manager
    /// shrinks windows of a test runner).
    private func capture(_ window: NSWindow, to path: String) async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        let shared = try XCTUnwrap(content.windows.first { $0.windowID == CGWindowID(window.windowNumber) })
        let pixels = window.convertToBacking(CGRect(origin: .zero, size: window.frame.size))
        let configuration = SCStreamConfiguration()
        configuration.width = Int(pixels.width); configuration.height = Int(pixels.height)
        configuration.colorSpaceName = CGColorSpace.sRGB
        configuration.showsCursor = false
        configuration.ignoreShadowsSingleWindow = true
        configuration.captureResolution = .best
        let image = try await SCScreenshotManager.captureImage(contentFilter: SCContentFilter(desktopIndependentWindow: shared), configuration: configuration)
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }

    // MARK: Helpers

    private func canvas(_ content: NSSize) -> (NSWindow, ImageScrollView, PhotographView) {
        let window = NSWindow(contentRect: NSRect(origin: CGPoint(x: 100, y: 100), size: content), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let scroll = ImageScrollView(frame: NSRect(origin: .zero, size: content))
        scroll.contentView = CenteringClipView()
        scroll.allowsMagnification = true; scroll.minMagnification = 0.005; scroll.maxMagnification = 16
        let view = PhotographView()
        scroll.documentView = view
        window.contentView = scroll
        return (window, scroll, view)
    }

    private func show(_ image: IOSurface, in view: PhotographView, scroll: ImageScrollView, rotation: Int, crop: CropSettings) {
        let size = CGSize(width: image.width, height: image.height)
        view.show(image, size: size, rotation: rotation, crop: crop)
        view.setFrameSize(view.displaySize)
        scroll.fitting = true
        scroll.applyZoom()
        view.layoutSubtreeIfNeeded()
        scroll.layoutSubtreeIfNeeded()
    }

    private func waitForSharp(_ view: PhotographView) async throws -> Bool {
        let probe = try surface(width: 16, height: 16, space: CGColorSpace.sRGB)
        if FitResampler.shared.resampleNow(probe, width: 8, height: 8) == nil { throw XCTSkip("No Apple GPU") }
        let deadline = Date().addingTimeInterval(10)
        while view.sharp.isHidden, Date() < deadline { try await Task.sleep(nanoseconds: 10_000_000) }
        return !view.sharp.isHidden
    }

    /// A layer's rectangle in the window's backing pixels.
    private func backingRect(of layer: CALayer, in view: PhotographView, throughTurn: Bool = false) -> CGRect {
        let host = view.layer!
        let turn = host.sublayers!.first!
        let inView = throughTurn ? layer.convert(layer.bounds, to: host) : layer.convert(layer.bounds, to: turn)
        let inWindow = view.convert(inView, to: nil)
        return view.window!.convertToBacking(inWindow)
    }

    private func surface(width: Int, height: Int, space: CFString?, value: (Int, Int) -> UInt8 = { _, _ in 128 }) throws -> IOSurface {
        let surface = try XCTUnwrap(IOSurface(properties: [.width: width, .height: height, .bytesPerElement: 4,
                                                           .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
                                                           .pixelFormat: kCVPixelFormatType_32BGRA]))
        surface.lock(options: [], seed: nil)
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        for y in 0..<height {
            for x in 0..<width {
                let v = value(x, y), offset = y * surface.bytesPerRow + x * 4
                base[offset] = v; base[offset + 1] = v; base[offset + 2] = v; base[offset + 3] = 255
            }
        }
        surface.unlock(options: [], seed: nil)
        if let space { IOSurfaceSetValue(surface, kIOSurfaceColorSpace, CGColorSpace(name: space)!.copyPropertyList()!) }
        return surface
    }

    private func pixel(_ surface: IOSurface, x: Int, y: Int) -> UInt8 {
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        return surface.baseAddress.assumingMemoryBound(to: UInt8.self)[y * surface.bytesPerRow + x * 4 + 1]
    }
}
