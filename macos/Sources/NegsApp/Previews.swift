import AppKit
import CoreGraphics
import Foundation
import Observation
import SwiftUI

/// Film and paper as foldouts of thumbnails: the photograph developed with
/// each stock and the current settings; and each preset's thumbnail in the
/// Presets panel. `EditorModel` schedules them (`pumpPreviews`); this
/// file holds the pure rules, the tiles and the grid.
enum PreviewRules {
    /// The thumbnail's long edge: two columns of a ~340 pt inspector at 2×.
    static let longSide = 320

    /// The edit with `id` chosen, exactly as choosing it from the menu makes
    /// it. A film brings its own grain/DIR/halation presets, sets scanning for
    /// slides, and brings its own paper unless `keepPaper` (Settings › Keep
    /// paper when changing film); a paper keeps the film.
    static func choosing(_ id: String, paper: Bool, keepPaper: Bool, in state: EditState, stocks: [Stock]) -> EditState {
        var next = state
        if paper {
            next.paper = id; next.params["print_render.development_time"] = .null
            return next
        }
        next.film = id
        next.params["film_render.development_time"] = .null
        // A new film brings its own grain, DIR and halation presets, as upstream applies them.
        for path in Stock.presetPaths { next.params[path] = .null }
        pairFilm(in: &next, stocks: stocks, paper: !keepPaper)
        return next
    }

    /// Scanning for slides, and the film's own paper unless `paper` is false.
    static func pairFilm(in state: inout EditState, stocks: [Stock], paper: Bool = true) {
        guard let film = stocks.first(where: { !$0.paper && $0.id == state.film }) else { return }
        state.params["io.scan_film"] = .bool(film.positive)
        if paper, stocks.contains(where: { $0.paper && $0.id == film.target }), state.paper != film.target {
            state.paper = film.target; state.params["print_render.development_time"] = .null
        }
    }

    /// The helper request for `stock`'s tile, without its scratch file. It
    /// identifies the thumbnail: the photograph (path and generation), every
    /// render setting with the stock chosen, and the crop and straighten.
    /// `state` is the edit as rendered (`EditorModel.renderState`); `keepPaper`
    /// as for `choosing`, so a film's tile shows what a click would develop.
    static func request(for stock: Stock, keepPaper: Bool, state: EditState, stocks: [Stock], source: URL, generation: Int) -> [String: JSONValue] {
        let chosen = choosing(stock.id, paper: stock.paper, keepPaper: keepPaper, in: state, stocks: stocks)
        var command: [String: JSONValue] = [
            "command": .string("thumbnail"), "input": .string(source.path),
            "source_generation": .number(Double(generation)),
            "film": .string(chosen.film), "paper": .string(chosen.paper), "params": chosen.params,
            "long_side": .number(Double(longSide))
        ]
        if let crop = EditorModel.cropJSON(chosen.crop) { command["crop"] = crop }
        return command
    }

    /// The helper request for a preset's tile, without its scratch file: the
    /// helper applies the preset over `state` as `preset_import` does.
    static func presetCommand(text: String, state: EditState, source: URL, generation: Int) -> [String: JSONValue] {
        var command: [String: JSONValue] = [
            "command": .string("thumbnail"), "input": .string(source.path),
            "source_generation": .number(Double(generation)),
            "film": .string(state.film), "paper": .string(state.paper), "params": state.params,
            "long_side": .number(Double(longSide)), "preset": .string(text)
        ]
        if let crop = EditorModel.cropJSON(state.crop) { command["crop"] = crop }
        return command
    }

    /// What identifies a preset's thumbnail: its command with only the
    /// settings a preset keeps (`kept`, the catalog's `preset_kept`), since
    /// the preset replaces the rest. A Lab edit then leaves the tiles alone.
    /// Without the list (an older helper), every setting counts.
    static func presetKey(_ command: [String: JSONValue], kept: [String]?, generation: Int) -> PreviewTile.Key {
        guard let kept else { return PreviewTile.Key(generation: generation, request: command) }
        var request = command
        let params = command["params"] ?? .null
        var subset = JSONValue.object([:])
        for path in kept { subset[path] = params[path] }
        request["params"] = subset
        // The preset brings its own film and paper.
        request["film"] = nil; request["paper"] = nil
        return PreviewTile.Key(generation: generation, request: request)
    }

