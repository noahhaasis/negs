import AppKit
import ImageIO
import SwiftUI

/// Folder browsing stays separate from the renderer and observes only the
/// current file. No folder watcher or full-resolution preview cache is needed.
enum PhotoFolder {
    static let extensions = Set(["jpg", "jpeg", "png", "tif", "tiff"] + EditorModel.rawExtensions)

    static func images(in directory: URL) throws -> [URL] {
        let files = try FileManager.default.contentsOfDirectory(at: directory,
            includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles])
        return try files.filter {
            try Task.checkCancellation()
            return extensions.contains($0.pathExtension.lowercased())
                && (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
        }
        // Directory enumeration resolves aliases such as /var → /private/var.
        // Keep the opened folder’s spelling so selection and scroll IDs match.
        .map { directory.appendingPathComponent($0.lastPathComponent) }
        .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
    }
}

/// One decode at a time, with a bounded, disposable cache. RAW thumbnails use
/// only embedded previews: browsing must never trigger a full RAW development.
actor PhotoThumbnails {
    static let shared = PhotoThumbnails()
    private let cache: NSCache<NSURL, CGImage> = {
        let cache = NSCache<NSURL, CGImage>()
        cache.totalCostLimit = 16 * 1024 * 1024
        cache.countLimit = 160
        return cache
    }()

    func image(for url: URL) -> CGImage? {
        guard !Task.isCancelled else { return nil }
        if let image = cache.object(forKey: url as NSURL) { return image }
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
        if let image { cache.setObject(image, forKey: url as NSURL, cost: image.bytesPerRow * image.height) }
        return image
    }
}

struct PhotoBrowser: View {
    let model: EditorModel
    let directory: URL
    @State private var files: [URL] = []
    @State private var loading = true
    @State private var failed = false
    @State private var scrollTarget: URL?

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 8) {
                ForEach(files, id: \.self) { url in
                    PhotoThumbnail(url: url, selected: url == model.source) {
                        if url != model.source { model.open(url) }
                    }
                    .id(url)
                }
            }
            .scrollTargetLayout()
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .scrollIndicators(.automatic)
        .scrollPosition(id: $scrollTarget, anchor: .center)
        .onChange(of: files) { _, _ in scrollTarget = model.source }
        .onChange(of: model.source) { _, source in scrollTarget = source }
        .onMoveCommand { direction in
            guard !model.cropping, let source = model.source,
                  let index = files.firstIndex(of: source) else { return }
            let next = direction == .left ? index - 1 : direction == .right ? index + 1 : index
            if next != index, files.indices.contains(next) { model.open(files[next]) }
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
            let scan = Task.detached(priority: .utility) { try PhotoFolder.images(in: directory) }
            do {
                let result = try await withTaskCancellationHandler {
                    try await scan.value
                } onCancel: { scan.cancel() }
                try Task.checkCancellation()
                files = result; loading = false
            } catch {
                guard !Task.isCancelled else { return }
                failed = true; loading = false
            }
        }
    }
}

private struct PhotoThumbnail: View {
    let url: URL
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
                Text(url.lastPathComponent)
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
        .task(id: visible) {
            guard visible else { return }
            let request = Task.detached(priority: .utility) { await PhotoThumbnails.shared.image(for: url) }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            image = result
        }
    }
}
