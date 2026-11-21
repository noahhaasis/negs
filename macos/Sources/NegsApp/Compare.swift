import AppKit
import SwiftUI

// Compare two photographs (ticket 46): two takes of one scene, one frame in
// two films, or a photograph against its original, side by side or under a
// slider. The panel keeps editing one of them: the open photograph, with
// its session, engine and render unchanged. The other develops in a helper
// of its own once the edited one has settled, as opening it would show it.

/// The two photographs on screen, and how they're shown.
struct Comparison: Equatable {
    enum Layout: String, Equatable { case sideBySide, slider }
    /// Where compare was entered, which leaving goes back to.
    enum Origin: Equatable { case grid, photo }
    enum Side: Equatable, Hashable {
        case photo(URL)
        /// The photograph's original, undeveloped.
        case original(URL)
        var item: URL { switch self { case .photo(let item), .original(let item): item } }
        var isOriginal: Bool { if case .original = self { true } else { false } }
    }

    /// Left or top, and right or bottom.
    var first: Side
    var second: Side
    /// The first is the active one: the one the panel edits, or against the
    /// original, the one last clicked.
    var activeFirst: Bool
    var layout = Layout.sideBySide
    let origin: Origin
    /// The selection as it was, given back on leaving.
    let selection: LibrarySelection
    /// The slider's line, as a fraction of the frame's width.
    var split: CGFloat = 0.5

    var active: Side { activeFirst ? first : second }
    var inactive: Side { activeFirst ? second : first }
    var againstOriginal: Bool { first.isOriginal || second.isOriginal }
    /// The original side is the active one: the panel can't edit it.
    var originalActive: Bool { active.isOriginal }
    /// The photograph the panel edits: the active one, or the developed one
    /// beside its original.
    var edited: URL { active.isOriginal ? inactive.item : active.item }
    /// The side ← → step: always the second.
    var stepped: Side { second }
}

/// The photograph that isn't the open one (or the original): what's shown
/// for it, from stand-ins to its develop, with its own edits and facts.
@MainActor @Observable final class CompareOther {
    enum Shown: Equatable {
        /// Nothing yet.
        case none
        /// The camera's preview, laid out as the photograph (`size`).
        case embedded
        /// The developed thumbnail: finished, cropped and bordered.
        case thumbnail
        /// The last develop kept for it (`EditedThumbnailStore.renders`).
        case kept
        /// Its develop now.
        case developed
        /// The original, undeveloped.
        case original
    }

    let side: Comparison.Side
    var item: URL { side.item }
    /// Its edits, as opening it would restore them; nil until read.
    var state: EditState?
    var facts: PhotoFacts?
    private(set) var image: AnyObject?
    /// The image's layout size: the photograph's, uncropped.
    private(set) var size = CGSize.zero
    private(set) var shown = Shown.none
    /// Its 35mm border strip, when it has that border.
    var filmStrip: FilmStrip?
    /// Why it couldn't be developed.
    var failure: String?

    init(side: Comparison.Side) { self.side = side }

    /// Shows `image` unless what's shown is better already.
    func show(_ image: AnyObject, size: CGSize, as kind: Shown) {
        guard kind.rank >= shown.rank else { return }
        self.image = image; self.size = size; shown = kind
    }

    /// Drawn as is, with no crop, turn or border of its own: a developed thumbnail.
    var finished: Bool { shown == .thumbnail }
}

private extension CompareOther.Shown {
    var rank: Int {
        switch self { case .none: 0; case .embedded: 1; case .thumbnail: 2; case .kept: 3; case .developed, .original: 4 }
    }
}

/// Where the photographs go (ticket 46): side by side in a row or stacked in
/// a column, whichever gives them more room, or one frame for the slider.
/// Worked out apart from the views so it can be tested.
enum CompareArrangement: Equatable {
    case row, column