    /// The white balance a RAW decode uses: the temperature and tint count
    /// only in Custom mode (`RawWhiteBalance::effective`).
    static func effectiveWhiteBalance(_ wb: JSONValue) -> JSONValue {
        let mode = wb["mode"].string.isEmpty ? "as_shot" : wb["mode"].string
        return mode == "custom" ? .object(["mode": .string(mode), "temperature": wb["temperature"], "tint": wb["tint"]])
                                : .object(["mode": .string(mode)])
    }

    /// Whether a preset's RAW white balance makes the helper decode the RAW
    /// again (~1.5 s for 60 MP).
    static func needsOwnDecode(preset: JSONValue, photo: JSONValue) -> Bool {
        effectiveWhiteBalance(preset) != effectiveWhiteBalance(photo)
    }

    /// The presets in list order, except on a RAW: those needing their own
    /// decode come last, grouped by white balance, so each decode is followed
    /// by the presets it serves and the quick ones show first.
    static func renderOrder<P>(_ presets: [P], raw: Bool, whiteBalance: (P) -> JSONValue?, photo: JSONValue) -> [P] {
        guard raw else { return presets }
        var groups: [JSONValue] = []
        let ranked = presets.enumerated().map { index, preset -> (rank: Int, index: Int, preset: P) in
            guard let wb = whiteBalance(preset), needsOwnDecode(preset: wb, photo: photo) else { return (0, index, preset) }
            let effective = effectiveWhiteBalance(wb)
            if !groups.contains(effective) { groups.append(effective) }
            return (1 + groups.firstIndex(of: effective)!, index, preset)
        }
        return ranked.sorted { ($0.rank, $0.index) < ($1.rank, $1.index) }.map(\.preset)
    }

    enum Display: Equatable { case current, dimmed, placeholder }

    /// How a tile shows while `wanted` is its thumbnail: as is, dimmed after
    /// an edit to the same photograph, or blank for another photograph (never
    /// the previous photograph's picture).
    static func display(shown: PreviewTile.Key?, wanted: PreviewTile.Key) -> Display {
        guard let shown else { return .placeholder }
        if shown == wanted { return .current }
        return shown.generation == wanted.generation ? .dimmed : .placeholder
    }
}

/// One stock's or preset's thumbnail. Each tile is observed on its own, so
/// a thumbnail arriving redraws only its tile.
@MainActor @Observable final class PreviewTile {
    struct Key: Equatable {
        let generation: Int
        let request: [String: JSONValue]
    }
    private(set) var image: CGImage?
    private(set) var dimmed = false
    /// What the image (or the failure that left it blank) was made for.
    @ObservationIgnored private(set) var shown: Key?

    init() {}

    /// Updates the look for `wanted`; true when the tile needs a new thumbnail.
    func mark(wanted: Key) -> Bool {
        switch PreviewRules.display(shown: shown, wanted: wanted) {
        case .current: if dimmed { dimmed = false }; return false
        case .dimmed: if !dimmed { dimmed = true }
        case .placeholder:
            if image != nil { image = nil }
            if dimmed { dimmed = false }
        }
        return true
    }
    /// A finished thumbnail, or nil when it failed (blank until the next edit).
    func show(_ image: CGImage?, for key: Key) { self.image = image; shown = key; dimmed = false }
    func clear() { image = nil; shown = nil; dimmed = false }
}

/// A stock and its tile.
struct StockEntry {
    let stock: Stock
    let tile: PreviewTile
}

