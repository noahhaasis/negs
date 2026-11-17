import Accelerate
import AppKit
import CoreVideo
import CryptoKit
import ImageIO
import IOSurface
import Observation
import SwiftUI
import UniformTypeIdentifiers
import UserNotifications

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
    /// Settings › Limit preview to 12 MP. A preference, not an edit, so Undo
    /// and Reset Adjustments leave it; the edit state carries a copy so the
    /// render and its cache follow it.
    var limitsPreview = UserDefaults.standard.bool(forKey: "limitPreview") {
        didSet {
            UserDefaults.standard.set(limitsPreview, forKey: "limitPreview")
            if !adoptingPreference { replaceState(state) }
        }
    }
    @ObservationIgnored private var adoptingPreference = false
    /// Settings › Use less memory for export: grain channels one at a time.
    /// Only exports read it; previews always develop the channels together.
    var lowMemoryExport = UserDefaults.standard.bool(forKey: "lowMemoryExport") {
        didSet { UserDefaults.standard.set(lowMemoryExport, forKey: "lowMemoryExport") }
    }
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
    /// The camera colour filters for B&W film, in the catalog's order.
    private(set) var colorFilters: [ColorFilter] = []
    private(set) var source: URL?
    /// The camera's facts for the caption over the photograph, read off the main actor.
    private(set) var photoFacts: PhotoFacts?
    /// The duplicate of `source` open (its sidecar variant); nil for the
    /// photograph itself.
    private(set) var sourceVariant: String?
    /// The open library item: `source`, or its duplicate (`URL.variant`).
    var sourceItem: URL? { source.map { file in sourceVariant.map(file.variant) ?? file } }
    /// The developed preview. Nil until the first render of `source` completes.
    private(set) var image: IOSurface?
    /// The unprocessed photograph, shown while the first render develops.
    private(set) var placeholder: CGImage?
    /// Its developed thumbnail, shown in the placeholder's place: the
    /// photograph as it will look, finished (cropped, turned, in its
    /// border), rather than the camera's own preview. From ctruett's fork.
    private(set) var standIn: CGImage?
    /// The film or paper tray, while it's open (IMPLEMENTATION.md §4.5, §5.1).
    private(set) var tray: TrayVisit?
    /// The tried stock's tile, enlarged on the canvas at once, until the
    /// develop of that edit is in (§5.1).
    private(set) var trialImage: CGImage?
    /// The tray and the names that open it: a click anywhere else keeps the
    /// stock being tried and closes the tray.
    @ObservationIgnored let trayHitAreas = NSHashTable<NSView>.weakObjects()
    /// The undeveloped photograph, shown in place of `image` while the
    /// Original button is held.
    private(set) var original: IOSurface?
    /// The tones of `image` within the crop; nil until it develops.
    private(set) var histogram: Histogram?
    /// The tone graph's curve for the current film, paper and settings
    /// (or the last ones, until the next arrives); nil while hidden.
    private(set) var toneCurve: ToneCurveData?
    /// The photograph's film exposure, from the last render.
    private(set) var filmHistogram: FilmHistogram?
    private(set) var waveform: Waveform?
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
    /// `io.flip`: mirrored left to right after the crop, before the turns.
    private(set) var imageFlip = false
    /// How the canvas shows the photograph, for the tray's and presets' tiles.
    var shownOrientation: Orientation { Orientation(turns: imageRotation, flip: imageFlip) }
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
    /// Why the latest edit couldn't be developed, said on the canvas over
    /// the last good preview (§4.14); nil when it could.
    private(set) var developFailure: String?
    var renderFailed: Bool { developFailure != nil }
    private(set) var saving = false
    private(set) var exporting = false
    private(set) var exportStarted = Date()
    /// A brightness solve is with the helper.
    private(set) var matchingBrightness = false
    /// A failed solve that trying again can't change: for the photograph
    /// (`inputs` nil, e.g. no camera preview) or for these match inputs.
    private(set) var brightnessMatchFailure: (generation: Int, inputs: JSONValue?, message: String)?
    /// Settings › Match new photographs to camera brightness: whether a new
    /// photograph starts with Prep › Match camera brightness on.
    var autoPlaceNewImages = UserDefaults.standard.object(forKey: "autoPlaceNewImages") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(autoPlaceNewImages, forKey: "autoPlaceNewImages")
            refreshUneditedLook()
        }
    }
    /// Settings › Fotufilm films.
    var fotufilmFilms = FotufilmFilms(rawValue: UserDefaults.standard.string(forKey: FotufilmFilms.key) ?? "") ?? .all {
        didSet {
            UserDefaults.standard.set(fotufilmFilms.rawValue, forKey: FotufilmFilms.key)
            pumpPreviews()
        }
    }
    /// Enlarger › Shifts move.
    var filterLink = FilterLink(rawValue: UserDefaults.standard.string(forKey: FilterLink.key) ?? "") ?? .separate {
        didSet { UserDefaults.standard.set(filterLink.rawValue, forKey: FilterLink.key) }
    }
    static let newPhotoFilmKey = "newPhotoFilm"
    static let newPhotoPaperKey = "newPhotoPaper"
    /// Settings › New photographs, as chosen: nil until a choice is made.
    private var chosenNewPhotoFilm = UserDefaults.standard.string(forKey: EditorModel.newPhotoFilmKey)
    private var chosenNewPhotoPaper = UserDefaults.standard.string(forKey: EditorModel.newPhotoPaperKey)
    /// Settings › Save ratings and edits next to photos: each photograph's
    /// edits in its sidecar (`Sidecar.swift`). Off, nothing is remembered per
    /// photograph; sidecars already there stay untouched until it's on again.
    /// Settings › Photographs › Show camera JPEGs separately: off, a RAW's
    /// camera JPEG of the same name goes with it as one photograph.
    var separateCameraJPEGs: Bool {
        get { !library.pairsCameraPictures }
        set {
            UserDefaults.standard.set(newValue, forKey: PhotoFolder.separateCameraPicturesKey)
            library.pairsCameraPictures = !newValue
        }
    }
    var sidecarsEnabled = UserDefaults.standard.object(forKey: "sidecars") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(sidecarsEnabled, forKey: "sidecars")
            sidecarWrite?.cancel(); sidecarWrite = nil
            library.store = sidecarsEnabled ? sidecars : nil
            // Off, every photograph develops as new; on, once read.
            pumpThumbnails()
            // The photograph shown is saved from its next edit on.
            if sidecarsEnabled, let source, let restore = readSidecar(source, variant: sourceVariant) {
                photoSidecar = sidecarContext(for: source, restore: restore)
            } else if !sidecarsEnabled { photoSidecar = nil }
        }
    }
    /// Lab › Film format › Custom picked while the value is still a format's,
    /// so the slider stays. Opening a photograph and Reset Adjustments clear it.
    var customFilmFormat = false
    /// Shown in the alert sheet until it's answered (`AlertSheet`).
    var error: String? {
        didSet { if error != oldValue { errorAlert = error.map { AlertRequest(alert: .error($0)) { _ in } } } }
    }
    /// `error` as the alert sheet shows it.
    private(set) var errorAlert: AlertRequest?
    /// The questions waiting for the alert sheet.
    @ObservationIgnored let alerts = AlertQueue()
    var status = "Starting…"
    var filmstripVisible = UserDefaults.standard.object(forKey: "filmstripVisible") as? Bool ?? true {
        didSet { UserDefaults.standard.set(filmstripVisible, forKey: "filmstripVisible") }
    }
    /// View › Image Only (F): the photograph with its caption and label,
    /// and the strip, status line and inspector hidden (§11). Their own
    /// settings stay as they were, so leaving it brings back what showed.
    /// Only in the photograph view, and not remembered across launches.
    private(set) var imageOnly = false
    /// The strip and the inspector as shown now: as set, unless Image Only hides them.
    var stripShown: Bool { filmstripVisible && !imageOnly }
    var inspectorShown: Bool { inspectorVisible && !imageOnly }
    /// T is held: the photograph on the canvas is turned upside down, to
    /// judge its composition alone, as Cartier-Bresson read his contact
    /// sheets. Display only: nothing renders, saves or exports differently.
    private(set) var upsideDown = false
    /// Photo › Advance After Rating: a rating or flag key moves on to the
    /// next photograph, as in Lightroom. Off until chosen.
    var advanceAfterRating = UserDefaults.standard.bool(forKey: "advanceAfterRating") {
        didSet { UserDefaults.standard.set(advanceAfterRating, forKey: "advanceAfterRating") }
    }
    /// The brief note over the canvas after a rating or flag key.
    private(set) var libraryNote: LibraryNote?
    /// The edit view or the grid. Every launch starts in the edit view.
    private(set) var mode = LibraryMode.photo {
        // The sheet stays drawn while it folds back into the strip; the
        // window clears this once it has (`SheetFlight`).
        didSet { if oldValue == .grid, mode == .photo { sheetFolding = true } }
    }
    /// The grid is folding back into the strip (§4.6).
    var sheetFolding = false
    /// Two photographs on screen together (ticket 46), in the photo view:
    /// the open one, which the panel edits, and another or its original.
    var comparison: Comparison?
    /// Each compared side the open photograph doesn't show.
    var compareSides: [Comparison.Side: CompareOther] = [:]
    /// Side by side as the canvas last laid it out: in a row or stacked.
    var compareArrangement = CompareArrangement.row
    @ObservationIgnored var compareDevelop: Task<Void, Never>?
    /// The helper that develops the other photograph, stopped on leaving.
    @ObservationIgnored var compareEngine: EngineClient?
    @ObservationIgnored var compareGeneration = 0
    /// ← → stepping the edited side: its open waits for the steps to stop.
    @ObservationIgnored var compareSteps: Task<Void, Never>?
    /// How long each other develop took, for tests and measurements.
    @ObservationIgnored var compareTimings: [TimeInterval] = []
    /// Original ⌄: what the Original word does.
    /// Original ⌄'s Show the RAW / Show the camera JPEG.
    var originalReference = OriginalReference.load() {
        didSet {
            guard originalReference != oldValue else { return }
            UserDefaults.standard.set(originalReference.rawValue, forKey: OriginalReference.key)
            originalReferenceChanged()
        }
    }
    var originalMode = OriginalMode.load() {
        didSet { UserDefaults.standard.set(originalMode.rawValue, forKey: OriginalMode.key); if originalMode != .split { clearSplit() } }
    }
    /// Original ⌄ › Split line: the line is on; the original shows left of it.
    private(set) var splitting = false
    /// Where the split line is, as a fraction of the canvas's width.
    var splitFraction: CGFloat = 0.5
    /// The original under the split line, once decoded.
    private(set) var splitOriginal: IOSurface?
    /// Grid thumbnails waiting while the grid opens or folds: a flyer is
    /// carrying them, or they fade in after.
    var sheetWaiting: Set<URL> = []
    /// Of those, the ones a flyer carries: their boxes stay where they are
    /// drawn in the grid rather than rising with the cell.
    var sheetCarried: Set<URL> = []
    /// Carried thumbnails whose boxes are hidden. They fade in as the
    /// flyer lands and fade out as it leaves, rather than appearing with
    /// the picture all at once.
    var sheetBoxesWaiting: Set<URL> = []
    /// Where the strip's frames and the grid's thumbnails are drawn, for
    /// the flight between them.
    @ObservationIgnored let places = ThumbnailPlaces()
    /// Each photograph's shape, for the strip's frames and the grid's rows.
    @ObservationIgnored let aspects = PhotoAspects()
    /// The selected photographs and the current one, which in the edit view
    /// is `source`. In the grid it moves without developing anything.
    private(set) var selection = LibrarySelection()
    /// The height of the grid's rows, in points (the sheet's Size).
    var gridCellSize = UserDefaults.standard.object(forKey: "gridThumbnailSize") as? Double ?? SheetLayout.defaultSize {
        didSet {
            let clamped = min(max(gridCellSize, SheetLayout.sizes.lowerBound), SheetLayout.sizes.upperBound)
            if clamped != gridCellSize { gridCellSize = clamped; return }
            UserDefaults.standard.set(gridCellSize, forKey: "gridThumbnailSize")
        }
    }
    /// The grid as laid out now, for moving up and down a row. Set by the sheet.
    @ObservationIgnored var gridLayout: SheetLayout?
    var inspectorVisible = true
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
    /// The preset applied last to each photograph this session, which the
    /// Presets pane rings (§4.12). By the photograph's item, so a duplicate
    /// has its own.
    private(set) var appliedPresets: [URL: URL] = [:]
    /// The preset applied last to the photograph on screen.
    var appliedPreset: URL? { sourceItem.flatMap { appliedPresets[$0] } }
    /// A preset New Preset just made: Escape while naming it removes it.
    @ObservationIgnored private var unnamedPreset: URL?
    var zoomCommand = UUID()
    var fitImage = true
    var recentFiles: [URL] = []
    @ObservationIgnored weak var undoManager: UndoManager?
    @ObservationIgnored private var defaults: JSONValue = .object([:])
    @ObservationIgnored private var catalog: JSONValue = .null
    @ObservationIgnored private var catalogRequest: Task<(Int, JSONValue), Error>?
    @ObservationIgnored private var engine: EngineClient?
    @ObservationIgnored private(set) lazy var filmCacheRelease = FilmCacheRelease { [weak self] in self?.engine }
    @ObservationIgnored private var exportJob: ExportJob?
    @ObservationIgnored private var revision = 0
    @ObservationIgnored private var sourceGeneration = 0
    /// The one brightness solve in flight; its completion looks again.
    @ObservationIgnored private var matchTask: Task<Void, Never>?
    @ObservationIgnored private var matchRequest: UUID?
    /// A settled preview rendered while the brightness match was pending:
    /// shown only if the match leaves the render as it is, so the canvas
    /// goes from the previous frame straight to the matched one.
    @ObservationIgnored private var heldPreview: (revision: Int, generation: Int, show: () -> Void)?
    /// The open photograph's camera preview for its brightness match, read
    /// once while the helper first decodes it: the colour space the RAW
    /// declares for it (nil until asked; null when it declares none) and the
    /// preview itself. The file is the same for the whole generation.
    @ObservationIgnored private var matchReference: (generation: Int, declared: JSONValue?, prepared: Task<CameraBrightnessReference.Prepared, Never>?)?
    /// Solved matches across photographs and launches (ticket 39).
    @ObservationIgnored var matchCache = BrightnessMatchCache.shared
    /// `BrightnessMatchCache.engineStamp` of the engine's paths, read once.
    @ObservationIgnored private var matchEngine: (helper: URL, data: URL, stamp: String)?
    /// The photograph the helper decoded alone (for a match, or before the
    /// first frame), and its size from that decode.
    @ObservationIgnored private var decodedFor: (generation: Int, size: CGSize)?
    /// What the canvas shows before the first frame: this generation's
    /// placeholder, at the photograph's size; `kept` when it's the
    /// photograph's kept last develop.
    @ObservationIgnored private var placeholderFor: (generation: Int, kept: Bool)?
    /// The edit whose decode for a brightness match failed: it renders
    /// without the match, which then reports the error as a render would.
    @ObservationIgnored private var matchDecodeFailed: FrameID?
    /// A discrete edit renders a canvas-size frame first, then the export-size
    /// one (`RenderStep.quick`). This is the edit whose quick frame is done
    /// (shown, held for the brightness match, or failed): its next render
    /// settles.
    @ObservationIgnored private var quickStep: FrameID?
    /// The shown frame is the quick frame of this edit.
    @ObservationIgnored private var quickFrame: FrameID?
    /// The helper's last output, when it's a settled render: the request
    /// without its post adjustments, which the helper applies to the kept
    /// output instead of rendering again. A quick frame would replace it,
    /// and a relaunched helper (`launch`) keeps none.
    @ObservationIgnored private var helperOutput: (key: [String: JSONValue], launch: Int)?
    struct FrameID: Equatable { var revision: Int, generation: Int }
    #if DEBUG
    /// Each render sent to the helper, for tests.
    @ObservationIgnored private(set) var renderLog: [RenderStep] = []
    /// Runs as each render starts, before it looks at anything, for tests.
    @ObservationIgnored var willRender: (() -> Void)?
    func killHelperForTesting() { engine?.killHelperForTesting() }
    var compareEngineRunning: Bool { compareEngine != nil }
    var holdingFrameForMatch: Bool { heldPreview != nil }
    /// Brightness matches solved by the helper (not found in `matchCache`), for tests.
    @ObservationIgnored private(set) var matchSolves = 0
    /// Runs before a brightness match's decode; throwing fails the decode, for tests.
    @ObservationIgnored var matchDecodeWillRun: (() throws -> Void)?
    /// The frame on the canvas: the edit it was rendered for, and how
    /// (nil: the restored session's saved frame), for tests.
    @ObservationIgnored private(set) var shownFrame: (frame: FrameID, step: RenderStep?)?
    var currentFrame: FrameID { FrameID(revision: revision, generation: sourceGeneration) }
    var renderScheduled: Bool { pendingRender != nil }
    var matchDecodeFailedHere: Bool { matchDecodeFailed == currentFrame }
    /// Each edited thumbnail or kept develop saved: the item, the look it's
    /// filed under, and the edits it shows (`userEdits`), for tests.
    @ObservationIgnored var thumbnailSaved: ((_ item: URL, _ look: String, _ edits: EditState, _ kind: String) -> Void)?
    #endif
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
    @ObservationIgnored private var toneGraphsShown = 0
    @ObservationIgnored private var toneCurveTask: Task<Void, Never>?
    @ObservationIgnored private var toneCurveStale = false
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
    /// The films the menus and the tray list (Settings › Fotufilm films).
    /// A hidden film in use stays listed, so a menu always shows its choice:
    /// the photograph's, and while the tray is open the one it opened on.
    var listedFilms: [Stock] { listedFilms(keeping: [film, tray?.before.film].compactMap { $0 }) }
    func listedFilms(keeping kept: [String]) -> [Stock] {
        films.filter { fotufilmFilms.lists($0) || kept.contains($0.id) }
    }
    var filmGroups: [StockGroup] { StockGroup.films(listedFilms, papers: papers) }
    var paperGroups: [StockGroup] { StockGroup.papers(papers) }
    var filmName: String { films.first { $0.id == film }?.name ?? film }
    var paperName: String { papers.first { $0.id == paper }?.name ?? paper }
    var canSave: Bool { previewCurrent && image != nil && !rendering && !saving && !cropping }
    var cropping: Bool { cropSession != nil }
    var canShowOriginal: Bool { image != nil && !cropping }
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
    /// Where photographs' sidecars are read and written; nil, none are.
    @ObservationIgnored let sidecars: SidecarStore?
    /// The open photograph's folder, with its photographs' ratings.
    @ObservationIgnored let library: PhotoLibrary
    /// The open photograph's sidecar; nil when it has none and gets none
    /// (sidecars off, no store).
    @ObservationIgnored private var photoSidecar: PhotoSidecarContext?
    /// The write 1 s after the last edit.
    @ObservationIgnored private var sidecarWrite: Task<Void, Never>?
    /// Folders whose sidecars couldn't be written, said once each.
    @ObservationIgnored private var unwritableFolders: Set<URL> = []
    /// The edits of the last photograph left with edits, for Paste from Previous.
    @ObservationIgnored private(set) var previousEdits: EditState?
    /// What Copy Edits took, for Paste Edits.
    private(set) var copiedEdits: CopiedEdits?
    /// The undo target of changes to several photographs' sidecars, so that
    /// opening another photograph, which clears the per-photograph undo,
    /// keeps them.
    @ObservationIgnored private let libraryUndo = LibraryUndoTarget()

    private struct PhotoSidecarContext {
        var photo: URL
        var variant: String
        var defaultsVersion: Int
        /// `variant` was made up when it opened: the file's first variant
        /// is written, should another app have made one since.
        var variantIsNew: Bool
        /// The user's edits as the photograph opened (`userEdits`). One without
        /// saved edits gets them only once they change.
        var opened: EditState
        var editedAtOpen: Bool
        var exists: Bool
        /// The stored edits last read or sent; the same edits aren't written again.
        var written: JSONValue?
        /// Why it isn't written: unreadable, from a newer Negs, unwritable.
        var blocked: String?
        /// Its edits are read again once the catalog arrives; until then
        /// they aren't written. Ratings and flags are.
        var awaitingCatalog = false
        /// The film and paper it names that this version lacks, with what is
        /// shown instead; written back while that stays chosen.
        var missingFilm: (saved: String, shown: String)?
        var missingPaper: (saved: String, shown: String)?
        /// A schema 2 session's edits, written once launch has settled.
        var migrating = false
    }

    init(previewCache: RenderedPreviewCache = RenderedPreviewCache(), presetLibrary: PresetLibrary = PresetLibrary(),
         sidecars: SidecarStore? = SidecarStore.forApp) {
        self.previewCache = previewCache
        self.presetLibrary = presetLibrary
        self.sidecars = sidecars
        let enabled = UserDefaults.standard.object(forKey: "sidecars") as? Bool ?? true
        library = PhotoLibrary(store: enabled ? sidecars : nil)
        recentFiles = (UserDefaults.standard.stringArray(forKey: "recentFiles") ?? []).map { URL(fileURLWithPath: $0) }
        library.sidecarChanged = { [weak self] photo in self?.sidecarChangedOnDisk(photo) }
        library.filterChanged = { [weak self] in self?.filterChanged() }
        library.looksChanged = { [weak self] in
            guard let self else { return }
            self.pumpThumbnails()
            self.aspects.read(self.library.items, around: self.sourceItem)
        }
        // Posted on any thread; delivered on the main queue.
        powerStateObserver = NotificationCenter.default.addObserver(forName: .NSProcessInfoPowerStateDidChange, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.lowPowerMode = ProcessInfo.processInfo.isLowPowerModeEnabled }
        }
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
            filmCacheRelease.watchMemoryPressure()
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
        // Nothing of the undone restore is written to its sidecar.
        // A different catalog can arrive once the window is shown and the
        // restored photograph is being cropped, compared or edited.
        forgetPhotograph()
        state = withPreferences(before.state); inspectorVisible = before.inspectorVisible; fitImage = before.fitImage; status = before.status
        if recentFiles != before.recentFiles { recentFiles = before.recentFiles; saveRecents() }
        updateHistogram()
    }

    /// No photograph open, and nothing of the last one written to its sidecar.
    private func forgetPhotograph() {
        endTray(keep: true)
        trialImage = nil
        photoSidecar = nil; sidecarWrite?.cancel(); sidecarWrite = nil
        sourceGeneration += 1
        photoFacts = nil
        source = nil; sourceVariant = nil; image = nil; placeholder = nil; standIn = nil; previewCurrent = false; developFailure = nil
        imageSize = .zero; placeholderSize = .zero; sourceSize = .zero; workingSize = .zero; previewScale = 1
        showingCachedPreview = false; isRaw = false; inputColour = .null; lensCorrectionSupported = false
        interactionStart = nil; cropSession = nil
        original = nil; originalCache = nil; originalLoad?.cancel(); originalLoad = nil; holdingOriginal = false
        imageOnly = false; upsideDown = false
        undoManager?.removeAllActions(withTarget: self)
    }

    func start(paths: EnginePaths? = nil, restoration: Task<EditorSession?, Error>? = nil) async {
        launch(paths: paths)
        let matches = matchCache
        Task.detached(priority: .utility) { matches.preload() }
        guard let request = catalogRequest, let engine else { return }
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
                // A photograph opened before this catalog: its edits under it.
                readoptSidecarEdits()
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
            if source == nil, status == "Starting…" { status = "" }
            if source != nil { changed(invalidateCache: false) }
            // A schema 2 session's edits move to the photograph's sidecar now,
            // off the launch path.
            if photoSidecar?.migrating == true { flushSidecar() }
        } catch { if !closing { report(error) } }
    }

    private func apply(catalog: JSONValue) {
        // The reset below is no edit: a write still waiting must not save it.
        sidecarWrite?.cancel(); sidecarWrite = nil
        self.catalog = catalog
        stocks = catalog["profiles"].array.map(Stock.init)
        colorFilters = catalog["color_filters"].array.map(ColorFilter.init)
        defaults = catalog["defaults"]
        NegativeScan.defaults = Dictionary(uniqueKeysWithValues: NegativeScan.ownPaths.map { ($0, defaults[$0]) })
        // The engine leaves lens correction off for params from before it, so
        // their crops keep their place; the app corrects every new edit.
        defaults[Self.lensCorrectionPath] = .bool(true)
        // Keep anything already applied to the photograph, such as its rotation.
        let rotation = state.params["io.rotate_cw"], flip = state.params["io.flip"]
        var next = state
        next.params = defaults
        if rotation != .null { next.params["io.rotate_cw"] = rotation }
        if flip != .null { next.params["io.flip"] = flip }
        if !films.contains(where: { $0.id == next.film }), let first = films.first { next.film = first.id }
        if !papers.contains(where: { $0.id == next.paper }), let first = papers.first { next.paper = first.id }
        PreviewRules.pairFilm(in: &next, stocks: stocks)
        state = next
        previews.update(stocks: stocks)
        scheduleToneCurve()
        refreshUneditedLook()
    }

    /// The look of every item without edits: what a new photograph opens
    /// with, so a new default film or paper, match setting or engine defaults
    /// make their thumbnails stale. Nil before the catalog.
    private(set) var uneditedLook: String?

    private func refreshUneditedLook() {
        let look = hasCatalog ? Self.uneditedLook(of: plainState) : nil
        guard look != uneditedLook else { return }
        uneditedLook = look
        pumpThumbnails()
    }

    /// A hash of `state` as an unedited photograph's thumbnail develops it.
    /// Image Size and the output colour, which follow the previous
    /// photograph, don't change a thumbnail and are left out.
    nonisolated static func uneditedLook(of state: EditState) -> String? {
        var plain = userEdits(state)
        for path in [ImageSize.targetPath, "io.output_color_space", "io.output_cctf_encoding"] { plain.params[path] = .null }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(["unedited": plain]) else { return nil }
        return SHA256.hash(data: data).prefix(16).map { String(format: "%02x", $0) }.joined()
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
        if border != state.border { border = state.border }
        if crop != state.crop { crop = state.crop; updateHistogram() }
        let rotation = Int(state.params["io.rotate_cw"].double), flip = state.params["io.flip"].bool
        if imageRotation != rotation || imageFlip != flip {
            imageRotation = rotation; imageFlip = flip
            updateHistogram()
            cropSession?.update(photo: shownImageSize, rotation: rotation, flip: flip)
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
    /// A control's ↺. On a negative, its base and balance are found again.
    func resetValue(_ path: String) {
        let before = state, since = negativeRequest
        set(path, defaults[path])
        refindNegative(after: before, paths: [path], since: since)
    }
    func number(_ path: String, range: ClosedRange<Double>, step: Double) -> Binding<Double> {
        Binding(get: { self.value(path).double }, set: { self.set(path, .number(Self.snap($0, to: range, step: step))) })
    }
    /// The enlarger's magenta or yellow shift, moving the other as Shifts
    /// move says; both change in one step. Reset stays per slider.
    func filterShift(_ path: String, step: Double) -> Binding<Double> {
        Binding(get: { self.value(path).double }, set: { newValue in
            let snapped = Self.snap(newValue, to: FilterLink.range, step: step)
            guard self.filterLink != .separate else { return self.set(path, .number(snapped)) }
            let (m, y) = self.filterLink.shifts(moving: path, to: snapped,
                                                magenta: self.value(FilterLink.magenta).double, yellow: self.value(FilterLink.yellow).double)
            var next = self.state
            next.params[FilterLink.magenta] = .number(m)
            next.params[FilterLink.yellow] = .number(y)
            self.apply(next, action: "Change Adjustment")
        })
    }
    /// The last drag or click that changed the edit: before and after.
    @ObservationIgnored private var lastInteraction: (start: EditState, end: EditState)?
    /// Resets a shift; linked (Together, Opposite ways), both reset, as
    /// they move, in one step.
    func resetFilterShift(_ path: String) {
        guard filterLink != .separate else { return resetValue(path) }
        var next = state
        for shift in [FilterLink.magenta, FilterLink.yellow] { next.params[shift] = defaults[shift] }
        let before = state, since = negativeRequest
        apply(next, action: "Change Adjustment")
        refindNegative(after: before, paths: [FilterLink.magenta, FilterLink.yellow], since: since)
    }
    /// A double-click on a shift's track. Its first click moved the shift to
    /// the pointer, and a linked one with it: that click is undone first,
    /// exactly, so the reset is one step from before it.
    func resetFilterShiftAfterClick(_ path: String) {
        if let last = lastInteraction, last.end == state, undoManager?.canUndo == true {
            undoManager?.undo()
        }
        lastInteraction = nil
        resetFilterShift(path)
    }
    /// Grain arrays (`film_render.grain.<field>`) are null until edited: the
    /// engine then uses the film's preset, which the catalog resolves.
    var filmIsBW: Bool { films.first { $0.id == film }?.bw ?? false }
    /// The camera colour filter in front of the lens, or `ColorFilter.none`.
    var colorFilter: String { value(ColorFilter.path).string }
    /// The filter the Color Filter tick box puts back once it took it off.
    @ObservationIgnored var lastColorFilter = "hoya_y2"
    func setColorFilter(_ key: String) {
        if key != ColorFilter.none { lastColorFilter = key }
        var next = state; next.params[ColorFilter.path] = .string(key)
        apply(next, action: key == ColorFilter.none ? "Remove Color Filter" : "Change Color Filter")
    }
    /// How many stops `key` takes on the current film, if it's a B&W film.
    func colorFilterStops(_ key: String) -> Double? { films.first { $0.id == film }?.filterStops[key] }
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
        lastInteraction = start != state ? (start, state) : nil
        if start != state { registerUndo(restoring: start, action: action) }
        // The drag frames were canvas-size already: letting go settles.
        quickStep = FrameID(revision: revision, generation: sourceGeneration)
        if showingInteractiveFrame, ready, source != nil, !closing { scheduleRender() }
        // What waited for the release goes on: a match the drag made stale
        // (its frames were settled ones where the canvas shows every pixel),
        // or the render after a match that finished during the drag.
        reconcileBrightnessMatch()
        if matchTask == nil { settleQuickFrame(); renderAfterMatch() }
        if !previewCurrent { scheduleActivity() }
        pumpPreviews()
    }

    func chooseFilm(_ stock: String) {
        apply(PreviewRules.choosing(stock, paper: false, keepPaper: previews.keepPaper, in: state, stocks: stocks), action: "Change Film")
    }
    func choosePaper(_ stock: String) {
        apply(PreviewRules.choosing(stock, paper: true, keepPaper: previews.keepPaper, in: state, stocks: stocks), action: "Change Paper")
    }

    // MARK: The tray (§4.5, §5.1)

    /// Opens the film or paper tray on the current stock, keeping whatever
    /// the other tray was trying.
    func openTray(paper: Bool, readAs: Bool = false) {
        guard previews.enabled, source != nil, !cropping, mode == .photo else { return }
        if let tray {
            if tray.paper == paper, tray.readAs == readAs { return }
            endTray(keep: true)
        }
        // A slide is scanned: there's no paper to choose.
        if paper, state.params["io.scan_film"].bool { return }
        negativePick = nil
        let films = !paper && !readAs
        tray = TrayVisit(paper: paper, before: state, readAs: readAs, opensOnScan: films && (isNegative || looksLikeANegative))
        if films { checkLooksNegative() }
        if paper { previews.paperOpen = true } else { previews.filmOpen = true }
        pumpPreviews()
    }
    /// A click on a name: opens its tray, or closes it keeping the stock.
    func toggleTray(paper: Bool, readAs: Bool = false) {
        if let tray, tray.paper == paper, tray.readAs == readAs { endTray(keep: true) } else { openTray(paper: paper, readAs: readAs) }
    }

    /// Tries a stock on the photograph; the tray stays open. Tries aren't
    /// undo steps: keeping one is (`endTray`).
    func tryStock(_ id: String) {
        guard let tray, let source else { return }
        if id == NegativeScan.stockID, !tray.paper, !tray.readAs {
            // Your negative: read as a negative; the base is found as part of the try.
            if !isNegative { replaceState(negativeState(from: state)) }
            return
        }
        // Only the tray's own kind: a film is never printed on as a paper.
        guard stocks.contains(where: { $0.paper == tray.paper && $0.id == id }) else { return }
        if tray.readAs, let stock = stocks.first(where: { $0.id == id }), !NegativeScan.readable(stock) { return }
        let next = PreviewRules.choosing(id, paper: tray.paper, keepPaper: previews.keepPaper, in: state, stocks: stocks, negative: tray.readAs)
        guard next != state else { return }
        replaceState(next)
        // The tile shows exactly this edit if it was made from the request
        // it would be made from now.
        guard !previewCurrent, let stock = stocks.first(where: { $0.paper == tray.paper && $0.id == id }),
              let tile = previews.tile(id, paper: tray.paper), let image = tile.image else { return }
        let wanted = PreviewTile.Key(generation: sourceGeneration, request: PreviewRules.request(
            for: stock, keepPaper: previews.keepPaper, state: renderState, stocks: stocks, source: source, generation: sourceGeneration,
            negative: tray.readAs, negativePoint: negativeTilePoint))
        if tile.shown == wanted { trialImage = image }
    }

    /// ← →: the previous or next stock, in the tray's order, through the
    /// favourites and on into the groups; among the matches while searching.
    func stepTray(by offset: Int) {
        guard let tray else { return }
        let sections = StockTray.sections(model: self, visit: tray)
        guard let place = StockTrayRules.step(from: tray.paper ? paper : film, inFavourites: tray.inFavourites, by: offset,
                                              in: StockTrayRules.places(sections)) else { return }
        tray.inFavourites = place.favourite
        tryStock(place.stock)
    }

    /// Return, Done or a click outside keeps the stock being tried, as one
    /// undo step from the stock before the tray opened; Esc puts that one
    /// back (§5.1). Either closes the tray. Undo while it's open keeps first
    /// (`EditorWindowController`), so it undoes the visit.
    func endTray(keep: Bool) {
        guard let tray else { return }
        self.tray = nil
        previews.filmOpen = false; previews.paperOpen = false
        let before = withPreferences(tray.before)
        let cropPending = negativeCropPending == sourceGeneration
        if cropPending { negativeCropPending = nil }
        if keep {
            // Your negative, kept from the tray: now its crop to the picture,
            // within the tray's undo step (the visit is one step), so the
            // step is named for reading the negative, undo and redo alike.
            if cropPending, let cropped = croppedToFoundPicture() { replaceState(cropped) }
            let read = isNegative && tray.before.params["workflow.route"].string != NegativeScan.route
            if state != before { registerUndo(restoring: tray.before, action: read ? "Read as Negative" : tray.action) }
        } else if state != before {
            replaceState(tray.before)
        }
        pumpPreviews()
    }
    /// The state with the app's preferences that travel in it.
    private func withPreferences(_ state: EditState) -> EditState {
        var state = state; state.limitPreview = limitsPreview; return state
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
        var next = state; next.params = defaults
        next.brightnessMatch = autoPlaceNewImages ? BrightnessMatch() : nil
        next.params["io.rotate_cw"] = state.params["io.rotate_cw"]
        next.params["io.flip"] = state.params["io.flip"]
        next.params[ImageSize.targetPath] = state.params[ImageSize.targetPath]
        next.params["io.scan_film"] = .bool(films.first(where: { $0.id == next.film })?.positive ?? false)
        customFilmFormat = false
        // Like the film, reading a negative stays; its base and balance are found again.
        if isNegative { next = negativeState(from: next) }
        let before = state, since = negativeRequest
        apply(next, action: "Reset Adjustments")
        refindNegative(after: before, paths: nil, since: since)
    }
    /// Puts the controls `targets` name back to their defaults, in one undo
    /// step (the ↺ on a tab's list of edits, and Reset on the tab).
    func resetEdits(_ targets: [ResetTarget], action: String) {
        guard !cropping, !targets.isEmpty else { return }
        var next = state
        var lanes: [String: [Int]] = [:]
        var space = false
        // Exposure first: turning matching on after it sets Exposure to 0.
        if targets.contains(.exposure) { next.params["prep.exposure_ev"] = .number(matchedEV) }
        for target in targets {
            switch target {
            case .param(let path): next.params[path] = defaults[path]
            case .stock(let path, let index): lanes[path, default: []].append(index)
            case .exposure: break
            case .matchBrightness:
                if autoPlaceNewImages {
                    next.brightnessMatch = BrightnessMatch()
                    next.params["prep.exposure_ev"] = .number(0)
                } else {
                    next.brightnessMatch = nil
                }
            case .scanFilm: next.params["io.scan_film"] = .bool(films.first(where: { $0.id == next.film })?.positive ?? false)
            case .previewSpace: space = true
            case .border: next.border = BorderSettings()
            case .filmFormat:
                next.params[FilmFormatPicker.path] = defaults[FilmFormatPicker.path]
                customFilmFormat = false
            }
        }
        for (path, indices) in lanes {
            var values = stockValues(path)
            for index in indices where values.indices.contains(index) { values[index] = stockDefault(path, index) }
            let preset = stockDefaults(path)
            let atPreset = values.count == preset.count && zip(values, preset).allSatisfy { abs($0 - $1) <= 1e-6 * max(1, abs($1)) }
            next.params[path] = atPreset ? .null : .array(values.map(JSONValue.number))
        }
        let before = state, since = negativeRequest
        apply(next, action: action)
        // On a negative, a reset of its base or balance finds them again.
        refindNegative(after: before, paths: targets.compactMap { if case .param(let path) = $0 { path } else { nil } }, since: since)
        if space { choosePreviewSpace(SaveSettings.colorSpaces[0]) }
    }
    /// Reset on one tab: every control the tab's count stands for.
    func resetEdits(tab: InspectorTab) {
        resetEdits(InspectorEdits.edits(tab, self).map(\.target), action: "Reset \(tab.rawValue)")
    }
    /// A lane of a grain array, the film's preset while it is untouched.
    func stockValue(_ path: String, _ index: Int) -> Double {
        let values = stockValues(path)
        return values.indices.contains(index) ? values[index] : 0
    }
    private func apply(_ next: EditState, action: String) {
        guard next != state else { return }
        // Any other edit keeps the stock being tried and closes the tray, so
        // the visit stays one undo step before it.
        endTray(keep: true)
        if !interacting { registerUndo(restoring: state, action: action) }
        replaceState(next)
    }
    /// A border, crop, or rotation change only redraws the canvas; anything
    /// else re-renders.
    private func replaceState(_ next: EditState) {
        guard !closing else { return }
        let next = withPreferences(next)
        let render = next.rendered != state.rendered
        // The enlarged tile was of the edit before.
        if render { trialImage = nil }
        let old = state
        state = next
        // A crop doesn't re-render the preview, but it changes the thumbnails.
        if render { changed() } else { pumpPreviews() }
        followNegative(from: old)
        reconcileBrightnessMatch()
        scheduleSidecarWrite()
    }
    private func registerUndo(restoring previous: EditState, action: String) {
        undoManager?.registerUndo(withTarget: self) { model in
            let previous = model.withPreferences(previous)
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

    /// Double-clicking the photograph, as in Lightroom: a zoomed one fits
    /// the window, a fitted one shows the grid. Cropping has its own.
    func doubleClickPhotograph(zoomed: Bool) {
        guard mode == .photo, !cropping else { return }
        if zoomed { zoom(fit: true) } else { showGrid() }
    }

    // MARK: Image Size

    /// Original develops the source's own pixels; Longest side resizes the
    /// source so the cropped photograph has that many pixels on its longest side.
    func chooseImageSize(_ mode: ImageSize.Mode) {
        switch mode {
        case .original:
            var next = state; next.params[ImageSize.targetPath] = .null
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

    /// The photograph and window to reopen at the next launch; the edits are
    /// in the photograph's sidecar (`flushSidecars`). A match solved for the
    /// current settings comes along: without edits nothing else keeps it.
    var session: EditorSession? {
        guard !closing, ready, let source else { return nil }
        var session = EditorSession(source: source, inspectorVisible: inspectorVisible, fitImage: fitImage)
        session.variant = sourceVariant
        if brightnessMatchActive, !brightnessMatchPending, !matchingBrightness {
            session.match = SessionMatch(state.brightnessMatch, file: source)
        }
        return session
    }

    private func restore(_ session: EditorSession) throws {
        try session.validate()
        if let state = session.state {
            guard films.contains(where: { $0.id == state.film }), papers.contains(where: { $0.id == state.paper }) else {
                throw EngineError(message: "The previous session uses an unavailable film or paper. Open a photograph to continue.")
            }
        }
        open(session.source, restoring: session)
    }

    // MARK: Cropping

    /// Shows the whole developed photograph, without the border, to crop it.
    func beginCrop() {
        guard !cropping, image != nil, imageSize != .zero, comparison == nil else { return }
        holdOriginal(false)
        clearSplit()
        holdUpsideDown(false)
        // Its tools are in the inspector and the status line.
        leaveImageOnly()
        // The straighten dial takes the label's place.
        endTray(keep: true)
        cropSession = CropSession(start: state, photo: shownImageSize, rotation: imageRotation, flip: imageFlip, source: sourceSize)
    }
    /// A quarter turn, which happens only while cropping (§4.10): from the
    /// menu, or ⌘R / ⌥⌘R, it starts cropping first. Done keeps it with the
    /// crop as one undo step; Cancel turns it back.
    func turn(clockwise: Bool) {
        if !cropping { beginCrop() }
        guard cropping else { return }
        let turns = (Int(value("io.rotate_cw").double) + (clockwise ? 1 : 3)) % 4
        var next = state; next.params["io.rotate_cw"] = .number(Double(turns)); replaceState(next)
    }
    /// Flip: mirrors the photograph left to right as shown, as a negative
    /// photographed from its base side needs. Like a quarter turn it
    /// happens while cropping; Done keeps it, Cancel takes it back.
    func flip() {
        if !cropping { beginCrop() }
        guard cropping else { return }
        let flipped = Orientation(params: state.params).flippedAsShown
        var next = state
        next.params["io.rotate_cw"] = .number(Double(flipped.turns)); next.params["io.flip"] = .bool(flipped.flip)
        replaceState(next)
    }

    /// Done applies the crop, and any rotation while cropping, as one undo
    /// step. Cancel restores the rotation and keeps the previous crop.
    func endCrop(apply: Bool) {
        guard let session = cropSession else { return }
        // A crop too small for the Longest side stays a draft, with its error shown.
        if apply, session.sizeError != nil { NSSound.beep(); return }
        cropSession = nil
        let rotation = session.start.params["io.rotate_cw"], flip = session.start.params["io.flip"]
        if apply {
            var previous = state; previous.crop = session.start.crop
            previous.params["io.rotate_cw"] = rotation; previous.params["io.flip"] = flip
            var next = state; next.crop = session.settings
            if next != previous { registerUndo(restoring: previous, action: "Crop") }
            replaceState(next)
        } else if state.params["io.rotate_cw"] != rotation || state.params["io.flip"] != flip {
            var next = state; next.params["io.rotate_cw"] = rotation; next.params["io.flip"] = flip; replaceState(next)
        }
        // What waited for the crop goes on, Done or Cancel: a match an edit
        // made stale meanwhile, or the render after one.
        reconcileBrightnessMatch()
        if matchTask == nil { renderAfterMatch() }
        zoom(fit: true)
    }

    // MARK: Opening

    nonisolated static let rawExtensions = ["dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "ptx", "srw", "x3f", "iiq", "3fr", "ari", "bay", "crw", "dcr", "drf", "erf", "fff", "k25", "kdc", "mef", "mos", "mrw", "rwl"]

    func openPanel() {
        let panel = Self.makeOpenPanel()
        if let source {
            // A file URL reveals and selects the file; a parent URL only opens its folder.
            panel.directoryURL = source
        }
        panel.begin { [weak self] response in
            if response == .OK, let url = panel.url { self?.open(url) }
        }
    }

    /// ⌘O's panel: a photograph, or a folder of them (§6.1).
    static func makeOpenPanel() -> NSOpenPanel {
        let panel = NSOpenPanel()
        panel.title = "Open"
        panel.allowedContentTypes = [.png, .tiff, .jpeg, .rawImage] + Self.rawExtensions.compactMap { UTType(filenameExtension: $0) }
        panel.canChooseFiles = true; panel.canChooseDirectories = true; panel.allowsMultipleSelection = false
        return panel
    }

    /// A folder's first photograph in name order, or nil if it has none
    /// (or isn't a folder Negs can read).
    nonisolated static func firstPhotograph(in folder: URL) -> URL? {
        (try? PhotoFolder.images(in: folder))?.first
    }

    func open(_ url: URL) {
        // A folder opens on its first photograph, in the grid.
        if (try? url.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true {
            guard let first = Self.firstPhotograph(in: url) else {
                error = "“\(url.lastPathComponent)” has no photographs Negs can open."
                return
            }
            open(first)
            if source == first { showGrid() }
            return
        }
        // A RAW's camera JPEG opens the RAW: the two are one photograph.
        if let raw = PhotoFolder.pairedRAW(of: url) { return open(raw) }
        explicitOpenRequested = true
        if comparison != nil { leaveCompare(toGrid: false) }
        mode = .photo
        // During launch it undoes a restore at launch; later it supersedes it.
        if launching { discardLaunchRestore() } else { launchRestore = nil }
        open(url, restoring: nil)
    }

    /// Opens a library item: a photograph, or a duplicate of one.
    private func open(_ item: URL, restoring session: EditorSession?) {
        let item = session?.variant.map { item.photoFile.variant($0) } ?? item
        let url = item.photoFile
        guard !closing, url.isFileURL else { return }
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            recentFiles.removeAll { $0 == url }; saveRecents()
            error = "“\(url.lastPathComponent)” can’t be opened. It may have been moved or deleted."
            return
        }
        // A stock being tried is kept, as by a click outside the tray.
        endTray(keep: true)
        trialImage = nil
        // The previous photograph's last edits go to its sidecar first, and
        // the preview on screen becomes its edited thumbnail.
        flushSidecar()
        saveOpenThumbnail()
        saveOpenRender()
        // Paste from Previous: the last photograph left with edits.
        if photoSidecar?.written != nil, photoSidecar?.photo == source { previousEdits = Self.userEdits(state) }
        let previous = state
        cancelBrightnessMatch(); brightnessMatchFailure = nil
        sourceDeleted = false
        source = url; sourceVariant = item.variantID; image = nil; placeholder = nil; standIn = nil; previewCurrent = false; developFailure = nil
        #if DEBUG
        shownFrame = nil
        #endif
        // Leaving the grid opens its current photograph and keeps the selection.
        if selection.current != item { selection.select(item) }
        sourceSize = .zero; workingSize = .zero; previewScale = 1
        showingCachedPreview = false
        updateHistogram()
        interactionStart = nil; cropSession = nil
        original = nil; originalCache = nil; originalLoad?.cancel(); originalLoad = nil; holdingOriginal = false
        splitting = false; splitOriginal = nil
        sourceGeneration += 1
        let generation = sourceGeneration
        customFilmFormat = false
        photoFacts = nil
        // The last photograph's film base said nothing about this one.
        negativeBaseRecords = []; negativeSurveys = []; negativePick = nil
        Task.detached(priority: .utility) { [weak self] in
            let facts = PhotoFacts.read(url)
            await self?.adopt(facts, generation: generation)
        }
        // A schema 2 session restores its edits as before; otherwise the
        // photograph's sidecar has them, read at launch with the session.
        adoptEdits(for: url, restore: session?.sidecar ?? readSidecar(url, variant: item.variantID), legacy: session?.state, previous: previous,
                   match: session?.match)
        if let session { inspectorVisible = session.inspectorVisible }
        isRaw = Self.rawExtensions.contains(url.pathExtension.lowercased())
        // Changes to several photographs (Paste Edits, Duplicate) stay undoable.
        undoManager?.removeAllActions(withTarget: self)
        zoom(fit: session?.fitImage ?? true)
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
        let crop = state.crop, rotation = Int(state.params["io.rotate_cw"].double), flip = state.params["io.flip"].bool
        let cachedFrame = session != nil ? paths.map { paths in
            LaunchFrame {
                guard let frame = await cache.load(source: url, state: snapshot, paths: paths) else { return nil }
                return LaunchFrame.Loaded(frame: frame, scopes: ImageScopes.of(frame.surface, crop: crop, rotation: rotation, flip: flip),
                                         scopesCrop: crop, scopesRotation: rotation, scopesFlip: flip)
            }
        } : nil
        launchFrame = restoringAtLaunch ? cachedFrame.map { ($0, generation, snapshot.rendered) } : nil
        // The developed thumbnail stands in until then, if it's been made.
        // Read now (~2 ms), as the window's next frame comes only after
        // SwiftUI has updated it for this photograph (~180 ms on an M1 Pro):
        // that frame shows it rather than an empty canvas. A restored
        // session tries its saved frame first, off the main actor.
        let standIn = developedThumbnail(of: item), kept = keptRender(of: item)
        if session == nil, let image = standIn?() { self.standIn = image }
        Task { [weak self] in
            if let cachedFrame, let cached = await cachedFrame.value {
                guard let self else { return }
                if self.showCached(cached, generation: generation, rendered: snapshot.rendered) { return }
            }
            if session != nil, let standIn, let image = await Task.detached(priority: .userInitiated, operation: standIn).value {
                self?.showStandIn(image, generation: generation)
            }
            // Its last develop, if it was kept: sharper, and laid out as the
            // develop will be, so the camera's preview isn't needed.
            if let kept, let (image, size) = await Task.detached(priority: .userInitiated, operation: kept).value,
               self?.showKept(image, size: size, generation: generation) == true { return }
            // Still read for the photograph's size, which lays out the canvas.
            let thumbnail = await Task.detached(priority: .userInitiated) { Self.thumbnail(url) }.value
            if let (thumbnail, size) = thumbnail { self?.showPlaceholder(thumbnail, size: size, generation: generation) }
        }
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
        showingCachedPreview = true; placeholder = nil; standIn = nil; hideActivity()
        #if DEBUG
        shownFrame = (FrameID(revision: revision, generation: generation), nil)
        #endif
        // Counted with the frame for the crop it was read with.
        if crop == cached.scopesCrop, imageRotation == cached.scopesRotation, imageFlip == cached.scopesFlip {
            histogramRequest += 1; histogram = cached.scopes.histogram; waveform = cached.scopes.waveform
        } else { updateHistogram() }
        updateFilmStrip()
        return true
    }

    /// Reads `item`'s developed thumbnail, if one was made for the edits it
    /// opens with.
    func developedThumbnail(of item: URL) -> (@Sendable () -> CGImage?)? {
        guard let edited = editedThumbnail(for: item), let file = library.items.first(where: { $0.url == item }),
              let key = edited.store.key(for: file, look: edited.look) else { return nil }
        let store = edited.store, look = edited.look
        return {
            store.image(key, maxPixels: Int(EditedThumbnailStore.longSide))
                ?? store.previousKey(for: file, look: look).flatMap { store.image($0, maxPixels: Int(EditedThumbnailStore.longSide)) }
        }
    }

    /// Reads `item`'s last develop, if one was kept for the edits it opens with.
    func keptRender(of item: URL) -> (@Sendable () -> (CGImage, CGSize)?)? {
        guard let look = thumbnailLook(for: item), let file = library.items.first(where: { $0.url == item }),
              let key = renderArchive.key(for: file, look: look) else { return nil }
        let archive = renderArchive
        return { (archive.render(key) ?? archive.previousKey(for: file, look: look).flatMap { archive.render($0) }).map { ($0.image, $0.photoSize) } }
    }

    /// True if it's shown, or no longer needed.
    private func showKept(_ render: CGImage, size: CGSize, generation: Int) -> Bool {
        guard generation == sourceGeneration, image == nil else { return true }
        placeholderSize = size; placeholder = render; standIn = nil; placeholderFor = (generation, true)
        updateFilmStrip()
        return true
    }

    private func adopt(_ facts: PhotoFacts?, generation: Int) {
        if generation == sourceGeneration { photoFacts = facts }
    }

    private func showStandIn(_ thumbnail: CGImage, generation: Int) {
        guard generation == sourceGeneration, image == nil else { return }
        standIn = thumbnail
    }

    private func showPlaceholder(_ thumbnail: CGImage, size: CGSize, generation: Int) {
        guard generation == sourceGeneration, image == nil else { return }
        placeholderSize = size; placeholder = thumbnail; placeholderFor = (generation, false)
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

    // MARK: Edits per photograph

    /// Settings › New photographs › Film: what a photograph without edits
    /// opens with. Until one is chosen, or if this version lacks the one
    /// chosen, the newest defaults table's.
    var newPhotoFilm: String { newPhotoStock(chosenNewPhotoFilm, table: \.film, in: films, fallback: EditState().film) }
    /// Settings › New photographs › Paper, as `newPhotoFilm`.
    var newPhotoPaper: String { newPhotoStock(chosenNewPhotoPaper, table: \.paper, in: papers, fallback: EditState().paper) }

    private func newPhotoStock(_ chosen: String?, table: KeyPath<SidecarDefaults, String>, in stocks: [Stock], fallback: String) -> String {
        let tabled = engine.flatMap { try? SidecarDefaults.load(version: SidecarDefaults.latest, data: $0.paths.data)[keyPath: table] }
        let candidates = [chosen, tabled].compactMap { $0 }
        // Before the catalog, nothing can be checked.
        guard !stocks.isEmpty else { return candidates.first ?? fallback }
        return candidates.first { id in stocks.contains { $0.id == id } } ?? stocks.first?.id ?? fallback
    }

    /// Chooses the film new photographs open with. Its own paper comes with
    /// it unless Settings › Keep paper when changing film, as in the inspector.
    func setNewPhotoFilm(_ film: String) {
        var choice = EditState(); choice.film = newPhotoFilm; choice.paper = newPhotoPaper
        let chosen = PreviewRules.choosing(film, paper: false, keepPaper: previews.keepPaper, in: choice, stocks: stocks)
        setNewPhotoStocks(film: chosen.film, paper: chosen.paper)
    }
    func setNewPhotoPaper(_ paper: String) { setNewPhotoStocks(film: newPhotoFilm, paper: paper) }

    private func setNewPhotoStocks(film: String, paper: String) {
        let before = (newPhotoFilm, newPhotoPaper)
        chosenNewPhotoFilm = film; chosenNewPhotoPaper = paper
        UserDefaults.standard.set(film, forKey: Self.newPhotoFilmKey)
        UserDefaults.standard.set(paper, forKey: Self.newPhotoPaperKey)
        if before != (newPhotoFilm, newPhotoPaper) { followNewPhotoStocks() }
        refreshUneditedLook()
    }

    /// The open photograph takes a new default film and paper while it has no
    /// edits of its own, so it shows what its thumbnail does. Not an edit: no
    /// undo step, and nothing is written beside it. Edited since it opened,
    /// it keeps its film and paper, but what it would be without edits takes
    /// the new ones: undoing every edit then leaves it edited, its film kept
    /// in its sidecar, rather than unedited showing another film.
    private func followNewPhotoStocks() {
        guard source != nil, !closing, var context = photoSidecar, context.photo == source,
              !context.editedAtOpen, context.blocked == nil else { return }
        guard Self.userEdits(state) == context.opened else {
            context.opened = withNewPhotoStocks(context.opened)
            photoSidecar = context
            return
        }
        guard !cropping, !interacting else { return }
        let next = withNewPhotoStocks(state)
        guard next != state else { return }
        replaceState(next)
        context.opened = Self.userEdits(state)
        photoSidecar = context
    }

    /// `edits` with the film and paper of Settings › New photographs.
    private func withNewPhotoStocks(_ edits: EditState) -> EditState {
        var next = edits
        next.film = newPhotoFilm; next.paper = newPhotoPaper
        next.params["io.scan_film"] = .bool(films.first(where: { $0.id == next.film })?.positive ?? false)
        return next
    }

    /// The state a photograph without saved edits opens with: the defaults,
    /// with the film and paper of Settings › New photographs. Image Size and
    /// the output colour follow the previous photograph, as a workflow's
    /// choices; its Prep, Lab, border, crop, rotation and how its file was
    /// read do not.
    private func newPhotoState(after previous: EditState) -> EditState {
        var next = previous
        let retainedPaths = [ImageSize.targetPath, "io.output_color_space", "io.output_cctf_encoding"]
        let retained = retainedPaths.map { ($0, previous.params[$0]) }
        next.film = newPhotoFilm; next.paper = newPhotoPaper
        next.params = defaults
        for (path, value) in retained where value != .null { next.params[path] = value }
        next.params["io.scan_film"] = .bool(films.first(where: { $0.id == next.film })?.positive ?? false)
        next.border = BorderSettings()
        next.params["io.rotate_cw"] = .number(0)
        next.params["io.flip"] = .bool(false)
        let interpretation = defaults["io.input_interpretation"]
        if interpretation != .null { next.params["io.input_interpretation"] = interpretation }
        next.params[Self.lensCorrectionPath] = defaults[Self.lensCorrectionPath]
        next.crop = CropSettings()
        next.brightnessMatch = autoPlaceNewImages ? BrightnessMatch() : nil
        next.roll = nil
        return withPreferences(next)
    }

    /// Saved edits under the engine's current defaults. `legacy`: a schema 2
    /// session's, which may predate lens correction and the preview limit.
    func restoredState(_ saved: EditState, legacy: Bool, matchSolved: Bool = false) -> EditState {
        var restored = saved
        let lensCorrection = restored.params[Self.lensCorrectionPath]
        restored.params = defaults.restoring(restored.params, nullable: Set(catalog["nullable_paths"].array.map(\.string)))
        // A session from before lens correction was developed without it.
        if legacy, lensCorrection == .null { restored.params[Self.lensCorrectionPath] = .bool(false) }
        // Sessions from before Image Size may carry the old working-resolution factor.
        let factor = defaults[ImageSize.factorPath]
        if factor != .null { restored.params[ImageSize.factorPath] = factor }
        // The inspector no longer edits Post tone or colour; an older session's
        // Post edits would otherwise stay applied with no control to undo them.
        if defaults["post"] != .null { restored.params["post"] = defaults["post"] }
        // Sessions from before it was a preference carried it per photograph.
        if legacy, UserDefaults.standard.object(forKey: "limitPreview") == nil, restored.limitPreview {
            adoptingPreference = true; limitsPreview = true; adoptingPreference = false
        }
        restored = withPreferences(restored)
        // A match stored as solved was solved for these very edits.
        if matchSolved, restored.brightnessMatch != nil {
            let inputs = matchInputs(restored)
            restored.brightnessMatch?.inputs = inputs
        }
        return restored
    }

    /// The user's own edits in `state`: without the brightness match's solved
    /// exposure, which isn't one, and the preview limit, a preference.
    nonisolated static func userEdits(_ state: EditState) -> EditState {
        var edits = state
        edits.limitPreview = false
        if let match = edits.brightnessMatch {
            let offset = edits.params["prep.exposure_ev"].double - (match.ev ?? 0)
            edits.params["prep.exposure_ev"] = .number((offset * 1e9).rounded() / 1e9)
            edits.brightnessMatch = BrightnessMatch()
        }
        return edits
    }

    /// `photo`'s sidecar, or nil when sidecars aren't used.
    private func readSidecar(_ photo: URL, variant: String?) -> SidecarRestore? {
        guard let sidecarLookup else { return nil }
        return SidecarRestore.read(photo: photo, variant: variant, store: sidecarLookup.store, data: sidecarLookup.data)
    }
    /// Where the launch reads the last photograph's sidecar; nil with sidecars off.
    var sidecarLookup: SidecarLookup? {
        guard sidecarsEnabled, let sidecars, let data = engine?.paths.data else { return nil }
        return SidecarLookup(store: sidecars, data: data)
    }

    /// Sets the state of the photograph being opened: its saved edits, a
    /// schema 2 session's, or a new photograph's, with the session's `match`
    /// if it was solved on this file for these settings. Then remembers its
    /// sidecar.
    private func adoptEdits(for url: URL, restore: SidecarRestore?, legacy: EditState?, previous: EditState, match: SessionMatch? = nil) {
        var missingFilm: (saved: String, shown: String)?, missingPaper: (saved: String, shown: String)?
        if let legacy {
            state = restoredState(legacy, legacy: true)
        } else if let saved = restore?.edits, restore?.problem == nil, hasCatalog {
            var restored = restoredState(saved.state, legacy: false, matchSolved: saved.matchSolved)
            // A film or paper this version lacks (another fork's, a newer
            // release's) falls back to the defaults table's; the photograph opens.
            let table = sidecarLookup.flatMap { try? SidecarDefaults.load(version: restore?.defaultsVersion ?? 1, data: $0.data) }
            if !films.contains(where: { $0.id == restored.film }), let shown = films.first(where: { $0.id == table?.film })?.id ?? films.first?.id {
                missingFilm = (restored.film, shown)
                restored.film = shown
                restored.params["io.scan_film"] = .bool(films.first { $0.id == shown }?.positive ?? false)
            }
            if !papers.contains(where: { $0.id == restored.paper }), let shown = papers.first(where: { $0.id == table?.paper })?.id ?? papers.first?.id {
                missingPaper = (restored.paper, shown)
                restored.paper = shown
            }
            if missingFilm != nil || missingPaper != nil {
                let names = [missingFilm.map { "the film “\($0.saved)”" }, missingPaper.map { "the paper “\($0.saved)”" }].compactMap { $0 }
                error = "\(url.lastPathComponent) was edited with \(names.joined(separator: " and ")), which this version of Negs doesn’t have. It shows \([missingFilm.map { _ in filmName(restored.film) }, missingPaper.map { _ in paperName(restored.paper) }].compactMap { $0 }.joined(separator: " and ")) instead, and keeps the saved choice unless you pick another."
            }
            state = restored
        } else {
            state = newPhotoState(after: previous)
            if let match, state.brightnessMatch != nil, matchInputs(state) == match.inputs, match.solved(on: url),
               let solved = Self.matching(state, ev: match.ev, limited: match.highlightLimited, inputs: match.inputs) {
                state = solved
            }
        }
        guard let restore else { photoSidecar = nil; return }
        library.opened(sourceItem ?? url, summary: LibrarySummary(rating: restore.rating, flag: restore.flag, edited: restore.edits != nil, problem: restore.problem))
        var context = sidecarContext(for: url, restore: restore)
        context.missingFilm = missingFilm; context.missingPaper = missingPaper
        if legacy != nil {
            // Written once launch settles, if it holds anything a new
            // photograph wouldn't have; it wins over a sidecar already there.
            context.opened = Self.userEdits(newPhotoState(after: state))
            context.written = nil
            context.migrating = true
        }
        // Before the catalog the edits can't be shown; `readoptSidecarEdits` reads them again.
        if !hasCatalog { context.awaitingCatalog = true }
        photoSidecar = context
        if let problem = restore.problem { error = problem }
    }

    private func filmName(_ id: String) -> String { films.first { $0.id == id }?.name ?? id }
    private func paperName(_ id: String) -> String { papers.first { $0.id == id }?.name ?? id }

    /// The open photograph's sidecar as it stands, with its current state as
    /// the one it opened with.
    private func sidecarContext(for url: URL, restore: SidecarRestore) -> PhotoSidecarContext {
        var context = PhotoSidecarContext(photo: url, variant: restore.variant, defaultsVersion: restore.defaultsVersion,
                                          variantIsNew: restore.newVariant, opened: Self.userEdits(state),
                                          editedAtOpen: restore.edits != nil, exists: restore.exists, blocked: restore.problem)
        if restore.edits != nil, context.blocked == nil { context.written = storedEdits(state, context) }
        return context
    }

    /// The catalog changed under an open photograph: shows its saved edits
    /// under the new defaults, or the new photograph state, as opening it does.
    private func readoptSidecarEdits() {
        guard let source, let context = photoSidecar, context.photo == source else { return }
        adoptEdits(for: source, restore: readSidecar(source, variant: sourceVariant), legacy: nil, previous: state)
    }

    /// `state` as its sidecar stores it.
    private func storedEdits(_ current: EditState, _ context: PhotoSidecarContext) -> JSONValue? {
        guard let data = sidecarLookup?.data, let table = try? SidecarDefaults.load(version: context.defaultsVersion, data: data) else { return nil }
        var saved = current
        if let missing = context.missingFilm, saved.film == missing.shown { saved.film = missing.saved }
        if let missing = context.missingPaper, saved.paper == missing.shown { saved.paper = missing.saved }
        let solved = current.brightnessMatch.map { $0.ev != nil && $0.inputs == matchInputs(current) } ?? false
        return PhotoSidecar.encodeEdits(saved, matchSolved: solved, table: table)
    }

    /// Writes the open photograph's sidecar 1 s after the last edit.
    private func scheduleSidecarWrite() {
        guard photoSidecar != nil, !closing else { return }
        sidecarWrite?.cancel()
        sidecarWrite = Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            self?.flushSidecar()
        }
    }

    /// The edits the open photograph's sidecar would be sent now: once the
    /// user has edited it, or always if it had edits (nil inside removes
    /// them). Nil when nothing changed.
    private func sidecarEditsToSend(_ context: PhotoSidecarContext) -> JSONValue?? {
        var current = state
        // Include a crop being drawn, without forcing the user to press Done.
        if let cropSession { current.crop = cropSession.settings }
        let edited = Self.userEdits(current) != context.opened
        let edits: JSONValue?
        if edited || context.editedAtOpen {
            guard let stored = storedEdits(current, context) else { return nil }
            edits = stored
        } else {
            edits = nil
        }
        return edits == context.written ? nil : .some(edits)
    }

    /// Sends the open photograph's edits to its sidecar now, if they changed.
    /// Back where it started, edits this visit saved are removed again.
    @discardableResult private func flushSidecar() -> Task<Void, Never>? {
        sidecarWrite?.cancel(); sidecarWrite = nil
        guard sidecarsEnabled, let sidecars, sidecars.writes, var context = photoSidecar, context.blocked == nil, !context.awaitingCatalog,
              context.photo == source, hasCatalog, !closing, let edits = sidecarEditsToSend(context) else { return nil }
        let photo = context.photo
        let known: [String: [JSONValue]]
        if let data = sidecarLookup?.data, let table = try? SidecarDefaults.load(version: context.defaultsVersion, data: data) {
            known = ["params": [table.params, defaults], "crop": [table.crop], "border": [table.border]]
        } else { return nil }
        sidecars.enqueue(SidecarPatch(variant: context.variant, primary: context.variantIsNew, edits: edits, known: known,
                                      defaultsVersion: context.defaultsVersion, app: Self.appVersion), for: photo)
        var summary = library.summary(for: photo)
        summary.edited = edits != nil
        library.set(summary, for: photo)
        // Its look too, which names its edited thumbnail.
        library.reload(photo)
        context.written = edits; context.migrating = false
        context.exists = edits != nil || summary.rating > 0 || summary.flag != nil
        photoSidecar = context
        return Task { [weak self] in
            let failures = await sidecars.flush()
            for (failed, reason) in failures { self?.sidecarFailed(failed, reason) }
        }
    }

    /// Writes the open photograph's last edits and everything still waiting,
    /// at quit.
    func flushSidecars() async {
        flushSidecar()
        if let failures = await sidecars?.flush() {
            for (failed, reason) in failures { sidecarFailed(failed, reason) }
        }
    }

    private func sidecarFailed(_ photo: URL, _ reason: String) {
        if photoSidecar?.photo.standardizedFileURL == photo.standardizedFileURL { photoSidecar?.blocked = reason }
        // The library showed the change as it was queued: back to what the file keeps.
        library.reload(library.files.first { $0.url.standardizedFileURL == photo.standardizedFileURL }?.url ?? photo)
        let folder = photo.deletingLastPathComponent().standardizedFileURL
        guard !closing, unwritableFolders.insert(folder).inserted else { return }
        error = "Edits to photographs in “\(folder.lastPathComponent)” can’t be saved beside them: \(reason)"
    }

    nonisolated static var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev"
    }

    /// Another app or Mac changed `photo`'s sidecar. For the open photograph
    /// a newer rating or flag shows at once (the library read it); newer
    /// edits replace these only if they're all saved, and otherwise these
    /// are written over them.
    func sidecarChangedOnDisk(_ photo: URL) {
        guard photo == source, !closing, var context = photoSidecar, context.photo == photo, context.blocked == nil,
              let lookup = sidecarLookup else { return }
        let restore = SidecarRestore.read(photo: photo, variant: sourceVariant, store: lookup.store, data: lookup.data)
        if let problem = restore.problem {
            // Never written over now; it opens with its edits shown as they are.
            photoSidecar?.blocked = problem
            return
        }
        context.exists = restore.exists
        if !restore.newVariant, context.variantIsNew { context.variant = restore.variant; context.variantIsNew = false }
        let table = try? SidecarDefaults.load(version: context.defaultsVersion, data: lookup.data)
        let ours = context.written.flatMap { written in table.flatMap { try? PhotoSidecar.decodeEdits(written, table: $0) } }
        guard restore.edits != ours || restore.defaultsVersion != context.defaultsVersion, !context.awaitingCatalog, hasCatalog,
              sidecarWrite == nil, !interacting, !cropping, sidecarEditsToSend(context) == nil else {
            photoSidecar = context
            return
        }
        cancelBrightnessMatch(); brightnessMatchFailure = nil
        customFilmFormat = false
        adoptEdits(for: photo, restore: restore, legacy: nil, previous: state)
        undoManager?.removeAllActions(withTarget: self)
        changed()
    }

    // MARK: Ratings and flags

    /// What a rating or flag key did, shown briefly over the canvas.
    struct LibraryNote: Equatable {
        var text: String
        /// Why it wasn't saved, if it wasn't.
        var detail: String?
        /// How many photographs it changed: several in the grid.
        var count = 1
        var serial: Int
    }

    /// The open photograph's rating and flag.
    var currentSummary: LibrarySummary? { sourceItem.map { library.summary(for: $0) } }

    /// What rating and flag commands act on: in the grid, every selected
    /// photograph it shows; in the edit view the current one only, so a
    /// forgotten selection in the strip never rates twenty photographs.
    var ratingTargets: [URL] {
        guard mode == .grid else { return sourceItem.map { [$0] } ?? [] }
        return selection.ordered(shownPhotos.map(\.url))
    }

    /// Rates what rating commands act on, 0 (none) to 5 stars.
    func setRating(_ stars: Int, targets: [URL]? = nil) {
        changeLibrary(action: "Rating", targets: targets) { $0.rating = max(0, min(5, stars)) } note: { Self.ratingText($0.rating) }
    }
    func changeRating(by offset: Int) {
        changeLibrary(action: "Rating", many: offset > 0 ? "Rating Raised" : "Rating Lowered") {
            $0.rating = max(0, min(5, $0.rating + offset))
        } note: { Self.ratingText($0.rating) }
    }
    func setFlag(_ flag: PhotoSidecar.Flag?, targets: [URL]? = nil) {
        changeLibrary(action: "Flag", targets: targets) { $0.flag = flag } note: { Self.flagText($0.flag) }
    }

    func hideLibraryNote(_ serial: Int) { if libraryNote?.serial == serial { libraryNote = nil } }

    nonisolated static func ratingText(_ stars: Int) -> String { stars == 0 ? "No Rating" : String(repeating: "★", count: stars) }
    nonisolated static func flagText(_ flag: PhotoSidecar.Flag?) -> String {
        switch flag { case .pick: "Pick"; case .reject: "Rejected"; case nil: "Unflagged" }
    }

    /// `many` is the note for several photographs, when `note` would differ
    /// between them.
    private func changeLibrary(action: String, many: String? = nil, targets: [URL]? = nil, _ change: (inout LibrarySummary) -> Void,
                               note: (LibrarySummary) -> String) {
        let targets = targets ?? ratingTargets
        guard !targets.isEmpty, !closing else { return }
        let order = shownPhotos.map(\.url), view = libraryView
        var unsaved: String?, last = LibrarySummary(), queued = false, befores: [URL: LibrarySummary] = [:]
        for photo in targets {
            let before = library.summary(for: photo)
            var after = before
            change(&after)
            last = after
            guard after != before else { continue }
            befores[photo] = before
            let reason = saveLibraryChange(from: before, to: after, photo: photo)
            if reason == nil { queued = true } else if unsaved == nil { unsaved = reason }
            library.set(after, for: photo)
        }
        if queued { flushLibraryChanges() }
        let text = targets.count == 1 ? note(last) : many ?? note(last)
        libraryNote = LibraryNote(text: text, detail: unsaved, count: targets.count, serial: (libraryNote?.serial ?? 0) + 1)
        moveOn(after: targets, order: order)
        registerLibraryUndo(befores, from: libraryView, to: view, action: action)
    }

    /// After rating or flagging `targets`: those the filter now leaves out
    /// go from the strip and the grid at once, the current photograph too.
    /// The next one shown becomes current (in the edit view it opens), or
    /// the one before at the end. With none left the grid shows that
    /// nothing matches, as it does for a filter nothing passes.
    private func moveOn(after targets: [URL], order: [URL]) {
        // Compare keeps its two on screen, and doesn't advance.
        guard comparison == nil else { return }
        let dropped = library.filter.active ? Set(targets.filter { !library.filter.includes(library.summary(for: $0)) }) : []
        if !dropped.isEmpty {
            let current = mode == .photo ? sourceItem : selection.current
            if let current, dropped.contains(current) {
                if let next = Self.next(after: current, in: order, skipping: dropped) {
                    if mode == .photo { open(next) } else { selection.select(next) }
                } else {
                    // The edit view has nothing to show, so the grid opens
                    // on its empty sheet; the open photograph stays loaded
                    // behind it for Undo or another filter to bring back.
                    if mode == .photo { showGrid() }
                    if mode == .grid { selection = LibrarySelection() }
                }
                return
            }
            selection.prune(to: Set(shownPhotos.map(\.url)))
        }
        guard advanceAfterRating else { return }
        if mode == .photo { step(by: 1) } else if targets.count == 1 { move(by: 1, extending: false) }
    }

    private func flushLibraryChanges() {
        guard let sidecars else { return }
        Task { [weak self] in
            let failures = await sidecars.flush()
            for (failed, reason) in failures { self?.sidecarFailed(failed, reason) }
        }
    }

    /// Where the library stood: the view, its selection and the open photograph.
    private struct LibraryView: Equatable {
        var mode: LibraryMode
        var selection: LibrarySelection
        var open: URL?
    }

    private var libraryView: LibraryView { LibraryView(mode: mode, selection: selection, open: sourceItem) }

    /// One undo step puts the ratings and flags in `summaries` back. If
    /// nothing has moved since (`now`), the view goes back to `then` too, so
    /// a photograph the filter dropped comes back as the current one.
    private func registerLibraryUndo(_ summaries: [URL: LibrarySummary], from now: LibraryView, to then: LibraryView, action: String) {
        guard !summaries.isEmpty else { return }
        undoManager?.registerUndo(withTarget: libraryUndo) { [weak self] _ in
            MainActor.assumeIsolated { self?.restoreLibrary(summaries, from: now, to: then, action: action) }
        }
        undoManager?.setActionName(action)
    }

    /// Undo and redo of a rating or flag change.
    private func restoreLibrary(_ summaries: [URL: LibrarySummary], from now: LibraryView, to then: LibraryView, action: String) {
        guard !closing else { return }
        let view = libraryView, items = Set(library.items.map(\.url))
        var befores: [URL: LibrarySummary] = [:], queued = false
        for (photo, summary) in summaries where items.contains(photo) {
            let before = library.summary(for: photo)
            var after = before
            after.rating = summary.rating; after.flag = summary.flag
            guard after != before else { continue }
            befores[photo] = before
            if saveLibraryChange(from: before, to: after, photo: photo) == nil { queued = true }
            library.set(after, for: photo)
        }
        if queued { flushLibraryChanges() }
        if view == now, !cropping {
            if then.mode == .photo, let open = then.open, items.contains(open) {
                if open == sourceItem, !sourceDeleted { selection.select(open); mode = .photo } else { self.open(open) }
            } else if then.mode == .grid {
                if mode == .photo { showGrid() }
                if mode == .grid { selection = then.selection }
                selection.prune(to: Set(shownPhotos.map(\.url)))
            }
        } else {
            selection.prune(to: Set(shownPhotos.map(\.url)))
        }
        // Redo takes it back to where it is now, and changes them again.
        registerLibraryUndo(befores, from: libraryView, to: view, action: action)
    }

    /// Queues a rating or flag change to `photo`'s sidecar; why it can't, if
    /// it can't. The open photograph's goes to its variant; another's to its
    /// file's first variant, or a new one if it has no file.
    private func saveLibraryChange(from before: LibrarySummary, to after: LibrarySummary, photo: URL) -> String? {
        guard sidecarsEnabled else { return "Not saved: Settings › Save ratings and edits is off" }
        guard let sidecars else { return "Not saved" }
        // Benchmarks keep ratings for the session without a note.
        guard sidecars.writes else { return nil }
        let rating = after.rating != before.rating ? after.rating : nil
        let flag: PhotoSidecar.Flag?? = after.flag != before.flag ? .some(after.flag) : nil
        if photo == sourceItem {
            guard var context = photoSidecar, context.photo == source else { return "Not saved" }
            guard context.blocked == nil else { return "Not saved: its .negs file can’t be changed" }
            guard PhotoLibrary.canWrite(beside: photo) else { return "Not saved: the folder is read-only" }
            sidecars.enqueue(SidecarPatch(variant: context.variant, primary: context.variantIsNew, rating: rating, flag: flag,
                                          defaultsVersion: context.defaultsVersion, app: Self.appVersion), for: context.photo)
            context.exists = context.written != nil || after.rating > 0 || after.flag != nil
            photoSidecar = context
        } else {
            guard before.problem == nil else { return "Not saved: its .negs file can’t be changed" }
            guard PhotoLibrary.canWrite(beside: photo.photoFile) else { return "Not saved: the folder is read-only" }
            // A duplicate's own variant; a photograph's first, or a new one if it has no file.
            sidecars.enqueue(SidecarPatch(variant: photo.variantID ?? SidecarRestore.newVariantID(), primary: photo.variantID == nil,
                                          rating: rating, flag: flag, defaultsVersion: SidecarDefaults.latest, app: Self.appVersion),
                             for: photo.photoFile)
        }
        return nil
    }

    /// Opens the photograph `offset` places on in the film strip, as it's
    /// filtered.
    func step(by offset: Int) {
        guard !cropping, let source = sourceItem else { return }
        let shown = shownPhotos
        guard let index = shown.firstIndex(where: { $0.url == source }), shown.indices.contains(index + offset) else { return }
        open(shown[index + offset].url)
    }

    // MARK: Selection and grid

    /// The photographs the strip and grid show: those the filter lets
    /// through, and the current one.
    var shownPhotos: [PhotoFile] { library.shown(current: selection.current) }

    /// A click on a photograph in the strip or the grid. ⌘ adds or removes
    /// it and ⇧ selects a range, changing only the selection; a plain click
    /// makes it current, which in the edit view opens it. ⌘ on the grid's
    /// current photograph hands current on to the next one selected; in the
    /// edit view that would open it, so there it stays.
    func click(_ photo: URL, command: Bool = false, shift: Bool = false) {
        guard !cropping else { return }
        if command { selection.toggle(photo, in: mode == .grid ? shownPhotos.map(\.url) : nil) }
        else if shift { selection.extend(to: photo, in: shownPhotos.map(\.url)) }
        else if mode == .photo, photo != sourceItem { open(photo) }
        else { selection.select(photo) }
    }

    /// ← and →. In the edit view they open the next photograph; in the grid
    /// the current photograph moves without developing. With ⇧ the
    /// selection grows or shrinks instead and the current photograph stays.
    func move(by offset: Int, extending: Bool) {
        guard !cropping else { return }
        let order = shownPhotos.map(\.url)
        if extending { return selection.extend(by: offset, in: order) }
        guard mode == .grid else { return step(by: offset) }
        guard let current = selection.current, let index = order.firstIndex(of: current) else {
            if let first = order.first { selection.select(first) }
            return
        }
        guard order.indices.contains(index + offset) else { return }
        selection.select(order[index + offset])
    }

    /// ↑ and ↓ in the grid: the photograph a row up or down, the one
    /// nearest across (`SheetLayout.vertical`). With ⇧ the selection grows
    /// or shrinks to it.
    func moveRow(by rows: Int, extending: Bool) {
        guard !cropping, mode == .grid else { return }
        let order = shownPhotos.map(\.url)
        // ⇧ moves the selection's moving end, as ⇧← and ⇧→ do.
        let from = extending ? selection.lead ?? selection.current : selection.current
        guard let from, let index = order.firstIndex(of: from) else {
            if let first = order.first, !extending { selection.select(first) }
            return
        }
        guard let layout = gridLayout, layout.frames.count == order.count,
              let to = layout.vertical(from: index, by: rows) else { return }
        if extending { selection.extend(by: to - index, in: order) } else { selection.select(order[to]) }
    }

    /// ⌘A: every photograph shown.
    func selectAll() { selection.selectAll(shownPhotos.map(\.url)) }
    /// ⌥⌘A: all but the current photograph.
    func deselectAll() { selection.deselectAll() }
    /// The grid's rubber band: `photos` inside it, added to `base`.
    func band(_ photos: [URL], adding base: Set<URL>) { selection.band(photos, adding: base) }

    func showGrid() {
        if comparison != nil { return leaveCompare(toGrid: true) }
        guard let source = sourceItem, !cropping, mode != .grid else { return }
        holdOriginal(false)
        holdUpsideDown(false)
        // The sheet grows out of the strip, and folds back into the view as set.
        leaveImageOnly()
        endTray(keep: true)
        // The grid shows the open photograph as edited.
        flushSidecar()
        saveOpenThumbnail()
        saveOpenRender()
        if selection.current == nil { selection.select(source) }
        mode = .grid
        filmCacheRelease.release(.editorClosed)
    }

    /// Back to the edit view, on the grid's current photograph. With none,
    /// as when a rating left nothing the filter passes, there's nothing to edit.
    func editCurrent() {
        guard mode == .grid, let current = selection.current else { return }
        if current != sourceItem || sourceDeleted { open(current) } else { mode = .photo }
    }

    func changeGridCellSize(larger: Bool) { gridCellSize += larger ? 40 : -40 }

    private func filterChanged() {
        guard !library.filtering else { return }
        // A grid left empty by a rating finds the open photograph again
        // once a filter shows it.
        if mode == .grid, selection.current == nil, let open = sourceItem, !sourceDeleted,
           library.shown(current: nil).contains(where: { $0.url == open }) { selection.select(open) }
        // A filter that leaves out the grid's current photograph takes it
        // away too: the nearest selected one still shown becomes current,
        // or else the next one shown (the one before at the end).
        if mode == .grid, let current = selection.current, !library.filter.includes(library.summary(for: current)) {
            let order = library.items.map(\.url)
            let gone = Set(order.filter { !library.filter.includes(library.summary(for: $0)) })
            if selection.selected.contains(where: { $0 != current && !gone.contains($0) }) {
                selection.toggle(current, in: order.filter { !gone.contains($0) || $0 == current })
            } else if let next = Self.next(after: current, in: order, skipping: gone) {
                selection.select(next)
            } else {
                selection = LibrarySelection()
            }
        }
        selection.prune(to: Set(library.shown(current: selection.current).map(\.url)))
    }

    // MARK: Edits per photograph

    struct CopiedEdits: Equatable {
        /// The user's edits (`userEdits`), without a solved brightness match.
        var state: EditState
        var groups: Set<EditGroup>
        /// The photograph had edits of its own. One without them pastes
        /// none: what it lands on is left without edits in `groups`.
        var edited = true
    }

    /// Copy Edits…'s choice, remembered for the next time.
    var copyGroups: Set<EditGroup> {
        get {
            let names = UserDefaults.standard.stringArray(forKey: "copyEditGroups")
            return names.map { Set($0.compactMap(EditGroup.init(rawValue:))) } ?? EditGroup.standard
        }
        set { UserDefaults.standard.set(EditGroup.allCases.filter(newValue.contains).map(\.rawValue), forKey: "copyEditGroups") }
    }

    /// Edits can be copied, pasted and reset: the catalog has arrived and no
    /// crop is being drawn.
    var canChangeEdits: Bool { source != nil && hasCatalog && !cropping && !closing }

    /// What Copy Edits copies: the grid's current photograph, or the open one.
    var copySource: URL? { mode == .grid ? selection.current : sourceItem }
    var canCopyEdits: Bool { canChangeEdits && copySource != nil }
    var canPasteEdits: Bool { copiedEdits != nil && canChangeEdits && !editTargets.isEmpty }

    /// What Paste Edits, Reset Edits and Duplicate act on: the selection in
    /// the grid, or in the film strip while it shows; else the open photograph.
    var editTargets: [URL] {
        guard mode == .grid || stripShown else { return sourceItem.map { [$0] } ?? [] }
        return selection.ordered(shownPhotos.map(\.url))
    }

    /// A context menu's: the selection if `item` is in it, else `item`
    /// alone, as in Finder.
    func contextTargets(_ item: URL) -> [URL] {
        selection.selected.contains(item) ? selection.ordered(shownPhotos.map(\.url)) : [item]
    }

    /// Photo › Copy Edits…: the groups to copy as tick boxes in the alert
    /// sheet, the last choice ticked. Copy remembers the choice and copies.
    func copyEditsChoosing() {
        let chosen = copyGroups
        Task {
            let answer = await ask(AppAlert(
                title: "Copy Edits", detail: "Choose the edits Paste Edits applies.",
                buttons: [.init(title: "Copy"), .init(title: "Cancel", role: .cancel)],
                choices: EditGroup.allCases.map { AppAlert.Choice(title: $0.title, on: chosen.contains($0)) }))
            guard answer.button == 0 else { return }
            let groups = Set(zip(EditGroup.allCases, answer.choices).filter(\.1).map(\.0))
            copyGroups = groups
            copyEdits(groups: groups)
        }
    }

    func copyEdits(from item: URL? = nil, groups: Set<EditGroup> = EditGroup.standard) {
        guard canChangeEdits, let item = item ?? copySource else { return }
        guard let (edits, edited) = userEdits(of: item) else {
            libraryNote = LibraryNote(text: "Edits Not Copied", detail: "Its .negs file can’t be read", serial: (libraryNote?.serial ?? 0) + 1)
            return
        }
        copiedEdits = CopiedEdits(state: edits, groups: groups, edited: edited)
        libraryNote = LibraryNote(text: "Edits Copied", serial: (libraryNote?.serial ?? 0) + 1)
    }

    func pasteEdits(onto items: [URL]? = nil) {
        guard let copied = copiedEdits else { return }
        changeEdits(items ?? editTargets, action: "Paste Edits", note: "Edits Pasted", fromUnedited: copied.edited ? nil : copied.groups) {
            EditGroup.merge(copied.groups, from: copied.state, into: $0)
        }
    }

    /// Applies the last photograph left with edits, no copy step.
    func pasteFromPrevious(onto items: [URL]? = nil) {
        guard let previous = previousEdits else { return }
        changeEdits(items ?? editTargets, action: "Paste from Previous", note: "Edits Pasted") {
            EditGroup.merge(EditGroup.standard, from: previous, into: $0)
        }
    }

    /// Removes the photographs' edits, crop and rotation included: they are
    /// as if never edited. The open one shows what a new photograph would
    /// get after it: the default film and paper, keeping its Image Size and
    /// output colour. Ratings and flags stay.
    func resetEdits(of items: [URL]? = nil) {
        changeEdits(items ?? editTargets, action: "Reset Edits", note: "Edits Reset", reset: true) { $0 }
    }

    /// `item`'s edits as `userEdits` gives them, and whether it has any of
    /// its own; nil if its sidecar can't be read.
    private func userEdits(of item: URL) -> (state: EditState, edited: Bool)? {
        if item == sourceItem { return (Self.userEdits(state), openPhotographEdited) }
        guard let lookup = sidecarLookup else { return (Self.userEdits(plainState), false) }
        let restore = SidecarRestore.read(photo: item.photoFile, variant: item.variantID, store: lookup.store, data: lookup.data)
        guard restore.problem == nil else { return nil }
        guard let saved = restore.edits else { return (Self.userEdits(plainState), false) }
        return (Self.userEdits(restoredState(saved.state, legacy: false)), true)
    }

    /// The open photograph has edits its sidecar keeps: saved before, or
    /// made since it opened. Without a sidecar there's no telling, and its
    /// settings count as edits.
    private var openPhotographEdited: Bool {
        guard let context = photoSidecar, context.photo == source else { return true }
        return context.editedAtOpen || Self.userEdits(state) != context.opened
    }

    /// A photograph without edits, not following any other.
    var plainState: EditState { newPhotoState(after: EditState()) }

    /// After pasting `groups` from a photograph without edits: `pasted` has
    /// none either when everything else is as a photograph without edits
    /// has it. The film and paper must be among `groups`, since a film kept
    /// in a sidecar is an edit, whichever it is. What the pasted groups hold
    /// doesn't count, such as the film the copied photograph showed by
    /// following the one before it.
    private func holdsNoEdits(_ pasted: EditState, pasting groups: Set<EditGroup>) -> Bool {
        guard groups.contains(.filmAndPaper) else { return false }
        return Self.userEdits(pasted) == Self.userEdits(EditGroup.merge(groups, from: pasted, into: plainState))
    }

    /// What a photograph's edits are stored against: `params` with the
    /// engine's defaults too, so values from a newer version stay.
    private func known(_ table: SidecarDefaults) -> [String: [JSONValue]] {
        ["params": [table.params, defaults], "crop": [table.crop], "border": [table.border]]
    }

    /// Changes the edits of `items` as one undo step: the open one in place,
    /// the others in their sidecars. Each sidecar is kept as it was first, so
    /// undo can put it back. `fromUnedited`: the groups pasted from a
    /// photograph without edits, which leave one without edits as it is,
    /// and one whose edits were all in them without any.
    private func changeEdits(_ items: [URL], action: String, note: String, reset: Bool = false, fromUnedited: Set<EditGroup>? = nil,
                             _ transform: (EditState) -> EditState) {
        changeEdits(items, action: action, note: note, reset: reset, fromUnedited: fromUnedited) { transform($1) }
    }
    /// `changeEdits`, with a change of its own for each photograph.
    private func changeEdits(_ items: [URL], action: String, note: String, reset: Bool = false, fromUnedited: Set<EditGroup>? = nil,
                             _ transform: (URL, EditState) -> EditState) {
        guard canChangeEdits, !items.isEmpty else { return }
        let open = sourceItem
        guard sidecarsEnabled, let sidecars, sidecars.writes, let lookup = sidecarLookup else {
            // Nothing is saved per photograph: only the open one changes, as any edit would.
            if let open, items.contains(open) { apply(reset ? newPhotoState(after: state) : transform(open, state), action: action) }
            let unsaved = items.contains { $0 != open } ? (sidecarsEnabled ? "Not saved" : "Not saved: Settings › Save ratings and edits is off") : nil
            libraryNote = LibraryNote(text: note, detail: unsaved, count: items.count, serial: (libraryNote?.serial ?? 0) + 1)
            return
        }
        // The open photograph's own edits are in its sidecar before it's kept.
        flushSidecar()
        var before: [URL: JSONValue?] = [:], unsaved: String?, openChanged = false
        for item in items {
            let photo = item.photoFile
            guard PhotoLibrary.canWrite(beside: photo) else { unsaved = unsaved ?? "Not saved: the folder is read-only"; continue }
            do {
                let raw = try sidecars.readPending(photo: photo)
                if item == open {
                    guard var context = photoSidecar, context.photo == photo, context.blocked == nil, !context.awaitingCatalog else {
                        unsaved = unsaved ?? "Not saved: its .negs file can’t be changed"; continue
                    }
                    // Nothing pasted onto nothing.
                    if fromUnedited != nil, !openPhotographEdited { continue }
                    let pasted = transform(item, state)
                    let cleared = fromUnedited.map { holdsNoEdits(pasted, pasting: $0) } ?? false
                    if before[photo] == nil { before[photo] = .some(raw) }
                    openChanged = true
                    if reset || cleared {
                        // Without edits it shows what a new photograph does now,
                        // not the film and paper the copy had then.
                        replaceState(newPhotoState(after: state))
                        // Unedited from here on: a later edit is measured from this.
                        sidecarWrite?.cancel(); sidecarWrite = nil
                        sidecars.enqueue(SidecarPatch(variant: context.variant, primary: context.variantIsNew, edits: .some(nil),
                                                      defaultsVersion: context.defaultsVersion, app: Self.appVersion), for: photo)
                        context.written = nil; context.editedAtOpen = false; context.opened = Self.userEdits(state)
                        photoSidecar = context
                    } else {
                        replaceState(pasted)
                        flushSidecar()
                    }
                    continue
                }
                let sidecar = try raw.map(PhotoSidecar.init(raw:))
                let variant = item.variantID.map { id in sidecar?.variants.first { $0.id == id } } ?? sidecar?.variants.first
                // A duplicate removed meanwhile.
                if item.variantID != nil, variant == nil { continue }
                // Nothing pasted onto nothing.
                if fromUnedited != nil, variant?.edits == nil { continue }
                if before[photo] == nil { before[photo] = .some(raw) }
                let version = sidecar?.defaultsVersion ?? SidecarDefaults.latest
                let table = try SidecarDefaults.load(version: version, data: lookup.data)
                var edits: JSONValue?
                if !reset {
                    let saved = try variant?.edits.map { try PhotoSidecar.decodeEdits($0, table: table) }
                    let base = saved.map { restoredState($0.state, legacy: false) } ?? plainState
                    let pasted = transform(item, base)
                    // Its match, if on, is solved again when it opens: the edits it was solved for changed.
                    if !(fromUnedited.map { holdsNoEdits(pasted, pasting: $0) } ?? false) {
                        edits = PhotoSidecar.encodeEdits(pasted, matchSolved: false, table: table)
                    }
                }
                sidecars.enqueue(SidecarPatch(variant: variant?.id ?? SidecarRestore.newVariantID(), primary: item.variantID == nil,
                                              edits: .some(edits), known: known(table), defaultsVersion: version, app: Self.appVersion), for: photo)
            } catch {
                unsaved = unsaved ?? "Not saved: its .negs file can’t be changed"
            }
        }
        finishSidecarChange(before, action: action, openChanged: openChanged)
        libraryNote = LibraryNote(text: note, detail: unsaved, count: items.count, serial: (libraryNote?.serial ?? 0) + 1)
    }

    /// Duplicate: a virtual copy of each item, a new variant in its sidecar
    /// with the same edits, rating and flag, listed after the others.
    /// The grid selects the copies; the edit view opens the first.
    func duplicate(_ items: [URL]? = nil) {
        let items = items ?? editTargets
        guard canChangeEdits, !items.isEmpty else { return }
        guard sidecarsEnabled, let sidecars, sidecars.writes else {
            libraryNote = LibraryNote(text: "Not Duplicated", detail: "Duplicates are kept in .negs files, which Settings › Save ratings and edits turns off",
                                      serial: (libraryNote?.serial ?? 0) + 1)
            return
        }
        flushSidecar()
        var before: [URL: JSONValue?] = [:], made: [URL] = [], unsaved: String?
        for item in items {
            let photo = item.photoFile
            guard PhotoLibrary.canWrite(beside: photo) else { unsaved = unsaved ?? "Not saved: the folder is read-only"; continue }
            do {
                let raw = try sidecars.readPending(photo: photo)
                // Checks the schema: a newer file is never written over.
                if let raw { _ = try PhotoSidecar(raw: raw) }
                if before[photo] == nil { before[photo] = .some(raw) }
                var top = raw?.object ?? ["negs": .number(Double(PhotoSidecar.schema)), "defaultsVersion": .number(Double(SidecarDefaults.latest))]
                var variants = top["variants"]?.array ?? []
                if variants.isEmpty {
                    // The photograph's own variant first, with the id the open one already uses.
                    let id = item == sourceItem ? photoSidecar?.variant ?? SidecarRestore.newVariantID() : SidecarRestore.newVariantID()
                    variants.append(.object(["id": .string(id)]))
                }
                let original = item.variantID.flatMap { id in variants.firstIndex { $0["id"] == .string(id) } } ?? 0
                let id = SidecarRestore.newVariantID()
                var copy: [String: JSONValue] = ["id": .string(id)]
                // Edits, rating and flag, as Lightroom's virtual copies; from here on its own.
                for key in ["edits", "rating", "flag"] { if let value = variants[original].object?[key] { copy[key] = value } }
                variants.append(.object(copy))
                top["variants"] = .array(variants)
                top["app"] = .string(Self.appVersion)
                sidecars.enqueue(SidecarPatch(variant: id, replacement: .some(.object(top)), defaultsVersion: SidecarDefaults.latest,
                                              app: Self.appVersion), for: photo)
                made.append(photo.variant(id))
            } catch {
                unsaved = unsaved ?? "Not saved: its .negs file can’t be changed"
            }
        }
        finishSidecarChange(before, action: made.count > 1 ? "Duplicate Photographs" : "Duplicate", openChanged: false)
        if let first = made.first {
            if mode == .grid { selection.select(made) } else { open(first) }
        }
        libraryNote = LibraryNote(text: made.isEmpty ? "Not Duplicated" : "Duplicated", detail: unsaved, count: max(made.count, 1),
                                  serial: (libraryNote?.serial ?? 0) + 1)
    }

    /// The batch exporting now, or the last one until its failures are seen.
    private(set) var batch: BatchRun?
    @ObservationIgnored var batchJournalURL = BatchJournal.url
    /// The batch's helper, for Match camera brightness; quit when it ends.
    @ObservationIgnored private var batchEngine: EngineClient?
    @ObservationIgnored private var batchGeneration = 0

    /// The Lightroom apps installed (ticket 48): what the last detection
    /// found, until this launch has looked off the main actor.
    var lightroomApps = Lightroom.remembered
    /// "Starting Lightroom…", then "Sent 3 photographs to Lightroom" for a
    /// few seconds, in the status line and the grid's foot.
    var lightroomWords: String?
    @ObservationIgnored var lightroomDetected: Date?
    /// Cleanup checks after a send, while TIFFs wait for Lightroom.
    @ObservationIgnored var lightroomCleanup: Task<Void, Never>?
    /// Its TIFFs are being renamed and handed over: no cleanup meanwhile.
    @ObservationIgnored var lightroomHandingOver = false

    // MARK: Edited thumbnails

    /// Where edited thumbnails are kept; tests use their own.
    @ObservationIgnored var editedThumbnails = EditedThumbnailStore.shared {
        // Read already: the new store names its thumbnails with it too.
        didSet { if let thumbnailStamp { editedThumbnails.engineStamp = thumbnailStamp } }
    }
    /// Where photographs' last develops are kept, screen-sized; tests use their own.
    @ObservationIgnored var renderArchive = EditedThumbnailStore.renders {
        didSet { if let thumbnailStamp { renderArchive.engineStamp = thumbnailStamp } }
    }
    /// The engine's identity, once read: until then only embedded previews show.
    private(set) var thumbnailStamp: String?
    /// Moves when an item's edited thumbnail is made, so its cells read it.
    private(set) var thumbnailVersions: [URL: Int] = [:]
    /// Items in the strip or grid now: developed first.
    @ObservationIgnored private(set) var thumbnailsOnScreen: Set<URL> = []
    /// Each item's look last checked for a thumbnail, and whether it had one.
    @ObservationIgnored private var thumbnailsChecked: [URL: (look: String, present: Bool)] = [:]
    /// Looks that failed to develop this session, not tried again.
    @ObservationIgnored private var thumbnailFailures: [URL: String] = [:]
    /// How many thumbnails develop at once, each in a helper of its own
    /// (about 550 MB each): two on a Mac with the cores and memory.
    nonisolated static let thumbnailHelpers =
        ProcessInfo.processInfo.activeProcessorCount >= 8 && ProcessInfo.processInfo.physicalMemory >= 16 << 30 ? 2 : 1
    /// Low Power Mode is on: only the thumbnails on screen are developed, in
    /// one helper; the rest of the folder waits until it's off. Follows the
    /// Mac's setting (tests set it).
    @ObservationIgnored var lowPowerMode = ProcessInfo.processInfo.isLowPowerModeEnabled {
        didSet {
            guard lowPowerMode != oldValue else { return }
            if lowPowerMode { stopSpareThumbnailEngines() } else { pumpThumbnails() }
        }
    }
    @ObservationIgnored private var powerStateObserver: NSObjectProtocol?
    /// How many helpers develop thumbnails now.
    private var thumbnailHelpersNow: Int { lowPowerMode ? 1 : Self.thumbnailHelpers }
    /// The background helpers, started while there's work, by slot.
    @ObservationIgnored private var thumbnailEngines: [Int: EngineClient] = [:]
    /// Each slot's thumbnail developing now, and its job.
    @ObservationIgnored private var thumbnailsDeveloping: [Int: (item: URL, task: Task<Void, Never>)] = [:]
    /// The scan for the next thumbnails to develop.
    @ObservationIgnored private var thumbnailTask: Task<Void, Never>?
    @ObservationIgnored private var thumbnailsPumpAgain = false
    @ObservationIgnored private var thumbnailIdle: Task<Void, Never>?
    @ObservationIgnored private var thumbnailGeneration = 0
    @ObservationIgnored private var readingThumbnailStamp = false
    /// Moves when the cache is emptied: a scan from before finds nothing.
    @ObservationIgnored private var thumbnailsEmptied = 0

    /// The look that names `item`'s developed thumbnail: its stored edits',
    /// or `uneditedLook` for one without edits once its sidecar has been read
    /// (with sidecars off, every photograph opens with the defaults). Nil
    /// while that isn't known, or when its sidecar can't be read.
    func thumbnailLook(for item: URL) -> String? {
        // Read lazily, so a cell depends only on what decided its look.
        if let look = library.looks[item] { return look }
        guard let unedited = uneditedLook else { return nil }
        if library.store == nil { return unedited }
        guard let summary = library.summaries[item], summary.problem == nil else { return nil }
        return unedited
    }

    /// What `thumbnailLook(for:)` reads, copied, so the pump can scan a whole
    /// folder off the main actor. Decides as it does.
    struct ThumbnailLooks: Sendable {
        let looks: [URL: String], summaries: [URL: LibrarySummary], unedited: String?, sidecars: Bool

        func look(for item: URL) -> String? {
            if let look = looks[item] { return look }
            guard let unedited else { return nil }
            if !sidecars { return unedited }
            guard let summary = summaries[item], summary.problem == nil else { return nil }
            return unedited
        }
    }
    private var thumbnailLooks: ThumbnailLooks {
        ThumbnailLooks(looks: library.looks, summaries: library.summaries, unedited: uneditedLook, sidecars: library.store != nil)
    }

    func editedThumbnail(for item: URL) -> EditedThumbnail? {
        guard let stamp = thumbnailStamp, let look = thumbnailLook(for: item) else { return nil }
        return EditedThumbnail(look: look, version: thumbnailVersions[item] ?? 0, stamp: stamp, store: editedThumbnails)
    }

    /// Where the pump develops out from once those on screen are done: the
    /// middle of the screen (indices in folder order, ascending), else the open photograph.
    nonisolated static func thumbnailCentre(onScreen: [Int], open: Int?) -> Int {
        onScreen.isEmpty ? open ?? 0 : onScreen[onScreen.count / 2]
    }

    /// `items` nearest `centre` first; of two as near, the one before.
    nonisolated static func outward<Item>(_ items: [(index: Int, item: Item)], from centre: Int) -> [Item] {
        items.sorted { (abs($0.index - centre), $0.index) < (abs($1.index - centre), $1.index) }.map(\.item)
    }

    /// A strip or grid cell came on screen or left it.
    func thumbnailShown(_ file: PhotoFile, _ visible: Bool) {
        prefetchThumbnails()
        guard visible else { thumbnailsOnScreen.remove(file.url); return }
        thumbnailsOnScreen.insert(file.url)
        if thumbnailLook(for: file.url) != nil { pumpThumbnails() }
    }

    /// How far either side of the middle of the screen thumbnails are read
    /// into memory ahead of scrolling: 25–45 MB in the grid, which the
    /// in-memory caches hold.
    nonisolated static let prefetchReach = 32
    /// The grid cells' size, for reading ahead at it.
    @ObservationIgnored var gridThumbnailSize = PhotoThumbnails.Size.medium
    @ObservationIgnored private var prefetchRound = 0
    @ObservationIgnored private var prefetching: Task<Void, Never>?

    /// Reads the thumbnails near the strip's or grid's view into memory, one
    /// at a time and nearest first, so they're there before they're scrolled
    /// to. Each change of the view starts again from its middle.
    func prefetchThumbnails() {
        #if SPEKTRAFILM_BENCHMARK
        if ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_NO_PREFETCH"] != nil { return }
        #endif
        prefetchRound += 1
        guard prefetching == nil else { return }
        prefetching = Task { [weak self] in
            // After this turn of the main actor, once the cells that changed have.
            await Task.yield()
            while let self, !Task.isCancelled, !self.closing {
                let round = self.prefetchRound
                for (file, size, edited) in await self.prefetchTargets() {
                    guard self.prefetchRound == round, !Task.isCancelled else { break }
                    let image = await Task.detached(priority: .utility) {
                        await PhotoThumbnails.shared.image(for: file, size: size, look: edited?.look, edited: edited?.store, ahead: true)
                    }.value
                    // Its frame takes the shape of what will be shown.
                    if let image { self.aspects.note(file.url, image: image) }
                }
                guard self.prefetchRound != round else { break }
                // While scrolling, a round at most every 30 ms.
                try? await Task.sleep(for: .milliseconds(30))
            }
            self?.prefetching = nil
        }
    }

    /// Those within `prefetchReach` of the middle of the screen (else the
    /// current photograph), nearest first, but those on screen, which their
    /// cells read; nothing when neither the strip nor the grid shows. Found
    /// off the main actor: a folder can hold thousands.
    private func prefetchTargets() async -> [(PhotoFile, PhotoThumbnails.Size, EditedThumbnail?)] {
        let grid = mode == .grid
        guard grid || (stripShown && source != nil), !library.filtering else { return [] }
        let size = grid ? gridThumbnailSize : .strip
        let shown = shownPhotos, onScreen = thumbnailsOnScreen, current = grid ? selection.current : sourceItem
        let near = await Task.detached(priority: .utility) { () -> [PhotoFile] in
            guard !shown.isEmpty else { return [] }
            var visible: [Int] = [], currentIndex: Int?
            for (index, file) in shown.enumerated() {
                if onScreen.contains(file.url) { visible.append(index) }
                if file.url == current { currentIndex = index }
            }
            let centre = Self.thumbnailCentre(onScreen: visible, open: currentIndex)
            let reach = max(0, centre - Self.prefetchReach)...min(shown.count - 1, centre + Self.prefetchReach)
            return Self.outward(reach.filter { !onScreen.contains(shown[$0].url) }.map { ($0, shown[$0]) }, from: centre)
        }.value
        return near.map { ($0, size, editedThumbnail(for: $0.url)) }
    }

    /// Reads the engine's identity once, off the main actor, after launch.
    private func readThumbnailStamp() {
        guard thumbnailStamp == nil, !readingThumbnailStamp, let paths = engine?.paths else { return }
        readingThumbnailStamp = true
        let store = editedThumbnails, archive = renderArchive
        Task { [weak self] in
            let stamp = await Task.detached(priority: .utility) { () -> String? in
                let stamp = EditedThumbnailStore.stamp(paths)
                store.engineStamp = stamp
                archive.engineStamp = stamp
                // The oldest go once a session.
                store.prune()
                archive.prune()
                return stamp
            }.value
            guard let self, !self.closing else { return }
            self.thumbnailStamp = stamp
            // The stores may have been replaced while it was read.
            self.editedThumbnails.engineStamp = stamp
            self.renderArchive.engineStamp = stamp
            self.pumpThumbnails()
        }
    }

    /// The bytes the thumbnails take on disk, for Settings.
    func thumbnailCacheSize() async -> Int {
        let store = editedThumbnails, archive = renderArchive
        return await Task.detached(priority: .utility) { store.size() + archive.size() }.value
    }

    /// Removes every thumbnail; the open folder's are developed again.
    /// Those on screen stay until each is made again.
    func emptyThumbnailCache() async {
        let store = editedThumbnails, archive = renderArchive
        await Task.detached(priority: .userInitiated) { store.removeAll(); archive.removeAll() }.value
        thumbnailsEmptied += 1
        thumbnailsChecked = [:]
        pumpThumbnails()
    }

    private func thumbnailMade(_ item: URL, look: String) {
        thumbnailsChecked[item] = (look, true)
        thumbnailVersions[item, default: 0] += 1
        prefetchThumbnails()
    }

    /// The open photograph's developed thumbnail, made from the preview on
    /// screen, so it costs no render: on leaving it for another photograph
    /// or the grid. One without edits only while it shows what a new
    /// photograph opens with (it may predate a new default).
    private func saveOpenThumbnail() {
        guard let item = sourceItem, let surface = image, previewCurrent, !showingInteractiveFrame, heldPreview == nil, cropSession == nil,
              !brightnessMatchPending, let look = thumbnailLook(for: item), thumbnailsChecked[item].map({ $0.look != look || !$0.present }) ?? true,
              library.looks[item] != nil ? shownEditsSaved : Self.uneditedLook(of: state) == look,
              let file = library.items.first(where: { $0.url == item }), let key = editedThumbnails.key(for: file, look: look) else { return }
        #if DEBUG
        thumbnailSaved?(item, look, Self.userEdits(state), "thumbnail")
        #endif
        let size = imageSize, crop = crop, rotation = imageRotation, flip = imageFlip, border = state.border, texture = filmTexture, strip = filmStrip
        let store = editedThumbnails, loose = store.looseKey(for: file, look: look)
        let shown = surface
        Task { [weak self] in
            guard let photo = await Task.detached(priority: .utility, operation: { ThumbnailComposer.image(of: shown) }).value,
                  let composed = ThumbnailComposer.compose(photo, size: size, crop: crop, cut: false, rotation: rotation, flip: flip, border: border, texture: texture,
                                                           strip: strip, space: photo.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB)!) else { return }
            let saved = await Task.detached(priority: .utility) { (try? store.save(composed, as: key, loose: loose)) != nil }.value
            if saved { self?.thumbnailMade(item, look: look) }
        }
    }

    /// The open photograph's develop, screen-sized and uncropped, kept as
    /// it's left so it stands in when the photograph next opens (`renderArchive`).
    /// Under the same conditions as its thumbnail, off the main actor.
    private func saveOpenRender() {
        guard let item = sourceItem, let surface = image, previewCurrent, !showingInteractiveFrame, heldPreview == nil, cropSession == nil,
              !brightnessMatchPending, let look = thumbnailLook(for: item),
              library.looks[item] != nil ? shownEditsSaved : Self.uneditedLook(of: state) == look,
              let file = library.items.first(where: { $0.url == item }), let key = renderArchive.key(for: file, look: look) else { return }
        #if DEBUG
        thumbnailSaved?(item, look, Self.userEdits(state), "kept develop")
        #endif
        let archive = renderArchive, size = imageSize, shown = surface, loose = archive.looseKey(for: file, look: look)
        Task.detached(priority: .utility) {
            guard !archive.contains(key),
                  let frame = ThumbnailComposer.image(of: shown, maxPixels: EditedThumbnailStore.renderLongSide) else { return }
            try? archive.save(frame, as: key, loose: loose, photoSize: size)
        }
    }

    /// The open photograph shows the edits its sidecar keeps: nothing waits
    /// to be sent, and nothing failed to be written. Only then is its preview
    /// filed under its stored edits' look.
    private var shownEditsSaved: Bool {
        guard let context = photoSidecar, context.photo == source else { return true }
        return context.blocked == nil && !context.awaitingCatalog && sidecarEditsToSend(context) == nil
    }

    /// The background renderer can use the machine: nothing is being edited,
    /// developed, exported or saved in the window.
    private var thumbnailsMayRun: Bool {
        !closing && !interacting && !rendering && pendingRender == nil && !exporting && !batchRunning && !saving && !cropping && matchTask == nil
            && comparison == nil && (mode == .grid || source == nil || previewCurrent || renderFailed)
    }

    /// Develops the next items without a current thumbnail, edited or not,
    /// in helpers of their own, at utility priority: those on screen first,
    /// then the folder in order. `thumbnailHelpers` at a time, waiting while
    /// the open photograph is edited or developed. The helpers quit when
    /// there's nothing left.
    func pumpThumbnails() {
        guard !closing, hasCatalog, let paths = engine?.paths else { return }
        // Its scan reads what it was started with: once more after, for what changed.
        guard thumbnailTask == nil else { thumbnailsPumpAgain = true; return }
        // Each that finishes pumps again.
        let free = (0..<thumbnailHelpersNow).filter { thumbnailsDeveloping[$0] == nil }
        guard !free.isEmpty else { return }
        if thumbnailStamp == nil { return readThumbnailStamp() }
        thumbnailTask = Task { [weak self] in
            // None is developed before then, so the folder isn't scanned
            // while the open photograph loads or is edited.
            guard let self, await self.waitForThumbnailTurn() else { return }
            let store = self.editedThumbnails, lookup = self.sidecarLookup
            // Copies (the dictionaries copy on write): a folder of thousands
            // is scanned for every thumbnail, which must stay off the main actor.
            let items = self.library.items, open = self.sourceItem, onScreen = self.thumbnailsOnScreen, looks = self.thumbnailLooks
            let failures = self.thumbnailFailures, checked = self.thumbnailsChecked, emptied = self.thumbnailsEmptied
            let developing = Set(self.thumbnailsDeveloping.values.map(\.item)), onlyOnScreen = self.lowPowerMode
            self.thumbnailsPumpAgain = false
            // Each item but the open one, whose thumbnail comes from its
            // preview, and those developing, on screen first, then outward
            // from the middle of the screen (or the open photograph), so the
            // next ones scrolled to or opened are ready: the first without a
            // thumbnail on disk, one for each free helper. Those with one are
            // noted. In Low Power Mode, those on screen only.
            let found = await Task.detached(priority: .utility) { () -> (present: [(URL, String)], next: [(file: PhotoFile, look: String)]) in
                var shown: [(file: PhotoFile, look: String)] = [], rest: [(index: Int, file: PhotoFile, look: String)] = []
                var onScreenIndices: [Int] = [], openIndex: Int?
                for (index, file) in items.enumerated() {
                    if file.url == open { openIndex = index; continue }
                    if onScreen.contains(file.url) { onScreenIndices.append(index) }
                    guard !developing.contains(file.url), let look = looks.look(for: file.url), failures[file.url] != look else { continue }
                    if let checked = checked[file.url], checked.look == look, checked.present { continue }
                    if onScreen.contains(file.url) { shown.append((file, look)) } else if !onlyOnScreen { rest.append((index, file, look)) }
                }
                let centre = Self.thumbnailCentre(onScreen: onScreenIndices, open: openIndex)
                let ordered = shown + Self.outward(rest.map { ($0.index, (file: $0.file, look: $0.look)) }, from: centre)
                var present: [(URL, String)] = [], next: [(file: PhotoFile, look: String)] = []
                for (file, look) in ordered {
                    guard let key = store.key(for: file, look: look) else { continue }
                    if store.contains(key) { present.append((file.url, look)) } else { next.append((file, look)) }
                    if next.count == free.count { break }
                }
                return (present, next)
            }.value
            guard !self.closing else { return }
            guard emptied == self.thumbnailsEmptied else {
                self.thumbnailTask = nil
                return self.pumpThumbnails()
            }
            for (item, look) in found.present { self.thumbnailsChecked[item] = (look, true) }
            for (slot, (file, look)) in zip(free, found.next) {
                self.thumbnailIdle?.cancel(); self.thumbnailIdle = nil
                self.thumbnailsChecked[file.url] = (look, false)
                let task = Task { [weak self] in
                    guard let self else { return }
                    if await !self.developThumbnail(file, look: look, lookup: lookup, paths: paths, slot: slot) { self.thumbnailFailures[file.url] = look }
                    self.thumbnailsDeveloping[slot] = nil
                    if slot >= self.thumbnailHelpersNow { self.stopSpareThumbnailEngines() }
                    self.pumpThumbnails()
                }
                self.thumbnailsDeveloping[slot] = (file.url, task)
            }
            self.thumbnailTask = nil
            if self.thumbnailsPumpAgain {
                self.pumpThumbnails()
            } else if self.thumbnailsDeveloping.isEmpty {
                self.stopThumbnailEngineSoon()
            }
        }
    }

    /// Quits the idle helpers past `thumbnailHelpersNow`, so their memory
    /// goes; one still developing finishes first.
    private func stopSpareThumbnailEngines() {
        let spare = thumbnailEngines.filter { $0.key >= thumbnailHelpersNow && thumbnailsDeveloping[$0.key] == nil }
        guard !spare.isEmpty else { return }
        spare.keys.forEach { thumbnailEngines[$0] = nil }
        let engines = Array(spare.values)
        Task.detached(priority: .utility) { engines.forEach { $0.stop() } }
    }

    /// Quits the background helpers after a while without work, so their memory goes.
    private func stopThumbnailEngineSoon() {
        guard !thumbnailEngines.isEmpty, thumbnailIdle == nil else { return }
        thumbnailIdle = Task { [weak self] in
            try? await Task.sleep(for: .seconds(20))
            guard let self, !Task.isCancelled, self.thumbnailTask == nil, self.thumbnailsDeveloping.isEmpty else { return }
            let engines = Array(self.thumbnailEngines.values)
            self.thumbnailEngines = [:]; self.thumbnailIdle = nil
            await Task.detached(priority: .utility) { engines.forEach { $0.stop() } }.value
        }
    }

    /// Waits until the background renderer may use the machine.
    private func waitForThumbnailTurn() async -> Bool {
        while !thumbnailsMayRun {
            guard !closing, !Task.isCancelled else { return false }
            try? await Task.sleep(for: .milliseconds(250))
        }
        return !closing
    }

    /// Develops `file`'s thumbnail as opening it would: with its stored
    /// edits, or, without edits (`uneditedLook`), as a new photograph. A
    /// match not yet solved for stored edits is solved on the whole decode
    /// and stored, unless its edits changed meanwhile; an unedited one's is
    /// solved on the reduced decode for the thumbnail only. Everything else
    /// reads only as much of a RAW as the thumbnail needs. False when it
    /// can't be made.
    private func developThumbnail(_ file: PhotoFile, look: String, lookup: SidecarLookup?, paths: EnginePaths, slot: Int) async -> Bool {
        let item = file.url, photo = item.photoFile
        var state: EditState, matchSolved: Bool
        /// Stores a solved match with the stored edits: their new look, or nil.
        var storeMatch: ((EditState) -> String?)?
        if look == uneditedLook, library.looks[item] == nil {
            state = plainState; matchSolved = false
        } else {
            guard let lookup, let sidecar = try? lookup.store.read(photo: photo),
                  let variant = item.variantID.map({ id in sidecar.variants.first { $0.id == id } }) ?? sidecar.variants.first,
                  let stored = variant.edits, PhotoSidecar.look(of: stored, defaultsVersion: sidecar.defaultsVersion) == look,
                  let table = try? SidecarDefaults.load(version: sidecar.defaultsVersion, data: lookup.data),
                  let saved = try? PhotoSidecar.decodeEdits(stored, table: table) else { return false }
            state = restoredState(saved.state, legacy: false, matchSolved: saved.matchSolved); matchSolved = saved.matchSolved
            storeMatch = { [weak self] in self?.storeSolvedMatch($0, item: item, variant: variant.id, stored: stored, sidecar: sidecar, table: table) }
        }
        let raw = Self.rawExtensions.contains(photo.pathExtension.lowercased())
        let engine: EngineClient
        do {
            if let running = thumbnailEngines[slot] { engine = running } else {
                engine = try EngineClient(paths: paths, quality: .utility); thumbnailEngines[slot] = engine
            }
        } catch { return false }
        thumbnailGeneration += 1
        let generation = thumbnailGeneration, space = previewSpace
        let buffer = engine.directory.appendingPathComponent("library-thumbnail.rgba")
        defer { try? FileManager.default.removeItem(at: buffer) }
        let longSide = Int(EditedThumbnailStore.longSide)
        func command(_ state: EditState, longSide: Int) -> [String: JSONValue] {
            var params = state.params
            params["io.output_color_space"] = .string(space)
            var command: [String: JSONValue] = [
                "command": .string("thumbnail"), "input": .string(photo.path), "source_generation": .number(Double(generation)),
                "film": .string(state.film), "paper": .string(state.paper), "params": params,
                "long_side": .number(Double(longSide)), "preview_path": .string(buffer.path),
                // A whole decode the helper already holds serves this too.
                "reduced_decode": .bool(true),
            ]
            if let crop = Self.cropJSON(state.crop) { command["crop"] = crop }
            return command
        }
        var finalLook = look
        do {
            let matchable = raw || state.params["io.input_interpretation.mode"].string == "auto"
            if state.brightnessMatch != nil, !matchSolved, matchable {
                // The decode first, which the match is solved on: the whole
                // photograph for a match that's stored, so opening it finds
                // the same value; else the thumbnail's own reduced read.
                // Not developed, and the camera preview's colour space read
                // meanwhile.
                guard await waitForThumbnailTurn() else { return false }
                var decode = command(state, longSide: storeMatch == nil ? longSide : 16)
                if storeMatch != nil { decode["reduced_decode"] = .bool(false) }
                decode["decode_only"] = .bool(true)
                if raw { decode["preview_colour_space"] = .bool(true) }
                // The camera preview read meanwhile (~35 ms).
                let prepared = raw ? Task.detached(priority: .utility) { CameraBrightnessReference.Prepared(from: photo) } : nil
                let cacheKey = storeMatch != nil ? matchCacheKey(photo, request: matchSolveState(state), paths: paths) : nil
                let (_, decoded) = try await engine.request(decode)
                let size = CGSize(width: decoded["source_width"].double, height: decoded["source_height"].double)
                guard await waitForThumbnailTurn() else { return false }
                let reference = engine.directory.appendingPathComponent("library-brightness.png")
                defer { try? FileManager.default.removeItem(at: reference) }
                if let solved = try? await Self.solveBrightnessMatch(engine: engine, source: photo, generation: generation, size: size, raw: raw,
                                                                    request: matchSolveState(state), reference: reference,
                                                                    declared: raw ? decoded["preview_colour_space"] : nil, prepared: prepared),
                   let matched = Self.matching(state, ev: solved.exposure, limited: solved.limited, inputs: matchInputs(state)) {
                    // Solved on the whole decode (a stored match), so opening
                    // the photograph would solve the same: kept for it.
                    // The reduced decode's can differ by 0.01 EV and isn't.
                    if let cacheKey, cacheKey == matchCacheKey(photo, request: matchSolveState(state), paths: paths) {
                        matchCache.store(cacheKey, ev: solved.exposure, limited: solved.limited)
                    }
                    state = matched
                    if let storeMatch { finalLook = storeMatch(state) ?? look }
                }
            }
            guard await waitForThumbnailTurn() else { return false }
            let (_, result) = try await engine.request(command(state, longSide: longSide))
            let (width, height) = (Int(result["width"].double), Int(result["height"].double))
            let display = result["display_color_space"].string
            let size = CGSize(width: result["source_width"].double, height: result["source_height"].double)
            let developed = try await Task.detached(priority: .utility) {
                try Self.thumbnailImage(Data(contentsOf: buffer), width: width, height: height, displaySpace: display)
            }.value
            guard !closing else { return false }
            let strip = state.border.style == .film35 ? FilmFormat(state.crop.size(of: size)).flatMap { FilmStrip.load(data: paths.data, film: state.film, format: $0) } : nil
            guard let composed = ThumbnailComposer.compose(developed, size: size, crop: state.crop, cut: true, rotation: Int(state.params["io.rotate_cw"].double),
                                                           flip: state.params["io.flip"].bool,
                                                           border: state.border, texture: filmTexture, strip: strip,
                                                           space: developed.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB)!),
                  let key = editedThumbnails.key(for: file, look: finalLook) else { return false }
            let store = editedThumbnails, loose = store.looseKey(for: file, look: finalLook)
            try await Task.detached(priority: .utility) { try store.save(composed, as: key, loose: loose) }.value
            thumbnailMade(item, look: finalLook)
            #if DEBUG
            thumbnailSaved?(item, finalLook, Self.userEdits(state), "background thumbnail")
            #endif
            if finalLook != look { library.reload(photo) }
            return true
        } catch {
            FileHandle.standardError.write(Data("library thumbnail \(file.name): \(error.localizedDescription)\n".utf8))
            return false
        }
    }

    /// Stores a match solved in the background, unless the item's edits
    /// changed since they were read. Its new look, or nil if not stored.
    private func storeSolvedMatch(_ state: EditState, item: URL, variant: String, stored: JSONValue, sidecar: PhotoSidecar, table: SidecarDefaults) -> String? {
        let photo = item.photoFile
        guard sidecarsEnabled, let sidecars, sidecars.writes, PhotoLibrary.canWrite(beside: photo) else { return nil }
        let edits = PhotoSidecar.encodeEdits(state, matchSolved: true, table: table)
        sidecars.enqueue(SidecarPatch(variant: variant, edits: .some(edits), known: known(table), onlyIfEdits: .some(stored),
                                      defaultsVersion: sidecar.defaultsVersion, app: Self.appVersion), for: photo)
        writeSidecars()
        guard let now = try? sidecars.read(photo: photo), let written = now.variants.first(where: { $0.id == variant }),
              written.edits != stored else { return nil }
        return PhotoSidecar.look(of: written.edits, defaultsVersion: now.defaultsVersion)
    }

    // MARK: Delete

    /// Where deleted photographs go.
    @ObservationIgnored var trash = PhotoTrash.system
    /// The open photograph was deleted while the grid showed. Leaving the
    /// grid opens its current photograph again, even if Undo put it back.
    private var sourceDeleted = false

    /// What Delete acts on: as the rating keys, the grid's selection, or in
    /// the edit view the open photograph only.
    var deleteTargets: [URL] { ratingTargets }

    /// Nothing is cropped, exported or saved from a file that might go.
    var canDelete: Bool { !closing && !cropping && !exporting && !saving }

    /// The folder's rejected photographs and duplicates, filtered out or not.
    var rejected: [URL] { library.items.map(\.url).filter { library.summary(for: $0).flag == .reject } }

    func deletePlan(_ items: [URL]) -> DeletePlan {
        DeletePlan(items, duplicates: { library.duplicates[$0] ?? [] }, companion: { library.companion(of: $0) })
    }

    /// ⌫ asks first and ⌘⌫ doesn't. Without a Trash it always asks, as
    /// nothing could be put back.
    func requestDelete(_ items: [URL]? = nil, asking: Bool) {
        let plan = deletePlan(items ?? deleteTargets)
        guard canDelete, let first = plan.items.first else { return }
        let folder = first.photoFile.deletingLastPathComponent()
        guard PhotoLibrary.canWrite(in: folder) else {
            error = "Nothing was deleted: the folder “\(folder.lastPathComponent)” is read-only."
            return
        }
        let permanent = !plan.files.isEmpty && !trash.available(folder)
        guard asking || permanent else { return delete(plan) }
        let question = plan.question(name: library.name(of:), permanent: permanent)
        let alert = AppAlert(title: question.message, detail: question.information,
                             buttons: [.init(title: question.button, role: .destructive), .init(title: "Cancel", role: .cancel)])
        Task { [weak self] in
            guard await self?.ask(alert).button == 0 else { return }
            self?.delete(plan, permanent: permanent)
        }
    }

    /// Moves `plan`'s photographs to the Trash with their sidecars, and
    /// removes its duplicates from theirs, as one undo step. If what was
    /// current goes, the next photograph shown becomes current.
    func delete(_ plan: DeletePlan, permanent: Bool = false) {
        guard canDelete, !plan.isEmpty else { return }
        // The open photograph's last edits go to its sidecar first, and so
        // to the Trash with it, or into the snapshot Undo puts back.
        flushSidecar()
        var gone = Set(plan.items)
        for file in plan.files { for id in library.duplicates[file] ?? [] { gone.insert(file.variant(id)) } }
        // One of the two going to the Trash ends compare.
        if let comparison, gone.contains(comparison.first.item) || gone.contains(comparison.second.item) { leaveCompare() }
        let current = mode == .grid ? selection.current : sourceItem
        let next = current.flatMap { current -> URL?? in
            guard gone.contains(current) else { return nil }
            return .some(Self.next(after: current, in: shownPhotos.map(\.url), skipping: gone)
                ?? Self.next(after: current, in: library.items.map(\.url), skipping: gone))
        }
        let open = sourceItem, openGone = sourceItem.map(gone.contains) ?? false
        // Nothing more is written for it.
        if openGone { sidecarWrite?.cancel(); sidecarWrite = nil; photoSidecar = nil }

        // A duplicate of a photograph that stays leaves its sidecar.
        var before: [URL: JSONValue?] = [:], failure: String?
        if let sidecars, sidecars.writes {
            for item in plan.duplicates {
                guard let id = item.variantID else { continue }
                let photo = item.photoFile
                do {
                    guard let raw = try sidecars.readPending(photo: photo) else { continue }
                    let removed = try PhotoSidecar.removing(variant: id, from: raw)
                    if before[photo] == nil { before[photo] = .some(raw) }
                    sidecars.enqueue(SidecarPatch(variant: id, replacement: .some(removed), defaultsVersion: SidecarDefaults.latest,
                                                  app: Self.appVersion), for: photo)
                } catch {
                    failure = failure ?? "“\(library.name(of: item))” couldn’t be removed: its .negs file can’t be changed."
                }
            }
            for photo in before.keys { library.reload(photo) }
        }
        library.remove(plan.files)

        // The selection moves on.
        switch next {
        case .some(.some(let next)):
            if mode == .grid { selection.select(next) } else { self.open(next, restoring: nil) }
        case .some(.none):
            // Nothing is left to show.
            forgetPhotograph()
            selection = LibrarySelection(); mode = .photo
        case .none:
            selection.prune(to: Set(library.items.map(\.url)))
        }
        if openGone, mode == .grid { sourceDeleted = true }

        let record = TrashRecord()
        if !permanent || !before.isEmpty {
            let reopen = openGone ? open : nil
            undoManager?.registerUndo(withTarget: libraryUndo) { [weak self] _ in
                MainActor.assumeIsolated { self?.undoDelete(permanent ? plan.withoutFiles : plan, record: record, sidecars: before, reopen: reopen) }
            }
            undoManager?.setActionName(plan.actionName)
        }
        let files = plan.files, companions = plan.companions, trash = self.trash, sidecars = self.sidecars, removing = failure
        record.task = Task { [weak self] in
            // Everything queued is written first, so nothing writes a sidecar
            // beside a photograph that's gone.
            if let failures = await sidecars?.flush() { for (failed, reason) in failures { self?.sidecarFailed(failed, reason) } }
            let moved = await Task.detached(priority: .userInitiated) { () -> (moves: [(original: URL, trashed: URL)], failure: String?) in
                var moves: [(original: URL, trashed: URL)] = [], failure: String?
                for file in files {
                    let sidecar = PhotoSidecar.url(for: file)
                    // A RAW's camera JPEG goes with it.
                    let picture = companions[file].map { [$0, PhotoSidecar.url(for: $0)] } ?? []
                    for item in [file, sidecar] + picture where item == file || FileManager.default.fileExists(atPath: item.path) {
                        do {
                            if permanent { try trash.remove(item) } else { moves.append((item, try trash.trash(item))) }
                        } catch {
                            failure = failure ?? "“\(file.lastPathComponent)” couldn’t be deleted. \(error.localizedDescription)"
                            break
                        }
                    }
                }
                return (moves, failure)
            }.value
            record.moves = moved.moves
            guard let self else { return }
            if let reason = moved.failure ?? removing { self.error = reason }
            self.library.unhide(files)
        }
        if files.isEmpty, let failure { error = failure }
    }

    /// The first photograph after `item` in `order` that isn't `gone`, or
    /// else the last one before it.
    nonisolated static func next(after item: URL, in order: [URL], skipping gone: Set<URL>) -> URL? {
        guard let index = order.firstIndex(of: item) else { return order.first { !gone.contains($0) } }
        return order[index...].first { !gone.contains($0) } ?? order[..<index].last { !gone.contains($0) }
    }

    /// Puts back what `plan` deleted: the files from the Trash and the
    /// duplicates' sidecars as they were. What comes back is selected; the
    /// edit view opens the photograph it showed.
    private func undoDelete(_ plan: DeletePlan, record: TrashRecord, sidecars before: [URL: JSONValue?], reopen: URL?) {
        guard !closing else { return }
        // Redo deletes them again, without asking.
        undoManager?.registerUndo(withTarget: libraryUndo) { [weak self] _ in MainActor.assumeIsolated { self?.delete(plan) } }
        undoManager?.setActionName(plan.actionName)
        let sidecars = self.sidecars
        for (photo, raw) in before {
            sidecars?.enqueue(SidecarPatch(variant: "", replacement: .some(raw), defaultsVersion: SidecarDefaults.latest, app: Self.appVersion), for: photo)
            library.reload(photo)
        }
        Task { [weak self] in
            await record.task?.value
            let moves = record.moves
            record.moves = []
            let failure = await Task.detached(priority: .userInitiated) { () -> String? in
                var failure: String?
                for move in moves.reversed() {
                    do { try FileManager.default.moveItem(at: move.trashed, to: move.original) } catch {
                        failure = failure ?? "“\(move.original.lastPathComponent)” couldn’t be put back. \(error.localizedDescription)"
                    }
                }
                return failure
            }.value
            if let failures = await sidecars?.flush() { for (failed, reason) in failures { self?.sidecarFailed(failed, reason) } }
            guard let self, !self.closing else { return }
            if let failure { self.error = failure }
            self.library.unhide(plan.files)
            let restored = plan.items.filter { FileManager.default.fileExists(atPath: $0.photoFile.path) }
            if self.mode == .photo, let reopen, restored.contains(reopen) || restored.contains(reopen.photoFile) {
                self.open(reopen, restoring: nil)
            } else if self.source == nil, let first = restored.first {
                self.open(first, restoring: nil)
            }
            if self.mode == .grid, !restored.isEmpty { self.selection.select(restored) }
        }
    }

    /// After changing several sidecars: the library shows them, one undo
    /// step puts back `before`, and they're written.
    private func finishSidecarChange(_ before: [URL: JSONValue?], action: String, openChanged: Bool) {
        guard !before.isEmpty else { return }
        for photo in before.keys { library.reload(photo) }
        // The open photograph's own history is of edits that are gone.
        if openChanged { undoManager?.removeAllActions(withTarget: self) }
        registerSidecarUndo(before, action: action)
        writeSidecars()
    }

    private func registerSidecarUndo(_ files: [URL: JSONValue?], action: String) {
        undoManager?.registerUndo(withTarget: libraryUndo) { [weak self] _ in
            MainActor.assumeIsolated { self?.restoreSidecars(files, action: action) }
        }
        undoManager?.setActionName(action)
    }

    /// Puts `files` back as they were (undo), keeping them as they are now
    /// for redo. The open photograph shows its edits as restored; a
    /// duplicate that's gone gives way to its photograph.
    private func restoreSidecars(_ files: [URL: JSONValue?], action: String) {
        guard let sidecars, !closing else { return }
        sidecarWrite?.cancel(); sidecarWrite = nil
        var now: [URL: JSONValue?] = [:]
        for (photo, raw) in files {
            let current: JSONValue?
            do { current = try sidecars.readPending(photo: photo) } catch { continue }
            now[photo] = .some(current)
            sidecars.enqueue(SidecarPatch(variant: "", replacement: .some(raw), defaultsVersion: SidecarDefaults.latest, app: Self.appVersion), for: photo)
            library.reload(photo)
        }
        registerSidecarUndo(now, action: action)
        if let current = selection.current, let id = current.variantID, library.duplicates[current.photoFile]?.contains(id) != true {
            selection.select(current.photoFile)
        }
        if let source, files.keys.contains(source) {
            if let id = sourceVariant, library.duplicates[source]?.contains(id) != true {
                open(source, restoring: nil)
            } else {
                cancelBrightnessMatch(); brightnessMatchFailure = nil
                customFilmFormat = false
                adoptEdits(for: source, restore: readSidecar(source, variant: sourceVariant), legacy: nil, previous: state)
                undoManager?.removeAllActions(withTarget: self)
                changed()
            }
        }
        writeSidecars()
    }

    /// Writes every queued sidecar change.
    private func writeSidecars() {
        guard let sidecars else { return }
        Task { [weak self] in
            let failures = await sidecars.flush()
            for (failed, reason) in failures { self?.sidecarFailed(failed, reason) }
        }
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
    /// The decoded original is the camera JPEG.
    @ObservationIgnored private var originalIsCameraJPEG = false
    private func dropStaleOriginal() {
        if originalLensCorrection != value(Self.lensCorrectionPath).bool || originalIsCameraJPEG != showsCameraJPEG { originalCache = nil }
    }

    /// The open photograph's camera JPEG (RAW+JPEG shooting), if it has one.
    var cameraJPEG: URL? { isRaw ? sourceItem.flatMap(library.companion(of:)) : nil }
    /// Original shows the camera JPEG rather than the RAW.
    var showsCameraJPEG: Bool { originalReference == .cameraJPEG && cameraJPEG != nil }
    /// What Original is called: the word in the status line, the compare
    /// side's name, Loading ….
    var originalWord: String { showsCameraJPEG ? "Camera JPEG" : "Original" }
    /// The same within a sentence: "original" or "camera JPEG".
    var originalPhrase: String { showsCameraJPEG ? "camera JPEG" : "original" }

    /// Decodes the original as Original shows it, at the developed preview's
    /// resolution (so it is as sharp at actual size), never above the
    /// source's own: Image Size can enlarge the preview.
    func originalLoader() -> (@Sendable () -> IOSurface?)? {
        guard let source, let image else { return nil }
        let sourceLong = Int(max(sourceSize.width, sourceSize.height))
        let maxPixels = sourceLong > 0 ? min(max(image.width, image.height), sourceLong) : max(image.width, image.height)
        if showsCameraJPEG, let picture = cameraJPEG {
            let frame = sourceSize.width > 0 ? sourceSize : CGSize(width: image.width, height: image.height)
            return { OriginalImage.loadCameraPicture(picture, frame: frame, maxPixels: maxPixels) }
        }
        let raw = isRaw, lensCorrection = value(Self.lensCorrectionPath).bool
        return { OriginalImage.load(source, raw: raw, lensCorrection: lensCorrection, maxPixels: maxPixels) }
    }

    /// Show the RAW / Show the camera JPEG changed: a split line or held
    /// original shows the other one.
    private func originalReferenceChanged() {
        guard cameraJPEG != nil else { return }
        originalLoad?.cancel(); originalLoad = nil
        originalCache = nil
        if splitting { splitOriginal = nil; loadOriginal() }
        if holdingOriginal { original = nil; loadOriginal() }
        reloadComparedOriginal()
    }

    /// The original is asked for, held or split, and still being decoded:
    /// the photograph's tag says Loading original meanwhile.
    var loadingOriginal: Bool {
        (holdingOriginal && original == nil) || (splitting && splitOriginal == nil)
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
                    guard let self, !self.holdingOriginal, !self.splitting, self.originalCache === shown else { return }
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
        guard originalLoad == nil, let source, let load = originalLoader() else { return }
        let generation = sourceGeneration
        let lensCorrection = value(Self.lensCorrectionPath).bool, cameraJPEG = showsCameraJPEG, word = originalPhrase
        originalLoad = Task { [weak self] in
            let surface = await Task.detached(priority: .userInitiated, operation: load).value
            guard let self, generation == self.sourceGeneration, !Task.isCancelled else { return }
            self.originalLoad = nil
            self.originalLensCorrection = lensCorrection
            self.originalIsCameraJPEG = cameraJPEG
            guard let surface else {
                if self.splitting { self.splitting = false }
                if self.holdingOriginal {
                    self.holdingOriginal = false
                    self.status = "The \(word) of \(source.lastPathComponent) couldn’t be shown"
                }
                return
            }
            self.originalCache = surface
            if self.holdingOriginal { self.original = surface }
            if self.splitting { self.splitOriginal = surface }
            if !self.holdingOriginal, !self.splitting { surface.makeVolatile() }
        }
    }

    /// A click on Original, by Original ⌄'s choice: holding is the press
    /// itself; Split line toggles the line; Side by side compares.
    func clickOriginal() {
        switch originalMode {
        case .hold: break
        case .split: splitting ? clearSplit() : startSplit()
        case .sideBySide: compareWithOriginal()
        }
    }

    /// The split line over the photograph, with the original left of it.
    private func startSplit() {
        guard canShowOriginal, comparison == nil, !splitting else { return }
        splitting = true
        dropStaleOriginal()
        if let cached = originalCache {
            if cached.makeNonVolatile() { splitOriginal = cached; return }
            originalCache = nil
        }
        loadOriginal()
    }

    func clearSplit() {
        guard splitting else { return }
        splitting = false
        let shown = splitOriginal
        splitOriginal = nil
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            guard let self, let shown, !self.holdingOriginal, !self.splitting, self.originalCache === shown else { return }
            shown.makeVolatile()
        }
    }

    // MARK: Compare (ticket 46): the model's own state Compare.swift reads

    var enginePaths: EnginePaths? { engine?.paths }
    var closingForCompare: Bool { closing }
    var sourceDeletedForCompare: Bool { sourceDeleted }
    /// The open photograph has settled: nothing renders, solves, saves or
    /// exports, so the other photograph's develop can have the machine.
    var editedSettled: Bool {
        !rendering && pendingRender == nil && matchTask == nil && !interacting && !exporting && !saving && heldPreview == nil
            && (previewCurrent || renderFailed || source == nil)
    }
    /// Opens `item` for editing in compare: the photo view, and its session.
    func openForCompare(_ item: URL) {
        if mode == .grid { mode = .photo }
        open(item, restoring: nil)
    }
    func setPhotoMode() { mode = .photo }
    func restoreSelection(_ restored: LibrarySelection) { if selection != restored { selection = restored } }
    func pauseThumbnailsForCompare() {}
    func resumeThumbnailsAfterCompare() { pumpThumbnails() }

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

    /// Try Again, after a develop failed: the same edit, developed again.
    func retryDevelop() {
        guard renderFailed else { return }
        changed(invalidateCache: false)
    }

    private func changed(invalidateCache: Bool = true) {
        heldPreview = nil
        scheduleToneCurve()
        if invalidateCache { previewCache.invalidate(); showingCachedPreview = false }
        revision += 1; previewCurrent = false
        if developFailure != nil { developFailure = nil }
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
            self.pumpThumbnails()
        }
    }
    /// What a render develops: the export-size frame, or as many pixels as
    /// the canvas shows, while a slider moves (`drag`) or ahead of the
    /// export-size frame of a discrete edit (`quick`).
    enum RenderStep: Equatable {
        case settled, drag(CGFloat), quick(CGFloat)
        var interactiveScale: CGFloat? {
            switch self { case .settled: nil; case .drag(let scale), .quick(let scale): scale }
        }
        var isDrag: Bool { if case .drag = self { true } else { false } }
        var isQuick: Bool { if case .quick = self { true } else { false } }
    }
    /// A quick frame only pays when the export-size render takes a while
    /// (about 33 ms per megapixel on the GPU, so ~150 ms here) and the canvas
    /// shows at most half of its pixels.
    nonisolated static let quickFrameMinimumPixels = 4.5e6
    nonisolated static let quickFrameMaximumPixelFraction = 0.5

    /// `showsFinishedFrame`: the canvas shows this edit's developed frame
    /// already (a restored launch frame, or a photograph's kept last
    /// develop), which a quick frame would only blur. A new photograph's
    /// first frame is quick too (ticket 39).
    nonisolated static func renderStep(interacting: Bool, screenScale: CGFloat?, framePixels: Double,
                                       showsFinishedFrame: Bool, quickFrameDone: Bool, reusesHelperOutput: Bool) -> RenderStep {
        let scale = screenScale.flatMap { $0 > 0 && $0 < 1 ? $0 : nil }
        if interacting { return scale.map(RenderStep.drag) ?? .settled }
        guard let scale, !showsFinishedFrame, !quickFrameDone, !reusesHelperOutput,
              framePixels >= quickFrameMinimumPixels,
              Double(scale * scale) <= quickFrameMaximumPixelFraction else { return .settled }
        return .quick(scale)
    }
    /// The parts of a render request the helper's reuse depends on.
    nonisolated static func reuseKey(_ command: [String: JSONValue]) -> [String: JSONValue] {
        var key = command
        key["preview_path"] = nil
        key["params"]?["post"] = .null
        return key
    }

    private func render() async {
        #if DEBUG
        willRender?()
        #endif
        guard !rendering, !closing, let source, let engine else { return }
        // The edit's settled frame shows already: a render scheduled before
        // it landed (on a slider's release) would only replace it. Every
        // edit clears `previewCurrent`.
        guard !previewCurrent else { return }
        let currentRevision = revision, snapshot = renderState, generation = sourceGeneration
        let frame = FrameID(revision: currentRevision, generation: generation)
        // A quick frame's settled render waits for a solve that started after
        // it was scheduled (matching turned on while the quick frame shows,
        // which at exposure 0 is no new edit): the old exposure never renders
        // at export size. The solve's end settles it (`settleQuickFrame`).
        if quickStep == frame, !previewCurrent, !interacting, matchTask != nil || heldPreview != nil { return }
        // A pending brightness match is solved first, on a decode alone, so
        // the old exposure is never rendered (ticket 39). The solve's end
        // renders the matched edit (`renderAfterMatch`).
        if !interacting, !cropping, brightnessMatchPending, matchDecodeFailed != frame {
            reconcileBrightnessMatch()
            if matchTask != nil { return }
        }
        // A photograph's first frame: its decode first, so the quick frame
        // that follows knows the photograph's size and the canvas has laid
        // out its placeholder, whose scale it renders at (ticket 39). The
        // helper keeps the decode for the render. Not when its kept last
        // develop or its restored frame shows: those go straight to export size.
        let firstFrame = image == nil
        let showsFinishedFrame = showingCachedPreview || firstFrame && placeholderFor.map { $0.generation == generation && $0.kept } == true
        if firstFrame, !showsFinishedFrame, !interacting, decodedFor?.generation != generation {
            rendering = true
            let decoded = try? await engine.request(decodeOnlyCommand(source: source, generation: generation, state: snapshot, engine: engine)).1
            rendering = false
            // A failed decode leaves its error to the render.
            if generation == sourceGeneration { decodedFor = (generation, decoded.map { CGSize(width: $0["source_width"].double, height: $0["source_height"].double) } ?? .zero) }
            guard !closing else { return }
            guard revision == currentRevision, generation == sourceGeneration else { return scheduleRender() }
            // Matching turned on during the decode (at exposure 0, no new
            // edit) is solved first too.
            if !interacting, !cropping, brightnessMatchPending, matchDecodeFailed != frame {
                reconcileBrightnessMatch()
                if matchTask != nil { return }
            }
        }
        let buffer = engine.directory.appendingPathComponent("preview.rgba")
        var command: [String: JSONValue] = [
            "command": .string("render"), "input": .string(source.path),
            "source_generation": .number(Double(generation)),
            "film": .string(snapshot.film), "paper": .string(snapshot.paper), "params": snapshot.params,
            "limit_preview": .bool(snapshot.limitPreview), "preview_path": .string(buffer.path)
        ]
        // The target describes the cropped photograph; the helper sizes the frame from the crop.
        if ImageSize.mode(of: snapshot.params) == .longestSide, let crop = Self.cropJSON(snapshot.crop) { command["crop"] = crop }
        let reuseKey = Self.reuseKey(command)
        // While a slider moves, render only as many pixels as the canvas
        // shows; the frame on release is the export-size one. A discrete
        // edit shows such a frame first, unless the helper can answer
        // quickly anyway: a small photograph, or post adjustments alone.
        // A first frame's canvas scale is the placeholder's, once it shows
        // this photograph at its size (a test's fixed scale stands in).
        let placeholderShown = placeholderFor.map { $0.generation == generation && !$0.kept } == true
        let screenScale = !firstFrame ? canvasScale.screenPixelsPerPhotoPixel
            : canvasScale.fixed ?? (placeholderShown ? canvasScale.screenPixelsPerPhotoPixel : nil)
        let frameSize = !firstFrame ? imageSize : decodedFor?.generation == generation ? decodedFor?.size ?? .zero : .zero
        let step = Self.renderStep(
            interacting: interacting, screenScale: screenScale,
            framePixels: Double(frameSize.width * frameSize.height),
            showsFinishedFrame: showsFinishedFrame, quickFrameDone: quickStep == frame,
            reusesHelperOutput: helperOutput.map { $0.key == reuseKey && $0.launch == engine.runningLaunch } ?? false)
        let interactiveScale = step.interactiveScale
        if let interactiveScale { command["interactive_scale"] = .number(Double(interactiveScale)) }
        rendering = true
        defer {
            rendering = false
            // The quick frame is done: the next render settles. Once that
            // ran, failed or not, nothing waits to settle (no retries).
            if revision == currentRevision, generation == sourceGeneration {
                if step.isQuick { quickStep = frame } else if step == .settled { quickStep = nil }
            }
            if step.isDrag, !interacting, !closing { scheduleRender() }
            if revision != currentRevision, !closing { scheduleRender() }
            // A match that went stale while this rendered (a drag's release,
            // matching turned on) is solved now, before the next render.
            reconcileBrightnessMatch()
            settleQuickFrame()
        }
        do {
            // The cache's name stats the helper's data (~18 ms): worked out
            // while the helper renders, not before (ticket 39).
            let cacheKeyTask = interactiveScale == nil ? Task.detached(priority: .utility) {
                RenderedPreviewCache.key(source: source, state: snapshot, paths: engine.paths)
            } : nil
            guard shouldShow(currentRevision, generation) else { cacheKeyTask?.cancel(); return }
            // The helper replaces its kept output with each render, and keeps
            // none if this one fails.
            helperOutput = nil
            #if DEBUG
            renderLog.append(step)
            #endif
            let (id, result, launch) = try await engine.requestOnLaunch(command)
            if step == .settled { helperOutput = (reuseKey, launch) }
            // A quick frame of an earlier edit still shows while edits keep
            // coming (Undo held down); the current one renders next.
            let quick = step.isQuick
            // A canvas-size frame never replaces the current edit's settled one.
            func showable() -> Bool {
                (quick ? !closing && generation == sourceGeneration : shouldShow(currentRevision, generation))
                    && (step == .settled || !previewCurrent)
            }
            guard showable() else { return }
            let width = Int(result["width"].double), height = Int(result["height"].double)
            let display = result["display_color_space"].string
            let developed = try await Task.detached(priority: .userInitiated) {
                try Self.readPreview(buffer, width: width, height: height, displaySpace: display)
            }.value
            let cacheKey = await cacheKeyTask?.value ?? nil
            guard showable() else { return }
            // The brightness match reads these; the rest waits for the frame.
            sourceSize = CGSize(width: result["source_width"].double, height: result["source_height"].double)
            isRaw = result["raw"].bool
            let show = { [weak self] in
                guard let self else { return }
                imageSize = CGSize(width: result["photo_width"].double, height: result["photo_height"].double)
                workingSize = CGSize(width: result["working_width"].double, height: result["working_height"].double)
                if interactiveScale == nil {
                    previewScale = workingSize.width > 0 ? min(Double(width) / workingSize.width, 1) : 1
                }
                showingInteractiveFrame = interactiveScale != nil
                quickFrame = quick ? frame : nil
                image = developed; placeholder = nil; standIn = nil
                #if DEBUG
                shownFrame = (frame, step)
                #endif
                if revision == currentRevision { trialImage = nil }
                updateHistogram()
                let film = FilmHistogram(result["film_histogram"])
                if film != filmHistogram { filmHistogram = film }
                cropSession?.update(photo: shownImageSize, rotation: imageRotation, flip: imageFlip)
                updateFilmStrip()
                if inputColour != result["input_colour"] { inputColour = result["input_colour"] }
                lensCorrectionSupported = result["lens_correction_supported"].bool
                noteFlatField(result["flat_field"])
                if revision == currentRevision, interactiveScale == nil {
                    renderID = id; previewCurrent = true; hideActivity()
                    surveyNegativeIfNeeded()
                    showingCachedPreview = false
                    renderAtExportSize = result["export_resolution"].bool
                    renderOnGPU = ExportEngine.ranOnGPU(backend: result["backend"].string)
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
            }
            // A settled or quick frame with a stale match would show the old
            // exposure for as long as the solve and the next render take: keep
            // the previous frame and the activity badge up until the match is
            // in. A stale match is solved before rendering, so this is only a
            // match that went stale while the frame rendered (turned on at
            // exposure 0, which is no new edit).
            // Not for an edit whose match decode failed: no solve would show it.
            if !step.isDrag, revision == currentRevision, !interacting, !cropping, brightnessMatchPending, matchDecodeFailed != frame {
                heldPreview = (currentRevision, generation, show)
            } else {
                show()
            }
        } catch {
            // A failed quick frame leaves the error to the export-size render that follows.
            if step.isQuick { return }
            if currentRevision == revision, generation == sourceGeneration, !closing {
                // Said on the canvas and in the status line, not in an alert (§6.10).
                developFailure = error.localizedDescription; hideActivity()
            }
        }
    }

    /// After a quick frame of the current edit, renders it at export size
    /// once no brightness match is pending or running: a match that changes
    /// the exposure is a new edit, with its own quick frame. Called wherever
    /// one of these conditions can clear; does nothing until all hold. A
    /// slider's release renders by itself.
    private func settleQuickFrame() {
        let frame = FrameID(revision: revision, generation: sourceGeneration)
        guard quickStep == frame, !previewCurrent, !renderFailed, !interacting, !rendering, pendingRender == nil,
              matchTask == nil, heldPreview == nil, ready, source != nil, !closing else { return }
        scheduleRender()
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
        guard let image else { histogram = nil; waveform = nil; return }
        let request = histogramRequest, crop = crop, rotation = imageRotation, flip = imageFlip
        Task.detached(priority: .userInitiated) { [weak self] in
            let counted = ImageScopes.of(image, crop: crop, rotation: rotation, flip: flip)
            await self?.showHistogram(counted, request: request)
        }
    }
    private func showHistogram(_ counted: ImageScopes, request: Int) {
        guard request == histogramRequest else { return }
        histogram = counted.histogram
        waveform = counted.waveform
    }

    // MARK: Tone graph

    func toneGraphAppeared(_ shown: Bool) {
        toneGraphsShown = max(0, toneGraphsShown + (shown ? 1 : -1))
        scheduleToneCurve()
    }

    /// Asks the helper for the current settings' curve, one request at a
    /// time; edits made meanwhile ask again once it answers.
    private func scheduleToneCurve() {
        guard toneGraphsShown > 0, hasCatalog, engine != nil, !closing else { return }
        guard toneCurveTask == nil else { toneCurveStale = true; return }
        toneCurveTask = Task { [weak self] in
            guard let self else { return }
            repeat {
                toneCurveStale = false
                let snapshot = renderState
                let request: JSONValue = .object(["film": .string(snapshot.film), "paper": .string(snapshot.paper), "params": snapshot.params])
                guard toneCurve?.request != request, let engine else { continue }
                do {
                    let (_, result) = try await engine.request([
                        "command": .string("tone_curve"), "film": .string(snapshot.film),
                        "paper": .string(snapshot.paper), "params": snapshot.params, "count": .number(256)])
                    let controls = ToneControls { snapshot.params[$0] }
                    if let curve = ToneCurveData(result, request: request, controls: controls) { toneCurve = curve }
                } catch {
                    // A curve that can't be computed leaves the last one; the render reports errors.
                }
            } while toneCurveStale && !closing
            toneCurveTask = nil
        }
    }

    /// Only mention an update once it has visibly taken a while.
    private func scheduleActivity() {
        guard activityTimer == nil, !showsActivity else { return }
        activityTimer = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            // A tried stock's tile is up: the tag only once the develop runs long.
            if self?.trialImage != nil { try? await Task.sleep(for: .milliseconds(1100)) }
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
        if !enabled { endTray(keep: true) }
        previews.enabled = enabled
        if enabled { pumpPreviews(); return }
        if previews.clear(), let engine {
            Task.detached(priority: .utility) { _ = try? await engine.request(["command": .string("release_thumbnails")]) }
        }
    }

    /// Settings › Keep paper when changing film. The film tiles show what a
    /// click would develop, so they go stale (their key holds the paper).
    func setKeepPaper(_ keep: Bool) {
        guard keep != previews.keepPaper else { return }
        previews.keepPaper = keep
        pumpPreviews()
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
        if previews.filmOpen, previews.filmShown {
            let listed = Set(listedFilms.map(\.id))
            let readAs = tray?.readAs ?? false
            entries += previews.filmsFavoritesFirst.filter { listed.contains($0.stock.id) && (!readAs || NegativeScan.readable($0.stock)) }
        }
        if previews.paperOpen, previews.paperShown, !scanning { entries += previews.papersFavoritesFirst }
        for entry in entries {
            let command = PreviewRules.request(for: entry.stock, keepPaper: previews.keepPaper, state: rendered, stocks: stocks,
                                               source: source, generation: generation, negative: tray?.readAs ?? false,
                                               negativePoint: negativeTilePoint)
            wanted.append((entry.tile, PreviewTile.Key(generation: generation, request: command), command, entry.stock.id))
        }
        if previews.presetsShown, checkingPresets == 0 {
            let kept = catalog["preset_kept"].array.map(\.string)
            // A preset keeps a negative's white balance: none decodes again.
            let negative = rendered.params["workflow.route"].string == NegativeScan.route
            let ordered = PreviewRules.renderOrder(presets, raw: isRaw, whiteBalance: { negative ? nil : self.presetWhiteBalances[$0] },
                                                        photo: rendered.params["io.raw_white_balance"])
            for preset in ordered {
                guard case .valid = presetStatus[preset], let text = presetTexts[preset], let tile = previews.tile(preset: preset.url) else { continue }
                let command = PreviewRules.presetCommand(text: text, state: rendered, source: source, generation: generation)
                wanted.append((tile, PreviewRules.presetKey(command, kept: kept.isEmpty ? nil : kept, generation: generation), command, preset.name))
            }
        }
        guard !wanted.isEmpty else { return }
        var next: (tile: PreviewTile, key: PreviewTile.Key, command: [String: JSONValue], name: String)?
        for item in wanted {
            if item.tile.mark(wanted: item.key), next == nil { next = item }
        }
        guard !previews.busy, ready, previewCurrent, !rendering, pendingRender == nil, !interacting, let engine else { return }
        guard let (tile, key, request, name) = next else { return }
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

    // MARK: Camera brightness

    /// Prep › Match camera brightness for this photograph.
    var matchesBrightness: Bool { state.brightnessMatch != nil }
    /// Why matching can't run for this photograph, or nil.
    var brightnessMatchUnavailable: String? {
        if let failure = brightnessMatchFailure, failure.generation == sourceGeneration, failure.inputs == nil { return failure.message }
        if !isRaw, state.params["io.input_interpretation.mode"].string != "auto" {
            return "Matching reads the file’s own colour tags. Set Input Colour to Automatic to use it."
        }
        return nil
    }
    private var brightnessMatchFailedHere: Bool {
        guard let failure = brightnessMatchFailure, failure.generation == sourceGeneration, let inputs = failure.inputs else { return false }
        return inputs == matchInputs(state)
    }
    /// On, possible, and not failed for the current settings: the meter then
    /// only sets where the highlight limit starts.
    var brightnessMatchActive: Bool { matchesBrightness && brightnessMatchUnavailable == nil && !brightnessMatchFailedHere }
    /// On and not yet solved for the current settings. Save and Export wait.
    var brightnessMatchPending: Bool {
        guard brightnessMatchActive, let match = state.brightnessMatch else { return false }
        return match.ev == nil || match.inputs != matchInputs(state)
    }
    /// The note under the Exposure slider.
    var brightnessMatchNote: String? {
        guard matchesBrightness else { return nil }
        if let unavailable = brightnessMatchUnavailable { return unavailable }
        if brightnessMatchFailedHere, let failure = brightnessMatchFailure { return failure.message }
        if brightnessMatchPending || matchingBrightness { return "Matching…" }
        guard let match = state.brightnessMatch, let ev = match.ev else { return nil }
        let value = ev.formatted(.number.precision(.fractionLength(2)).sign(strategy: .always()))
        let reference = isRaw ? "the camera" : "the original"
        return match.highlightLimited
            ? "Starts at \(value) EV to match \(reference), held back to protect highlights."
            : "Starts at \(value) EV to match \(reference)."
    }

    /// Turns matching on (from the match, Exposure back to 0) or off (the
    /// brightness stays; Exposure then shows all of it). One undo step.
    func setMatchesBrightness(_ on: Bool) {
        guard on != matchesBrightness else { return }
        var next = state
        if on {
            next.brightnessMatch = BrightnessMatch()
            next.params["prep.exposure_ev"] = .number(0)
        } else {
            next.brightnessMatch = nil
        }
        apply(next, action: on ? "Match Camera Brightness" : "Stop Matching Camera Brightness")
    }

    /// The Exposure slider: the user's exposure above the match.
    private var matchedEV: Double { brightnessMatchActive ? state.brightnessMatch?.ev ?? 0 : 0 }
    /// Keeps the engine's total within its ±3 EV.
    var exposureOffsetRange: ClosedRange<Double> {
        let matched = matchedEV
        return (-3 - matched)...(3 - matched)
    }
    var exposureOffset: Binding<Double> {
        Binding(get: { self.value("prep.exposure_ev").double - self.matchedEV }, set: { value in
            let range = self.exposureOffsetRange
            let offset = min(max((value / 0.05).rounded() * 0.05, range.lowerBound), range.upperBound)
            let total = abs(offset) < 1e-9 ? self.matchedEV : self.matchedEV + offset
            self.set("prep.exposure_ev", .number(min(3, max(-3, total))))
        })
    }
    var exposureOffsetModified: Bool { abs(value("prep.exposure_ev").double - matchedEV) > 1e-6 }
    func resetExposureOffset() { set("prep.exposure_ev", .number(matchedEV)) }

    /// What the match depends on: everything that changes how bright the
    /// developed photograph is, except the brightness edits that are the
    /// user's own (Prep, and the enlarger's print exposure) and settings that
    /// can't change it. A change re-solves the match; Prep edits never do.
    func matchInputs(_ state: EditState) -> JSONValue {
        var params = state.params
        for path in ["prep", "enlarger.print_exposure", "post", "film_render.grain", "io.rotate_cw", "io.flip", "io.crop",
                     "io.crop_center", "io.crop_size", "io.output_color_space", "io.output_cctf_encoding",
                     "settings.serial_grain"] {
            params[path] = .null
        }
        return .object(["film": .string(state.film), "paper": .string(state.paper), "params": params])
    }

    private func cancelBrightnessMatch() {
        matchTask?.cancel(); matchTask = nil; matchRequest = nil; matchingBrightness = false
    }

    /// Solves the match when it's on and stale, before the edit renders
    /// (ticket 39): a decode alone first, which the helper keeps for the
    /// render (nearly free when it holds the photograph already), then the
    /// solve on it. One solve at a time; its completion looks again, so
    /// edits made meanwhile coalesce, then renders. A result is applied only
    /// if the inputs it was solved for are still current, without an undo
    /// step of its own: Undo restores a state together with the match solved
    /// for it, and a restored state whose match is stale is solved again.
    func reconcileBrightnessMatch() {
        // Cheap checks first: this runs on every edit, drag frames included.
        guard matchTask == nil, !interacting, !rendering, ready, !cropping, !closing,
              let engine, let source, brightnessMatchPending,
              matchDecodeFailed != FrameID(revision: revision, generation: sourceGeneration) else { return }
        let inputs = matchInputs(state)
        let id = UUID(), generation = sourceGeneration, frame = FrameID(revision: revision, generation: sourceGeneration)
        let request = matchSolveState(renderState)
        let reference = engine.directory.appendingPathComponent("camera-brightness-\(id.uuidString).png")
        // The camera preview is read once a photograph, while the helper
        // decodes; a RAW by its extension (the decode says for sure).
        if matchReference?.generation != generation {
            let raw = Self.rawExtensions.contains(source.pathExtension.lowercased())
            matchReference = (generation, nil, raw ? Task.detached(priority: .userInitiated) { CameraBrightnessReference.Prepared(from: source) } : nil)
        }
        let askDeclared = matchReference?.declared == nil && matchReference?.prepared != nil
        let prepared = matchReference?.prepared
        var decode = decodeOnlyCommand(source: source, generation: generation, state: request, engine: engine)
        if askDeclared { decode["preview_colour_space"] = .bool(true) }
        // Solved before on this file for this very request: the same value.
        let cache = matchCache, key = matchCacheKey(source, request: request, paths: engine.paths)
        matchRequest = id; matchingBrightness = true
        matchTask = Task { [weak self] in
            defer {
                try? FileManager.default.removeItem(at: reference)
                if let self, self.matchRequest == id {
                    self.matchTask = nil; self.matchRequest = nil; self.matchingBrightness = false
                    self.reconcileBrightnessMatch()
                    if self.matchTask == nil { self.showHeldPreview(); self.settleQuickFrame(); self.renderAfterMatch() }
                }
            }
            @MainActor func stillWanted() throws {
                guard let self, !self.closing, self.sourceGeneration == generation, self.matchRequest == id else { throw CancellationError() }
            }
            if let key, let kept = cache.solved(key) {
                guard (try? stillWanted()) != nil, let self else { return }
                self.applyBrightnessMatch(kept.ev, limited: kept.limited, inputs: inputs)
                return
            }
            #if DEBUG
            self?.matchSolves += 1
            #endif
            do {
                // The whole photograph, as a render decodes it, with the
                // pipeline the solve and the render use.
                let decoded: JSONValue
                do {
                    #if DEBUG
                    try self?.matchDecodeWillRun?()
                    #endif
                    decoded = try await engine.request(decode).1
                } catch {
                    try stillWanted()
                    // The render reports why, as it would without the match.
                    self?.matchDecodeFailed = frame
                    return
                }
                try stillWanted()
                let raw = decoded["raw"].bool
                let size = CGSize(width: decoded["source_width"].double, height: decoded["source_height"].double)
                self?.decodedFor = (generation, size)
                if askDeclared, let self, self.matchReference?.generation == generation {
                    self.matchReference?.declared = decoded["preview_colour_space"]
                }
                let declared: JSONValue? = raw ? (self?.matchReference).flatMap { $0.generation == generation ? $0.declared : nil } : nil
                let solved = try await Self.solveBrightnessMatch(engine: engine, source: source, generation: generation, size: size, raw: raw,
                                                                 request: request, reference: reference, declared: declared,
                                                                 prepared: raw ? prepared : nil, stillWanted: stillWanted)
                guard let self, !self.closing, self.sourceGeneration == generation, self.matchRequest == id else { return }
                // Kept unless the file changed while it was solved.
                if let key, key == self.matchCacheKey(source, request: request, paths: engine.paths) {
                    cache.store(key, ev: solved.exposure, limited: solved.limited)
                }
                self.applyBrightnessMatch(solved.exposure, limited: solved.limited, inputs: inputs)
            } catch is CancellationError {
                // A different photograph or shutdown supersedes this request.
            } catch {
                guard let self, !self.closing, self.sourceGeneration == generation, self.matchRequest == id,
                      self.matchInputs(self.state) == inputs else { return }
                let failure = CameraBrightnessReference.failure(error)
                self.brightnessMatchFailure = (generation, failure.forPhoto ? nil : inputs, failure.message)
            }
        }
    }

    /// Has the helper decode `source` whole, as a render would, without
    /// developing it (and build the pipeline `state` renders with).
    private func decodeOnlyCommand(source: URL, generation: Int, state: EditState, engine: EngineClient) -> [String: JSONValue] {
        [
            "command": .string("thumbnail"), "input": .string(source.path), "source_generation": .number(Double(generation)),
            "film": .string(state.film), "paper": .string(state.paper), "params": state.params,
            "long_side": .number(16), "preview_path": .string(engine.directory.appendingPathComponent("decode-only.rgba").path),
            "reduced_decode": .bool(false), "decode_only": .bool(true)
        ]
    }

    /// The match cache's name for solving `request` on `file` in the helper at `paths`.
    func matchCacheKey(_ file: URL, request: EditState, paths: EnginePaths) -> String? {
        if matchEngine?.helper != paths.helper || matchEngine?.data != paths.data {
            matchEngine = (paths.helper, paths.data, BrightnessMatchCache.engineStamp(paths))
        }
        return matchEngine.flatMap { BrightnessMatchCache.key(file: file, request: request, engine: $0.stamp) }
    }

    /// What the match is solved with: `state` as rendered, without the
    /// brightness edits that are the user's own.
    func matchSolveState(_ state: EditState) -> EditState {
        var request = state
        request.params["prep"] = defaults["prep"]
        request.params["enlarger.print_exposure"] = defaults["enlarger.print_exposure"]
        return request
    }

    /// Solves Match camera brightness for `source`, which `engine` holds
    /// decoded as `generation` (`size` pixels): the exposure that develops it
    /// as bright as the camera's preview, and whether highlights limited it.
    /// `stillWanted` throws to stop before the helper is asked. `declared`:
    /// the helper's `preview_colour_space` answer, if it was read already.
    nonisolated static func solveBrightnessMatch(engine: EngineClient, source: URL, generation: Int, size: CGSize, raw: Bool, request: EditState,
                                                 reference: URL, declared given: JSONValue? = nil, prepared: Task<CameraBrightnessReference.Prepared, Never>? = nil,
                                                 stillWanted: @MainActor () throws -> Void = {}) async throws -> (exposure: Double, limited: Bool) {
        // What the RAW says its preview is in, read by the engine as on the
        // web and Windows.
        let declared: PreviewColourSpace?
        if !raw {
            declared = nil
        } else if let given {
            declared = PreviewColourSpace(rawValue: given.string)
        } else {
            declared = PreviewColourSpace(rawValue: try await engine.request([
                "command": .string("preview_colour_space"), "input": .string(source.path)
            ]).1["colour_space"].string)
        }
        try Task.checkCancellation()
        let data = if raw, let prepared {
            try await prepared.value.png(expectedSize: size, declared: declared)
        } else {
            try await Task.detached(priority: .userInitiated) {
                raw ? try CameraBrightnessReference.png(from: source, expectedSize: size, declared: declared)
                    : try CameraBrightnessReference.filePNG(from: source, expectedSize: size)
            }.value
        }
        try Task.checkCancellation()
        try await stillWanted()
        try data.write(to: reference, options: .atomic)
        let (_, result) = try await engine.request([
            "command": .string("place_exposure"), "input": .string(source.path),
            "source_generation": .number(Double(generation)), "film": .string(request.film),
            "paper": .string(request.paper), "params": request.params,
            "reference_path": .string(reference.path)
        ])
        try Task.checkCancellation()
        try await stillWanted()
        guard case .number(let exposure) = result["exposure_ev"], exposure.isFinite, (-3...3).contains(exposure) else {
            throw EngineError(message: "No reliable brightness match with these settings.")
        }
        return (exposure, result["highlight_limited"].bool)
    }

    /// The match is in, or failed, without changing the render: the held
    /// frame is the current one. A change re-renders and drops it.
    private func showHeldPreview() {
        guard let held = heldPreview else { return }
        heldPreview = nil
        guard held.revision == revision, held.generation == sourceGeneration, !closing else { return }
        held.show()
        pumpPreviews()
        settleQuickFrame()
    }
    /// The shown frame is the settled render of its edit, not one rendered
    /// at canvas size (a slider's or a discrete edit's quick frame). It
    /// changes only with `image`, which the canvas observes.
    var showsSettledFrame: Bool { showingCachedPreview || (!showingInteractiveFrame && quickFrame == nil) }
    /// The solve is over and the edit it waited for is still unrendered:
    /// a match that moved the exposure rendered already (a new edit), so
    /// this is one it left as it was, or a failed one.
    private func renderAfterMatch() {
        guard ready, source != nil, !closing, !previewCurrent, !renderFailed, !rendering, pendingRender == nil,
              !interacting, heldPreview == nil else { return }
        scheduleRender()
    }

    /// Moves the total with the match, keeping the user's exposure above it.
    /// A change under 0.05 EV keeps the values and only marks them current.
    private func applyBrightnessMatch(_ ev: Double, limited: Bool, inputs: JSONValue) {
        guard matchInputs(state) == inputs, let next = Self.matching(state, ev: ev, limited: limited, inputs: inputs) else { return }
        replaceState(next)
    }

    /// `state` with its match solved as `ev`, for `inputs`.
    nonisolated static func matching(_ state: EditState, ev: Double, limited: Bool, inputs: JSONValue) -> EditState? {
        guard var match = state.brightnessMatch else { return nil }
        var next = state
        let offset = next.params["prep.exposure_ev"].double - (match.ev ?? 0)
        if match.ev.map({ abs(ev - $0) >= 0.05 }) ?? true {
            match.ev = ev
            next.params["prep.exposure_ev"] = .number(min(3, max(-3, abs(offset) < 1e-9 ? ev : ev + offset)))
        }
        match.inputs = inputs; match.highlightLimited = limited
        next.brightnessMatch = match
        return next
    }

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

    // MARK: Your negative (ticket 33)

    /// The photograph is a scan of a colour negative: upstream's
    /// convert-film route reads it back to the film's dyes, then prints.
    var isNegative: Bool { value("workflow.route").string == NegativeScan.route }
    /// Where the film base now shown came from, for Prep's Film base line:
    /// what the detection that made it said, else how it compares with
    /// what a detection finds on this reading (`negativeSurvey`). Nil while
    /// that isn't known yet. Undo, redo and resets bring back a base, so
    /// it's looked up by the base itself, never kept from the last answer.
    var negativeBase: NegativeScan.BaseStatus? {
        guard isNegative else { return nil }
        let base = state.params["film_render.base"]
        if let record = negativeBaseRecords.last(where: { $0.generation == sourceGeneration && NegativeScan.sameBase($0.base, base) }) {
            return record.status
        }
        // Put back to the default, to be found again.
        guard !NegativeScan.sameBase(base, NegativeScan.defaults["film_render.base"] ?? .null),
              let survey = negativeSurvey, survey.base != .null else { return nil }
        if NegativeScan.sameBase(survey.base, base) { return NegativeScan.BaseStatus(found: survey.found, picked: false, edge: survey.edge) }
        // Not what a detection finds: a frame of a roll without clear film
        // of its own has the roll's; otherwise it was picked or set by hand.
        return NegativeScan.BaseStatus(found: false, picked: false, from: state.roll != nil && !survey.found ? .roll : .hand)
    }
    /// What each base found or picked on this photograph was found from,
    /// newest last.
    private var negativeBaseRecords: [BaseRecord] = []
    private struct BaseRecord {
        var generation: Int
        /// `film_render.base` as the answer wrote it.
        var base: JSONValue
        var status: NegativeScan.BaseStatus
    }
    private func recordBase(_ base: JSONValue, _ status: NegativeScan.BaseStatus, generation: Int) {
        negativeBaseRecords.removeAll { $0.generation != generation || NegativeScan.sameBase($0.base, base) }
        negativeBaseRecords.append(BaseRecord(generation: generation, base: base, status: status))
        if negativeBaseRecords.count > 32 { negativeBaseRecords.removeFirst() }
    }
    /// A request about the negative is running (a base detection, a
    /// balance, Auto, a pick, Crop to picture, the roll): what Prep says
    /// meanwhile. Only the newest request clears it.
    private(set) var negativeWorking: String?
    /// The canvas takes the next click as a point to read.
    var negativePick: NegativeScan.Pick? {
        didSet { if negativePick != nil { endTray(keep: true) } }
    }
    /// Where the base was picked on this photograph, kept for a re-detect
    /// after a change of film, paper or scan light.
    @ObservationIgnored private var negativeBasePoint: (generation: Int, point: CGPoint)?
    /// Where the base was picked on this photograph, for the read-as tiles.
    private var negativeTilePoint: CGPoint? { negativeBasePoint.flatMap { $0.generation == sourceGeneration ? $0.point : nil } }
    /// Counts requests about the negative: the newest supersedes the others,
    /// so a late answer never lands on a newer edit or on a newer request's.
    @ObservationIgnored private var negativeRequest = 0
    /// The newest request finds the base (and balances the print for it).
    @ObservationIgnored private var negativeDetecting = false
    /// The newest request moves the filtration to another paper, from the
    /// paper it was balanced for (`from`), while it's running.
    @ObservationIgnored private var negativeRebalancing: (request: Int, from: String)?
    /// Answers folded into the edit that asked for them, newest last: undo
    /// or redo putting that edit back (one made before the answer came)
    /// puts its answer back with it.
    @ObservationIgnored private var negativeFolded: [NegativeFold] = []
    private struct NegativeFold {
        var generation: Int
        /// The edit as it was when it asked.
        var asked: EditState
        var changes: [String: JSONValue]
    }
    /// What the rebate finder read on this photograph, for each way of
    /// reading it lately, newest last.
    private var negativeSurveys: [NegativeScan.Survey] = []
    /// What the rebate finder read on this photograph as it's read now: the
    /// picture, whether it looks like a negative, the base a detection finds.
    var negativeSurvey: NegativeScan.Survey? {
        guard let key = negativeKey(state) else { return nil }
        return negativeSurveys.last { $0.generation == sourceGeneration && $0.key == key }
    }
    private func keepSurvey(_ survey: NegativeScan.Survey) {
        negativeSurveys.removeAll { $0.generation != survey.generation || $0.key == survey.key }
        negativeSurveys.append(survey)
        if negativeSurveys.count > 8 { negativeSurveys.removeFirst() }
    }
    /// Surveys running, by source generation and reading: never two alike.
    @ObservationIgnored private var negativeSurveying: Set<String> = []
    /// Your negative was tried in the tray: crop to the picture if it's kept.
    @ObservationIgnored private var negativeCropPending: Int?

    /// The film a negative is read as: the current one if it's a colour
    /// negative, else Portra 400 (decision 2: say so, no averaged profile).
    private func negativeFilm(for state: EditState) -> String? {
        if let film = films.first(where: { $0.id == state.film }), NegativeScan.readable(film) { return film.id }
        if films.contains(where: { $0.id == NegativeScan.defaultFilm }) { return NegativeScan.defaultFilm }
        return films.first(where: NegativeScan.readable)?.id
    }

    /// `state` read as a negative.
    func negativeState(from state: EditState) -> EditState {
        var next = state
        next.params["workflow.route"] = .string(NegativeScan.route)
        next.params["io.scan_film"] = .bool(false)
        if let film = negativeFilm(for: state), film != state.film {
            next = PreviewRules.choosing(film, paper: false, keepPaper: previews.keepPaper, in: next, stocks: stocks, negative: true)
            // Entering, from a slide say, the film's own paper comes too
            // (unless Keep paper); changing the read-as film later keeps it.
            PreviewRules.pairFilm(in: &next, stocks: stocks, paper: !previews.keepPaper)
        }
        // The camera's preview is of the orange negative: nothing to match.
        next.brightnessMatch = nil
        return next
    }

    /// Whether this photograph looks like a colour negative (the helper's
    /// `negative_check`), for its source generation: asked only when the
    /// film menu or tray opens, kept in memory so opening it again doesn't ask.
    @ObservationIgnored private(set) var looksNegative: (generation: Int, looks: Bool)?
    @ObservationIgnored private var checkingNegative: Int?

    /// The helper found the photograph looks like a colour negative: the
    /// film tray opens on Scan. False while that isn't known yet.
    var looksLikeANegative: Bool { looksNegative.map { $0.generation == sourceGeneration && $0.looks } ?? false }

    /// Asks whether the photograph looks like a colour negative, read as the
    /// film it would be read as, with its own white balance, lens correction
    /// and interpretation, from the source its preview decoded. Once per
    /// source generation; an open film tray moves to Scan on a yes.
    func checkLooksNegative() {
        let generation = sourceGeneration
        // Only once a develop has decoded the source, so the check never
        // decodes it again, smaller, for the develop to decode once more.
        guard looksNegative?.generation != generation, checkingNegative != generation, image != nil, !showingCachedPreview,
              let engine, let source, let film = negativeFilm(for: state) else { return }
        checkingNegative = generation
        let command: [String: JSONValue] = ["command": .string("negative_check"), "input": .string(source.path),
                                            "source_generation": .number(Double(generation)), "film": .string(film),
                                            "params": renderState.params]
        Task {
            defer { if checkingNegative == generation { checkingNegative = nil } }
            guard let (_, result) = try? await engine.request(command), generation == sourceGeneration else { return }
            looksNegative = (generation, result["looks_negative"].bool)
            if looksLikeANegative, let tray, !tray.paper, !tray.readAs { tray.opensOnScan = true }
        }
    }

    /// The label's "Your negative" outside the tray: reads the photograph
    /// as a negative, finds its base and balances the print, one undo step.
    func chooseNegative() {
        guard !isNegative, canChangeEdits else { return }
        apply(negativeState(from: state), action: "Read as Negative")
    }

    /// The film a negative is read as, from the plain menu.
    func chooseNegativeFilm(_ stock: String) {
        apply(PreviewRules.choosing(stock, paper: false, keepPaper: previews.keepPaper, in: state, stocks: stocks, negative: true),
              action: "Change Negative Film")
    }

    /// What a base was found for: changing any of it finds it again. The
    /// paper isn't: a new paper only balances the print again.
    private func negativeKey(_ state: EditState) -> String? {
        guard state.params["workflow.route"].string == NegativeScan.route else { return nil }
        return [state.film, state.params["film_render.convert.scan_illuminant"].string,
                "\(state.params["io.raw_white_balance"])", "\(state.params["io.input_interpretation"])",
                "\(state.params["io.lens_correction"])", state.params[NegativeScan.flatFieldPath].string].joined(separator: "|")
    }

    /// The helper's last word on the flat field (`flat_field` of a render
    /// or a base detection): `{path, used, reason}` and the photograph it
    /// was about (`input`), or null.
    private(set) var flatFieldAnswer: JSONValue = .null

    /// Prep's Flat field row: this frame's light frame and whether it was used.
    var flatField: NegativeScan.FlatField {
        let answer = flatFieldAnswer["input"].string == source?.path ? flatFieldAnswer : .null
        return NegativeScan.FlatField(path: value(NegativeScan.flatFieldPath).string, answer: answer)
    }

    /// Keeps the helper's answer about the flat field for this photograph,
    /// when it gave one.
    func noteFlatField(_ answer: JSONValue) {
        guard answer != .null, let source else { return }
        var answer = answer
        answer["input"] = .string(source.path)
        if answer != flatFieldAnswer { flatFieldAnswer = answer }
    }

    /// Prep › Flat field › Choose…: the light frame for this negative (and,
    /// with Use for whole roll, its roll). Finding the base again follows.
    func chooseFlatField(_ url: URL) {
        guard isNegative, canChangeEdits else { return }
        var next = state
        next.params[NegativeScan.flatFieldPath] = .string(url.path)
        apply(next, action: "Choose Flat Field")
    }

    /// Prep › Flat field › Off.
    func clearFlatField() {
        guard isNegative, canChangeEdits, flatField.isOn else { return }
        var next = state
        next.params[NegativeScan.flatFieldPath] = .string("")
        apply(next, action: "Flat Field Off")
    }

    func chooseFlatFieldPanel() {
        let panel = Self.makeOpenPanel()
        panel.title = "Choose Light Frame"
        panel.message = "A frame of the bare light, shot with this roll's camera, lens and aperture."
        panel.canChooseDirectories = false
        if let source { panel.directoryURL = source.deletingLastPathComponent() }
        panel.begin { [weak self] response in
            if response == .OK, let url = panel.url { self?.chooseFlatField(url) }
        }
    }
    /// What an answer about the base or balance was asked for.
    private func negativeAnswerKey(_ state: EditState) -> String? { negativeKey(state).map { "\($0)|\(state.paper)" } }

    /// Starts a request about the negative, superseding any running: their
    /// answers are dropped and they no longer clear `negativeWorking`.
    private func beginNegative(_ working: String, detecting: Bool = false) -> Int {
        negativeRequest += 1
        negativeWorking = working
        negativeDetecting = detecting
        return negativeRequest
    }

    /// `request` has ended: Prep's buttons come back if it's still the newest.
    private func endNegative(_ request: Int) {
        guard negativeRequest == request else { return }
        negativeWorking = nil
        negativeDetecting = false
        negativeRebalancing = nil
    }

    /// `request`'s answer still fits: it's the newest, for this photograph,
    /// and the negative is read as when it asked (`key`, `negativeAnswerKey`).
    private func negativeCurrent(_ request: Int, generation: Int, key: String?) -> Bool {
        negativeRequest == request && generation == sourceGeneration && negativeAnswerKey(state) == key
    }

    /// Folds `changes`, the answer to `asked`, into the current edit, and
    /// keeps them for an undo or redo that puts `asked` back.
    private func foldNegative(_ changes: [String: JSONValue], asked: EditState, generation: Int) {
        negativeFolded.removeAll { $0.generation != generation || $0.asked == asked }
        negativeFolded.append(NegativeFold(generation: generation, asked: asked, changes: changes))
        if negativeFolded.count > 16 { negativeFolded.removeFirst() }
        var next = state
        for (path, value) in changes { next.params[path] = value }
        replaceState(next)
    }

    /// Undo or redo put back a negative's edit: one undone before its answer
    /// came gets that answer back; one with a base never found (its answer
    /// was dropped) finds it now. Never an orange print.
    private func restoreNegativeAnswer() {
        guard isNegative else { return }
        if let fold = negativeFolded.last(where: { $0.generation == sourceGeneration && $0.asked == state }) {
            var next = state
            for (path, value) in fold.changes { next.params[path] = value }
            replaceState(next)
        } else if state.params["film_render.base"] == NegativeScan.defaults["film_render.base"] {
            detectNegativeBase(undoable: false)
        }
    }

    /// After a reset on a negative that put back its base, balance or print
    /// exposure (`paths`, nil for all): they're this frame's measurements,
    /// not settings with a default, so they're found again as part of the
    /// reset, unless the reset's own change of how the scan is read has a
    /// detection running already (`since`: `negativeRequest` before it).
    private func refindNegative(after before: EditState, paths: [String]?, since: Int) {
        guard isNegative, state != before else { return }
        if let paths, !paths.contains(where: NegativeScan.isFound) { return }
        if negativeRequest != since, negativeDetecting { return }
        detectNegativeBase(undoable: false)
    }

    /// After an edit: entering negative mode, or a new film, scan light or
    /// white balance, finds the base and balance again; a new paper (a
    /// preset's, say) balances the print again on the same base. Both are
    /// part of that edit (no undo step of their own). Entering also crops to
    /// the picture, as a step of its own.
    private func followNegative(from old: EditState) {
        guard let key = negativeKey(state) else {
            if negativeKey(old) != nil {
                negativePick = nil
                // Back to a scene: Match camera brightness as for a new photograph.
                if state.brightnessMatch == nil, autoPlaceNewImages, old.params["workflow.route"] != state.params["workflow.route"] {
                    var next = state; next.brightnessMatch = BrightnessMatch(); replaceState(next)
                }
            }
            return
        }
        // Undo and redo put back a state whose base and balance were found
        // for it already (a roll frame's filtration among them), or bring
        // its answer back (`restoreNegativeAnswer`).
        if undoManager?.isUndoing == true || undoManager?.isRedoing == true { restoreNegativeAnswer(); return }
        let before = negativeKey(old)
        if key != before {
            detectNegativeBase(undoable: false, cropToPicture: before == nil)
        } else if state.paper != old.paper {
            // A detection still running balances for the old paper: find
            // the base again, which balances for the new one.
            if negativeDetecting {
                detectNegativeBase(undoable: false)
            } else if Self.filtration(state) == Self.filtration(old) {
                // The filtration came along to the new paper: moved by the
                // papers' difference. One that came with the paper (Esc in
                // the tray putting the frame back) is the paper's already.
                rebalanceNegative(from: negativeRebalancing.flatMap { $0.request == negativeRequest ? $0.from : nil } ?? old.paper)
            }
        }
    }

    /// A negative's magenta and yellow.
    private static func filtration(_ state: EditState) -> [JSONValue?] {
        [state.params["enlarger.m_filter_shift"], state.params["enlarger.y_filter_shift"]]
    }

    /// The scan doesn't look like a colour negative, in Prep's words.
    var negativeWarning: String? {
        guard isNegative, let survey = negativeSurvey, survey.generation == sourceGeneration, !survey.looksNegative else { return nil }
        return NegativeScan.notNegative(raw: isRaw)
    }

    /// Finds the film base (or reads it at `point`, fractions of the whole
    /// photograph), balances the print for it and meters the print
    /// exposure. `undoable`: its own undo step (Detect again, a pick); else
    /// folded into the edit that asked, after a moment's wait so trying
    /// film after film reads only the last. `cropToPicture`: then crops to
    /// the picture found, as its own undo step, unless the photograph is cropped.
    func detectNegativeBase(at point: CGPoint? = nil, undoable: Bool = true, cropToPicture: Bool = false) {
        guard isNegative, let engine, let source else { return }
        let generation = sourceGeneration
        if let point { negativeBasePoint = (generation, point) }
        let point = point ?? negativeBasePoint.flatMap { $0.generation == generation ? $0.point : nil }
        let request = beginNegative(point == nil ? "Finding the film base…" : "Reading the base…", detecting: true)
        let asked = state, snapshot = renderState, key = negativeAnswerKey(state)
        var command = negativeRequestCommand("negative_base", source: source, generation: generation, state: snapshot)
        if let point { command["point"] = .array([.number(point.x), .number(point.y)]) }
        // Found without a point, the answer is this reading's survey too.
        let surveying = point == nil ? negativeKey(asked).map { "\(generation)|\($0)" } : nil
        if let surveying { negativeSurveying.insert(surveying) }
        // A frame of a roll keeps the roll's filtration (§9.2): its base is
        // measured afresh, its print exposure metered for that filtration.
        // On another paper than the roll's, it's balanced for its own paper.
        let roll = asked.roll, filtration = roll.flatMap { $0.paper == nil || $0.paper == asked.paper ? $0 : nil }
        Task {
            defer {
                endNegative(request)
                if let surveying { negativeSurveying.remove(surveying) }
            }
            do {
                if !undoable { try await Task.sleep(for: Self.negativeSettle) }
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                let (_, result) = try await engine.request(command)
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                noteFlatField(result["flat_field"])
                if point == nil { keepSurvey(NegativeScan.Survey(generation: generation, key: negativeKey(asked), result: result)) }
                var changes: [String: JSONValue] = [
                    "film_render.base": result["base"], "film_render.convert.exposure_compensation_ev": result["exposure_ev"],
                    "enlarger.m_filter_shift": filtration.map { .number($0.magenta) } ?? result["m_filter_shift"],
                    "enlarger.y_filter_shift": filtration.map { .number($0.yellow) } ?? result["y_filter_shift"],
                ]
                // Each frame gets its own print exposure, as a printer exposes each frame.
                if let exposure = await meterNegative(snapshot, changes, engine: engine, source: source, generation: generation) {
                    changes["enlarger.print_exposure"] = exposure
                }
                guard negativeCurrent(request, generation: generation, key: key), state.roll == roll else { return }
                let base = NegativeScan.BaseStatus(found: result["found"].bool, picked: result["picked"].bool,
                                                   edge: result["edge"].string.isEmpty ? nil : result["edge"].string)
                recordBase(result["base"], base, generation: generation)
                if undoable {
                    var next = state
                    for (path, value) in changes { next.params[path] = value }
                    apply(next, action: point == nil ? "Detect Film Base" : "Pick Film Base")
                } else {
                    foldNegative(changes, asked: asked, generation: generation)
                }
                status = base.sentence(as: Orientation(params: state.params))
                if cropToPicture {
                    // Trying Your negative in the tray: crop once it's kept.
                    if tray != nil { negativeCropPending = generation } else { cropToFoundPicture() }
                }
            } catch { if negativeRequest == request { report(error) } }
        }
    }

    /// The crop to the picture last found, as its own undo step, unless the
    /// photograph is cropped already (its crop is the user's).
    private func cropToFoundPicture() {
        if let cropped = croppedToFoundPicture() { apply(cropped, action: "Crop to Picture") }
    }

    /// The state cropped to the picture last found; nil if there's none or
    /// the photograph is cropped already.
    private func croppedToFoundPicture() -> EditState? {
        guard isNegative, !state.crop.cropped, canChangeEdits, let survey = negativeSurvey, survey.generation == sourceGeneration,
              let crop = NegativeScan.crop(picture: survey.picture) else { return nil }
        var cropped = state; cropped.crop = crop
        return cropped
    }

    /// A helper request about this negative, as it's read in `state`.
    private func negativeRequestCommand(_ name: String, source: URL, generation: Int, state: EditState) -> [String: JSONValue] {
        ["command": .string(name), "input": .string(source.path), "source_generation": .number(Double(generation)),
         "film": .string(state.film), "paper": .string(state.paper), "params": state.params]
    }

    /// Reads this photograph's film edge, as `state` reads the scan, if no
    /// base detection has: the picture, whether it looks like a negative
    /// and the base a detection finds (a negative opened again, say).
    /// Changes no setting.
    private func surveyNegative(engine: EngineClient, source: URL, generation: Int, state: EditState) async throws -> NegativeScan.Survey? {
        let key = negativeKey(state)
        if let survey = negativeSurveys.last(where: { $0.generation == generation && $0.key == key }) { return survey }
        let (_, result) = try await engine.request(negativeRequestCommand("negative_base", source: source, generation: generation, state: state))
        guard generation == sourceGeneration else { return nil }
        let survey = NegativeScan.Survey(generation: generation, key: key, result: result)
        keepSurvey(survey)
        return survey
    }

    /// A negative shown without its survey (opened again, at launch, or put
    /// back by undo with another reading) is surveyed once its preview is
    /// in, so Prep's warning and Film base line are there unasked. Not
    /// while a detection that answers it runs.
    private func surveyNegativeIfNeeded() {
        guard isNegative, negativeSurvey == nil, let engine, let source, let key = negativeKey(state) else { return }
        let generation = sourceGeneration, tag = "\(generation)|\(key)"
        guard !negativeSurveying.contains(tag) else { return }
        negativeSurveying.insert(tag)
        let snapshot = renderState
        Task {
            defer { negativeSurveying.remove(tag) }
            _ = try? await surveyNegative(engine: engine, source: source, generation: generation, state: snapshot)
        }
    }

    /// The print exposure `state` with `changes` meters to, in the picture
    /// when the rebate finder found one (else the frame less a border).
    /// Nil if it can't be metered: the exposure then stays.
    private func meterNegative(_ state: EditState, _ changes: [String: JSONValue], engine: EngineClient, source: URL,
                               generation: Int) async -> JSONValue? {
        var metered = state
        for (path, value) in changes { metered.params[path] = value }
        var command = negativeRequestCommand("negative_meter", source: source, generation: generation, state: metered)
        let survey = try? await surveyNegative(engine: engine, source: source, generation: generation, state: state)
        if let bounds = survey.flatMap({ NegativeScan.bounds(picture: $0.picture) }) { command["bounds"] = bounds }
        guard let (_, result) = try? await engine.request(command), result["print_exposure"] != .null else { return nil }
        return result["print_exposure"]
    }

    /// The print balanced again for a new paper on the same base: the
    /// filtration moved by the difference between `from`'s neutral and the
    /// new paper's, so an Auto print, a picked grey or a hand-set filter
    /// stays as it was, and the exposure metered; folded into the edit
    /// that changed the paper.
    private func rebalanceNegative(from: String) {
        guard isNegative, let engine, let source else { return }
        let request = beginNegative("Balancing the print…")
        negativeRebalancing = (request, from)
        let generation = sourceGeneration, asked = state, snapshot = renderState, key = negativeAnswerKey(state)
        Task {
            defer { endNegative(request) }
            do {
                // Trying paper after paper balances only for the last, from
                // the paper the filtration was balanced for.
                try await Task.sleep(for: Self.negativeSettle)
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                let (_, result) = try await engine.request(["command": .string("negative_paper"), "film": .string(snapshot.film),
                                                            "from_paper": .string(from), "paper": .string(snapshot.paper),
                                                            "params": snapshot.params])
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                var changes = ["enlarger.m_filter_shift": result["m_filter_shift"], "enlarger.y_filter_shift": result["y_filter_shift"]]
                if let exposure = await meterNegative(snapshot, changes, engine: engine, source: source, generation: generation) {
                    changes["enlarger.print_exposure"] = exposure
                }
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                foldNegative(changes, asked: asked, generation: generation)
            } catch { if negativeRequest == request { report(error) } }
        }
    }

    /// How long a detection or balance that follows an edit waits for the next.
    static let negativeSettle = Duration.milliseconds(200)

    /// Prep › Find the base again, forgetting a picked point.
    func redetectNegativeBase() {
        negativeBasePoint = nil
        detectNegativeBase()
    }

    /// Prep › Crop to picture: crops to the picture inside the clear film,
    /// levelled by the rebate, as one undo step.
    func cropToPicture() {
        guard isNegative, negativeWorking == nil, canChangeEdits, let engine, let source else { return }
        let request = beginNegative("Finding the picture…")
        let generation = sourceGeneration, snapshot = renderState, key = negativeAnswerKey(state)
        Task {
            defer { endNegative(request) }
            do {
                guard let survey = try await surveyNegative(engine: engine, source: source, generation: generation, state: snapshot),
                      negativeCurrent(request, generation: generation, key: key), canChangeEdits else { return }
                guard let crop = NegativeScan.crop(picture: survey.picture) else {
                    status = "No clear film round the picture to crop to: crop it by hand."
                    return
                }
                var next = state; next.crop = crop
                apply(next, action: "Crop to Picture")
            } catch { if negativeRequest == request { report(error) } }
        }
    }

    /// Prep › Auto: a minilab's print of this frame. Writes the enlarger's
    /// magenta, yellow and print exposure, where Lab shows them.
    func autoPrintNegative() {
        negativeCommand("negative_auto", point: nil, working: "Printing like a minilab…", action: "Auto Print")
    }

    /// The neutral picker: the magenta and yellow that print this point
    /// grey, then the print exposure metered for them.
    func neutralPrintNegative(at point: CGPoint) {
        negativeCommand("negative_neutral", point: point, working: "Balancing on the grey…", action: "Pick Neutral")
    }

    private func negativeCommand(_ name: String, point: CGPoint?, working: String, action: String) {
        guard isNegative, negativeWorking == nil, let engine, let source else { return }
        let request = beginNegative(working)
        let generation = sourceGeneration, snapshot = renderState, key = negativeAnswerKey(state)
        var command = negativeRequestCommand(name, source: source, generation: generation, state: snapshot)
        if let point { command["point"] = .array([.number(point.x), .number(point.y)]) }
        Task {
            defer { endNegative(request) }
            do {
                if point == nil, let survey = try await surveyNegative(engine: engine, source: source, generation: generation, state: snapshot),
                   let bounds = NegativeScan.bounds(picture: survey.picture) {
                    command["bounds"] = bounds
                }
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                let (_, result) = try await engine.request(command)
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                var changes = ["enlarger.m_filter_shift": result["m_filter_shift"], "enlarger.y_filter_shift": result["y_filter_shift"]]
                if result["print_exposure"] != .null {
                    changes["enlarger.print_exposure"] = result["print_exposure"]
                } else if let exposure = await meterNegative(snapshot, changes, engine: engine, source: source, generation: generation) {
                    changes["enlarger.print_exposure"] = exposure
                }
                guard negativeCurrent(request, generation: generation, key: key) else { return }
                var next = state
                for (path, value) in changes { next.params[path] = value }
                apply(next, action: action)
                status = Self.balanceSentence(changes)
            } catch { if negativeRequest == request { report(error) } }
        }
    }

    /// "Magenta 4.4, yellow −1.8, print exposure 0.87".
    private static func balanceSentence(_ changes: [String: JSONValue]) -> String {
        let f = { (v: JSONValue?) in (v?.double ?? 0).formatted(.number.precision(.fractionLength(1))) }
        let exposure = changes["enlarger.print_exposure"].map { ", print exposure \($0.double.formatted(.number.precision(.fractionLength(2))))" } ?? ""
        return "Magenta \(f(changes["enlarger.m_filter_shift"])), yellow \(f(changes["enlarger.y_filter_shift"]))\(exposure)"
    }

    /// A click on the canvas while picking: `point` is a fraction of the
    /// whole, uncropped photograph, from its top left.
    func pickedNegativePoint(_ point: CGPoint) {
        guard let pick = negativePick else { return }
        negativePick = nil
        guard (0...1).contains(point.x), (0...1).contains(point.y) else { return }
        switch pick {
        case .base: detectNegativeBase(at: point)
        case .neutral: neutralPrintNegative(at: point)
        }
    }

    // MARK: Darkroom roll (§9.2, §9.3)

    /// Prep › Roll's filtration: a frame that set its own goes back to the
    /// roll's magenta and yellow, its exposure metered again.
    func useRollFiltration() {
        guard isNegative, negativeWorking == nil, let roll = state.roll, !roll.filtered(state), let engine, let source else { return }
        let request = beginNegative("Metering the print…")
        let generation = sourceGeneration, snapshot = renderState, key = negativeAnswerKey(state)
        Task {
            defer { endNegative(request) }
            var changes: [String: JSONValue] = ["enlarger.m_filter_shift": .number(roll.magenta), "enlarger.y_filter_shift": .number(roll.yellow)]
            if let exposure = await meterNegative(snapshot, changes, engine: engine, source: source, generation: generation) {
                changes["enlarger.print_exposure"] = exposure
            }
            guard negativeCurrent(request, generation: generation, key: key), state.roll == roll else { return }
            var next = state
            for (path, value) in changes { next.params[path] = value }
            apply(next, action: "Use Roll’s Filtration")
            status = Self.balanceSentence(changes)
        }
    }

    /// What reading one frame of a roll found for it.
    private struct RollFrame {
        /// Its own base (from its rebate) or the roll's exposure shifted
        /// by its EXIF, and its metered print exposure.
        var changes: [String: JSONValue] = [:]
        /// The crop to its picture, for a frame not cropped yet.
        var crop: CropSettings?
        /// Read with the roll's settings, it looks like a colour negative;
        /// one that doesn't is left as it was.
        var looksNegative = true
    }

    /// Prep › Use for whole roll: every photograph in view is printed as a
    /// frame of this roll (§9.2), as one undo step. The film, paper,
    /// backlight, base tuning and filtration are this frame's; a frame of
    /// the roll that set its own filtration keeps it. Each other frame is
    /// read first: its base from its own rebate when it shows clear film,
    /// else this frame's with the exposure shifted by the camera's
    /// settings (§9.3); its print exposure metered; and, if uncropped, the
    /// crop to its picture. A photograph that doesn't look like a colour
    /// negative read so is skipped: left as it was. This one is checked
    /// too; if it doesn't, nothing changes.
    func useNegativeForRoll() {
        guard isNegative, negativeWorking == nil, canChangeEdits, let engine, let open = sourceItem, let source else { return }
        let paths = engine.paths
        var items = shownPhotos.map(\.url)
        if !items.contains(open) { items.insert(open, at: 0) }
        let reference = state
        let roll = NegativeRoll(id: reference.roll?.id ?? SidecarRestore.newVariantID(),
                                magenta: reference.params["enlarger.m_filter_shift"].double,
                                yellow: reference.params["enlarger.y_filter_shift"].double, paper: reference.paper)
        // Without sidecars only the open photograph changes (`changeEdits`).
        let others = sidecarLookup == nil ? [] : items.filter { $0 != open }
        let facts = photoFacts ?? PhotoFacts.read(source)
        let request = beginNegative("Reading the roll…")
        let generation = sourceGeneration, key = negativeAnswerKey(state)
        Task {
            defer { endNegative(request) }
            var frames: [URL: RollFrame] = [:]
            // This frame is checked like the others, never taken on trust.
            let survey: NegativeScan.Survey?
            do { survey = try await surveyNegative(engine: engine, source: source, generation: generation, state: reference) } catch {
                if negativeRequest == request { report(error) }; return
            }
            guard let survey, negativeRequest == request else { return }
            // Its film, base and filtration would print the roll: not from
            // a photograph that doesn't look like a negative.
            guard survey.looksNegative else {
                status = "The roll wasn't shared: this photograph doesn't look like a negative."
                libraryNote = LibraryNote(text: "Roll Not Shared", detail: "this photograph doesn't look like a negative",
                                          serial: (libraryNote?.serial ?? 0) + 1)
                return
            }
            if !others.isEmpty {
                do {
                    // A helper of its own, so the open photograph's decode stays.
                    let helper = try EngineClient(paths: paths)
                    defer { Task.detached(priority: .utility) { helper.stop() } }
                    for (index, item) in others.enumerated() {
                        guard negativeRequest == request else { break }
                        negativeWorking = "Reading frame \(index + 1) of \(others.count)…"
                        guard let target = openingState(of: item) else { continue }
                        frames[item] = try await readRollFrame(item.photoFile, target: target, generation: index + 1, roll: roll,
                                                               reference: reference, facts: facts, helper: helper)
                    }
                } catch { if negativeRequest == request { report(error) }; return }
            }
            guard sourceItem == open, canChangeEdits, negativeCurrent(request, generation: generation, key: key) else {
                status = "The roll wasn't changed: the photograph changed meanwhile."; return
            }
            let skipped = items.filter { frames[$0]?.looksNegative == false }
            let joining = items.filter { !skipped.contains($0) }
            defer {
                if !skipped.isEmpty {
                    let words = NegativeScan.skipped(skipped.count)
                    status = words
                    if var note = libraryNote {
                        note.detail = [note.detail, String(words.dropLast())].compactMap { $0 }.joined(separator: " · ")
                        libraryNote = note
                    }
                }
            }
            changeEdits(joining, action: "Use for Whole Roll", note: "Roll Shared") { item, target in
                guard item != open else { var next = target; next.roll = roll; return next }
                var next = NegativeScan.joining(roll, from: reference, into: target)
                if let frame = frames[item] {
                    for (path, value) in frame.changes { next.params[path] = value }
                    if let crop = frame.crop, !next.crop.cropped { next.crop = crop }
                }
                return next
            }
        }
    }

    /// One frame of a roll, read with the roll's settings.
    private func readRollFrame(_ photo: URL, target: EditState, generation: Int, roll: NegativeRoll, reference: EditState,
                               facts: PhotoFacts?, helper: EngineClient) async throws -> RollFrame {
        let joined = NegativeScan.joining(roll, from: reference, into: target)
        let (_, found) = try await helper.request(negativeRequestCommand("negative_base", source: photo, generation: generation, state: joined))
        var frame = RollFrame()
        frame.looksNegative = found["looks_negative"].bool
        guard frame.looksNegative else { return frame }
        if found["found"].bool {
            frame.changes["film_render.base"] = found["base"]
            frame.changes["film_render.convert.exposure_compensation_ev"] = found["exposure_ev"]
        } else {
            let stops = NegativeScan.scanStops(of: PhotoFacts.read(photo), from: facts)
            frame.changes["film_render.convert.exposure_compensation_ev"] =
                .number(NegativeScan.exposureEV(roll: reference.params["film_render.convert.exposure_compensation_ev"].double, scanStops: stops))
        }
        var metered = joined
        for (path, value) in frame.changes { metered.params[path] = value }
        var command = negativeRequestCommand("negative_meter", source: photo, generation: generation, state: metered)
        if let bounds = NegativeScan.bounds(picture: found["picture"]) { command["bounds"] = bounds }
        if let (_, result) = try? await helper.request(command), result["print_exposure"] != .null {
            frame.changes["enlarger.print_exposure"] = result["print_exposure"]
        }
        if !target.crop.cropped { frame.crop = NegativeScan.crop(picture: found["picture"]) }
        return frame
    }

    /// `item`'s edits as it opens with them; nil if its sidecar can't be read.
    private func openingState(of item: URL) -> EditState? {
        if item == sourceItem { return state }
        guard let lookup = sidecarLookup else { return plainState }
        let restore = SidecarRestore.read(photo: item.photoFile, variant: item.variantID, store: lookup.store, data: lookup.data)
        guard restore.problem == nil else { return nil }
        return restore.edits.map { restoredState($0.state, legacy: false) } ?? plainState
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
        let panel = NSSavePanel()
        panel.title = export ? "Export Photograph" : "Save Preview"
        panel.canCreateDirectories = true
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
        let previewSpace = previewSpace
        let picker = FormatPicker(panel: panel, selected: OutputFormat.last, engine: export ? ExportEngine.selected : nil, size: ExportSize.last) { engine, size, save in
            let extras = extras + (size.summary.map { " \($0)." } ?? "") + (save.summary(export: export, previewSpace: previewSpace).map { " \($0)." } ?? "")
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
            picker.remember()
            Self.lastSaveFolder = url.deletingLastPathComponent()
            let engine = picker.engine ?? .reference
            let size = picker.size
            let save = picker.settings
            if export { ExportEngine.selected = engine }
            Task {
                if export { await self.export(to: url, engine: engine, size: size, save: save) } else { await self.savePreview(to: url, size: size, save: save) }
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

    /// Saves the current preview with the crop, flip, rotation, border, size, and encoding.
    private func saveCommand(to url: URL, size: ExportSize, save: SaveSettings) -> [String: JSONValue] {
        var command: [String: JSONValue] = ["command": .string("save"), "render_id": .number(Double(renderID)), "output": .string(url.path),
                                            "rotate_cw": .number(Double(imageRotation)), "flip": .bool(imageFlip),
                                            "save_options": save.json]
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

    /// A pending brightness match changes the exposure: Save and Export
    /// wait for it and its preview, so they write what the inspector shows.
    private func settleBrightnessMatch() async {
        let deadline = Date().addingTimeInterval(30)
        while !closing, Date() < deadline, brightnessMatchPending || matchingBrightness || !previewCurrent && brightnessMatchActive {
            try? await Task.sleep(for: .milliseconds(20))
        }
    }

    private func savePreview(to url: URL, size: ExportSize = .full, save: SaveSettings = SaveSettings()) async {
        await settleBrightnessMatch()
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
        if let rating = currentSummary.flatMap(ExportRating.value) {
            do { try ExportRating.embed(rating, in: staging) } catch { report(error); return true }
        }
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
        await settleBrightnessMatch()
        // An edit made meanwhile makes the save fail; develop the photograph then.
        if exportEngine == .fast, await exportPreview(to: url, size: size, save: save) { return }
        guard !exporting, !saving, let source, let engine else { return }
        let job = ExportJob(); exportJob = job; exporting = true; exportStarted = Date()
        var exportState = state
        exportState.params["settings.serial_grain"] = .bool(lowMemoryExport)
        defer { exporting = false; exportJob = nil }
        do {
            let outcome = try await job.run(paths: engine.paths, scratch: engine.scratch, source: source, destination: url, state: exportState,
                                            crop: state.crop.cropped ? state.crop : nil, border: state.border.exported(for: photoSize), size: size, save: save,
                                            engine: exportEngine, rating: currentSummary.flatMap(ExportRating.value))
            status = outcome.fellBackToCPU
                ? "Exported \(url.lastPathComponent) on the CPU (32-bit): no GPU was available for Fast export"
                : "Exported \(url.lastPathComponent)"
        } catch is CancellationError { status = "Export cancelled" }
        catch { report(error) }
    }
    func cancelExport() { exportJob?.cancel() }
    func flushPreviewCache() async { await previewCache.flush() }

    // MARK: Batch export

    /// What Export acts on: the open photograph in the edit view, the selection in the grid.
    var exportTargets: [URL] { ratingTargets }
    /// Export is a single export: the edit view's photograph, or the open one alone selected in the grid.
    private var exportsSingle: Bool {
        mode == .photo || (!sourceDeleted && sourceItem.map { exportTargets == [$0] } ?? false)
    }
    /// The menu also needs no crop being drawn; crop has its own Return and Esc then.
    var canExport: Bool {
        guard ready, !exporting, !saving, !batchRunning else { return false }
        return exportsSingle ? source != nil : !exportTargets.isEmpty && hasCatalog && engine != nil
    }
    /// "Export Photograph…", or "Export 12 Photographs…" for a grid selection.
    var exportTitle: String {
        let count = exportsSingle ? 1 : exportTargets.count
        return count > 1 ? "Export \(count) Photographs…" : "Export Photograph…"
    }
    var batchRunning: Bool { batch?.running == true }

    /// Export: a single export, or a batch of the grid's selection.
    func exportPanel() {
        guard canExport, !cropping else { return }
        if exportsSingle { return savePanel(export: true) }
        batchPanel(exportTargets)
    }

    /// The context menu's Export…: what Export acts on, or the batch panel
    /// for the right-clicked photographs.
    func exportPanel(_ targets: [URL]) {
        guard !targets.isEmpty else { return }
        if targets == exportTargets { return exportPanel() }
        guard canStartBatch, ready, !saving, !cropping else { return }
        batchPanel(targets)
    }
    /// Whether the context menu's Export… can start now.
    func canExport(_ targets: [URL]) -> Bool {
        targets == exportTargets ? canExport && !cropping : !targets.isEmpty && ready && !saving && !cropping && canStartBatch
    }

    /// Picks a folder for `targets`, or for a single photograph that isn't
    /// open a file as today's single export does, then starts the batch.
    private func batchPanel(_ targets: [URL]) {
        let single = targets.count == 1
        let panel: NSSavePanel
        if single { panel = NSSavePanel() } else {
            let open = NSOpenPanel()
            open.canChooseFiles = false; open.canChooseDirectories = true; open.allowsMultipleSelection = false
            open.prompt = "Export"
            panel = open
        }
        panel.title = single ? "Export Photograph" : "Export \(targets.count) Photographs"
        panel.canCreateDirectories = true
        if let folder = Self.lastSaveFolder { panel.directoryURL = folder }
        // The first photograph's name, to show what the naming does.
        var named = BatchOptions()
        named.format = OutputFormat.last.rawValue
        let first = batchItems([targets[0]], options: named)[0]
        var chosenFormat = OutputFormat.last
        let rows = single ? nil : BatchRows { naming, custom in
            var options = BatchOptions(); options.naming = naming; options.custom = custom
            options.format = chosenFormat.rawValue
            return options.fileName(name: Self.batchName(first.item, title: first.title), film: first.state.film)
        }
        let previewSpace = previewSpace
        let count = targets.count
        let picker = FormatPicker(panel: panel, selected: OutputFormat.last, engine: ExportEngine.selected, size: ExportSize.last,
                                  restrictsType: single, extraRows: rows?.views ?? []) { engine, size, save in
            let extras = (size.summary.map { " \($0)." } ?? "") + (save.summary(export: true, previewSpace: previewSpace).map { " \($0)." } ?? "")
            let what = count == 1 ? "With its own edits, crop and border." : "Each of the \(count) photographs with its own edits, crop and border, one after another."
            return switch engine {
            case .fast?: "\(what) Fast looks the same as Reference, in a fraction of the time.\(extras)"
            default: "\(what) Reference is the most precise; about half a minute for each large photo.\(extras)"
            }
        }
        rows?.changed = { [weak picker] in picker?.refreshMessage() }
        picker.formatChanged = { [weak picker, weak rows] in
            if let picker { chosenFormat = picker.format }
            rows?.refresh()
        }
        if single { panel.nameFieldStringValue = first.fileName }
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            let folder = single ? url.deletingLastPathComponent() : url
            if single, Self.sameFile(url, targets[0].photoFile) {
                self.error = "Choose a different filename to preserve the original photograph."; return
            }
            OutputFormat.last = picker.format
            ExportSize.last = picker.size
            picker.remember(); rows?.remember()
            Self.lastSaveFolder = folder
            let engine = picker.engine ?? .reference
            ExportEngine.selected = engine
            let save = picker.settings, size = picker.size
            var options = BatchOptions(format: picker.format.rawValue, engine: engine.rawValue, edge: size.edge.rawValue, pixels: size.pixels,
                                       bitDepth: save.bitDepth, colorSpace: save.colorSpace, jpegQuality: save.jpegQuality,
                                       lowMemory: self.lowMemoryExport)
            // The save panel asked before replacing.
            if let rows { options.naming = rows.naming; options.custom = rows.custom; options.existing = rows.existing } else { options.existing = .replace }
            Task { await self.exportBatch(targets, to: folder, options: options, named: single ? url.lastPathComponent : nil) }
        }
    }

    /// The name `{name}` stands for: the file's without its extension, a
    /// duplicate's number after it (`IMG_0412 (2)`).
    nonisolated static func batchName(_ item: URL, title: String) -> String {
        let photo = item.photoFile
        let suffix = title.hasPrefix(photo.lastPathComponent) ? String(title.dropFirst(photo.lastPathComponent.count)) : ""
        return photo.deletingPathExtension().lastPathComponent + suffix
    }

    /// Starts exporting `targets` to `folder`, with their edits as they are
    /// now; `named` names a batch of one. Nil if it couldn't start.
    @discardableResult
    func exportBatch(_ targets: [URL], to folder: URL, options: BatchOptions, named: String? = nil) async -> BatchRun? {
        if let open = sourceItem, targets.contains(open) { await settleBrightnessMatch() }
        guard canStartBatch else { return nil }
        var items = batchItems(targets, options: options)
        if let named, items.count == 1 { items[0].fileName = named }
        // Replacing a photograph's own file is refused, as for a single export.
        if options.existing == .replace, items.contains(where: { Self.sameFile(folder.appendingPathComponent($0.fileName), $0.item.photoFile) }) {
            error = "Choose a different folder or file name to preserve the original photographs."
            return nil
        }
        let journal = BatchJournal(destination: folder, bookmark: try? folder.bookmarkData(), options: options, items: items)
        return startBatch(journal)
    }

    /// One exporter at a time: not during a single export either.
    var canStartBatch: Bool { !closing && !batchRunning && !exporting && hasCatalog && engine != nil }

    /// Each target's edits as they are now, and the name it gets.
    func batchItems(_ targets: [URL], options: BatchOptions) -> [BatchItem] {
        let lookup = sidecarLookup
        let open = sourceDeleted ? nil : sourceItem
        let current = state
        var items = targets.map { target -> BatchItem in
            let photo = target.photoFile
            let raw = Self.rawExtensions.contains(photo.pathExtension.lowercased())
            let title = library.name(of: target)
            var item = BatchItem(item: target, title: title, state: current, needsMatch: false, storesMatch: false,
                                 rating: ExportRating.value(library.summary(for: target)), fileName: "")
            func matchable(_ state: EditState) -> Bool {
                state.brightnessMatch != nil && (raw || state.params["io.input_interpretation.mode"].string == "auto")
            }
            if target == open, source != nil {
                // As on screen, its match settled.
                let size = image != nil ? imageSize : placeholderSize
                item.size = size == .zero ? nil : size
            } else if let lookup, case let restore = SidecarRestore.read(photo: photo, variant: target.variantID, store: lookup.store, data: lookup.data),
                      let saved = restore.edits, restore.problem == nil {
                item.state = restoredState(saved.state, legacy: false, matchSolved: saved.matchSolved)
                item.needsMatch = !saved.matchSolved && matchable(item.state)
                item.storesMatch = true
                item.look = library.looks[target]
                let film = item.state.film, paper = item.state.paper
                let missing = [films.contains { $0.id == film } ? nil : "the film “\(film)”", papers.contains { $0.id == paper } ? nil : "the paper “\(paper)”"].compactMap { $0 }
                if !missing.isEmpty {
                    item.status = .failed
                    item.reason = "Edited with \(missing.joined(separator: " and ")), which this version of Negs doesn’t have."
                }
            } else {
                // Never edited: as opening it now would show it.
                item.state = newPhotoState(after: current)
                item.needsMatch = matchable(item.state)
            }
            item.fileName = options.fileName(name: Self.batchName(target, title: title), film: item.state.film)
            return item
        }
        let names = BatchNames.unique(items.map(\.fileName))
        for index in items.indices { items[index].fileName = names[index] }
        return items
    }

    @discardableResult
    private func startBatch(_ journal: BatchJournal) -> BatchRun? {
        guard canStartBatch else { return nil }
        let run = BatchRun(journal: journal, journalURL: batchJournalURL)
        batch = run
        requestNotificationPermission()
        launchBatch(run)
        return run
    }

    private func launchBatch(_ run: BatchRun) {
        run.running = true; run.cancelled = false
        run.task = Task { [weak self] in await self?.runBatch(run) }
    }

    /// Exports what's waiting, one photograph at a time, writing the journal
    /// after each. At quit the journal stays for the next launch.
    private func runBatch(_ run: BatchRun) async {
        let activity = ProcessInfo.processInfo.beginActivity(options: [.userInitiated, .idleSystemSleepDisabled], reason: "Exporting photographs")
        defer { ProcessInfo.processInfo.endActivity(activity) }
        try? run.journal.write(to: run.journalURL)
        while !run.cancelled, !closing, let index = run.journal.items.firstIndex(where: { $0.status == .waiting }) {
            run.current = index
            run.journal.items[index] = await exportBatchItem(run.journal.items[index], of: run)
            if run.journal.items[index].status == .waiting { break }
            try? run.journal.write(to: run.journalURL)
        }
        run.current = nil
        guard !closing else { return }
        run.running = false
        BatchJournal.remove(at: run.journalURL)
        if let engine = batchEngine {
            batchEngine = nil
            await Task.detached(priority: .utility) { engine.stop() }.value
        }
        batchFinished(run)
        pumpThumbnails()
    }

    /// One photograph: its match solved if it needs one, then today's
    /// exporter. Still waiting if it was cancelled.
    private func exportBatchItem(_ item: BatchItem, of run: BatchRun) async -> BatchItem {
        var item = item
        func failed(_ reason: String) -> BatchItem { item.status = .failed; item.reason = reason; return item }
        let photo = item.item.photoFile, folder = run.journal.destination, options = run.journal.options
        let fm = FileManager.default
        var isFolder: ObjCBool = false
        guard fm.fileExists(atPath: folder.path, isDirectory: &isFolder), isFolder.boolValue else {
            return failed("The folder “\(folder.lastPathComponent)” is missing.")
        }
        guard fm.fileExists(atPath: photo.path) else { return failed("The photograph is missing.") }
        guard let paths = engine?.paths, let scratch = engine?.scratch else { return failed("The engine isn’t running.") }
        var name = item.fileName
        func taken(_ name: String) -> Bool { fm.fileExists(atPath: folder.appendingPathComponent(name).path) }
        if taken(name) {
            switch options.existing {
            case .skip:
                item.status = .skipped; item.reason = "\(name) already exists."
                return item
            case .number: name = BatchNames.numbered(name, taken: taken)
            case .replace: break
            }
        }
        let destination = folder.appendingPathComponent(name)
        guard !Self.sameFile(destination, photo) else { return failed("It would replace the photograph’s own file.") }
        var state = item.state, size = item.size
        let border = state.border
        let needsSize = size == nil && border.enabled && border.export && border.style == .film35
        if item.needsMatch || needsSize {
            do {
                let solved = try await solveBatchMatch(item, paths: paths)
                state = solved.state; size = solved.size
            } catch is CancellationError {
                return item
            } catch {
                // Unmatched, as the photograph would open; a 35mm border needs the size.
                if needsSize { return failed(error.localizedDescription) }
            }
            if run.cancelled || closing { return item }
        }
        var exportState = state
        exportState.params["settings.serial_grain"] = .bool(options.lowMemory)
        let job = ExportJob()
        run.job = job
        defer { run.job = nil }
        do {
            try await job.run(paths: paths, scratch: scratch, source: photo, destination: destination, state: exportState,
                              crop: state.crop.cropped ? state.crop : nil, border: state.border.exported(for: state.crop.size(of: size ?? .zero)),
                              size: options.size, save: options.save, engine: options.exportEngine, rating: item.rating)
            item.status = .done; item.written = destination; item.reason = nil
        } catch is CancellationError {
            return item
        } catch {
            return failed(error.localizedDescription)
        }
        return item
    }

    /// Solves Match camera brightness for a batch photograph in the batch's
    /// own helper, and stores it if its edits are unchanged. Also gives its
    /// size, which the helper reads with the decode.
    private func solveBatchMatch(_ item: BatchItem, paths: EnginePaths) async throws -> (state: EditState, size: CGSize) {
        let photo = item.item.photoFile
        let raw = Self.rawExtensions.contains(photo.pathExtension.lowercased())
        let engine: EngineClient
        if let running = batchEngine { engine = running } else { engine = try EngineClient(paths: paths); batchEngine = engine }
        batchGeneration += 1
        let generation = batchGeneration
        let buffer = engine.directory.appendingPathComponent("batch-decode.rgba")
        defer { try? FileManager.default.removeItem(at: buffer) }
        var state = item.state
        let (_, decoded) = try await engine.request([
            "command": .string("thumbnail"), "input": .string(photo.path), "source_generation": .number(Double(generation)),
            "film": .string(state.film), "paper": .string(state.paper), "params": state.params,
            "long_side": .number(16), "preview_path": .string(buffer.path),
        ])
        let size = CGSize(width: decoded["source_width"].double, height: decoded["source_height"].double)
        guard item.needsMatch else { return (state, size) }
        let reference = engine.directory.appendingPathComponent("batch-brightness.png")
        defer { try? FileManager.default.removeItem(at: reference) }
        let solved = try await Self.solveBrightnessMatch(engine: engine, source: photo, generation: generation, size: size, raw: raw,
                                                         request: matchSolveState(state), reference: reference)
        guard let matched = Self.matching(state, ev: solved.exposure, limited: solved.limited, inputs: matchInputs(state)) else { return (state, size) }
        state = matched
        // Stored as opening it would, unless it was edited since Export.
        if item.storesMatch, let lookup = sidecarLookup, let sidecar = try? lookup.store.read(photo: photo),
           let variant = item.item.variantID.map({ id in sidecar.variants.first { $0.id == id } }) ?? sidecar.variants.first,
           let stored = variant.edits, PhotoSidecar.look(of: stored, defaultsVersion: sidecar.defaultsVersion) == item.look,
           let table = try? SidecarDefaults.load(version: sidecar.defaultsVersion, data: lookup.data),
           storeSolvedMatch(state, item: item.item, variant: variant.id, stored: stored, sidecar: sidecar, table: table) != nil {
            library.reload(photo)
        }
        return (state, size)
    }

    /// The failed photographs again.
    func retryBatch() {
        guard let run = batch, !run.running, !closing else { return }
        for index in run.journal.items.indices where run.journal.items[index].status == .failed {
            run.journal.items[index].status = .waiting; run.journal.items[index].reason = nil
        }
        guard run.journal.remaining > 0 else { return }
        launchBatch(run)
    }

    /// Forgets a finished batch, once its popover is closed.
    func clearBatch() {
        guard let run = batch, !run.running else { return }
        batch = nil
    }

    /// The batch's files selected in the Finder, or its folder.
    func showBatchInFinder() {
        guard let run = batch else { return }
        let written = run.items.compactMap(\.written).filter { FileManager.default.fileExists(atPath: $0.path) }
        if written.isEmpty { NSWorkspace.shared.open(run.journal.destination) } else { NSWorkspace.shared.activateFileViewerSelecting(written) }
    }

    private func batchFinished(_ run: BatchRun) {
        let exported = run.exported.count, failed = run.failures.count
        let folder = run.journal.destination.lastPathComponent
        let text = (exported == 1 ? "Exported 1 photograph" : "Exported \(exported) photographs") + " to “\(folder)”"
            + (failed > 0 ? "; \(failed) failed" : "") + (run.skipped.isEmpty ? "" : "; \(run.skipped.count) skipped")
        status = run.cancelled ? "Export cancelled. " + text : text
        if !run.needsAttention, !batchPopoverShown { batch = nil }
        if let kind = run.journal.options.lightroom { return lightroomBatchFinished(run, kind: kind) }
        if Self.notificationsAvailable, NSApp?.isActive == false, !run.cancelled { notifyBatchFinished(exported: exported, failed: failed, folder: folder) }
    }

    /// Whether the popover is open: it clears a finished batch when it closes.
    @ObservationIgnored var batchPopoverShown = false
    /// Shows the batch's popover under a view; set by the window controller.
    @ObservationIgnored var presentBatch: (NSView) -> Void = { _ in }

    /// A batch the last session didn't finish, offered once launch has settled.
    func offerBatchResume() {
        guard batch == nil, !closing else { return }
        let url = batchJournalURL
        Task { [weak self] in
            guard let journal = await Task.detached(priority: .utility, operation: { BatchJournal.read(from: url) }).value else { return }
            guard let self, !self.closing, self.batch == nil else { return }
            guard journal.remaining > 0 else { return BatchJournal.remove(at: url) }
            let count = journal.remaining
            let photographs = count == 1 ? "1 photograph" : "\(count) photographs"
            let answer = await self.ask(AppAlert(
                title: journal.options.lightroom.map { "Resume sending \(photographs) to \($0.title)?" }
                    ?? "Resume exporting \(photographs) to “\(journal.destination.lastPathComponent)”?",
                detail: "Negs quit before the export finished. Each photograph exports with its edits as they were when you chose Export.",
                buttons: [.init(title: "Resume"), .init(title: "Discard")]))
            guard answer.button == 0 else { return BatchJournal.remove(at: url) }
            // Started now, without the open photograph's state.
            if self.startBatch(journal) == nil { self.error = "The export couldn’t resume yet. Try again once Negs has started." }
        }
    }

    // MARK: Notifications

    /// Only an app bundle can post them; never under XCTest.
    private static var notificationsAvailable: Bool {
        NSClassFromString("XCTestCase") == nil && Bundle.main.bundleURL.pathExtension == "app" && Bundle.main.bundleIdentifier != nil
    }

    private func requestNotificationPermission() {
        guard Self.notificationsAvailable else { return }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    private func notifyBatchFinished(exported: Int, failed: Int, folder: String) {
        guard Self.notificationsAvailable else { return }
        let content = UNMutableNotificationContent()
        content.title = failed > 0 ? "Export finished with failures" : "Export finished"
        content.body = (exported == 1 ? "1 photograph" : "\(exported) photographs") + " exported to “\(folder)”" + (failed > 0 ? "; \(failed) failed." : ".")
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
    // MARK: Presets

    /// The pane header's Adjustments and Presets words, ⌥⌘I and ⌥⌘P: show that panel, or
    /// hide the column if it already shows it.
    /// In Image Only they end it and show that panel.
    func togglePane(_ pane: InspectorPane) {
        if inspectorShown && inspectorPane == pane { inspectorVisible = false } else { showPane(pane) }
    }
    func showPane(_ pane: InspectorPane) { leaveImageOnly(); inspectorPane = pane; inspectorVisible = true }

    // MARK: Image Only and upside down

    /// F: Image Only on or off. From the grid it opens the current
    /// photograph in Image Only. Not while cropping, whose tools are in the
    /// inspector and the status line.
    func toggleImageOnly() {
        if imageOnly { return leaveImageOnly() }
        guard !cropping else { return }
        if mode == .grid {
            guard selection.current != nil else { return }
            editCurrent()
        }
        guard mode == .photo, source != nil else { return }
        imageOnly = true
    }
    func leaveImageOnly() { if imageOnly { imageOnly = false } }
    /// ⌥⌘F: in Image Only it ends it and shows the strip.
    func toggleFilmstrip() {
        if imageOnly { leaveImageOnly(); filmstripVisible = true } else { filmstripVisible.toggle() }
    }
    /// Holding T. Only the photograph view's canvas turns, not while cropping.
    func holdUpsideDown(_ held: Bool) {
        let turned = held && mode == .photo && source != nil && !cropping
        if turned != upsideDown { upsideDown = turned }
    }
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
    /// output encoding, the preview limit and the Post values.
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
                                                                "params": snapshot.params])
                    guard state == snapshot else { continue }
                    let (film, paper) = (result["film"].string, result["paper"].string)
                    guard films.contains(where: { $0.id == film }), papers.contains(where: { $0.id == paper }) else {
                        throw EngineError(message: "This version of Negs doesn’t have the preset’s film or paper.")
                    }
                    var next = state
                    next.film = film; next.paper = paper; next.params = result["params"]
                    // A negative stays as it's read (decision 6).
                    next = NegativeScan.keepingNegative(next, from: state)
                    apply(next, action: "Apply Preset “\(preset.name)”")
                    notePresetApplied(preset.url)
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
        let snapshot = state, name = PresetName.from(fileName: filmName), snapshotItem = sourceItem
        Task {
            do {
                let (_, result) = try await engine.request(["command": .string("preset_export"), "film": .string(snapshot.film),
                                                            "paper": .string(snapshot.paper), "params": snapshot.params])
                let file = try await presetLibrary.add(Data(result["text"].string.utf8), name: name)
                // The photograph's settings are this preset's: it's the one ringed.
                notePresetApplied(file.url, to: snapshotItem)
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
        forgetApplied(preset.url)
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
                appliedPresets = appliedPresets.mapValues { $0 == preset.url ? renamed.url : $0 }
                // Same file, same thumbnail: no blank tile while it's renamed.
                previews.moveTile(preset: preset.url, to: renamed.url)
            } catch { report(error) }
            await refreshPresets()
        }
    }

    /// Notes `preset` as the one applied last to `item` (the photograph on
    /// screen when nil).
    func notePresetApplied(_ preset: URL, to item: URL? = nil) {
        guard let item = item ?? sourceItem else { return }
        appliedPresets[item] = preset
    }

    private func forgetApplied(_ preset: URL) {
        appliedPresets = appliedPresets.filter { $0.value != preset }
    }

    func deletePreset(_ preset: PresetFile) {
        Task {
            let answer = await ask(AppAlert(
                title: "Delete the preset “\(preset.name)”?",
                detail: "This can’t be undone. Photographs edited with it keep their settings.",
                buttons: [.init(title: "Delete", role: .destructive), .init(title: "Cancel", role: .cancel)]))
            guard answer.button == 0 else { return }
            do {
                try await presetLibrary.delete(preset)
                forgetApplied(preset.url)
            } catch { report(error) }
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
        cancelBrightnessMatch()
        matchCache.flush()

        closing = true; pendingRender?.cancel(); activityTimer?.cancel()
        stopCompareDevelop()
        if let powerStateObserver { NotificationCenter.default.removeObserver(powerStateObserver); self.powerStateObserver = nil }
        thumbnailTask?.cancel(); thumbnailIdle?.cancel()
        thumbnailsDeveloping.values.forEach { $0.task.cancel() }; thumbnailEngines.values.forEach { $0.stop() }
        exportJob?.cancel(); batch?.job?.cancel(); batchEngine?.stop(); engine?.stop()
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
        return .object(options)
    }
    /// The exporter's arguments.
    var arguments: [String] {
        ["--jpeg-quality", "\(jpegQuality)", "--color-space", spaceName] + (bitDepth.map { ["--bit-depth", "\($0)"] } ?? [])
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

/// The save panel's Format, Bit Depth, Colour Space, JPEG Quality and Size
/// controls, and for exports the Engine pop-up. Changing the format
/// restricts the panel to that type, which also rewrites the extension in
/// the name field, and shows its bit depth (JPEG's fixed) and, for JPEG
/// only, the quality. ProPhoto RGB needs 16 bits: at 8 it is disabled and
/// Adobe RGB stands in until 16 bits bring it back. Changing anything
/// updates the message.
@MainActor final class FormatPicker: NSObject, NSTextFieldDelegate {
    private let popup = NSPopUpButton()
    private let enginePopup: NSPopUpButton?
    private let depthPopup = NSPopUpButton()
    private let spacePopup = NSPopUpButton()
    private let qualitySlider = NSSlider(value: 95, minValue: Double(SaveSettings.qualityRange.lowerBound),
                                         maxValue: Double(SaveSettings.qualityRange.upperBound), target: nil, action: nil)
    private let qualityLabel = NSTextField(labelWithString: "")
    private var qualityRow: NSStackView!
    private let stack = NSStackView()
    private let edgePopup = NSPopUpButton()
    private let pixelsField = NSTextField()
    private let message: (ExportEngine?, ExportSize, SaveSettings) -> String
    private weak var panel: NSSavePanel?
    /// A folder chooser picks no file type.
    private let restrictsType: Bool
    /// Called when the format changes.
    var formatChanged: () -> Void = {}
    /// The colour space picked, and each lossless format's bit depth.
    private var chosenSpace = SaveSettings.chosenSpace
    private var depths: [OutputFormat: Int] = [.png: SaveSettings.bitDepth(for: .png), .tiff: SaveSettings.bitDepth(for: .tiff)]
    var format: OutputFormat { OutputFormat.allCases[max(popup.indexOfSelectedItem, 0)] }
    /// nil for Save Preview.
    var engine: ExportEngine? { enginePopup.map { ExportEngine.allCases[max($0.indexOfSelectedItem, 0)] } }
    var size: ExportSize {
        ExportSize(edge: ExportSize.Edge.allCases[max(edgePopup.indexOfSelectedItem, 0)],
                   pixels: ExportSize.pixels(from: pixelsField.stringValue))
    }
    var settings: SaveSettings {
        let depth = depths[format] ?? 8
        return SaveSettings(bitDepth: depth, colorSpace: SaveSettings.effectiveSpace(chosenSpace, bitDepth: depth),
                            jpegQuality: Int(qualitySlider.doubleValue.rounded()))
    }

    /// `extraRows` go below the size: a batch's naming rows.
    init(panel: NSSavePanel, selected: OutputFormat, engine: ExportEngine?, size: ExportSize, restrictsType: Bool = true, extraRows: [NSView] = [],
         message: @escaping (ExportEngine?, ExportSize, SaveSettings) -> String) {
        self.panel = panel
        self.message = message
        self.restrictsType = restrictsType
        enginePopup = engine.map { _ in NSPopUpButton() }
        super.init()
        popup.addItems(withTitles: OutputFormat.allCases.map(\.title))
        popup.selectItem(at: OutputFormat.allCases.firstIndex(of: selected) ?? 0)
        popup.target = self; popup.action = #selector(changed)
        var rows = [NSStackView(views: [NSTextField(labelWithString: "Format:"), popup])]
        depthPopup.addItems(withTitles: ["8-bit", "16-bit"])
        depthPopup.target = self; depthPopup.action = #selector(depthChanged)
        rows.append(NSStackView(views: [NSTextField(labelWithString: "Bit Depth:"), depthPopup]))
        spacePopup.addItems(withTitles: SaveSettings.colorSpaces)
        spacePopup.autoenablesItems = false
        spacePopup.item(at: SaveSettings.prophoto)?.toolTip = "ProPhoto RGB needs 16 bits; 8 bits would band."
        spacePopup.target = self; spacePopup.action = #selector(spaceChanged)
        spacePopup.toolTip = "The colour space the file is saved in, with its ICC profile."
        rows.append(NSStackView(views: [NSTextField(labelWithString: "Colour Space:"), spacePopup]))
        qualitySlider.doubleValue = Double(SaveSettings.lastQuality)
        qualitySlider.isContinuous = true
        qualitySlider.target = self; qualitySlider.action = #selector(qualityChanged)
        qualitySlider.widthAnchor.constraint(equalToConstant: 160).isActive = true
        qualityLabel.alignment = .right
        qualityLabel.widthAnchor.constraint(equalToConstant: 28).isActive = true
        qualityRow = NSStackView(views: [NSTextField(labelWithString: "JPEG Quality:"), qualitySlider, qualityLabel])
        rows.append(qualityRow)
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
        // No minimum or maximum here: `size` reads `stringValue` on every
        // keystroke, which checks the text against the formatter, and a
        // range would clear every partial number ("1" on the way to "1600").
        // `size` clamps instead, and leaving the field shows the result.
        let formatter = NumberFormatter()
        formatter.allowsFloats = false
        pixelsField.formatter = formatter
        pixelsField.integerValue = size.pixels
        pixelsField.alignment = .right
        pixelsField.delegate = self
        pixelsField.setAccessibilityLabel("Size in pixels")
        pixelsField.widthAnchor.constraint(equalToConstant: 64).isActive = true
        pixelsField.isEnabled = size.edge != .full
        rows.append(NSStackView(views: [NSTextField(labelWithString: "Size:"), edgePopup, pixelsField, NSTextField(labelWithString: "px")]))
        rows.forEach(stack.addArrangedSubview)
        extraRows.forEach(stack.addArrangedSubview)
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 10, left: 20, bottom: 10, right: 20)
        panel.accessoryView = stack
        if restrictsType { panel.allowedContentTypes = [selected.type] }
        sync()
    }

    /// Stores the choices for the next save.
    func remember() {
        SaveSettings.chosenSpace = chosenSpace
        for (format, depth) in depths { SaveSettings.setBitDepth(depth, for: format) }
        SaveSettings.lastQuality = settings.jpegQuality
    }

    /// Brings the bit depth, colour space and quality controls in line with
    /// the format, then the message.
    private func sync() {
        let current = settings
        depthPopup.selectItem(at: current.bitDepth == 16 ? 1 : 0)
        depthPopup.isEnabled = format != .jpeg
        depthPopup.toolTip = format == .jpeg ? "JPEG is 8-bit only." : nil
        spacePopup.item(at: SaveSettings.prophoto)?.isEnabled = current.bitDepth == 16
        spacePopup.selectItem(at: current.colorSpace)
        qualityLabel.stringValue = "\(current.jpegQuality)"
        qualityRow.isHidden = format != .jpeg
        stack.setFrameSize(stack.fittingSize)
        panel?.message = message(engine, size, current)
    }

    @objc private func changed() {
        if restrictsType { panel?.allowedContentTypes = [format.type] }
        sync()
        formatChanged()
    }
    @objc private func depthChanged() {
        if format != .jpeg { depths[format] = depthPopup.indexOfSelectedItem == 1 ? 16 : 8 }
        sync()
    }
    @objc private func spaceChanged() { chosenSpace = max(spacePopup.indexOfSelectedItem, 0); sync() }
    @objc private func qualityChanged() { qualityLabel.stringValue = "\(settings.jpegQuality)" }
    @objc private func engineChanged() { panel?.message = message(engine, size, settings) }
    @objc private func sizeChanged() {
        pixelsField.isEnabled = size.edge != .full
        if pixelsField.isEnabled {
            // Show the value that will be used, should the field have been
            // left blank or out of range, then focus it for typing.
            pixelsField.integerValue = size.pixels
            panel?.makeFirstResponder(pixelsField)
        }
        panel?.message = message(engine, size, settings)
    }
    func controlTextDidChange(_ notification: Notification) { panel?.message = message(engine, size, settings) }
    /// The message again, after a row of the caller's changed.
    func refreshMessage() { panel?.message = message(engine, size, settings) }
    /// Leaving the field writes back the clamped value, so a blank or
    /// out-of-range entry never sits beside a different export size.
    func controlTextDidEndEditing(_ notification: Notification) {
        let pixels = size.pixels
        if pixelsField.integerValue != pixels || pixelsField.stringValue.isEmpty { pixelsField.integerValue = pixels }
        panel?.message = message(engine, size, settings)
    }
}

/// The undo target of changes to several photographs (`EditorModel.libraryUndo`).
final class LibraryUndoTarget: NSObject {}
