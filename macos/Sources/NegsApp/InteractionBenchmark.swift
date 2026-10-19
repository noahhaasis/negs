#if SPEKTRAFILM_BENCHMARK
import AppKit

/// Drags a slider on the real event loop and reports how long each main
/// run-loop pass stays busy, which is what a user feels as a hitch.
/// Compiled only with -DSPEKTRAFILM_BENCHMARK; see docs/native-macos-ui.md.
@MainActor enum InteractionBenchmark {
    static func start(_ model: EditorModel) {
        guard let photo = ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] else { return }
        let launched = Date()
        var phase = 0, dragStart = Date(), frames = 0, last: IOSurface?
        var busy: [Double] = [], awake = CFAbsoluteTimeGetCurrent(), measuring = false
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
                    report("opened and developed in \(Int(Date().timeIntervalSince(launched) * 1000)) ms after launch")
                    model.beginInteraction(); dragStart = Date(); last = model.image; measuring = true; phase = 2
                case 2:
                    let t = Date().timeIntervalSince(dragStart)
                    if model.image !== last { frames += 1; last = model.image }
                    if t < 4 {
                        model.set("enlarger.print_exposure", .number(EditorModel.snap(1 + 0.5 * sin(t * 2), to: 0.1...5, step: 0.01)))
                    } else {
                        measuring = false; model.endInteraction(); phase = 3
                    }
                case 3 where model.previewCurrent:
                    timer.invalidate()
                    busy.sort()
                    let pct = { (p: Int) in Int(busy[min(busy.count - 1, busy.count * p / 100)].rounded()) }
                    report("4 s slider drag: \(frames) live previews, main run-loop busy p50 \(pct(50)) ms, p95 \(pct(95)) ms, p99 \(pct(99)) ms, worst \(Int(busy.last ?? 0)) ms over \(busy.count) passes")
                    NSApp.terminate(nil)
                default: break
                }
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    private static func report(_ line: String) {
        FileHandle.standardError.write(Data("benchmark: \(line)\n".utf8))
    }
}
#endif