    /// Above each photograph in a row: its caption; below: its label (as in the photo view).
    static let caption: CGFloat = 54, label: CGFloat = 68
    /// Stacked, only a caption line, which takes the film and paper too.
    static let stackedCaption: CGFloat = 40
    /// Between the two, and round the canvas's edges.
    static let gap: CGFloat = 28, stackedGap: CGFloat = 14, side: CGFloat = 36, bottom: CGFloat = 12

    struct Layout: Equatable {
        var arrangement: CompareArrangement
        /// Each photograph's frame, first then second, in the canvas.
        var frames: [CGRect]
        var area: CGFloat { frames.reduce(0) { $0 + $1.width * $1.height } }
    }

    /// `aspects`: each photograph's width ÷ height, as shown (with its border).
    static func layout(canvas: CGSize, aspects: (CGFloat, CGFloat)) -> Layout {
        let row = rowLayout(canvas: canvas, aspects: aspects), column = columnLayout(canvas: canvas, aspects: aspects)
        return column.area > row.area ? column : row
    }

    static func rowLayout(canvas: CGSize, aspects: (CGFloat, CGFloat)) -> Layout {
        let width = max((canvas.width - 2 * side - gap) / 2, 1)
        let height = max(canvas.height - caption - label, 1)
        let sizes = [fit(aspects.0, in: CGSize(width: width, height: height)), fit(aspects.1, in: CGSize(width: width, height: height))]
        let frames = sizes.enumerated().map { index, size in
            let x = side + CGFloat(index) * (width + gap) + (width - size.width) / 2
            return CGRect(x: x.rounded(), y: (caption + (height - size.height) / 2).rounded(), width: size.width.rounded(), height: size.height.rounded())
        }
        return Layout(arrangement: .row, frames: frames)
    }

    static func columnLayout(canvas: CGSize, aspects: (CGFloat, CGFloat)) -> Layout {
        let width = max(canvas.width - 2 * side, 1)
        let height = max((canvas.height - 2 * stackedCaption - stackedGap - bottom) / 2, 1)
        let sizes = [fit(aspects.0, in: CGSize(width: width, height: height)), fit(aspects.1, in: CGSize(width: width, height: height))]
        // The pair sits in the middle of the canvas, as one block.
        let block = 2 * stackedCaption + stackedGap + sizes[0].height + sizes[1].height
        var y = max((canvas.height - bottom - block) / 2, 0)
        var frames: [CGRect] = []
        for size in sizes {
            y += stackedCaption
            frames.append(CGRect(x: ((canvas.width - size.width) / 2).rounded(), y: y.rounded(), width: size.width.rounded(), height: size.height.rounded()))
            y += size.height + stackedGap
        }
        return Layout(arrangement: .column, frames: frames)
    }

    /// The slider's one frame, at the first photograph's proportions.
    static func sliderFrame(canvas: CGSize, aspect: CGFloat) -> CGRect {
        let room = CGSize(width: max(canvas.width - 2 * side, 1), height: max(canvas.height - caption - label, 1))
        let size = fit(aspect, in: room)
        return CGRect(x: ((canvas.width - size.width) / 2).rounded(), y: (caption + (room.height - size.height) / 2).rounded(),
                      width: size.width.rounded(), height: size.height.rounded())
    }

    static func fit(_ aspect: CGFloat, in room: CGSize) -> CGSize {
        let aspect = aspect.isFinite && aspect > 0 ? aspect : 1.5
        return room.width / room.height > aspect ? CGSize(width: room.height * aspect, height: room.height)
            : CGSize(width: room.width, height: room.width / aspect)
    }
}

/// What differs between the two photographs, for the pencil under it.
struct CompareDifferences: Equatable {
    /// Indices into `PhotoFacts.parts`.
    var facts: Set<PhotoFacts.Part> = []
    var film = false
    var paper = false

    init(_ a: PhotoFacts?, _ b: PhotoFacts?, film: (String, String)?, paper: (String, String)?) {
        let first = a?.parts ?? [:], second = b?.parts ?? [:]
        for part in PhotoFacts.Part.allCases where first[part] != nil || second[part] != nil {
            if first[part] != second[part] { facts.insert(part) }
        }
        self.film = film.map { $0.0 != $0.1 } ?? false
        self.paper = paper.map { $0.0 != $0.1 } ?? false
    }

