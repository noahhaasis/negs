#if SPEKTRAFILM_BENCHMARK
import AppKit
import QuartzCore

/// SPEKTRAFILM_BENCHMARK_GRID: opens the grid on the session's folder and
/// scrolls it on the display's own clock, at fixed speeds, reporting how
/// often a frame was missed and how long the main run loop stayed busy,
/// which is what a user feels as a stutter, and the app's memory. The value
/// is the scroll, `speed:seconds,…` in points per second; at either end it
/// turns round. SPEKTRAFILM_BENCHMARK_SHOTS names a folder for a screenshot
/// in the middle of each speed and after the last.
///
/// SPEKTRAFILM_BENCHMARK_GRID_FILL: waits for the first edited thumbnail the
/// background helper makes, copies it to every other edited item's name,
/// and quits, so a later run scrolls with every edited thumbnail on disk
/// without developing them all. Compiled only with -DSPEKTRAFILM_BENCHMARK.
@MainActor final class GridBenchmark: NSObject {
    private let model: EditorModel
    private let segments: [(speed: Double, seconds: Double)]
    private let shots: URL?
    private var phase = 0, phaseStart = Date()
    private var scrollView: NSScrollView?
    private var link: CADisplayLink?
    private var segment = 0, segmentStart = 0.0, lastTimestamp = 0.0, direction = 1.0, position = 0.0, shot = false
    private var intervals: [Double] = [], nominal: [Double] = [], busy: [Double] = []
    private var awake = CFAbsoluteTimeGetCurrent(), measuring = false
    private var peak = 0, startFootprint = 0
    private static var running: GridBenchmark?

    static func start(_ model: EditorModel) {
        let environment = ProcessInfo.processInfo.environment
        guard !LaunchBenchmark.enabled else { return }
        if environment["SPEKTRAFILM_BENCHMARK_GRID_FILL"] != nil { return fill(model) }
        guard let spec = environment["SPEKTRAFILM_BENCHMARK_GRID"] else { return }
        let segments = spec.split(separator: ",").compactMap { part -> (Double, Double)? in
            let values = part.split(separator: ":").compactMap { Double($0) }
            return values.count == 2 ? (values[0], values[1]) : nil
        }
        guard !segments.isEmpty else { return report("SPEKTRAFILM_BENCHMARK_GRID is speed:seconds,…") }
        let benchmark = GridBenchmark(model: model, segments: segments,
                                      shots: environment["SPEKTRAFILM_BENCHMARK_SHOTS"].map { URL(fileURLWithPath: $0) })
        running = benchmark
        benchmark.begin()
    }

    private init(model: EditorModel, segments: [(Double, Double)], shots: URL?) {
        self.model = model
        self.segments = segments.map { (speed: $0.0, seconds: $0.1) }
        self.shots = shots
    }