/// The tiles, the foldouts' open state and the settings.
@MainActor @Observable final class Previews {
    static let defaultsKey = "previews"
    static let keepPaperKey = "keep_paper"
    /// Settings › Film, paper and preset previews; a missing value means on.
    var enabled: Bool { didSet { defaults.set(enabled, forKey: Self.defaultsKey) } }
    /// Settings › Keep paper when changing film; a missing value means off.
    /// Here because the film tiles follow it too.
    var keepPaper: Bool { didSet { defaults.set(keepPaper, forKey: Self.keepPaperKey) } }
    static let favoriteFilmsKey = "favoriteFilms"
    static let favoritePapersKey = "favoritePapers"
    /// Starred for new users, as on the web; once they change the list,
    /// theirs is kept, even when empty.
    static let defaultFavoriteFilms = ["kodak_gold_200", "kodak_portra_400", "kodak_ektar_100", "fujifilm_c200"]
    /// Starred stock ids, in the order starred. Ids the catalog no longer
    /// has are kept, in case it brings them back.
    private(set) var favoriteFilms: [String] { didSet { defaults.set(favoriteFilms, forKey: Self.favoriteFilmsKey) } }
    private(set) var favoritePapers: [String] { didSet { defaults.set(favoritePapers, forKey: Self.favoritePapersKey) } }
    @ObservationIgnored private let defaults: UserDefaults
    /// Collapsed at every launch.
    var filmOpen = false
    var paperOpen = false
    /// Whether each grid is on screen (not on another tab or pane).
    @ObservationIgnored var filmShown = false
    @ObservationIgnored var paperShown = false
    /// Whether the Presets panel's list is on screen.
    @ObservationIgnored var presetsShown = false
    @ObservationIgnored private(set) var films: [StockEntry] = []
    @ObservationIgnored private(set) var papers: [StockEntry] = []
    /// By preset file (`moveTile` follows a rename).
    @ObservationIgnored private(set) var presets: [URL: PreviewTile] = [:]
    @ObservationIgnored var busy = false

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        enabled = defaults.object(forKey: Self.defaultsKey) as? Bool ?? true
        keepPaper = defaults.object(forKey: Self.keepPaperKey) as? Bool ?? false
        favoriteFilms = defaults.stringArray(forKey: Self.favoriteFilmsKey) ?? Self.defaultFavoriteFilms
        favoritePapers = defaults.stringArray(forKey: Self.favoritePapersKey) ?? []
    }

    func isFavorite(_ id: String, paper: Bool) -> Bool { (paper ? favoritePapers : favoriteFilms).contains(id) }
    /// Stars `id` (last among the favourites) or unstars it.
    func toggleFavorite(_ id: String, paper: Bool) {
        var ids = paper ? favoritePapers : favoriteFilms
        if let index = ids.firstIndex(of: id) { ids.remove(at: index) } else { ids.append(id) }
        if paper { favoritePapers = ids } else { favoriteFilms = ids }
    }
    /// The starred stocks among `stocks`, in the order starred.
    func favorites(in stocks: [Stock], paper: Bool) -> [Stock] {
        (paper ? favoritePapers : favoriteFilms).compactMap { id in stocks.first { $0.paper == paper && $0.id == id } }
    }
    /// The tiles of one kind, favourites first, so their thumbnails come first.
    var filmsFavoritesFirst: [StockEntry] { favoritesFirst(films, paper: false) }
    var papersFavoritesFirst: [StockEntry] { favoritesFirst(papers, paper: true) }
    private func favoritesFirst(_ entries: [StockEntry], paper: Bool) -> [StockEntry] {
        let ids = paper ? favoritePapers : favoriteFilms
        let starred = ids.compactMap { id in entries.first { $0.stock.id == id } }
        return starred + entries.filter { !ids.contains($0.stock.id) }
    }

    /// Tiles for the catalog's stocks, kept across catalog reloads by id.
    func update(stocks: [Stock]) {
        let old = Dictionary((films + papers).map { ("\($0.stock.paper):\($0.stock.id)", $0.tile) }, uniquingKeysWith: { a, _ in a })
        func entries(_ paper: Bool) -> [StockEntry] {
            stocks.filter { $0.paper == paper }.map { StockEntry(stock: $0, tile: old["\(paper):\($0.id)"] ?? PreviewTile()) }
        }
        films = entries(false); papers = entries(true)
    }
    func tile(_ id: String, paper: Bool) -> PreviewTile? { (paper ? papers : films).first { $0.stock.id == id }?.tile }

    /// Tiles for the listed presets, kept by file.
    func update(presets files: [PresetFile]) {
        presets = Dictionary(files.map { ($0.url, presets[$0.url] ?? PreviewTile()) }, uniquingKeysWith: { a, _ in a })
    }
    func tile(preset: URL) -> PreviewTile? { presets[preset] }
    /// A renamed preset keeps its thumbnail.
    func moveTile(preset old: URL, to new: URL) {
        if let tile = presets.removeValue(forKey: old) { presets[new] = tile }
    }

    /// Drops every thumbnail; true if there were any.
    func clear() -> Bool {
        var had = false
        for tile in (films + papers).map(\.tile) + presets.values where tile.shown != nil || tile.image != nil {
            had = true; tile.clear()
        }
        return had
    }
}

