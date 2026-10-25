import Accelerate
import AppKit
import CoreVideo
import ImageIO
import IOSurface
import Observation
import SwiftUI
import UniformTypeIdentifiers

/// Observation tracks each property separately, so a finished render only
/// updates the canvas and status bar, and an edit doesn't redraw the canvas.
@MainActor @Observable final class EditorModel {
    /// The whole edit, for rendering, undo, and export. Views read the
    /// per-parameter accessors below, so an edit only invalidates its own row.
    private(set) var state = EditState() {
        didSet { publish() }
    }
    private(set) var film = EditState().film
    private(set) var paper = EditState().paper
    private(set) var limitsPreview = false
    private(set) var border = BorderSettings()
    private(set) var crop = CropSettings()
    /// The crop being drawn; nil unless cropping.
    private(set) var cropSession: CropSession?
    /// The film border's frame, loaded from the engine's data folder.
    private(set) var filmTexture: CGImage?
    /// The 35mm strip for the film and the photograph's format, loaded while
    /// that border is chosen.
    private(set) var filmStrip: FilmStrip?
    private(set) var stocks: [Stock] = []
    private(set) var source: URL?
    /// The developed preview. Nil until the first render of `source` completes.
    private(set) var image: IOSurface?
    /// The unprocessed photograph, shown while the first render develops.
    private(set) var placeholder: CGImage?
    /// The undeveloped photograph, shown in place of `image` while the
    /// Original button is held.
    private(set) var original: IOSurface?
    /// The tones of `image` within the crop; nil until it develops.
    private(set) var histogram: Histogram?
    private(set) var imageSize = CGSize.zero
    private(set) var placeholderSize = CGSize.zero
    private(set) var imageRotation = 0
    private(set) var isRaw = false
    private(set) var ready = false
    private(set) var rendering = false
    private(set) var previewCurrent = false
    /// The shown preview is out of date, and the update is slow enough to mention.
    private(set) var showsActivity = false
    /// The latest edit couldn't be developed; the last good preview stays.
    private(set) var renderFailed = false
    private(set) var saving = false
    private(set) var exporting = false
    private(set) var exportStarted = Date()
    var error: String?
    var status = "Starting…"
    var filmstripVisible = UserDefaults.standard.object(forKey: "filmstripVisible") as? Bool ?? true {
        didSet { UserDefaults.standard.set(filmstripVisible, forKey: "filmstripVisible") }
    }
    var inspectorVisible = true
    var zoomCommand = UUID()
    var fitImage = true
    var recentFiles: [URL] = []
    @ObservationIgnored weak var undoManager: UndoManager?
    @ObservationIgnored private var defaults: JSONValue = .object([:])
    @ObservationIgnored private var catalog: JSONValue = .null
    @ObservationIgnored private var catalogRequest: Task<(Int, JSONValue), Error>?
    @ObservationIgnored private var engine: EngineClient?
    @ObservationIgnored private var exportJob: ExportJob?
    @ObservationIgnored private var revision = 0
    @ObservationIgnored private var sourceGeneration = 0
    @ObservationIgnored private var renderID = 0
    /// The last render is at export size: saving it gives the same file as
    /// a Fast export, byte for byte.
    @ObservationIgnored private var renderAtExportSize = false
    @ObservationIgnored private var renderOnGPU = false
    /// The shown frame was rendered at canvas size while a slider moved;
    /// letting go renders the photograph at export size.
    @ObservationIgnored private var showingInteractiveFrame = false
    @ObservationIgnored private var showingCachedPreview = false
    @ObservationIgnored private let previewCache: RenderedPreviewCache
    @ObservationIgnored let canvasScale = CanvasScale()
    @ObservationIgnored private var pendingRender: Task<Void, Never>?
    @ObservationIgnored private var activityTimer: Task<Void, Never>?
    @ObservationIgnored private var interactionStart: EditState?
    /// The last border colour change, which the next one to the same colour
    /// joins if nothing else changed in between.
    @ObservationIgnored private var lastColorEdit: (setting: WritableKeyPath<BorderSettings, BorderColor>, state: EditState, at: Date)?
    @ObservationIgnored private var closing = false
    @ObservationIgnored private var explicitOpenRequested = false
    @ObservationIgnored private var parameters: [String: ObservedParameter] = [:]
    @ObservationIgnored private var stripRequest: (film: String, format: FilmFormat)?
    @ObservationIgnored private var histogramRequest = 0
    /// The source's original, decoded on first use and kept until another
    /// photograph opens. Volatile while hidden.
    @ObservationIgnored private var originalCache: IOSurface?
    @ObservationIgnored private var originalLoad: Task<Void, Never>?
    /// The Original button is held; `original` is nil until its decode finishes.
    private(set) var holdingOriginal = false

    /// The stocks and defaults are known, from the helper or the last launch's
    /// catalog. Edits are possible; rendering waits for `ready`.
    var hasCatalog: Bool { !stocks.isEmpty }
    var films: [Stock] { stocks.filter { !$0.paper } }
    var papers: [Stock] { stocks.filter(\.paper) }
    var filmGroups: [StockGroup] { StockGroup.films(films, papers: papers) }
    var paperGroups: [StockGroup] { StockGroup.papers(papers) }
    var filmName: String { films.first { $0.id == film }?.name ?? film }
    var paperName: String { papers.first { $0.id == paper }?.name ?? paper }
    var canSave: Bool { previewCurrent && image != nil && !rendering && !saving && !cropping }
    var cropping: Bool { cropSession != nil }
    var canShowOriginal: Bool { image != nil && !cropping }
    /// The shown photograph's size after cropping, before rotation; zero
    /// before one opens. Borders fit this size.
    var photoSize: CGSize { crop.size(of: image != nil ? imageSize : placeholderSize) }
    /// The developed photograph as shown, uncropped, after rotation.
    private var shownImageSize: CGSize { imageRotation % 2 != 0 ? CGSize(width: imageSize.height, height: imageSize.width) : imageSize }
    /// A slider is being dragged; its edits share one undo step.
    var interacting: Bool { interactionStart != nil }

