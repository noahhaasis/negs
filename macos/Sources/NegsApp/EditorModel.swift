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
    @ObservationIgnored private var recentFrames: [RecentFrame] = []
    @ObservationIgnored private var editedSinceSave = false
    @ObservationIgnored private var autosave: Task<Void, Never>?
    private static let autosaveDelay = 0.8
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
    /// Shown until the developed frame: another frame, or the embedded preview.
    private(set) var placeholder: AnyObject?
    /// The undeveloped photograph, shown in place of `image` while the
    /// Original button is held.
    private(set) var original: IOSurface?
    /// The tones of `image` within the crop; nil until it develops.
    private(set) var histogram: Histogram?
    /// The photograph's pins, in parameter order.
    var pins: [TonePin] {
        let anchors = state.params["post.tone_anchors"].array
        return anchors.indices.compactMap { index in
            let path = "post.tone_anchors.\(index)"
            let input = state.params["\(path).input"].double
            let target = state.params["\(path).target"].double
            guard input.isFinite, target.isFinite else { return nil }
            let radius = state.params["\(path).source.radius"].double
            let centre = CGPoint(
                x: state.params["\(path).source.x"].double,
                y: state.params["\(path).source.y"].double
            )
            let source = radius.isFinite && radius >= 0 && centre.x.isFinite && centre.y.isFinite
                && state.params["\(path).source.x"] != .null
                ? PinSource(centre: centre, radius: radius)
                : nil
            return TonePin(input: input, target: target, zone: ZoneScale.zone(forTarget: target), source: source)
        }
    }

    /// The pins the canvas draws, with a resize in progress already applied.
    var pinsToShow: [TonePinMarker] {
        guard pinsVisible, !imageOnly, original == nil, !holdingOriginal else { return [] }
        return pins.enumerated().compactMap { index, pin in
            guard var source = pin.source else { return nil }
            if let pendingResize, pendingResize.index == index { source.radius = pendingResize.radius }
            return TonePinMarker(
                index: index, zone: pin.zone, source: source,
                selected: selectedPin == index, ignored: ignoredPins.contains(index)
            )
        }
    }

    /// Whether the pins and their lines are drawn; hidden, they still shape the
    /// photograph. Remembered between launches.
    var pinsVisible = UserDefaults.standard.object(forKey: "showTonePins") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(pinsVisible, forKey: "showTonePins")
            if !pinsVisible { selectedPin = nil }
        }
    }

    /// The panel shows with a photograph, except while cropping.
    var showsZonePanel: Bool { source != nil && !cropping && !gridVisible && !imageOnly }

    /// Where the zone panel sits against the photograph.
    var zonePlacement: ZonePlacement?

    /// F: nothing in the window but the photograph and its camera details. The
    /// grid and cropping bring their own controls back while they're up.
    var imageOnly: Bool { imageOnlyChosen && source != nil && !cropping && !gridVisible }
    private var imageOnlyChosen = false

    func toggleImageOnly() {
        guard source != nil, !cropping, !gridVisible else { return }
        imageOnlyChosen.toggle()
    }

    /// The window is full screen, where the status bar carries its title.
    var windowFullScreen = false
    /// What the title bar says, for the status bar in full screen.
    var fullScreenCaption: String? {
        guard windowFullScreen, let source else { return nil }
        let stock = value("io.scan_film").bool ? filmName : "\(filmName) → \(paperName)"
        return ready ? "\(source.lastPathComponent)   \(stock)" : source.lastPathComponent
    }

    /// The grid is up in place of the photograph. G, or a double-click to leave.
    private(set) var gridVisible = false
    /// Develops the folder's final edits while the grid is up.
    private(set) var rollDeveloper: RollDeveloper?

    func toggleGrid() {
        guard source != nil, !cropping else { return }
        gridVisible ? closeGrid() : openGrid()
    }

    private func openGrid() {
        startRollDeveloper()
        // Edits made in the edit view since, which it wasn't watching.
        if let source {
            rollDeveloper?.invalidate(source)
        }
        gridVisible = true
    }

    private func startRollDeveloper() {
        guard rollDeveloper == nil, let paths = engine?.paths else { return }
        rollDeveloper = try? RollDeveloper(paths: paths) { [weak self] url in self?.frameRequest(for: url) }
        // The open photograph comes first: its own develop, an edit in progress, an export.
        rollDeveloper?.waits = { [weak self] in
            guard let self, !self.gridVisible, !self.renderFailed else { return false }
            return self.interacting || self.exporting || self.rendering || !self.previewCurrent
        }
    }

    /// Develops the rest of the folder behind the scenes, out from `url`, so the
    /// grid and the next photograph have their frames waiting.
    private func developFolder(around url: URL) {
        guard hasCatalog else { return }
        startRollDeveloper()
        let directory = url.deletingLastPathComponent()
        let rawOnly = filmstripRawOnly
        folderListing?.cancel()
        folderListing = Task { [weak self] in
            let files = await Task.detached(priority: .utility) { try? PhotoFolder.snapshot(in: directory) }.value
            guard let self, !Task.isCancelled, self.source == url, let files else { return }
            let shown = PhotoFolder.shown(files, rawOnly: rawOnly)
            self.rollDeveloper?.show(files: shown)
            self.rollDeveloper?.setCentre(shown.firstIndex { $0.url == url } ?? 0)
        }
    }
    @ObservationIgnored private var folderListing: Task<Void, Never>?

    /// The developer stays, with its frames, carrying on through the folder, so
    /// coming back doesn't start over. Only another folder ends it.
    private func closeGrid() {
        gridVisible = false
    }

    /// Back to the edit view, on `url`.
    func leaveGrid(opening url: URL) {
        closeGrid()
        if url != source { open(url) }
    }

    /// The helper request for a photograph as it would open: the open one as it
    /// is now, or its saved edit, or what an unedited one starts with.
    func frameRequest(for url: URL) -> (command: [String: JSONValue], turns: Int)? {
        guard hasCatalog else { return nil }
        var edit: EditState
        if url == source {
            edit = state
        } else if let saved = try? PhotoSettingsFile.read(beside: url),
                  films.contains(where: { $0.id == saved.state.film }), papers.contains(where: { $0.id == saved.state.paper }) {
            edit = merged(saved.state)
        } else {
            edit = uneditedState(in: url.deletingLastPathComponent())
        }
        edit.params["io.output_color_space"] = .string(previewSpace)
        // The grid's frames have their own size, so a new Image Size is no new frame.
        edit.params[ImageSize.targetPath] = defaults[ImageSize.targetPath]
        var command: [String: JSONValue] = [
            "command": .string("thumbnail"), "input": .string(url.path),
            "film": .string(edit.film), "paper": .string(edit.paper), "params": edit.params,
            "long_side": .number(Double(Self.rollFrameSide)),
            "decode_long_side": .number(Double(Self.rollReadSide))
        ]
        if let crop = Self.cropJSON(edit.crop) { command["crop"] = crop }
        return (command, Int(edit.params["io.rotate_cw"].double))
    }
    private static let rollFrameSide = 800
    /// A small frame doesn't need a RAW's whole sensor, and reading less is what
    /// makes it quicker: the film simulation itself is a few percent of the time.
    private static let rollReadSide = 1200

    /// A zone being dragged from the panel.
    var zoneDrag: ZoneDrag?

    /// The size the next pin's circle starts at: the last one used.
    var sampleRadius = SampleSize.initial

    /// The selected pin: its tile is outlined, and the scroll wheel sizes it.
    private(set) var selectedPin: Int?
    /// The pins the tone curve can't honour, from the last render.
    private(set) var ignoredPins: Set<Int> = []
    /// A resize waiting for the wheel to stop, shown before it's an edit.
    private var pendingResize: (index: Int, radius: Double)?
    @ObservationIgnored private var resizeCommit: Task<Void, Never>?
    /// The developed preview's frame, uncropped, which the canvas lays out:
    /// the working frame, or less with the 12 MP limit.
    private(set) var imageSize = CGSize.zero
    /// The decoded source and the uncapped frame an export develops, from the
    /// last render; zero before one finishes.
    private(set) var sourceSize = CGSize.zero
    private(set) var workingSize = CGSize.zero
    /// The settled preview's linear fraction of the working frame, below 1
    /// when limited or larger than the display proxy. Temporary drag frames
    /// leave this unchanged so the Image Size notice doesn't resize the form.
    private(set) var previewScale = 1.0
    private(set) var placeholderSize = CGSize.zero
    private(set) var imageRotation = 0
    private(set) var isRaw = false
    /// The colour space the preview develops in: the one Save and Export
    /// last used, so editing shows the export's gamut and Fast export and
    /// Save Preview can keep the preview's own values.
    private(set) var previewSpace = SaveSettings.colorSpaces[SaveSettings.chosenSpace]
    /// How the engine read the photograph's colours: the last render's
    /// `input_colour` (summary, automatic reading, warning).
    private(set) var inputColour: JSONValue = .null
    /// Whether the last render's RAW decoder can apply the camera's lens
    /// corrections to this photograph (`io.lens_correction`).
    private(set) var lensCorrectionSupported = false
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
    // MARK: Export settings
    //
    // The Export tab's choices, remembered between launches. The save panel
    // is now only a file chooser; these used to live in its accessory view.
    var exportFormat = OutputFormat.last {
        didSet {
            OutputFormat.last = exportFormat
            // Each lossless format keeps its own depth; JPEG is always 8-bit.
            exportBitDepth = SaveSettings.bitDepth(for: exportFormat)
        }
    }
    var exportBitDepth = SaveSettings.bitDepth(for: OutputFormat.last) {
        didSet { SaveSettings.setBitDepth(exportBitDepth, for: exportFormat) }
    }
    var exportEngine = ExportEngine.selected {
        didSet { ExportEngine.selected = exportEngine }
    }
    /// Settings › Export: the resolution written into saved files.
    var exportDPI = SaveSettings.lastDPI {
        didSet { SaveSettings.lastDPI = exportDPI }
    }
    var exportSize = ExportSize.last {
        didSet { ExportSize.last = exportSize }
    }
    var exportQuality = SaveSettings.lastQuality {
        didSet {
            let clamped = min(max(exportQuality, SaveSettings.qualityRange.lowerBound), SaveSettings.qualityRange.upperBound)
            if clamped != exportQuality { exportQuality = clamped } else { SaveSettings.lastQuality = clamped }
        }
    }
    /// The file settings a save or export uses now. ProPhoto RGB at 8 bits
    /// falls back to Adobe RGB, as its picker says.
    var exportSaveSettings: SaveSettings {
        let depth = exportFormat == .jpeg ? 8 : exportBitDepth
        let chosen = SaveSettings.colorSpaces.firstIndex(of: previewSpace) ?? SaveSettings.chosenSpace
        return SaveSettings(bitDepth: depth, colorSpace: SaveSettings.effectiveSpace(chosen, bitDepth: depth),
                            jpegQuality: exportQuality, dpi: exportDPI)
    }
    /// Settings › Film Strip: only the folder's RAW files are on the roll.
    var filmstripRawOnly = UserDefaults.standard.bool(forKey: "filmstripRawOnly") {
        didSet {
            UserDefaults.standard.set(filmstripRawOnly, forKey: "filmstripRawOnly")
            if let source, rollDeveloper != nil { developFolder(around: source) }
        }
    }
    /// The film strip's height, set by dragging its top edge.
    var filmstripHeight = EditorModel.savedFilmstripHeight {
        didSet {
            let range = FilmRollLayout.stripHeightRange
            let clamped = min(max(filmstripHeight, range.lowerBound), range.upperBound)
            if clamped != filmstripHeight { filmstripHeight = clamped }
            UserDefaults.standard.set(Double(filmstripHeight), forKey: "filmstripHeight")
        }
    }
    private static var savedFilmstripHeight: CGFloat {
        let saved = UserDefaults.standard.double(forKey: "filmstripHeight")
        return FilmRollLayout.stripHeightRange.contains(saved) ? saved : FilmRollLayout.defaultStripHeight
    }
    var filmstripVisible = UserDefaults.standard.object(forKey: "filmstripVisible") as? Bool ?? true {
        didSet { UserDefaults.standard.set(filmstripVisible, forKey: "filmstripVisible") }
    }
    var inspectorVisible = true
    /// The inspector tab chosen in this session. Like the pane, it survives
    /// changing photos; unlike it, it isn't carried into the next launch.
    var inspectorTab = InspectorTab.camera
    /// Settings › Inspector: every control on its tab, or the everyday ones on one page.
    func toggleInspectorMode() { inspectorMode = inspectorMode == .simple ? .advanced : .simple }
    var inspectorMode = InspectorMode(rawValue: UserDefaults.standard.string(forKey: "inspectorMode") ?? "") ?? .advanced {
        didSet { UserDefaults.standard.set(inspectorMode.rawValue, forKey: "inspectorMode") }
    }
    /// Film format › Custom picked while the value is still a format's, so the
    /// slider stays. Reset Adjustments and a fresh photograph clear it, as they
    /// do the edits.
    var customFilmFormat = false
    /// Settings › Presets › Save Finish parameters in presets. Stored so the
    /// helper's preset files can carry the post-scan tone controls.
    static let saveFinishInPresetsKey = "saveFinishInPresets"
    var saveFinishInPresets = UserDefaults.standard.bool(forKey: "saveFinishInPresets") {
        didSet {
            guard saveFinishInPresets != oldValue else { return }
            UserDefaults.standard.set(saveFinishInPresets, forKey: Self.saveFinishInPresetsKey)
            pumpPreviews()
        }
    }
    /// Which panel the inspector column shows, remembered across launches.
    var inspectorPane = InspectorPane(rawValue: UserDefaults.standard.string(forKey: "inspectorPane") ?? "") ?? .adjustments {
        didSet { UserDefaults.standard.set(inspectorPane.rawValue, forKey: "inspectorPane") }
    }
    private(set) var presets: [PresetFile] = []
    /// What the engine made of each listed file, by the file's identity.
    private(set) var presetStatus: [PresetFile: PresetStatus] = [:]
    /// Each readable preset file's text, for its thumbnail.
    @ObservationIgnored private var presetTexts: [PresetFile: String] = [:]
    /// Each valid preset's RAW white balance, to make thumbnails that need
    /// their own RAW decode last.
    @ObservationIgnored private var presetWhiteBalances: [PresetFile: JSONValue] = [:]
    /// While the engine checks preset files: their thumbnails wait, so the
    /// order (quick ones first) covers every preset.
    @ObservationIgnored private var checkingPresets = 0
    /// The preset whose name is being edited in the list.
    var renamingPreset: URL?
    /// A preset New Preset just made: Escape while naming it removes it.
    @ObservationIgnored private var unnamedPreset: URL?
    var zoomCommand = UUID()
    /// True fits the whole photograph in the viewer, but never past 1:1;
    /// false shows it at 1:1 even when that crops it. The viewer starts fitted
    /// and keeps whatever the user chooses.
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
    @ObservationIgnored private(set) var showingCachedPreview = false
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
    /// The session restored at launch and what restoring it replaced, until
    /// the helper's catalog confirms it. `discardLaunchRestore()` goes back.
    @ObservationIgnored private var launchRestore: (session: EditorSession, before: LaunchRestoreBaseline)?
    private struct LaunchRestoreBaseline {
        var state: EditState, inspectorVisible: Bool, fitImage: Bool, status: String, recentFiles: [URL]
    }
    /// True until `finishLaunching`: Finder's open arrives after the restore
    /// at launch, and the photograph it opens must not take the previous
    /// session's edits, as before restoring early.
    @ObservationIgnored private var launching = true
    @ObservationIgnored private var restoringAtLaunch = false
    /// The saved frame being read for the session restored at launch.
    @ObservationIgnored private(set) var launchFrame: (frame: LaunchFrame, generation: Int, rendered: EditState)?
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
    /// Whether a zone can be dropped: a drop during a render waits for it, but
    /// the original or the crop must not be what's on screen.
    var canPlaceZones: Bool { image != nil && !cropping && !holdingOriginal && original == nil }
    /// The shown photograph's size after cropping, before rotation; zero
    /// before one opens. Borders fit this size.
    var photoSize: CGSize { crop.size(of: image != nil ? imageSize : placeholderSize) }
    var imageSizeMode: ImageSize.Mode { ImageSize.mode(of: state.params) }
    /// The frame the current edit develops at, uncapped, projected from the
    /// source like the helper resolves it; zero until the source's size is known.
    var projectedWorkingSize: CGSize {
        sourceSize == .zero ? .zero : ImageSize.working(source: sourceSize, params: state.params, crop: crop)
    }
    /// The exported photograph, cropped, before rotation, borders and export
    /// reductions; zero until the source's size is known.
    var photographSize: CGSize { crop.size(of: projectedWorkingSize) }
    /// The current edit asks for more working pixels than the helper develops.
    var imageSizeError: String? {
        let working = projectedWorkingSize
        return ImageSize.exceedsLimit(working: working, source: sourceSize) ? ImageSize.limitMessage(working: working) : nil
    }
    /// The developed photograph as shown, uncropped, after rotation.
    private var shownImageSize: CGSize { imageRotation % 2 != 0 ? CGSize(width: imageSize.height, height: imageSize.width) : imageSize }
    /// A slider is being dragged; its edits share one undo step.
    var interacting: Bool { interactionStart != nil }

    @ObservationIgnored let presetLibrary: PresetLibrary
    /// Film and paper preview grids (`Previews.swift`).
    @ObservationIgnored let previews = Previews()

    init(previewCache: RenderedPreviewCache = RenderedPreviewCache(), presetLibrary: PresetLibrary = PresetLibrary()) {
        self.previewCache = previewCache
        self.presetLibrary = presetLibrary
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

    /// Restores the previous session with the last launch's catalog, before
    /// the helper answers. False without a cached catalog or if the session
    /// doesn't fit it; `start` then restores it as before.
    func restoreAtLaunch(_ session: EditorSession) -> Bool {
        guard hasCatalog, launching, !ready, !explicitOpenRequested, !closing, source == nil else { return false }
        let before = LaunchRestoreBaseline(state: state, inspectorVisible: inspectorVisible, fitImage: fitImage, status: status, recentFiles: recentFiles)
        restoringAtLaunch = true
        defer { restoringAtLaunch = false }
        guard (try? restore(session)) != nil else { return false }
        launchRestore = (session, before)
        return true
    }

    /// Goes back to no photograph and the defaults, as before the restore at
    /// launch, so the caller continues as if it had never happened.
    private func discardLaunchRestore() {
        guard let (_, before) = launchRestore else { return }
        launchRestore = nil; launchFrame = nil
        sourceGeneration += 1
        source = nil; image = nil; placeholder = nil; previewCurrent = false; renderFailed = false
        imageSize = .zero; placeholderSize = .zero; sourceSize = .zero; workingSize = .zero; previewScale = 1
        showingCachedPreview = false; isRaw = false; inputColour = .null; lensCorrectionSupported = false
        // A different catalog can arrive once the window is shown and the
        // restored photograph is being cropped, compared or edited.
        interactionStart = nil; cropSession = nil
        original = nil; originalCache = nil; originalLoad?.cancel(); originalLoad = nil; holdingOriginal = false
        undoManager?.removeAllActions()
        state = before.state; inspectorVisible = before.inspectorVisible; fitImage = before.fitImage; status = before.status
        if recentFiles != before.recentFiles { recentFiles = before.recentFiles; saveRecents() }
        updateHistogram()
    }

    func start(paths: EnginePaths? = nil, restoration: Task<EditorSession?, Error>? = nil) async {
        launch(paths: paths)
        guard let request = catalogRequest, let engine else { return }
        EdgePrint.registerFont(data: engine.paths.data)
        catalogRequest = nil
        var restoration = restoration
        do {
            let (_, catalog) = try await request.value
            guard !closing else { return }
            guard catalog["protocol"].double == 1 else { throw EngineError(message: "The image engine version doesn't match this app. Rebuild the app bundle.") }
            // An unchanged catalog keeps edits made while the helper started.
            if catalog != self.catalog {
                // Applying a catalog resets the parameters, so a session
                // restored with the cached one is undone and restored under
                // this catalog, as without a cached one.
                if let (session, _) = launchRestore, source == session.source {
                    discardLaunchRestore()
                    restoration = Task { session }
                }
                apply(catalog: catalog)
                CatalogCache.save(catalog, for: engine.paths)
            }
            // The restore at launch stands: its open skipped the system's
            // recent documents while it could still be undone.
            if let (session, _) = launchRestore, source == session.source {
                NSDocumentController.shared.noteNewRecentDocumentURL(session.source)
            }
            launchRestore = nil
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
            // A photograph that arrived before the catalog, or from the last session.
            if let source { developFolder(around: source) }
            if source == nil, status == "Starting…" { status = "" }
            if source != nil { changed(invalidateCache: false) }
        } catch { if !closing { report(error) } }
    }

    private func apply(catalog: JSONValue) {
        self.catalog = catalog
        stocks = catalog["profiles"].array.map(Stock.init)
        defaults = catalog["defaults"]
        // The engine leaves lens correction off for params from before it, so
        // their crops keep their place; the app corrects every new edit.
        defaults[Self.lensCorrectionPath] = .bool(true)
        // Keep anything already applied to the photograph, such as its rotation.
        let rotation = state.params["io.rotate_cw"]
        var next = state
        next.params = defaults
        if rotation != .null { next.params["io.rotate_cw"] = rotation }
        if let size = Self.savedImageSize { next.params[ImageSize.targetPath] = size }
        if !films.contains(where: { $0.id == next.film }), let first = films.first { next.film = first.id }
        if !papers.contains(where: { $0.id == next.paper }), let first = papers.first { next.paper = first.id }
        PreviewRules.pairFilm(in: &next, stocks: stocks)
        state = next
        previews.update(stocks: stocks)
        // A photograph opened before the catalog arrived, as from the command line.
        if let source, launchRestore == nil { _ = loadSettings(beside: source) }
    }

    private func setFilmTexture(_ texture: CGImage?) { filmTexture = texture }

    /// The engine's data folder, once it has started.
    var dataDirectory: URL? { engine?.paths.data }

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
        if crop != state.crop {
            crop = state.crop
            selectedPin = nil
            updateHistogram()
        }
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
    /// Grain arrays (`film_render.grain.<field>`) are null until edited: the
    /// engine then uses the film's preset, which the catalog resolves.
    var filmIsBW: Bool { films.first { $0.id == film }?.bw ?? false }
    func stockDefaults(_ path: String) -> [Double] {
        let field = path.split(separator: ".").last.map(String.init) ?? path
        return films.first { $0.id == film }?.grain[field].array.map(\.double) ?? []
    }
    func stockDefault(_ path: String, _ index: Int) -> Double {
        let preset = stockDefaults(path)
        return preset.indices.contains(index) ? preset[index] : 0
    }
    private func stockValues(_ path: String) -> [Double] {
        let saved = value(path).array.map(\.double)
        return saved.isEmpty ? stockDefaults(path) : saved
    }
    func stockNumber(_ path: String, _ index: Int, range: ClosedRange<Double>, step: Double) -> Binding<Double> {
        Binding(get: {
            let values = self.stockValues(path)
            return values.indices.contains(index) ? values[index] : 0
        }, set: { self.setStock(path, index, Self.snap($0, to: range, step: step)) })
    }
    func isStockModified(_ path: String, _ index: Int) -> Bool {
        let values = stockValues(path)
        guard values.indices.contains(index) else { return false }
        let preset = stockDefault(path, index)
        return abs(values[index] - preset) > 1e-6 * max(1, abs(preset))
    }
    func resetStock(_ path: String, _ index: Int) { setStock(path, index, stockDefault(path, index)) }
    /// Back at the preset in every lane, the array returns to null so a
    /// later film change applies that film's preset.
    private func setStock(_ path: String, _ index: Int, _ newValue: Double) {
        var values = stockValues(path)
        guard values.indices.contains(index) else { return }
        values[index] = newValue
        let preset = stockDefaults(path)
        let atPreset = values.count == preset.count
            && zip(values, preset).allSatisfy { abs($0 - $1) <= 1e-6 * max(1, abs($1)) }
        set(path, atPreset ? .null : .array(values.map(JSONValue.number)))
    }
    /// The input colour pop-up: "auto", "manual", or "assume:" and the
    /// colour space (`io.input_interpretation`).
    var inputInterpretation: Binding<String> {
        Binding(get: {
            let mode = self.value("io.input_interpretation.mode").string
            return mode == "assume" ? "assume:" + self.value("io.input_interpretation.color_space").string : mode.isEmpty ? "auto" : mode
        }, set: { tag in
            var next = self.state
            if tag.hasPrefix("assume:") {
                next.params["io.input_interpretation.mode"] = .string("assume")
                next.params["io.input_interpretation.color_space"] = .string(String(tag.dropFirst("assume:".count)))
            } else {
                next.params["io.input_interpretation.mode"] = .string(tag)
            }
            self.apply(next, action: "Change Input Colour")
        })
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
        pumpPreviews()
    }

    func chooseFilm(_ stock: String) {
        apply(PreviewRules.choosing(stock, paper: false, in: state, stocks: stocks), action: "Change Film")
    }
    func choosePaper(_ stock: String) {
        apply(PreviewRules.choosing(stock, paper: true, in: state, stocks: stocks), action: "Change Paper")
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
    /// Keeps the border, crop, and Image Size's Longest side, like the film,
    /// paper, and rotation.
    func reset() {
        guard !cropping else { return }
        var next = state; next.params = defaults; next.limitPreview = false
        next.params["io.rotate_cw"] = state.params["io.rotate_cw"]
        next.params[ImageSize.targetPath] = state.params[ImageSize.targetPath]
        next.params["io.scan_film"] = .bool(films.first(where: { $0.id == next.film })?.positive ?? false)
        customFilmFormat = false
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
        let previousAnchors = state.params["post.tone_anchors"]
        let render = next.rendered != state.rendered
        let size = next.params[ImageSize.targetPath]
        if size != state.params[ImageSize.targetPath] { Self.savedImageSize = size }
        state = next
        if next.params["post.tone_anchors"] != previousAnchors { cancelResize() }
        if let selectedPin, !pins.indices.contains(selectedPin) { self.selectedPin = nil }
        // A crop doesn't re-render the preview, but it changes the thumbnails.
        if render { changed() } else { pumpPreviews() }
    }
    private func registerUndo(restoring previous: EditState, action: String) {
        markEdited()
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

    /// The photograph's pixels in an export, which set its size on the canvas, one
    /// to a screen pixel. Nothing else zooms it, and the window stays the user's to
    /// size and place. A border isn't counted, though
    /// an export limit that fits the framed file still scales the photograph.
    var exportPixels: CGSize {
        let rotated = imageRotation % 2 != 0
        let turned = rotated ? CGSize(width: photographSize.height, height: photographSize.width) : photographSize
        let frame = border.exported(for: turned)
        return ExportPixels.photograph(photographSize, rotated: rotated, withBorder: frame.map { frame in
            { size in
                let canvas = frame.canvas(for: size, strip: self.filmStrip)
                return frame.instagram ? frame.instagramCanvas(for: canvas) : canvas
            }
        }, fit: exportSize)
    }
    // MARK: Image Size

    /// Original develops the source's own pixels; Longest side resizes the
    /// source so the cropped photograph has that many pixels on its longest side.
    func chooseImageSize(_ mode: ImageSize.Mode) {
        switch mode {
        case .original:
            guard imageSizeMode != .original else { return }
            var next = state; next.params[ImageSize.targetPath] = .null
            // A size change keeps the current zoom: fitted stays whole-image,
            // and a manual 1:1 stays 1:1.
            apply(next, action: "Change Image Size")
        case .longestSide:
            guard imageSizeMode != .longestSide else { return }
            _ = setLongestSide(ImageSize.remembered ?? nativeLongestSide)
        }
    }

    /// The crop's longest side in the source's own pixels, within the field's range.
    private var nativeLongestSide: Int {
        let size = crop.size(of: sourceSize == .zero ? placeholderSize : sourceSize)
        let side = Int(max(size.width, size.height))
        return side > 0 ? min(max(side, ImageSize.range.lowerBound), ImageSize.range.upperBound) : 4000
    }

    /// Sets a Longest side target; returns why it can't be used, changing nothing.
    @discardableResult func setLongestSide(_ pixels: Int) -> String? {
        guard ImageSize.range.contains(pixels) else {
            return "Enter a whole number from \(ImageSize.range.lowerBound) to \(ImageSize.range.upperBound.formatted()) px."
        }
        if sourceSize != .zero {
            let working = ImageSize.working(source: sourceSize, target: pixels, crop: crop)
            if ImageSize.exceedsLimit(working: working, source: sourceSize) { return ImageSize.limitMessage(working: working) }
        }
        ImageSize.remembered = pixels
        var next = state; next.params[ImageSize.targetPath] = .number(Double(pixels))
        apply(next, action: "Change Image Size")
        return nil
    }

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
        if gridVisible { closeGrid() }
        holdOriginal(false)
        cropSession = CropSession(start: state, photo: shownImageSize, rotation: imageRotation, source: sourceSize)
    }
    /// Done applies the crop, and any rotation while cropping, as one undo
    /// step. Cancel restores the rotation and keeps the previous crop.
    func endCrop(apply: Bool) {
        guard let session = cropSession else { return }
        // A crop too small for the Longest side stays a draft, with its error shown.
        if apply, session.sizeError != nil { NSSound.beep(); return }
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
        // During launch it undoes a restore at launch; later it supersedes it.
        if launching { discardLaunchRestore() } else { launchRestore = nil }
        open(url, restoring: nil)
    }

    private func open(_ url: URL, restoring session: EditorSession?) {
        guard !closing, url.isFileURL else { return }
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            recentFiles.removeAll { $0 == url }; saveRecents()
            error = "“\(url.lastPathComponent)” can’t be opened. It may have been moved or deleted."
            return
        }
        // The photograph being left keeps an edit made just now.
        saveEditsNow()
        if rollDeveloper != nil, source?.deletingLastPathComponent().standardizedFileURL != url.deletingLastPathComponent().standardizedFileURL {
            rollDeveloper?.stop()
            rollDeveloper = nil
        }
        // Its frame may have just gone out of date.
        if let source { rollDeveloper?.invalidate(source) }
        let leaving = image, leavingSize = imageSize, leavingCrop = state.crop, leavingRotation = state.params["io.rotate_cw"]
        source = url; image = nil; placeholder = nil; previewCurrent = false; renderFailed = false
        sourceSize = .zero; workingSize = .zero; previewScale = 1
        showingCachedPreview = false
        selectedPin = nil
        updateHistogram()
        interactionStart = nil; cropSession = nil
        original = nil; originalCache = nil; originalLoad?.cancel(); originalLoad = nil; holdingOriginal = false
        sourceGeneration += 1
        let generation = sourceGeneration
        developFolder(around: url)
        photoExif = nil
        Task { [weak self] in
            let exif = await Task.detached(priority: .userInitiated) { PhotoExif.read(url) }.value
            if let self, self.sourceGeneration == generation { self.photoExif = exif }
        }
        if let session {
            state = merged(session.state)
            inspectorVisible = session.inspectorVisible
        } else {
            state = uneditedState(in: url.deletingLastPathComponent())
            customFilmFormat = false
        }
        // A photograph's own settings file, unless a session restored something newer.
        let beforeSettings = session == nil ? loadSettings(beside: url) : nil
        isRaw = Self.rawExtensions.contains(url.pathExtension.lowercased())
        undoManager?.removeAllActions()
        if let beforeSettings { registerUndo(restoring: beforeSettings, action: "Load Settings") }
        // Opening it isn't an edit.
        autosave?.cancel(); autosave = nil
        editedSinceSave = false
        // Launch fits the restored photograph (never past 1:1). Changing
        // photographs keeps the fit or 1:1 setting the previous one was at.
        zoom(fit: session == nil ? fitImage : true)
        recentFiles.removeAll { $0 == url }; recentFiles.insert(url, at: 0)
        recentFiles = Array(recentFiles.prefix(10)); saveRecents()
        // A restore at launch may still be undone; `start` notes it.
        if !restoringAtLaunch { NSDocumentController.shared.noteNewRecentDocumentURL(url) }
        status = ready ? "" : "Starting…"
        // Try the exact developed frame before decoding the raw placeholder,
        // so a cache hit never flashes the unprocessed photograph.
        let snapshot = renderState, paths = engine?.paths, cache = previewCache
        inputColour = .null; lensCorrectionSupported = false
        // Read off the main actor at once, with its histogram. At launch the
        // main actor stays busy building the window, which takes the frame
        // through `finishLaunching` before it first appears.
        let crop = state.crop
        // Already in memory, it shows now and never gaps.
        let recent = session == nil ? recentFrame(url, rendered: snapshot.rendered) : nil
        if let recent { showRecent(recent) }
        // Otherwise the last photograph holds until this one's final develop is
        // ready, so no in-between image shows. A different crop or turn would
        // frame it wrongly, and with nothing to hold the previews still stand in.
        let holding = recent == nil && session == nil && leaving != nil
            && state.crop == leavingCrop && state.params["io.rotate_cw"] == leavingRotation
        if holding, let leaving { placeholder = leaving; placeholderSize = leavingSize }
        let cachedFrame = recent == nil ? paths.map { paths in
            LaunchFrame {
                guard let frame = await cache.load(source: url, state: snapshot, paths: paths) else { return nil }
                return LaunchFrame.Loaded(frame: frame, histogram: Histogram.of(frame.surface, crop: crop), histogramCrop: crop)
            }
        } : nil
        launchFrame = restoringAtLaunch ? cachedFrame.map { ($0, generation, snapshot.rendered) } : nil
        if recent == nil { Task { [weak self] in
            if let cachedFrame, let cached = await cachedFrame.value {
                guard let self else { return }
                if self.showCached(cached, generation: generation, rendered: snapshot.rendered) { return }
            }
            if holding { return }
            // The grid's smaller frame has the final look, unlike the embedded preview.
            if let self, let frame = await self.rollFrame(url, generation: generation) {
                self.showPlaceholder(frame, size: CGSize(width: frame.width, height: frame.height), generation: generation)
                return
            }
            let thumbnail = await Task.detached(priority: .userInitiated) { Self.thumbnail(url) }.value
            if let (thumbnail, size) = thumbnail { self?.showPlaceholder(thumbnail, size: size, generation: generation) }
        } }
        changed(invalidateCache: session == nil)
    }
    /// Ends launch, before the window is shown: a restore at launch is no
    /// longer undone by an open, and its saved frame is shown now if it's
    /// read within `timeout`, so the window's first frame has the
    /// photograph; otherwise it appears when read.
    func finishLaunching(waitingUpTo timeout: TimeInterval) {
        guard launching else { return }
        launching = false
        guard let (frame, generation, rendered) = launchFrame else { return }
        launchFrame = nil
        if let loaded = frame.wait(timeout: timeout) { _ = showCached(loaded, generation: generation, rendered: rendered) }
    }

    /// True if the saved frame is shown, or it no longer applies because
    /// another photograph or image is shown; false if the edits changed.
    private func showCached(_ cached: LaunchFrame.Loaded, generation: Int, rendered: EditState) -> Bool {
        guard generation == sourceGeneration, image == nil, !closing else { return true }
        guard renderState.rendered == rendered else { return false }
        let frame = cached.frame
        image = frame.surface; imageSize = frame.photoSize; isRaw = frame.raw
        sourceSize = frame.sourceSize; workingSize = frame.workingSize
        previewScale = frame.workingSize.width > 0 ? Double(frame.surface.width) / frame.workingSize.width : 1
        showingCachedPreview = true; placeholder = nil; hideActivity()
        // Counted with the frame for the crop it was read with.
        if crop == cached.histogramCrop { histogramRequest += 1; histogram = cached.histogram } else { updateHistogram() }
        updateFilmStrip()
        return true
    }

    // MARK: Frames kept in memory

    private struct RecentFrame {
        var url: URL
        var rendered: EditState
        var frame: RenderedPreviewCache.Frame
        var bytes: Int
    }
    /// About 90 frames of 2000 px, or 16 of 12 MP.
    private static let recentFramesLimit = 768 << 20

    private func recentFrame(_ url: URL, rendered: EditState) -> RecentFrame? {
        recentFrames.last { $0.url == url && $0.rendered == rendered }
    }

    private func rememberFrame(_ surface: IOSurface, url: URL, rendered: EditState) {
        recentFrames.removeAll { $0.url == url && $0.rendered == rendered }
        recentFrames.append(RecentFrame(url: url, rendered: rendered,
                                        frame: .init(surface: surface, photoSize: imageSize, sourceSize: sourceSize, workingSize: workingSize, raw: isRaw),
                                        bytes: surface.width * surface.height * 4))
        while recentFrames.count > 1, recentFrames.reduce(0, { $0 + $1.bytes }) > Self.recentFramesLimit { recentFrames.removeFirst() }
    }

    private func showRecent(_ recent: RecentFrame) {
        let frame = recent.frame
        image = frame.surface; imageSize = frame.photoSize; isRaw = frame.raw
        sourceSize = frame.sourceSize; workingSize = frame.workingSize
        previewScale = frame.workingSize.width > 0 ? Double(frame.surface.width) / frame.workingSize.width : 1
        showingCachedPreview = true; placeholder = nil; hideActivity()
        updateHistogram()
        updateFilmStrip()
    }

    /// The grid's developed frame for this edit, if it has one. It's already cut and
    /// turned, so only a frame the canvas won't transform again can stand in.
    private func rollFrame(_ url: URL, generation: Int) async -> CGImage? {
        guard generation == sourceGeneration, image == nil, !state.crop.cropped, Int(value("io.rotate_cw").double) % 4 == 0,
              let request = frameRequest(for: url), let helper = engine?.paths.helper else { return nil }
        let file = PhotoFile(url: url)
        return await Task.detached(priority: .userInitiated) {
            RollDeveloper.cachedFrame(file: file, command: request.command, helper: helper)
        }.value
    }

    private func showPlaceholder(_ thumbnail: AnyObject, size: CGSize, generation: Int) {
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
        dropStaleOriginal()
        if canShowOriginal, originalCache == nil { loadOriginal() }
    }

    static let lensCorrectionPath = "io.lens_correction.enabled"
    /// The original is framed like the developed image only with the same lens correction.
    @ObservationIgnored private var originalLensCorrection = false
    private func dropStaleOriginal() {
        if originalLensCorrection != value(Self.lensCorrectionPath).bool { originalCache = nil }
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
        dropStaleOriginal()
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
        // Never above the source's own resolution: Image Size can enlarge the preview.
        let sourceLong = Int(max(sourceSize.width, sourceSize.height))
        let raw = isRaw, maxPixels = sourceLong > 0 ? min(max(image.width, image.height), sourceLong) : max(image.width, image.height), generation = sourceGeneration
        let lensCorrection = value(Self.lensCorrectionPath).bool
        originalLoad = Task { [weak self] in
            let surface = await Task.detached(priority: .userInitiated) {
                OriginalImage.load(source, raw: raw, lensCorrection: lensCorrection, maxPixels: maxPixels)
            }.value
            guard let self, generation == self.sourceGeneration, !Task.isCancelled else { return }
            self.originalLoad = nil
            self.originalLensCorrection = lensCorrection
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

    /// The edit as the helper renders it: in the preview space. Sessions
    /// keep the edit itself; the space is the app's.
    var renderState: EditState {
        var rendered = state
        rendered.params["io.output_color_space"] = .string(previewSpace)
        return rendered
    }
    /// Develops the preview in `name` from now on, and remembers it for the
    /// next save.
    func choosePreviewSpace(_ name: String) {
        guard name != previewSpace, let index = SaveSettings.colorSpaces.firstIndex(of: name) else { return }
        SaveSettings.chosenSpace = index
        previewSpace = name
        changed()
    }

    private func changed(invalidateCache: Bool = true) {
        if invalidateCache { previewCache.invalidate(); showingCachedPreview = false }
        revision += 1; previewCurrent = false
        if renderFailed { renderFailed = false }
        // Dims the thumbnails now, or blanks them for a new photograph.
        pumpPreviews()
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
            // The main preview goes first; thumbnails follow once it's current.
            self.pumpPreviews()
        }
    }
    private func render() async {
        guard !rendering, !closing, let source, let engine else { return }
        let currentRevision = revision, snapshot = renderState, generation = sourceGeneration
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
            // The target describes the cropped photograph; the helper sizes the frame from the crop.
            if ImageSize.mode(of: snapshot.params) == .longestSide, let crop = Self.cropJSON(snapshot.crop) { command["crop"] = crop }
            let (id, result) = try await engine.request(command)
            guard shouldShow(currentRevision, generation) else { return }
            let width = Int(result["width"].double), height = Int(result["height"].double)
            let display = result["display_color_space"].string
            let developed = try await Task.detached(priority: .userInitiated) {
                try Self.readPreview(buffer, width: width, height: height, displaySpace: display)
            }.value
            guard shouldShow(currentRevision, generation) else { return }
            imageSize = CGSize(width: result["photo_width"].double, height: result["photo_height"].double)
            sourceSize = CGSize(width: result["source_width"].double, height: result["source_height"].double)
            workingSize = CGSize(width: result["working_width"].double, height: result["working_height"].double)
            if interactiveScale == nil {
                previewScale = workingSize.width > 0 ? min(Double(width) / workingSize.width, 1) : 1
            }
            showingInteractiveFrame = interactiveScale != nil
            image = developed; placeholder = nil
            updateHistogram()
            cropSession?.update(photo: shownImageSize, rotation: imageRotation)
            updateFilmStrip()
            isRaw = result["raw"].bool
            if inputColour != result["input_colour"] { inputColour = result["input_colour"] }
            lensCorrectionSupported = result["lens_correction_supported"].bool
            let ignored = Set(result["ignored_pins"].array.map { Int($0.double) })
            if ignored != ignoredPins { ignoredPins = ignored }
            if revision == currentRevision, interactiveScale == nil {
                renderID = id; previewCurrent = true; hideActivity()
                showingCachedPreview = false
                renderAtExportSize = result["export_resolution"].bool
                renderOnGPU = ExportEngine.ranOnGPU(backend: result["backend"].string)
                if let source = self.source { rememberFrame(developed, url: source, rendered: snapshot.rendered) }
                if let cacheKey { previewCache.save(developed, photoSize: imageSize, sourceSize: sourceSize, workingSize: workingSize, raw: isRaw, key: cacheKey) }
            }
            let seconds = result["elapsed_ms"].double / 1000
            let time = "developed in \(seconds.formatted(.number.precision(.fractionLength(seconds < 10 ? 2 : 1)))) s"
            // The photograph as exported, and whether the shown texture has fewer
            // pixels: inspect grain in an export when it does.
            let photograph = crop.size(of: workingSize)
            status = interactiveScale != nil
                ? "Preview · \(Int(result["output_width"].double)) × \(Int(result["output_height"].double)) · \(time)"
                : "\(Int(photograph.width)) × \(Int(photograph.height))\(previewScale < 1 ? " · shown at \(Self.percent(previewScale))" : "") · \(time)"
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

    // MARK: Settings file

    /// Saved state brought up to the engine's current parameters.
    private func merged(_ saved: EditState) -> EditState {
        var restored = saved
        let lensCorrection = restored.params[Self.lensCorrectionPath]
        restored.params = defaults.restoring(restored.params, nullable: Set(catalog["nullable_paths"].array.map(\.string)))
        // Saved from before lens correction, it was developed without it.
        if lensCorrection == .null { restored.params[Self.lensCorrectionPath] = .bool(false) }
        // Saved from before Image Size, it may carry the old working-resolution factor.
        let factor = defaults[ImageSize.factorPath]
        if factor != .null { restored.params[ImageSize.factorPath] = factor }
        // The export size belongs to no photograph. What's saved with one is
        // passed over, except to set it for the first time.
        if let size = Self.savedImageSize {
            restored.params[ImageSize.targetPath] = size
        } else {
            Self.savedImageSize = restored.params[ImageSize.targetPath]
        }
        return restored
    }

    /// The Image Size every photograph exports at, or nil before there's been one.
    private static var savedImageSize: JSONValue? {
        get {
            guard let pixels = UserDefaults.standard.object(forKey: "imageSize") as? Double else { return nil }
            return pixels > 0 ? .number(pixels) : .null
        }
        set {
            UserDefaults.standard.set(newValue.map { if case .number(let pixels) = $0 { pixels } else { 0.0 } }, forKey: "imageSize")
        }
    }

    /// Applies the settings file beside a photograph, if it has one, and returns
    /// the state it replaced. A file that can't be used is reported, and the
    /// photograph opens without it.
    private func loadSettings(beside url: URL) -> EditState? {
        guard hasCatalog else { return nil }
        do {
            guard let saved = try PhotoSettingsFile.read(beside: url) else { return nil }
            try EditorSession(source: url, state: saved.state).validate()
            guard films.contains(where: { $0.id == saved.state.film }),
                  papers.contains(where: { $0.id == saved.state.paper }) else {
                throw EngineError(message: "“\(url.lastPathComponent)” has settings for a film or paper that isn’t available, so it opened without them.")
            }
            let before = state
            state = merged(saved.state)
            return before
        } catch let failure as EngineError {
            error = failure.message
        } catch {
            self.error = error.localizedDescription
        }
        return nil
    }

    // MARK: Saving edits automatically

    /// Every undoable edit comes here. Its file is written shortly after, once a
    /// run of edits has paused, so each photograph keeps what was last done to it.
    private func markEdited() {
        guard source != nil else { return }
        editedSinceSave = true
        autosave?.cancel()
        autosave = Task { [weak self] in
            try? await Task.sleep(for: .seconds(Self.autosaveDelay))
            guard !Task.isCancelled else { return }
            self?.saveEditsNow()
        }
    }

    /// Writes an edit that hasn't been saved. It's only ever background upkeep,
    /// so a folder that can't be written doesn't interrupt with an alert.
    private func saveEditsNow() {
        autosave?.cancel(); autosave = nil
        guard editedSinceSave, let source else { return }
        editedSinceSave = false
        _ = try? PhotoSettingsFile(image: source.lastPathComponent, state: state).write(beside: source)
    }

    // MARK: What an unedited photograph opens with

    /// A folder's default, or the app's: opening a photograph never passes on
    /// the last one's edits. Image Size and the output colour space are export
    /// choices, not edits, so they stay as they are.
    private func startingState(in directory: URL) -> EditState {
        guard hasCatalog else { return state }
        var next = Self.folderDefaults[directory.standardizedFileURL.path].map(merged) ?? {
            var fresh = EditState()
            fresh.params = defaults
            if !films.contains(where: { $0.id == fresh.film }), let first = films.first { fresh.film = first.id }
            if !papers.contains(where: { $0.id == fresh.paper }), let first = papers.first { fresh.paper = first.id }
            PreviewRules.pairFilm(in: &fresh, stocks: stocks)
            return fresh
        }()
        next.limitPreview = state.limitPreview
        for path in [ImageSize.targetPath, "io.output_color_space", "io.output_cctf_encoding"] where state.params[path] != .null {
            next.params[path] = state.params[path]
        }
        return next
    }

    /// What a photograph with no edits of its own opens with.
    private func uneditedState(in directory: URL) -> EditState {
        var next = startingState(in: directory)
        next.params["io.rotate_cw"] = .number(0)
        // How one file's colours are read never carries over to the next.
        let interpretation = defaults["io.input_interpretation"]
        if interpretation != .null { next.params["io.input_interpretation"] = interpretation }
        next.params[Self.lensCorrectionPath] = defaults[Self.lensCorrectionPath]
        // Pins describe the tones of the photograph they were measured on,
        // so they never carry to another file.
        if next.params["post.tone_anchors"] != defaults["post.tone_anchors"] {
            next.params["post.tone_anchors"] = defaults["post.tone_anchors"]
        }
        next.crop = CropSettings()
        return next
    }

    /// The open photograph's camera details, if it has any.
    private(set) var photoExif: PhotoExif?

    /// ⌘D: what this folder's photographs without edits of their own open with,
    /// from now on. Those with edits keep them, and pins and crop stay here.
    func setFolderDefault() {
        guard let source, !cropping else { return }
        let directory = source.deletingLastPathComponent()
        var defaults = Self.folderDefaults
        defaults[directory.standardizedFileURL.path] = state
        Self.folderDefaults = defaults
        rollDeveloper?.invalidateAll()
        status = "Default set for \(directory.lastPathComponent)"
    }

    /// Puts this folder back on the app's defaults.
    func clearFolderDefault() {
        guard let source else { return }
        var defaults = Self.folderDefaults
        defaults[source.deletingLastPathComponent().standardizedFileURL.path] = nil
        Self.folderDefaults = defaults
    }

    /// The folders' defaults, by path.
    private static var folderDefaults: [String: EditState] {
        get {
            UserDefaults.standard.data(forKey: "rollLooks").flatMap { try? JSONDecoder().decode([String: EditState].self, from: $0) } ?? [:]
        }
        set { UserDefaults.standard.set(try? JSONEncoder().encode(newValue), forKey: "rollLooks") }
    }

    /// Saves the photograph's edit to a JSON file beside it.
    func saveSettings() {
        guard let source else { return }
        autosave?.cancel(); autosave = nil
        editedSinceSave = false
        do {
            let url = try PhotoSettingsFile(image: source.lastPathComponent, state: state).write(beside: source)
            status = "Saved \(url.lastPathComponent)"
        } catch {
            report(error)
        }
    }

    // MARK: The zone system

    /// Adds a pin reading the circle at `centre` to `zone`, and selects it. The
    /// average of a zone's pins prints as that zone. A drop during a render waits
    /// for it. `centre` is in fractions of the uncropped photograph.
    func placeZone(_ zone: Int, at centre: CGPoint) async {
        guard ZoneScale.zones.indices.contains(zone) else { return }
        let radius = sampleRadius
        guard let lstar = await measureTone(at: centre, radius: radius) else { return }
        let anchor = Self.anchor(input: lstar, target: ZoneScale.zones[zone].target, centre: centre, radius: radius)
        var anchors = state.params["post.tone_anchors"].array
        anchors.append(anchor)
        setAnchors(anchors, action: "Place Tone Pin")
        // A new pin is there to be seen and sized.
        pinsVisible = true
        selectedPin = anchors.indices.last
    }

    /// Measures a pin again at a new spot, keeping its zone and size.
    func movePin(_ index: Int, to centre: CGPoint) async {
        guard pins.indices.contains(index) else { return }
        let radius = pins[index].source?.radius ?? sampleRadius
        guard let lstar = await measureTone(at: centre, radius: radius), pins.indices.contains(index) else { return }
        var anchors = state.params["post.tone_anchors"].array
        anchors[index] = Self.anchor(input: lstar, target: pins[index].target, centre: centre, radius: radius)
        setAnchors(anchors, action: "Move Tone Pin")
        selectedPin = index
    }

    /// One step of the scroll wheel on the selected pin. The circle follows at
    /// once; when the wheel stops the area is measured as one edit.
    func resizeSelectedPin(scrollDelta: Double, precise: Bool) {
        guard let index = selectedPin, pins.indices.contains(index), pins[index].source != nil else { return }
        let current = pendingResize?.index == index ? pendingResize!.radius : pins[index].source!.radius
        let radius = SampleSize.radius(from: current, scrollDelta: scrollDelta, precise: precise)
        pendingResize = (index, radius)
        sampleRadius = radius
        resizeCommit?.cancel()
        resizeCommit = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else { return }
            await self?.commitResize(of: index, to: radius)
        }
    }

    private func commitResize(of index: Int, to radius: Double) async {
        guard pins.indices.contains(index), let centre = pins[index].source?.centre,
              let lstar = await measureTone(at: centre, radius: radius) else {
            if !Task.isCancelled { pendingResize = nil }
            return
        }
        guard !Task.isCancelled, pins.indices.contains(index) else { return }
        var anchors = state.params["post.tone_anchors"].array
        anchors[index] = Self.anchor(input: lstar, target: pins[index].target, centre: centre, radius: radius)
        setAnchors(anchors, action: "Resize Tone Pin")
    }

    private func cancelResize() {
        resizeCommit?.cancel()
        resizeCommit = nil
        pendingResize = nil
    }

    func selectPin(_ index: Int?) { selectedPin = index }

    var canRemovePin: Bool { selectedPin != nil }

    func removeSelectedPin() {
        guard let index = selectedPin, pins.indices.contains(index) else { return }
        var anchors = state.params["post.tone_anchors"].array
        anchors.remove(at: index)
        setAnchors(anchors, action: "Remove Tone Pin")
        selectedPin = nil
    }

    func clearToneSelection() { selectedPin = nil }

    // MARK: The tone curve

    private static let toneCurvePasteboard = NSPasteboard.PasteboardType("com.negs.tone-curve")
    private static let editPasteboard = NSPasteboard.PasteboardType("com.negs.edit")
    /// Counts copies, so what depends on the pasteboard looks again.
    private var copies = 0

    var toneCurve: ToneCurve { ToneCurve(anchors: pins.map { ($0.input, $0.target) }) }

    /// One point per target, as the engine reads them, left to right.
    var curvePoints: [CurvePoint] {
        var zones: [Int: [Int]] = [:]
        let pins = pins
        for (index, pin) in pins.enumerated() { zones[Int((min(max(pin.target, 0), 100) * 2).rounded()), default: []].append(index) }
        return zones.map { key, anchors in
            CurvePoint(input: anchors.map { pins[$0].input }.reduce(0, +) / Double(anchors.count), target: Double(key) / 2,
                       anchors: anchors, pinned: anchors.contains { pins[$0].source != nil },
                       ignored: anchors.allSatisfy(ignoredPins.contains))
        }.sorted { ($0.input, $0.target) < ($1.input, $1.target) }
    }

    /// Where a point at `from` may go: it stops at its neighbours, so the curve
    /// keeps rising and the point stays where it's put.
    private func curvePosition(input: Double, target: Double, from: Double, excluding anchors: [Int]) -> (input: Double, target: Double) {
        let others = curvePoints.filter { Set($0.anchors).isDisjoint(with: anchors) }
        let below = others.last { $0.input < from }, above = others.first { $0.input >= from }
        let inputs = (low: below.map { $0.input + 0.5 } ?? 0, high: above.map { $0.input - 0.5 } ?? 99.5)
        let targets = (low: below.map { $0.target + 0.5 } ?? 0, high: above.map { $0.target - 0.5 } ?? 100)
        return (min(max(input, inputs.low), max(inputs.high, inputs.low)),
                (min(max(target, targets.low), max(targets.high, targets.low)) * 2).rounded() / 2)
    }

    private static func plainAnchor(input: Double, target: Double) -> JSONValue {
        .object(["input": .number(input), "target": .number(target)])
    }

    /// Adds a point, returning the anchors now behind it.
    @discardableResult func addCurvePoint(input: Double, target: Double) -> [Int] {
        let place = curvePosition(input: input, target: target, from: min(max(input, 0), 99.5), excluding: [])
        var anchors = state.params["post.tone_anchors"].array
        anchors.append(Self.plainAnchor(input: place.input, target: place.target))
        setAnchors(anchors, action: "Add Curve Point")
        selectedPin = anchors.indices.last
        return [anchors.count - 1]
    }

    /// Moves a point. Any pins behind it come off the photograph, replaced by
    /// one plain point here. Returns the anchors now behind it.
    @discardableResult func moveCurvePoint(_ point: [Int], input: Double, target: Double) -> [Int] {
        let pins = pins
        guard !point.isEmpty, point.allSatisfy(pins.indices.contains) else { return point }
        let from = point.map { pins[$0].input }.reduce(0, +) / Double(point.count)
        let place = curvePosition(input: input, target: target, from: from, excluding: point)
        let moving = Set(point)
        var anchors = state.params["post.tone_anchors"].array.enumerated().filter { !moving.contains($0.offset) }.map(\.element)
        anchors.append(Self.plainAnchor(input: place.input, target: place.target))
        setAnchors(anchors, action: "Edit Tone Curve")
        selectedPin = anchors.indices.last
        return [anchors.count - 1]
    }

    func removeCurvePoint(_ point: [Int]) {
        let removing = Set(point)
        let anchors = state.params["post.tone_anchors"].array.enumerated().filter { !removing.contains($0.offset) }.map(\.element)
        setAnchors(anchors, action: "Remove Curve Point")
        selectedPin = nil
    }

    /// Takes every point and pin off, as one edit.
    func resetToneCurve() {
        setAnchors([], action: "Reset Tone Curve")
        selectedPin = nil
    }

    func copyToneCurve() {
        let points = curvePoints.map { Self.plainAnchor(input: $0.input, target: $0.target) }
        guard let data = try? JSONEncoder().encode(JSONValue.array(points)) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setData(data, forType: Self.toneCurvePasteboard)
        copies += 1
    }

    var canPasteToneCurve: Bool {
        _ = copies
        return NSPasteboard.general.availableType(from: [Self.toneCurvePasteboard]) != nil
    }

    /// Replaces the curve. The points were measured elsewhere, so they arrive
    /// as plain points, not pins on this photograph.
    func pasteToneCurve() {
        guard let data = NSPasteboard.general.data(forType: Self.toneCurvePasteboard),
              case .array(let points)? = try? JSONDecoder().decode(JSONValue.self, from: data) else { return }
        setAnchors(points, action: "Paste Tone Curve")
        selectedPin = nil
    }

    // MARK: Copying a whole edit

    func copyEdit() {
        guard let data = try? JSONEncoder().encode(state) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setData(data, forType: Self.editPasteboard)
        copies += 1
    }

    var canPasteEdit: Bool {
        _ = copies
        return NSPasteboard.general.availableType(from: [Self.editPasteboard]) != nil
    }

    /// Takes another photograph's edit. What's particular to this one stays: its
    /// pins, crop, rotation and how its file is read, and the export's settings.
    func pasteEdit() {
        guard hasCatalog, source != nil, !cropping, let data = NSPasteboard.general.data(forType: Self.editPasteboard),
              let copied = try? JSONDecoder().decode(EditState.self, from: data) else { return }
        var next = merged(copied)
        next.crop = state.crop
        next.limitPreview = state.limitPreview
        for path in ["post.tone_anchors", "io.rotate_cw", "io.input_interpretation", Self.lensCorrectionPath,
                     ImageSize.targetPath, "io.output_color_space", "io.output_cctf_encoding"] where state.params[path] != .null {
            next.params[path] = state.params[path]
        }
        apply(next, action: "Paste Edit")
    }

    /// The tone of a circle on the current render, as the pins read it: before
    /// the curve they make. Waits out any render. Nil if the photograph
    /// changes first, cropping starts or the render fails.
    private func measureTone(at centre: CGPoint, radius: Double) async -> Double? {
        guard let engine else { return nil }
        let generation = sourceGeneration
        while generation == sourceGeneration, !closing, !cropping, !renderFailed, !Task.isCancelled {
            guard previewCurrent, !rendering else {
                try? await Task.sleep(for: .milliseconds(20))
                continue
            }
            let measuring = revision
            do {
                let (_, result) = try await engine.request([
                    "command": .string("sample_tone"), "render_id": .number(Double(renderID)),
                    "x": .number(centre.x), "y": .number(centre.y), "radius": .number(radius)
                ])
                if measuring == revision, generation == sourceGeneration { return result["lstar"].double }
            } catch {
                report(error)
                return nil
            }
        }
        return nil
    }

    private static func anchor(input: Double, target: Double, centre: CGPoint, radius: Double) -> JSONValue {
        .object([
            "input": .number(input), "target": .number(target),
            "source": .object(["x": .number(centre.x), "y": .number(centre.y), "radius": .number(radius)])
        ])
    }

    /// Replaces the pins as one undoable edit.
    private func setAnchors(_ anchors: [JSONValue], action: String) {
        var next = state
        next.params["post.tone_anchors"] = .array(anchors)
        apply(next, action: action)
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

    // MARK: Film, paper and preset previews

    /// Settings › Film, paper and preset previews. Off shows the menus and
    /// the plain preset list, and frees the thumbnails here and in the helper.
    func setPreviews(_ enabled: Bool) {
        guard enabled != previews.enabled else { return }
        previews.enabled = enabled
        if enabled { pumpPreviews(); return }
        if previews.clear(), let engine {
            Task.detached(priority: .utility) { _ = try? await engine.request(["command": .string("release_thumbnails")]) }
        }
    }

    /// Marks out-of-date thumbnails (dimmed, or blank for another photograph)
    /// and requests the next one of an open grid or the Presets list: one
    /// helper request at a time, and only while the main preview is current
    /// and nothing is dragged.
    func pumpPreviews() {
        guard previews.enabled, !closing, let source else { return }
        let scanning = state.params["io.scan_film"].bool
        let rendered = renderState, generation = sourceGeneration
        // Each wanted tile: its look's key, the request, and a name for logs.
        var wanted: [(tile: PreviewTile, key: PreviewTile.Key, command: [String: JSONValue], name: String)] = []
        var entries: [StockEntry] = []
        if previews.filmOpen, previews.filmShown { entries += previews.films }
        if previews.paperOpen, previews.paperShown, !scanning { entries += previews.papers }
        for entry in entries {
            let command = PreviewRules.request(for: entry.stock, state: rendered, stocks: stocks, source: source, generation: generation)
            wanted.append((entry.tile, PreviewTile.Key(generation: generation, request: command), command, entry.stock.id))
        }
        if previews.presetsShown, checkingPresets == 0 {
            let kept = catalog["preset_kept"].array.map(\.string)
            let ordered = PreviewRules.renderOrder(presets, raw: isRaw, whiteBalance: { self.presetWhiteBalances[$0] },
                                                        photo: rendered.params["io.raw_white_balance"])
            for preset in ordered {
                guard case .valid = presetStatus[preset], let text = presetTexts[preset], let tile = previews.tile(preset: preset.url) else { continue }
                let command = PreviewRules.presetCommand(text: text, state: rendered, source: source, generation: generation,
                                                         includeFinish: saveFinishInPresets)
                wanted.append((tile, PreviewRules.presetKey(command, kept: kept.isEmpty ? nil : kept, generation: generation), command, preset.name))
            }
        }
        guard !wanted.isEmpty else { return }
        var next: (tile: PreviewTile, key: PreviewTile.Key, command: [String: JSONValue], name: String)?
        for item in wanted {
            if item.tile.mark(wanted: item.key), next == nil { next = item }
        }
        guard !previews.busy, ready, previewCurrent, !rendering, pendingRender == nil, !interacting, let engine else { return }
        guard let (tile, key, request, name) = next else {
            if let batch = previews.batch {
                previews.batch = nil
                let ms = Date().timeIntervalSince(batch.started) * 1000
                previews.lastBatch = (batch.count, ms)
                if ProcessInfo.processInfo.environment["NEGS_STOCK_THUMB_LOG"] != nil {
                    FileHandle.standardError.write(Data(String(format: "thumbnails: %d in %.0f ms (%.0f ms each)\n", batch.count, ms, ms / Double(max(batch.count, 1))).utf8))
                }
            }
            return
        }
        let started = previews.batch?.started ?? Date()
        previews.batch = (started, (previews.batch?.count ?? 0) + 1)
        previews.busy = true
        let buffer = engine.directory.appendingPathComponent("thumbnail-\(UUID().uuidString).rgba")
        var command = request
        command["preview_path"] = .string(buffer.path)
        Task { [weak self] in
            let image: CGImage?
            do {
                let (_, result) = try await engine.request(command)
                let (width, height) = (Int(result["width"].double), Int(result["height"].double))
                let display = result["display_color_space"].string
                image = await Task.detached(priority: .userInitiated) {
                    defer { try? FileManager.default.removeItem(at: buffer) }
                    return try? Self.thumbnailImage(Data(contentsOf: buffer), width: width, height: height, displaySpace: display)
                }.value
                if ProcessInfo.processInfo.environment["NEGS_STOCK_THUMB_LOG"] != nil {
                    FileHandle.standardError.write(Data("thumb \(name): \(Int(result["elapsed_ms"].double)) ms \(result["timing_ms"])\n".utf8))
                }
            } catch {
                try? FileManager.default.removeItem(at: buffer)
                FileHandle.standardError.write(Data("thumbnail \(name): \(error.localizedDescription)\n".utf8))
                image = nil
            }
            guard let self else { return }
            self.previews.busy = false
            guard !self.closing, self.previews.enabled else { return }
            // The photograph changed meanwhile: drop it, the next pump retries.
            // A failure stays blank until the next edit, so nothing spins.
            if key.generation == self.sourceGeneration { tile.show(image, for: key) }
            self.pumpPreviews()
        }
    }

    /// The helper's RGBA thumbnail as an image tagged like the main preview
    /// (`display_color_space`), so it shows the same colours.
    nonisolated static func thumbnailImage(_ data: Data, width: Int, height: Int, displaySpace: String) throws -> CGImage {
        guard width > 0, height > 0, width <= 4096, height <= 4096, data.count == width * height * 4,
              let space = displayColorSpace(displaySpace), let provider = CGDataProvider(data: data as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider,
                                  decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { throw EngineError(message: "The image engine returned an invalid thumbnail.") }
        return image
    }

    /// Reads the engine's RGBA buffer into an IOSurface tagged with its
    /// colour space (`display_color_space`). Core Animation composites it
    /// directly, converting for the display, with nothing on the main thread.
    nonisolated static func readPreview(_ url: URL, width: Int, height: Int, displaySpace: String = "sRGB") throws -> IOSurface {
        try previewSurface(Data(contentsOf: url), width: width, height: height, displaySpace: displaySpace)
    }

    /// The helper's proxy spaces: sRGB for an sRGB preview, otherwise
    /// Rec. 2020 primaries with the sRGB curve, which holds the wide spaces.
    nonisolated static func displayColorSpace(_ name: String) -> CGColorSpace? {
        CGColorSpace(name: name == "Rec. 2020 (sRGB transfer)" ? CGColorSpace.itur_2020_sRGBGamma : CGColorSpace.sRGB)
    }

    /// The proxy space a surface from `previewSurface` is tagged with.
    nonisolated static func displaySpace(of surface: IOSurface) -> String {
        let tag = IOSurfaceCopyValue(surface, kIOSurfaceColorSpace).flatMap { CGColorSpace(propertyListPlist: $0 as CFPropertyList) }
        return tag?.name == CGColorSpace.itur_2020_sRGBGamma ? "Rec. 2020 (sRGB transfer)" : "sRGB"
    }

    nonisolated static func previewSurface(_ data: Data, width: Int, height: Int, bgra: Bool = false, displaySpace: String = "sRGB") throws -> IOSurface {
        let invalid = EngineError(message: "The image engine returned an invalid preview buffer.")
        guard width > 0, height > 0, width <= 8192, height <= 8192 else { throw EngineError(message: "Invalid preview dimensions") }
        guard data.count == width * height * 4,
              let surface = IOSurface(properties: [
                  .width: width, .height: height, .bytesPerElement: 4,
                  .bytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4),
                  .pixelFormat: kCVPixelFormatType_32BGRA
              ]),
              let colorSpace = displayColorSpace(displaySpace)?.copyPropertyList()
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
        IOSurfaceSetValue(surface, kIOSurfaceColorSpace, colorSpace)
        return surface
    }

    // MARK: Saving

    var canExportLUT: Bool { hasCatalog && engine != nil && !saving && !exporting && !cropping }

    // MARK: Print balance

    private(set) var balancingPrint = false
    var canBalancePrint: Bool { hasCatalog && engine != nil && !balancingPrint && !cropping && !state.params["io.scan_film"].bool }

    /// Solves the enlarger's magenta and yellow shifts so a mid-grey prints
    /// neutral with the current settings (upstream's print balance), and
    /// sets them as one undoable edit. Doesn't depend on the photograph.
    func balancePrint() {
        guard canBalancePrint, let engine else { return }
        let snapshot = state
        balancingPrint = true
        status = "Balancing print…"
        Task {
            defer { balancingPrint = false }
            do {
                let (_, result) = try await engine.request(["command": .string("balance_print"), "film": .string(snapshot.film),
                                                            "paper": .string(snapshot.paper), "params": snapshot.params])
                let m = result["m_filter_shift"].double, y = result["y_filter_shift"].double
                var next = state
                next.params["enlarger.m_filter_shift"] = .number(m)
                next.params["enlarger.y_filter_shift"] = .number(y)
                apply(next, action: "Balance Print")
                status = "Print balanced: magenta \(m.formatted(.number.precision(.fractionLength(2)))), yellow \(y.formatted(.number.precision(.fractionLength(2))))"
            } catch { report(error) }
        }
    }

    func exportLUTPanel() {
        guard canExportLUT else { return }
        let panel = NSSavePanel()
        panel.title = "Export LUT"
        panel.canCreateDirectories = true
        panel.allowedContentTypes = [UTType(filenameExtension: "cube") ?? .data]
        panel.nameFieldStringValue = "\(state.film)_\(state.params["io.scan_film"].bool ? "direct_scan" : state.paper).cube"
        panel.message = "33 × 33 × 33 CUBE · sRGB input and output. Uses the current film and paper color settings. Auto exposure, print filter shifts, preflash, grain, halation, glare, diffusion, blur, sharpening, RAW development, and Post edits are excluded."
        if let path = UserDefaults.standard.string(forKey: "lastLUTFolder") {
            panel.directoryURL = URL(fileURLWithPath: path, isDirectory: true)
        }
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            UserDefaults.standard.set(url.deletingLastPathComponent().path, forKey: "lastLUTFolder")
            Task { await self.exportLUT(to: url) }
        }
    }

    func exportLUT(to url: URL) async {
        guard canExportLUT, let engine else { return }
        if let source, Self.sameFile(url, source) {
            error = "Choose a different filename to preserve the original photograph."; return
        }
        let snapshot = state
        let temporary = engine.directory.appendingPathComponent("lut-\(UUID().uuidString).cube")
        saving = true
        status = "Exporting LUT…"
        defer { saving = false; try? FileManager.default.removeItem(at: temporary) }
        do {
            _ = try await engine.request(["command": .string("export_lut"), "film": .string(snapshot.film),
                                          "paper": .string(snapshot.paper), "params": snapshot.params,
                                          "size": .number(33), "output": .string(temporary.path)])
            try await Task.detached {
                try Data(contentsOf: temporary).write(to: url, options: .atomic)
            }.value
            status = "Exported \(url.lastPathComponent)"
        } catch { report(error) }
    }

    func savePanel(export: Bool) {
        guard let source, !cropping, export ? !exporting && !saving : canSave else { return }
        let format = exportFormat
        let engine = exportEngine
        // Save Preview keeps the whole preview; Export uses the tab's size.
        let size = export ? exportSize : .full
        let save = exportSaveSettings
        let panel = NSSavePanel()
        panel.title = export ? "Export Photograph" : "Save Preview"
        panel.canCreateDirectories = true
        panel.allowedContentTypes = [format.type]
        // Without a directory the panel shows the last folder of any panel,
        // usually the Open panel's, so start where the last save went.
        if let folder = Self.lastSaveFolder { panel.directoryURL = folder }
        var extras = ""
        let photograph = photographSize
        if photograph != .zero { extras += " The photograph is \(Int(photograph.width)) × \(Int(photograph.height)) px." }
        if state.crop.cropped { extras += " Crops the photograph." }
        if let border = state.border.exported(for: photoSize) {
            extras += " Adds the \(border.style.title.lowercased()) border\(border.instagram ? ", extended to 3:4 for Instagram" : "")."
        }
        if let fit = size.summary { extras += " \(fit)." }
        if let space = save.summary(export: export, previewSpace: previewSpace) { extras += " \(space)." }
        let engineNote = export
            ? (engine == .reference ? "The most precise result. Can take about half a minute for large photos." : "Looks the same as Reference, in a fraction of the time.")
            : "Saves the preview as shown."
        panel.message = engineNote + extras
        panel.nameFieldStringValue = "\(source.deletingPathExtension().lastPathComponent)_\(state.film).\(format.type.preferredFilenameExtension ?? "png")"
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            guard !Self.sameFile(url, source) else {
                self.error = "Choose a different filename to preserve the original photograph."; return
            }
            Self.lastSaveFolder = url.deletingLastPathComponent()
            Task {
                if export { await self.export(to: url, engine: engine, size: size, save: save) }
                else { await self.savePreview(to: url, size: size, save: save) }
                // Then the preview follows the space just saved in.
                self.choosePreviewSpace(save.spaceName)
            }
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

    /// Saves the current preview with the crop, rotation, border, size, and encoding.
    func saveCommand(to url: URL, size: ExportSize, save: SaveSettings) -> [String: JSONValue] {
        var command: [String: JSONValue] = ["command": .string("save"), "render_id": .number(Double(renderID)), "output": .string(url.path),
                                            "rotate_cw": .number(Double(imageRotation)), "save_options": save.json,
                                            "crop_after_post": .bool(true)]
        if let crop = Self.cropJSON(state.crop) { command["crop"] = crop }
        if let border = state.border.exported(for: photoSize) {
            command["border"] = .string(border.style.rawValue)
            command["border_options"] = .object(border.options)
            command["instagram"] = .bool(border.instagram)
        }
        if let fit = size.json { command["fit"] = fit }
        return command
    }

    /// A crop for the helper, or nil when uncropped.
    nonisolated static func cropJSON(_ crop: CropSettings) -> JSONValue? {
        guard crop.cropped else { return nil }
        let r = crop.rect
        return .object(["x": .number(r.minX), "y": .number(r.minY), "width": .number(r.width), "height": .number(r.height), "angle": .number(crop.angle)])
    }

    /// A reduction such as 68%; below 10% with one decimal.
    nonisolated static func percent(_ fraction: Double) -> String {
        (fraction * 100).formatted(.number.precision(.fractionLength(fraction < 0.1 ? 1 : 0))) + "%"
    }

    private func savePreview(to url: URL, size: ExportSize = .full, save: SaveSettings = SaveSettings()) async {
        guard canSave, let engine else { return }
        saving = true
        defer { saving = false }
        do {
            _ = try await engine.request(saveCommand(to: url, size: size, save: save))
            status = "Saved \(url.lastPathComponent)"
        } catch { report(error) }
    }

    /// A current preview at export size is what a Fast export would render,
    /// so save it instead of developing the photograph again. Like ExportJob,
    /// write a sibling temporary file and rename it, so a failure leaves an
    /// existing file untouched. False if the preview could not be saved.
    /// Another colour space than the preview's develops again in it.
    private func exportPreview(to url: URL, size: ExportSize, save: SaveSettings) async -> Bool {
        guard canSave, renderAtExportSize, save.spaceName == previewSpace, let engine else { return false }
        saving = true
        defer { saving = false }
        let staging = url.deletingLastPathComponent().appendingPathComponent(".negs-\(UUID().uuidString).\(url.pathExtension)")
        defer { try? FileManager.default.removeItem(at: staging) }
        guard (try? await engine.request(saveCommand(to: staging, size: size, save: save))) != nil else { return false }
        guard rename(staging.path, url.path) == 0 else {
            report(EngineError(message: "Could not move the completed export to \(url.path): \(String(cString: strerror(errno)))"))
            return true
        }
        status = renderOnGPU
            ? "Exported \(url.lastPathComponent)"
            : "Exported \(url.lastPathComponent) on the CPU (32-bit): no GPU was available for Fast export"
        return true
    }

    func export(to url: URL, engine exportEngine: ExportEngine, size: ExportSize = .full, save: SaveSettings = SaveSettings()) async {
        // An edit made meanwhile makes the save fail; develop the photograph then.
        if exportEngine == .fast, await exportPreview(to: url, size: size, save: save) { return }
        guard !exporting, !saving, let source, let engine else { return }
        let job = ExportJob(); exportJob = job; exporting = true; exportStarted = Date()
        defer { exporting = false; exportJob = nil }
        do {
            let outcome = try await job.run(paths: engine.paths, scratch: engine.scratch, source: source, destination: url, state: state,
                                            crop: state.crop.cropped ? state.crop : nil, border: state.border.exported(for: photoSize), size: size, save: save, engine: exportEngine)
            status = outcome.fellBackToCPU
                ? "Exported \(url.lastPathComponent) on the CPU (32-bit): no GPU was available for Fast export"
                : "Exported \(url.lastPathComponent)"
        } catch is CancellationError { status = "Export cancelled" }
        catch { report(error) }
    }
    func cancelExport() { exportJob?.cancel() }
    func flushPreviewCache() async { await previewCache.flush() }
    // MARK: Presets

    /// The toolbar's Adjustments and Presets buttons: show that panel, or
    /// hide the column if it already shows it.
    func togglePane(_ pane: InspectorPane) {
        if inspectorVisible && inspectorPane == pane { inspectorVisible = false } else { inspectorPane = pane; inspectorVisible = true }
    }
    func showPane(_ pane: InspectorPane) { inspectorPane = pane; inspectorVisible = true }
    var canChangePresets: Bool { hasCatalog && !cropping && engine != nil }

    /// Lists the folder again, so files copied in by hand appear, and asks
    /// the engine about files it hasn't seen.
    func refreshPresets() async {
        do { presets = try await presetLibrary.list() } catch { presets = []; report(error) }
        presetStatus = presetStatus.filter { presets.contains($0.key) }
        presetTexts = presetTexts.filter { presets.contains($0.key) }
        presetWhiteBalances = presetWhiteBalances.filter { presets.contains($0.key) }
        previews.update(presets: presets)
        guard let engine else { return }
        checkingPresets += 1
        defer { checkingPresets -= 1; pumpPreviews() }
        for preset in presets where presetStatus[preset] == nil {
            do {
                let text = try Self.presetText(try await presetLibrary.read(preset))
                let (_, result) = try await engine.request(["command": .string("preset_import"), "text": .string(text)])
                presetStatus[preset] = .valid(film: result["film"].string, paper: result["paper"].string,
                                              scan: result["params"]["io.scan_film"].bool)
                presetTexts[preset] = text
                presetWhiteBalances[preset] = result["params"]["io.raw_white_balance"]
            } catch {
                presetStatus[preset] = .invalid(error.localizedDescription)
            }
        }
    }

    /// The film, paper and Lab settings of a preset, as one undo step. The
    /// photograph's own settings stay: Image Size, rotation, crop, border,
    /// output encoding, the preview limit and the Post tab.
    func applyPreset(_ preset: PresetFile) {
        guard canChangePresets, let engine else { return }
        Task {
            do {
                let text = try Self.presetText(try await presetLibrary.read(preset))
                // The engine keeps the photograph's settings from the params
                // it's given; ask again if they changed while it answered.
                for _ in 0..<3 {
                    let snapshot = state
                    let (_, result) = try await engine.request(["command": .string("preset_import"), "text": .string(text),
                                                                "params": snapshot.params,
                                                                "include_finish": .bool(saveFinishInPresets)])
                    guard state == snapshot else { continue }
                    let (film, paper) = (result["film"].string, result["paper"].string)
                    guard films.contains(where: { $0.id == film }), papers.contains(where: { $0.id == paper }) else {
                        throw EngineError(message: "This version of Negs doesn’t have the preset’s film or paper.")
                    }
                    var next = state
                    next.film = film; next.paper = paper; next.params = result["params"]
                    apply(next, action: "Apply Preset “\(preset.name)”")
                    status = "Applied preset “\(preset.name)”"
                    return
                }
            } catch {
                report(error)
                await refreshPresets()
            }
        }
    }

    /// Saves the film, paper and Lab settings as a new preset, named after
    /// the film, and starts renaming it.
    func newPreset() {
        guard canChangePresets, let engine else { return }
        showPane(.presets)
        let snapshot = state, name = PresetName.from(fileName: filmName)
        Task {
            do {
                let (_, result) = try await engine.request(["command": .string("preset_export"), "film": .string(snapshot.film),
                                                            "paper": .string(snapshot.paper), "params": snapshot.params,
                                                            "include_finish": .bool(saveFinishInPresets)])
                let file = try await presetLibrary.add(Data(result["text"].string.utf8), name: name)
                await refreshPresets()
                unnamedPreset = file.url
                renamingPreset = file.url
            } catch { report(error) }
        }
    }

    func importPresetsPanel() {
        guard canChangePresets else { return }
        let panel = NSOpenPanel()
        panel.title = "Import Presets"
        panel.message = "Choose presets saved by Negs or by spektrafilm (GUI state files)."
        panel.prompt = "Import"
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.json, .plainText]
        panel.begin { [weak self] response in
            guard response == .OK, let self else { return }
            MainActor.assumeIsolated { self.importPresets(panel.urls) }
        }
    }

    /// Copies each file that the engine reads into the Presets folder, under
    /// its own name, and lists the ones that it refuses. They go to the end,
    /// or before the preset at `position` (a drop between rows).
    func importPresets(_ urls: [URL], at position: Int? = nil) {
        guard canChangePresets, let engine, !urls.isEmpty else { return }
        showPane(.presets)
        Task {
            var failures: [String] = [], added: [URL] = []
            for url in urls {
                do {
                    let data = try await Task.detached(priority: .userInitiated) { try PresetLibrary.read(url) }.value
                    _ = try await engine.request(["command": .string("preset_import"), "text": .string(try Self.presetText(data))])
                    added.append(try await presetLibrary.add(data, name: PresetName.from(fileName: url.deletingPathExtension().lastPathComponent)).url)
                } catch {
                    failures.append("\(url.lastPathComponent): \(error.localizedDescription)")
                }
            }
            let imported = added.count
            if let position, imported > 0, let listed = try? await presetLibrary.list() {
                let moved = listed.filter { added.contains($0.url) }
                var order = listed.filter { !added.contains($0.url) }
                let before = listed.prefix(position).filter { !added.contains($0.url) }.count
                order.insert(contentsOf: moved, at: min(before, order.count))
                do { try await presetLibrary.setOrder(order) } catch { report(error) }
            }
            await refreshPresets()
            if imported > 0 { status = imported == 1 ? "Imported 1 preset" : "Imported \(imported) presets" }
            if !failures.isEmpty {
                error = (imported == 0 ? "No presets were imported.\n\n" : "Some files weren’t imported.\n\n")
                    + failures.joined(separator: "\n\n")
            }
        }
    }

    /// Moves presets in the list, as dragged, and remembers the order.
    func movePresets(from source: IndexSet, to destination: Int) {
        var order = presets
        order.move(fromOffsets: source, toOffset: destination)
        guard order != presets else { return }
        presets = order
        Task {
            do { try await presetLibrary.setOrder(order) } catch { report(error) }
            await refreshPresets()
        }
    }

    /// Escape while naming: keeps the old name, or removes a preset New
    /// Preset just made.
    func cancelRename(_ preset: PresetFile) {
        renamingPreset = nil
        guard preset.url == unnamedPreset else { return }
        unnamedPreset = nil
        Task {
            do { try await presetLibrary.delete(preset) } catch { report(error) }
            await refreshPresets()
        }
    }

    func renamePreset(_ preset: PresetFile, to name: String) {
        renamingPreset = nil
        if preset.url == unnamedPreset { unnamedPreset = nil }
        guard name != preset.name else { return }
        Task {
            do {
                let renamed = try await presetLibrary.rename(preset, to: name)
                // Same file, same thumbnail: no blank tile while it's renamed.
                previews.moveTile(preset: preset.url, to: renamed.url)
            } catch { report(error) }
            await refreshPresets()
        }
    }

    func deletePreset(_ preset: PresetFile) {
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Delete the preset “\(preset.name)”?"
        alert.informativeText = "This can’t be undone. Photographs edited with it keep their settings."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        Task {
            do { try await presetLibrary.delete(preset) } catch { report(error) }
            await refreshPresets()
        }
    }

    /// Writes a copy wherever the user picks, byte for byte.
    func exportPresetPanel(_ preset: PresetFile) {
        let panel = NSSavePanel()
        panel.title = "Export Preset"
        panel.canCreateDirectories = true
        panel.allowedContentTypes = [.json]
        panel.nameFieldStringValue = "\(preset.name).json"
        panel.message = "spektrafilm’s GUI state format: opens in Negs and in spektrafilm."
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            MainActor.assumeIsolated {
                Task {
                    do { try await self.presetLibrary.read(preset).write(to: url, options: .atomic) }
                    catch { self.report(error); await self.refreshPresets() }
                }
            }
        }
    }

    func showPresetsFolder() {
        let folder = presetLibrary.folder
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            NSWorkspace.shared.open(folder)
        } catch { report(error) }
    }

    nonisolated static func presetText(_ data: Data) throws -> String {
        guard let text = String(data: data, encoding: .utf8) else { throw EngineError(message: "The file isn’t text.") }
        return text
    }

    func shutdown() {
        saveEditsNow()
        rollDeveloper?.stop()
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
        case .png: "PNG (lossless)"
        case .jpeg: "JPEG (smaller)"
        case .tiff: "TIFF (lossless)"
        }
    }

    /// The format of the last save or export, offered first next time.
    static var last: OutputFormat {
        get { UserDefaults.standard.string(forKey: "outputFormat").flatMap(OutputFormat.init) ?? .png }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "outputFormat") }
    }
}