// MARK: Views

/// A Film or Paper foldout: the current stock's name on the header, a
/// two-column grid of tiles under the menu's headings inside. Film, the
/// long list, has a search field over its grid.
/// The stocks as developed tiles, with search for films: opened from a
/// name in the label under the photograph. Until the tray (step 4) it's a
/// popover there.
struct StockFoldout: View {
    let model: EditorModel
    let paper: Bool
    @State private var query = ""
    static let size = CGSize(width: 460, height: 400)
    static func favoriteID(_ id: String) -> String { "favorite:\(id)" }
    /// The stock a tile or menu tag stands for.
    static func stockID(tag: String) -> String { tag.hasPrefix("favorite:") ? String(tag.dropFirst("favorite:".count)) : tag }
    /// The current stock's tile, the copy among the favourites if starred.
    private var scrollID: String {
        let id = paper ? model.paper : model.film
        return model.previews.isFavorite(id, paper: paper) ? Self.favoriteID(id) : id
    }

    /// Opens or closes the picker, which starts or stops its tiles developing.
    static func setOpen(_ open: Bool, paper: Bool, model: EditorModel) {
        if paper { model.previews.paperOpen = open } else { model.previews.filmOpen = open }
        model.pumpPreviews()
    }

    var body: some View {
        // It opens on the current stock.
        ScrollViewReader { proxy in
            VStack(spacing: 8) {
                if !paper {
                    StockSearchField(text: $query, prompt: "Search films") {
                        // Return takes the first match, as listed.
                        if let first = StockGroup.matching(query, in: model.filmGroups).first?.stocks.first {
                            model.chooseFilm(first.id)
                        }
                    }
                }
                ScrollView {
                    StockGrid(model: model, paper: paper, query: query)
                }
                .onAppear { proxy.scrollTo(scrollID, anchor: .center) }
                // Each search starts at the top of its matches; clearing it
                // goes back to the current stock.
                .onChange(of: query) {
                    if query.isEmpty { proxy.scrollTo(scrollID, anchor: .center) } else { proxy.scrollTo(StockGrid.topID, anchor: .top) }
                }
            }
            .padding(12)
        }
        .frame(width: Self.size.width, height: Self.size.height)
    }
}

private struct StockGrid: View {
    let model: EditorModel
    let paper: Bool
    var query = ""
    static let topID = "stock-grid-top"

    var body: some View {
        let previews = model.previews
        let searching = !query.trimmingCharacters(in: .whitespaces).isEmpty
        let groups = StockGroup.matching(query, in: paper ? model.paperGroups : model.filmGroups)
        // While searching, the matches show once, under their groups.
        let favorites = searching ? [] : previews.favorites(in: paper ? model.papers : model.films, paper: paper)
        Color.clear.frame(height: 0).id(Self.topID)
        if groups.isEmpty {
            Text("No \(paper ? "papers" : "films") match “\(query.trimmingCharacters(in: .whitespaces))”")
                .font(.callout).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity).padding(.top, 24)
        }
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 8, alignment: .top), GridItem(.flexible(), spacing: 8, alignment: .top)],
                  alignment: .leading, spacing: 8) {
            // The favourites first, untitled; each stays in its group too.
            if !favorites.isEmpty {
                Section { tiles(favorites, favorites: true) }
            }
            ForEach(groups) { group in
                Section {
                    tiles(group.stocks)
                } header: {
                    Text(group.title).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                }
            }
        }
        .padding(.vertical, 4)
        .onAppear { setShown(true) }
        .onDisappear { setShown(false) }
    }

    /// A favourite's tile is in the grid twice, so the copy on top has its
    /// own scroll id (`StockFoldout.scrollID`).
    private func tiles(_ stocks: [Stock], favorites: Bool = false) -> some View {
        let previews = model.previews
        let selected = paper ? model.paper : model.film
        return ForEach(stocks) { stock in
            if let tile = previews.tile(stock.id, paper: paper) {
                StockTileView(stock: stock, tile: tile, selected: stock.id == selected,
                              favorite: previews.isFavorite(stock.id, paper: paper),
                              toggleFavorite: { previews.toggleFavorite(stock.id, paper: paper) }) {
                    if paper { model.choosePaper(stock.id) } else { model.chooseFilm(stock.id) }
                }
                .id(favorites ? StockFoldout.favoriteID(stock.id) : stock.id)
            }
        }
    }

    private func setShown(_ shown: Bool) {
        if paper { model.previews.paperShown = shown } else { model.previews.filmShown = shown }
        model.pumpPreviews()
    }
}