    var isEmpty: Bool { facts.isEmpty && !film && !paper }

    /// "shutter", "film and paper", "ISO, shutter and film".
    var words: String {
        var names = PhotoFacts.Part.allCases.filter(facts.contains).map(\.name)
        if film { names.append("film") }
        if paper { names.append("paper") }
        guard names.count > 1 else { return names.first ?? "" }
        return names.dropLast().joined(separator: ", ") + " and " + names.last!
    }
}

extension PhotoFacts {
    enum Part: Int, CaseIterable, Hashable {
        case camera, iso, shutter, aperture, focalLength
        var name: String {
            switch self { case .camera: "camera"; case .iso: "ISO"; case .shutter: "shutter"; case .aperture: "aperture"; case .focalLength: "focal length" }
        }
    }

    /// The caption's items by part, as the caption writes them.
    var parts: [Part: String] {
        var parts: [Part: String] = [:]
        if let camera { parts[.camera] = camera }
        if let iso { parts[.iso] = "ISO \(Int(iso.rounded()))" }
        if let exposure { parts[.shutter] = Self.shutter(exposure) }
        if let aperture { parts[.aperture] = "f/\(aperture.formatted(.number.precision(.fractionLength(0...1))))" }
        if let focalLength { parts[.focalLength] = "\(focalLength.formatted(.number.precision(.fractionLength(0...1)))) mm" }
        return parts
    }
}

/// What the photo view's Original word does (Original ⌄), remembered.
enum OriginalMode: String, CaseIterable {
    /// Hold it to see the whole original: as before.
    case hold
    /// A click toggles a line over the photograph: the original left of it.
    case split
    /// A click compares the photograph with its original.
    case sideBySide

    static let key = "originalMode"
    var title: String {
        switch self { case .hold: "Hold to see the whole"; case .split: "Split line"; case .sideBySide: "Side by side" }
    }
    static func load(_ defaults: UserDefaults = .standard) -> OriginalMode {
        defaults.string(forKey: key).flatMap(OriginalMode.init(rawValue:)) ?? .hold
    }
}

/// What Original shows of a RAW shot as RAW+JPEG (Original ⌄), remembered:
/// the RAW as macOS develops it, or the camera's own JPEG. A photograph
/// without a camera JPEG always shows the RAW.
enum OriginalReference: String, CaseIterable {
    case raw
    case cameraJPEG

    static let key = "originalReference"
    var title: String {
        switch self { case .raw: "Show the RAW"; case .cameraJPEG: "Show the camera JPEG" }
    }
    static func load(_ defaults: UserDefaults = .standard) -> OriginalReference {
        defaults.string(forKey: key).flatMap(OriginalReference.init(rawValue:)) ?? .raw
    }
}

extension PhotoLibrary {
    /// The number a photograph is known by on the roll: its place in the
    /// folder, counted from 1, and a duplicate's letter after it (7a, 7b).
    func frameNumber(of item: URL) -> String {
        let file = item.photoFile
        guard let index = files.firstIndex(where: { $0.url == file }) else { return "" }
        guard let id = item.variantID, let duplicate = duplicates[file]?.firstIndex(of: id) else { return "\(index + 1)" }
        let letters = Array("abcdefghijklmnopqrstuvwxyz")
        return "\(index + 1)\(duplicate < letters.count ? String(letters[duplicate]) : "·\(duplicate + 2)")"
    }
}

// MARK: The model's side

extension EditorModel {
    /// How the menu item and the Compare word read for `targets`.
    struct CompareOffer: Equatable {
        var title: String
        var enabled: Bool
        /// The two to compare, first then second; nil with the original.
        var pair: (URL, URL)?
        var withOriginal = false
        static func == (a: CompareOffer, b: CompareOffer) -> Bool {
            a.title == b.title && a.enabled == b.enabled && a.pair?.0 == b.pair?.0 && a.pair?.1 == b.pair?.1 && a.withOriginal == b.withOriginal
        }
    }

