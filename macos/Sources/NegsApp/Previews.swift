import AppKit
import CoreGraphics
import Foundation
import Observation
import SwiftUI

/// Film and paper as trays of thumbnails: the photograph developed with
/// each stock and the current settings; and each preset's thumbnail in the
/// Presets panel. `EditorModel` schedules them (`pumpPreviews`); this
/// file holds the pure rules, the tiles and the grid.
enum PreviewRules {
    /// The thumbnail's long edge: a 212 × 118 pt tile in the tray at 2×
    /// (a 3:2 landscape fills it), and sharp enough to stand in, enlarged, for
    /// a tried stock until its develop is in.
    static let longSide = 480

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

/// The tiles, which tray is open and the settings.
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
    /// Which tray is open (`EditorModel.tray`): its tiles develop.
    var filmOpen = false
    var paperOpen = false
    /// Whether each tray's tiles are on screen.
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

/// The plain menu's tags, with the previews off in Settings: a favourite
/// is listed twice, so its copy among the favourites has its own tag.
enum StockMenu {
    static func favoriteID(_ id: String) -> String { "favorite:\(id)" }
    /// The stock a menu tag stands for.
    static func stockID(tag: String) -> String { tag.hasPrefix("favorite:") ? String(tag.dropFirst("favorite:".count)) : tag }
}
