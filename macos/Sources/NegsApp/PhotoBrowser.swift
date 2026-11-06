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

    /// A photograph's width over its height as shown, read from its metadata
    /// without decoding it; nil if the file doesn't say.
    static func aspect(of url: URL) -> CGFloat? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.doubleValue,
              let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.doubleValue,
              width > 0, height > 0 else { return nil }
        // Orientations 5 to 8 are a quarter turn.
        let turned = ((properties[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1) >= 5
        return CGFloat(turned ? height / width : width / height)
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
    /// Each photograph's shape, known before it's shown so frames never change width.
    private(set) var aspects: [URL: CGFloat] = [:]
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
        let known = aspects
        let scan = Task.detached(priority: .utility) { () -> ([PhotoFile], [URL: CGFloat]) in
            let files = try PhotoFolder.snapshot(in: directory)
            var shapes: [URL: CGFloat] = [:]
            for file in files { shapes[file.url] = known[file.url] ?? PhotoFolder.aspect(of: file.url) }
            return (files, shapes)
        }
        do {
            let (result, shapes) = try await withTaskCancellationHandler {
                try await scan.value
            } onCancel: { scan.cancel() }
            try Task.checkCancellation()
            if aspects != shapes { aspects = shapes }
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

    @State private var viewport: CGFloat = 0
    @State private var scroller = FilmRollScroller()
    /// Which stretch of film is in view, in coarse steps.
    @State private var band = 0

    var body: some View {
        ScrollView(.horizontal) {
            // Placed by their known edges, so the film is exactly as long as its
            // frames say: a lazy stack only guesses at frames of different lengths.
            let edges = edges
            let shown = shown
            ZStack(alignment: .topLeading) {
                ForEach(Array(FilmRollLayout.framesInView(edges: edges, band: band)), id: \.self) { index in
                    let file = shown[index]
                    FilmFrame(height: height, length: edges[index + 1] - edges[index], file: file,
                              selected: file.url == model.source) {
                        if file.url != model.source { model.open(file.url) }
                    }
                    .offset(x: edges[index])
                }
            }
            .frame(width: edges[edges.count - 1], height: height, alignment: .topLeading)
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
        .onAppear {
            scroller.onScroll = { [scroller] in
                let next = FilmRollLayout.band(offset: scroller.offset)
                if next != band { band = next }
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
                guard let index = FilmRollLayout.photo(at: point, edges: edges, height: height),
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
        .background(Color.black)
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
        .task(id: directory) {
            // Once the strip has its new length.
            await listing.watch(directory) { DispatchQueue.main.async { revealOpenPhotograph() } }
        }
    }

    private var height: CGFloat { model.filmstripHeight }
    private var shown: [PhotoFile] { PhotoFolder.shown(listing.files, rawOnly: model.filmstripRawOnly) }
    private func length(of file: PhotoFile) -> CGFloat {
        FilmRollLayout.frameLength(stripHeight: height, aspect: listing.aspects[file.url] ?? 3.0 / 2)
    }
    /// Where the frames start and end along the strip.
    private var edges: [CGFloat] { FilmRollLayout.frameEdges(lengths: shown.map(length)) }

    /// The open photograph's frame, if it's wholly out of view. Nothing else
    /// moves the film but the person.
    private func revealOpenPhotograph() {
        guard let source = model.source, let index = shown.firstIndex(where: { $0.url == source }) else { return }
        let edges = edges
        scroller.reveal(start: edges[index], end: edges[index + 1])
    }
}
