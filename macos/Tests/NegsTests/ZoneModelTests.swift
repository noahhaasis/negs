import AppKit
import XCTest
@testable import NegsApp

/// The system end to end, through the release helper: a pick, the measured
/// curve, and the placement a zone click applies.
@MainActor final class ZoneModelTests: XCTestCase {
    private var directory: URL!
    private var ramp: URL!
    private var other: URL!

    override func setUpWithError() throws {
        _ = NSApplication.shared
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("negs-zone-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        ramp = directory.appendingPathComponent("ramp.png")
        other = directory.appendingPathComponent("other.png")
        try writeGradient(to: ramp)
        try writeGradient(to: other, mirrored: true)
    }

    override func tearDownWithError() throws {
        if directory != nil { try? FileManager.default.removeItem(at: directory) }
    }

    /// A horizontal grey ramp in sRGB, the tones a pick can read.
    private func writeGradient(to url: URL, mirrored: Bool = false) throws {
        let (width, height) = (64, 48)
        let bitmap = try XCTUnwrap(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
            bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: width * 3, bitsPerPixel: 24
        ))
        let base = bitmap.bitmapData!
        for y in 0..<height {
            for x in 0..<width {
                let t = mirrored ? Double(width - 1 - x) : Double(x)
                let value = UInt8((t / Double(width - 1) * 255).rounded())
                let pixel = base + y * bitmap.bytesPerRow + x * 3
                pixel[0] = value; pixel[1] = value; pixel[2] = value
            }
        }
        try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: url)
    }

    private func paths() throws -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        let paths = EnginePaths(
            helper: root.appendingPathComponent("target/release/spektrafilm-native"),
            exporter: root.appendingPathComponent("target/release/spektrafilm-f64"),
            data: root.appendingPathComponent("data")
        )
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else {
            throw XCTSkip("Build the release helper first")
        }
        return paths
    }

    /// A settled model on the ramp, with grain off for stable measurements.
    private func makeModel(
        scanFilm: Bool = false,
        autoExposure: Bool = false,
        compensation: Bool = true
    ) async throws -> EditorModel {
        let model = EditorModel(previewCache: RenderedPreviewCache(
            file: directory.appendingPathComponent("preview-\(UUID().uuidString).cache")
        ))
        await model.start(paths: try paths())
        XCTAssertTrue(model.ready, model.error ?? "No engine")
        model.chooseFilm("kodak_portra_400")
        model.choosePaper("kodak_portra_endura")
        model.set("film_render.grain.active", .bool(false))
        model.set("camera.auto_exposure", .bool(autoExposure))
        model.set("camera.placement_ev", .number(0))
        model.set("enlarger.print_exposure_compensation", .bool(compensation))
        if scanFilm { model.set("io.scan_film", .bool(true)) }
        model.open(ramp)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        return model
    }

    private func waitUntil(_ predicate: () -> Bool, timeout: TimeInterval = 90) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(predicate(), "Timed out waiting for the engine")
    }

    private func pick(_ point: CGPoint = CGPoint(x: 0.5, y: 0.5), on model: EditorModel) async throws {
        model.pickTone(.small(point))
        try await waitUntil { model.zoneReadout != nil && model.toneCurve != nil }
    }
}