    init(previewCache: RenderedPreviewCache = RenderedPreviewCache()) {
        self.previewCache = previewCache
        recentFiles = (UserDefaults.standard.stringArray(forKey: "recentFiles") ?? []).map { URL(fileURLWithPath: $0) }
    }

    /// Starts the helper and asks for its catalog at once, so both load while
    /// AppKit builds the window. Call it before building the inspector: the
    /// last launch's catalog, if the helper is unchanged, fills in the stocks
    /// and defaults so the inspector's first frame is complete.
    func launch(paths: EnginePaths? = nil) {
        guard !closing, engine == nil else { return }
        do {
            let engine = try EngineClient(paths: paths ?? EnginePaths.locate())
            self.engine = engine
            catalogRequest = Task.detached { try await engine.request(["command": .string("catalog")]) }
            let data = engine.paths.data
            Task.detached(priority: .utility) { [weak self] in
                let texture = BorderStyle.loadFilmTexture(data: data)
                await self?.setFilmTexture(texture)
            }
            if let cached = CatalogCache.load(for: engine.paths) { apply(catalog: cached) }
        } catch { report(error) }
    }

    func start(paths: EnginePaths? = nil, restoration: Task<EditorSession?, Error>? = nil) async {
        launch(paths: paths)
        guard let request = catalogRequest, let engine else { return }
        catalogRequest = nil
        do {
            let (_, catalog) = try await request.value
            guard !closing else { return }
            guard catalog["protocol"].double == 1 else { throw EngineError(message: "The image engine version doesn't match this app. Rebuild the app bundle.") }
            // An unchanged catalog keeps edits made while the helper started.
            if catalog != self.catalog {
                apply(catalog: catalog)
                CatalogCache.save(catalog, for: engine.paths)
            }
            // The engine and session file load concurrently. Apply the restored
            // parameters before ready/changed so no default render is wasted.
            if let restoration, !explicitOpenRequested {
                do {
                    if let session = try await restoration.value, !explicitOpenRequested, !closing {
                        try restore(session)
                    }
                } catch {
                    if !explicitOpenRequested { status = error.localizedDescription }
                }
            }
            guard !closing else { return }
            ready = true
            if source == nil, status == "Starting…" { status = "" }
            if source != nil { changed(invalidateCache: false) }
        } catch { if !closing { report(error) } }
    }

    private func apply(catalog: JSONValue) {
        self.catalog = catalog
        stocks = catalog["profiles"].array.map(Stock.init)
        defaults = catalog["defaults"]
        // Keep anything already applied to the photograph, such as its rotation.
        let rotation = state.params["io.rotate_cw"]
        var next = state
        next.params = defaults
        if rotation != .null { next.params["io.rotate_cw"] = rotation }
        if !films.contains(where: { $0.id == next.film }), let first = films.first { next.film = first.id }
        if !papers.contains(where: { $0.id == next.paper }), let first = papers.first { next.paper = first.id }
        pairFilm(in: &next)
        state = next
    }

    private func setFilmTexture(_ texture: CGImage?) { filmTexture = texture }

    /// Loads the 35mm strip when that border is chosen and the film or the
    /// photograph's format changes.
    private func updateFilmStrip() {
        guard border.enabled, border.style == .film35, let format = FilmFormat(photoSize), let data = engine?.paths.data else { return }
        if let filmStrip, filmStrip.film == film, filmStrip.format == format { return }
        if let stripRequest, stripRequest == (film, format) { return }
        stripRequest = (film, format)
        let film = film
        Task.detached(priority: .userInitiated) { [weak self] in
            let strip = FilmStrip.load(data: data, film: film, format: format)
            await self?.setFilmStrip(strip, film: film, format: format)
        }
    }
    private func setFilmStrip(_ strip: FilmStrip?, film: String, format: FilmFormat) {
        guard let stripRequest, stripRequest == (film, format) else { return }
        self.stripRequest = nil
        filmStrip = strip
        if strip == nil { error = "The 35mm border for \(filmName) couldn’t be loaded." }
    }

    // MARK: Parameters

    func value(_ path: String) -> JSONValue {
        if let parameter = parameters[path] { return parameter.value }
        let parameter = ObservedParameter(state.params[path])
        parameters[path] = parameter
        return parameter.value
    }
    private func publish() {
        for (path, parameter) in parameters {
            let current = state.params[path]
            if parameter.value != current { parameter.value = current }
        }
        if film != state.film { film = state.film }
        if paper != state.paper { paper = state.paper }
        if limitsPreview != state.limitPreview { limitsPreview = state.limitPreview }
        if border != state.border { border = state.border }
        if crop != state.crop { crop = state.crop; updateHistogram() }
        let rotation = Int(state.params["io.rotate_cw"].double)
        if imageRotation != rotation {
            imageRotation = rotation
            cropSession?.update(photo: shownImageSize, rotation: rotation)
        }
        updateFilmStrip()
    }
    func defaultValue(_ path: String) -> JSONValue { defaults[path] }
    func isModified(_ path: String) -> Bool {
        let (current, original) = (value(path), defaults[path])
        if case .number(let a) = current, case .number(let b) = original {
            // Rust defaults arrive as widened f32 values; slider edits are decimal.
            return abs(a - b) > 1e-6 * max(1, abs(b))
        }
        return current != original
    }
    func set(_ path: String, _ value: JSONValue) {
        var next = state; next.params[path] = value
        apply(next, action: "Change Adjustment")
    }
    func resetValue(_ path: String) { set(path, defaults[path]) }
    func number(_ path: String, range: ClosedRange<Double>, step: Double) -> Binding<Double> {
        Binding(get: { self.value(path).double }, set: { self.set(path, .number(Self.snap($0, to: range, step: step))) })
    }
    func toggle(_ path: String) -> Binding<Bool> {
        Binding(get: { self.value(path).bool }, set: { self.set(path, .bool($0)) })
    }
    func choice(_ path: String) -> Binding<String> {
        Binding(get: { self.value(path).string }, set: { self.set(path, .string($0)) })
    }

