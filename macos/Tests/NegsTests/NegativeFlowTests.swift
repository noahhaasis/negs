import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

/// Synthetic scans of a Portra 400 negative, made here so nothing borrowed
/// is committed: each pixel is the scan of a film density the film can
/// hold (`FilmConverter::forward`, kodak_portra_400 at default settings,
/// worked out once), as linear RGB.
enum SyntheticNegative {
    /// The scan of clear film: the orange base.
    static let clear: [Double] = [0.706165, 0.236752, 0.143739]
    /// Scans of 27 densities, cyan, magenta and yellow each 0.3, 0.6 or 0.9.
    static let palette: [[Double]] = [
        [0.415618, 0.114352, 0.067519], [0.413750, 0.113307, 0.025822], [0.411644, 0.111855, 0.005096],
        [0.374901, 0.053249, 0.071111], [0.372921, 0.053146, 0.030190], [0.370931, 0.052426, 0.010018],
        [0.338250, 0.022854, 0.072495], [0.336185, 0.023434, 0.032151], [0.334279, 0.023244, 0.012397],
        [0.276594, 0.116903, 0.068603], [0.275545, 0.115941, 0.027305], [0.274260, 0.114564, 0.006831],
        [0.246983, 0.056582, 0.072025], [0.245731, 0.056527, 0.031472], [0.244467, 0.055851, 0.011533],
        [0.219771, 0.026794, 0.073260], [0.218356, 0.027397, 0.033256], [0.217094, 0.027230, 0.013719],
        [0.189534, 0.117028, 0.069097], [0.189049, 0.116150, 0.028164], [0.188335, 0.114847, 0.007921],
        [0.168065, 0.057386, 0.072414], [0.167313, 0.057381, 0.032193], [0.166550, 0.056751, 0.012468],
        [0.147691, 0.028145, 0.073551], [0.146721, 0.028775, 0.033854], [0.145901, 0.028634, 0.014514],
    ]

    /// A scan `width` × `height`: clear film down the left `rebate` of it
    /// (none for 0), the picture a scatter of the palette, `light` times as
    /// bright as a scan at 1, `shift` moving the scatter for another picture.
    /// A 16-bit sRGB TIFF, with the camera's shutter, aperture and ISO.
    static func write(to url: URL, width: Int = 480, height: Int = 320, rebate: Double = 0.12, light: Double = 0.6, shift: Int = 0,
                      exposure: Double = 1.0 / 50, aperture: Double = 8, iso: Double = 100) throws {
        var pixels = [UInt16](repeating: 0, count: width * height * 3)
        let clearEnd = Int(rebate * Double(width))
        func encode(_ v: Double) -> UInt16 {
            let c = min(max(v, 0), 1)
            let e = c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1 / 2.4) - 0.055
            return UInt16((e * 65535).rounded())
        }
        for y in 0..<height {
            for x in 0..<width {
                var h = UInt32(truncatingIfNeeded: x &* 374_761_393) ^ UInt32(truncatingIfNeeded: (y + shift) &* 668_265_263)
                h = (h ^ (h >> 13)) &* 1_274_126_177
                h ^= h >> 16
                let rgb = x < clearEnd ? clear : palette[Int(h % UInt32(palette.count))]
                for c in 0..<3 { pixels[(y * width + x) * 3 + c] = encode(rgb[c] * light) }
            }
        }
        let data = pixels.withUnsafeBufferPointer { Data(buffer: $0) }
        let provider = try XCTUnwrap(CGDataProvider(data: data as CFData))
        let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 16, bitsPerPixel: 48, bytesPerRow: width * 6,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue | CGBitmapInfo.byteOrder16Little.rawValue),
                                          provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.tiff.identifier as CFString, 1, nil))
        let exif: [CFString: Any] = [kCGImagePropertyExifExposureTime: exposure, kCGImagePropertyExifFNumber: aperture,
                                     kCGImagePropertyExifISOSpeedRatings: [Int(iso)]]
        CGImageDestinationAddImage(destination, image, [kCGImagePropertyExifDictionary: exif] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }
}