/// A native search field: filters as you type, Escape or its clear button
/// empties it, Return calls `submit`.
private struct StockSearchField: NSViewRepresentable {
    @Binding var text: String
    let prompt: String
    let submit: () -> Void

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.controlSize = .small
        field.font = .systemFont(ofSize: NSFont.systemFontSize(for: .small))
        field.placeholderString = prompt
        field.setAccessibilityLabel(prompt)
        field.sendsSearchStringImmediately = true
        // Never the system blue (§1): no focus ring; the caret is `paper`.
        field.focusRingType = .none
        field.target = context.coordinator
        field.action = #selector(Coordinator.changed(_:))
        field.delegate = context.coordinator
        field.stringValue = text
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != text { field.stringValue = text }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, NSSearchFieldDelegate {
        var parent: StockSearchField
        init(_ parent: StockSearchField) { self.parent = parent }

        @objc func changed(_ field: NSSearchField) {
            if parent.text != field.stringValue { parent.text = field.stringValue }
        }

        func controlTextDidBeginEditing(_ notification: Notification) {
            if let editor = (notification.object as? NSTextField)?.currentEditor() as? NSTextView { InkTextField.style(editor) }
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.insertNewline(_:)) else { return false }
            parent.submit()
            return true
        }
    }
}

private struct StockTileView: View {
    let stock: Stock
    let tile: PreviewTile
    let selected: Bool
    let favorite: Bool
    let toggleFavorite: () -> Void
    let choose: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: choose) {
            VStack(alignment: .leading, spacing: 3) {
                // The thumbnail is an overlay so its fill can't widen the
                // tile past its column (as a ZStack child it did, and the
                // tiles overlapped).
                RoundedRectangle(cornerRadius: 5).fill(.quaternary)
                    .frame(maxWidth: .infinity).frame(height: 92)
                    .overlay {
                        if let image = tile.image {
                            Image(decorative: image, scale: 2).resizable().aspectRatio(contentMode: .fill)
                                .opacity(tile.dimmed ? 0.55 : 1)
                        }
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 5))
                .overlay {
                    if selected { RoundedRectangle(cornerRadius: 5).strokeBorder(Theme.accent, lineWidth: 2) }
                }
                Text(stock.name).font(.caption).lineLimit(1).truncationMode(.tail)
                    .foregroundStyle(selected ? .primary : .secondary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(stock.name)
        .accessibilityLabel(stock.name)
        .accessibilityAddTraits(selected ? .isSelected : [])
        // The star sits over the thumbnail's corner, outside the tile's own
        // button so a click stars rather than chooses. Shown on a favourite,
        // or while the pointer is over the tile.
        .overlay(alignment: .topTrailing) {
            if favorite || hovering {
                Button(action: toggleFavorite) {
                    Image(systemName: favorite ? "star.fill" : "star")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(favorite ? Color.yellow : Color.white)
                        .shadow(color: .black.opacity(0.6), radius: 1.5)
                        .frame(width: 22, height: 22)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(favorite ? "Remove from favorites" : "Add to favorites")
                .accessibilityLabel(favorite ? "Remove \(stock.name) from favorites" : "Add \(stock.name) to favorites")
            }
        }
        .onHover { hovering = $0 }
    }
}