    /// Sliders are continuous to avoid AppKit tick marks; the engine expects
    /// the stepped value (integer fields reject fractions).
    nonisolated static func snap(_ value: Double, to range: ClosedRange<Double>, step: Double) -> Double {
        let clamped = min(max(value, range.lowerBound), range.upperBound)
        let steps = ((clamped - range.lowerBound) / step).rounded()
        let decimals = max(0, Int(-log10(step).rounded(.down)) + 1)
        let scale = pow(10, Double(decimals))
        return min(((range.lowerBound + steps * step) * scale).rounded() / scale, range.upperBound)
    }

    func beginInteraction() {
        if interactionStart == nil { interactionStart = state }
        hideActivity()
    }
    func endInteraction(action: String = "Change Adjustment") {
        guard let start = interactionStart else { return }
        interactionStart = nil
        if start != state { registerUndo(restoring: start, action: action) }
        if showingInteractiveFrame, ready, source != nil, !closing { scheduleRender() }
        if !previewCurrent { scheduleActivity() }
    }

    func chooseFilm(_ stock: String) {
        var next = state; next.film = stock
        next.params["film_render.development_time"] = .null
        pairFilm(in: &next); apply(next, action: "Change Film")
    }
    func choosePaper(_ stock: String) {
        var next = state; next.paper = stock; next.params["print_render.development_time"] = .null
        apply(next, action: "Change Paper")
    }
    private func pairFilm(in state: inout EditState) {
        guard let film = films.first(where: { $0.id == state.film }) else { return }
        state.params["io.scan_film"] = .bool(film.positive)
        if papers.contains(where: { $0.id == film.target }), state.paper != film.target {
            state.paper = film.target; state.params["print_render.development_time"] = .null
        }
    }
    func limitPreview(_ enabled: Bool) {
        var next = state; next.limitPreview = enabled; apply(next, action: "Change Preview Resolution")
    }
    func border<Value>(_ setting: WritableKeyPath<BorderSettings, Value>) -> Binding<Value> {
        Binding(get: { self.border[keyPath: setting] }, set: {
            var next = self.state; next.border[keyPath: setting] = $0; self.apply(next, action: "Change Border")
        })
    }
    /// A border width as a percentage of the long edge, stepped like the
    /// adjustment sliders. Back at the default it stores the default exactly,
    /// so exports match ones made before widths could change.
    func borderPercent(_ setting: WritableKeyPath<BorderSettings, Double>, range: ClosedRange<Double>, step: Double) -> Binding<Double> {
        Binding(get: { self.border[keyPath: setting] * 100 }, set: {
            let fraction = Self.snap($0, to: range, step: step) / 100, initial = BorderSettings()[keyPath: setting]
            self.border(setting).wrappedValue = abs(fraction - initial) < 1e-9 ? initial : fraction
        })
    }
    /// The colour panel sends a stream of colours while one is dragged, so
    /// one undo step covers each run of changes to the same colour.
    func borderColor(_ setting: WritableKeyPath<BorderSettings, BorderColor>) -> Binding<CGColor> {
        Binding(get: { self.border[keyPath: setting].cgColor }, set: {
            var next = self.state; next.border[keyPath: setting] = BorderColor($0)
            guard next != self.state else { return }
            if let last = self.lastColorEdit, last.setting == setting, last.state == self.state, Date().timeIntervalSince(last.at) < 1 {
                self.replaceState(next)
            } else {
                self.apply(next, action: "Change Border Color")
            }
            self.lastColorEdit = (setting, next, Date())
        })
    }
    /// Keeps the border and crop, like the film, paper, and rotation.
    func reset() {
        guard !cropping else { return }
        var next = state; next.params = defaults; next.limitPreview = false
        next.params["io.rotate_cw"] = state.params["io.rotate_cw"]
        next.params["io.scan_film"] = .bool(films.first(where: { $0.id == next.film })?.positive ?? false)
        apply(next, action: "Reset Adjustments")
    }
    private func apply(_ next: EditState, action: String) {
        guard next != state else { return }
        if !interacting { registerUndo(restoring: state, action: action) }
        replaceState(next)
    }
    /// A border, crop, or rotation change only redraws the canvas; anything
    /// else re-renders.
    private func replaceState(_ next: EditState) {
        guard !closing else { return }
        let render = next.rendered != state.rendered
        state = next
        if render { changed() }
    }
    private func registerUndo(restoring previous: EditState, action: String) {
        undoManager?.registerUndo(withTarget: self) { model in
            guard previous != model.state else { return }
            model.registerUndo(restoring: model.state, action: action)
            model.replaceState(previous)
        }
        undoManager?.setActionName(action)
    }
    func rotate() {
        let turned = JSONValue.number(Double((Int(value("io.rotate_cw").double) + 1) % 4))
        if cropping {
            // Turns with the crop: one undo step on Done, undone by Cancel.
            var next = state; next.params["io.rotate_cw"] = turned; replaceState(next)
        } else {
            set("io.rotate_cw", turned)
            zoom(fit: true)
        }
    }
    func zoom(fit: Bool) { fitImage = fit; zoomCommand = UUID() }

