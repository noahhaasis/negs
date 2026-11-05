import XCTest
@testable import NegsApp

final class PreviewTests: XCTestCase {
    private let stocks: [Stock] = [
        Stock(.object(["id": .string("gold"), "name": .string("Gold 200"), "paper": .bool(false), "positive": .bool(false),
                       "bw": .bool(false), "target_print": .string("endura"), "development_times": .array([])])),
        Stock(.object(["id": .string("velvia"), "name": .string("Velvia 50"), "paper": .bool(false), "positive": .bool(true),
                       "bw": .bool(false), "target_print": .string("endura"), "development_times": .array([])])),
        Stock(.object(["id": .string("trix"), "name": .string("Tri-X 400"), "paper": .bool(false), "positive": .bool(false),
                       "bw": .bool(true), "target_print": .string("ilford"), "development_times": .array([])])),
        Stock(.object(["id": .string("endura"), "name": .string("Endura"), "paper": .bool(true), "positive": .bool(false),
                       "bw": .bool(false), "target_print": .string(""), "development_times": .array([])])),
        Stock(.object(["id": .string("ilford"), "name": .string("Multigrade"), "paper": .bool(true), "positive": .bool(false),
                       "bw": .bool(true), "target_print": .string(""), "development_times": .array([])]))
    ]

    private func edited() -> EditState {
        var state = EditState()
        state.film = "gold"; state.paper = "endura"
        state.params["camera.exposure_compensation_ev"] = .number(0.7)
        state.params["io.scan_film"] = .bool(false)
        state.params["film_render.development_time"] = .number(6)
        state.params["print_render.development_time"] = .number(2)
        for path in Stock.presetPaths { state.params[path] = .number(1.5) }
        return state
    }

    func testChoosingAFilmPairsItsPaperAndResetsItsPresets() {
        let state = PreviewRules.choosing("trix", paper: false, keepPaper: false, in: edited(), stocks: stocks)
        XCTAssertEqual(state.film, "trix")
        XCTAssertEqual(state.paper, "ilford")
        XCTAssertEqual(state.params["film_render.development_time"], .null)
        XCTAssertEqual(state.params["print_render.development_time"], .null)
        for path in Stock.presetPaths { XCTAssertEqual(state.params[path], .null, path) }
        XCTAssertEqual(state.params["io.scan_film"], .bool(false))
        // Every other setting carries over.
        XCTAssertEqual(state.params["camera.exposure_compensation_ev"], .number(0.7))

        let slide = PreviewRules.choosing("velvia", paper: false, keepPaper: false, in: edited(), stocks: stocks)
        XCTAssertEqual(slide.params["io.scan_film"], .bool(true))
        // Its own paper is the current one, so the print development time stays.
        XCTAssertEqual(slide.paper, "endura")
        XCTAssertEqual(slide.params["print_render.development_time"], .number(2))
    }

    func testKeepPaperLeavesThePaperAlone() {
        // Gold's paper is Endura; start from Multigrade with a print time.
        var start = edited(); start.paper = "ilford"
        for keep in [false, true] {
            let state = PreviewRules.choosing("gold", paper: false, keepPaper: keep, in: start, stocks: stocks)
            XCTAssertEqual(state.film, "gold")
            XCTAssertEqual(state.paper, keep ? "ilford" : "endura")
            XCTAssertEqual(state.params["print_render.development_time"], keep ? .number(2) : .null)
            // Everything else a film change does still happens.
            XCTAssertEqual(state.params["film_render.development_time"], .null)
            for path in Stock.presetPaths { XCTAssertEqual(state.params[path], .null, path) }
        }
        // Slides still scan directly.
        let slide = PreviewRules.choosing("velvia", paper: false, keepPaper: true, in: start, stocks: stocks)
        XCTAssertEqual(slide.params["io.scan_film"], .bool(true))
        XCTAssertEqual(slide.paper, "ilford")
        // A film's tile shows it on the current paper.
        let photo = URL(fileURLWithPath: "/photos/a.jpg"), gold = stocks[0]
        let kept = PreviewRules.request(for: gold, keepPaper: true, state: start, stocks: stocks, source: photo, generation: 1)
        let paired = PreviewRules.request(for: gold, keepPaper: false, state: start, stocks: stocks, source: photo, generation: 1)
        XCTAssertEqual(kept["paper"], .string("ilford"))
        XCTAssertEqual(paired["paper"], .string("endura"))
    }

