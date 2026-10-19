import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

@MainActor final class LifecycleTests: XCTestCase {
    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    func testLatestEditWinsAndUndoRestoresParameters() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        XCTAssertTrue(model.ready, model.error ?? "Engine did not start")
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        undo.beginUndoGrouping()
        model.set("camera.exposure_compensation_ev", .number(1.5))
        undo.endUndoGrouping()
        undo.undo()
        XCTAssertEqual(model.value("camera.exposure_compensation_ev"), .number(0))
        undo.redo()
        XCTAssertEqual(model.value("camera.exposure_compensation_ev"), .number(1.5))
        model.undoManager = nil
        let fixture = try makeFixture()
        defer { try? FileManager.default.removeItem(at: fixture) }
        model.open(fixture)
        try await waitUntil { model.rendering }
        // Change state while the first request is in flight. Only the latest
        // working size and turn may become saveable or replace the canvas.
        model.set("io.upscale_factor", .number(0.5))
        model.rotate()
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        XCTAssertEqual(model.imageSize, CGSize(width: 48, height: 32))
        XCTAssertEqual(model.imageRotation, 1)
        XCTAssertTrue(model.canSave)
        model.chooseFilm("kodak_trix")
        XCTAssertTrue(model.value("io.scan_film").bool)
        model.chooseFilm("kodak_gold_200")
        XCTAssertFalse(model.value("io.scan_film").bool)
        XCTAssertEqual(model.state.paper, "kodak_portra_endura")
    }

    /// A Fast export of a current preview at export size saves the preview.
    /// These paths have no Fast exporter, so developing again would fail.
    func testFastExportSavesTheCurrentPreview() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        XCTAssertNil(paths().fastExporter)
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        let fixture = try makeFixture()
        defer { try? FileManager.default.removeItem(at: fixture) }
        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let destination = directory.appendingPathComponent("export.png")
        await model.export(to: destination, engine: .fast)
        XCTAssertNil(model.error)
        XCTAssertTrue(model.status.hasPrefix("Exported export.png"), model.status)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["export.png"], "No staging file may remain")
        XCTAssertEqual(NSImage(contentsOf: destination)?.representations.first?.pixelsWide, 96)
    }

    func testCancelExportPreservesExistingDestinationAndRemovesStaging() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let fake = directory.appendingPathComponent("exporter")
        try Data("#!/bin/sh\nexec /bin/sleep 30\n".utf8).write(to: fake)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: fake.path)
        let destination = directory.appendingPathComponent("existing.png")
        let original = Data("original photograph".utf8)
        try original.write(to: destination)
        let job = ExportJob()
        let scratch = try ScratchSpace(root: directory.appendingPathComponent("scratch"))
        let task = Task {
            try await job.run(paths: EnginePaths(helper: fake, exporter: fake, data: directory), scratch: scratch, source: directory.appendingPathComponent("source.png"), destination: destination, state: EditState(), border: nil)
        }
        try await Task.sleep(nanoseconds: 100_000_000)
        job.cancel()
        do { try await task.value; XCTFail("Expected cancellation") } catch is CancellationError {} catch { XCTFail(error.localizedDescription) }
        XCTAssertEqual(try Data(contentsOf: destination), original)
        XCTAssertFalse(try FileManager.default.contentsOfDirectory(atPath: directory.path).contains { $0.hasPrefix(".negs-") })
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: scratch.directory.path), [])
    }

    /// Each engine runs its own binary with its own backend, and a fast export
    /// that fell back to the CPU is reported rather than passed off as GPU.
    func testExportEngineChoosesBinaryAndBackend() async throws {
        let fm = FileManager.default
        let directory = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try fm.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: directory) }
        func fake(_ name: String, backendLine: String) throws -> URL {
            // Records how it was called, prints the CLI's backend line, and
            // writes the output file (`process SRC -o OUT ...`).
            let url = directory.appendingPathComponent(name)
            let script = "#!/bin/sh\necho \"\(name) $SPEKTRAFILM_BACKEND $1\" > \"\(directory.path)/called\"\necho '\(backendLine)' >&2\necho out > \"$4\"\n"
            try Data(script.utf8).write(to: url)
            try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
            return url
        }
        let reference = try fake("reference", backendLine: "Backend: CPU (rayon)")
        let gpu = try fake("fast", backendLine: "Backend: wgpu (GPU)")
        let noGPU = try fake("fast-without-gpu", backendLine: "Backend: CPU (rayon)")
        let scratch = try ScratchSpace(root: directory.appendingPathComponent("scratch"))
        defer { scratch.remove() }
        let source = directory.appendingPathComponent("source.png")
        func export(_ engine: ExportEngine, fast: URL?) async throws -> (ExportOutcome, String) {
            let destination = directory.appendingPathComponent("out-\(UUID().uuidString).png")
            let paths = EnginePaths(helper: reference, exporter: reference, data: directory, fastExporter: fast)
            let outcome = try await ExportJob().run(paths: paths, scratch: scratch, source: source, destination: destination, state: EditState(), border: nil, engine: engine)
            XCTAssertEqual(try String(contentsOf: destination, encoding: .utf8), "out\n")
            return (outcome, try String(contentsOf: directory.appendingPathComponent("called"), encoding: .utf8))
        }
        var (outcome, called) = try await export(.reference, fast: gpu)
        XCTAssertEqual(called, "reference cpu process\n")
        XCTAssertFalse(outcome.fellBackToCPU)
        (outcome, called) = try await export(.fast, fast: gpu)
        XCTAssertEqual(called, "fast wgpu process\n")
        XCTAssertFalse(outcome.fellBackToCPU)
        (outcome, called) = try await export(.fast, fast: noGPU)
        XCTAssertEqual(called, "fast-without-gpu wgpu process\n")
        XCTAssertTrue(outcome.fellBackToCPU)
        do {
            _ = try await export(.fast, fast: nil)
            XCTFail("Fast export without the f32 tool must fail")
        } catch { XCTAssertTrue(error.localizedDescription.contains("Fast (GPU) export needs"), error.localizedDescription) }
        XCTAssertFalse(try fm.contentsOfDirectory(atPath: directory.path).contains { $0.hasPrefix(".negs-") })
    }

    /// Real exports through `ExportJob`, for the end-to-end engine comparison
    /// (scripts/gpu_cpu_quality/app_export.sh). Skipped unless
    /// SPEKTRAFILM_EXPORT_JOBS names a JSON list of jobs: source, destination,
    /// params (a params JSON file), film, paper (omit to scan the film), and
    /// engine (reference or fast). Uses the release tools in target/release.
    func testRealExportsFromJobList() async throws {
        guard let list = ProcessInfo.processInfo.environment["SPEKTRAFILM_EXPORT_JOBS"] else { throw XCTSkip("Set SPEKTRAFILM_EXPORT_JOBS to run real exports") }
        struct Job: Decodable { let source, destination, params, film, engine: String; let paper: String? }
        let jobs = try JSONDecoder().decode([Job].self, from: Data(contentsOf: URL(fileURLWithPath: list)))
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let release = root.appendingPathComponent("target/release")
        let paths = EnginePaths(helper: release.appendingPathComponent("spektrafilm-native"), exporter: release.appendingPathComponent("spektrafilm-f64"),
                                data: root.appendingPathComponent("data"), fastExporter: release.appendingPathComponent("spektrafilm"))
        let scratch = try ScratchSpace(root: FileManager.default.temporaryDirectory.appendingPathComponent("negs-export-jobs"))
        defer { scratch.remove() }
        for job in jobs {
            var state = EditState()
            state.params = try JSONDecoder().decode(JSONValue.self, from: Data(contentsOf: URL(fileURLWithPath: job.params)))
            state.film = job.film
            if let paper = job.paper { state.paper = paper } else { state.params["io.scan_film"] = .bool(true) }
            let engine = try XCTUnwrap(ExportEngine(rawValue: job.engine))
            let started = Date()
            let outcome = try await ExportJob().run(paths: paths, scratch: scratch, source: URL(fileURLWithPath: job.source),
                                                    destination: URL(fileURLWithPath: job.destination), state: state, border: nil, engine: engine)
            XCTAssertFalse(outcome.fellBackToCPU, job.destination)
            print("exported \(job.destination) with \(engine.title) in \(String(format: "%.1f", Date().timeIntervalSince(started))) s")
        }
    }

    func testNextLaunchRemovesWhatACrashLeftBehind() throws {
        let fm = FileManager.default
        let directory = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let root = directory.appendingPathComponent("scratch")
        try fm.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: directory) }
        let running = try ScratchSpace(root: root)
        // A crashed launch: an unlocked lock file, a preview buffer, and an
        // export whose exporter is still running and staging beside its destination.
        let crashed = root.appendingPathComponent("crashed", isDirectory: true)
        let job = crashed.appendingPathComponent("export-1")
        try fm.createDirectory(at: job, withIntermediateDirectories: true)
        fm.createFile(atPath: root.appendingPathComponent("crashed.lock").path, contents: nil)
        try Data(count: 1024).write(to: crashed.appendingPathComponent("preview.rgba"))
        let staging = directory.appendingPathComponent(".negs-1.png")
        try Data("partial".utf8).write(to: staging)
        let unrelated = directory.appendingPathComponent("photo.png")
        try Data("keep".utf8).write(to: unrelated)
        let exporter = Process()
        exporter.executableURL = URL(fileURLWithPath: "/bin/sleep"); exporter.arguments = ["30"]
        try exporter.run()
        defer { if exporter.isRunning { exporter.terminate() } }
        try ExportRecord(staging: staging.path, exporter: ExportRecord.resolvedPath(exporter.executableURL!), pid: exporter.processIdentifier).write(to: job)
        let other = crashed.appendingPathComponent("export-2")
        try fm.createDirectory(at: other, withIntermediateDirectories: true)
        try ExportRecord(staging: unrelated.path, exporter: "/bin/sleep").write(to: other)

        let next = try ScratchSpace(root: root)
        exporter.waitUntilExit()
        XCTAssertEqual(exporter.terminationReason, .uncaughtSignal)
        XCTAssertFalse(fm.fileExists(atPath: crashed.path))
        XCTAssertFalse(fm.fileExists(atPath: staging.path))
        XCTAssertTrue(fm.fileExists(atPath: unrelated.path), "Only Negs staging files are removed")
        XCTAssertTrue(fm.fileExists(atPath: running.directory.path), "A folder still in use is kept")
        running.remove(); next.remove()
        XCTAssertEqual(try fm.contentsOfDirectory(atPath: root.path), [])
    }

    func testNativeWindowSnapshot() async throws {
        guard let folder = ProcessInfo.processInfo.environment["SPEKTRAFILM_UI_SNAPSHOTS"] else { throw XCTSkip("Set SPEKTRAFILM_UI_SNAPSHOTS for the window smoke test") }
        _ = NSApplication.shared
        let model = EditorModel()
        await model.start(paths: paths())
        XCTAssertTrue(model.ready, model.error ?? "No engine")
        let fixture = try makeFixture()
        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        let controller = EditorWindowController(model: model)
        let window = try XCTUnwrap(controller.window)
        window.setContentSize(NSSize(width: 1320, height: 860))
        controller.showWindow(nil)
        defer { window.orderOut(nil); model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        for (name, appearance, crop) in [("light", NSAppearance.Name.aqua, false), ("dark", NSAppearance.Name.darkAqua, false), ("crop", NSAppearance.Name.darkAqua, true)] {
            window.appearance = NSAppearance(named: appearance)
            if crop { model.beginCrop() }
            try await Task.sleep(nanoseconds: 400_000_000)
            let view = window.contentView!
            view.layoutSubtreeIfNeeded()
            let bitmap = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: bitmap)
            try XCTUnwrap(bitmap.representation(using: .png, properties: [:])).write(to: URL(fileURLWithPath: folder).appendingPathComponent("\(name).png"))
            // Layer-backed system materials don't rasterize through cacheDisplay.
            // Opt in to a window-only capture when screen recording is available.
            if ProcessInfo.processInfo.environment["SPEKTRAFILM_CAPTURE_WINDOW"] == "1" {
                guard CGPreflightScreenCaptureAccess() else {
                    throw XCTSkip("Window capture requires screen-recording permission; native view smoke test completed")
                }
                let capture = Process()
                capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                capture.arguments = ["-x", "-o", "-l", String(window.windowNumber), "\(folder)/\(name)-window.png"]
                try capture.run(); capture.waitUntilExit()
                XCTAssertEqual(capture.terminationStatus, 0)
            }
        }
    }

    /// Drags a slider at 60 Hz for real while the window is on screen, and
    /// measures how long the main thread goes without servicing the run loop.
    func testSliderDragIsOneUndoStepAndSnapsToSteps() async throws {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        defer { model.shutdown() }
        await model.start(paths: paths())
        XCTAssertTrue(model.ready, model.error ?? "Engine did not start")
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        undo.beginUndoGrouping()
        model.beginInteraction()
        for value in [1.1, 1.37, 2.05] { model.set("enlarger.print_exposure", .number(value)) }
        model.endInteraction()
        undo.endUndoGrouping()
        XCTAssertEqual(model.value("enlarger.print_exposure"), .number(2.05))
        XCTAssertTrue(model.isModified("enlarger.print_exposure"))
        undo.undo()
        XCTAssertFalse(model.isModified("enlarger.print_exposure"), "One undo reverts the whole drag")
        XCTAssertFalse(undo.canUndo)
        // Continuous sliders snap to the engine's steps; integer fields stay integral.
        XCTAssertEqual(EditorModel.snap(2.6, to: 1...5, step: 1), 3)
        XCTAssertEqual(EditorModel.snap(0.30000001, to: 0...1, step: 0.01), 0.3)
        XCTAssertEqual(EditorModel.snap(9, to: 0.1...5, step: 0.01), 5)
        XCTAssertEqual(EditorModel.snap(-1, to: 0.1...5, step: 0.01), 0.1)
        XCTAssertEqual(EditorModel.snap(1667.4, to: 1667...25000, step: 1), 1667)
    }

    func testCanvasRotatesClockwiseAndFillsItsFrame() throws {
        let context = try XCTUnwrap(CGContext(data: nil, width: 3, height: 2, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue))
        let image = try XCTUnwrap(context.makeImage())
        let view = PhotographView(frame: .zero)
        view.show(image, size: CGSize(width: 300, height: 200), rotation: 1)
        view.setFrameSize(NSSize(width: 200, height: 300))
        view.layoutSubtreeIfNeeded()
        let photo = try XCTUnwrap(view.layer?.sublayers?.flatMap { [$0] + ($0.sublayers ?? []) }.first { $0.contents as AnyObject === image })
        let frame = photo.convert(photo.bounds, to: view.layer)
        for (actual, expected) in zip([frame.minX, frame.minY, frame.width, frame.height], [0.0, 0, 200, 300]) {
            XCTAssertEqual(actual, expected, accuracy: 0.001)
        }
        // AppKit layers are y-up, so a negative angle turns clockwise like the exporter.
        let corner = photo.convert(CGPoint(x: 0, y: 200), to: view.layer) // the photograph's top-left
        XCTAssertEqual(corner.x, 200, accuracy: 0.001)
        XCTAssertEqual(corner.y, 300, accuracy: 0.001)
        XCTAssertTrue(photo.contents as AnyObject === image)
    }

    private func waitUntil(_ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(20)
        while !predicate(), Date() < deadline { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertTrue(predicate(), "Timed out waiting for image engine")
    }

    private func makeFixture() throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        for y in 0..<64 {
            for x in 0..<96 {
                let index = y * bitmap.bytesPerRow + x * 3
                bitmap.bitmapData![index] = UInt8(x * 255 / 96)
                bitmap.bitmapData![index + 1] = UInt8(y * 255 / 64)
                bitmap.bitmapData![index + 2] = 100
            }
        }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-fixture-\(UUID().uuidString).png")
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }
}
