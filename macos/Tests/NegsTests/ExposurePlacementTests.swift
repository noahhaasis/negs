import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import NegsApp

final class ExposurePlacementTests: XCTestCase {
    /// The preview is read in the colour space the RAW declares (the engine's
    /// `preview_colour_space`), as on the web and Windows: Adobe RGB is
    /// converted, nothing said is sRGB, anything else is refused. A JPEG
    /// with an embedded thumbnail stands in for a RAW's preview.
    func testPreviewIsReadInTheDeclaredColourSpace() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("embedded-thumbnail.jpg")
        let context = try XCTUnwrap(CGContext(data: nil, width: 640, height: 480, bitsPerComponent: 8, bytesPerRow: 2560,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        context.setFillColor(red: 0.2, green: 0.7, blue: 0.3, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: 640, height: 480))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), [kCGImageDestinationEmbedThumbnail: true] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let size = CGSize(width: 640, height: 480)
        func centre(_ declared: PreviewColourSpace?) throws -> [UInt8] {
            let data = try CameraBrightnessReference.png(from: url, expectedSize: size, declared: declared)
            let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil)), 0, nil))
            XCTAssertEqual(image.colorSpace?.name, CGColorSpace.sRGB)
            var pixel = [UInt8](repeating: 0, count: 4)
            let read = try XCTUnwrap(CGContext(data: &pixel, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
            read.draw(image, in: CGRect(x: -image.width / 2, y: -image.height / 2, width: image.width, height: image.height))
            return Array(pixel.prefix(3))
        }
        let (untagged, srgb, adobe) = (try centre(nil), try centre(.srgb), try centre(.adobeRGB))
        XCTAssertEqual(untagged, srgb)
        // Adobe RGB's green is more saturated than the same numbers in sRGB.
        XCTAssertLessThan(Int(adobe[0]), Int(srgb[0]) - 10, "\(adobe) vs \(srgb)")
        XCTAssertThrowsError(try CameraBrightnessReference.png(from: url, expectedSize: size, declared: .other)) {
            XCTAssertEqual(CameraBrightnessReference.failure($0).message, "The camera preview’s colour space isn’t supported.")
            XCTAssertTrue(CameraBrightnessReference.failure($0).forPhoto)
        }
    }

    /// A thumbnail's reference read while the helper decodes is the one read
    /// after: the same bytes, or the same refusal, whatever the reply says.
    func testReferenceReadAheadIsTheSame() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let context = try XCTUnwrap(CGContext(data: nil, width: 640, height: 480, bitsPerComponent: 8, bytesPerRow: 2560,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        for x in stride(from: 0, to: 640, by: 16) {
            context.setFillColor(red: CGFloat(x) / 640, green: 0.7, blue: 0.3, alpha: 1)
            context.fill(CGRect(x: x, y: 0, width: 16, height: 480))
        }
        let embedded = directory.appendingPathComponent("embedded-thumbnail.jpg"), none = directory.appendingPathComponent("none.png")
        try writeImage(try XCTUnwrap(context.makeImage()), to: embedded, type: .jpeg, properties: [kCGImageDestinationEmbedThumbnail: true])
        try writeImage(try XCTUnwrap(context.makeImage()), to: none, type: .png)
        var files = [(embedded, CGSize(width: 640, height: 480)), (none, CGSize(width: 640, height: 480))]
        if let path = ProcessInfo.processInfo.environment["SPEKTRAFILM_RAW_SAMPLE"] {
            let url = URL(fileURLWithPath: path)
            let properties = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil)), 0, nil) as? [CFString: Any])
            files.append((url, CGSize(width: try XCTUnwrap(properties[kCGImagePropertyPixelWidth] as? Double),
                                      height: try XCTUnwrap(properties[kCGImagePropertyPixelHeight] as? Double))))
        }
        func outcome(_ make: () throws -> Data) -> String {
            do { return "png \(try make().base64EncodedString())" } catch { return "refused: \(CameraBrightnessReference.failure(error).message)" }
        }
        for (url, size) in files {
            let prepared = CameraBrightnessReference.Prepared(from: url)
            let sizes = [size, CGSize(width: size.width + 40, height: size.height), CGSize(width: size.height, height: size.width), .zero]
            for expected in sizes {
                for declared in [nil, .srgb, .adobeRGB, .other] as [PreviewColourSpace?] {
                    let now = outcome { try CameraBrightnessReference.png(from: url, expectedSize: expected, declared: declared) }
                    XCTAssertEqual(outcome { try prepared.png(expectedSize: expected, declared: declared) }, now,
                                   "\(url.lastPathComponent) \(expected) \(String(describing: declared))")
                    if url != none, expected == size, declared != .other { XCTAssertTrue(now.hasPrefix("png "), url.lastPathComponent) }
                }
            }
        }
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

    private func writeImage(_ image: CGImage, to url: URL, type: UTType, properties: [CFString: Any] = [:]) throws {
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }
    private func gradient(width: Int, height: Int, space: CGColorSpace, alpha: Bool = false) throws -> CGImage {
        let context = try XCTUnwrap(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: space, bitmapInfo: (alpha ? CGImageAlphaInfo.premultipliedLast : .noneSkipLast).rawValue))
        for x in 0..<width {
            context.setFillColor(CGColor(colorSpace: space, components: [CGFloat(x) / CGFloat(width), 0.5, 0.3, alpha ? 0.5 : 1])!)
            context.fill(CGRect(x: x, y: 0, width: 1, height: height))
        }
        return try XCTUnwrap(context.makeImage())
    }

    /// Non-RAW files are their own reference: any profile, read into a
    /// bounded sRGB PNG; the EXIF orientation is applied, as the engine does.
    func testFileReferenceReadsProfilesAndOrientation() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let p3 = directory.appendingPathComponent("p3.jpg")
        try writeImage(try gradient(width: 1200, height: 800, space: CGColorSpace(name: CGColorSpace.displayP3)!), to: p3, type: .jpeg)
        let data = try CameraBrightnessReference.filePNG(from: p3, expectedSize: CGSize(width: 1200, height: 800))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil)), 0, nil))
        XCTAssertEqual([image.width, image.height], [512, 341])
        XCTAssertEqual(image.colorSpace?.name, CGColorSpace.sRGB)
        // Stored landscape, shown portrait (orientation 6): the engine's source is portrait.
        let rotated = directory.appendingPathComponent("rotated.jpg")
        try writeImage(try gradient(width: 600, height: 400, space: CGColorSpace(name: CGColorSpace.sRGB)!), to: rotated, type: .jpeg,
                       properties: [kCGImagePropertyOrientation: 6])
        XCTAssertNoThrow(try CameraBrightnessReference.filePNG(from: rotated, expectedSize: CGSize(width: 400, height: 600)))
        XCTAssertThrowsError(try CameraBrightnessReference.filePNG(from: rotated, expectedSize: CGSize(width: 600, height: 400)))
    }

    func testTransparentFilesAreNotMatched() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("alpha.png")
        try writeImage(try gradient(width: 200, height: 100, space: CGColorSpace(name: CGColorSpace.sRGB)!, alpha: true), to: url, type: .png)
        XCTAssertThrowsError(try CameraBrightnessReference.filePNG(from: url, expectedSize: CGSize(width: 200, height: 100))) {
            XCTAssertEqual(CameraBrightnessReference.failure($0).message, "Transparent images can’t be matched.")
            XCTAssertTrue(CameraBrightnessReference.failure($0).forPhoto)
        }
    }
}