    var session: EditorSession? {
        guard !closing, ready, let source else { return nil }
        var saved = state
        // Include a crop being drawn, without forcing the user to press Done.
        if let cropSession { saved.crop = cropSession.settings }
        return EditorSession(source: source, state: saved, inspectorVisible: inspectorVisible, fitImage: fitImage)
    }

    private func restore(_ session: EditorSession) throws {
        try session.validate()
        guard films.contains(where: { $0.id == session.state.film }),
              papers.contains(where: { $0.id == session.state.paper }) else {
            throw EngineError(message: "The previous session uses an unavailable film or paper. Open a photograph to continue.")
        }
        open(session.source, restoring: session)
    }

    // MARK: Cropping

    /// Shows the whole developed photograph, without the border, to crop it.
    func beginCrop() {
        guard !cropping, image != nil, imageSize != .zero else { return }
        holdOriginal(false)
        cropSession = CropSession(start: state, photo: shownImageSize, rotation: imageRotation)
    }
    /// Done applies the crop, and any rotation while cropping, as one undo
    /// step. Cancel restores the rotation and keeps the previous crop.
    func endCrop(apply: Bool) {
        guard let session = cropSession else { return }
        cropSession = nil
        let rotation = session.start.params["io.rotate_cw"]
        if apply {
            var previous = state; previous.crop = session.start.crop; previous.params["io.rotate_cw"] = rotation
            var next = state; next.crop = session.settings
            if next != previous { registerUndo(restoring: previous, action: "Crop") }
            replaceState(next)
        } else if state.params["io.rotate_cw"] != rotation {
            var next = state; next.params["io.rotate_cw"] = rotation; replaceState(next)
        }
        zoom(fit: true)
    }

    // MARK: Opening

    nonisolated static let rawExtensions = ["dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "ptx", "srw", "x3f", "iiq", "3fr", "ari", "bay", "crw", "dcr", "drf", "erf", "fff", "k25", "kdc", "mef", "mos", "mrw", "rwl"]

    func openPanel() {
        let panel = NSOpenPanel()
        panel.title = "Open Photograph"
        panel.allowedContentTypes = [.png, .tiff, .jpeg, .rawImage] + Self.rawExtensions.compactMap { UTType(filenameExtension: $0) }
        panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        if let source {
            // A file URL reveals and selects the file; a parent URL only opens its folder.
            panel.directoryURL = source
        }
        panel.begin { [weak self] response in
            if response == .OK, let url = panel.url { self?.open(url) }
        }
    }

    func open(_ url: URL) {
        explicitOpenRequested = true
        open(url, restoring: nil)
    }

    private func open(_ url: URL, restoring session: EditorSession?) {
        guard !closing, url.isFileURL else { return }
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            recentFiles.removeAll { $0 == url }; saveRecents()
            error = "“\(url.lastPathComponent)” can’t be opened. It may have been moved or deleted."
            return
        }
        source = url; image = nil; placeholder = nil; previewCurrent = false; renderFailed = false
        showingCachedPreview = false
        updateHistogram()
        interactionStart = nil; cropSession = nil
        original = nil; originalCache = nil; originalLoad?.cancel(); originalLoad = nil; holdingOriginal = false
        sourceGeneration += 1
        let generation = sourceGeneration
        if let session {
            var restored = session.state
            restored.params = defaults.restoring(restored.params)
            state = restored
            inspectorVisible = session.inspectorVisible
        } else {
            state.params["io.rotate_cw"] = .number(0)
            state.crop = CropSettings()
        }
        isRaw = Self.rawExtensions.contains(url.pathExtension.lowercased())
        undoManager?.removeAllActions()
        zoom(fit: session?.fitImage ?? true)
        recentFiles.removeAll { $0 == url }; recentFiles.insert(url, at: 0)
        recentFiles = Array(recentFiles.prefix(10)); saveRecents()
        NSDocumentController.shared.noteNewRecentDocumentURL(url)
        status = ready ? "" : "Starting…"
        // Try the exact developed frame before decoding the raw placeholder,
        // so a cache hit never flashes the unprocessed photograph.
        let snapshot = state, paths = engine?.paths, cache = previewCache
        Task { [weak self] in
            if session != nil, let paths,
               let cached = await cache.load(source: url, state: snapshot, paths: paths) {
                guard let self, generation == self.sourceGeneration, self.image == nil, !self.closing else { return }
                if self.state.rendered == snapshot.rendered {
                    self.image = cached.surface; self.imageSize = cached.photoSize; self.isRaw = cached.raw
                    self.showingCachedPreview = true; self.placeholder = nil; self.hideActivity()
                    self.updateHistogram(); self.updateFilmStrip()
                    return
                }
            }
            let thumbnail = await Task.detached(priority: .userInitiated) { Self.thumbnail(url) }.value
            if let (thumbnail, size) = thumbnail { self?.showPlaceholder(thumbnail, size: size, generation: generation) }
        }
        changed(invalidateCache: session == nil)
    }
    private func showPlaceholder(_ thumbnail: CGImage, size: CGSize, generation: Int) {
        guard generation == sourceGeneration, image == nil else { return }
        placeholderSize = size; placeholder = thumbnail
        updateFilmStrip()
    }
    private func saveRecents() { UserDefaults.standard.set(recentFiles.map(\.path), forKey: "recentFiles") }