    /// The context menu's item for a right-click on `item` (ticket 46):
    /// worded by the photographs it would act on.
    func compareOffer(for item: URL) -> CompareOffer {
        let targets = contextTargets(item)
        if targets.count > 2 { return CompareOffer(title: "Compare Two Photographs", enabled: false) }
        if targets.count == 2 {
            return CompareOffer(title: "Compare \(library.frameNumber(of: targets[0])) and \(library.frameNumber(of: targets[1]))",
                                enabled: canCompare, pair: (targets[0], targets[1]))
        }
        let current = mode == .grid ? selection.current : sourceItem
        if let current, current != item {
            return CompareOffer(title: "Compare with \(library.frameNumber(of: current))", enabled: canCompare, pair: (current, item))
        }
        return CompareOffer(title: "Compare with \(originalWord)", enabled: canCompare && canCompareWithOriginal(item), withOriginal: true)
    }

    /// The title bar's Compare word, the C key and View › Compare: the two selected, in shown order.
    var selectedPair: (URL, URL)? {
        let selected = selection.ordered(shownPhotos.map(\.url))
        return selected.count == 2 ? (selected[0], selected[1]) : nil
    }

    /// The Compare word's tooltip, and whether it acts.
    var compareWord: (enabled: Bool, help: String) {
        let count = selection.ordered(shownPhotos.map(\.url)).count
        if let (first, second) = selectedPair, canCompare {
            return (true, "Compare \(library.frameNumber(of: first)) and \(library.frameNumber(of: second)) (C)")
        }
        if count > 2 { return (false, "Compare takes two photographs. Three or more can't be compared.") }
        return (false, "Select a second photograph to compare (⌘-click)")
    }

    var canCompare: Bool { source != nil && hasCatalog && !cropping && !closingForCompare }

    private func canCompareWithOriginal(_ item: URL) -> Bool { item == sourceItem && mode == .photo && image != nil }

    /// C and View › Compare: in compare it leaves; otherwise it compares the two selected.
    var canToggleCompare: Bool { comparison != nil || (selectedPair != nil && canCompare) }
    func toggleCompare() {
        if comparison != nil { return leaveCompare() }
        guard let (first, second) = selectedPair else { return }
        compare(first, second)
    }

    /// Compares two photographs: `first` left or on top. The panel edits the
    /// current photograph if it's one of them, else the first.
    func compare(_ first: URL, _ second: URL) {
        guard canCompare, first != second, comparison == nil else { return }
        let origin: Comparison.Origin = mode == .grid ? .grid : .photo
        let current = mode == .grid ? selection.current : sourceItem
        let edited = current == second ? second : first
        begin(Comparison(first: .photo(first), second: .photo(second), activeFirst: edited == first, origin: origin, selection: selection), editing: edited)
    }

    /// The open photograph against its original: developed first, the original second.
    func compareWithOriginal() {
        guard canCompare, comparison == nil, let item = sourceItem, mode == .photo else { return }
        begin(Comparison(first: .photo(item), second: .original(item), activeFirst: true, origin: .photo, selection: selection), editing: item)
    }

    private func begin(_ comparison: Comparison, editing edited: URL) {
        endTray(keep: true)
        holdOriginal(false)
        holdUpsideDown(false)
        clearSplit()
        self.comparison = comparison
        if edited != sourceItem || mode == .grid {
            if edited != sourceItem || sourceDeletedForCompare { openForCompare(edited) } else { setPhotoMode() }
        }
        // Opening the edited one may have narrowed the selection; it's given back on leaving.
        prepareCompareSides()
        pauseThumbnailsForCompare()
    }