@MainActor final class ExposurePlacementLifecycleTests: XCTestCase {
    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    /// Settings for a test: auto-match on, restored afterwards.
    private func preferences(_ model: EditorModel, match: Bool = true) -> () -> Void {
        let defaults = UserDefaults.standard
        let saved = ["autoPlaceNewImages"].map { ($0, defaults.object(forKey: $0)) }
        model.autoPlaceNewImages = match
        return { for (key, value) in saved { defaults.set(value, forKey: key) } }
    }
    private func settled(_ model: EditorModel) async throws {
        try await waitUntil { model.error != nil || model.previewCurrent && !model.matchingBrightness && !model.brightnessMatchPending }
        try await Task.sleep(for: .milliseconds(100))
        try await waitUntil { model.error != nil || model.previewCurrent && !model.matchingBrightness && !model.brightnessMatchPending }
        XCTAssertNil(model.error)
    }
    private func offset(_ model: EditorModel) -> Double { model.exposureOffset.wrappedValue }
    private func total(_ model: EditorModel) -> Double { model.value("prep.exposure_ev").double }

    func testNewRawIsMatchedAndTheMatchIsSavedWithTheSession() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        // A copy, so its sidecar isn't written beside the sample.
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let raw = dir.appendingPathComponent(URL(fileURLWithPath: path).lastPathComponent)
        try FileManager.default.copyItem(at: URL(fileURLWithPath: path), to: raw)
        let sidecars = SidecarStore()
        let model = EditorModel(sidecars: sidecars)
        defer { model.shutdown() }
        let restore = preferences(model)
        defer { restore() }
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(raw)
        XCTAssertTrue(model.matchesBrightness)
        try await settled(model)
        let ev = try XCTUnwrap(model.state.brightnessMatch?.ev)
        XCTAssertEqual(total(model), ev, accuracy: 1e-9)
        XCTAssertEqual(offset(model), 0, accuracy: 1e-9)
        XCTAssertFalse(model.exposureOffsetModified)
        XCTAssertTrue(model.brightnessMatchNote?.hasPrefix("Starts at") == true, model.brightnessMatchNote ?? "No note")
        // Prep edits never re-solve.
        model.set("prep.shadows", .number(30))
        model.exposureOffset.wrappedValue = 0.5
        try await settled(model)
        XCTAssertEqual(model.state.brightnessMatch?.ev, ev)
        XCTAssertEqual(offset(model), 0.5, accuracy: 1e-9)
        XCTAssertTrue(model.exposureOffsetModified)