    /// A display-sized decode through ImageIO, with EXIF orientation applied
    /// like the engine's loader. RAW files use their embedded preview.
    nonisolated static func thumbnail(_ url: URL, maxPixels: Int = 2048) -> (CGImage, CGSize)? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary) else { return nil }
        let raw = rawExtensions.contains(url.pathExtension.lowercased())
        let options: [CFString: Any] = [
            raw ? kCGImageSourceCreateThumbnailFromImageIfAbsent : kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
            kCGImageSourceShouldCacheImmediately: true
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        // Lay the thumbnail out at the photograph's full size so the canvas
        // doesn't jump when the developed render arrives.
        var size = CGSize(width: image.width, height: image.height)
        if !raw, let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
           let width = properties[kCGImagePropertyPixelWidth] as? Int, let height = properties[kCGImagePropertyPixelHeight] as? Int {
            let turned = ((properties[kCGImagePropertyOrientation] as? Int) ?? 1) >= 5
            size = turned ? CGSize(width: height, height: width) : CGSize(width: width, height: height)
        }
        return (image, size)
    }

    // MARK: Original

    /// Decodes the original ahead of a press, such as when the pointer
    /// reaches the button.
    func prepareOriginal() {
        if canShowOriginal, originalCache == nil { loadOriginal() }
    }

    /// Shows the original in place of the developed photograph while held.
    /// The canvas keeps its crop, rotation, and border.
    func holdOriginal(_ held: Bool) {
        guard held, canShowOriginal else {
            holdingOriginal = false
            if let shown = original {
                original = nil
                // Core Animation may still be compositing it for a frame.
                Task { [weak self] in
                    try? await Task.sleep(for: .seconds(1))
                    guard let self, !self.holdingOriginal, self.originalCache === shown else { return }
                    shown.makeVolatile()
                }
            }
            return
        }
        guard !holdingOriginal else { return }
        holdingOriginal = true
        if let cached = originalCache {
            if cached.makeNonVolatile() { original = cached; return }
            originalCache = nil
        }
        loadOriginal()
    }

    /// Decodes at the developed preview's resolution, so it is as sharp at
    /// actual size.
    private func loadOriginal() {
        guard originalLoad == nil, let source, let image else { return }
        let raw = isRaw, maxPixels = max(image.width, image.height), generation = sourceGeneration
        originalLoad = Task { [weak self] in
            let surface = await Task.detached(priority: .userInitiated) {
                OriginalImage.load(source, raw: raw, maxPixels: maxPixels)
            }.value
            guard let self, generation == self.sourceGeneration, !Task.isCancelled else { return }
            self.originalLoad = nil
            guard let surface else {
                if self.holdingOriginal {
                    self.holdingOriginal = false
                    self.status = "The original of \(source.lastPathComponent) couldn’t be shown"
                }
                return
            }
            self.originalCache = surface
            if self.holdingOriginal { self.original = surface } else { surface.makeVolatile() }
        }
    }

    // MARK: Rendering

    private func changed(invalidateCache: Bool = true) {
        if invalidateCache { previewCache.invalidate(); showingCachedPreview = false }
        revision += 1; previewCurrent = false
        if renderFailed { renderFailed = false }
        guard ready, source != nil, !closing else { return }
        scheduleRender()
        scheduleActivity()
    }
    /// Renders run one at a time; edits made meanwhile coalesce into the next.
    private func scheduleRender() {
        guard pendingRender == nil else { return }
        pendingRender = Task { [weak self] in
            await Task.yield()
            guard let self else { return }
            self.pendingRender = nil
            await self.render()
        }
    }
    private func render() async {
        guard !rendering, !closing, let source, let engine else { return }
        let currentRevision = revision, snapshot = state, generation = sourceGeneration
        // While a slider moves, render only as many pixels as the canvas
        // shows; the frame on release is the export-size one.
        let interactiveScale = interacting ? canvasScale.screenPixelsPerPhotoPixel.flatMap { $0 < 1 ? $0 : nil } : nil
        rendering = true
        let buffer = engine.directory.appendingPathComponent("preview.rgba")
        defer {
            rendering = false
            if revision != currentRevision || (interactiveScale != nil && !interacting), !closing { scheduleRender() }
        }
        do {
            let cacheKey = await Task.detached(priority: .utility) {
                interactiveScale == nil ? RenderedPreviewCache.key(source: source, state: snapshot, paths: engine.paths) : nil
            }.value
            guard shouldShow(currentRevision, generation) else { return }
            var command: [String: JSONValue] = [
                "command": .string("render"), "input": .string(source.path),
                "source_generation": .number(Double(generation)),
                "film": .string(snapshot.film), "paper": .string(snapshot.paper), "params": snapshot.params,
                "limit_preview": .bool(snapshot.limitPreview), "preview_path": .string(buffer.path)
            ]
            if let interactiveScale { command["interactive_scale"] = .number(Double(interactiveScale)) }
            let (id, result) = try await engine.request(command)
            guard shouldShow(currentRevision, generation) else { return }
            let width = Int(result["width"].double), height = Int(result["height"].double)
            let developed = try await Task.detached(priority: .userInitiated) {
                try Self.readPreview(buffer, width: width, height: height)
            }.value
            guard shouldShow(currentRevision, generation) else { return }
            imageSize = CGSize(width: result["photo_width"].double, height: result["photo_height"].double)
            showingInteractiveFrame = interactiveScale != nil
            image = developed; placeholder = nil
            updateHistogram()
            cropSession?.update(photo: shownImageSize, rotation: imageRotation)
            updateFilmStrip()
            isRaw = result["raw"].bool
            if revision == currentRevision, interactiveScale == nil {
                renderID = id; previewCurrent = true; hideActivity()
                showingCachedPreview = false
                renderAtExportSize = result["export_resolution"].bool
                renderOnGPU = ExportEngine.ranOnGPU(backend: result["backend"].string)
                if let cacheKey { previewCache.save(developed, photoSize: imageSize, raw: isRaw, key: cacheKey) }
            }
            let seconds = result["elapsed_ms"].double / 1000
            let time = "developed in \(seconds.formatted(.number.precision(.fractionLength(seconds < 10 ? 2 : 1)))) s"
            status = interactiveScale != nil
                ? "Preview · \(Int(result["output_width"].double)) × \(Int(result["output_height"].double)) · \(time)"
                : "\(Int(imageSize.width)) × \(Int(imageSize.height)) · \(time)"
        } catch {
            if currentRevision == revision, generation == sourceGeneration, !closing {
                renderFailed = true; hideActivity(); report(error)
            }
        }
    }

    /// While a slider moves, show each finished frame. Otherwise only the
    /// latest edit, so a discrete change never flashes an older state.
    private func shouldShow(_ renderedRevision: Int, _ generation: Int) -> Bool {
        !closing && generation == sourceGeneration && (revision == renderedRevision || interacting)
    }

    /// Counts the preview's tones off the main thread. A newer preview or
    /// crop supersedes a count still running.
    private func updateHistogram() {
        histogramRequest += 1
        guard let image else { histogram = nil; return }
        let request = histogramRequest, crop = crop
        Task.detached(priority: .userInitiated) { [weak self] in
            let counted = Histogram.of(image, crop: crop)
            await self?.showHistogram(counted, request: request)
        }
    }
    private func showHistogram(_ counted: Histogram, request: Int) {
        guard request == histogramRequest, counted != histogram else { return }
        histogram = counted
    }

    /// Only mention an update once it has visibly taken a while.
    private func scheduleActivity() {
        guard activityTimer == nil, !showsActivity else { return }
        activityTimer = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            guard let self, !Task.isCancelled else { return }
            self.activityTimer = nil
            if !self.previewCurrent, !self.showingCachedPreview, !self.interacting, !self.renderFailed, self.source != nil {
                self.showsActivity = true
            }
        }
    }
    private func hideActivity() {
        activityTimer?.cancel(); activityTimer = nil
        if showsActivity { showsActivity = false }
    }

    /// Reads the engine's RGBA buffer into an sRGB-tagged IOSurface. Core
    /// Animation composites it directly, with no conversion on the main thread.
    nonisolated static func readPreview(_ url: URL, width: Int, height: Int) throws -> IOSurface {
        try previewSurface(Data(contentsOf: url), width: width, height: height)
    }

    nonisolated static func previewSurface(_ data: Data, width: Int, height: Int, bgra: Bool = false) throws -> IOSurface {
        let invalid = EngineError(message: "The image engine returned an invalid preview buffer.")
        guard width > 0, height > 0, width <= 8192, height <= 8192 else { throw EngineError(message: "Invalid preview dimensions") }
        guard data.count == width * height * 4,
              let surface = IOSurface(properties: [
                  .width: width, .height: height, .bytesPerElement: 4,
                  .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
                  .pixelFormat: kCVPixelFormatType_32BGRA
              ]),
              let sRGB = CGColorSpace(name: CGColorSpace.sRGB)?.copyPropertyList()
        else { throw invalid }
        surface.lock(options: [], seed: nil)
        defer { surface.unlock(options: [], seed: nil) }
        let permuted = data.withUnsafeBytes { raw -> vImage_Error in
            var source = vImage_Buffer(data: UnsafeMutableRawPointer(mutating: raw.baseAddress), height: vImagePixelCount(height), width: vImagePixelCount(width), rowBytes: width * 4)
            var destination = vImage_Buffer(data: surface.baseAddress, height: vImagePixelCount(height), width: vImagePixelCount(width), rowBytes: surface.bytesPerRow)
            // RGBA → BGRA, and opaque regardless of the engine's alpha byte.
            let error = vImagePermuteChannels_ARGB8888(&source, &destination, bgra ? [0, 1, 2, 3] : [2, 1, 0, 3], vImage_Flags(kvImageNoFlags))
            return error == kvImageNoError ? vImageOverwriteChannelsWithScalar_ARGB8888(255, &destination, &destination, 0x1, vImage_Flags(kvImageNoFlags)) : error
        }
        guard permuted == kvImageNoError else { throw invalid }
        IOSurfaceSetValue(surface, kIOSurfaceColorSpace, sRGB)
        return surface
    }

    // MARK: Saving

    func savePanel(export: Bool) {
        guard let source, !cropping, export ? !exporting : canSave else { return }
        let panel = NSSavePanel()
        panel.title = export ? "Export Photograph" : "Save Preview"
        panel.canCreateDirectories = true
        // Without a directory the panel shows the last folder of any panel,
        // usually the Open panel's, so start where the last save went.
        if let folder = Self.lastSaveFolder { panel.directoryURL = folder }
        var extras = ""
        if state.crop.cropped { extras += " Crops the photograph." }
        if let border = state.border.exported(for: photoSize) {
            extras += " Adds the \(border.style.title.lowercased()) border\(border.instagram ? ", extended to 3:4 for Instagram" : "")."
        }
        let picker = FormatPicker(panel: panel, selected: OutputFormat.last, engine: export ? ExportEngine.selected : nil, size: ExportSize.last) { engine, size in
            let extras = extras + (size.summary.map { " \($0)." } ?? "")
            return switch engine {
            case .reference?: "The most precise result. Can take about half a minute for large photos.\(extras)"
            case .fast?: "Looks the same as Reference, in a fraction of the time.\(extras)"
            case nil: "Saves the preview as shown.\(extras)"
            }
        }
        panel.nameFieldStringValue = "\(source.deletingPathExtension().lastPathComponent)_\(state.film).\(picker.format.type.preferredFilenameExtension ?? "png")"
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            guard !Self.sameFile(url, source) else {
                self.error = "Choose a different filename to preserve the original photograph."; return
            }
            OutputFormat.last = picker.format
            ExportSize.last = picker.size
            Self.lastSaveFolder = url.deletingLastPathComponent()
            let engine = picker.engine ?? .reference
            let size = picker.size
            if export { ExportEngine.selected = engine }
            Task { if export { await self.export(to: url, engine: engine, size: size) } else { await self.savePreview(to: url, size: size) } }
        }
    }

    /// The folder of the last save or export, if it still exists.
    static var lastSaveFolder: URL? {
        get {
            guard let path = UserDefaults.standard.string(forKey: "lastSaveFolder") else { return nil }
            var isDirectory: ObjCBool = false
            return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && isDirectory.boolValue ? URL(fileURLWithPath: path, isDirectory: true) : nil
        }
        set { UserDefaults.standard.set(newValue?.path, forKey: "lastSaveFolder") }
    }

    /// Path comparison misses case-only differences on case-insensitive
    /// volumes and hard links, so compare file identity when both exist.
    nonisolated static func sameFile(_ a: URL, _ b: URL) -> Bool {
        if a.resolvingSymlinksInPath().standardizedFileURL == b.resolvingSymlinksInPath().standardizedFileURL { return true }
        let key: Set<URLResourceKey> = [.fileResourceIdentifierKey]
        guard let x = try? a.resourceValues(forKeys: key).fileResourceIdentifier,
              let y = try? b.resourceValues(forKeys: key).fileResourceIdentifier else { return false }
        return x.isEqual(y)
    }

    /// Saves the current preview with the crop, rotation, border, and size.
    private func saveCommand(to url: URL, size: ExportSize) -> [String: JSONValue] {
        var command: [String: JSONValue] = ["command": .string("save"), "render_id": .number(Double(renderID)), "output": .string(url.path),
                                            "rotate_cw": .number(Double(imageRotation))]
        if state.crop.cropped {
            let r = state.crop.rect
            command["crop"] = .object(["x": .number(r.minX), "y": .number(r.minY), "width": .number(r.width), "height": .number(r.height), "angle": .number(state.crop.angle)])
        }
        if let border = state.border.exported(for: photoSize) {
            command["border"] = .string(border.style.rawValue)
            command["border_options"] = .object(border.options)
            command["instagram"] = .bool(border.instagram)
        }
        if let fit = size.json { command["fit"] = fit }
        return command
    }

    private func savePreview(to url: URL, size: ExportSize = .full) async {
        guard canSave, let engine else { return }
        saving = true
        defer { saving = false }
        do {
            _ = try await engine.request(saveCommand(to: url, size: size))
            status = "Saved \(url.lastPathComponent)"
        } catch { report(error) }
    }

    /// A current preview at export size is what a Fast export would render,
    /// so save it instead of developing the photograph again. Like ExportJob,
    /// write a sibling temporary file and rename it, so a failure leaves an
    /// existing file untouched. False if the preview could not be saved.
    private func exportPreview(to url: URL, size: ExportSize) async -> Bool {
        guard canSave, renderAtExportSize, let engine else { return false }
        saving = true
        defer { saving = false }
        let staging = url.deletingLastPathComponent().appendingPathComponent(".negs-\(UUID().uuidString).\(url.pathExtension)")
        defer { try? FileManager.default.removeItem(at: staging) }
        guard (try? await engine.request(saveCommand(to: staging, size: size))) != nil else { return false }
        guard rename(staging.path, url.path) == 0 else {
            report(EngineError(message: "Could not move the completed export to \(url.path): \(String(cString: strerror(errno)))"))
            return true
        }
        status = renderOnGPU
            ? "Exported \(url.lastPathComponent)"
            : "Exported \(url.lastPathComponent) on the CPU (32-bit): no GPU was available for Fast export"
        return true
    }

    func export(to url: URL, engine exportEngine: ExportEngine, size: ExportSize = .full) async {
        // An edit made meanwhile makes the save fail; develop the photograph then.
        if exportEngine == .fast, await exportPreview(to: url, size: size) { return }
        guard !exporting, let source, let engine else { return }
        let job = ExportJob(); exportJob = job; exporting = true; exportStarted = Date()
        defer { exporting = false; exportJob = nil }
        do {
            let outcome = try await job.run(paths: engine.paths, scratch: engine.scratch, source: source, destination: url, state: state,
                                            crop: state.crop.cropped ? state.crop : nil, border: state.border.exported(for: photoSize), size: size, engine: exportEngine)
            status = outcome.fellBackToCPU
                ? "Exported \(url.lastPathComponent) on the CPU (32-bit): no GPU was available for Fast export"
                : "Exported \(url.lastPathComponent)"
        } catch is CancellationError { status = "Export cancelled" }
        catch { report(error) }
    }
    func cancelExport() { exportJob?.cancel() }
    func flushPreviewCache() async { await previewCache.flush() }
    func shutdown() {
        closing = true; pendingRender?.cancel(); activityTimer?.cancel()
        exportJob?.cancel(); engine?.stop()
    }
    private func report(_ error: Error) { self.error = error.localizedDescription; status = "Could not complete the operation" }
}