/// How a save encodes the file: bits per channel, colour space, and JPEG
/// quality. `colorSpace` is the space the file is in: ProPhoto RGB at 8 bits
/// is already replaced by Adobe RGB (`effectiveSpace`).
struct SaveSettings: Equatable {
    /// As colour names them, which the engine takes.
    static let colorSpaces = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"]
    /// ProPhoto RGB bands at 8 bits, so it needs 16; Adobe RGB stands in.
    static let prophoto = 3, prophotoFallback = 2
    static let qualityRange = 1...100

    /// nil is the format's default: 16 for TIFF, 8 for PNG and JPEG.
    var bitDepth: Int? = nil
    var colorSpace = 0
    var jpegQuality = 95
    /// Written into the file; it changes no pixels. nil leaves it to the format.
    var dpi: Int? = nil

    static let dpiRange = 1...9600
    /// 144 is 72 × 2, so apps that read it show a file at the size it has on a
    /// Retina screen at one file pixel to one screen pixel.
    static let defaultDPI = 144
    static var lastDPI: Int {
        get { let dpi = UserDefaults.standard.integer(forKey: "exportDPI"); return dpiRange.contains(dpi) ? dpi : defaultDPI }
        set { UserDefaults.standard.set(min(max(newValue, dpiRange.lowerBound), dpiRange.upperBound), forKey: "exportDPI") }
    }

