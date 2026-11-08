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
        try listing(in: directory).photos
    }

    /// The folder's photographs in name order, and the sidecars beside them
    /// (by the photograph's file name), from one directory listing.
    static func listing(in directory: URL) throws -> (photos: [PhotoFile], sidecars: [String: PhotoFile]) {
        let files = try FileManager.default.contentsOfDirectory(at: directory,
            includingPropertiesForKeys: [.isRegularFileKey, .contentModificationDateKey, .fileSizeKey], options: [.skipsHiddenFiles])
        var photos: [PhotoFile] = [], sidecars: [String: PhotoFile] = [:]
        for url in files {
            try Task.checkCancellation()
            let ext = url.pathExtension.lowercased()
            let sidecar = ext == PhotoSidecar.fileExtension
            guard sidecar || extensions.contains(ext),
                  let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .contentModificationDateKey, .fileSizeKey]),
                  values.isRegularFile == true else { continue }
            // Preserve the opened folder's spelling through symlinks.
            let file = PhotoFile(url: directory.appendingPathComponent(url.lastPathComponent), values: values)
            if sidecar { sidecars[url.deletingPathExtension().lastPathComponent] = file } else { photos.append(file) }
        }
        photos.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        return (photos, sidecars)
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
    /// The film strip's size, and the grid's two (for cells up to about 190
    /// and 380 points on a Retina screen).
    enum Size: Int, CaseIterable, Sendable {
        case strip = 192, medium = 384, large = 768
        static func grid(points: CGFloat, scale: CGFloat) -> Size { points * scale <= 384 ? .medium : .large }
    }
    private let caches: [Size: NSCache<NSURL, Cached>] = {
        var caches: [Size: NSCache<NSURL, Cached>] = [:]
        for (size, megabytes, count) in [(Size.strip, 16, 160), (.medium, 48, 400), (.large, 96, 200)] {
            let cache = NSCache<NSURL, Cached>()
            cache.totalCostLimit = megabytes * 1024 * 1024
            cache.countLimit = count
            caches[size] = cache
        }
        return caches
    }()

    func image(for url: URL) -> CGImage? {
        image(for: PhotoFile(url: url))
    }

    func image(for file: PhotoFile, size: Size = .strip) -> CGImage? {
        let url = file.url
        guard !Task.isCancelled, let cache = caches[size] else { return nil }
        if let cached = cache.object(forKey: url as NSURL), cached.file == file { return cached.image }
        let image: CGImage? = autoreleasepool {
            guard let source = CGImageSourceCreateWithURL(url as CFURL,
                [kCGImageSourceShouldCache: false] as CFDictionary) else { return nil }
            var options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: size.rawValue,
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

/// The film strip: the library's photographs, as filtered. The library
/// lists the folder (`EditorCanvas` keeps it current); thumbnails never wait
/// for the ratings.
struct PhotoBrowser: View {
    let model: EditorModel
    @State private var scrollTarget: URL?
    /// A click takes the keys, so ← and → move on from there.
    @FocusState private var focused: Bool

    var body: some View {
        let library = model.library
        let shown = library.filtering ? [] : model.shownPhotos
        let selected = model.selection.selected
        ScrollView(.horizontal) {
            LazyHStack(spacing: 8) {
                ForEach(shown, id: \.url) { file in
                    PhotoThumbnail(file: file, current: file.url == model.source, selected: selected.contains(file.url),
                                   summary: library.summary(for: file.url), readOnly: library.readOnly) {
                        let modifiers = NSEvent.modifierFlags
                        focused = true
                        model.click(file.url, command: modifiers.contains(.command), shift: modifiers.contains(.shift))
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
        .onChange(of: library.loading) { _, loading in if !loading { scrollTarget = model.source } }
        .onChange(of: library.filtering) { _, filtering in if !filtering { scrollTarget = model.source } }
        .onAppear { scrollTarget = model.source }
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        // ← and → open the previous and next photograph; with ⇧ they select
        // a range and the current photograph stays.
        .onKeyPress(keys: [.leftArrow, .rightArrow], phases: [.down, .repeat]) { press in
            // Arrow keys also carry the numeric pad and function flags.
            guard press.modifiers.isDisjoint(with: [.command, .option, .control]) else { return .ignored }
            model.move(by: press.key == .leftArrow ? -1 : 1, extending: press.modifiers.contains(.shift))
            return .handled
        }
        .frame(height: 112)
        .background(Color(nsColor: .controlBackgroundColor))
        .overlay {
            // Only an active filter waits for the ratings.
            if library.loading || library.filtering { ProgressView().controlSize(.small) }
            else if library.failed { Text("This folder couldn’t be read.").foregroundStyle(.secondary) }
            else if library.files.isEmpty { Text("No supported photographs in this folder.").foregroundStyle(.secondary) }
        }
        .disabled(model.cropping)
        .accessibilityLabel("Film Strip")
    }
}

private struct PhotoThumbnail: View {
    let file: PhotoFile
    private var url: URL { file.url }
    /// The open photograph, outlined thickly; others selected, thinly.
    let current: Bool
    let selected: Bool
    let summary: LibrarySummary
    let readOnly: Bool
    let open: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    private var rejected: Bool { summary.flag == .reject }

    var body: some View {
        Button(action: open) {
            VStack(spacing: 5) {
                ZStack {
                    RoundedRectangle(cornerRadius: 4).fill(.primary.opacity(0.04))
                    Group {
                        if let image {
                            Image(decorative: image, scale: 2).resizable().scaledToFit().padding(3)
                        } else {
                            Image(systemName: "photo").foregroundStyle(.tertiary)
                        }
                    }
                    // Rejects are greyed; their name stays readable.
                    .opacity(rejected ? 0.4 : 1)
                }
                .frame(width: 96, height: 68)
                .overlay { PhotoBadges(summary: summary) }
                .overlay {
                    RoundedRectangle(cornerRadius: 4)
                        .strokeBorder(current || selected ? Color.accentColor : .clear, lineWidth: current ? 2 : 1)
                }
                Text(file.name)
                    .font(.caption2)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundStyle(current || selected ? .primary : .secondary)
            }
            .frame(width: 96)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(PhotoBadges.help(url, summary: summary, readOnly: readOnly))
        .accessibilityLabel(PhotoBadges.accessibilityLabel(url, summary: summary))
        .accessibilityAddTraits(current || selected ? [.isSelected] : [])
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

/// A thumbnail's flag (top left), edited mark (top right) and stars
/// (bottom). Unrated, unflagged, unedited photographs show none of them.
struct PhotoBadges: View {
    let summary: LibrarySummary

    var body: some View {
        ZStack {
            if let flag = summary.flag {
                badge(flag == .pick ? "flag.fill" : "xmark").frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
            if summary.edited {
                badge("slider.horizontal.3").frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
            }
            if summary.rating > 0 {
                HStack(spacing: 1) {
                    ForEach(0..<summary.rating, id: \.self) { _ in Image(systemName: "star.fill") }
                }
                .font(.system(size: 7))
                .foregroundStyle(.white)
                .padding(.horizontal, 4)
                .padding(.vertical, 2)
                .background(Capsule().fill(.black.opacity(0.55)))
                .padding(.bottom, 4)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
            }
        }
        .accessibilityHidden(true)
        .allowsHitTesting(false)
    }

    private func badge(_ symbol: String) -> some View {
        Image(systemName: symbol)
            .font(.system(size: 8, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: 15, height: 15)
            .background(Circle().fill(.black.opacity(0.55)))
            .padding(4)
    }

    static func help(_ url: URL, summary: LibrarySummary, readOnly: Bool) -> String {
        var lines = [url.lastPathComponent]
        if let problem = summary.problem { lines.append(problem) }
        if readOnly { lines.append("Ratings and edits can’t be saved in this folder: it’s read-only.") }
        return lines.joined(separator: "\n")
    }

    static func accessibilityLabel(_ url: URL, summary: LibrarySummary) -> String {
        var parts = [url.lastPathComponent]
        if summary.rating > 0 { parts.append(summary.rating == 1 ? "1 star" : "\(summary.rating) stars") }
        if let flag = summary.flag { parts.append(flag == .pick ? "Pick" : "Rejected") }
        if summary.edited { parts.append("Edited") }
        return parts.joined(separator: ", ")
    }
}
