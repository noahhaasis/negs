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
    /// The film strip's size (its frames are 140 pt tall, up to about 210
    /// wide), and the grid's two (for pictures up to about 190 and 380
    /// points on their long side on a Retina screen).
    enum Size: Int, CaseIterable, Sendable {
        case strip = 432, medium = 384, large = 768
        static func grid(points: CGFloat, scale: CGFloat) -> Size { points * scale <= 384 ? .medium : .large }
    }
    private let caches: [Size: NSCache<NSURL, Cached>] = {
        var caches: [Size: NSCache<NSURL, Cached>] = [:]
        for (size, megabytes, count) in [(Size.strip, 48, 120), (.medium, 48, 400), (.large, 96, 200)] {
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

/// The film strip (§4.4): the library's photographs, as filtered, laid edge
/// to edge on black film at their own proportions, the current frame at full
/// strength and the others dimmed. A 12 pt rebate under the frames carries
/// the pencil under the current one. No numbers, no names. The library lists
/// the folder (`EditorCanvas` keeps it current); thumbnails never wait for
/// the ratings.
struct PhotoBrowser: View {
    let model: EditorModel
    /// A click takes the keys, so ← and → move on from there.
    @FocusState private var focused: Bool
    /// Brings the current frame back to the middle once the window has
    /// stopped changing size.
    @State private var settling = StripSettling()
    /// Where the strip is scrolled to: which frames are built.
    @State private var watch = ScrollWatch()
    /// The strip's width, once it has one.
    @State private var viewWidth: CGFloat?

    /// The pencil under the current frame: as heavy as the grid's yellow
    /// box, so it still reads under a pick's box (round 21). It sits 3 pt
    /// into the rebate, clear of the box's bottom side.
    static let pencilWidth: CGFloat = 4

    /// The picture's height: the strip less its top padding and rebate.
    static let pictureHeight = Theme.Size.strip - Theme.Size.stripTopPadding - Theme.Size.stripRebate

    /// Frames are built this far either side of the strip's view, so a
    /// scroll finds the next ones there already.
    static let margin: CGFloat = 400

    /// A frame's width: the picture's height at its photograph's proportions.
    static func frameWidth(_ file: PhotoFile, ratios: [URL: CGFloat]) -> CGFloat {
        (pictureHeight * (ratios[file.url] ?? PhotoAspects.fallback)).rounded()
    }

    /// The photographs the strip shows, and their frames laid out.
    static func row(_ model: EditorModel) -> (shown: [PhotoFile], layout: StripLayout) {
        let shown = model.library.filtering ? [] : model.shownPhotos
        let ratios = model.aspects.ratios
        return (shown, StripLayout(widths: shown.map { frameWidth($0, ratios: ratios) }))
    }

    /// The scroll offset that puts the current frame in the middle, as far
    /// as the row's ends let it; nil without one or before the strip has a width.
    private static func centred(_ model: EditorModel, in watch: ScrollWatch) -> CGFloat? {
        let (shown, layout) = row(model)
        guard let item = model.sourceItem, let visible = watch.visible, visible.width > 0,
              let index = shown.firstIndex(where: { $0.url == item }) else { return nil }
        return StripSettling.target(middle: layout.middle(of: index), visible: visible.width, content: layout.width)
    }

    /// Puts the current frame in the middle at once.
    private static func centre(_ model: EditorModel, in watch: ScrollWatch) {
        if let target = centred(model, in: watch) { watch.scroll(x: target) }
    }

    /// After a resize: the current frame glides back to the middle, in one
    /// movement, unless it's there already (or as near as the row's ends
    /// let it). Not while the grid covers the strip or folds back into it.
    private static func recentre(model: EditorModel, in settling: StripSettling, watch: ScrollWatch) {
        guard model.mode == .photo, !model.sheetFolding, let visible = watch.visible,
              let target = centred(model, in: watch), abs(target - visible.minX) >= 1 else { return }
        settling.glide(to: target)
    }

    /// The row was laid out anew from the same photographs, as their shapes
    /// were read: the current frame, or the one in the middle if it's out
    /// of view, stays where it is on screen, rather than the row sliding
    /// under it.
    private static func hold(_ model: EditorModel, from old: StripLayout, to new: StripLayout, in watch: ScrollWatch) {
        guard old.count == new.count, let visible = watch.visible, visible.width > 0 else { return }
        let current = model.sourceItem.flatMap { item in model.shownPhotos.firstIndex { $0.url == item } }
        let shown = current.map { old.xs[$0] < visible.maxX && old.xs[$0] + old.widths[$0] > visible.minX } ?? false
        guard let index = shown ? current : old.index(at: visible.midX) else { return }
        let x = visible.minX + new.middle(of: index) - old.middle(of: index)
        watch.scroll(x: min(max(x, 0), max(new.width - visible.width, 0)))
    }

    /// The row of frames `widths` wide, edge to edge but for the gaps.
    static func rowWidth(_ widths: [CGFloat]) -> CGFloat {
        widths.reduce(0, +) + CGFloat(max(widths.count - 1, 0)) * Theme.Size.stripGap
    }

    var body: some View {
        let library = model.library
        let selected = model.selection.selected
        let aspects = model.aspects
        // Reads the shapes, so a shape learnt redraws the strip.
        let (shown, layout) = Self.row(model)
        // Only the frames in or near view are built, each where the row puts it.
        let left = watch.origin.x - Self.margin
        let built = layout.indices(from: left, to: left + Self.margin * 2 + ScrollWatch.step + (viewWidth ?? 1600))
        let frames = built.map { (index: $0, file: shown[$0]) }
        ScrollView(.horizontal) {
            ZStack(alignment: .topLeading) {
                ForEach(frames, id: \.file.url) { frame in
                    let file = frame.file
                    StripFrame(file: file, width: layout.widths[frame.index],
                               current: file.url == model.sourceItem, selected: selected.contains(file.url),
                               summary: library.summary(for: file.url), readOnly: library.readOnly,
                               edited: model.editedThumbnail(for: file.url)) { visible in
                        model.thumbnailShown(file, visible)
                    } loaded: { image in
                        aspects.note(file.url, image: image)
                    } placed: { place in
                        model.places.strip[file.url] = place
                    } open: {
                        let modifiers = NSEvent.modifierFlags
                        focused = true
                        model.click(file.url, command: modifiers.contains(.command), shift: modifiers.contains(.shift))
                    }
                    .contextMenu { LibraryContextMenu(model: model, item: file.url) }
                    .offset(x: layout.xs[frame.index])
                }
                // A pick's box reaches past its frame, over the gap and the
                // frames beside it, so the boxes are drawn above every frame,
                // placed as the frames are (as the grid draws its own).
                ForEach(frames, id: \.file.url) { frame in
                    ZStack {
                        if library.summary(for: frame.file.url).flag == .pick {
                            BoxInk(name: frame.file.name, picked: true, inStrip: true)
                        }
                    }
                    .frame(width: layout.widths[frame.index], height: Self.pictureHeight)
                    .offset(x: layout.xs[frame.index])
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                }
            }
            // The row's exact width, from the frames' own.
            .frame(width: layout.width, alignment: .topLeading)
            // Inside the scroll view, so the boxes' tops show in it.
            .padding(.top, Theme.Size.stripTopPadding)
            .background(EnclosingScrollView { [model, watch] scrollView in
                settling.scrollView = scrollView
                watch.attach(scrollView)
                Self.centre(model, in: watch)
            })
        }
        .scrollIndicators(.never)
        // Centred when it changes, not held there. A resize keeps the row
        // where it is; once it's over, the current frame glides back to the
        // middle (`StripSettling`).
        .onChange(of: model.sourceItem) { _, _ in Self.centre(model, in: watch) }
        .onChange(of: library.loading) { _, loading in if !loading { Self.centre(model, in: watch) } }
        .onChange(of: library.filtering) { _, filtering in if !filtering { Self.centre(model, in: watch) } }
        .onChange(of: layout) { old, new in Self.hold(model, from: old, to: new, in: watch) }
        .onAppear {
            Self.centre(model, in: watch)
            settling.settle = { [model, settling, watch] in Self.recentre(model: model, in: settling, watch: watch) }
        }
        .onGeometryChange(for: CGFloat.self, of: \.size.width) { width in
            let first = viewWidth == nil
            viewWidth = width
            settling.widthChanged(width)
            // The strip appearing: centred once it has a width.
            if first { Self.centre(model, in: watch) }
        }
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
        .frame(height: Theme.Size.strip)
        .background(Theme.film)
        .overlay {
            // Only an active filter waits for the ratings.
            if library.loading || library.filtering { ProgressView().controlSize(.small) }
            else if library.failed { StripNote(text: "This folder couldn’t be read.") }
            else if library.files.isEmpty { StripNote(text: "No supported photographs in this folder.") }
        }
        .disabled(model.cropping)
        .accessibilityLabel("Film Strip")
    }
}

/// Tells the strip when the window has stopped changing size: at the end
/// of a live resize or of the full-screen animation, or a moment after the
/// last change of the strip's width otherwise (zoom, tiling, the inspector
/// sliding in or out). Nothing moves during the change itself.
@MainActor final class StripSettling {
    /// The re-centring's length.
    static let duration: TimeInterval = 0.35
    /// How long the width stays put before a change made without a live
    /// resize counts as over.
    static let quiet: TimeInterval = 0.25

    /// Called once the size has settled.
    var settle: (() -> Void)?
    /// The strip's scroll view, under SwiftUI's.
    weak var scrollView: NSScrollView?
    private var width: CGFloat?
    /// Between the start and the end of the full-screen animation.
    private var transitioning = false
    private var pending: DispatchWorkItem?
    private var observers: [NSObjectProtocol] = []
    private var glider: Glider?

    init() {
        let centre = NotificationCenter.default
        let observe = { (name: Notification.Name, action: @escaping @MainActor (StripSettling) -> Void) in
            self.observers.append(centre.addObserver(forName: name, object: nil, queue: .main) { [weak self] note in
                // The editing window's own, not Settings'.
                guard note.object is EditorWindow else { return }
                MainActor.assumeIsolated { if let self { action(self) } }
            })
        }
        for name in [NSWindow.willEnterFullScreenNotification, NSWindow.willExitFullScreenNotification] {
            observe(name) { $0.transitioning = true; $0.pending?.cancel() }
        }
        for name in [NSWindow.didEnterFullScreenNotification, NSWindow.didExitFullScreenNotification] {
            observe(name) { $0.transitioning = false; $0.settled() }
        }
        // Full screen resizes the window as a live resize too, ended at
        // once, while its animation still has a while to run.
        observe(NSWindow.didEndLiveResizeNotification) { if !$0.transitioning { $0.settled() } }
    }

    deinit { observers.forEach(NotificationCenter.default.removeObserver) }

    func widthChanged(_ width: CGFloat) {
        let first = self.width == nil
        self.width = width
        pending?.cancel()
        // The first width is the strip appearing, already centred; a live
        // resize and the full-screen animation say when they end.
        guard !first, !transitioning, !NSApp.windows.contains(where: { $0 is EditorWindow && $0.inLiveResize }) else { return }
        settle(after: Self.quiet)
    }

    private func settle(after delay: TimeInterval) {
        pending?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.settled() }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    private func settled() {
        pending?.cancel()
        pending = nil
        settle?()
    }

    /// Scrolls the strip to `x` over `duration`, easing in and out; at once
    /// with Reduce Motion. Moved frame by frame: SwiftUI's animated
    /// `scrollTo` takes its own quick curve whatever the animation asks,
    /// in steps of 40 pt, and to the lazy stack's idea of the middle.
    func glide(to x: CGFloat) {
        guard let scrollView else { return }
        glider?.stop()
        guard !SheetFlight.reduceMotion else { return Glider.place(scrollView, at: x) }
        let glider = Glider(scrollView: scrollView, to: x, duration: Self.duration)
        self.glider = glider
        glider.start()
    }

    /// The scroll offset that puts `middle` (in the row) in the middle of a
    /// strip `visible` wide, as far as a row `content` wide goes.
    static func target(middle: CGFloat, visible: CGFloat, content: CGFloat) -> CGFloat {
        min(max(middle - visible / 2, 0), max(content - visible, 0))
    }
}

/// One glide of the strip's clip view, a step each display frame.
@MainActor private final class Glider: NSObject {
    private weak var scrollView: NSScrollView?
    private let from: CGFloat, to: CGFloat, duration: TimeInterval
    private var link: CADisplayLink?
    private var began: CFTimeInterval?

    init(scrollView: NSScrollView, to: CGFloat, duration: TimeInterval) {
        self.scrollView = scrollView
        self.from = scrollView.contentView.bounds.origin.x
        self.to = to
        self.duration = duration
    }

    func start() {
        guard let scrollView else { return }
        let link = scrollView.displayLink(target: self, selector: #selector(tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    func stop() {
        link?.invalidate()
        link = nil
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let scrollView else { return stop() }
        let began = self.began ?? link.timestamp
        self.began = began
        let t = min((link.targetTimestamp - began) / duration, 1)
        // Ease in and out (smoothstep).
        let eased = t * t * (3 - 2 * t)
        Self.place(scrollView, at: from + (to - from) * eased)
        if t >= 1 { stop() }
    }

    static func place(_ scrollView: NSScrollView, at x: CGFloat) {
        let clip = scrollView.contentView
        clip.setBoundsOrigin(NSPoint(x: x, y: clip.bounds.origin.y))
        scrollView.reflectScrolledClipView(clip)
    }
}

/// Hands over the AppKit scroll view a SwiftUI one is drawn in.
private struct EnclosingScrollView: NSViewRepresentable {
    let found: (NSScrollView?) -> Void
    func makeNSView(context: Context) -> Probe { Probe(found: found) }
    func updateNSView(_ view: Probe, context: Context) { view.found = found }

    final class Probe: NSView {
        var found: (NSScrollView?) -> Void
        init(found: @escaping (NSScrollView?) -> Void) {
            self.found = found
            super.init(frame: .zero)
        }
        required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            found(enclosingScrollView)
        }
    }
}

private struct StripNote: View {
    let text: String
    var body: some View { Text(text).font(Theme.serif(14, italic: true)).foregroundStyle(Theme.dim) }
}

/// One frame of the strip: the picture, dimmed unless it's current, and the
/// rebate under it.
private struct StripFrame: View {
    let file: PhotoFile
    let width: CGFloat
    /// The open photograph: at full strength, with the pencil under it.
    let current: Bool
    /// Selected for the context menu: at full strength, with a plain rule under it.
    let selected: Bool
    let summary: LibrarySummary
    let readOnly: Bool
    let edited: EditedThumbnail?
    /// On and off screen: what's shown is developed first.
    let shown: (Bool) -> Void
    let loaded: (CGImage) -> Void
    /// Where its picture is drawn, for the grid's flight; nil once gone.
    let placed: (ThumbnailPlaces.Place?) -> Void
    let open: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    private struct Load: Hashable { let file: PhotoFile; let edited: EditedThumbnail? }

    var body: some View {
        Button(action: open) {
            VStack(spacing: 0) {
                StripPicture(image: image, width: width, current: current, selected: selected, rejected: summary.flag == .reject)
                    .overlay { PhotoMarks(summary: summary, name: file.name) }
                    .modifier(PlaceReport(image: image, placed: placed))
                ZStack(alignment: .top) {
                    if current {
                        PencilStroke().pencil(width: PhotoBrowser.pencilWidth).frame(height: Theme.pencilHeight).padding(.top, 3)
                    } else if selected {
                        Rectangle().fill(Theme.label).frame(height: 1.5).padding(.top, 3)
                    }
                }
                .frame(width: width, height: Theme.Size.stripRebate, alignment: .top)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(StripFrameStyle())
        .help(PhotoMarks.help(file.name, summary: summary, readOnly: readOnly))
        .accessibilityLabel(PhotoMarks.accessibilityLabel(file.name, summary: summary))
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
            if let result { image = result; loaded(result) }
        }
    }
}

/// The picture, filling its frame. Frames not current are at 0.42, 0.75
/// under the pointer and 0.9 pressed; a reject is greyed further.
private struct StripPicture: View {
    let image: CGImage?
    let width: CGFloat
    let current: Bool
    let selected: Bool
    let rejected: Bool
    @Environment(\.stripFrameState) private var state

    var body: some View {
        ZStack {
            if let image {
                Image(decorative: image, scale: 2).resizable().aspectRatio(contentMode: .fill)
            } else {
                Theme.blank
            }
        }
        .frame(width: width, height: PhotoBrowser.pictureHeight)
        .clipped()
        .opacity(opacity)
    }

    private var opacity: Double {
        if rejected { return PhotoMarks.rejectOpacity(inStrip: true, current: current || selected) }
        if current || selected { return 1 }
        return state.pressed ? 0.9 : state.hovered ? 0.75 : Theme.stripDim
    }
}

private struct StripFrameState: Equatable { var hovered = false, pressed = false }

private extension EnvironmentValues {
    @Entry var stripFrameState = StripFrameState()
}

private struct StripFrameStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { Hovering(configuration: configuration) }

    private struct Hovering: View {
        let configuration: Configuration
        @State private var hovered = false
        var body: some View {
            configuration.label
                .environment(\.stripFrameState, StripFrameState(hovered: hovered, pressed: configuration.isPressed))
                .onHover { hovered = $0 }
        }
    }
}

/// The grid's handle (§4.4): at the strip's right end the film rises into a
/// tongue cut on the slant, like the start of a roll, with an arrow pointing
/// up. It rises a little under the pointer. A click, or G, opens the grid.
struct StripLeader: View {
    let model: EditorModel
    @State private var hovered = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    static let width: CGFloat = 92
    static let height: CGFloat = 24
    static let raised: CGFloat = 30
    /// The slant takes the first 28 pt.
    static let slant: CGFloat = 28

    var body: some View {
        Button { model.showGrid() } label: {
            LeaderShape()
                .fill(Theme.film)
                .frame(width: Self.width, height: hovered ? Self.raised : Self.height)
                .overlay(alignment: .topTrailing) {
                    LeaderArrow()
                        .stroke(hovered ? Theme.paper : Theme.label, style: StrokeStyle(lineWidth: 2, lineCap: .square, lineJoin: .miter))
                        .frame(width: 12.7, height: 6.4)
                        // The arrow's box: 9 pt square, 22 pt from the right, 10 from the top.
                        .padding(.trailing, 22 + 4.5 - 6.35)
                        .padding(.top, 10 + 4.5 - 6.36 - (hovered ? 1 : 0))
                }
                .contentShape(LeaderShape())
                .frame(height: Self.raised, alignment: .bottom)
        }
        .buttonStyle(.plain)
        .onHover { over in
            withAnimation(reduceMotion ? nil : .timingCurve(0.3, 1.5, 0.5, 1, duration: 0.28)) { hovered = over }
        }
        .help("Show all photographs (G)")
        .accessibilityLabel("Show all photographs")
    }
}

/// The tongue: `polygon(0 100%, 28px 0, 100% 0, 100% 100%)`.
private struct LeaderShape: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX + StripLeader.slant, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.closeSubpath()
        return path
    }
}

/// A chevron, its point at the top middle of the rect; turned over, it points down.
struct LeaderArrow: Shape {
    var down = false
    func path(in rect: CGRect) -> Path {
        var path = Path()
        let (top, bottom) = down ? (rect.maxY, rect.minY) : (rect.minY, rect.maxY)
        path.move(to: CGPoint(x: rect.minX, y: bottom))
        path.addLine(to: CGPoint(x: rect.midX, y: top))
        path.addLine(to: CGPoint(x: rect.maxX, y: bottom))
        return path
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
