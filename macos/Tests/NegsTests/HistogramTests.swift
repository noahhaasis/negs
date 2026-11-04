import IOSurface
import XCTest
@testable import NegsApp

final class HistogramTests: XCTestCase {
    /// A BGRA surface from a per-pixel colour function, tagged with its
    /// display space like the helper's previews.
    private func surface(
        width: Int = 100, height: Int = 60, displaySpace: String = "sRGB",
        _ pixel: (Int, Int) -> (r: UInt8, g: UInt8, b: UInt8)
    ) throws -> IOSurface {
        let surface = try XCTUnwrap(IOSurface(properties: [
            .width: width, .height: height, .bytesPerElement: 4,
            .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
            .pixelFormat: kCVPixelFormatType_32BGRA
        ]))
        surface.lock(options: [], seed: nil)
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        for y in 0..<height {
            for x in 0..<width {
                let (r, g, b) = pixel(x, y)
                let p = base + y * surface.bytesPerRow + x * 4
                p[0] = b; p[1] = g; p[2] = r; p[3] = 255
            }
        }
        surface.unlock(options: [], seed: nil)
        if displaySpace != "sRGB",
           let space = EditorModel.displayColorSpace(displaySpace)?.copyPropertyList() {
            IOSurfaceSetValue(surface, kIOSurfaceColorSpace, space)
        }
        return surface
    }

    /// A BGRA surface whose left half is black and right half is white, with
    /// a red column down the middle.
    private func surface(width: Int = 100, height: Int = 60) throws -> IOSurface {
        try surface(width: width, height: height) { x, _ in
            x == width / 2 ? (200, 0, 0) : x < width / 2 ? (0, 0, 0) : (255, 255, 255)
        }
    }

    func testCountsEveryPixelOfTheCrop() throws {
        let image = try surface()
        let whole = Histogram.of(image, crop: CropSettings())
        XCTAssertEqual(whole.red.reduce(0, +), 6000)
        // Bins are L*: black is 0, the red column's 200 code is L* 80.6, white is 255.
        XCTAssertEqual(whole.red[0], 50 * 60)
        XCTAssertEqual(whole.red[205], 60)
        XCTAssertEqual(whole.red[255], 49 * 60)
        XCTAssertEqual(whole.green[0], 51 * 60)
        XCTAssertEqual(whole.blue[255], 49 * 60)
        // The left 40%: only black.
        let left = Histogram.of(image, crop: CropSettings(rect: CGRect(x: 0, y: 0, width: 0.4, height: 1)))
        XCTAssertEqual(left.red[0], 40 * 60)
        XCTAssertEqual(left.red.reduce(0, +), 40 * 60)
    }

    func testSamplesLargeCropsEvenly() throws {
        let sampled = Histogram.of(try surface(), crop: CropSettings(), maxSamples: 1500)
        let total = Double(sampled.red.reduce(0, +))
        XCTAssertLessThanOrEqual(total, 1500)
        XCTAssertEqual(Double(sampled.red[0]) / total, 0.5, accuracy: 0.05)
        XCTAssertEqual(Double(sampled.red[255]) / total, 0.49, accuracy: 0.05)
    }

    /// A straightened crop reads the turned photograph: half a turn about
    /// the centre swaps the black and white halves.
    func testStraightenedCropTurnsBackToThePhotograph() throws {
        let image = try surface()
        let left = CGRect(x: 0, y: 0.25, width: 0.4, height: 0.5)
        let turned = Histogram.of(image, crop: CropSettings(rect: left, angle: 180))
        XCTAssertEqual(turned.red[255], turned.red.reduce(0, +))
        XCTAssertEqual(Histogram.of(image, crop: CropSettings(rect: left)).red[0], 40 * 30)
    }

    func testPeakIgnoresClippedEnds() {
        var histogram = Histogram()
        histogram.red[0] = 1000; histogram.green[128] = 40; histogram.blue[255] = 900
        XCTAssertEqual(ToneHistogram(histogram).peak, 40)
        XCTAssertEqual(ToneHistogram(Histogram()).peak, 0)
        var clipped = Histogram(); clipped.red[255] = 7
        XCTAssertEqual(ToneHistogram(clipped).peak, 7)
    }

    /// Bins 0–4 and 251–255 don't set the scale either, so a spike at the
    /// scanner's black or paper white can run off the top.
    func testPeakIgnoresBinsZeroToFourAnd251Up() throws {
        var histogram = Histogram()
        histogram.red[3] = 5000; histogram.green[252] = 5000; histogram.blue[128] = 40
        XCTAssertEqual(ToneHistogram(histogram).peak, 40)
        // The neighbours of the excluded ends still count.
        histogram.blue[5] = 50
        XCTAssertEqual(ToneHistogram(histogram).peak, 50)
        histogram.red[250] = 60
        XCTAssertEqual(ToneHistogram(histogram).peak, 60)
        // A flat clipped frame still gets a scale, as before.
        var ends = Histogram(); ends.red[3] = 5000; ends.green[254] = 4000
        XCTAssertEqual(ToneHistogram(ends).peak, 5000)
    }

    // MARK: Lightness and channels

