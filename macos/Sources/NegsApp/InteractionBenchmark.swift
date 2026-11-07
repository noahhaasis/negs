#if SPEKTRAFILM_BENCHMARK
import AppKit

/// Drags a slider on the real event loop and reports how long each main
/// run-loop pass stays busy, which is what a user feels as a hitch, and how
/// often a new frame arrived. Compiled only with -DSPEKTRAFILM_BENCHMARK;
/// see docs/native-macos-ui.md. SPEKTRAFILM_BENCHMARK_PARAM picks the slider
/// (print exposure or exposure); SPEKTRAFILM_BENCHMARK_SHOTS names a folder
/// for window screenshots during the drag and after letting go.
@MainActor enum InteractionBenchmark {
    static func start(_ model: EditorModel) {
        let environment = ProcessInfo.processInfo.environment
        guard !LaunchBenchmark.enabled, let photo = environment["SPEKTRAFILM_BENCHMARK_PHOTO"] else { return }
        let exposure = environment["SPEKTRAFILM_BENCHMARK_PARAM"] == "camera.exposure_compensation_ev"
        let shots = environment["SPEKTRAFILM_BENCHMARK_SHOTS"].map { URL(fileURLWithPath: $0) }
        let launched = Date()
        var phase = 0, dragStart = Date(), released = Date(), frames = 0, last: IOSurface?, lastFrame = Date(), shot = false
        var busy: [Double] = [], intervals: [Double] = [], awake = CFAbsoluteTimeGetCurrent(), measuring = false
        let observer = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue, true, 0) { _, activity in
            let now = CFAbsoluteTimeGetCurrent()
            if activity == .afterWaiting { awake = now } else if measuring { busy.append((now - awake) * 1000) }
        }
        CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
        let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { timer in
            MainActor.assumeIsolated {
                switch phase {
                case 0 where model.ready:
                    report("editor ready in \(Int(Date().timeIntervalSince(launched) * 1000)) ms after launch")
                    if model.source == nil { model.open(URL(fileURLWithPath: photo)) }
                    phase = 1
                case 1 where model.previewCurrent:
                    report("opened and developed in \(Int(Date().timeIntervalSince(launched) * 1000)) ms after launch · \(model.status)")
                    model.beginInteraction(); dragStart = Date(); lastFrame = dragStart; last = model.image; measuring = true; phase = 2
                case 2:
                    let t = Date().timeIntervalSince(dragStart)
                    if model.image !== last {
                        frames += 1; last = model.image
                        if frames > 1 { intervals.append(Date().timeIntervalSince(lastFrame) * 1000) }
                        lastFrame = Date()
                    }
                    if t > 2, !shot, let shots { shot = true; capture(shots.appendingPathComponent("dragging.png"), status: model.status) }
                    if t < 4 {
                        if exposure {
                            model.set("camera.exposure_compensation_ev", .number(EditorModel.snap(sin(t * 2), to: -5...5, step: 0.1)))
                        } else {
                            model.set("enlarger.print_exposure", .number(EditorModel.snap(1 + 0.5 * sin(t * 2), to: 0.1...5, step: 0.01)))
                        }
                    } else {
                        measuring = false; model.endInteraction(); released = Date(); phase = 3
                    }
                case 3 where model.previewCurrent:
                    timer.invalidate()
                    busy.sort(); intervals.sort()
                    let pct = { (values: [Double], p: Int) in Int(values.isEmpty ? 0 : values[min(values.count - 1, values.count * p / 100)].rounded()) }
                    report("4 s slider drag: \(frames) live previews (\(String(format: "%.1f", Double(frames) / 4)) fps), frame interval p50 \(pct(intervals, 50)) ms, p95 \(pct(intervals, 95)) ms; main run-loop busy p50 \(pct(busy, 50)) ms, p95 \(pct(busy, 95)) ms, p99 \(pct(busy, 99)) ms, worst \(Int(busy.last ?? 0)) ms over \(busy.count) passes")
                    report("released → full frame in \(Int(Date().timeIntervalSince(released) * 1000)) ms · \(model.status)")
                    if let shots { capture(shots.appendingPathComponent("released.png"), status: model.status) }
                    NSApp.terminate(nil)
                default: break
                }
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    /// The editor window only, without its shadow.
    private static func capture(_ url: URL, status: String) {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) else { return }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        task.arguments = ["-x", "-o", "-l", String(window.windowNumber), url.path]
        try? task.run(); task.waitUntilExit()
        report("screenshot \(url.lastPathComponent) · \(status)")
    }

    private static func report(_ line: String) {
        FileHandle.standardError.write(Data("benchmark: \(line)\n".utf8))
    }
}

/// SPEKTRAFILM_BENCHMARK_LAUNCH: times launch from the process's exec, then
/// quits. Window: the commit after the window is shown. Photo: the same, if
/// the photograph was in that first frame, else the first timer tick with it
/// on the canvas. Preview: the first current render. Settled: the end of the
/// last main run-loop pass busy for more than 4 ms before the main thread
/// stays quiet for a second.
@MainActor enum LaunchBenchmark {
    static var enabled: Bool { ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_LAUNCH"] != nil }
    private static var started = 0.0, window = 0.0, photo = 0.0, preview = 0.0, lastBusy = 0.0
    private static weak var model: EditorModel?

    /// Seconds since 1970 when this process was exec'd.
    private static func processStart() -> Double {
        var info = kinfo_proc(), size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0 else { return Date().timeIntervalSince1970 }
        let time = info.kp_proc.p_un.__p_starttime
        return Double(time.tv_sec) + Double(time.tv_usec) / 1e6
    }
    private static func now() -> Double { (Date().timeIntervalSince1970 - started) * 1000 }

    /// Call first thing in applicationWillFinishLaunching.
    static func begin(_ model: EditorModel) {
        guard enabled else { return }
        self.model = model
        started = processStart()
        var awake = now()
        let observer = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue, true, 0) { _, activity in
            let time = now()
            if activity == .afterWaiting { awake = time } else if time - awake > 4 { lastBusy = time }
        }
        CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
        let timer = Timer(timeInterval: 1.0 / 120, repeats: true) { timer in
            MainActor.assumeIsolated {
                let time = now()
                if photo == 0, window > 0, model.image != nil || model.placeholder != nil { photo = time }
                if preview == 0, model.previewCurrent { preview = time }
                guard window > 0, preview > 0 || time > 8000, time - lastBusy > 1000 else { return }
                timer.invalidate()
                FileHandle.standardError.write(Data(String(format: "benchmark: launch window %.0f ms, photo %.0f ms, preview %.0f ms, settled %.0f ms from exec · %@%@\n",
                                                           window, photo, preview, lastBusy, model.status, model.error.map { " · error: \($0)" } ?? "").utf8))
                NSApp.terminate(nil)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    /// Call right after the window is shown.
    static func windowShown() {
        guard enabled else { return }
        let inFirstFrame = model?.image != nil || model?.placeholder != nil
        CATransaction.setCompletionBlock {
            MainActor.assumeIsolated {
                window = now()
                if inFirstFrame { photo = window }
            }
        }
    }
}
#endif
