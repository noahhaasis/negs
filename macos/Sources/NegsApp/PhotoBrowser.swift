import AppKit
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
        cache.totalCostLimit = 16 * 1024 * 1024
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
                kCGImageSourceThumbnailMaxPixelSize: 192,
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

struct PhotoBrowser: View {
    let model: EditorModel
    let directory: URL
    @State private var files: [PhotoFile] = []
    @State private var loading = true
    @State private var failed = false
    @State private var scrollTarget: URL?

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 8) {
                ForEach(files, id: \.url) { file in
                    PhotoThumbnail(file: file, selected: file.url == model.source) {
                        if file.url != model.source { model.open(file.url) }
                    }
                    .id(file.url)
                }
            }
            .scrollTargetLayout()
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .scrollIndicators(.automatic)
        .scrollPosition(id: $scrollTarget, anchor: .center)
        .onChange(of: model.source) { _, source in scrollTarget = source }
        .onMoveCommand { direction in
            guard !model.cropping, let source = model.source,
                  let index = files.firstIndex(where: { $0.url == source }) else { return }
            let next = direction == .left ? index - 1 : direction == .right ? index + 1 : index
            if next != index, files.indices.contains(next) { model.open(files[next].url) }
        }
        .frame(height: 112)
        .background(Color(nsColor: .controlBackgroundColor))
        .overlay {
            if loading { ProgressView().controlSize(.small) }
            else if failed { Text("This folder couldn’t be read.").foregroundStyle(.secondary) }
            else if files.isEmpty { Text("No supported photographs in this folder.").foregroundStyle(.secondary) }
        }
        .disabled(model.cropping)
        .accessibilityLabel("Film Strip")
        .task(id: directory) {
            loading = true; failed = false; files = []
            // Arm before the initial scan so arrivals during enumeration are
            // not missed. Cancellation on hide/folder switch releases the watch.
            let watcher = PhotoFolderWatcher(directory: directory)
            defer { watcher?.stop() }
            await refresh(initial: true)
            if let watcher {
                for await _ in watcher.changes {
                    guard !Task.isCancelled else { return }
                    await refresh(initial: false)
                }
            }
        }
    }

    private func refresh(initial: Bool) async {
        let scan = Task.detached(priority: .utility) { try PhotoFolder.snapshot(in: directory) }
        do {
            let result = try await withTaskCancellationHandler {
                try await scan.value
            } onCancel: { scan.cancel() }
            try Task.checkCancellation()
            if files != result { files = result }
            failed = false; loading = false
            if initial { scrollTarget = model.source }
        } catch {
            guard !Task.isCancelled else { return }
            failed = true; loading = false
        }
    }
}

private struct PhotoThumbnail: View {
    let file: PhotoFile
    private var url: URL { file.url }
    let selected: Bool
    let open: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    var body: some View {
        Button(action: open) {
            VStack(spacing: 5) {
                ZStack {
                    RoundedRectangle(cornerRadius: 4).fill(.primary.opacity(0.04))
                    if let image {
                        Image(decorative: image, scale: 2).resizable().scaledToFit().padding(3)
                    } else {
                        Image(systemName: "photo").foregroundStyle(.tertiary)
                    }
                }
                .frame(width: 96, height: 68)
                .overlay { RoundedRectangle(cornerRadius: 4).strokeBorder(selected ? Color.accentColor : .clear, lineWidth: 2) }
                Text(file.name)
                    .font(.caption2)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundStyle(selected ? .primary : .secondary)
            }
            .frame(width: 96)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(url.lastPathComponent)
        .accessibilityLabel(url.lastPathComponent)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
        .onAppear { visible = true }
        .onDisappear { visible = false; image = nil }
        .task(id: visible ? file : nil) {
            guard visible else { return }
            let request = Task.detached(priority: .utility) { await PhotoThumbnails.shared.image(for: file) }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            image = result
        }
    }
}