    func testChoosingAPaperKeepsTheFilm() {
        let state = PreviewRules.choosing("ilford", paper: true, keepPaper: false, in: edited(), stocks: stocks)
        XCTAssertEqual(state.film, "gold")
        XCTAssertEqual(state.paper, "ilford")
        XCTAssertEqual(state.params["print_render.development_time"], .null)
        XCTAssertEqual(state.params["film_render.development_time"], .number(6))
        for path in Stock.presetPaths { XCTAssertEqual(state.params[path], .number(1.5), path) }
    }

    func testTheTileKeyFollowsTheCropAndThePhotograph() {
        let photo = URL(fileURLWithPath: "/photos/a.jpg"), trix = stocks[2]
        func key(_ state: EditState, _ source: URL = photo, generation: Int = 1) -> PreviewTile.Key {
            PreviewTile.Key(generation: generation, request: PreviewRules.request(for: trix, keepPaper: false, state: state, stocks: stocks, source: source, generation: generation))
        }
        let base = key(edited())
        XCTAssertNil(base.request["crop"])
        XCTAssertEqual(base.request["film"], .string("trix"))
        XCTAssertEqual(base.request["paper"], .string("ilford"))

        var cropped = edited(); cropped.crop.rect = CGRect(x: 0.1, y: 0.1, width: 0.8, height: 0.8)
        XCTAssertNotEqual(key(cropped), base)
        var straightened = edited(); straightened.crop.angle = 3
        XCTAssertNotEqual(key(straightened), base)
        XCTAssertEqual(key(straightened).request["crop"]?["angle"], .number(3))
        // A border is presentation only; the thumbnail ignores it.
        var bordered = edited(); bordered.border.enabled = true
        XCTAssertEqual(key(bordered), base)

        XCTAssertNotEqual(key(edited(), URL(fileURLWithPath: "/photos/b.jpg")), base)
        XCTAssertNotEqual(key(edited(), generation: 2), base)
        var exposed = edited(); exposed.params["camera.exposure_compensation_ev"] = .number(1)
        XCTAssertNotEqual(key(exposed), base)
    }

