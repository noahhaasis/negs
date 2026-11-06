import AppKit
import Observation
import ImageIO
import SwiftUI

/// Metadata lets live refreshes keep unchanged thumbnails, including when a
/// session sidecar is written in the same folder. No image pixels are read here.
struct PhotoFile: Hashable, Sendable {
    let url: URL
    let name: String
    let modified: Date?
    let size: Int?

    init(url: URL, values: URLResourceValues? = nil) {
        self.url = url
        name = url.lastPathComponent
        let values = values ?? (try? url.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey]))
        modified = values?.contentModificationDate
        size = values?.fileSize
    }
}

/// Folder browsing stays separate from the renderer.
enum PhotoFolder {
    static let extensions = Set(["jpg", "jpeg", "png", "tif", "tiff"] + EditorModel.rawExtensions)

    static func images(in directory: URL) throws -> [URL] {
        try snapshot(in: directory).map(\.url)
    }

    /// The files the film strip shows of a folder's.
    static func shown(_ files: [PhotoFile], rawOnly: Bool) -> [PhotoFile] {
        rawOnly ? files.filter { EditorModel.rawExtensions.contains($0.url.pathExtension.lowercased()) } : files
    }

    static func snapshot(in directory: URL) throws -> [PhotoFile] {
        let files = try FileManager.default.contentsOfDirectory(at: directory,
            includingPropertiesForKeys: [.isRegularFileKey, .contentModificationDateKey, .fileSizeKey], options: [.skipsHiddenFiles])
        return try files.compactMap { url -> PhotoFile? in
            try Task.checkCancellation()
            guard extensions.contains(url.pathExtension.lowercased()),
                  let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .contentModificationDateKey, .fileSizeKey]),
                  values.isRegularFile == true else { return nil }
            // Preserve the opened folder's spelling through symlinks.
            return PhotoFile(url: directory.appendingPathComponent(url.lastPathComponent), values: values)
        }
        .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}

/// One decode at a time, with a bounded, disposable cache. RAW thumbnails use
/// only embedded previews: browsing must never trigger a full RAW development.
actor PhotoThumbnails {
    static let shared = PhotoThumbnails()
    private final class Cached {
        let file: PhotoFile
        let image: CGImage
        init(file: PhotoFile, image: CGImage) { self.file = file; self.image = image }
    }
    private let cache: NSCache<NSURL, Cached> = {
        let cache = NSCache<NSURL, Cached>()
        cache.totalCostLimit = 96 * 1024 * 1024
        cache.countLimit = 160
        return cache
    }()

    func image(for url: URL) -> CGImage? {
        image(for: PhotoFile(url: url))
    }

    func image(for file: PhotoFile) -> CGImage? {
        let url = file.url
        guard !Task.isCancelled else { return nil }
        if let cached = cache.object(forKey: url as NSURL), cached.file == file { return cached.image }
        let image: CGImage? = autoreleasepool {
            guard let source = CGImageSourceCreateWithURL(url as CFURL,
                [kCGImageSourceShouldCache: false] as CFDictionary) else { return nil }
            var options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: 640,
                kCGImageSourceShouldCacheImmediately: true
            ]
            if !EditorModel.rawExtensions.contains(url.pathExtension.lowercased()) {
                options[kCGImageSourceCreateThumbnailFromImageAlways] = true
            }
            return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
        }
        guard !Task.isCancelled else { return nil }
        if let image { cache.setObject(Cached(file: file, image: image), forKey: url as NSURL, cost: image.bytesPerRow * image.height) }
        return image
    }
}

/// A folder's photographs, kept up to date while `watch` runs.
@MainActor @Observable final class FolderListing {
    private(set) var files: [PhotoFile] = []
    private(set) var loading = true
    private(set) var failed = false

    /// Lists `directory`, calling `listed` after the first scan, and follows
    /// its changes until cancelled, which also releases the watch.
    func watch(_ directory: URL, listed: () -> Void) async {
        loading = true; failed = false; files = []
        // Arm before the initial scan so arrivals during enumeration are not missed.
        let watcher = PhotoFolderWatcher(directory: directory)
        defer { watcher?.stop() }
        await refresh(directory)
        guard !Task.isCancelled else { return }
        listed()
        if let watcher {
            for await _ in watcher.changes {
                guard !Task.isCancelled else { return }
                await refresh(directory)
            }
        }
    }

