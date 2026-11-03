import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

final class ExposurePlacementTests: XCTestCase {
    func testOnlyFreshRawSelectionGetsOneAutomaticAttempt() {
        var gate = ExposurePlacementGate()
        gate.opened(generation: 1, restored: false, raw: true, enabled: true)
        XCTAssertTrue(gate.takeAutomatic(generation: 1, raw: true, enabled: true))
        XCTAssertFalse(gate.takeAutomatic(generation: 1, raw: true, enabled: true))
        for (restored, raw, enabled) in [(true, true, true), (false, false, true), (false, true, false)] {
            gate.opened(generation: 2, restored: restored, raw: raw, enabled: enabled)
            XCTAssertFalse(gate.takeAutomatic(generation: 2, raw: true, enabled: true))
        }
    }

    func testEditDuringLoadAndOlderRenderCannotTriggerAutomaticPlacement() {
        var gate = ExposurePlacementGate()
        gate.opened(generation: 2, restored: false, raw: true, enabled: true)
        XCTAssertFalse(gate.takeAutomatic(generation: 1, raw: true, enabled: true))
        XCTAssertEqual(gate.automaticGeneration, 2, "An old render must not consume a new photograph's attempt")
        gate.edited()
        XCTAssertFalse(gate.takeAutomatic(generation: 2, raw: true, enabled: true))
    }

    func testReferenceDoesNotGenerateThumbnailWhenNoneIsEmbedded() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("no-embedded-preview.png")
        let context = try XCTUnwrap(CGContext(data: nil, width: 320, height: 240, bitsPerComponent: 8, bytesPerRow: 1280,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        XCTAssertThrowsError(try CameraBrightnessReference.png(from: url, expectedSize: CGSize(width: 320, height: 240)))
    }

    /// A grey test preview with a textured picture and black bars (value 2,
    /// like the Leica previews) on the given edges.
    private func preview(width: Int, height: Int, top: Int = 0, bottom: Int = 0, left: Int = 0, right: Int = 0) throws -> CGImage {
        var pixels = [UInt8](repeating: 2, count: width * height)
        for y in top..<(height - bottom) {
            for x in left..<(width - right) { pixels[y * width + x] = UInt8(60 + (x * 7 + y * 13) % 150) }
        }
        let context = try XCTUnwrap(CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width,
            space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue))
        return try XCTUnwrap(context.makeImage())
    }

    func testLetterboxedPreviewsAreTrimmedToThePicture() throws {
        let leicaM240 = 5976.0 / 3992, leicaM8 = 3916.0 / 2634
        // M (Typ 240) / Monochrom (Typ 246): 160×120, 14 black rows at the top only.
        let m240 = try XCTUnwrap(CameraBrightnessReference.trimmingBars(try preview(width: 160, height: 120, top: 14), toAspect: leicaM240))
        XCTAssertEqual([m240.width, m240.height], [160, 106])
        // M8: 320×240, 13 rows top and bottom.
        let m8 = try XCTUnwrap(CameraBrightnessReference.trimmingBars(try preview(width: 320, height: 240, top: 13, bottom: 13), toAspect: leicaM8))
        XCTAssertEqual([m8.width, m8.height], [320, 214])
        // Pillarboxed: a 4:3 picture in a 3:2 preview.
        let sides = try XCTUnwrap(CameraBrightnessReference.trimmingBars(try preview(width: 512, height: 341, left: 29, right: 28), toAspect: 4.0 / 3))
        XCTAssertEqual([sides.width, sides.height], [455, 341])
    }

    func testCroppedOrDarkEdgedPreviewIsNotTrimmed() throws {
        // A real crop has no bars to remove.
        XCTAssertNil(CameraBrightnessReference.trimmingBars(try preview(width: 160, height: 120), toAspect: 1.5))
        // Black at the top that leaves the wrong shape (a night sky, say) is refused.
        XCTAssertNil(CameraBrightnessReference.trimmingBars(try preview(width: 160, height: 120, top: 30), toAspect: 1.5))
        // What's left after the bars must still be big enough to meter.
        XCTAssertNil(CameraBrightnessReference.trimmingBars(try preview(width: 90, height: 68, top: 4, bottom: 4), toAspect: 1.5))
    }