extension ZoneModelTests {
    func testPlacingLandsThePickOnTheZone() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pick(on: model)
        XCTAssertEqual(model.zoneReadout?.state, .ready)
        model.placeInZone(5, centre: false)
        try await waitUntil {
            model.previewCurrent && model.toneCurve != nil
                && (model.zoneReadout?.distances[5].map { abs($0) < 0.1 } ?? false)
        }
        XCTAssertEqual(model.zoneReadout!.distances[5]!, 0, accuracy: 0.1)
    }

    func testPlacingWorksWithPrintCompensationOnAndOff() async throws {
        for compensation in [true, false] {
            let model = try await makeModel(compensation: compensation)
            defer { model.shutdown() }
            try await pick(on: model)
            XCTAssertEqual(model.zoneReadout?.state, .ready, "compensation \(compensation)")
            model.placeInZone(4, centre: false)
            try await waitUntil {
                model.previewCurrent && model.toneCurve != nil
                    && (model.zoneReadout?.distances[4].map { abs($0) < 0.1 } ?? false)
            }
            XCTAssertEqual(model.zoneReadout!.distances[4]!, 0, accuracy: 0.1)
        }
    }

    func testPlacingWorksWithAutoExposureOn() async throws {
        let model = try await makeModel(autoExposure: true)
        defer { model.shutdown() }
        try await pick(on: model)
        XCTAssertEqual(model.zoneReadout?.state, .ready)
        model.placeInZone(5, centre: true)
        try await waitUntil {
            model.previewCurrent && model.toneCurve != nil
                && (model.zoneReadout?.distances[5].map { $0 < -0.1 } ?? false)
        }
        // The centre is half a zone above the start, so the pick lands at
        // minus half the zone's own height.
        let distances = model.zoneReadout!.distances
        let height = distances[6]! - distances[5]!
        XCTAssertEqual(distances[5]!, -height / 2, accuracy: 0.15)
    }

    func testPlacementIsOneUndoStep() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let undo = UndoManager()
        model.undoManager = undo
        try await pick(on: model)
        model.placeInZone(5, centre: false)
        try await waitUntil { model.previewCurrent && model.canPlaceZone && abs(model.value("camera.placement_ev").double) > 0.05 }
        let placed = model.value("camera.placement_ev").double
        XCTAssertEqual(undo.undoActionName, "Place in Zone")
        undo.undo()
        try await waitUntil { model.previewCurrent && model.toneCurve != nil }
        XCTAssertEqual(model.value("camera.placement_ev").double, 0, accuracy: 1e-9)
        XCTAssertNotEqual(placed, 0)
        // The readout describes the restored tone again.
        try await waitUntil { (model.zoneReadout?.distances[5].map { abs($0) > 0.05 }) ?? false }
    }

    func testZonesAreDisabledWhileRendering() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pick(on: model)
        XCTAssertTrue(model.canPlaceZone)
        model.set("camera.exposure_compensation_ev", .number(0.3))
        // The curve on screen describes the edit before this one.
        XCTAssertFalse(model.canPlaceZone)
        try await waitUntil { model.canPlaceZone }
    }

    func testStaleCurveIsDiscarded() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let middle = 49.0
        let base = try XCTUnwrap(model.toneCurve?.ev(forLStar: middle))
        model.set("camera.placement_ev", .number(1))
        try await waitUntil { model.previewCurrent && model.toneCurve != nil }
        model.set("camera.placement_ev", .number(2))
        model.set("camera.placement_ev", .number(3))
        // Placement moves the scene up the curve, so the same output tone is
        // reached three stops earlier.
        try await waitUntil {
            model.previewCurrent && model.toneCurve != nil
                && abs((model.toneCurve?.ev(forLStar: middle) ?? .nan) - (base - 3)) < 0.3
        }
        XCTAssertEqual(model.toneCurve!.ev(forLStar: middle)!, base - 3, accuracy: 0.3)
    }

    func testNegativeScanIsUnavailable() async throws {
        let model = try await makeModel(scanFilm: true)
        defer { model.shutdown() }
        model.pickTone(.small(CGPoint(x: 0.5, y: 0.5)))
        try await waitUntil { model.zoneReadout != nil }
        XCTAssertEqual(model.zoneReadout?.state, .unavailable)
        XCTAssertFalse(model.canPlaceZone)
    }
}