    func testAPresetTileFollowsOnlyWhatThePresetKeeps() {
        let photo = URL(fileURLWithPath: "/photos/a.jpg"), text = #"{"simulation": {"film_stock": "kodak_portra_400"}}"#
        let kept = ["io.output_color_space", "io.crop", "post"]
        func command(_ state: EditState, text: String = text, generation: Int = 1) -> [String: JSONValue] {
            PreviewRules.presetCommand(text: text, state: state, source: photo, generation: generation)
        }
        func key(_ state: EditState, text: String = text, generation: Int = 1) -> PreviewTile.Key {
            PreviewRules.presetKey(command(state, text: text, generation: generation), kept: kept, generation: generation)
        }
        var state = edited()
        state.params["io.output_color_space"] = .string("sRGB")
        state.params["post.saturation"] = .number(0)
        // The helper gets every setting and the preset; it applies the preset.
        let sent = command(state)
        XCTAssertEqual(sent["preset"], .string(text))
        XCTAssertEqual(sent["params"], state.params)
        XCTAssertEqual(sent["film"], .string("gold"))

        let base = key(state)
        // The preset replaces the Lab settings and the stocks, so they don't count.
        var exposed = state; exposed.params["camera.exposure_compensation_ev"] = .number(2)
        XCTAssertEqual(key(exposed), base)
        XCTAssertEqual(key(PreviewRules.choosing("trix", paper: false, keepPaper: false, in: state, stocks: stocks)), base)
        // What it keeps, the crop, the photograph and the file's text do.
        var space = state; space.params["io.output_color_space"] = .string("Display P3")
        XCTAssertNotEqual(key(space), base)
        var post = state; post.params["post.saturation"] = .number(0.5)
        XCTAssertNotEqual(key(post), base)
        var cropped = state; cropped.crop.angle = 3
        XCTAssertNotEqual(key(cropped), base)
        XCTAssertNotEqual(key(state, generation: 2), base)
        XCTAssertNotEqual(key(state, text: #"{"simulation": {"film_stock": "kodak_gold_200"}}"#), base)
        // An older helper doesn't list what presets keep: every setting counts.
        let everything = PreviewRules.presetKey(command(exposed), kept: nil, generation: 1)
        XCTAssertNotEqual(everything, PreviewRules.presetKey(command(state), kept: nil, generation: 1))
    }

    func testPresetsNeedingTheirOwnRawDecodeComeLast() {
        let asShot: JSONValue = .object(["mode": .string("as_shot"), "temperature": .number(5500), "tint": .number(1)])
        let daylight: JSONValue = .object(["mode": .string("daylight"), "temperature": .number(5500), "tint": .number(1)])
        var daylightOther = daylight; daylightOther["temperature"] = .number(6500)
        let custom: JSONValue = .object(["mode": .string("custom"), "temperature": .number(5500), "tint": .number(1)])
        var customOther = custom; customOther["temperature"] = .number(6500)
        // Temperature and tint count only for a custom white balance.
        XCTAssertFalse(PreviewRules.needsOwnDecode(preset: daylightOther, photo: daylight))
        XCTAssertTrue(PreviewRules.needsOwnDecode(preset: daylight, photo: asShot))
        XCTAssertTrue(PreviewRules.needsOwnDecode(preset: customOther, photo: custom))
        XCTAssertFalse(PreviewRules.needsOwnDecode(preset: custom, photo: custom))

        // Quick ones first, then each decode followed by the presets it serves.
        let balances = ["a": daylight, "b": asShot, "c": custom, "d": asShot, "e": daylightOther]
        let order = { (raw: Bool) in
            PreviewRules.renderOrder(["a", "b", "c", "d", "e", "unknown"], raw: raw, whiteBalance: { balances[$0] }, photo: asShot)
        }
        XCTAssertEqual(order(true), ["b", "d", "unknown", "a", "e", "c"])
        XCTAssertEqual(order(false), ["a", "b", "c", "d", "e", "unknown"])
    }

    func testStaleTilesDimForTheSamePhotographAndBlankForAnother() {
        let request: [String: JSONValue] = ["film": .string("gold")]
        let shown = PreviewTile.Key(generation: 3, request: request)
        XCTAssertEqual(PreviewRules.display(shown: shown, wanted: shown), .current)
        XCTAssertEqual(PreviewRules.display(shown: shown, wanted: .init(generation: 3, request: ["film": .string("trix")])), .dimmed)
        XCTAssertEqual(PreviewRules.display(shown: shown, wanted: .init(generation: 4, request: request)), .placeholder)
        XCTAssertEqual(PreviewRules.display(shown: nil, wanted: shown), .placeholder)
    }

    @MainActor func testTilesKeepTheirPictureOnlyForTheSamePhotograph() throws {
        let tile = PreviewTile()
        let image = try EditorModel.thumbnailImage(Data(repeating: 128, count: 4 * 4 * 4), width: 4, height: 4, displaySpace: "sRGB")
        let first = PreviewTile.Key(generation: 1, request: ["ev": .number(0)])
        XCTAssertTrue(tile.mark(wanted: first))
        tile.show(image, for: first)
        XCTAssertFalse(tile.mark(wanted: first))
        XCTAssertTrue(tile.mark(wanted: .init(generation: 1, request: ["ev": .number(1)])))
        XCTAssertNotNil(tile.image); XCTAssertTrue(tile.dimmed)
        XCTAssertTrue(tile.mark(wanted: .init(generation: 2, request: ["ev": .number(1)])))
        XCTAssertNil(tile.image); XCTAssertFalse(tile.dimmed)
        // A failure is remembered, so the same request isn't retried.
        let failed = PreviewTile.Key(generation: 2, request: ["ev": .number(1)])
        tile.show(nil, for: failed)
        XCTAssertFalse(tile.mark(wanted: failed))
    }

    @MainActor func testMissingSettingMeansPreviewsOn() {
        let suite = "negs-previews-\(UUID().uuidString)", defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let previews = Previews(defaults: defaults)
        XCTAssertTrue(previews.enabled)
        previews.enabled = false
        XCTAssertFalse(Previews(defaults: defaults).enabled)
    }

    @MainActor func testMissingSettingMeansThePaperFollowsTheFilm() {
        let suite = "negs-keep-paper-\(UUID().uuidString)", defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let previews = Previews(defaults: defaults)
        XCTAssertFalse(previews.keepPaper)
        previews.keepPaper = true
        XCTAssertTrue(Previews(defaults: defaults).keepPaper)
        XCTAssertEqual(defaults.object(forKey: "keep_paper") as? Bool, true)
    }

    @MainActor func testFavoritesKeepTheOrderStarredAndPersist() {
        let suite = "negs-favorites-\(UUID().uuidString)", defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let previews = Previews(defaults: defaults)
        XCTAssertTrue(previews.favorites(in: stocks, paper: false).isEmpty)
        previews.toggleFavorite("trix", paper: false)
        previews.toggleFavorite("gold", paper: false)
        previews.toggleFavorite("ilford", paper: true)
        // A film id among the papers isn't a favourite paper.
        XCTAssertFalse(previews.isFavorite("trix", paper: true))

        let reloaded = Previews(defaults: defaults)
        XCTAssertEqual(reloaded.favorites(in: stocks, paper: false).map(\.id), ["trix", "gold"])
        XCTAssertEqual(reloaded.favorites(in: stocks, paper: true).map(\.id), ["ilford"])
        reloaded.update(stocks: stocks)
        XCTAssertEqual(reloaded.filmsFavoritesFirst.map(\.stock.id), ["trix", "gold", "velvia"])

        reloaded.toggleFavorite("trix", paper: false)
        reloaded.toggleFavorite("trix", paper: false)
        XCTAssertEqual(Previews(defaults: defaults).favorites(in: stocks, paper: false).map(\.id), ["gold", "trix"])
    }

    func testFilmSearchMatchesEveryWordInNameIdOrGroup() {
        let films = stocks.filter { !$0.paper }, papers = stocks.filter(\.paper)
        let groups = StockGroup.films(films, papers: papers)
        func ids(_ query: String) -> [String] { StockGroup.matching(query, in: groups).flatMap(\.stocks).map(\.id) }
        XCTAssertEqual(ids(""), ["gold", "velvia", "trix"])
        XCTAssertEqual(ids("  "), ["gold", "velvia", "trix"])
        XCTAssertEqual(ids("tri-x"), ["trix"])
        XCTAssertEqual(ids("GOLD 200"), ["gold"])
        XCTAssertEqual(ids("slide"), ["velvia"])
        XCTAssertEqual(ids("black 400"), ["trix"])
        XCTAssertEqual(ids("gold 400"), [])
        XCTAssertEqual(StockGroup.matching("velvia", in: groups).map(\.title), ["Color Slide"])
    }

    func testAFavoritesOwnTagStandsForItsStock() {
        XCTAssertEqual(StockFoldout.stockID(tag: StockFoldout.favoriteID("kodak_gold_200")), "kodak_gold_200")
        XCTAssertEqual(StockFoldout.stockID(tag: "kodak_gold_200"), "kodak_gold_200")
        XCTAssertNotEqual(StockFoldout.favoriteID("kodak_gold_200"), "kodak_gold_200")
    }

    @MainActor func testAFavoriteMissingFromTheCatalogIsKeptButNotShown() {
        let suite = "negs-favorites-missing-\(UUID().uuidString)", defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set(["gone", "velvia"], forKey: Previews.favoriteFilmsKey)
        let previews = Previews(defaults: defaults)
        XCTAssertEqual(previews.favorites(in: stocks, paper: false).map(\.id), ["velvia"])
        previews.toggleFavorite("gold", paper: false)
        XCTAssertEqual(defaults.stringArray(forKey: Previews.favoriteFilmsKey), ["gone", "velvia", "gold"])
    }
}
