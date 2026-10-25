import CoreServices
import Foundation

/// FSEvents also reports writes to existing children (a directory DispatchSource
/// alone cannot tell us when a newly copied photograph finishes writing).
final class PhotoFolderWatcher {
    let changes: AsyncStream<Void>
    private let continuation: AsyncStream<Void>.Continuation
    private var stream: FSEventStreamRef?

    private final class Context {
        let directory: URL
        let continuation: AsyncStream<Void>.Continuation

        init(directory: URL, continuation: AsyncStream<Void>.Continuation) {
            self.directory = directory.resolvingSymlinksInPath().standardizedFileURL
            self.continuation = continuation
        }

        func changed(count: Int, paths: UnsafeMutableRawPointer, flags: UnsafePointer<FSEventStreamEventFlags>) {
            let paths = paths.assumingMemoryBound(to: UnsafePointer<CChar>.self)
            let rescan = FSEventStreamEventFlags(kFSEventStreamEventFlagMustScanSubDirs
                | kFSEventStreamEventFlagRootChanged | kFSEventStreamEventFlagUnmount)
            for index in 0..<count {
                let url = URL(fileURLWithPath: String(cString: paths[index])).standardizedFileURL
                // Events may spell /var as /private/var even within one batch.
                // Resolve the parent: a removed file itself no longer resolves.
                if flags[index] & rescan != 0 || url.resolvingSymlinksInPath().path == directory.path
                    || (PhotoFolder.extensions.contains(url.pathExtension.lowercased())
                        && url.deletingLastPathComponent().resolvingSymlinksInPath().path == directory.path) {
                    continuation.yield(())
                    return
                }
            }
        }
    }

    init?(directory: URL) {
        // At most one rescan waits behind a running scan. The OS batches bursts
        // with 200 ms latency; nothing runs periodically while the folder is idle.
        let (changes, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        self.changes = changes
        self.continuation = continuation
        let context = Context(directory: directory, continuation: continuation)
        var info = FSEventStreamContext(version: 0,
            info: Unmanaged.passUnretained(context).toOpaque(),
            retain: { pointer in
                guard let pointer else { return nil }
                _ = Unmanaged<Context>.fromOpaque(pointer).retain()
                return pointer
            },
            release: { pointer in
                if let pointer { Unmanaged<Context>.fromOpaque(pointer).release() }
            }, copyDescription: nil)
        stream = FSEventStreamCreate(nil, { _, info, count, paths, flags, _ in
            guard let info else { return }
            Unmanaged<Context>.fromOpaque(info).takeUnretainedValue()
                .changed(count: count, paths: paths, flags: flags)
        }, &info, [context.directory.path] as CFArray,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 0.2,
            FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagWatchRoot))
        guard let stream else { continuation.finish(); return nil }
        FSEventStreamSetDispatchQueue(stream, DispatchQueue(label: "negs.folder-events", qos: .utility))
        guard FSEventStreamStart(stream) else {
            stop()
            continuation.finish()
            return nil
        }
    }

    func stop() {
        continuation.finish()
        guard let stream else { return }
        self.stream = nil
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
    }

    deinit { stop() }
}