    func testLightnessBinsAFlatGrey() throws {
        // sRGB 119 is L* 50.0, and L* × 2.55 = 127.6, binned by truncation.
        let histogram = Histogram.of(try surface(width: 8, height: 8) { _, _ in (119, 119, 119) }, crop: CropSettings())
        XCTAssertEqual(histogram.lightness[127], 64)
        XCTAssertEqual(histogram.lightness.reduce(0, +), 64)
        // A channel value of 119 is the same tone, so it bins the same.
        XCTAssertEqual(histogram.red[127], 64)
        XCTAssertEqual(histogram.green[127], 64)
        XCTAssertEqual(histogram.blue[127], 64)
        // Lightness never sets the graph's vertical scale.
        XCTAssertEqual(ToneHistogram(histogram).peak, 64)
    }

    /// Each channel is read as a tone of its own: a saturated red's red
    /// channel bins near white, its green and blue channels at black.
    func testChannelBinsAreTheChannelsOwnLightness() throws {
        let image = try surface(width: 6, height: 6) { _, _ in (255, 0, 0) }
        let histogram = Histogram.of(image, crop: CropSettings())
        XCTAssertEqual(histogram.red[255], 36)
        XCTAssertEqual(histogram.green[0], 36)
        XCTAssertEqual(histogram.blue[0], 36)
        // Its luminance is sRGB red's: L* 53.2, binned at 135.
        XCTAssertEqual(histogram.lightness[135], 36)
    }

    /// The same bytes mean different luminance under different tags, and L*\*
    /// follows the proxy's space.
    func testLightnessUsesTheProxysLuminance() throws {
        let green = { (_: Int, _: Int) in (r: UInt8(0), g: UInt8(255), b: UInt8(0)) }
        let srgb = Histogram.of(try surface(width: 4, height: 4) { green($0, $1) }, crop: CropSettings())
        let wide = Histogram.of(
            try surface(width: 4, height: 4, displaySpace: "Rec. 2020 (sRGB transfer)") { green($0, $1) },
            crop: CropSettings()
        )
        let srgbBin = srgb.lightness.firstIndex { $0 > 0 }!
        let wideBin = wide.lightness.firstIndex { $0 > 0 }!
        // sRGB green's Y is 0.7152, Rec. 2020's 0.6780: about four bins.
        XCTAssertEqual(srgbBin, 223, accuracy: 1)
        XCTAssertEqual(wideBin, 219, accuracy: 1)
        XCTAssertGreaterThan(srgbBin, wideBin)
        // The channel bars are the channel's own tone in both.
        XCTAssertEqual(srgb.green[255], 16)
        XCTAssertEqual(wide.green[255], 16)
    }

    // MARK: Sampling a circle

    /// L\* of an sRGB code, as the sampler measures it.
    private func lstar(_ code: UInt8) -> Double {
        let v = Double(code) / 255
        let y = v <= 0.040_45 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
        return y > 216.0 / 24389.0 ? 116 * cbrt(y) - 16 : y * 24389.0 / 27.0
    }

    func testATinyCircleSamplesThePixelUnderIt() throws {
        let image = try surface(width: 11, height: 11) { x, y in
            x == 5 && y == 5 ? (200, 200, 200) : (100, 100, 100)
        }
        let pixel = try XCTUnwrap(
            TonePick.circle(centre: CGPoint(x: 5.5 / 11, y: 5.5 / 11), radius: 0).sample(image)
        )
        XCTAssertEqual(pixel.lstar, lstar(200), accuracy: 1e-3)
        let flat = try XCTUnwrap(
            TonePick.circle(centre: CGPoint(x: 0.1, y: 0.1), radius: 0).sample(image)
        )
        XCTAssertEqual(flat.lstar, lstar(100), accuracy: 1e-3)
    }

    /// The drag's radius averages everything inside it: a circle over the
    /// black and white halves lands between them.
    func testACircleAveragesItsArea() throws {
        let image = try surface()
        let left = try XCTUnwrap(
            TonePick.circle(centre: CGPoint(x: 0.25, y: 0.5), radius: 0).sample(image)
        )
        XCTAssertEqual(left.lstar, 0, accuracy: 1e-9, "the black half")
        let both = try XCTUnwrap(
            TonePick.circle(centre: CGPoint(x: 0.5, y: 0.5), radius: 0.3).sample(image)
        )
        XCTAssertGreaterThan(both.lstar, 5)
        XCTAssertLessThan(both.lstar, 95)
    }

    func testACircleCentredOutsideTheImageIsNil() throws {
        let image = try surface(width: 10, height: 10) { _, _ in (128, 128, 128) }
        XCTAssertNil(TonePick.circle(centre: CGPoint(x: 1.5, y: 0.5), radius: 0).sample(image))
        XCTAssertNil(TonePick.circle(centre: CGPoint(x: 0.5, y: -0.2), radius: 0).sample(image))
    }

    func testASmallerPreviewSamplesTheSameTone() throws {
        let flat = { (_: Int, _: Int) in (r: UInt8(128), g: UInt8(128), b: UInt8(128)) }
        let full = try XCTUnwrap(
            TonePick.circle(centre: CGPoint(x: 0.5, y: 0.5), radius: 0.2)
                .sample(try surface(width: 20, height: 20) { flat($0, $1) })
        )
        let half = try XCTUnwrap(
            TonePick.circle(centre: CGPoint(x: 0.5, y: 0.5), radius: 0.2)
                .sample(try surface(width: 10, height: 10) { flat($0, $1) })
        )
        XCTAssertEqual(full.lstar, half.lstar, accuracy: 1e-6)
        XCTAssertEqual(full.lstar, lstar(128), accuracy: 1e-3)
    }
}