    /// Optional: colon-separated RAWs with letterboxed previews (Leica M8,
    /// M (Typ 240); samples on raw.pixls.us) must give a reference of the photo's shape.
    func testRealLetterboxedPreviewsGiveAReference() throws {
        guard let paths = ProcessInfo.processInfo.environment["NEGS_TEST_LETTERBOX_RAWS"] else { throw XCTSkip("Set NEGS_TEST_LETTERBOX_RAWS") }
        for path in paths.split(separator: ":").map(String.init) {
            let source = try XCTUnwrap(CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil))
            let properties = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
            let size = CGSize(width: try XCTUnwrap(properties[kCGImagePropertyPixelWidth] as? Double),
                              height: try XCTUnwrap(properties[kCGImagePropertyPixelHeight] as? Double))
            let data = try CameraBrightnessReference.png(from: URL(fileURLWithPath: path), expectedSize: size)
            let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil)), 0, nil))
            XCTAssertTrue(CameraBrightnessReference.sameShape(Double(image.width) / Double(image.height), size.width / size.height), path)
        }
    }

    /// Optional real-camera fixture: tests exercise the same ImageIO path used
    /// by the app, with no camera files stored in the repository.
    func testRealEmbeddedReferenceHasBoundedTaggedSRGBOutput() throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW to a 4:3 camera RAW fixture") }
        let data = try CameraBrightnessReference.png(from: URL(fileURLWithPath: path), expectedSize: CGSize(width: 4, height: 3))
        let source = try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        XCTAssertLessThanOrEqual(max(image.width, image.height), 512)
        XCTAssertEqual(image.colorSpace?.name, CGColorSpace.sRGB)
        XCTAssertThrowsError(try CameraBrightnessReference.png(from: URL(fileURLWithPath: path), expectedSize: CGSize(width: 3, height: 4)))
    }
}

