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

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(60)
        while !predicate(), Date() < deadline { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Timed out waiting for exposure placement")
    }
}
