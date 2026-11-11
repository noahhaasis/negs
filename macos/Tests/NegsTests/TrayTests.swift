import AppKit
import SwiftUI
import XCTest
@testable import NegsApp

/// The tray's model (IMPLEMENTATION.md §5.1, §6 item 8): a click tries a
/// stock and the tray stays open, the whole visit is one undo step, Esc
/// puts back the stock from before, ← → step stocks, and the tried stock's
/// cached tile shows at once.
@MainActor final class TrayTests: XCTestCase {
    private func stock(_ id: String, _ name: String, paper: Bool = false, bw: Bool = false, positive: Bool = false) -> Stock {
        Stock(.object(["id": .string(id), "name": .string(name), "paper": .bool(paper), "positive": .bool(positive),
                       "bw": .bool(bw), "target_print": .string(paper ? "" : "endura"), "development_times": .array([])]))
    }

    // MARK: Rules

    func testFavouritesComeFirstAndHideWhileSearching() {
        let gold = stock("gold", "Kodak Gold 200"), portra = stock("portra", "Kodak Portra 400"), trix = stock("trix", "Kodak Tri-X 400", bw: true)
        let groups = [StockGroup(title: "Color Negative", stocks: [gold, portra]), StockGroup(title: "Black & White", stocks: [trix])]
        let sections = StockTrayRules.sections(groups, favorites: [portra], query: "")
        XCTAssertEqual(sections.map(\.title), ["Favourites", "Color Negative", "Black & White"])
        XCTAssertEqual(sections[0].stocks.map(\.id), ["portra"])
        XCTAssertEqual(sections[1].stocks.map(\.id), ["gold", "portra"], "A favourite stays in its own group too")
        XCTAssertTrue(sections[0].favorites)

        let searching = StockTrayRules.sections(groups, favorites: [portra], query: "400")
        XCTAssertEqual(searching.map(\.title), ["Color Negative", "Black & White"], "No favourites while searching")
        XCTAssertEqual(searching.flatMap { $0.stocks.map(\.id) }, ["portra", "trix"])
        XCTAssertTrue(StockTrayRules.sections(groups, favorites: [], query: "velvia").isEmpty)
        XCTAssertEqual(StockTrayRules.noMatch(" velvia 400 ", paper: false), "No films match “velvia 400”")
        XCTAssertEqual(StockTrayRules.noMatch("x", paper: true), "No papers match “x”")
    }

    func testTheStarSpellsFavouritesAsTheHeadingDoes() {
        XCTAssertEqual(StockTrayRules.starHelp(favourite: true), "Remove from favourites")
        XCTAssertEqual(StockTrayRules.starHelp(favourite: false), "Add to favourites")
        XCTAssertEqual(StockTrayRules.starLabel("Kodak Gold 200", favourite: true), "Remove Kodak Gold 200 from favourites")
        XCTAssertEqual(StockTrayRules.starLabel("Kodak Gold 200", favourite: false), "Add Kodak Gold 200 to favourites")
        XCTAssertTrue(StockTrayRules.starHelp(favourite: true).contains(StockTrayRules.favouritesTitle.lowercased()))
    }

    func testArrowsStepEachTileAndHoldAtTheEnds() {
        let groups = [StockGroup(title: "A", stocks: [stock("a1", "A1"), stock("a2", "A2")]), StockGroup(title: "B", stocks: [stock("b1", "B1")])]
        let places = StockTrayRules.places(StockTrayRules.sections(groups, favorites: [], query: ""))
        XCTAssertEqual(places.map(\.stock), ["a1", "a2", "b1"])
        func step(_ from: String, _ by: Int) -> String? { StockTrayRules.step(from: from, inFavourites: true, by: by, in: places)?.stock }
        XCTAssertEqual(step("a2", 1), "b1", "Across groups")
        XCTAssertEqual(step("a2", -1), "a1")
        XCTAssertEqual(step("b1", 1), "b1", "Held at the end")
        XCTAssertEqual(step("a1", -1), "a1", "Held at the start")
        XCTAssertEqual(step("gone", 1), "a1", "From a stock not listed (search), the first")
        XCTAssertNil(StockTrayRules.step(from: "a1", inFavourites: true, by: 1, in: []))
    }

