import AppKit
import ImageIO
import os
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

    private init(url: URL, name: String, modified: Date?, size: Int?) {
        self.url = url; self.name = name; self.modified = modified; self.size = size
    }

    /// The photograph's `number`th variant, a duplicate: `IMG_0412.CR3 (2)`.
    func duplicate(_ id: String, number: Int) -> PhotoFile {
        PhotoFile(url: url.variant(id), name: "\(name) (\(number))", modified: modified, size: size)
    }

    /// The photograph a duplicate shows.
    var photo: PhotoFile { url.variantID == nil ? self : PhotoFile(url: url.photoFile, name: url.lastPathComponent, modified: modified, size: size) }
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
        /// The edited thumbnail's name (`previous(_:)` for the last engine's
        /// standing in for it), or nil for the embedded preview.
        let look: String?
        let image: CGImage
        init(file: PhotoFile, look: String?, image: CGImage) { self.file = file; self.look = look; self.image = image }
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

    private static func previous(_ key: String) -> String { "previous|" + key }

    #if SPEKTRAFILM_BENCHMARK
    /// Cells' requests, and those the in-memory cache answered, for GridBenchmark.
    static let cellCounts = OSAllocatedUnfairLock(initialState: (requests: 0, hits: 0))
    private func count(_ ahead: Bool, hit: Bool) {
        guard !ahead else { return }
        Self.cellCounts.withLock { $0.requests += 1; if hit { $0.hits += 1 } }
    }
    #else
    @inline(__always) private func count(_ ahead: Bool, hit: Bool) {}
    #endif

    func image(for url: URL) -> CGImage? {
        image(for: PhotoFile(url: url))
    }

    /// An edited photograph's thumbnail when `edited` has one for `look`
    /// (its stored edits), else the embedded preview. `ahead`: read before
    /// a cell asks.
    func image(for item: PhotoFile, size: Size = .strip, look: String? = nil, edited: EditedThumbnailStore? = nil, ahead: Bool = false) -> CGImage? {
        guard !Task.isCancelled, let cache = caches[size] else { return nil }
        if let look, let edited, let key = edited.key(for: item, look: look) {
            if let cached = cache.object(forKey: item.url as NSURL), cached.file == item,
               cached.look == key || cached.look == Self.previous(key) && !edited.contains(key) { count(ahead, hit: true); return cached.image }
            if let image = edited.image(key, maxPixels: size.rawValue) {
                count(ahead, hit: false)
                cache.setObject(Cached(file: item, look: key, image: image), forKey: item.url as NSURL, cost: image.bytesPerRow * image.height)
                return image
            }
            // After an update, the last engine's until this one's is made.
            if let previous = edited.previousKey(for: item, look: look), let image = edited.image(previous, maxPixels: size.rawValue) {
                count(ahead, hit: false)
                cache.setObject(Cached(file: item, look: Self.previous(key), image: image), forKey: item.url as NSURL, cost: image.bytesPerRow * image.height)
                return image
            }
        }
        // A duplicate shows its photograph's embedded preview.
        let file = item.photo
        let url = file.url
        if let cached = cache.object(forKey: url as NSURL), cached.file == file, cached.look == nil { count(ahead, hit: true); return cached.image }
        count(ahead, hit: false)
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
        if let image { cache.setObject(Cached(file: file, look: nil, image: image), forKey: url as NSURL, cost: image.bytesPerRow * image.height) }
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
                    PhotoThumbnail(file: file, current: file.url == model.sourceItem, selected: selected.contains(file.url),
                                   summary: library.summary(for: file.url), readOnly: library.readOnly,
                                   edited: model.editedThumbnail(for: file.url)) { visible in
                        model.thumbnailShown(file, visible)
                    } open: {
                        let modifiers = NSEvent.modifierFlags
                        focused = true
                        model.click(file.url, command: modifiers.contains(.command), shift: modifiers.contains(.shift))
                    }
                    .contextMenu { LibraryContextMenu(model: model, item: file.url) }
                    .id(file.url)
                }
            }
            .scrollTargetLayout()
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .scrollIndicators(.automatic)
        .scrollPosition(id: $scrollTarget, anchor: .center)
        .onChange(of: model.sourceItem) { _, source in scrollTarget = source }
        .onChange(of: library.loading) { _, loading in if !loading { scrollTarget = model.sourceItem } }
        .onChange(of: library.filtering) { _, filtering in if !filtering { scrollTarget = model.sourceItem } }
        .onAppear { scrollTarget = model.sourceItem }
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
    let edited: EditedThumbnail?
    /// On and off screen: what's shown is developed first.
    let shown: (Bool) -> Void
    let open: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    private struct Load: Hashable { let file: PhotoFile; let edited: EditedThumbnail? }

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
        .help(PhotoBadges.help(file.name, summary: summary, readOnly: readOnly))
        .accessibilityLabel(PhotoBadges.accessibilityLabel(file.name, summary: summary))
        .accessibilityAddTraits(current || selected ? [.isSelected] : [])
        .onAppear { visible = true; shown(true) }
        .onDisappear { visible = false; image = nil; shown(false) }
        .task(id: visible ? Load(file: file, edited: edited) : nil) {
            guard visible else { return }
            let file = file, edited = edited
            let request = Task.detached(priority: .utility) {
                await PhotoThumbnails.shared.image(for: file, look: edited?.look, edited: edited?.store)
            }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            // An edited thumbnail on its way keeps the last one shown.
            if let result { image = result }
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

    static func help(_ name: String, summary: LibrarySummary, readOnly: Bool) -> String {
        var lines = [name]
        if let problem = summary.problem { lines.append(problem) }
        if readOnly { lines.append("Ratings and edits can’t be saved in this folder: it’s read-only.") }
        return lines.joined(separator: "\n")
    }

    static func accessibilityLabel(_ name: String, summary: LibrarySummary) -> String {
        var parts = [name]
        if summary.rating > 0 { parts.append(summary.rating == 1 ? "1 star" : "\(summary.rating) stars") }
        if let flag = summary.flag { parts.append(flag == .pick ? "Pick" : "Rejected") }
        if summary.edited { parts.append("Edited") }
        return parts.joined(separator: ", ")
    }
}