    static func effectiveSpace(_ chosen: Int, bitDepth: Int) -> Int {
        let chosen = min(max(chosen, 0), colorSpaces.count - 1)
        return chosen == prophoto && bitDepth == 8 ? prophotoFallback : chosen
    }
    var spaceName: String { Self.colorSpaces[min(max(colorSpace, 0), Self.colorSpaces.count - 1)] }
    var isSRGB: Bool { colorSpace == 0 }

    /// The `save_options` of a save command.
    var json: JSONValue {
        var options: [String: JSONValue] = ["jpeg_quality": .number(Double(jpegQuality)), "color_space": .string(spaceName)]
        if let bitDepth { options["bit_depth"] = .number(Double(bitDepth)) }
        if let dpi { options["dpi"] = .number(Double(dpi)) }
        return .object(options)
    }
    /// The exporter's arguments.
    var arguments: [String] {
        ["--jpeg-quality", "\(jpegQuality)", "--color-space", spaceName] + (bitDepth.map { ["--bit-depth", "\($0)"] } ?? [])
            + (dpi.map { ["--dpi", "\($0)"] } ?? [])
    }
    /// For the save panel's message; nil for sRGB in an sRGB preview. The
    /// preview develops in `previewSpace`; afterwards it follows this save.
    func summary(export: Bool, previewSpace: String) -> String? {
        let follows = spaceName == previewSpace ? "" : "; the preview then shows \(spaceName)"
        if export { return isSRGB && follows.isEmpty ? nil : "Develops in \(spaceName) and embeds its profile\(follows)" }
        if spaceName == previewSpace { return isSRGB ? nil : "Keeps the preview's \(spaceName) colours and embeds its profile" }
        return "Converts the \(previewSpace) preview to \(spaceName), clipping colours outside the smaller space\(follows)"
    }