    private func refresh(_ directory: URL) async {
        let scan = Task.detached(priority: .utility) { try PhotoFolder.snapshot(in: directory) }
        do {
            let result = try await withTaskCancellationHandler {
                try await scan.value
            } onCancel: { scan.cancel() }
            try Task.checkCancellation()
            if files != result { files = result }
            failed = false; loading = false
        } catch {
            guard !Task.isCancelled else { return }
            failed = true; loading = false
        }
    }
}

struct PhotoBrowser: View {
    let model: EditorModel
    let directory: URL
    @State private var listing = FolderListing()

    @State private var strip: FilmStrip?
    @State private var viewport: CGFloat = 0
    @State private var scroller = FilmRollScroller()

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 0) {
                ForEach(Array(shown.enumerated()), id: \.element.url) { index, file in
                    FilmFrame(height: height, file: file, number: index + 1, selected: file.url == model.source, strip: strip, film: model.film) {
                        if file.url != model.source { model.open(file.url) }
                    }
                    .id(file.url)
                }
                // A short folder's roll still runs the window's width.
                let frame = frameLength
                ForEach(0..<FilmRollLayout.blankFrames(photos: shown.count, viewport: viewport, frameLength: frame), id: \.self) { index in
                    BlankFilmFrame(height: height, number: shown.count + index + 1, strip: strip, film: model.film)
                }
            }
            .scrollTargetLayout()
            .background(EnclosingScrollFinder(scroller: scroller))
        }
        .scrollIndicators(.hidden)
        .onChange(of: model.source) { _, _ in revealOpenPhotograph() }
        // The film gets a different length: hold it within it, on the open frame if it's there.
        .onChange(of: model.filmstripRawOnly) { _, _ in
            DispatchQueue.main.async {
                scroller.offset = scroller.offset
                revealOpenPhotograph()
            }
        }
        .onDisappear { scroller.stop() }
        .onMoveCommand { direction in
            guard !model.cropping, let source = model.source,
                  let index = shown.firstIndex(where: { $0.url == source }) else { return }
            let next = direction == .up || direction == .left ? index - 1 : index + 1
            if shown.indices.contains(next) { model.open(shown[next].url) }
        }
        // Over the scroll view, since SwiftUI doesn't pass mouse drags through one.
        .overlay {
            FilmMouseSurface(scroller: scroller, enabled: !model.cropping) { point in
                let frame = CGSize(width: frameLength, height: height)
                let gate = FilmRollLayout.gate(window: strip?.window ?? FilmRollLayout.defaultWindow, frame: frame)
                guard let index = FilmRollLayout.photo(at: point, frame: frame, gate: gate, photos: shown.count),
                      shown[index].url != model.source else { return }
                model.open(shown[index].url)
            }
        }
        // Half over the canvas, so the edge is easy to catch.
        .overlay(alignment: .top) {
            StripResizeHandle(height: height) { model.filmstripHeight = $0 }
                .frame(height: 8)
                .offset(y: -4)
        }
        .frame(height: height)
        // The same part of the roll stays in view as its frames change size.
        .onChange(of: height) { old, new in
            let offset = scroller.offset * new / old
            DispatchQueue.main.async { scroller.offset = offset }
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { viewport = $0 }
        .background(FilmRollLayout.filmBase)
        .overlay {
            Group {
                if listing.loading { ProgressView().controlSize(.small) }
                else if listing.failed { Text("This folder couldn’t be read.") }
                else if shown.isEmpty { Text(model.filmstripRawOnly ? "No RAW files in this folder." : "No supported photographs in this folder.") }
            }
            .foregroundStyle(.white.opacity(0.75))
            .multilineTextAlignment(.center)
            .padding(12)
        }
        .disabled(model.cropping)
        .accessibilityLabel("Film Strip")
        .task(id: "\(model.film) \(model.hasCatalog)") {
            if let loaded = await model.loadRollStrip() { strip = loaded }
        }
        .task(id: directory) {
            // Once the film has its new length.
            await listing.watch(directory) { DispatchQueue.main.async { revealOpenPhotograph() } }
        }
    }

    private var height: CGFloat { model.filmstripHeight }
    private var shown: [PhotoFile] { PhotoFolder.shown(listing.files, rawOnly: model.filmstripRawOnly) }
    private var frameLength: CGFloat {
        FilmRollLayout.frameLength(stripHeight: height, texture: strip?.pixelSize ?? FilmRollLayout.defaultTexture)
    }

    /// The open photograph's frame, if it's wholly out of view. Nothing else
    /// moves the film but the person.
    private func revealOpenPhotograph() {
        guard let source = model.source, let index = shown.firstIndex(where: { $0.url == source }) else { return }
        scroller.reveal(frame: index, length: frameLength)
    }
}