/// One parameter's value, observed on its own.
@Observable final class ObservedParameter {
    var value: JSONValue
    init(_ value: JSONValue) { self.value = value }
}

/// The engine picks the encoder from the file extension.
enum OutputFormat: String, CaseIterable {
    case png, jpeg, tiff

    var type: UTType {
        switch self {
        case .png: .png
        case .jpeg: .jpeg
        case .tiff: .tiff
        }
    }
    var title: String {
        switch self {
        case .png: "PNG (lossless, 8-bit)"
        case .jpeg: "JPEG (smaller, 8-bit)"
        case .tiff: "TIFF (16-bit)"
        }
    }

    /// The format of the last save or export, offered first next time.
    static var last: OutputFormat {
        get { UserDefaults.standard.string(forKey: "outputFormat").flatMap(OutputFormat.init) ?? .png }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "outputFormat") }
    }
}

/// The size a save or export is fitted to, like `Fit` in
/// `spektrafilm-io::resize`: full size, or a number of pixels on the long or
/// short edge of the finished file, border included. Never enlarged.
struct ExportSize: Equatable {
    enum Edge: String, CaseIterable {
        case full, long, short
        var title: String {
            switch self {
            case .full: "Full Size"
            case .long: "Long Edge"
            case .short: "Short Edge"
            }
        }
    }
    var edge: Edge
    var pixels: Int

