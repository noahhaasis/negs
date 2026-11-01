import IOSurface
import XCTest
@testable import NegsApp

final class HistogramTests: XCTestCase {
    /// A BGRA surface whose left half is black and right half is white, with
    /// a red column down the middle.
    private func surface(width: Int = 100, height: Int = 60) throws -> IOSurface {
        let surface = try XCTUnwrap(IOSurface(properties: [
            .width: width, .height: height, .bytesPerElement: 4,
            .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
            .pixelFormat: kCVPixelFormatType_32BGRA
        ]))
        surface.lock(options: [], seed: nil)
        defer { surface.unlock(options: [], seed: nil) }
        let base = surface.baseAddress.assumingMemoryBound(to: UInt8.self)
        for y in 0..<height {
            for x in 0..<width {
                let pixel = base + y * surface.bytesPerRow + x * 4
                let (b, g, r): (UInt8, UInt8, UInt8) = x == width / 2 ? (0, 0, 200) : x < width / 2 ? (0, 0, 0) : (255, 255, 255)
                pixel[0] = b; pixel[1] = g; pixel[2] = r; pixel[3] = 255
            }
        }
        return surface
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

    func testWaveformPreservesHorizontalPositionAndRGBLevels() throws {
        let scopes = ImageScopes.of(try surface(), crop: CropSettings())
        let waveform = scopes.waveform
        XCTAssertEqual(waveform.width, 100)
        XCTAssertEqual(waveform.sampleCount, 6000)
        XCTAssertEqual(waveform.red[0], 60, "Black is at the bottom of the left column")
        XCTAssertEqual(waveform.red[200 * 100 + 50], 60, "The red column keeps its level and position")
        XCTAssertEqual(waveform.green[50], 60)
        XCTAssertEqual(waveform.blue[255 * 100 + 99], 60, "White is at the top of the right column")
        XCTAssertEqual(waveform.red.reduce(0, +), scopes.histogram.red.reduce(0, +))
        XCTAssertGreaterThan(waveform.rgba[(255 * 100) * 4], 0, "Raster draws black levels at the bottom")
        XCTAssertGreaterThan(waveform.rgba[99 * 4], 0, "Raster draws white levels at the top")
        XCTAssertNotNil(waveform.image)
    }

    func testWaveformUsesTheCroppedAndStraightenedFrame() throws {
        let image = try surface()
        let crop = CGRect(x: 0.5, y: 0, width: 0.5, height: 1)
        let waveform = ImageScopes.of(image, crop: CropSettings(rect: crop)).waveform
        XCTAssertEqual(waveform.width, 50)
        XCTAssertEqual(waveform.red[200 * 50], 60, "The red column moves to the cropped frame's left edge")
        XCTAssertEqual(waveform.red[255 * 50 + 49], 60)
        let turned = ImageScopes.of(image, crop: CropSettings(angle: 180)).waveform
        XCTAssertEqual(turned.red[255 * 100], 60, "Straightening also reverses waveform positions")
        XCTAssertEqual(turned.red[99], 60)
    }

    func testSampleBudgetAlsoBoundsThinPanoramas() throws {
        for (width, height) in [(10000, 1), (1, 10000), (101, 61)] {
            let scopes = ImageScopes.of(try surface(width: width, height: height), crop: CropSettings(), maxSamples: 127)
            XCTAssertLessThanOrEqual(scopes.waveform.sampleCount, 127)
            XCTAssertGreaterThan(scopes.waveform.sampleCount, 0)
            XCTAssertEqual(Int(scopes.histogram.red.reduce(0, +)), scopes.waveform.sampleCount)
            XCTAssertLessThanOrEqual(scopes.waveform.width, 256)
        }
    }

    func testWaveformFollowsCanvasQuarterTurns() throws {
        let image = try surface()
        let halfTurn = ImageScopes.of(image, crop: CropSettings(), rotation: 2).waveform
        XCTAssertEqual(halfTurn.red[255 * 100], 60)
        XCTAssertEqual(halfTurn.red[99], 60)
        for rotation in [1, 3, -1] {
            let waveform = ImageScopes.of(image, crop: CropSettings(), rotation: rotation).waveform
            XCTAssertEqual(waveform.width, 60)
            for x in 0..<60 {
                XCTAssertEqual(waveform.red[x], 50, "Every rotated column spans the original black half")
                XCTAssertEqual(waveform.red[200 * 60 + x], 1)
                XCTAssertEqual(waveform.red[255 * 60 + x], 49)
            }
        }
    }

    func testEmptySamplingProducesAnEmptyScopeWithoutAnImage() throws {
        let scopes = ImageScopes.of(try surface(), crop: CropSettings(), maxSamples: 0)
        XCTAssertEqual(scopes.histogram.peak, 0)
        XCTAssertEqual(scopes.waveform.sampleCount, 0)
        XCTAssertNil(scopes.waveform.image)
    }
}
