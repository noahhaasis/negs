#if SPEKTRAFILM_BENCHMARK
import AppKit
import Darwin

/// Measures the film/paper previews in the real app (experiment only, remove
/// with the other stock-preview hooks): NEGS_STOCK_MEASURE=PHOTO opens it,
/// times the first develop and, with NEGS_STOCK_EXPAND=film,paper, each
/// batch of thumbnails (first open, after an exposure edit, 80 % + 3° and
/// 50 % crops), and reports the app's and helper's memory. NEGS_STOCK_SHOTS
/// names a folder for a window screenshot. Compiled only with
/// -DSPEKTRAFILM_BENCHMARK.
@MainActor enum StockPreviewBenchmark {
    static func start(_ model: EditorModel) {
        let environment = ProcessInfo.processInfo.environment
        guard let photo = environment["NEGS_STOCK_MEASURE"] else { return }
        let shots = environment["NEGS_STOCK_SHOTS"].map { URL(fileURLWithPath: $0) }
        let launched = Date()
        Task { @MainActor in
            await until { model.ready }
            report("ready \(ms(launched)) ms after launch")
            let previews = model.previews
            previews.lastBatch = nil
            model.open(URL(fileURLWithPath: photo))
            let opened = Date()
            await until { model.previewCurrent }
            report("developed \(ms(opened)) ms after open (\(ms(launched)) ms after launch) · \(model.status)")
            let open = previews.enabled && (previews.filmOpen || previews.paperOpen)
            if open { await batch(previews, "first open", since: opened) }
            if let shots { capture(shots.appendingPathComponent("first-open.png")) }
            await settle(); memory("settled after opening")
            guard open else { exit(0) }

            previews.lastBatch = nil
            model.set("camera.exposure_compensation_ev", .number(0.5))
            let edited = Date()
            await until { model.previewCurrent }
            report("exposure edit developed in \(ms(edited)) ms")
            await batch(previews, "after an exposure edit", since: edited)
            if let shots { capture(shots.appendingPathComponent("after-edit.png")) }

            for (label, rect, angle) in [("crop 80% + 3°", CGRect(x: 0.1, y: 0.1, width: 0.8, height: 0.8), 3.0),
                                         ("crop 50%", CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5), 0.0)] {
                previews.lastBatch = nil
                model.beginCrop()
                model.cropSession?.rect = rect
                model.cropSession?.beginStraightening(); model.cropSession?.straighten(to: angle); model.cropSession?.endStraightening()
                let cropped = Date()
                model.endCrop(apply: true)
                await batch(previews, "\(label) (\(String(format: "%.2f", model.crop.rect.width)) wide, \(model.crop.angle)°)", since: cropped)
            }
            if let shots { capture(shots.appendingPathComponent("cropped.png")) }
            await settle(); memory("settled with the grids open")
            exit(0)
        }
    }

    private static func until(_ condition: () -> Bool) async {
        while !condition() { try? await Task.sleep(for: .milliseconds(2)) }
    }
    private static func batch(_ previews: Previews, _ label: String, since: Date) async {
        await until { previews.lastBatch != nil }
        let batch = previews.lastBatch!
        report("\(label): \(batch.count) thumbnails in \(Int(batch.ms)) ms (\(Int(batch.ms / Double(max(batch.count, 1)))) ms each), \(ms(since)) ms after the change")
    }
    private static func settle() async { try? await Task.sleep(for: .seconds(8)) }
    private static func ms(_ since: Date) -> Int { Int(Date().timeIntervalSince(since) * 1000) }

    /// Physical footprints, as Activity Monitor's Memory column shows them.
    private static func memory(_ label: String) {
        func footprint(_ pid: pid_t) -> Int {
            var info = rusage_info_v4()
            let status = withUnsafeMutablePointer(to: &info) {
                $0.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V4, $0) }
            }
            return status == 0 ? Int(info.ri_phys_footprint) >> 20 : -1
        }
        var children = [pid_t](repeating: 0, count: 16)
        let count = Int(proc_listchildpids(getpid(), &children, Int32(children.count * MemoryLayout<pid_t>.size)))
        let helpers = children.prefix(max(count, 0)).filter { $0 > 0 }.map { footprint($0) }
        report("memory \(label): app \(footprint(getpid())) MB, helper \(helpers.map(String.init).joined(separator: "+")) MB")
    }

    private static func capture(_ url: URL) {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) else { return }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        task.arguments = ["-x", "-o", "-l", String(window.windowNumber), url.path]
        try? task.run(); task.waitUntilExit()
        report("screenshot \(url.path)")
    }

    private static func report(_ line: String) {
        FileHandle.standardError.write(Data("stock-measure: \(line)\n".utf8))
    }
}
#endif
