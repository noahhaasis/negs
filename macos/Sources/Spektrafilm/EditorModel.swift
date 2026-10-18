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
    var inspectorVisible = true
    var zoomCommand = UUID()
    var fitImage = true
    var recentFiles: [URL] = []
    @ObservationIgnored weak var undoManager: UndoManager?
    @ObservationIgnored private var defaults: JSONValue = .object([:])
    @ObservationIgnored private var engine: EngineClient?
    @ObservationIgnored private var exportJob: ExportJob?
    @ObservationIgnored private var revision = 0
    @ObservationIgnored private var sourceGeneration = 0
    @ObservationIgnored private var renderID = 0
    @ObservationIgnored private var pendingRender: Task<Void, Never>?
    @ObservationIgnored private var activityTimer: Task<Void, Never>?
    @ObservationIgnored private var interactionStart: EditState?
    @ObservationIgnored private var closing = false
    @ObservationIgnored private var parameters: [String: ObservedParameter] = [:]
    @ObservationIgnored private var stripRequest: (film: String, format: FilmFormat)?

    var films: [Stock] { stocks.filter { !$0.paper } }
    var papers: [Stock] { stocks.filter(\.paper) }
    var filmGroups: [StockGroup] { StockGroup.films(films, papers: papers) }
    var paperGroups: [StockGroup] { StockGroup.papers(papers) }
    var filmName: String { films.first { $0.id == film }?.name ?? film }
    var paperName: String { papers.first { $0.id == paper }?.name ?? paper }
    var canSave: Bool { previewCurrent && image != nil && !rendering && !saving && !cropping }
    var cropping: Bool { cropSession != nil }
    /// The shown photograph's size after cropping, before rotation; zero
    /// before one opens. Borders fit this size.
    var photoSize: CGSize { crop.size(of: image != nil ? imageSize : placeholderSize) }
    /// The developed photograph as shown, uncropped, after rotation.
    private var shownImageSize: CGSize { imageRotation % 2 != 0 ? CGSize(width: imageSize.height, height: imageSize.width) : imageSize }
    /// A slider is being dragged; its edits share one undo step.
    var interacting: Bool { interactionStart != nil }

    init() {
        recentFiles = (UserDefaults.standard.stringArray(forKey: "recentFiles") ?? []).map { URL(fileURLWithPath: $0) }
    }

    func start(paths: EnginePaths? = nil) async {
        guard engine == nil else { return }
        do {
            let engine = try EngineClient(paths: paths ?? EnginePaths.locate())
            self.engine = engine
            let (_, catalog) = try await engine.request(["command": .string("catalog")])
            guard catalog["protocol"].double == 1 else { throw EngineError(message: "The image engine version doesn't match this app. Rebuild the app bundle.") }
            let data = engine.paths.data
            Task.detached(priority: .utility) { [weak self] in
                let texture = BorderStyle.loadFilmTexture(data: data)
                await self?.setFilmTexture(texture)
            }
            stocks = catalog["profiles"].array.map(Stock.init)
            defaults = catalog["defaults"]
            // Keep anything already applied to the photograph, such as its rotation.
            let rotation = state.params["io.rotate_cw"]
            state.params = defaults
            if rotation != .null { state.params["io.rotate_cw"] = rotation }
            if !films.contains(where: { $0.id == state.film }), let first = films.first { state.film = first.id }
            if !papers.contains(where: { $0.id == state.paper }), let first = papers.first { state.paper = first.id }
            pairFilm(in: &state)
            ready = true
            if source == nil { status = "" }
            if source != nil { changed() }
        } catch { report(error) }
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
        if crop != state.crop { crop = state.crop }
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
    func endInteraction() {
        guard let start = interactionStart else { return }
        interactionStart = nil
        if start != state { registerUndo(restoring: start, action: "Change Adjustment") }
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
    /// A border change only redraws the canvas; anything else re-renders.
    private func replaceState(_ next: EditState) {
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

    // MARK: Cropping

    /// Shows the whole developed photograph, without the border, to crop it.
    func beginCrop() {
        guard !cropping, image != nil, imageSize != .zero else { return }
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
        guard url.isFileURL else { return }
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            recentFiles.removeAll { $0 == url }; saveRecents()
            error = "“\(url.lastPathComponent)” can’t be opened. It may have been moved or deleted."
            return
        }
        source = url; image = nil; placeholder = nil; previewCurrent = false; renderFailed = false
        interactionStart = nil; cropSession = nil
        sourceGeneration += 1
        let generation = sourceGeneration
        state.params["io.rotate_cw"] = .number(0)
        state.crop = CropSettings()
        imageRotation = 0
        isRaw = Self.rawExtensions.contains(url.pathExtension.lowercased())
        undoManager?.removeAllActions()
        zoom(fit: true)
        recentFiles.removeAll { $0 == url }; recentFiles.insert(url, at: 0)
        recentFiles = Array(recentFiles.prefix(10)); saveRecents()
        NSDocumentController.shared.noteNewRecentDocumentURL(url)
        status = ready ? "" : "Starting…"
        // Show the photograph at once; the developed render replaces it.
        Task.detached(priority: .userInitiated) { [weak self] in
            guard let (thumbnail, size) = Self.thumbnail(url) else { return }
            await self?.showPlaceholder(thumbnail, size: size, generation: generation)
        }
        changed()
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

    // MARK: Rendering

    private func changed() {
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
        rendering = true
        let buffer = engine.directory.appendingPathComponent("preview.rgba")
        defer {
            rendering = false
            if revision != currentRevision, !closing { scheduleRender() }
        }
        do {
            let (id, result) = try await engine.request([
                "command": .string("render"), "input": .string(source.path),
                "source_generation": .number(Double(generation)),
                "film": .string(snapshot.film), "paper": .string(snapshot.paper), "params": snapshot.params,
                "limit_preview": .bool(snapshot.limitPreview), "preview_path": .string(buffer.path)
            ])
            guard shouldShow(currentRevision, generation) else { return }
            let width = Int(result["width"].double), height = Int(result["height"].double)
            let developed = try await Task.detached(priority: .userInitiated) {
                try Self.readPreview(buffer, width: width, height: height)
            }.value
            guard shouldShow(currentRevision, generation) else { return }
            imageSize = CGSize(width: result["output_width"].double, height: result["output_height"].double)
            imageRotation = Int(snapshot.params["io.rotate_cw"].double)
            image = developed; placeholder = nil
            cropSession?.update(photo: shownImageSize, rotation: imageRotation)
            updateFilmStrip()
            isRaw = result["raw"].bool
            if revision == currentRevision {
                renderID = id; previewCurrent = true; hideActivity()
            }
            let seconds = result["elapsed_ms"].double / 1000
            status = "\(Int(imageSize.width)) × \(Int(imageSize.height)) · developed in \(seconds.formatted(.number.precision(.fractionLength(seconds < 10 ? 2 : 1)))) s"
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

    /// Only mention an update once it has visibly taken a while.
    private func scheduleActivity() {
        guard activityTimer == nil, !showsActivity else { return }
        activityTimer = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            guard let self, !Task.isCancelled else { return }
            self.activityTimer = nil
            if !self.previewCurrent, !self.interacting, !self.renderFailed, self.source != nil {
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
        let invalid = EngineError(message: "The image engine returned an invalid preview buffer.")
        guard width > 0, height > 0, width <= 8192, height <= 8192 else { throw EngineError(message: "Invalid preview dimensions") }
        var bytes = try Data(contentsOf: url)
        guard bytes.count == width * height * 4,
              let surface = IOSurface(properties: [
                  .width: width, .height: height, .bytesPerElement: 4,
                  .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
                  .pixelFormat: kCVPixelFormatType_32BGRA
              ]),
              let sRGB = CGColorSpace(name: CGColorSpace.sRGB)?.copyPropertyList()
        else { throw invalid }
        surface.lock(options: [], seed: nil)
        defer { surface.unlock(options: [], seed: nil) }
        let permuted = bytes.withUnsafeMutableBytes { raw -> vImage_Error in
            var source = vImage_Buffer(data: raw.baseAddress, height: vImagePixelCount(height), width: vImagePixelCount(width), rowBytes: width * 4)
            var destination = vImage_Buffer(data: surface.baseAddress, height: vImagePixelCount(height), width: vImagePixelCount(width), rowBytes: surface.bytesPerRow)
            // RGBA → BGRA, and opaque regardless of the engine's alpha byte.
            let error = vImagePermuteChannels_ARGB8888(&source, &destination, [2, 1, 0, 3], vImage_Flags(kvImageNoFlags))
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
        let picker = FormatPicker(panel: panel, selected: OutputFormat.last)
        panel.nameFieldStringValue = "\(source.deletingPathExtension().lastPathComponent)_\(state.film).\(picker.format.type.preferredFilenameExtension ?? "png")"
        panel.message = export ? "Render at reference precision using the current working resolution." : "Save the current preview at its rendered resolution."
        if state.crop.cropped { panel.message += " Crops the photograph." }
        if let border = state.border.exported(for: photoSize) { panel.message += " Adds the \(border.title.lowercased()) border." }
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            guard !Self.sameFile(url, source) else {
                self.error = "Choose a different filename to preserve the original photograph."; return
            }
            OutputFormat.last = picker.format
            Task { if export { await self.export(to: url) } else { await self.savePreview(to: url) } }
        }
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

    private func savePreview(to url: URL) async {
        guard canSave, let engine else { return }
        saving = true
        defer { saving = false }
        do {
            var command: [String: JSONValue] = ["command": .string("save"), "render_id": .number(Double(renderID)), "output": .string(url.path)]
            if state.crop.cropped {
                let r = state.crop.rect
                command["crop"] = .object(["x": .number(r.minX), "y": .number(r.minY), "width": .number(r.width), "height": .number(r.height), "angle": .number(state.crop.angle)])
            }
            if let border = state.border.exported(for: photoSize) { command["border"] = .string(border.rawValue) }
            _ = try await engine.request(command)
            status = "Saved \(url.lastPathComponent)"
        } catch { report(error) }
    }

    private func export(to url: URL) async {
        guard !exporting, let source, let engine else { return }
        let job = ExportJob(); exportJob = job; exporting = true; exportStarted = Date()
        defer { exporting = false; exportJob = nil }
        do {
            try await job.run(paths: engine.paths, scratch: engine.scratch, source: source, destination: url, state: state,
                              crop: state.crop.cropped ? state.crop : nil, border: state.border.exported(for: photoSize))
            status = "Exported \(url.lastPathComponent)"
        } catch is CancellationError { status = "Export cancelled" }
        catch { report(error) }
    }
    func cancelExport() { exportJob?.cancel() }
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

/// The save panel's Format pop-up. Changing the format restricts the panel to
/// that type, which also rewrites the extension in the name field.
@MainActor private final class FormatPicker: NSObject {
    private let popup = NSPopUpButton()
    private weak var panel: NSSavePanel?
    var format: OutputFormat { OutputFormat.allCases[max(popup.indexOfSelectedItem, 0)] }

    init(panel: NSSavePanel, selected: OutputFormat) {
        self.panel = panel
        super.init()
        popup.addItems(withTitles: OutputFormat.allCases.map(\.title))
        popup.selectItem(at: OutputFormat.allCases.firstIndex(of: selected) ?? 0)
        popup.target = self; popup.action = #selector(changed)
        let row = NSStackView(views: [NSTextField(labelWithString: "Format:"), popup])
        row.edgeInsets = NSEdgeInsets(top: 10, left: 20, bottom: 10, right: 20)
        row.setFrameSize(row.fittingSize)
        panel.accessoryView = row
        panel.allowedContentTypes = [selected.type]
    }

    @objc private func changed() { panel?.allowedContentTypes = [format.type] }
}