@MainActor final class ExposurePlacementLifecycleTests: XCTestCase {
    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    func testFreshRawIsMatchedOnceAndRestoredExposureIsPreserved() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        let defaults = UserDefaults.standard, savedPreference = UserDefaults.standard.object(forKey: "autoPlaceNewImages")
        defer { defaults.set(savedPreference, forKey: "autoPlaceNewImages") }
        let model = EditorModel()
        defer { model.shutdown() }
        model.autoPlaceNewImages = true
        let savedFresh = defaults.object(forKey: "freshEditsForNewPhotos")
        defer { defaults.set(savedFresh, forKey: "freshEditsForNewPhotos") }
        model.freshEditsForNewPhotos = true
        await model.start(paths: paths(), restoration: Task { nil })
        XCTAssertEqual(model.defaultValue("prep.exposure_ev"), .number(0))
        model.setLongestSide(512)
        model.set("prep.exposure_ev", .number(-1))
        model.open(URL(fileURLWithPath: path))
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(0), "New photographs start with no inherited exposure placement")
        try await waitUntil { model.exposurePlacementMessage != nil || model.error != nil }
        try await waitUntil { !model.placingExposure && model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        XCTAssertTrue(model.exposurePlacementMessage?.hasPrefix("Matched") == true, model.exposurePlacementMessage ?? "No automatic match")
        let matched = model.value("prep.exposure_ev")
        model.set("prep.shadows", .number(10))
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertEqual(model.value("prep.exposure_ev"), matched, "A later render must not repeat or compound the match")
        var session = try XCTUnwrap(model.session)
        session.state.params["prep.exposure_ev"] = .number(1.25)
        let restored = EditorModel()
        defer { restored.shutdown() }
        await restored.start(paths: paths(), restoration: Task { session })
        try await waitUntil { restored.previewCurrent || restored.error != nil }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertNil(restored.error)
        XCTAssertEqual(restored.value("prep.exposure_ev"), .number(1.25))
        XCTAssertNil(restored.exposurePlacementMessage, "Restoring a saved edit must never auto-match")
    }

    func testEditingDuringLoadSuppressesAutoAndManualStaleResult() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        let defaults = UserDefaults.standard, savedPreference = UserDefaults.standard.object(forKey: "autoPlaceNewImages")
        defer { defaults.set(savedPreference, forKey: "autoPlaceNewImages") }
        let model = EditorModel()
        defer { model.shutdown() }
        model.autoPlaceNewImages = true
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        model.set("prep.shadows", .number(10))
        try await waitUntil { model.previewCurrent || model.error != nil }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertNil(model.error)
        XCTAssertNil(model.exposurePlacementMessage)
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(0))
        model.matchCameraBrightness()
        XCTAssertTrue(model.placingExposure)
        XCTAssertTrue(model.canSave, "Analysing brightness must keep the finished preview saveable")
        model.set("prep.exposure_ev", .number(-0.5))
        try await waitUntil { !model.placingExposure }
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(-0.5), "A pending match must not replace a newer manual edit")
        XCTAssertEqual(model.value("prep.shadows"), .number(10))
    }

    /// An edit and its Undo restore an equal state, but the match was made
    /// for the photograph as it was before them: it must not be applied.
    func testEditThenUndoDuringMatchSkipsItAndUndoMatchDoesNotRematch() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        let defaults = UserDefaults.standard, savedPreference = defaults.object(forKey: "autoPlaceNewImages")
        defer { defaults.set(savedPreference, forKey: "autoPlaceNewImages") }
        let model = EditorModel()
        defer { model.shutdown() }
        model.autoPlaceNewImages = false
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        model.matchCameraBrightness()
        XCTAssertTrue(model.placingExposure)
        undo.beginUndoGrouping()
        model.border(\.enabled).wrappedValue = !model.border.enabled
        undo.endUndoGrouping()
        undo.undo()
        try await waitUntil { !model.placingExposure }
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(0))
        XCTAssertEqual(model.exposurePlacementMessage, "Adjustments changed; brightness match skipped.")

        // The match is applied later, so its group stays open until then
        // (an ungrouped registration raises with groupsByEvent off).
        undo.beginUndoGrouping()
        model.matchCameraBrightness()
        try await waitUntil { !model.placingExposure }
        undo.endUndoGrouping()
        XCTAssertTrue(model.exposurePlacementMessage?.hasPrefix("Matched") == true, model.exposurePlacementMessage ?? "No match")
        undo.undo()
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(0))
        try await waitUntil { model.previewCurrent || model.error != nil }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertFalse(model.placingExposure, "Undoing a match must not start another")
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(0))
    }

    /// Optional: a RAW without an embedded preview (the Sigma fp's DNG on
    /// raw.pixls.us). The automatic match fails quietly: the button stays off
    /// for this photograph, edits included, with the reason under it.
    func testRawWithoutPreviewDisablesMatchingForThePhoto() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW_NO_PREVIEW"] else { throw XCTSkip("Set NEGS_TEST_RAW_NO_PREVIEW") }
        let defaults = UserDefaults.standard, savedPreference = defaults.object(forKey: "autoPlaceNewImages")
        defer { defaults.set(savedPreference, forKey: "autoPlaceNewImages") }
        let model = EditorModel()
        defer { model.shutdown() }
        model.autoPlaceNewImages = true
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await waitUntil { model.exposurePlacementFailureMessage != nil || model.error != nil }
        try await waitUntil { model.previewCurrent && !model.placingExposure || model.error != nil }
        XCTAssertNil(model.error)
        XCTAssertEqual(model.exposurePlacementFailureMessage, "This RAW has no camera preview to match.")
        XCTAssertNil(model.exposurePlacementMessage)
        XCTAssertFalse(model.canMatchCameraBrightness)
        XCTAssertEqual(model.value("prep.exposure_ev"), .number(0))
        model.set("prep.shadows", .number(10))
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertFalse(model.canMatchCameraBrightness, "Another try can't find a preview either")
        XCTAssertNotNil(model.exposurePlacementFailureMessage)
    }

    func testEngineReasonsBecomeShortNotes() {
        // The engine's wording, from exposure_placement.rs.
        let cases: [(String, String, Bool)] = [
            ("Camera preview has too few usable midtone quantiles; exposure unchanged",
             "The camera preview is too dark or too bright to match.", true),
            ("Camera brightness requires exposure outside the supported ±3 EV range",
             "Matching would take more than ±3 EV.", false),
            ("Selected film response is not reliably monotone for Auto exposure",
             "No reliable brightness match with these settings.", false),
        ]
        for (reason, message, forPhoto) in cases {
            let failure = CameraBrightnessReference.failure(EngineError(message: reason))
            XCTAssertEqual(failure.message, message)
            XCTAssertEqual(failure.forPhoto, forPhoto, reason)
        }
        let unusable = CameraBrightnessReference.failure(CameraPreviewUnusable(message: "This RAW has no camera preview to match."))
        XCTAssertEqual(unusable.message, "This RAW has no camera preview to match.")
        XCTAssertTrue(unusable.forPhoto)
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(60)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Timed out waiting for exposure placement")
    }
}
