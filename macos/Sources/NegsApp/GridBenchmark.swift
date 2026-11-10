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
/// SPEKTRAFILM_BENCHMARK_GRID_FILL: waits for the first thumbnail the
/// background helper makes, copies it to every other item's name, and
/// quits, so a later run scrolls with every thumbnail on disk without
/// developing them all.
///
/// SPEKTRAFILM_BENCHMARK_THUMBNAILS: opens the grid in the same window and
/// times the background helper developing the folder's thumbnails: until
/// the cells first on screen have theirs, each tenth of the folder, and the
/// whole, with the app's memory. Quits once every photograph has one, or
/// after a minute without a new one.
///
/// SPEKTRAFILM_BENCHMARK_GRID_POSITION: leaves the session's photograph for
/// the grid as a double-click does, while the inspector slides away, and
/// reports whether the photograph is on screen. Compiled only with
/// -DSPEKTRAFILM_BENCHMARK.
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
    private var segmentCells = (requests: 0, hits: 0)
    private var peak = 0, startFootprint = 0
    private static var running: GridBenchmark?

    static func start(_ model: EditorModel) {
        let environment = ProcessInfo.processInfo.environment
        guard !LaunchBenchmark.enabled else { return }
        if environment["SPEKTRAFILM_BENCHMARK_GRID_FILL"] != nil { return fill(model) }
        if environment["SPEKTRAFILM_BENCHMARK_THUMBNAILS"] != nil { return thumbnails(model) }
        if environment["SPEKTRAFILM_BENCHMARK_GRID_POSITION"] != nil { return position(model) }
        if environment["SPEKTRAFILM_BENCHMARK_OPEN_NEXT"] != nil {
            return openNext(model, shots: environment["SPEKTRAFILM_BENCHMARK_SHOTS"].map { URL(fileURLWithPath: $0) })
        }
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
        segmentCells = PhotoThumbnails.cellCounts.withLock { $0 }
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
        let cells = PhotoThumbnails.cellCounts.withLock { $0 }
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
                    + "main run-loop busy p50 \(pct(sortedBusy, 50)) p95 \(pct(sortedBusy, 95)) p99 \(pct(sortedBusy, 99)) worst \(pct(sortedBusy, 100)) ms over \(busy.count) passes; "
                    + "cells' thumbnails \(cells.requests - segmentCells.requests) asked, \(cells.requests - cells.hits - segmentCells.requests + segmentCells.hits) not in memory")
    }

    /// The tallest scroll view in the editor window: the grid's.
    private static func scrollViews() -> [NSScrollView] {
        guard let root = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil })?.contentView else { return [] }
        var found: [NSScrollView] = [], stack: [NSView] = [root]
        while let view = stack.popLast() {
            if let scroll = view as? NSScrollView { found.append(scroll) }
            stack.append(contentsOf: view.subviews)
        }
        return found
    }

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

    /// SPEKTRAFILM_BENCHMARK_GRID_POSITION: leaves the session's photograph
    /// for the grid as a double-click on it does, and reports whether the
    /// photograph is on screen a moment later and once settled.
    private static func position(_ model: EditorModel) {
        var phase = 0, shown = Date()
        let timer = Timer(timeInterval: 1.0 / 30, repeats: true) { timer in
            MainActor.assumeIsolated {
                let library = model.library
                switch phase {
                case 0 where model.ready && library.summariesLoaded && (model.previewCurrent || model.renderFailed):
                    if let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) {
                        window.setFrame(NSRect(x: 40, y: 40, width: 1400, height: 900), display: true)
                    }
                    shown = Date(); phase = -1
                case -1 where Date().timeIntervalSince(shown) > 1:
                    // Settled in the photo view, as a user double-clicking is.
                    model.doubleClickPhotograph(zoomed: false)
                    shown = Date(); phase = 1
                case 1 where Date().timeIntervalSince(shown) > 0.12, 2 where Date().timeIntervalSince(shown) > 2:
                    let shownPhotos = model.shownPhotos
                    // The grid's own scroll view: its document is as tall as the grid's metrics say.
                    // Laid out at the full width until it settles, as LibraryGrid does.
                    @MainActor func layoutWidth(_ scroll: NSScrollView) -> CGFloat {
                        phase == 1 ? max(scroll.contentView.bounds.width, model.fullCanvasWidth()) : scroll.contentView.bounds.width
                    }
                    let grid = scrollViews().first { scroll in
                        let height = GridMetrics(width: layoutWidth(scroll), cell: model.gridCellSize).height(shownPhotos.count)
                        return abs((scroll.documentView?.frame.height ?? 0) - max(height, scroll.contentView.bounds.height)) < 2
                    }
                    guard let scroll = grid, let current = model.selection.current,
                          let index = shownPhotos.firstIndex(where: { $0.url == current }) else {
                        report("no grid or no current photograph"); timer.invalidate(); return NSApp.terminate(nil)
                    }
                    let visible = scroll.contentView.bounds
                    let metrics = GridMetrics(width: layoutWidth(scroll), cell: model.gridCellSize)
                    let cell = metrics.frame(index)
                    report("grid position after \(phase == 1 ? "0.12" : "2") s: grid \(Int(visible.width)) pt wide (full \(Int(model.fullCanvasWidth()))), photograph \(index + 1) of \(shownPhotos.count), \(metrics.columns) columns, cell y \(Int(cell.minY))–\(Int(cell.maxY)), visible y \(Int(visible.minY))–\(Int(visible.maxY)) of \(Int(scroll.documentView?.frame.height ?? 0)): \(visible.minY <= cell.minY && cell.maxY <= visible.maxY ? "on screen" : "OFF SCREEN")")
                    if let shots = ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_SHOTS"] { capture(URL(fileURLWithPath: shots).appendingPathComponent("grid-\(phase).png")) }
                    if phase == 2 { timer.invalidate(); NSApp.terminate(nil) }
                    phase += 1
                default: break
                }
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    /// SPEKTRAFILM_BENCHMARK_OPEN_NEXT: once the next five photographs'
    /// thumbnails are developed (or after a minute), opens each in turn as →
    /// does and reports when its stand-in, the camera's preview and the
    /// develop first showed; with SPEKTRAFILM_BENCHMARK_SHOTS, a screenshot
    /// at the stand-in and at the develop.
    private static func openNext(_ model: EditorModel, shots: URL?) {
        var phase = 0, started = Date(), opened = 0
        var standIn: Double?, placeholder: Double?, developed: Double?
        let timer = Timer(timeInterval: 1.0 / 240, repeats: true) { timer in
            MainActor.assumeIsolated {
                let library = model.library, store = model.editedThumbnails
                let elapsed = Date().timeIntervalSince(started) * 1000
                switch phase {
                case 0 where model.ready && library.summariesLoaded && model.thumbnailStamp != nil && model.previewCurrent:
                    if let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) {
                        window.setFrame(NSRect(x: 40, y: 40, width: 1400, height: 900), display: true)
                    }
                    phase = 1; started = Date()
                case 1:
                    let items = library.items
                    guard let open = model.sourceItem, let index = items.firstIndex(where: { $0.url == open }) else { return }
                    let next = items[(index + 1)..<min(index + 6, items.count)]
                    let ready = next.allSatisfy { file in
                        model.thumbnailLook(for: file.url).flatMap { store.key(for: file, look: $0) }.map(store.contains) ?? false
                    }
                    guard ready || elapsed > 60_000 else { return }
                    report("next thumbnails \(ready ? "ready" : "NOT all ready") after \(Int(elapsed)) ms")
                    phase = 2
                case 2:
                    standIn = nil; placeholder = nil; developed = nil
                    started = Date()
                    model.step(by: 1)
                    opened += 1
                    phase = 3
                case 3:
                    if standIn == nil, model.standIn != nil {
                        standIn = elapsed
                        if let shots { capture(shots.appendingPathComponent("open-\(opened)-standin.png"), wait: false) }
                    }
                    if placeholder == nil, model.placeholder != nil { placeholder = elapsed }
                    if developed == nil, model.image != nil { developed = elapsed }
                    guard model.previewCurrent, !model.brightnessMatchPending, developed != nil else { return }
                    if let shots { capture(shots.appendingPathComponent("open-\(opened)-developed.png")) }
                    func ms(_ value: Double?) -> String { value.map { "\(Int($0)) ms" } ?? "never" }
                    report("opened \(model.sourceItem?.lastPathComponent ?? "?"): stand-in \(ms(standIn)), camera preview \(ms(placeholder)), develop \(ms(developed))")
                    if opened == 5 { timer.invalidate(); NSApp.terminate(nil) } else { phase = 2 }
                default: break
                }
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    private static func fill(_ model: EditorModel) {
        let timer = Timer(timeInterval: 0.25, repeats: true) { timer in
            MainActor.assumeIsolated {
                let library = model.library, store = model.editedThumbnails
                guard library.summariesLoaded, let made = model.thumbnailVersions.keys.first,
                      let madeFile = library.items.first(where: { $0.url == made }), let look = model.thumbnailLook(for: made),
                      let source = store.key(for: madeFile, look: look).map(store.url(for:)) else { return }
                timer.invalidate()
                var copied = 0
                // Every item, edited or not: each has a developed thumbnail once the folder's filled.
                for file in library.items where file.url != made {
                    guard let look = model.thumbnailLook(for: file.url), let key = store.key(for: file, look: look), !store.contains(key) else { continue }
                    if (try? FileManager.default.copyItem(at: source, to: store.url(for: key))) != nil { copied += 1 }
                }
                report("filled \(copied) thumbnails from \(made.lastPathComponent)")
                NSApp.terminate(nil)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    private static func thumbnails(_ model: EditorModel) {
        let launched = Date()
        var shown: Date?, screen: Set<URL> = [], screenDone = false, tenth = 1
        var lastMade = 0, lastChange = Date(), peak = 0
        let timer = Timer(timeInterval: 0.25, repeats: true) { timer in
            MainActor.assumeIsolated {
                let library = model.library
                guard let start = shown else {
                    guard model.ready, library.summariesLoaded, model.thumbnailStamp != nil, model.previewCurrent || model.renderFailed else { return }
                    if let window = NSApp.windows.first(where: { $0.isVisible && $0.contentViewController != nil }) {
                        window.setFrame(NSRect(x: 40, y: 40, width: 1400, height: 900), display: true)
                    }
                    model.showGrid()
                    shown = Date()
                    report("grid shown \(Int(Date().timeIntervalSince(launched) * 1000)) ms after launch, \(library.items.count) items, \(library.looks.count) edited")
                    return
                }
                let elapsed = Date().timeIntervalSince(start), versions = model.thumbnailVersions
                let made = versions.count, total = library.items.count
                peak = max(peak, footprint())
                // The cells the grid laid out first (before then, the strip's).
                if screen.isEmpty, elapsed > 2 { screen = model.thumbnailsOnScreen }
                if !screenDone, !screen.isEmpty, screen.allSatisfy({ versions[$0] != nil }) {
                    screenDone = true
                    report("the \(screen.count) first on screen in \(Int(elapsed * 1000)) ms")
                }
                while tenth <= 10, made * 10 >= total * tenth {
                    report(String(format: "%d of %d in %.1f s, %.0f ms each, %.0f MB", made, total, elapsed, elapsed * 1000 / Double(max(made, 1)), Double(footprint()) / 1e6))
                    tenth += 1
                }
                if made != lastMade { lastMade = made; lastChange = Date() }
                let finished = library.items.allSatisfy { versions[$0.url] != nil }
                guard finished || Date().timeIntervalSince(lastChange) > 60 else { return }
                if !finished { report("stopped at \(made) of \(total): a minute without a new one") }
                report(String(format: "memory: peak %.0f MB, %.0f MB at the end", Double(peak) / 1e6, Double(footprint()) / 1e6))
                timer.invalidate()
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