    /// ← → go through the favourites too, then on into the groups, from
    /// whichever copy of a favourite was opened on, clicked or stepped to.
    func testArrowsStepThroughTheFavourites() {
        let a1 = stock("a1", "A1"), a2 = stock("a2", "A2"), b1 = stock("b1", "B1")
        let groups = [StockGroup(title: "A", stocks: [a1, a2]), StockGroup(title: "B", stocks: [b1])]
        let places = StockTrayRules.places(StockTrayRules.sections(groups, favorites: [b1, a1], query: ""))
        typealias Place = StockTrayRules.Place
        XCTAssertEqual(places, [Place(stock: "b1", favourite: true), Place(stock: "a1", favourite: true),
                                Place(stock: "a1", favourite: false), Place(stock: "a2", favourite: false), Place(stock: "b1", favourite: false)])
        func step(_ from: String, inFavourites: Bool, _ by: Int) -> Place? { StockTrayRules.step(from: from, inFavourites: inFavourites, by: by, in: places) }
        XCTAssertEqual(step("b1", inFavourites: true, 1), Place(stock: "a1", favourite: true), "From the favourites' copy, the next favourite")
        XCTAssertEqual(step("a1", inFavourites: true, 1), Place(stock: "a2", favourite: false),
                       "On into the groups, past the last favourite's own copy next to it")
        XCTAssertEqual(step("a2", inFavourites: false, -1), Place(stock: "a1", favourite: false))
        XCTAssertEqual(step("b1", inFavourites: false, -1), Place(stock: "a2", favourite: false), "From a group's copy, its neighbour")
        XCTAssertEqual(step("b1", inFavourites: true, -1), Place(stock: "b1", favourite: true), "Held at the first favourite")
        XCTAssertEqual(step("a2", inFavourites: true, -1), Place(stock: "a1", favourite: false), "Not a favourite: its group's copy")
    }

    func testRowGeometry() {
        let sections = [StockTrayRules.Section(title: "Favourites", stocks: [stock("b", "B")], favorites: true),
                        StockTrayRules.Section(title: "A", stocks: [stock("a", "A"), stock("b", "B")], favorites: false)]
        // 8 inset; a section is its title (18), 9, then 212-wide tiles 8 apart; sections 20 apart.
        XCTAssertEqual(StockTrayRules.starts(sections), [8, 8 + 18 + 9 + 212 + 20])
        let first: CGFloat = 18 + 9 + 212, second: CGFloat = 18 + 9 + 2 * 212 + 8
        XCTAssertEqual(StockTrayRules.contentWidth(sections), 8 + first + 20 + second + 8)
        XCTAssertEqual(StockTrayRules.tileX("b", in: 1, of: sections), 267 + 27 + 220)
        XCTAssertNil(StockTrayRules.tileX("a", in: 0, of: sections))
        XCTAssertEqual(StockTrayRules.section(of: "b", in: sections), 0, "Opens on a favourite's copy among the favourites")
        XCTAssertEqual(StockTrayRules.section(of: "b", in: sections, favoritesFirst: false), 1, "A step shows it in its group")
        XCTAssertEqual(StockTrayRules.current(scrolledTo: 0, starts: [8, 267]), 0)
        XCTAssertEqual(StockTrayRules.current(scrolledTo: 230, starts: [8, 267]), 1, "Within 40 pt of the left edge counts as in view")
        XCTAssertEqual(StockTrayRules.centring(700, visible: 600, content: 1000), 400, "Clamped to the row's end")
        XCTAssertEqual(StockTrayRules.centring(300, visible: 600, content: 2000), 300 - 194)
        XCTAssertEqual(StockTrayRules.jump(to: 8, visible: 600, content: 2000), 0)
        XCTAssertEqual(StockTrayRules.jump(to: 500, visible: 600, content: 2000), 470)
    }

