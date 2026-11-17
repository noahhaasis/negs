import AppKit
import XCTest
@testable import NegsApp

/// Prep › Film base › Flat field (ticket 33, §9.4): the light frame a
/// negative's scans are divided by, kept in its convert settings, and the
/// helper's word on whether this frame used it.
@MainActor final class FlatFieldTests: XCTestCase {
    // MARK: The row

    func testOffShowsOffAndNoHint() {
        let flat = NegativeScan.FlatField(path: "")
        XCTAssertFalse(flat.isOn)
        XCTAssertEqual(flat.value, "Off")
        XCTAssertNil(flat.hint)
        // An answer means nothing while it's off.
        let answered = NegativeScan.FlatField(path: "", answer: .object(["path": .string(""), "used": .bool(false), "reason": .string("x")]))
        XCTAssertNil(answered.used)
        XCTAssertNil(answered.hint)
    }

    func testOnShowsTheLightFramesNameAndWhyItIsntUsed() {
        let path = "/Rolls/Portra/light.NEF"
        let waiting = NegativeScan.FlatField(path: path)
        XCTAssertTrue(waiting.isOn)
        XCTAssertEqual(waiting.value, "light.NEF")
        XCTAssertNil(waiting.used)
        XCTAssertNil(waiting.hint, "Nothing to say until the helper has")

        let used = NegativeScan.FlatField(path: path, answer: .object(["path": .string(path), "used": .bool(true), "reason": .null]))
        XCTAssertEqual(used.used, true)
        XCTAssertNil(used.hint)

        let reason = "The light frame was shot with a different lens."
        let unused = NegativeScan.FlatField(path: path, answer: .object(["path": .string(path), "used": .bool(false), "reason": .string(reason)]))
        XCTAssertEqual(unused.used, false)
        XCTAssertEqual(unused.hint, reason)

        // An answer about another light frame is stale.
        let stale = NegativeScan.FlatField(path: path, answer: .object(["path": .string("/Rolls/old.NEF"), "used": .bool(false), "reason": .string(reason)]))
        XCTAssertNil(stale.used)
        XCTAssertNil(stale.hint)
    }

    /// It sits in the negative's convert settings: Use for whole roll copies
    /// them, and leaving negative mode puts them back.
    func testTheFlatFieldBelongsToTheNegativesConvertSettings() {
        XCTAssertTrue(NegativeScan.flatFieldPath.hasPrefix("film_render.convert."))
        XCTAssertTrue(NegativeScan.ownPaths.contains("film_render.convert"))
        var state = EditState()
        state.params["workflow.route"] = .string(NegativeScan.route)
        state.params[NegativeScan.flatFieldPath] = .string("/light.NEF")
        NegativeScan.defaults["film_render.convert"] = .object(["scan_illuminant": .string("D55")])
        defer { NegativeScan.defaults = [:] }
        XCTAssertEqual(NegativeScan.leaving(state).params[NegativeScan.flatFieldPath], .null)
    }

    // MARK: With the helper

    /// Choose… and Off are undo steps; the helper divides a scan by a light
    /// frame shot as it was, and says why not for one of another shape.
    func testChoosingAndClearingTheLightFrame() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        XCTAssertTrue(model.ready, model.error ?? "Engine did not start")
        let scan = try fixture(width: 96, height: 64, value: { x, _ in UInt8(200 - x) })
        let light = try fixture(width: 96, height: 64, value: { x, y in UInt8(250 - abs(x - 48) - abs(y - 32)) })
        let square = try fixture(width: 64, height: 64, value: { _, _ in 240 })
        defer { for url in [scan, light, square] { try? FileManager.default.removeItem(at: url) } }
        model.open(scan)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)

        // Only a negative has one.
        model.chooseFlatField(light)
        XCTAssertFalse(model.flatField.isOn)

        model.chooseNegative()
        try await waitUntil { model.negativeBase != nil && model.negativeWorking == nil }
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        undo.beginUndoGrouping(); model.chooseFlatField(light); undo.endUndoGrouping()
        XCTAssertEqual(undo.undoActionName, "Choose Flat Field")
        XCTAssertEqual(model.flatField.value, light.lastPathComponent)
        // The base is read again on the flattened scan, and the render says it was used.
        try await waitUntil { model.negativeWorking == nil && model.previewCurrent && model.flatField.used != nil }
        XCTAssertEqual(model.flatField.used, true, model.flatField.reason ?? "")
        XCTAssertNil(model.flatField.hint)

        undo.beginUndoGrouping(); model.chooseFlatField(square); undo.endUndoGrouping()
        try await waitUntil { model.negativeWorking == nil && model.previewCurrent && model.flatField.used != nil }
        XCTAssertEqual(model.flatField.used, false)
        XCTAssertEqual(model.flatField.hint, "The light frame isn't the shape of this frame.")

        undo.beginUndoGrouping(); model.clearFlatField(); undo.endUndoGrouping()
        XCTAssertEqual(undo.undoActionName, "Flat Field Off")
        XCTAssertFalse(model.flatField.isOn)
        XCTAssertNil(model.flatField.hint)
        undo.undo()
        XCTAssertEqual(model.flatField.value, square.lastPathComponent)
    }

    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    private func waitUntil(_ seconds: TimeInterval = 60, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !predicate(), Date() < deadline { try await Task.sleep(nanoseconds: 20_000_000) }
        XCTAssertTrue(predicate(), "Timed out")
    }

    /// An orange-ish grey PNG, `value` its level at each pixel.
    private func fixture(width: Int, height: Int, value: (Int, Int) -> UInt8) throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: width * 3, bitsPerPixel: 24)!
        for y in 0..<height {
            for x in 0..<width {
                let p = bitmap.bitmapData! + (y * width + x) * 3, v = value(x, y)
                p[0] = v; p[1] = UInt8(Double(v) * 0.7); p[2] = UInt8(Double(v) * 0.45)
            }
        }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-flat-\(UUID().uuidString).png")
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }
}