    static let full = ExportSize(edge: .full, pixels: 2048)
    static let pixelRange = 16...65_535

    /// The `fit` of a save command; nil at full size.
    var json: JSONValue? {
        switch edge {
        case .full: nil
        case .long: .object(["long_edge": .number(Double(pixels))])
        case .short: .object(["short_edge": .number(Double(pixels))])
        }
    }
    /// The exporter's arguments.
    var arguments: [String] {
        switch edge {
        case .full: []
        case .long: ["--long-edge", "\(pixels)"]
        case .short: ["--short-edge", "\(pixels)"]
        }
    }
    /// For the save panel's message; nil at full size.
    var summary: String? {
        edge == .full ? nil : "Fits \(pixels) px on the \(edge == .long ? "long" : "short") edge"
    }

    /// The size of the last save or export, offered first next time.
    static var last: ExportSize {
        get {
            let defaults = UserDefaults.standard
            let edge = defaults.string(forKey: "exportEdge").flatMap(Edge.init) ?? .full
            let pixels = defaults.integer(forKey: "exportEdgePixels")
            return ExportSize(edge: edge, pixels: pixelRange.contains(pixels) ? pixels : full.pixels)
        }
        set {
            UserDefaults.standard.set(newValue.edge.rawValue, forKey: "exportEdge")
            UserDefaults.standard.set(newValue.pixels, forKey: "exportEdgePixels")
        }
    }
}