    func testFootWords() {
        XCTAssertEqual(StockTrayRules.hint(before: nil), "click to try · ← → to step")
        XCTAssertEqual(StockTrayRules.hint(before: "Kodak Gold 200"), "return keeps · esc goes back to Kodak Gold 200")
        XCTAssertEqual(StockTrayRules.matches(1), "1 match · return tries the first")
        XCTAssertEqual(StockTrayRules.matches(3), "3 matches · return tries the first")
        XCTAssertEqual(StockTrayRules.matches(0), "")
    }

    // MARK: The model, with the engine

    private func paths() -> EnginePaths {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return EnginePaths(helper: root.appendingPathComponent("target/release/spektrafilm-native"), exporter: root.appendingPathComponent("target/release/spektrafilm-f64"), data: root.appendingPathComponent("data"))
    }

    /// A model with a small photograph developed, and an undo manager that
    /// groups only when told.
    private func opened() async throws -> (EditorModel, UndoManager, URL) {
        guard FileManager.default.isExecutableFile(atPath: paths().helper.path) else { throw XCTSkip("Build the release helper first") }
        let model = EditorModel()
        await model.start(paths: paths())
        XCTAssertTrue(model.ready, model.error ?? "Engine did not start")
        let fixture = try makeFixture()
        model.open(fixture)
        try await waitUntil { model.previewCurrent || model.error != nil }
        XCTAssertNil(model.error)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        return (model, undo, fixture)
    }

    /// Only around what should register a step: an ungrouped registration
    /// throws, so a try outside one proves it registers nothing.
    private func grouped(_ undo: UndoManager, _ body: () -> Void) {
        undo.beginUndoGrouping(); body(); undo.endUndoGrouping()
    }

    private func others(_ model: EditorModel, count: Int) -> [String] {
        // Negatives, so the paper stays printed.
        let negatives = model.films.filter { !$0.positive && $0.id != model.film }
        return Array(negatives.map(\.id).prefix(count))
    }