    private func begin() {
        let launched = Date()
        let observer = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue, true, 0) { [weak self] _, activity in
            MainActor.assumeIsolated {
                guard let self else { return }
                let now = CFAbsoluteTimeGetCurrent()
                if activity == .afterWaiting { self.awake = now } else if self.measuring { self.busy.append((now - self.awake) * 1000) }
            }
        }
        CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
        let timer = Timer(timeInterval: 1.0 / 30, repeats: true) { [weak self] timer in
            MainActor.assumeIsolated {
                guard let self else { return timer.invalidate() }
                let model = self.model, library = model.library
                switch self.phase {
                case 0 where model.ready && library.summariesLoaded && model.thumbnailStamp != nil && (model.previewCurrent || model.renderFailed):
                    // The same window for every run.
                    if let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) {
                        window.setFrame(NSRect(x: 40, y: 40, width: 1400, height: 900), display: true)
                    }
                    model.showGrid()
                    Self.report("grid shown \(Int(Date().timeIntervalSince(launched) * 1000)) ms after launch, \(library.items.count) items, \(library.looks.count) edited")
                    self.phase = 1; self.phaseStart = Date()
                case 1 where Date().timeIntervalSince(self.phaseStart) > 3:
                    guard let scroll = Self.gridScrollView() else {
                        Self.report("no grid scroll view")
                        timer.invalidate(); return NSApp.terminate(nil)
                    }
                    self.scrollView = scroll
                    self.position = 0
                    self.setPosition()
                    self.phase = 2; self.phaseStart = Date()
                case 2 where Date().timeIntervalSince(self.phaseStart) > 2:
                    // Settled at the top: scroll.
                    let documentHeight = self.scrollView?.documentView?.frame.height ?? 0
                    Self.report("grid \(Int(self.scrollView?.contentView.bounds.width ?? 0))×\(Int(self.scrollView?.contentView.bounds.height ?? 0)) pt, \(model.gridColumns) columns, document \(Int(documentHeight)) pt high")
                    self.startFootprint = Self.footprint(); self.peak = self.startFootprint
                    self.phase = 3
                    self.startSegment(0)
                case 4 where Date().timeIntervalSince(self.phaseStart) > 1.5:
                    if let shots = self.shots { Self.capture(shots.appendingPathComponent("after.png")) }
                    Self.report(String(format: "memory: %.0f MB before scrolling, peak %.0f MB, %.0f MB after", Double(self.startFootprint) / 1e6, Double(self.peak) / 1e6, Double(Self.footprint()) / 1e6))
                    timer.invalidate()
                    NSApp.terminate(nil)
                default: break
                }
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    private func startSegment(_ index: Int) {
        segment = index; shot = false
        intervals = []; nominal = []; busy = []
        lastTimestamp = 0
        guard let view = scrollView else { return }
        if link == nil {
            let link = view.displayLink(target: self, selector: #selector(tick(_:)))
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
            link.add(to: .main, forMode: .common)
            self.link = link
        }
        measuring = true
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        guard lastTimestamp > 0 else { lastTimestamp = now; segmentStart = now; return }
        let delta = now - lastTimestamp
        lastTimestamp = now
        intervals.append(delta * 1000)
        nominal.append(link.duration * 1000)
        let (speed, seconds) = segments[segment]
        position += direction * speed * delta
        setPosition()
        if now - segmentStart > seconds / 2, !shot, let shots {
            shot = true
            Self.capture(shots.appendingPathComponent("scrolling-\(Int(speed)).png"), wait: false)
        }
        if intervals.count % 30 == 0 { peak = max(peak, Self.footprint()) }
        guard now - segmentStart >= seconds else { return }
        measuring = false
        finishSegment()
        if segment + 1 < segments.count {
            startSegment(segment + 1)
        } else {
            link.invalidate(); self.link = nil
            peak = max(peak, Self.footprint())
            phase = 4; phaseStart = Date()
        }
    }

    /// Moves the grid to `position` points from the top, turning at the ends.
    private func setPosition() {
        guard let scroll = scrollView, let document = scroll.documentView else { return }
        let range = max(0, document.frame.height - scroll.contentView.bounds.height)
        if position > range { position = range - (position - range); direction = -1 }
        if position < 0 { position = -position; direction = 1 }
        position = min(max(position, 0), range)
        let y = document.isFlipped ? position : range - position
        scroll.contentView.scroll(to: NSPoint(x: scroll.contentView.bounds.minX, y: y))
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    private func finishSegment() {
        let (speed, seconds) = segments[segment]
        let frame = nominal.sorted()[nominal.count / 2]
        // A frame the display showed without a new one from the app.
        let missed = intervals.reduce(0) { $0 + max(0, Int(($1 / frame).rounded()) - 1) }
        let expected = intervals.reduce(0, +) / frame
        let sortedIntervals = intervals.sorted(), sortedBusy = busy.sorted()
        func pct(_ values: [Double], _ p: Int) -> String {
            String(format: "%.1f", values.isEmpty ? 0 : values[min(values.count - 1, values.count * p / 100)])
        }
        Self.report(String(format: "scroll %.0f pt/s for %.0f s: %d frames at %.1f ms, %d missed (%.1f%%); frame interval p50 %@ p95 %@ p99 %@ worst %@ ms; ",
                           speed, seconds, intervals.count, frame, missed, 100 * Double(missed) / max(expected, 1),
                           pct(sortedIntervals, 50), pct(sortedIntervals, 95), pct(sortedIntervals, 99), pct(sortedIntervals, 100))
                    + "main run-loop busy p50 \(pct(sortedBusy, 50)) p95 \(pct(sortedBusy, 95)) p99 \(pct(sortedBusy, 99)) worst \(pct(sortedBusy, 100)) ms over \(busy.count) passes")
    }

    /// The tallest scroll view in the editor window: the grid's.
    private static func gridScrollView() -> NSScrollView? {
        guard let root = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil })?.contentView else { return nil }
        var best: NSScrollView?, stack: [NSView] = [root]
        while let view = stack.popLast() {
            if let scroll = view as? NSScrollView, let height = scroll.documentView?.frame.height,
               height > best?.documentView?.frame.height ?? 0 { best = scroll }
            stack.append(contentsOf: view.subviews)
        }
        return best
    }

    private static func fill(_ model: EditorModel) {
        let timer = Timer(timeInterval: 0.25, repeats: true) { timer in
            MainActor.assumeIsolated {
                let library = model.library, store = model.editedThumbnails
                guard library.summariesLoaded, let made = model.thumbnailVersions.keys.first,
                      let madeFile = library.items.first(where: { $0.url == made }), let look = library.looks[made],
                      let source = store.key(for: madeFile, look: look).map(store.url(for:)) else { return }
                timer.invalidate()
                var copied = 0
                for file in library.items where file.url != made {
                    guard let look = library.looks[file.url], let key = store.key(for: file, look: look), !store.contains(key) else { continue }
                    if (try? FileManager.default.copyItem(at: source, to: store.url(for: key))) != nil { copied += 1 }
                }
                report("filled \(copied) edited thumbnails from \(made.lastPathComponent)")
                NSApp.terminate(nil)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    /// The process's footprint, as Activity Monitor's Memory column shows it.
    private static func footprint() -> Int {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
        }
        return result == KERN_SUCCESS ? Int(info.phys_footprint) : 0
    }

    /// The editor window only, without its shadow.
    private static func capture(_ url: URL, wait: Bool = true) {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) else { return }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        task.arguments = ["-x", "-o", "-l", String(window.windowNumber), url.path]
        try? task.run()
        if wait { task.waitUntilExit() }
    }

    private static func report(_ line: String) {
        FileHandle.standardError.write(Data("benchmark: \(line)\n".utf8))
    }
}
#endif