/// The save panel's Format and Size controls, and for exports the Engine
/// pop-up. Changing the format restricts the panel to that type, which also
/// rewrites the extension in the name field. Changing the engine or the size
/// updates the message.
@MainActor private final class FormatPicker: NSObject, NSTextFieldDelegate {
    private let popup = NSPopUpButton()
    private let enginePopup: NSPopUpButton?
    private let edgePopup = NSPopUpButton()
    private let pixelsField = NSTextField()
    private let message: (ExportEngine?, ExportSize) -> String
    private weak var panel: NSSavePanel?
    var format: OutputFormat { OutputFormat.allCases[max(popup.indexOfSelectedItem, 0)] }
    /// nil for Save Preview.
    var engine: ExportEngine? { enginePopup.map { ExportEngine.allCases[max($0.indexOfSelectedItem, 0)] } }
    var size: ExportSize {
        let pixels = Int(pixelsField.stringValue.filter(\.isNumber)) ?? ExportSize.full.pixels
        return ExportSize(edge: ExportSize.Edge.allCases[max(edgePopup.indexOfSelectedItem, 0)],
                          pixels: min(max(pixels, ExportSize.pixelRange.lowerBound), ExportSize.pixelRange.upperBound))
    }

    init(panel: NSSavePanel, selected: OutputFormat, engine: ExportEngine?, size: ExportSize, message: @escaping (ExportEngine?, ExportSize) -> String) {
        self.panel = panel
        self.message = message
        enginePopup = engine.map { _ in NSPopUpButton() }
        super.init()
        popup.addItems(withTitles: OutputFormat.allCases.map(\.title))
        popup.selectItem(at: OutputFormat.allCases.firstIndex(of: selected) ?? 0)
        popup.target = self; popup.action = #selector(changed)
        var rows = [NSStackView(views: [NSTextField(labelWithString: "Format:"), popup])]
        if let enginePopup, let engine {
            enginePopup.addItems(withTitles: ExportEngine.allCases.map(\.title))
            enginePopup.selectItem(at: ExportEngine.allCases.firstIndex(of: engine) ?? 0)
            enginePopup.target = self; enginePopup.action = #selector(engineChanged)
            enginePopup.toolTip = "Reference develops the photo again with extra precision. Fast uses the preview's result and looks the same."
            rows.append(NSStackView(views: [NSTextField(labelWithString: "Engine:"), enginePopup]))
        }
        edgePopup.addItems(withTitles: ExportSize.Edge.allCases.map(\.title))
        edgePopup.selectItem(at: ExportSize.Edge.allCases.firstIndex(of: size.edge) ?? 0)
        edgePopup.target = self; edgePopup.action = #selector(sizeChanged)
        edgePopup.toolTip = "Fits the finished file, border included, to this many pixels. Never enlarges."
        let formatter = NumberFormatter()
        formatter.allowsFloats = false
        formatter.minimum = ExportSize.pixelRange.lowerBound as NSNumber
        formatter.maximum = ExportSize.pixelRange.upperBound as NSNumber
        pixelsField.formatter = formatter
        pixelsField.integerValue = size.pixels
        pixelsField.alignment = .right
        pixelsField.delegate = self
        pixelsField.widthAnchor.constraint(equalToConstant: 64).isActive = true
        pixelsField.isEnabled = size.edge != .full
        rows.append(NSStackView(views: [NSTextField(labelWithString: "Size:"), edgePopup, pixelsField, NSTextField(labelWithString: "px")]))
        let stack = NSStackView(views: rows)
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 10, left: 20, bottom: 10, right: 20)
        stack.setFrameSize(stack.fittingSize)
        panel.accessoryView = stack
        panel.allowedContentTypes = [selected.type]
        panel.message = message(self.engine, self.size)
    }

    @objc private func changed() { panel?.allowedContentTypes = [format.type] }
    @objc private func engineChanged() { panel?.message = message(engine, size) }
    @objc private func sizeChanged() {
        pixelsField.isEnabled = size.edge != .full
        if pixelsField.isEnabled { panel?.makeFirstResponder(pixelsField) }
        panel?.message = message(engine, size)
    }
    func controlTextDidChange(_ notification: Notification) { panel?.message = message(engine, size) }
}