    func testAVisitIsOneUndoStepAndTheTrayStaysOpen() async throws {
        let (model, undo, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let before = model.film, tries = others(model, count: 2)
        XCTAssertFalse(undo.canUndo)

        model.openTray(paper: false)
        XCTAssertEqual(model.tray?.paper, false)
        XCTAssertTrue(model.previews.filmOpen, "The film tiles develop while the tray is open")
        model.tryStock(tries[0])
        model.tryStock(tries[1])
        XCTAssertEqual(model.film, tries[1])
        XCTAssertNotNil(model.tray, "Trying a stock leaves the tray open")
        grouped(undo) { model.endTray(keep: true) }
        XCTAssertNil(model.tray)
        XCTAssertFalse(model.previews.filmOpen)
        XCTAssertEqual(model.film, tries[1], "Return keeps the stock being tried")
        XCTAssertEqual(undo.undoActionName, "Change Film")

        undo.undo()
        XCTAssertEqual(model.film, before, "One undo goes back past every try to the stock before the tray opened")
        XCTAssertFalse(undo.canUndo, "The tries in between aren't steps")
        undo.redo()
        XCTAssertEqual(model.film, tries[1])
    }

    func testEscGoesBackWithoutAnUndoStep() async throws {
        let (model, undo, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let before = model.state, tries = others(model, count: 2)
        model.openTray(paper: false)
        model.tryStock(tries[0])
        model.tryStock(tries[1])
        model.endTray(keep: false)
        XCTAssertNil(model.tray)
        XCTAssertEqual(model.film, before.film)
        XCTAssertEqual(model.paper, before.paper)
        XCTAssertEqual(model.state.params, before.params, "Esc puts back the whole edit from before, the film's presets too")
        XCTAssertFalse(undo.canUndo, "Going back leaves nothing to undo")

        // Keeping the stock it opened on isn't a step either.
        model.openTray(paper: false)
        model.tryStock(tries[0])
        model.tryStock(before.film)
        XCTAssertEqual(model.state, before)
        model.endTray(keep: true)
        XCTAssertFalse(undo.canUndo)
    }

    func testTriesLeaveEarlierStepsAlone() async throws {
        let (model, undo, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let tries = others(model, count: 2)
        grouped(undo) { model.rotate() }
        model.openTray(paper: false)
        model.tryStock(tries[0])
        model.tryStock(tries[1])
        XCTAssertEqual(undo.undoActionName, "Change Adjustment", "While trying, the last step is still the one before the tray")
        // What ⌘Z or a menu does while the tray is open: keep, then undo the visit.
        grouped(undo) { model.endTray(keep: true) }
        undo.undo()
        XCTAssertEqual(model.value("io.rotate_cw"), .number(1), "Only the visit is undone")
        undo.undo()
        XCTAssertEqual(model.value("io.rotate_cw"), .number(0))
    }

    func testArrowsStepStocksAndAnotherEditKeepsTheTry() async throws {
        let (model, undo, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let before = model.film
        let places = StockTrayRules.places(StockTrayRules.sections(model.filmGroups, favorites: model.previews.favorites(in: model.films, paper: false), query: ""))
        // It opens on a favourite's copy among the favourites, and steps on from there.
        let next = try XCTUnwrap(StockTrayRules.step(from: before, inFavourites: true, by: 1, in: places)).stock
        XCTAssertNotEqual(next, before)
        model.openTray(paper: false)
        model.stepTray(by: 1)
        XCTAssertEqual(model.film, next)
        model.stepTray(by: 1)
        model.stepTray(by: -1)
        XCTAssertEqual(model.film, next)

        // Searching narrows what the arrows step through.
        model.tray?.query = "velvia"
        let velvias = StockTrayRules.places(StockTrayRules.sections(model.filmGroups, favorites: [], query: "velvia")).map(\.stock)
        model.stepTray(by: 1)
        XCTAssertEqual(model.film, velvias.first, "From a stock not among the matches, the first match")
        model.tray?.query = ""

        // The paper's name opens its tray and keeps the film being tried.
        model.tryStock(next)
        grouped(undo) { model.openTray(paper: true) }
        XCTAssertEqual(model.tray?.paper, true)
        XCTAssertEqual(model.film, next)
        // Any other edit keeps it too, and closes the tray.
        grouped(undo) { model.rotate() }
        XCTAssertNil(model.tray)
        undo.undo()
        XCTAssertEqual(model.film, next, "The rotation was its own step")
        undo.undo()
        XCTAssertEqual(model.film, before)
    }

    func testCroppingKeepsTheTryAndClosesTheTray() async throws {
        let (model, undo, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let tries = others(model, count: 1)
        model.openTray(paper: false)
        model.tryStock(tries[0])
        try await waitUntil { model.previewCurrent }
        grouped(undo) { model.beginCrop() }
        XCTAssertNil(model.tray, "The straighten dial takes the label's place")
        XCTAssertEqual(model.film, tries[0])
        model.endCrop(apply: false)
        model.openTray(paper: false)
        XCTAssertNotNil(model.tray)
        // Settings › previews off: the plain menu again.
        model.setPreviews(false)
        XCTAssertNil(model.tray)
        model.openTray(paper: false)
        XCTAssertNil(model.tray, "No tray without the previews")
        model.setPreviews(true)
    }

    func testTheTriedStocksTileShowsAtOnce() async throws {
        let (model, undo, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let target = try XCTUnwrap(others(model, count: 1).first)
        model.openTray(paper: false)
        // What the tray's view does when its tiles are on screen.
        model.previews.filmShown = true
        model.pumpPreviews()
        let tile = try XCTUnwrap(model.previews.tile(target, paper: false))
        try await waitUntil(seconds: 60) { tile.image != nil && !tile.dimmed }
        model.tryStock(target)
        XCTAssertNotNil(model.trialImage, "The tile stands in, enlarged, until the develop is in")
        XCTAssertTrue(model.trialImage === tile.image)
        try await waitUntil { model.previewCurrent }
        XCTAssertNil(model.trialImage, "The develop replaces it")

        // Esc goes back: the tile was of the tried stock, so it goes.
        model.tryStock(model.films.first { $0.id != target && $0.id != model.film }!.id)
        model.endTray(keep: false)
        if !model.previewCurrent { XCTAssertNil(model.trialImage) }
        model.previews.filmShown = false
    }

    /// The paper's name clicked while the film tray is open: the tray in
    /// the window becomes the paper tray, tiles and all. It used to keep the
    /// film tiles under the papers' foot, so a click on one printed the
    /// photograph on a film ("printed on kodak_gold_200").
    func testTheOtherStocksNameShowsItsOwnTiles() async throws {
        let (model, _, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        _ = NSApplication.shared
        // Where the window puts the tray (NegsApp's body).
        struct Slot: View {
            let model: EditorModel
            var body: some View {
                if let tray = model.tray { StockTray(model: model, visit: tray) } else { Color.clear }
            }
        }
        let host = NSHostingView(rootView: Slot(model: model).frame(width: 900, height: Theme.Size.tray))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: Theme.Size.tray), styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        defer { window.close() }

        func tiles() -> [NSHostingView<TrayTiles>] {
            func walk(_ view: NSView) -> [NSHostingView<TrayTiles>] {
                (view as? NSHostingView<TrayTiles>).map { [$0] } ?? view.subviews.flatMap(walk)
            }
            return walk(host)
        }
        func settle() async throws {
            for _ in 0..<5 { host.layoutSubtreeIfNeeded(); try await Task.sleep(nanoseconds: 20_000_000) }
        }

        model.openTray(paper: false)
        try await settle()
        let films = try XCTUnwrap(tiles().first, "The film tray's tiles are in the window")
        XCTAssertFalse(films.rootView.visit.paper)
        XCTAssertTrue(model.previews.filmShown)

        model.openTray(paper: true)
        try await settle()
        let visit = try XCTUnwrap(model.tray)
        XCTAssertTrue(visit.paper)
        let shown = tiles()
        XCTAssertEqual(shown.count, 1)
        let papers = try XCTUnwrap(shown.first)
        XCTAssertTrue(papers.rootView.visit === visit, "The tiles are the paper visit's, not the film visit's")
        let ids = Set(StockTray.sections(model: model, visit: papers.rootView.visit).flatMap(\.stocks).map(\.id))
        XCTAssertTrue(ids.isSubset(of: Set(model.papers.map(\.id))), "Only papers under the papers' foot")
        XCTAssertTrue(model.previews.paperShown, "The paper tiles develop")
        XCTAssertFalse(model.previews.filmShown, "The film tiles stop developing")
    }

    /// A film's ID can't be tried as the paper, or a paper's as the film,
    /// whatever asks for it.
    func testATrayTriesOnlyItsOwnKind() async throws {
        let (model, _, fixture) = try await opened()
        defer { model.shutdown(); try? FileManager.default.removeItem(at: fixture) }
        let before = model.state
        let film = try XCTUnwrap(others(model, count: 1).first)
        let paper = try XCTUnwrap(model.papers.first { $0.id != model.paper }).id

        model.openTray(paper: true)
        model.tryStock(film)
        XCTAssertEqual(model.state, before, "A film isn't a paper")
        model.tryStock(paper)
        XCTAssertEqual(model.paper, paper)
        model.endTray(keep: false)

        model.openTray(paper: false)
        model.tryStock(paper)
        XCTAssertEqual(model.state, before, "A paper isn't a film")
        model.endTray(keep: false)
    }

    private func waitUntil(seconds: TimeInterval = 20, _ predicate: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while !predicate(), Date() < deadline { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertTrue(predicate(), "Timed out waiting for image engine")
    }

    private func makeFixture() throws -> URL {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 64, bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 96 * 3, bitsPerPixel: 24)!
        for y in 0..<64 { for x in 0..<96 { let p = bitmap.bitmapData! + (y * 96 + x) * 3; p[0] = UInt8(x * 2); p[1] = UInt8(y * 3); p[2] = 120 } }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("negs-tray-\(UUID().uuidString).png")
        try bitmap.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }
}