    /// The colour space last picked, which may be ProPhoto RGB at 8 bits.
    static var chosenSpace: Int {
        get { UserDefaults.standard.string(forKey: "colorSpace").flatMap { colorSpaces.firstIndex(of: $0) } ?? 0 }
        set { UserDefaults.standard.set(colorSpaces[min(max(newValue, 0), colorSpaces.count - 1)], forKey: "colorSpace") }
    }
    /// The bit depth last picked for `format`; JPEG is always 8-bit.
    static func bitDepth(for format: OutputFormat) -> Int {
        switch format {
        case .jpeg: 8
        case .png: UserDefaults.standard.integer(forKey: "bitDepth.png") == 16 ? 16 : 8
        case .tiff: UserDefaults.standard.integer(forKey: "bitDepth.tiff") == 8 ? 8 : 16
        }
    }
    static func setBitDepth(_ depth: Int, for format: OutputFormat) {
        if format != .jpeg { UserDefaults.standard.set(depth, forKey: "bitDepth.\(format.rawValue)") }
    }
    static var lastQuality: Int {
        get { let q = UserDefaults.standard.integer(forKey: "jpegQuality"); return qualityRange.contains(q) ? q : 95 }
        set { UserDefaults.standard.set(newValue, forKey: "jpegQuality") }
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

    /// What a Size field's text means: its digits, or the default when there
    /// are none, clamped to the range. The field's formatter doesn't enforce
    /// the range while typing, so partial entries such as "2" stay in the
    /// field and this settles the value.
    static func pixels(from text: String) -> Int {
        let value = Int(text.filter(\.isNumber)) ?? full.pixels
        return min(max(value, pixelRange.lowerBound), pixelRange.upperBound)
    }
}