        let sessionRead = await relaunchSession(of: model, store: sidecars, data: paths().data)
        let session = try XCTUnwrap(sessionRead)
        let restored = EditorModel(sidecars: sidecars)
        defer { restored.shutdown() }
        await restored.start(paths: paths(), restoration: Task { session })
        try await settled(restored)
        XCTAssertEqual(restored.state.brightnessMatch, model.state.brightnessMatch, "A current match restores without a new solve")
        XCTAssertEqual(total(restored), total(model), accuracy: 1e-9)
    }

    /// Changing the film re-solves the match and keeps the user's exposure
    /// above it; Undo restores the film together with its match.
    func testFilmChangeReSolvesKeepingTheOffsetAndUndoRestoresBoth() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        let model = EditorModel()
        defer { model.shutdown() }
        let restore = preferences(model)
        defer { restore() }
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await settled(model)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        undo.beginUndoGrouping(); model.exposureOffset.wrappedValue = 0.3; undo.endUndoGrouping()
        let before = model.state
        undo.beginUndoGrouping(); model.choosePaper("kodak_2302"); undo.endUndoGrouping()
        try await settled(model)
        let match = try XCTUnwrap(model.state.brightnessMatch)
        XCTAssertEqual(match.inputs, model.matchInputs(model.state))
        XCTAssertEqual(offset(model), 0.3, accuracy: 1e-9)
        XCTAssertGreaterThan(abs((match.ev ?? 0) - (before.brightnessMatch?.ev ?? 0)), 0.05, "This paper prints much brighter")
        undo.undo()
        XCTAssertEqual(model.state, before)
        try await settled(model)
        XCTAssertEqual(model.state, before, "A restored state with a current match isn't solved again")
    }

    /// An edit made while a solve is pending, then undone: the restored
    /// state's match is stale, so it is solved again and stays consistent.
    func testEditDuringASolveAndItsUndoConverge() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        let model = EditorModel()
        defer { model.shutdown() }
        let restore = preferences(model)
        defer { restore() }
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await settled(model)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        undo.beginUndoGrouping(); model.choosePaper("kodak_2302"); undo.endUndoGrouping()
        undo.beginUndoGrouping(); model.set("prep.highlights", .number(-40)); undo.endUndoGrouping()
        try await settled(model)
        let solved = try XCTUnwrap(model.state.brightnessMatch)
        XCTAssertEqual(solved.inputs, model.matchInputs(model.state))
        undo.undo()
        XCTAssertEqual(model.value("prep.highlights"), .number(0))
        try await settled(model)
        XCTAssertEqual(model.state.brightnessMatch?.inputs, model.matchInputs(model.state))
        XCTAssertEqual(offset(model), 0, accuracy: 1e-9)
        XCTAssertEqual(model.state.brightnessMatch?.ev ?? 0, solved.ev ?? 0, accuracy: 0.05)
    }

    func testTurningMatchingOffKeepsTheBrightnessAndShowsTheMeter() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW"] else { throw XCTSkip("Set NEGS_TEST_RAW") }
        let model = EditorModel()
        defer { model.shutdown() }
        let restore = preferences(model)
        defer { restore() }
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await settled(model)
        let matched = total(model)
        model.setMatchesBrightness(false)
        XCTAssertFalse(model.brightnessMatchActive)
        XCTAssertEqual(total(model), matched, accuracy: 1e-9)
        XCTAssertEqual(offset(model), matched, accuracy: 1e-9, "Off, Exposure shows all of it")
        model.choosePaper("kodak_2302")
        try await settled(model)
        XCTAssertEqual(total(model), matched, accuracy: 1e-9, "Off, nothing re-solves")
        model.setMatchesBrightness(true)
        XCTAssertEqual(total(model), 0)
        try await settled(model)
        XCTAssertEqual(offset(model), 0, accuracy: 1e-9)
        XCTAssertNotNil(model.state.brightnessMatch?.ev)
    }

    /// Optional: a RAW without an embedded preview (the Sigma fp's DNG on
    /// raw.pixls.us). Matching is unavailable for the photograph, with the
    /// reason, and the meter shows.
    func testRawWithoutPreviewDisablesMatchingForThePhoto() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_RAW_NO_PREVIEW"] else { throw XCTSkip("Set NEGS_TEST_RAW_NO_PREVIEW") }
        let model = EditorModel()
        defer { model.shutdown() }
        let restore = preferences(model)
        defer { restore() }
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await settled(model)
        XCTAssertEqual(model.brightnessMatchUnavailable, "This RAW has no camera preview to match.")
        XCTAssertFalse(model.brightnessMatchActive)
        XCTAssertTrue(model.canSave)
        XCTAssertEqual(total(model), 0)
        model.set("prep.shadows", .number(10))
        try await settled(model)
        XCTAssertNotNil(model.brightnessMatchUnavailable, "Another try can't find a preview either")
    }

    /// Optional: a real JPEG (`NEGS_TEST_JPEG`) is matched to itself.
    func testJPEGIsMatchedToItself() async throws {
        guard let path = ProcessInfo.processInfo.environment["NEGS_TEST_JPEG"] else { throw XCTSkip("Set NEGS_TEST_JPEG") }
        let model = EditorModel()
        defer { model.shutdown() }
        let restore = preferences(model)
        defer { restore() }
        await model.start(paths: paths(), restoration: Task { nil })
        model.setLongestSide(512)
        model.open(URL(fileURLWithPath: path))
        try await settled(model)
        XCTAssertNil(model.brightnessMatchUnavailable)
        XCTAssertNotNil(model.state.brightnessMatch?.ev, model.brightnessMatchNote ?? "No note")
        XCTAssertTrue(model.brightnessMatchNote?.hasSuffix("match the original.") == true
                      || model.brightnessMatchNote?.contains("held back") == true, model.brightnessMatchNote ?? "No note")
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
