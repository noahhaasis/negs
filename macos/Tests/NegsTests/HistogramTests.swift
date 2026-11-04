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
        XCTAssertEqual(whole.red[0], 50 * 60); XCTAssertEqual(whole.red[200], 60); XCTAssertEqual(whole.red[255], 49 * 60)
        XCTAssertEqual(whole.green[0], 51 * 60); XCTAssertEqual(whole.blue[255], 49 * 60)
        // The left 40%: only black.
        let left = Histogram.of(image, crop: CropSettings(rect: CGRect(x: 0, y: 0, width: 0.4, height: 1)))
        XCTAssertEqual(left.red[0], 40 * 60); XCTAssertEqual(left.red.reduce(0, +), 40 * 60)
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
        XCTAssertEqual(histogram.peak, 40)
        XCTAssertEqual(Histogram().peak, 0)
        var clipped = Histogram(); clipped.red[255] = 7
        XCTAssertEqual(clipped.peak, 7)
    }

    /// Bins 0–4 and 251–255 don't set the scale either, so a spike at the
    /// scanner's black or paper white can run off the top.
    func testPeakIgnoresBinsZeroToFourAnd251Up() throws {
        var histogram = Histogram()
        histogram.red[3] = 5000; histogram.green[252] = 5000; histogram.blue[128] = 40
        XCTAssertEqual(histogram.peak, 40)
        // The neighbours of the excluded ends still count.
        histogram.blue[5] = 50
        XCTAssertEqual(histogram.peak, 50)
        histogram.red[250] = 60
        XCTAssertEqual(histogram.peak, 60)
        // A flat clipped frame still gets a scale, as before.
        var ends = Histogram(); ends.red[3] = 5000; ends.green[254] = 4000
        XCTAssertEqual(ends.peak, 5000)
    }

    // MARK: Lightness

    func testLightnessBinsAFlatGrey() throws {
        // sRGB 119 is L* 50.04, and L* × 2.55 = 127.6, binned by truncation.
        let histogram = Histogram.of(try surface(width: 8, height: 8) { _, _ in (119, 119, 119) }, crop: CropSettings())
        XCTAssertEqual(histogram.lightness[127], 64)
        XCTAssertEqual(histogram.lightness.reduce(0, +), 64)
        XCTAssertEqual(histogram.red[119], 64)
        // Lightness never sets the graph's vertical scale.
        XCTAssertEqual(histogram.peak, 64)
    }

    /// The same bytes mean different luminance under different tags, and L\*
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
        // The bars themselves are the proxy's codes in both.
        XCTAssertEqual(srgb.green[255], 16)
        XCTAssertEqual(wide.green[255], 16)
    }

    // MARK: Picks

    func testSampleAveragesTheWindow() throws {
        // One bright pixel in a flat field: the wider window averages it down.
        let image = try surface(width: 11, height: 11) { x, y in
            x == 5 && y == 5 ? (200, 200, 200) : (100, 100, 100)
        }
        let point = CGPoint(x: 5.5 / 11, y: 5.5 / 11)
        let small = try XCTUnwrap(TonePick.small(point).sample(image, output: .identity))
        let large = try XCTUnwrap(TonePick.large(point).sample(image, output: .identity))
        XCTAssertEqual(small.red, (8 * 100 + 200) / 9, accuracy: 1e-9)
        XCTAssertEqual(large.red, (48 * 100 + 200) / 49, accuracy: 1e-9)
        XCTAssertEqual(small.green, small.red)
        XCTAssertEqual(small.blue, small.red)
        // L* follows the same average of the picked pixels.
        XCTAssertGreaterThan(small.lstar, large.lstar)
    }

    func testPickSurvivesASmallerInteractiveSurface() throws {
        let flat = { (_: Int, _: Int) in (r: UInt8(128), g: UInt8(128), b: UInt8(128)) }
        let full = try XCTUnwrap(
            TonePick.small(CGPoint(x: 0.5, y: 0.5)).sample(try surface(width: 20, height: 20) { flat($0, $1) }, output: .identity)
        )
        let half = try XCTUnwrap(
            TonePick.small(CGPoint(x: 0.5, y: 0.5)).sample(try surface(width: 10, height: 10) { flat($0, $1) }, output: .identity)
        )
        XCTAssertEqual(full, half)
        XCTAssertEqual(full.red, 128, accuracy: 1e-9)
    }

    func testSampleClipsTheWindowAtTheEdge() throws {
        let image = try surface(width: 11, height: 11) { x, y in
            x == 0 && y == 0 ? (200, 200, 200) : (100, 100, 100)
        }
        let corner = try XCTUnwrap(TonePick.small(CGPoint(x: 0, y: 0)).sample(image, output: .identity))
        // Only the four in-image pixels of the 3×3 window.
        XCTAssertEqual(corner.red, 125, accuracy: 1e-9)
    }

    func testSampleOutsideTheImageIsNil() throws {
        let image = try surface(width: 10, height: 10) { _, _ in (128, 128, 128) }
        XCTAssertNil(TonePick.small(CGPoint(x: 1.5, y: 0.5)).sample(image, output: .identity))
        XCTAssertNil(TonePick.small(CGPoint(x: 0.5, y: -0.2)).sample(image, output: .identity))
        XCTAssertNil(TonePick.large(CGPoint(x: 2, y: 2)).sample(image, output: .identity))
    }


    func testRegionSampleAveragesTheRectangle() throws {
        let image = try surface()
        let left = CropSettings(rect: CGRect(x: 0, y: 0, width: 0.5, height: 1))
        let sample = try XCTUnwrap(TonePick.region(left).sample(image, output: .identity))
        XCTAssertEqual(sample.red, 0, accuracy: 1e-9)
        let all = try XCTUnwrap(TonePick.region(CropSettings()).sample(image, output: .identity))
        // 50 black columns, the red one, then 49 white: the red average.
        let expected = (50.0 * 0 + 200 + 49.0 * 255) / 100
        XCTAssertEqual(all.red, expected, accuracy: 1.5)
    }

    func testRegionCountsOnlyTheSelection() throws {
        let image = try surface()
        let left = CropSettings(rect: CGRect(x: 0, y: 0, width: 0.5, height: 1))
        let counted = Histogram.of(image, crop: left)
        XCTAssertEqual(counted.red[0], counted.red.reduce(0, +))
        XCTAssertEqual(counted.red[0], 50 * 60)
        // No selection means the whole crop, as the model passes it.
        XCTAssertEqual(Histogram.of(image, crop: CropSettings()).red.reduce(0, +), 6000)
    }

    func testRegionOnAStraightenedPhotoReadsWhatIsShown() throws {
        let image = try surface()
        let left = CGRect(x: 0, y: 0.25, width: 0.4, height: 0.5)
        let sample = try XCTUnwrap(
            TonePick.region(CropSettings(rect: left, angle: 180)).sample(image, output: .identity)
        )
        XCTAssertEqual(sample.red, 255, accuracy: 1e-9, "a half turn shows the white half")
    }

    // MARK: The output space

    private func mapping(_ rows: [[Double]], encode: (Double) -> Double) -> OutputMapping {
        let table = (0..<1024).map { encode(pow(Double($0) / 1023, 2)) }
        return OutputMapping(
            matrix: .array(rows.map { .array($0.map(JSONValue.number)) }),
            encode: .array(table.map(JSONValue.number))
        )!
    }

    /// A Rec. 2020 proxy is not the output space: its bars pass through the
    /// render's matrix before they are binned.
    func testBarsAreInTheOutputSpaceOnAWideProxy() throws {
        // Linear Rec. 2020 (0.5, 0.2, 0.1), which the sRGB transfer encodes.
        let bytes = [0.5, 0.2, 0.1].map { UInt8((OutputMapping.srgbEncode($0) * 255).rounded()) }
        let image = try surface(width: 6, height: 6, displaySpace: "Rec. 2020 (sRGB transfer)") { _, _ in
            (bytes[0], bytes[1], bytes[2])
        }
        // An output space that swaps red and blue, with linear encoding.
        let swapped = mapping([[0, 0, 1], [0, 1, 0], [1, 0, 0]], encode: { $0 })
        let mapped = Histogram.of(image, crop: CropSettings(), output: swapped)
        XCTAssertEqual(mapped.red.reduce(0, +), 36)
        XCTAssertEqual(mapped.red[25...27].reduce(0, +), 36, "blue's code becomes red's")
        XCTAssertEqual(mapped.blue[127...129].reduce(0, +), 36, "red's code becomes blue's")
        XCTAssertEqual(mapped.green[50...52].reduce(0, +), 36, "green keeps its code")
        // Without the mapping the proxy's own codes are counted.
        let plain = Histogram.of(image, crop: CropSettings())
        XCTAssertEqual(plain.red[Int(bytes[0])], 36)
        XCTAssertEqual(plain.blue[Int(bytes[2])], 36)
        // And the pick reads the same output-space codes.
        let sample = try XCTUnwrap(TonePick.small(CGPoint(x: 0.5, y: 0.5)).sample(image, output: swapped))
        XCTAssertEqual(sample.red, 25.5, accuracy: 1)
        XCTAssertEqual(sample.blue, 127.5, accuracy: 1)
    }

    func testIdentityMappingRoundTripsTheProxyBytes() throws {
        let image = try surface(width: 4, height: 4) { x, _ in (UInt8(x * 60), UInt8(x * 40), 20) }
        let histogram = Histogram.of(image, crop: CropSettings(), output: .identity)
        XCTAssertEqual(histogram.red[0], 4)
        XCTAssertEqual(histogram.red[60], 4)
        XCTAssertEqual(histogram.red[180], 4)
        let sample = try XCTUnwrap(TonePick.small(CGPoint(x: 2.5 / 4, y: 0.5)).sample(image, output: .identity))
        XCTAssertEqual(sample.red, 120, accuracy: 1e-9)
        XCTAssertEqual(sample.green, 80, accuracy: 1e-9)
        XCTAssertEqual(sample.blue, 20, accuracy: 1e-9)
    }
}