    /// Leaves compare for where it was entered: the grid, or the photo view
    /// with the edited photograph open. The selection is as it was.
    func leaveCompare(toGrid: Bool? = nil) {
        guard let comparison else { return }
        let grid = toGrid ?? (comparison.origin == .grid)
        // The grid's sheet flies the two back to their cells from here.
        places.leftCompare = grid ? places.compare.mapValues(\.rect) : [:]
        self.comparison = nil
        compareSteps?.cancel(); compareSteps = nil
        stopCompareDevelop()
        compareSides = [:]
        var restored = comparison.selection
        // The photo view's current photograph is the open one; the grid's
        // can be another, as it was.
        if !grid, let open = sourceItem { restored.focus(open) }
        // Gone since (trashed, or filtered out): what's left of it.
        restored.prune(to: Set(library.shown(current: restored.current).map(\.url)))
        restoreSelection(restored)
        if grid { showGrid() }
        resumeThumbnailsAfterCompare()
    }

    /// S: side by side ↔ slider.
    func toggleCompareLayout() {
        guard var comparison else { return }
        comparison.layout = comparison.layout == .sideBySide ? .slider : .sideBySide
        self.comparison = comparison
    }
    func setCompareLayout(_ layout: Comparison.Layout) { comparison?.layout = layout }
    func moveSplit(to fraction: CGFloat) { comparison?.split = min(max(fraction, 0), 1) }

    /// Swap: the two change places; the same photograph stays edited.
    func swapCompared() {
        guard var comparison else { return }
        swap(&comparison.first, &comparison.second)
        comparison.activeFirst.toggle()
        self.comparison = comparison
    }

    /// A click on a photograph (or its number ring) edits it instead. The
    /// one edited until now keeps its last developed frame on its side.
    func activateCompared(first: Bool) {
        guard var comparison, comparison.activeFirst != first else { return }
        let side = first ? comparison.first : comparison.second
        comparison.activeFirst = first
        self.comparison = comparison
        // Against the original, only which side is active changes.
        guard case .photo(let item) = side, item != sourceItem else { return }
        compareSteps?.cancel(); compareSteps = nil
        keepOpenAsCompared()
        openForCompare(item)
        prepareCompareSides()
    }