/// Your negative's finished flow (ticket 33 §9): reading a negative, the
/// crop to its picture, picks, the warning, presets, the histogram and a
/// darkroom roll, through the model and the release helper.
@MainActor final class NegativeFlowTests: XCTestCase {
    private var paths: EnginePaths {
        EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"),
                    exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func folder() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-negative-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func waitUntil(_ message: String = "", seconds: TimeInterval = 60, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Timed out: \(message)")
    }

    /// A model on the release helper; `store` saves sidecars (a roll needs them).
    private func model(store: SidecarStore? = nil, presets: PresetLibrary = PresetLibrary()) async throws -> EditorModel {
        guard FileManager.default.isExecutableFile(atPath: paths.helper.path) else { throw XCTSkip("Build the release helper first") }
        let defaults = UserDefaults.standard
        let cleared = ["advanceAfterRating", LibraryFilter.defaultsKey, EditorModel.newPhotoFilmKey, EditorModel.newPhotoPaperKey]
        let saved = (["autoPlaceNewImages", "sidecars"] + cleared).map { ($0, defaults.object(forKey: $0)) }
        addTeardownBlock { for (key, value) in saved { defaults.set(value, forKey: key) } }
        defaults.set(false, forKey: "autoPlaceNewImages")
        defaults.set(store != nil, forKey: "sidecars")
        for key in cleared { defaults.removeObject(forKey: key) }
        let model = store.map { EditorModel(presetLibrary: presets, sidecars: $0) } ?? EditorModel(presetLibrary: presets)
        addTeardownBlock { @MainActor in model.shutdown() }
        await model.start(paths: paths, restoration: Task { nil })
        return model
    }

    /// Opens `photo` and reads it as a negative: the base found, the print
    /// balanced and metered (one step), then the crop to its picture (one
    /// step of its own, if it has one).
    private func readNegative(_ photo: URL, in model: EditorModel, undo: UndoManager, picture: Bool = true) async throws {
        model.open(photo)
        try await waitUntil("the photograph") { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        undo.beginUndoGrouping()
        model.chooseNegative()
        undo.endUndoGrouping()
        undo.beginUndoGrouping()
        try await waitUntil("the base") { model.negativeBase != nil && model.negativeWorking == nil && (!picture || model.state.crop.cropped) }
        undo.endUndoGrouping()
    }

    private func base(_ state: EditState) -> [Double] { ["cyan", "magenta", "yellow"].map { state.params["film_render.base.\($0)"].double } }

    /// The print exposure the helper meters for `state` on `photo`, inside `bounds`.
    private func metered(_ state: EditState, photo: URL, bounds: JSONValue?) async throws -> Double {
        let engine = try EngineClient(paths: paths)
        defer { engine.stop() }
        var command: [String: JSONValue] = ["command": .string("negative_meter"), "input": .string(photo.path), "source_generation": .number(1),
                                            "film": .string(state.film), "paper": .string(state.paper), "params": state.params]
        if let bounds { command["bounds"] = bounds }
        return try await engine.request(command).1["print_exposure"].double
    }

    // MARK: Reading a negative

    /// Reading a negative finds its base, balances the print and meters its
    /// exposure in one step, then crops to the picture as a step of its own.
    /// Printing it as a scene again leaves the crop.
    func testReadingANegativeCropsToItsPictureAndLeavingKeepsTheCrop() async throws {
        let photo = try folder().appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        XCTAssertTrue(model.isNegative)
        XCTAssertEqual(model.negativeBase, NegativeScan.BaseStatus(found: true, picked: false, edge: "left"))
        XCTAssertNil(model.negativeWarning, "a negative isn't warned about")
        let survey = try XCTUnwrap(model.negativeSurvey)
        XCTAssertTrue(survey.looksNegative)
        // The crop is the picture's rectangle and straightening, copied.
        let crop = try XCTUnwrap(NegativeScan.crop(picture: survey.picture))
        XCTAssertEqual(model.state.crop, crop)
        XCTAssertGreaterThan(crop.rect.minX, 0.12)
        XCTAssertLessThan(crop.rect.minX, 0.16)
        XCTAssertEqual(crop.angle, 0, accuracy: 0.2)
        // The print exposure is this frame's, metered in its picture.
        let exposure = try await metered(model.renderState, photo: photo, bounds: NegativeScan.bounds(picture: survey.picture))
        XCTAssertEqual(model.value("enlarger.print_exposure").double, exposure, accuracy: 1e-6)

        XCTAssertEqual(undo.undoActionName, "Crop to Picture")
        undo.undo()
        XCTAssertFalse(model.state.crop.cropped)
        XCTAssertTrue(model.isNegative, "undoing the crop keeps the negative")
        XCTAssertEqual(undo.undoActionName, "Read as Negative")
        undo.redo()
        XCTAssertEqual(model.state.crop, crop)

        // Printed as a scene again: the negative's settings go, the crop stays.
        undo.beginUndoGrouping()
        model.chooseFilm("kodak_gold_200")
        undo.endUndoGrouping()
        XCTAssertFalse(model.isNegative)
        XCTAssertNil(model.negativeBase)
        XCTAssertEqual(model.state.crop, crop)
        XCTAssertEqual(model.value("enlarger.print_exposure"), NegativeScan.defaults["enlarger.print_exposure"])
    }

    /// Prep › Crop to picture crops again, as one undo step; on a scan
    /// without clear film it says so and leaves the crop.
    func testCropToPictureIsOneUndoStep() async throws {
        let dir = try folder()
        let photo = dir.appendingPathComponent("frame.tif"), bare = dir.appendingPathComponent("bare.tif")
        try SyntheticNegative.write(to: photo)
        try SyntheticNegative.write(to: bare, rebate: 0)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        let crop = model.state.crop
        undo.undo()
        XCTAssertFalse(model.state.crop.cropped)
        undo.beginUndoGrouping()
        model.cropToPicture()
        try await waitUntil { model.negativeWorking == nil && model.state.crop.cropped }
        undo.endUndoGrouping()
        XCTAssertEqual(model.state.crop, crop)
        XCTAssertEqual(undo.undoActionName, "Crop to Picture")

        try await readNegative(bare, in: model, undo: undo, picture: false)
        XCTAssertEqual(model.negativeBase?.found, false)
        XCTAssertFalse(model.state.crop.cropped, "no clear film, no crop")
        model.cropToPicture()
        try await waitUntil { model.negativeWorking == nil }
        XCTAssertFalse(model.state.crop.cropped)
        XCTAssertEqual(model.status, "No clear film round the picture to crop to: crop it by hand.")
    }

    /// Trying Your negative in the tray reads it without cropping; keeping
    /// it crops to the picture, in the same undo step as keeping it, named
    /// Read as Negative (not the crop's name for undo and the tray's for redo).
    func testYourNegativeFromTheTrayCropsOnceKept() async throws {
        let photo = try folder().appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        model.open(photo)
        try await waitUntil("the photograph") { model.previewCurrent || model.error != nil }
        model.openTray(paper: false)
        model.tryStock(NegativeScan.stockID)
        try await waitUntil("the base") { model.negativeBase != nil && model.negativeWorking == nil }
        XCTAssertTrue(model.isNegative)
        XCTAssertNotNil(model.tray, "a try doesn't close the tray")
        XCTAssertFalse(model.state.crop.cropped, "nor crop")
        undo.beginUndoGrouping()
        model.endTray(keep: true)
        undo.endUndoGrouping()
        XCTAssertTrue(model.state.crop.cropped)
        // Named for what it does, undone and redone alike.
        XCTAssertEqual(undo.undoActionName, "Read as Negative")
        undo.undo()
        XCTAssertFalse(model.state.crop.cropped)
        XCTAssertFalse(model.isNegative)
        XCTAssertEqual(undo.redoActionName, "Read as Negative")
        undo.redo()
        XCTAssertTrue(model.isNegative)
        XCTAssertTrue(model.state.crop.cropped)
    }

    /// A photograph of a scene read as a negative is said not to look like
    /// one, without blame; a RAW is pointed at its white balance.
    /// A photograph of a scene, not a negative: a blue sky over a white
    /// wall over green grass.
    private func writeScene(to photo: URL) throws {
        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3,
                                                    hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24))
        for y in 0..<64 {
            for x in 0..<96 {
                let rgb: [UInt8] = y < 24 ? [110, 160, 230] : y < 44 ? [245, 245, 240] : [70, 140 + UInt8(x % 40), 60]
                for c in 0..<3 { bitmap.bitmapData![(y * 96 + x) * 3 + c] = rgb[c] }
            }
        }
        try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: photo)
    }

    func testAPhotographThatIsntANegativeSaysSo() async throws {
        let photo = try folder().appendingPathComponent("scene.png")
        try writeScene(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo, picture: false)
        XCTAssertEqual(model.negativeWarning, "This doesn't look like a colour negative.")
        XCTAssertEqual(NegativeScan.notNegative(raw: true), "This doesn't look like a colour negative. If it is, set White balance to Daylight.")
    }

    /// The base picked on the rebate of a flipped, turned negative is the
    /// one found there: the click goes through the canvas's mapping to the
    /// photograph's own pixel. The neutral picker then meters the
    /// exposure for its filtration.
    func testPicksFollowTheGeometry() async throws {
        let photo = try folder().appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        let found = base(model.state)
        undo.beginUndoGrouping()
        model.turn(clockwise: true)
        model.flip()
        model.endCrop(apply: true)
        undo.endUndoGrouping()
        // Picking shows the whole frame, as the canvas does while picking.
        let image = try XCTUnwrap(model.image)
        let view = PhotographView(frame: .zero)
        view.show(image, size: model.imageSize, rotation: model.imageRotation, flip: model.imageFlip, crop: CropSettings())
        view.setFrameSize(view.displaySize)
        view.layoutSubtreeIfNeeded()
        // The rebate is the photograph's left edge; turned a quarter and flipped as shown, it's along the top.
        XCTAssertEqual(Orientation(turns: model.imageRotation, flip: model.imageFlip), Orientation(turns: 3, flip: true))
        let shown = Orientation(turns: model.imageRotation, flip: model.imageFlip).shown(CGPoint(x: 0.05, y: 0.5))
        XCTAssertLessThan(shown.y, 0.1)
        let fraction = try XCTUnwrap(view.photographFraction(CGPoint(x: shown.x * view.displaySize.width, y: (1 - shown.y) * view.displaySize.height)))
        XCTAssertEqual(fraction.x, 0.05, accuracy: 0.01); XCTAssertEqual(fraction.y, 0.5, accuracy: 0.01)
        model.negativePick = .base
        undo.beginUndoGrouping()
        model.pickedNegativePoint(fraction)
        try await waitUntil { model.negativeWorking == nil && model.negativeBase?.picked == true }
        undo.endUndoGrouping()
        XCTAssertEqual(undo.undoActionName, "Pick Film Base")
        for (picked, wanted) in zip(base(model.state), found) { XCTAssertEqual(picked, wanted, accuracy: 0.01) }

        model.negativePick = .neutral
        undo.beginUndoGrouping()
        model.pickedNegativePoint(CGPoint(x: 0.6, y: 0.5))
        try await waitUntil { model.negativeWorking == nil && undo.undoActionName == "Pick Neutral" }
        undo.endUndoGrouping()
        let survey = try XCTUnwrap(model.negativeSurvey)
        let exposure = try await metered(model.renderState, photo: photo, bounds: NegativeScan.bounds(picture: survey.picture))
        XCTAssertEqual(model.value("enlarger.print_exposure").double, exposure, accuracy: 1e-6)
    }

    /// The tone graph's histogram on a negative is the read film's density:
    /// more scan exposure in the base tuning reads thinner film, so it
    /// moves down, and it follows the change.
    func testTheHistogramReadsTheFilmDensity() async throws {
        let photo = try folder().appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        try await waitUntil("the preview") { model.previewCurrent }
        let before = try XCTUnwrap(model.filmHistogram)
        XCTAssertEqual(before.counts.map(\.count), [128, 128, 128])
        func mean(_ h: FilmHistogram) -> Double {
            let bins = h.bins(nil), total = bins.reduce(0, +)
            return bins.indices.reduce(0) { $0 + Double($1) * bins[$1] } / max(total, 1)
        }
        let ev = model.value("film_render.convert.exposure_compensation_ev").double
        undo.beginUndoGrouping()
        model.set("film_render.convert.exposure_compensation_ev", .number(ev + 1))
        undo.endUndoGrouping()
        try await waitUntil("the histogram") { model.previewCurrent && model.filmHistogram != before }
        let after = try XCTUnwrap(model.filmHistogram)
        print("film histogram mean bin \(mean(before)) → \(mean(after)) for +1 EV of scan gain")
        XCTAssertLessThan(mean(after), mean(before) - 4, "more light on the scan reads thinner film")
    }

    /// A negative's flat field is kept in its sidecar and is there again
    /// when the photograph is opened again (the defaults carry its key).
    func testAFlatFieldSurvivesAReopen() async throws {
        let dir = try folder()
        let a = dir.appendingPathComponent("a.tif"), b = dir.appendingPathComponent("b.tif")
        try SyntheticNegative.write(to: a)
        try SyntheticNegative.write(to: b, shift: 5)
        let light = dir.appendingPathComponent("light.tif")
        try SyntheticNegative.write(to: light, rebate: 0, light: 1, shift: 2)
        let store = SidecarStore()
        let model = try await model(store: store)
        XCTAssertEqual(NegativeScan.defaults["film_render.convert"]?["flat_field"], .string(""), "the defaults carry the key")
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(a, in: model, undo: undo)
        undo.beginUndoGrouping()
        model.chooseFlatField(light)
        try await waitUntil("the base") { model.negativeWorking == nil && model.previewCurrent }
        undo.endUndoGrouping()
        XCTAssertEqual(model.flatField.path, light.path)
        await model.flushSidecars()
        let saved = try XCTUnwrap(SidecarRestore.read(photo: a, store: store, data: paths.data).edits?.state)
        XCTAssertEqual(model.restoredState(saved, legacy: false).params[NegativeScan.flatFieldPath], .string(light.path))

        model.open(b)
        try await waitUntil("the other") { model.sourceItem == b && model.previewCurrent }
        XCTAssertFalse(model.flatField.isOn)
        model.open(a)
        try await waitUntil("the first again") { model.sourceItem == a && model.previewCurrent }
        XCTAssertTrue(model.isNegative)
        XCTAssertEqual(model.flatField.path, light.path)
    }

    /// What the helper finds for `state` on `photo` from scratch: the base,
    /// and the print balanced for it.
    private func detected(_ state: EditState, photo: URL) async throws -> (base: [Double], magenta: Double, yellow: Double) {
        let engine = try EngineClient(paths: paths)
        defer { engine.stop() }
        let command: [String: JSONValue] = ["command": .string("negative_base"), "input": .string(photo.path), "source_generation": .number(1),
                                            "film": .string(state.film), "paper": .string(state.paper), "params": state.params]
        let result = try await engine.request(command).1
        return (["cyan", "magenta", "yellow"].map { result["base"][$0].double }, result["m_filter_shift"].double, result["y_filter_shift"].double)
    }

    /// Every request about the negative goes through one discipline: a
    /// newer one supersedes Auto, whose answer for the old light is
    /// dropped; Prep stays busy until the newest is done, and what's left
    /// is the newest's answer.
    func testANewerRequestSupersedesAnOlderOne() async throws {
        let dir = try folder()
        let photo = dir.appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        // Crop to picture's quick answer doesn't free Prep while a newer
        // detection runs.
        undo.beginUndoGrouping()
        model.cropToPicture()
        XCTAssertEqual(model.negativeWorking, "Finding the picture…")
        model.set("film_render.convert.scan_illuminant", .string("D50"))
        XCTAssertEqual(model.negativeWorking, "Finding the film base…")
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertNotNil(model.negativeWorking, "the detection still runs")
        try await waitUntil("the base") { model.negativeWorking == nil }
        undo.endUndoGrouping()
        // Auto, then a new light before its answer: Auto's is dropped.
        undo.beginUndoGrouping()
        model.autoPrintNegative()
        XCTAssertEqual(model.negativeWorking, "Printing like a minilab…")
        model.set("film_render.convert.scan_illuminant", .string("D65"))
        XCTAssertEqual(model.negativeWorking, "Finding the film base…")
        try await waitUntil("the base") { model.negativeWorking == nil }
        undo.endUndoGrouping()
        XCTAssertEqual(undo.undoActionName, "Change Adjustment", "Auto's answer was dropped")
        let fresh = try await detected(model.renderState, photo: photo)
        for (a, b) in zip(base(model.state), fresh.base) { XCTAssertEqual(a, b, accuracy: 1e-9) }
        // Balanced for that base (at the print exposure of before, which
        // the meter then moved a little), not Auto's minilab print.
        XCTAssertEqual(model.value("enlarger.m_filter_shift").double, fresh.magenta, accuracy: 0.05)
        XCTAssertEqual(model.value("enlarger.y_filter_shift").double, fresh.yellow, accuracy: 0.05)
    }

    /// An edit made before a base detection's answer came, undone, leaves
    /// the edit that asked with its answer: never the base of before.
    func testUndoAfterAQuickEditKeepsTheBase() async throws {
        let dir = try folder()
        let photo = dir.appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        let d55 = base(model.state)
        undo.beginUndoGrouping()
        model.set("film_render.convert.scan_illuminant", .string("D50"))
        undo.endUndoGrouping()
        // Quicker than the detection.
        undo.beginUndoGrouping()
        model.set("film_render.grain.active", .bool(!model.value("film_render.grain.active").bool))
        undo.endUndoGrouping()
        try await waitUntil("the base") { model.negativeWorking == nil }
        let d50 = base(model.state)
        XCTAssertNotEqual(d50, d55)
        undo.undo()
        XCTAssertEqual(model.value("film_render.convert.scan_illuminant"), .string("D50"))
        XCTAssertEqual(base(model.state), d50, "the light's own base, not the one before")
        XCTAssertNil(model.negativeWorking, "put back, not found again")
        undo.redo()
        XCTAssertEqual(base(model.state), d50)
        // Undoing the light puts its base of before back.
        undo.undo(); undo.undo()
        XCTAssertEqual(base(model.state), d55)
    }

    /// Reset Lab on a negative puts its base and balance back as this frame
    /// measures them, in the reset's own step.
    func testResettingLabFindsTheBaseAgain() async throws {
        let dir = try folder()
        let photo = dir.appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let model = try await model()
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        let found = model.state
        undo.beginUndoGrouping()
        model.set("film_render.base.cyan", .number(found.params["film_render.base.cyan"].double + 0.2))
        model.set("enlarger.m_filter_shift", .number(found.params["enlarger.m_filter_shift"].double + 5))
        undo.endUndoGrouping()
        undo.beginUndoGrouping()
        model.resetEdits(tab: .lab)
        XCTAssertNotNil(model.negativeWorking, "found again")
        try await waitUntil("the base") { model.negativeWorking == nil }
        undo.endUndoGrouping()
        XCTAssertEqual(undo.undoActionName, "Reset Lab")
        XCTAssertEqual(base(model.state), base(found))
        for path in ["enlarger.m_filter_shift", "enlarger.y_filter_shift", "enlarger.print_exposure"] {
            XCTAssertEqual(model.value(path).double, found.params[path].double, accuracy: 1e-9, path)
        }
    }

    /// The film a negative is read as doesn't choose its paper, and its
    /// tile reads the negative for that film.
    func testReadAsKeepsThePaperAndItsTilesReadTheirFilm() async throws {
        let model = try await model()
        var negative = EditState()
        negative.film = "kodak_portra_400"; negative.paper = "fujifilm_crystal_archive_typeii"
        negative.params["workflow.route"] = .string(NegativeScan.route)
        let gold = try XCTUnwrap(model.stocks.first { $0.id == "kodak_gold_200" })
        XCTAssertFalse(gold.target.isEmpty)
        XCTAssertNotEqual(gold.target, negative.paper)
        let chosen = PreviewRules.choosing(gold.id, paper: false, keepPaper: false, in: negative, stocks: model.stocks, negative: true)
        XCTAssertEqual(chosen.film, gold.id)
        XCTAssertEqual(chosen.paper, negative.paper, "the paper stays")
        XCTAssertEqual(chosen.params["workflow.route"].string, NegativeScan.route)
        let source = URL(fileURLWithPath: "/a.tif")
        let tile = PreviewRules.request(for: gold, keepPaper: false, state: negative, stocks: model.stocks, source: source, generation: 1,
                                        negative: true, negativePoint: CGPoint(x: 0.05, y: 0.5))
        XCTAssertEqual(tile["read_negative"], .object(["point": .array([.number(0.05), .number(0.5)])]))
        let own = try XCTUnwrap(model.stocks.first { $0.id == negative.film })
        XCTAssertNil(PreviewRules.request(for: own, keepPaper: false, state: negative, stocks: model.stocks, source: source, generation: 1,
                                          negative: true)["read_negative"], "the current film's tile is the edit as it is")
        XCTAssertNil(PreviewRules.request(for: gold, keepPaper: false, state: negative, stocks: model.stocks, source: source, generation: 1)["read_negative"],
                     "a film printed as a scene")
    }

    // MARK: Presets

    /// A preset brings the look and the paper; the negative stays as it's
    /// read: film, base, backlight, route, its print balance and its roll.
    func testPresetsKeepTheNegative() {
        var negative = EditState()
        negative.film = "kodak_portra_400"; negative.paper = "kodak_portra_endura"
        negative.params["workflow.route"] = .string(NegativeScan.route)
        negative.params["film_render.base.cyan"] = .number(1.1)
        negative.params["film_render.convert.scan_illuminant"] = .string("D50")
        negative.params["enlarger.m_filter_shift"] = .number(4.4)
        negative.params["enlarger.print_exposure"] = .number(0.87)
        negative.params["io.scan_film"] = .bool(false)
        // A NEF read as shot, with its lens corrected.
        negative.params["io.raw_white_balance"] = .object(["mode": .string("as_shot"), "temperature": .number(5500), "tint": .number(1)])
        negative.params["io.lens_correction.enabled"] = .bool(true)
        negative.roll = NegativeRoll(id: "r1", magenta: 4.4, yellow: 0)
        var preset = negative
        preset.film = "fujifilm_provia_100f"; preset.paper = "kodak_supra_endura"
        // A preset brings the frozen GUI state's Daylight.
        preset.params = .object(["film_render": .object(["grain": .object(["active": .bool(false)])]),
                                 "io": .object(["scan_film": .bool(true), "raw_white_balance": .object(["mode": .string("daylight")]),
                                                "lens_correction": .object(["enabled": .bool(false)])])])
        let applied = NegativeScan.keepingNegative(preset, from: negative)
        XCTAssertEqual(applied.film, "kodak_portra_400")
        XCTAssertEqual(applied.paper, "kodak_supra_endura")
        XCTAssertEqual(applied.params["film_render.grain.active"], .bool(false))
        for path in ["workflow.route", "film_render.base.cyan", "film_render.convert.scan_illuminant", "enlarger.m_filter_shift", "enlarger.print_exposure",
                     "io.raw_white_balance", "io.lens_correction.enabled"] {
            XCTAssertEqual(applied.params[path], negative.params[path], path)
        }
        XCTAssertEqual(applied.params["io.scan_film"], .bool(false))
        XCTAssertEqual(applied.roll, negative.roll)
        // A scene takes the preset whole.
        var scene = negative; scene.params["workflow.route"] = .string("input > film > print > scan")
        XCTAssertEqual(NegativeScan.keepingNegative(preset, from: scene), preset)

        // A preset's thumbnail on a negative changes with its film, base and balance too.
        func key(_ state: EditState) -> PreviewTile.Key {
            PreviewRules.presetKey(PreviewRules.presetCommand(text: "{}", state: state, source: URL(fileURLWithPath: "/a.tif"), generation: 1),
                                   kept: ["prep"], generation: 1)
        }
        var other = negative; other.params["film_render.base.cyan"] = .number(1.2)
        XCTAssertNotEqual(key(other), key(negative))
        other = negative; other.film = "kodak_gold_200"
        XCTAssertNotEqual(key(other), key(negative))
        other = negative; other.params["io.raw_white_balance.mode"] = .string("daylight")
        XCTAssertNotEqual(key(other), key(negative), "read with another white balance")
        // A scene's doesn't: the preset brings its own film and balance.
        var otherScene = scene; otherScene.film = "kodak_gold_200"; otherScene.params["enlarger.m_filter_shift"] = .number(1)
        XCTAssertEqual(key(otherScene), key(scene))
    }

    /// Applying a preset file to a negative through the helper: the look
    /// and the paper change, the negative is read as before, and the new
    /// paper's print is balanced and metered again, in the same undo step.
    func testApplyingAPresetFileKeepsTheNegative() async throws {
        let dir = try folder()
        let photo = dir.appendingPathComponent("frame.tif")
        try SyntheticNegative.write(to: photo)
        let library = PresetLibrary(folder: dir.appendingPathComponent("Presets", isDirectory: true))
        let model = try await model(presets: library)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(photo, in: model, undo: undo)
        let before = model.state
        // Read as shot, as a NEF often is: the preset's Daylight would read it anew.
        XCTAssertEqual(before.params["io.raw_white_balance.mode"], .string("as_shot"))
        let text = #"{"simulation": {"film_stock": "kodak_gold_200", "print_paper": "kodak_supra_endura", "enlarger": {"print_exposure": 1.7}}}"#
        _ = try await library.add(Data(text.utf8), name: "Supra")
        await model.refreshPresets()
        let preset = try XCTUnwrap(model.presets.first)
        undo.beginUndoGrouping()
        model.applyPreset(preset)
        try await waitUntil { model.paper == "kodak_supra_endura" }
        try await waitUntil("the balance") { model.negativeWorking == nil }
        undo.endUndoGrouping()
        XCTAssertEqual(undo.undoActionName, "Apply Preset “Supra”")
        XCTAssertEqual(model.film, before.film, "still read as Portra 400")
        XCTAssertTrue(model.isNegative)
        XCTAssertEqual(base(model.state), base(before))
        for path in ["film_render.convert", "io.scan_film", "io.raw_white_balance", "io.lens_correction", "io.input_interpretation"] {
            XCTAssertEqual(model.value(path), before.params[path], path)
        }
        XCTAssertNotEqual(model.value("enlarger.m_filter_shift"), before.params["enlarger.m_filter_shift"], "balanced for the new paper")
        XCTAssertNotEqual(model.value("enlarger.print_exposure"), .number(1.7), "metered, not the preset's")
        let survey = try XCTUnwrap(model.negativeSurvey)
        let exposure = try await metered(model.renderState, photo: photo, bounds: NegativeScan.bounds(picture: survey.picture))
        XCTAssertEqual(model.value("enlarger.print_exposure").double, exposure, accuracy: 1e-6)
        undo.undo()
        XCTAssertEqual(model.state, before)
    }

    // MARK: Darkroom roll

    /// The camera's shutter, aperture and ISO give the extra light on a scan.
    func testScanExposureFromTheCamera() {
        let reference = PhotoFacts(iso: 100, exposure: 1.0 / 50, aperture: 8)
        XCTAssertEqual(NegativeScan.scanStops(of: PhotoFacts(iso: 100, exposure: 1.0 / 25, aperture: 8), from: reference)!, 1, accuracy: 1e-12)
        XCTAssertEqual(NegativeScan.scanStops(of: PhotoFacts(iso: 200, exposure: 1.0 / 50, aperture: 8), from: reference)!, 1, accuracy: 1e-12)
        XCTAssertEqual(NegativeScan.scanStops(of: PhotoFacts(iso: 100, exposure: 1.0 / 50, aperture: 4), from: reference)!, 2, accuracy: 1e-12)
        // What only one of them recorded doesn't count.
        XCTAssertEqual(NegativeScan.scanStops(of: PhotoFacts(iso: 100, exposure: 1.0 / 25), from: reference)!, 1, accuracy: 1e-12)
        XCTAssertNil(NegativeScan.scanStops(of: reference, from: nil))
        XCTAssertNil(NegativeScan.scanStops(of: PhotoFacts(), from: reference))
        // A stop more light reads with a stop less gain.
        XCTAssertEqual(NegativeScan.exposureEV(roll: -0.5, scanStops: 1), -1.5)
        XCTAssertEqual(NegativeScan.exposureEV(roll: -0.5, scanStops: nil), -0.5)
    }

    /// Joining a roll takes the reference frame's film, paper, backlight
    /// and base tuning, and the roll's filtration unless the frame set its own.
    func testJoiningARollKeepsAFramesOwnFiltration() {
        var reference = EditState()
        reference.film = "kodak_portra_400"
        reference.params["workflow.route"] = .string(NegativeScan.route)
        reference.params["film_render.base.cyan"] = .number(1.1)
        reference.params["film_render.convert.scan_illuminant"] = .string("D50")
        reference.params["film_render.convert.exposure_compensation_ev"] = .number(-0.4)
        reference.params["io.raw_white_balance.mode"] = .string("daylight")
        reference.params["io.lens_correction.enabled"] = .bool(true)
        reference.params["io.input_interpretation.mode"] = .string("manual")
        let roll = NegativeRoll(id: "r1", magenta: 4, yellow: -2)
        var scene = EditState(); scene.brightnessMatch = BrightnessMatch()
        scene.params["enlarger.m_filter_shift"] = .number(9)
        scene.params["io.raw_white_balance.mode"] = .string("as_shot")
        scene.params["io.lens_correction.enabled"] = .bool(false)
        let joined = NegativeScan.joining(roll, from: reference, into: scene)
        // The roll is read alike: white balance, lens correction, interpretation.
        for path in NegativeScan.scanReading { XCTAssertEqual(joined.params[path], reference.params[path], path) }
        XCTAssertEqual(joined.film, "kodak_portra_400")
        XCTAssertEqual(joined.params["workflow.route"].string, NegativeScan.route)
        XCTAssertEqual(joined.params["film_render.convert.scan_illuminant"], .string("D50"))
        XCTAssertEqual(joined.params["film_render.convert.exposure_compensation_ev"], .number(-0.4))
        XCTAssertEqual(joined.params["film_render.base.cyan"], .number(1.1))
        XCTAssertEqual(joined.params["enlarger.m_filter_shift"], .number(4))
        XCTAssertEqual(joined.params["enlarger.y_filter_shift"], .number(-2))
        XCTAssertNil(joined.brightnessMatch)
        XCTAssertEqual(joined.roll, roll)
        XCTAssertTrue(roll.filtered(joined))
        // A frame of this roll that set its own keeps it; one of another roll doesn't.
        var own = joined; own.params["enlarger.m_filter_shift"] = .number(6)
        let changed = NegativeRoll(id: "r1", magenta: 5, yellow: -1)
        XCTAssertEqual(NegativeScan.joining(changed, from: reference, into: own).params["enlarger.m_filter_shift"], .number(6))
        XCTAssertEqual(NegativeScan.joining(changed, from: reference, into: joined).params["enlarger.m_filter_shift"], .number(5))
        var other = own; other.roll?.id = "r2"
        XCTAssertEqual(NegativeScan.joining(changed, from: reference, into: other).params["enlarger.m_filter_shift"], .number(5))
    }

    /// The roll is kept in each frame's sidecar, and doesn't render.
    func testTheRollIsKeptInTheSidecar() throws {
        let table = try SidecarDefaults.load(version: SidecarDefaults.latest, data: root.appendingPathComponent("data"))
        var state = EditState(); state.film = table.film; state.paper = table.paper; state.params = table.params
        state.roll = NegativeRoll(id: "ab12cd34", magenta: 4.4, yellow: -1.75)
        let edits = PhotoSidecar.encodeEdits(state, matchSolved: false, table: table)
        XCTAssertEqual(edits["roll"], .object(["id": .string("ab12cd34"), "magenta": .number(4.4), "yellow": .number(-1.75)]))
        XCTAssertEqual(try PhotoSidecar.decodeEdits(edits, table: table).state.roll, state.roll)
        XCTAssertThrowsError(try PhotoSidecar.decodeEdits(.object(["roll": .object(["id": .string("x")])]), table: table))
        // With the paper it's balanced for.
        var onPaper = state; onPaper.roll?.paper = "kodak_supra_endura"
        let withPaper = PhotoSidecar.encodeEdits(onPaper, matchSolved: false, table: table)
        XCTAssertEqual(withPaper["roll"]["paper"], .string("kodak_supra_endura"))
        XCTAssertEqual(try PhotoSidecar.decodeEdits(withPaper, table: table).state.roll, onPaper.roll)
        var plain = state; plain.roll = nil
        XCTAssertEqual(state.rendered, plain.rendered)
        XCTAssertEqual(PhotoSidecar.encodeEdits(plain, matchSolved: false, table: table)["roll"], .null)
        // It goes with reading the negative when edits are copied.
        XCTAssertEqual(EditGroup.merge([.filmAndPaper], from: state, into: EditState()).roll, state.roll)
        XCTAssertNil(EditGroup.merge([.lab], from: state, into: EditState()).roll)
        XCTAssertNil(NegativeScan.leaving(state).roll)
    }

    /// Use for whole roll, through the helper: every frame in view shares the
    /// film, backlight and filtration; a frame with clear film of its own
    /// gets its own base, one without the roll's, its exposure shifted by
    /// the camera's settings; each meters its own print exposure. A frame
    /// that sets its own filtration keeps it through a later roll change.
    /// Each change is one undo step.
    func testUseForWholeRoll() async throws {
        let dir = try folder()
        let a = dir.appendingPathComponent("a.tif"), b = dir.appendingPathComponent("b.tif"), c = dir.appendingPathComponent("c.tif")
        try SyntheticNegative.write(to: a)
        // No clear film, a stop more light from a slower shutter, another picture.
        try SyntheticNegative.write(to: b, rebate: 0, light: 1.2, shift: 7, exposure: 1.0 / 25)
        // Clear film, scanned a stop darker though the camera says the same.
        try SyntheticNegative.write(to: c, light: 0.3, shift: 3)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        let watching = Task { await model.library.watch(dir) }
        defer { watching.cancel() }
        try await waitUntil("the folder") { model.library.summariesLoaded && model.library.files.count == 3 }
        try await readNegative(a, in: model, undo: undo)
        // The light box: the base is found again for it.
        undo.beginUndoGrouping()
        model.set("film_render.convert.scan_illuminant", .string("D50"))
        try await waitUntil { model.negativeWorking == nil && model.negativeBase != nil }
        undo.endUndoGrouping()
        let reference = model.state

        func shareRoll() async throws {
            undo.beginUndoGrouping()
            model.useNegativeForRoll()
            XCTAssertNotNil(model.negativeWorking)
            try await waitUntil("the roll", seconds: 120) { model.negativeWorking == nil }
            undo.endUndoGrouping()
            XCTAssertEqual(undo.undoActionName, "Use for Whole Roll")
            await model.flushSidecars()
        }
        func saved(_ photo: URL) throws -> EditState {
            let restore = SidecarRestore.read(photo: photo, store: store, data: paths.data)
            return try XCTUnwrap(restore.edits?.state, photo.lastPathComponent)
        }
        try await shareRoll()
        let roll = try XCTUnwrap(model.state.roll)
        XCTAssertEqual(roll.magenta, reference.params["enlarger.m_filter_shift"].double)
        XCTAssertEqual(roll.yellow, reference.params["enlarger.y_filter_shift"].double)
        let (frameB, frameC) = (try saved(b), try saved(c))
        for frame in [frameB, frameC] {
            XCTAssertEqual(frame.roll, roll)
            XCTAssertEqual(frame.film, reference.film)
            XCTAssertEqual(frame.paper, reference.paper)
            XCTAssertEqual(frame.params["workflow.route"].string, NegativeScan.route)
            XCTAssertEqual(frame.params["film_render.convert.scan_illuminant"], .string("D50"))
            XCTAssertTrue(roll.filtered(frame))
        }
        let ev = { (s: EditState) in s.params["film_render.convert.exposure_compensation_ev"].double }
        // b has no clear film: the roll's base, a stop less gain for its stop more light.
        XCTAssertEqual(base(frameB), base(reference))
        XCTAssertEqual(ev(frameB), ev(reference) - 1, accuracy: 1e-9)
        XCTAssertFalse(frameB.crop.cropped)
        // c has: its own, a stop more gain for its darker scan, whatever the camera says.
        print("roll ev \(ev(reference)), c's own \(ev(frameC)); bases \(base(reference)) \(base(frameC))")
        XCTAssertEqual(ev(frameC), ev(reference) + 1, accuracy: 0.05)
        for (own, roll) in zip(base(frameC), base(reference)) { XCTAssertEqual(own, roll, accuracy: 0.01) }
        XCTAssertTrue(frameC.crop.cropped)
        // Each meters its own print exposure.
        let exposureB = try await metered(frameB, photo: b, bounds: nil)
        XCTAssertEqual(frameB.params["enlarger.print_exposure"].double, exposureB, accuracy: 1e-6)
        XCTAssertNotEqual(frameB.params["enlarger.print_exposure"], reference.params["enlarger.print_exposure"])

        // Undo puts the frames back as they were.
        undo.undo()
        await model.flushSidecars()
        XCTAssertNil(SidecarRestore.read(photo: b, store: store, data: paths.data).edits)
        XCTAssertNil(model.state.roll)
        undo.redo()
        await model.flushSidecars()
        XCTAssertEqual(try saved(b).roll, roll)

        // b sets its own filtration.
        model.open(b)
        try await waitUntil("b") { model.previewCurrent }
        XCTAssertTrue(model.isNegative)
        undo.beginUndoGrouping()
        model.autoPrintNegative()
        try await waitUntil { model.negativeWorking == nil && undo.undoActionName == "Auto Print" }
        undo.endUndoGrouping()
        XCTAssertFalse(roll.filtered(model.state), "b's own filtration")
        let ownB = model.state.params["enlarger.m_filter_shift"]

        // A roll change from a: new filtration reaches c, not b.
        model.open(a)
        try await waitUntil("a") { model.previewCurrent }
        undo.beginUndoGrouping()
        model.set("enlarger.m_filter_shift", .number(roll.magenta + 3))
        undo.endUndoGrouping()
        try await shareRoll()
        XCTAssertEqual(model.state.roll?.magenta, roll.magenta + 3)
        XCTAssertEqual(model.state.roll?.id, roll.id, "the same roll")
        XCTAssertEqual(try saved(c).params["enlarger.m_filter_shift"], .number(roll.magenta + 3))
        XCTAssertEqual(try saved(b).params["enlarger.m_filter_shift"], ownB)

        // b goes back to the roll's filtration, its exposure metered again.
        model.open(b)
        try await waitUntil("b") { model.previewCurrent }
        undo.beginUndoGrouping()
        model.useRollFiltration()
        try await waitUntil { model.negativeWorking == nil && undo.undoActionName == "Use Roll’s Filtration" }
        undo.endUndoGrouping()
        XCTAssertEqual(model.state.params["enlarger.m_filter_shift"], .number(roll.magenta + 3))
        XCTAssertTrue(try XCTUnwrap(model.state.roll).filtered(model.state))
    }

    /// The model reads `dir` as its library, with sidecars in `store`.
    private func watching(_ dir: URL, count: Int, in model: EditorModel) async throws -> Task<Void, Never> {
        let watching = Task { await model.library.watch(dir) }
        try await waitUntil("the folder") { model.library.summariesLoaded && model.library.files.count == count }
        return watching
    }

    /// Use for whole roll leaves a photograph that doesn't look like a
    /// negative exactly as it was, and says so; from such a photograph it
    /// changes nothing.
    func testUseForWholeRollSkipsPhotographsThatArentNegatives() async throws {
        let dir = try folder()
        let a = dir.appendingPathComponent("a.tif"), b = dir.appendingPathComponent("b.tif"), scene = dir.appendingPathComponent("scene.png")
        try SyntheticNegative.write(to: a)
        try SyntheticNegative.write(to: b, shift: 3)
        try writeScene(to: scene)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        let watching = try await watching(dir, count: 3, in: model)
        defer { watching.cancel() }
        try await readNegative(a, in: model, undo: undo)
        undo.beginUndoGrouping()
        model.useNegativeForRoll()
        try await waitUntil("the roll", seconds: 120) { model.negativeWorking == nil }
        undo.endUndoGrouping()
        await model.flushSidecars()
        XCTAssertEqual(undo.undoActionName, "Use for Whole Roll")
        let roll = try XCTUnwrap(model.state.roll)
        XCTAssertEqual(SidecarRestore.read(photo: b, store: store, data: paths.data).edits?.state.roll, roll)
        XCTAssertNil(SidecarRestore.read(photo: scene, store: store, data: paths.data).edits, "the scene is left as it was")
        XCTAssertEqual(model.status, "1 photograph skipped: it doesn't look like a negative.")
        XCTAssertEqual(NegativeScan.skipped(2), "2 photographs skipped: they don't look like negatives.")

        // From the scene read as a negative: nothing changes.
        try await readNegative(scene, in: model, undo: undo, picture: false)
        XCTAssertNotNil(model.negativeWarning)
        undo.beginUndoGrouping()
        model.useNegativeForRoll()
        try await waitUntil("the check") { model.negativeWorking == nil }
        undo.endUndoGrouping()
        await model.flushSidecars()
        XCTAssertEqual(model.status, "The roll wasn't shared: this photograph doesn't look like a negative.")
        XCTAssertNil(model.state.roll)
        XCTAssertNotEqual(undo.undoActionName, "Use for Whole Roll")
        XCTAssertEqual(SidecarRestore.read(photo: b, store: store, data: paths.data).edits?.state.roll, roll)
    }

    /// The not-a-negative warning is there when the photograph is opened
    /// again, and after a relaunch, with nothing asked.
    func testTheWarningComesBackOnReopen() async throws {
        let dir = try folder()
        let scene = dir.appendingPathComponent("scene.png"), other = dir.appendingPathComponent("other.tif")
        try writeScene(to: scene)
        try SyntheticNegative.write(to: other)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(scene, in: model, undo: undo, picture: false)
        let warning = try XCTUnwrap(model.negativeWarning)
        await model.flushSidecars()
        model.open(other)
        try await waitUntil("the other") { model.sourceItem == other && model.previewCurrent }
        XCTAssertNil(model.negativeWarning)
        model.open(scene)
        try await waitUntil("the scene") { model.sourceItem == scene && model.previewCurrent }
        XCTAssertTrue(model.isNegative)
        try await waitUntil("the warning") { model.negativeWarning != nil }
        XCTAssertEqual(model.negativeWarning, warning)
        // Its Film base line says where the base came from too.
        XCTAssertNotNil(model.negativeBase)
        XCTAssertNil(model.negativeWorking, "nothing was asked")

        // A new launch.
        let again = try await self.model(store: store)
        again.open(scene)
        try await waitUntil("the scene") { again.previewCurrent }
        XCTAssertTrue(again.isNegative)
        try await waitUntil("the warning") { again.negativeWarning != nil }
        XCTAssertEqual(again.negativeWarning, warning)
    }

    /// Prep's Film base line describes the base shown: after undo, redo
    /// and a reopen, never the last answer's words.
    func testTheFilmBaseLineFollowsUndo() async throws {
        let dir = try folder()
        let a = dir.appendingPathComponent("a.tif"), b = dir.appendingPathComponent("b.tif")
        try SyntheticNegative.write(to: a)
        try SyntheticNegative.write(to: b, shift: 4)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        try await readNegative(a, in: model, undo: undo)
        let found = NegativeScan.BaseStatus(found: true, picked: false, edge: "left")
        XCTAssertEqual(model.negativeBase, found)
        // Set by hand in Lab.
        undo.beginUndoGrouping()
        model.set("film_render.base.cyan", .number(model.value("film_render.base.cyan").double + 0.1))
        undo.endUndoGrouping()
        XCTAssertEqual(model.negativeBase?.from, .hand)
        XCTAssertEqual(model.negativeBase?.sentence(), "Film base as picked or set in Lab.")
        undo.undo()
        XCTAssertEqual(model.negativeBase, found)
        // Back to a scene and read again by redo: the base found, not "Reading the negative as".
        undo.undo(); undo.undo()
        XCTAssertFalse(model.isNegative)
        XCTAssertNil(model.negativeBase)
        undo.redo()
        XCTAssertTrue(model.isNegative)
        XCTAssertEqual(model.negativeBase, found)
        XCTAssertNil(model.negativeWorking)
        // A pick (in the picture, so another base), undone and redone.
        undo.redo()
        model.negativePick = .base
        undo.beginUndoGrouping()
        model.pickedNegativePoint(CGPoint(x: 0.6, y: 0.5))
        try await waitUntil { model.negativeWorking == nil && model.negativeBase?.picked == true }
        undo.endUndoGrouping()
        undo.undo()
        XCTAssertEqual(model.negativeBase, found)
        undo.redo()
        XCTAssertEqual(model.negativeBase?.picked, true)
        // Opened again, the base found is said as found, with nothing asked.
        undo.undo()
        await model.flushSidecars()
        model.open(b)
        try await waitUntil("b") { model.sourceItem == b && model.previewCurrent }
        model.open(a)
        try await waitUntil("a") { model.sourceItem == a && model.previewCurrent }
        try await waitUntil("the survey") { model.negativeBase != nil }
        XCTAssertEqual(model.negativeBase, found)
    }

    /// On a frame of a roll, Reset Lab and Find again measure its base
    /// afresh, but its filtration goes back to the roll's, on the roll, in
    /// the same step. Undo puts its own back, and Prep says the base is the
    /// roll's again.
    func testResettingARollFrameKeepsTheRollsFiltration() async throws {
        let dir = try folder()
        let a = dir.appendingPathComponent("a.tif"), b = dir.appendingPathComponent("b.tif")
        try SyntheticNegative.write(to: a)
        // No clear film: the roll's base.
        try SyntheticNegative.write(to: b, rebate: 0, light: 1.2, shift: 7, exposure: 1.0 / 25)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        let watching = try await watching(dir, count: 2, in: model)
        defer { watching.cancel() }
        try await readNegative(a, in: model, undo: undo)
        undo.beginUndoGrouping()
        model.useNegativeForRoll()
        try await waitUntil("the roll", seconds: 120) { model.negativeWorking == nil }
        undo.endUndoGrouping()
        let roll = try XCTUnwrap(model.state.roll)
        await model.flushSidecars()
        model.open(b)
        try await waitUntil("b") { model.sourceItem == b && model.previewCurrent }
        try await waitUntil("the survey") { model.negativeBase != nil }
        XCTAssertEqual(model.negativeBase?.from, .roll)
        let rollBase = base(model.state)

        func auto() async throws {
            undo.beginUndoGrouping()
            model.autoPrintNegative()
            try await waitUntil { model.negativeWorking == nil && undo.undoActionName == "Auto Print" }
            undo.endUndoGrouping()
            XCTAssertFalse(roll.filtered(model.state), "b's own filtration")
        }
        func checkOnRoll(_ action: String) async throws {
            XCTAssertEqual(undo.undoActionName, action)
            XCTAssertEqual(model.state.roll, roll, "still on its roll")
            XCTAssertTrue(roll.filtered(model.state), "the roll's filtration")
            let fresh = try await detected(model.renderState, photo: b)
            for (x, y) in zip(base(model.state), fresh.base) { XCTAssertEqual(x, y, accuracy: 1e-9) }
            let exposure = try await metered(model.renderState, photo: b, bounds: nil)
            XCTAssertEqual(model.value("enlarger.print_exposure").double, exposure, accuracy: 1e-6)
            XCTAssertEqual(model.negativeBase?.guessed, true, "this frame's own guess")
        }

        try await auto()
        let own = model.state
        undo.beginUndoGrouping()
        model.resetEdits(tab: .lab)
        try await waitUntil("the base") { model.negativeWorking == nil }
        undo.endUndoGrouping()
        try await checkOnRoll("Reset Lab")
        undo.undo()
        XCTAssertEqual(model.state, own)
        XCTAssertEqual(model.negativeBase?.from, .roll, "the roll's base again")
        XCTAssertEqual(base(model.state), rollBase)

        undo.beginUndoGrouping()
        model.redetectNegativeBase()
        try await waitUntil("the base") { model.negativeWorking == nil && undo.undoActionName == "Detect Film Base" }
        undo.endUndoGrouping()
        try await checkOnRoll("Detect Film Base")
        undo.undo()
        XCTAssertEqual(model.negativeBase?.from, .roll)
        XCTAssertEqual(model.negativeBase?.sentence(), "Film base from the roll: this frame shows no clear film of its own.")
    }

    /// Scan is always the film tray's first group. The tray opens on it
    /// once the helper says the photograph looks like a colour negative:
    /// asked as the tray opens, an open tray moves there on a yes, and the
    /// next opening starts there. A scene opens on its film. A photograph
    /// read as a negative opens on Scan. Asked once per opening of the photograph.
    func testTheFilmTrayOpensOnScanForANegative() async throws {
        let dir = try folder()
        let negative = dir.appendingPathComponent("negative.tif"), scene = dir.appendingPathComponent("scene.png")
        try SyntheticNegative.write(to: negative)
        try writeScene(to: scene)
        let model = try await model()
        func scanIndex() throws -> Int? {
            StockTray.sections(model: model, visit: try XCTUnwrap(model.tray)).firstIndex { $0.title == "Scan" }
        }
        for (photo, looks) in [(negative, true), (scene, false)] {
            model.open(photo)
            try await waitUntil("the photograph") { model.previewCurrent }
            XCTAssertFalse(model.looksLikeANegative, "not known yet")
            model.openTray(paper: false)
            XCTAssertEqual(try scanIndex(), 0, "always first")
            XCTAssertEqual(model.tray?.opensOnScan, false, "not known yet")
            try await waitUntil("the check") { model.looksNegative?.looks == looks && model.looksLikeANegative == looks }
            XCTAssertEqual(model.tray?.opensOnScan, looks, "the open tray moves to Scan on a yes")
            XCTAssertEqual(try scanIndex(), 0)
            model.endTray(keep: false)
            let asked = try XCTUnwrap(model.looksNegative)
            model.openTray(paper: false)
            XCTAssertEqual(model.tray?.opensOnScan, looks, photo.lastPathComponent)
            XCTAssertEqual(model.looksNegative?.generation, asked.generation, "not asked again")
            model.endTray(keep: false)
        }
        XCTAssertFalse(model.isNegative, "nothing was read as a negative")
        model.open(negative)
        try await waitUntil("the negative") { model.previewCurrent }
        model.chooseNegative()
        try await waitUntil("read as a negative") { model.isNegative && model.negativeWorking == nil }
        model.openTray(paper: false)
        XCTAssertEqual(model.tray?.opensOnScan, true, "read as a negative")
        model.endTray(keep: false)
        model.openTray(paper: true)
        XCTAssertEqual(model.tray?.opensOnScan, false, "the paper tray has no Scan")
        model.endTray(keep: false)
    }

    /// A frame of a roll put on another paper gets its own filtration,
    /// balanced for that paper, and stays on its roll; Prep says the paper
    /// is why. Finding its base again keeps it balanced for its paper. A
    /// frame whose filtration was simply moved is said to have its own.
    func testARollFrameOnAnotherPaperHasItsOwnFiltration() async throws {
        let dir = try folder()
        let a = dir.appendingPathComponent("a.tif"), b = dir.appendingPathComponent("b.tif")
        try SyntheticNegative.write(to: a)
        try SyntheticNegative.write(to: b, shift: 5)
        let store = SidecarStore()
        let model = try await model(store: store)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        let watching = try await watching(dir, count: 2, in: model)
        defer { watching.cancel() }
        try await readNegative(a, in: model, undo: undo)
        undo.beginUndoGrouping()
        model.useNegativeForRoll()
        try await waitUntil("the roll", seconds: 120) { model.negativeWorking == nil }
        undo.endUndoGrouping()
        let roll = try XCTUnwrap(model.state.roll)
        XCTAssertEqual(roll.paper, model.paper)
        await model.flushSidecars()
        model.open(b)
        try await waitUntil("b") { model.sourceItem == b && model.previewCurrent }
        XCTAssertTrue(roll.filtered(model.state))

        // Moved by hand: its own, no reason given.
        undo.beginUndoGrouping()
        model.set("enlarger.m_filter_shift", .number(roll.magenta + 2))
        undo.endUndoGrouping()
        XCTAssertEqual(roll.ownFiltration(model.state), "This frame has its own filtration.")
        undo.undo()

        let other = try XCTUnwrap(model.papers.first { $0.id != roll.paper }).id
        undo.beginUndoGrouping()
        model.choosePaper(other)
        try await waitUntil("the balance") { model.paper == other && model.negativeWorking == nil && !roll.filtered(model.state) }
        undo.endUndoGrouping()
        XCTAssertEqual(model.state.roll, roll, "still on its roll")
        XCTAssertEqual(roll.ownFiltration(model.state), "On another paper than its roll, so it has its own filtration.")
        let balanced = try await detected(model.renderState, photo: b)
        XCTAssertEqual(model.value("enlarger.m_filter_shift").double, balanced.magenta, accuracy: 0.05)

        // Found again: balanced for its own paper, not the roll's.
        undo.beginUndoGrouping()
        model.redetectNegativeBase()
        try await waitUntil("the base") { model.negativeWorking == nil && undo.undoActionName == "Detect Film Base" }
        undo.endUndoGrouping()
        XCTAssertFalse(roll.filtered(model.state))
        XCTAssertEqual(model.value("enlarger.m_filter_shift").double, balanced.magenta, accuracy: 1e-9)
        XCTAssertEqual(model.state.roll, roll)
    }
}