/// The film strip's and grid's context menu. It acts on the selection if the
/// clicked photograph is in it, otherwise on that photograph alone, as in
/// Finder.
struct LibraryContextMenu: View {
    let model: EditorModel
    let item: URL

    var body: some View {
        let targets = model.contextTargets(item)
        let several = targets.count > 1 ? " to \(targets.count) Photographs" : ""
        let editing = model.canChangeEdits
        if model.mode == .grid {
            Button("Edit Photograph") { model.click(item); model.editCurrent() }
            Divider()
        }
        Menu("Rating") {
            ForEach(0...5, id: \.self) { stars in
                Button(stars == 0 ? "None" : String(repeating: "★", count: stars)) { model.setRating(stars, targets: targets) }
            }
        }
        Menu("Flag") {
            Button("Pick") { model.setFlag(.pick, targets: targets) }
            Button("Reject") { model.setFlag(.reject, targets: targets) }
            Button("Unflagged") { model.setFlag(nil, targets: targets) }
        }
        Divider()
        Button("Copy Edits") { model.copyEdits(from: item) }.disabled(!editing)
        Button("Paste Edits" + several) { model.pasteEdits(onto: targets) }.disabled(!editing || model.copiedEdits == nil)
        Button("Paste from Previous" + several) { model.pasteFromPrevious(onto: targets) }.disabled(!editing || model.previousEdits == nil)
        Divider()
        Button(targets.count > 1 ? "Duplicate \(targets.count) Photographs" : "Duplicate") { model.duplicate(targets) }.disabled(!editing)
        Button(targets.count > 1 ? "Reset Edits of \(targets.count) Photographs" : "Reset Edits") { model.resetEdits(of: targets) }.disabled(!editing)
        Divider()
        Button(targets.count > 1 ? "Move \(targets.count) Photographs to Trash…" : "Move to Trash…") {
            model.requestDelete(targets, asking: true)
        }.disabled(!model.canDelete)
        Divider()
        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting(Array(Set(targets.map(\.photoFile)))) }
    }
}
