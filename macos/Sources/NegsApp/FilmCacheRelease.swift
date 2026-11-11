import Foundation

/// Lets the helper free the film side it keeps on the GPU between renders
/// (ticket 41: up to ~0.7 GB for a 60 MP photograph) when it won't be used
/// soon: the editor went back to the grid, or the system is short of memory.
/// The helper already frees it when another photograph is decoded, so a
/// photo switch sends nothing, and neither does anything during editing.
///
/// The command goes through the helper's serial request path like any
/// other, after whatever is already queued; failures are ignored (an older
/// helper doesn't know it), and nothing is sent while no helper runs, which
/// would only start one. The next render after a release simply develops the
/// film again, as it would have without the cache.
@MainActor final class FilmCacheRelease {
    enum Reason: Equatable { case editorClosed, memoryPressure }

    private let engine: () -> EngineClient?
    private var pressure: DispatchSourceMemoryPressure?
    /// Every release sent, and what the helper answered it freed (nil if it
    /// failed), for tests and diagnostics.
    private(set) var sent: [(reason: Reason, releasedBytes: Double?)] = []

    init(engine: @escaping () -> EngineClient?) {
        self.engine = engine
    }

    /// Releases on the system's memory warnings, warning and critical.
    func watchMemoryPressure() {
        guard pressure == nil else { return }
        let source = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .main)
        source.setEventHandler { [weak self, weak source] in
            guard let event = source?.data else { return }
            MainActor.assumeIsolated { self?.memoryPressure(event) }
        }
        source.activate()
        pressure = source
    }

    /// A memory-pressure event, as the source delivers it (tests send one).
    func memoryPressure(_ event: DispatchSource.MemoryPressureEvent) {
        guard !event.intersection([.warning, .critical]).isEmpty else { return }
        release(.memoryPressure)
    }

    func release(_ reason: Reason) {
        guard let engine = engine(), engine.runningLaunch != nil else { return }
        Task { [weak self] in
            let answer = try? await Task.detached(priority: .utility) {
                try await engine.request(["command": .string("release_film_cache")])
            }.value
            self?.sent.append((reason, answer?.1["released_bytes"].double))
        }
    }

    deinit { pressure?.cancel() }
}