    /// ← →: the second photograph steps through the shown photographs,
    /// skipping the first. The ends stop it, as in the strip.
    func stepCompared(by offset: Int) {
        guard var comparison, !comparison.againstOriginal else { return }
        let order = shownPhotos.map(\.url)
        guard let next = Self.compareStep(from: comparison.second.item, skipping: comparison.first.item, in: order, by: offset) else { return }
        let editedStepped = !comparison.activeFirst
        comparison.second = .photo(next)
        self.comparison = comparison
        prepareCompareSides()
        // The edited side opens once the steps stop, with stand-ins meanwhile.
        guard editedStepped else { return }
        compareSteps?.cancel()
        compareSteps = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(250))
            guard let self, !Task.isCancelled, let comparison = self.comparison, comparison.second == .photo(next), !comparison.activeFirst else { return }
            self.compareSteps = nil
            self.openForCompare(next)
            self.prepareCompareSides()
        }
    }

    /// The photograph `offset` places from `from` in `order`, skipping
    /// `first`; nil past the ends. One no longer shown steps from the first.
    nonisolated static func compareStep(from: URL, skipping first: URL, in order: [URL], by offset: Int) -> URL? {
        let candidates = order.filter { $0 != first }
        let start: Int
        if let index = candidates.firstIndex(of: from) {
            start = index
        } else if let index = order.firstIndex(of: first) {
            // From where the first is: the one after it is a step on.
            start = offset > 0 ? index - 1 : index
        } else { return nil }
        let to = start + offset
        return candidates.indices.contains(to) ? candidates[to] : nil
    }

    /// Each side's number on the roll.
    func compareNumber(_ side: Comparison.Side) -> String { library.frameNumber(of: side.item) }

    /// The status line's left-hand words.
    var compareSummary: String {
        guard let comparison else { return "" }
        let first = compareNumber(comparison.first)
        if comparison.againstOriginal { return "Comparing \(first) with its \(originalPhrase)" }
        let differences = compareDifferences
        return "Comparing \(first) and \(compareNumber(comparison.second))" + (differences.isEmpty ? "" : " · differ in \(differences.words)")
    }

    /// What differs between the two, from what's known so far.
    var compareDifferences: CompareDifferences {
        guard let comparison, !comparison.againstOriginal else { return CompareDifferences(nil, nil, film: nil, paper: nil) }
        let a = compareFacts(comparison.first), b = compareFacts(comparison.second)
        let stocks = (compareStocks(comparison.first), compareStocks(comparison.second))
        let film = stocks.0.flatMap { a in stocks.1.map { (a.film, $0.film) } }
        let paper = stocks.0.flatMap { a in stocks.1.map { (a.paper, $0.paper) } }
        return CompareDifferences(a, b, film: film, paper: paper)
    }

    /// The side shows the open photograph, as the panel edits it.
    func comparedIsOpen(_ side: Comparison.Side) -> Bool {
        if case .photo(let item) = side { return item == sourceItem && !sourceDeletedForCompare }
        return false
    }

    func compareFacts(_ side: Comparison.Side) -> PhotoFacts? {
        comparedIsOpen(side) ? photoFacts : compareSides[side]?.facts
    }

    /// The film and paper ids it develops with; nil while unknown.
    func compareStocks(_ side: Comparison.Side) -> (film: String, paper: String)? {
        if comparedIsOpen(side) { return (film, paper) }
        return compareSides[side]?.state.map { ($0.film, $0.paper) }
    }

    func stockName(_ id: String, paper: Bool) -> String {
        (paper ? papers : films).first { $0.id == id }?.name ?? id
    }

    // MARK: The other photograph

    /// Makes sure each side not shown by the open photograph has its
    /// stand-ins and develop under way; drops the rest.
    func prepareCompareSides() {
        guard let comparison else { return }
        let wanted = [comparison.first, comparison.second].filter { !comparedIsOpen($0) }
        for side in compareSides.keys where !wanted.contains(side) { compareSides[side] = nil }
        for side in wanted where compareSides[side] == nil {
            let other = CompareOther(side: side)
            compareSides[side] = other
            load(other)
        }
        scheduleCompareDevelop()
    }

    /// The open photograph, about to be replaced by the other one, stays on
    /// its side with its last developed frame and its edits.
    private func keepOpenAsCompared() {
        guard let item = sourceItem else { return }
        let other = CompareOther(side: .photo(item))
        other.state = state; other.facts = photoFacts; other.filmStrip = filmStrip
        // Clicked away before its develop arrived: its stand-in, never nothing.
        if let image {
            other.show(image, size: imageSize, as: showsSettledFrame ? .developed : .kept)
        } else if let standIn {
            other.show(standIn, size: CanvasArea.standInSize(standIn, photo: placeholderSize), as: .thumbnail)
        } else if let placeholder {
            other.show(placeholder, size: placeholderSize, as: .embedded)
        }
        if other.facts == nil {
            let photo = item.photoFile
            Task { [weak other] in
                let facts = await Task.detached(priority: .userInitiated) { PhotoFacts.read(photo) }.value
                if other?.facts == nil { other?.facts = facts }
            }
        }
        compareSides[.photo(item)] = other
    }

    /// The stand-ins at once, as opening it would show them: its last
    /// develop if kept, else its developed thumbnail, else the camera's preview.
    private func load(_ other: CompareOther) {
        let item = other.item, photo = item.photoFile
        if other.side.isOriginal {
            other.state = state; other.facts = photoFacts
            return loadComparedOriginal(other)
        }
        other.state = comparedState(of: item)
        let thumbnail = developedThumbnail(of: item), kept = keptRender(of: item)
        Task { [weak self, weak other] in
            let facts = await Task.detached(priority: .userInitiated) { PhotoFacts.read(photo) }.value
            other?.facts = facts
            if let kept, let (image, size) = await Task.detached(priority: .userInitiated, operation: kept).value {
                other?.show(image, size: size, as: .kept)
                self?.loadComparedStrip(other)
                return
            }
            if let thumbnail, let image = await Task.detached(priority: .userInitiated, operation: thumbnail).value {
                other?.show(image, size: CanvasArea.standInSize(image, photo: .zero), as: .thumbnail)
            }
            if let (image, size) = await Task.detached(priority: .userInitiated, operation: { Self.thumbnail(photo) }).value {
                other?.show(image, size: size, as: .embedded)
                self?.loadComparedStrip(other)
            }
        }
    }

    /// Its 35mm strip, for the film and the photograph's format, once its size is known.
    private func loadComparedStrip(_ other: CompareOther?) {
        guard let other, other.filmStrip == nil, let state = other.state, state.border.style == .film35, let data = enginePaths?.data,
              let format = FilmFormat(state.crop.size(of: other.size)) else { return }
        other.filmStrip = FilmStrip.load(data: data, film: state.film, format: format)
    }

    /// Show the RAW / Show the camera JPEG changed while comparing with the original.
    func reloadComparedOriginal() {
        guard let comparison, comparison.againstOriginal else { return }
        if let other = compareSides[comparison.first.isOriginal ? comparison.first : comparison.second] { loadComparedOriginal(other) }
    }

    /// The original at the developed preview's resolution, as Original shows it.
    private func loadComparedOriginal(_ other: CompareOther) {
        guard let load = originalLoader() else { return }
        let size = imageSize, word = originalPhrase
        Task { [weak other] in
            let surface = await Task.detached(priority: .userInitiated, operation: load).value
            if let surface { other?.show(surface, size: size, as: .original) } else { other?.failure = "The \(word) couldn’t be shown." }
        }
    }

    /// `item`'s edits as opening it would restore them, or a new
    /// photograph's; nil if its sidecar can't be read.
    func comparedState(of item: URL) -> EditState? {
        guard hasCatalog else { return nil }
        var state: EditState
        if library.looks[item] == nil {
            state = plainState
        } else {
            guard let lookup = sidecarLookup else { return nil }
            let restore = SidecarRestore.read(photo: item.photoFile, variant: item.variantID, store: lookup.store, data: lookup.data)
            guard restore.problem == nil, let saved = restore.edits else { return nil }
            state = restoredState(saved.state, legacy: false, matchSolved: saved.matchSolved)
        }
        state.limitPreview = limitsPreview
        return state
    }

    /// Develops each side that needs it, one at a time, after the open
    /// photograph has settled: never two RAW decodes at once.
    func scheduleCompareDevelop() {
        guard comparison != nil, compareDevelop == nil else { return }
        guard let next = compareSides.values.first(where: { !$0.side.isOriginal && $0.shown != .developed && $0.failure == nil && $0.state != nil }),
              let paths = enginePaths else { return }
        compareDevelop = Task { [weak self] in
            guard let self else { return }
            await self.develop(next, paths: paths)
            self.compareDevelop = nil
            self.scheduleCompareDevelop()
        }
    }

    private func develop(_ other: CompareOther, paths: EnginePaths) async {
        // The edited photograph first.
        while !editedSettled || compareSteps != nil {
            guard comparison != nil, compareSides[other.side] === other, !Task.isCancelled else { return }
            try? await Task.sleep(for: .milliseconds(120))
        }
        guard comparison != nil, compareSides[other.side] === other, var state = other.state else { return }
        let photo = other.item.photoFile
        let engine: EngineClient
        do {
            if let running = compareEngine { engine = running } else { engine = try EngineClient(paths: paths, quality: .userInitiated); compareEngine = engine }
        } catch { other.failure = error.localizedDescription; return }
        compareGeneration += 1
        let generation = compareGeneration, space = previewSpace
        let started = Date()
        do {
            let raw = Self.rawExtensions.contains(photo.pathExtension.lowercased())
            let matchable = raw || state.params["io.input_interpretation.mode"].string == "auto"
            if state.brightnessMatch != nil, matchable, state.brightnessMatch?.ev == nil || state.brightnessMatch?.inputs != matchInputs(state) {
                state = try await solveComparedMatch(state, photo: photo, raw: raw, engine: engine, generation: generation, paths: paths)
                other.state = state
            }
            var rendered = state
            rendered.params["io.output_color_space"] = .string(space)
            let buffer = engine.directory.appendingPathComponent("compare.rgba")
            defer { try? FileManager.default.removeItem(at: buffer) }
            var command: [String: JSONValue] = [
                "command": .string("render"), "input": .string(photo.path), "source_generation": .number(Double(generation)),
                "film": .string(rendered.film), "paper": .string(rendered.paper), "params": rendered.params,
                "limit_preview": .bool(rendered.limitPreview), "preview_path": .string(buffer.path)
            ]
            if ImageSize.mode(of: rendered.params) == .longestSide, let crop = Self.cropJSON(rendered.crop) { command["crop"] = crop }
            let (_, result) = try await engine.request(command)
            guard compareSides[other.side] === other else { return }
            let width = Int(result["width"].double), height = Int(result["height"].double)
            let display = result["display_color_space"].string
            let surface = try await Task.detached(priority: .userInitiated) {
                try Self.readPreview(buffer, width: width, height: height, displaySpace: display)
            }.value
            guard compareSides[other.side] === other else { return }
            let size = CGSize(width: result["photo_width"].double, height: result["photo_height"].double)
            other.show(surface, size: size, as: .developed)
            loadComparedStrip(other)
            compareTimings.append(Date().timeIntervalSince(started))
        } catch {
            if compareSides[other.side] === other { other.failure = error.localizedDescription }
        }
    }

    /// Solves a match its edits haven't solved yet, on the whole decode the
    /// render then uses, as opening it would; the cache first.
    private func solveComparedMatch(_ state: EditState, photo: URL, raw: Bool, engine: EngineClient, generation: Int, paths: EnginePaths) async throws -> EditState {
        let request = matchSolveState(state), inputs = matchInputs(state)
        let key = matchCacheKey(photo, request: request, paths: paths)
        if let key, let solved = matchCache.solved(key) {
            return Self.matching(state, ev: solved.ev, limited: solved.limited, inputs: inputs) ?? state
        }
        var decode: [String: JSONValue] = [
            "command": .string("thumbnail"), "input": .string(photo.path), "source_generation": .number(Double(generation)),
            "film": .string(state.film), "paper": .string(state.paper), "params": state.params,
            "long_side": .number(16), "preview_path": .string(engine.directory.appendingPathComponent("compare-decode.rgba").path),
            "reduced_decode": .bool(false), "decode_only": .bool(true)
        ]
        if raw { decode["preview_colour_space"] = .bool(true) }
        let prepared = raw ? Task.detached(priority: .userInitiated) { CameraBrightnessReference.Prepared(from: photo) } : nil
        let (_, decoded) = try await engine.request(decode)
        let size = CGSize(width: decoded["source_width"].double, height: decoded["source_height"].double)
        let reference = engine.directory.appendingPathComponent("compare-brightness.png")
        defer { try? FileManager.default.removeItem(at: reference) }
        guard let solved = try? await Self.solveBrightnessMatch(engine: engine, source: photo, generation: generation, size: size, raw: raw,
                                                                request: request, reference: reference,
                                                                declared: raw ? decoded["preview_colour_space"] : nil, prepared: prepared),
              let matched = Self.matching(state, ev: solved.exposure, limited: solved.limited, inputs: inputs) else { return state }
        if let key { matchCache.store(key, ev: solved.exposure, limited: solved.limited) }
        return matched
    }

    func stopCompareDevelop() {
        compareDevelop?.cancel(); compareDevelop = nil
        guard let engine = compareEngine else { return }
        compareEngine = nil
        Task.detached(priority: .utility) { engine.stop() }
    }
}

extension LibrarySelection {
    /// Makes `photo` current, keeping the rest selected.
    mutating func focus(_ photo: URL) {
        let others = selected
        select(photo)
        band(others, adding: [photo])
    }
}