extension ZoneModelTests {
    func testNoPickFromOriginal() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pick(on: model)
        let picked = model.tonePick
        model.holdOriginal(true)
        try await waitUntil { model.original != nil }
        model.pickTone(.large(CGPoint(x: 0.9, y: 0.1)))
        XCTAssertEqual(model.tonePick, picked, "the unprocessed photograph has no print to place")
        model.holdOriginal(false)
    }

    func testNewPhotographClearsThePick() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await pick(on: model)
        model.setSelection(CropSettings(rect: CGRect(x: 0.2, y: 0.2, width: 0.4, height: 0.4)), picking: true)
        model.open(other)
        XCTAssertNil(model.tonePick)
        XCTAssertNil(model.toneSample)
        XCTAssertNil(model.zoneReadout)
        XCTAssertNil(model.selection)
    }

    func testEditsKeepThePick() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let point = CGPoint(x: 0.4, y: 0.6)
        try await pick(point, on: model)
        XCTAssertEqual(model.tonePick, .small(point))
        model.set("camera.exposure_compensation_ev", .number(0.2))
        try await waitUntil { model.previewCurrent && model.zoneReadout != nil }
        XCTAssertEqual(model.tonePick, .small(point))
        XCTAssertNotNil(model.toneSample)
    }

    func testPlacementClampsAtTheSliderRange() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.set("camera.placement_ev", .number(7.9))
        try await waitUntil { model.previewCurrent && model.toneCurve != nil }
        // A dark tone, so it is still mid-print with the placement pushed up.
        try await pick(CGPoint(x: 0.03, y: 0.5), on: model)
        XCTAssertEqual(model.zoneReadout?.state, .ready)
        model.placeInZone(7, centre: false)
        try await waitUntil { model.previewCurrent && model.value("camera.placement_ev").double == 8 }
        XCTAssertEqual(model.value("camera.placement_ev").double, 8, accuracy: 1e-9)
        XCTAssertTrue(model.status.contains("±8"), model.status)
    }

    func testSelectionLimitsTheHistogram() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        try await waitUntil { model.histogram != nil }
        let whole = try XCTUnwrap(model.histogram).red.lastIndex { $0 > 0 }!
        XCTAssertGreaterThan(whole, 200, "the ramp reaches paper white")
        model.setSelection(CropSettings(rect: CGRect(x: 0, y: 0, width: 0.5, height: 1)), picking: false)
        // The left half of the ramp is all darker than the right half.
        try await waitUntil { (model.histogram?.red.lastIndex { $0 > 0 } ?? whole) < whole - 20 }
        // And no selection counts the whole crop again.
        model.setSelection(nil, picking: false)
        try await waitUntil { (model.histogram?.red.lastIndex { $0 > 0 } ?? 0) >= whole - 1 }
    }

    func testSelectionSurvivesAnEdit() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let region = CropSettings(rect: CGRect(x: 0.1, y: 0.1, width: 0.5, height: 0.5))
        model.setSelection(region, picking: false)
        model.set("camera.exposure_compensation_ev", .number(0.2))
        try await waitUntil { model.previewCurrent && model.histogram != nil }
        XCTAssertEqual(model.selection, region)
    }

    func testNewPhotographOrCropClearsTheSelection() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let region = CropSettings(rect: CGRect(x: 0.1, y: 0.1, width: 0.5, height: 0.5))
        model.setSelection(region, picking: false)
        XCTAssertEqual(model.selection, region)
        model.beginCrop()
        model.cropSession?.rect = CGRect(x: 0.2, y: 0.2, width: 0.6, height: 0.6)
        model.endCrop(apply: true)
        XCTAssertNil(model.selection)
        model.setSelection(region, picking: false)
        model.open(other)
        XCTAssertNil(model.selection)
    }

    func testPlainSelectionLeavesThePickAlone() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let point = CGPoint(x: 0.5, y: 0.5)
        try await pick(point, on: model)
        model.setSelection(CropSettings(rect: CGRect(x: 0.2, y: 0.2, width: 0.3, height: 0.3)), picking: false)
        XCTAssertEqual(model.tonePick, .small(point), "the gold numbers keep describing the last ⌃ pick")
    }

    func testControlSelectionMovesThePick() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        let region = CropSettings(rect: CGRect(x: 0.2, y: 0.2, width: 0.3, height: 0.3))
        model.setSelection(region, picking: true)
        XCTAssertEqual(model.tonePick, .region(region))
        try await waitUntil { model.toneSample != nil }
        XCTAssertNotNil(model.toneSample)
    }

    func testNoSelectionFromOriginal() async throws {
        let model = try await makeModel()
        defer { model.shutdown() }
        model.holdOriginal(true)
        try await waitUntil { model.original != nil }
        model.setSelection(CropSettings(rect: CGRect(x: 0, y: 0, width: 0.5, height: 1)), picking: true)
        XCTAssertNil(model.selection)
        model.holdOriginal(false)
    }
}

