import AppKit
import Observation
import SwiftUI

/// One visit to the film or paper tray: the edit when it opened, so Esc can
/// go back to it and keeping a stock is one undo step from it
/// (IMPLEMENTATION.md §5.1).
@MainActor @Observable final class TrayVisit {
    let paper: Bool
    /// The edit when the tray opened.
    let before: EditState
    /// Films only, as today.
    var query = ""
    /// The stock being tried is its copy among the favourites (where the
    /// tray opens on a favourite, or the copy last clicked or stepped to),
    /// so ← → go on from there rather than from its group.
    @ObservationIgnored var inFavourites = true

    init(paper: Bool, before: EditState) {
        self.paper = paper
        self.before = before
    }

    /// The app's undo names for a stock change.
    var action: String { paper ? "Change Paper" : "Change Film" }
    /// The stock the tray opened on.
    var beforeID: String { paper ? before.paper : before.film }
}

/// The tray's order, sizes and words, kept apart from the views so they can
/// be tested.
@MainActor enum StockTrayRules {
    static let tile: CGFloat = 212, picture: CGFloat = 118
    static let tileGap: CGFloat = 8, groupGap: CGFloat = 20
    /// The group's name, written up its left side, and the gap after it.
    static let title: CGFloat = 18, titleGap: CGFloat = 9
    /// Room round the tiles for the pencil ring.
    static let inset: CGFloat = 8
    static let favouritesTitle = "Favourites"
    /// The star's tooltip, and what VoiceOver hears, in the heading's spelling.
    static func starHelp(favourite: Bool) -> String { favourite ? "Remove from favourites" : "Add to favourites" }
    static func starLabel(_ name: String, favourite: Bool) -> String {
        favourite ? "Remove \(name) from favourites" : "Add \(name) to favourites"
    }

    struct Section: Identifiable {
        let title: String
        let stocks: [Stock]
        let favorites: Bool
        var id: String { title }
    }

    /// The groups as shown: the favourites first under their own title
    /// (each also stays in its group), hidden while searching, as today.
    static func sections(_ groups: [StockGroup], favorites: [Stock], query: String) -> [Section] {
        let searching = !query.trimmingCharacters(in: .whitespaces).isEmpty
        let matching = StockGroup.matching(query, in: groups).map { Section(title: $0.title, stocks: $0.stocks, favorites: false) }
        guard !searching, !favorites.isEmpty else { return matching }
        return [Section(title: favouritesTitle, stocks: favorites, favorites: true)] + matching
    }

    /// A tile in the row: its stock, and whether it's the favourites' copy.
    struct Place: Equatable {
        let stock: String
        let favourite: Bool
    }

    /// The order ← → step through: the tiles as shown, the favourites first.
    static func places(_ sections: [Section]) -> [Place] {
        sections.flatMap { section in section.stocks.map { Place(stock: $0.id, favourite: section.favorites) } }
    }

    /// The tile `offset` along from `current` (its favourites' copy if
    /// `inFavourites` and it has one), held at either end; from a stock not
    /// in the row, the first. A copy of the same stock next to it, the last
    /// favourite's own in the first group, is passed over, so every step
    /// tries another stock.
    static func step(from current: String, inFavourites: Bool, by offset: Int, in places: [Place]) -> Place? {
        guard !places.isEmpty else { return nil }
        guard var index = places.firstIndex(of: Place(stock: current, favourite: inFavourites)) ?? places.firstIndex(where: { $0.stock == current })
        else { return places.first }
        let direction = offset < 0 ? -1 : 1
        for _ in 0..<abs(offset) {
            var next = index + direction
            while places.indices.contains(next), places[next].stock == places[index].stock { next += direction }
            guard places.indices.contains(next) else { break }
            index = next
        }
        return places[index]
    }

    static func width(of section: Section) -> CGFloat {
        let count = CGFloat(section.stocks.count)
        let tiles: CGFloat = count * tile + max(count - 1, 0) * tileGap
        return title + titleGap + tiles
    }
    /// Where each section starts in the row.
    static func starts(_ sections: [Section]) -> [CGFloat] {
        var x = inset, starts: [CGFloat] = []
        for section in sections {
            starts.append(x)
            x += width(of: section) + groupGap
        }
        return starts
    }
    /// The row's whole width.
    static func contentWidth(_ sections: [Section]) -> CGFloat {
        sections.reduce(2 * inset) { $0 + width(of: $1) } + CGFloat(max(sections.count - 1, 0)) * groupGap
    }
    /// The left edge of `stock`'s tile in `section`.
    static func tileX(_ stock: String, in section: Int, of sections: [Section]) -> CGFloat? {
        guard sections.indices.contains(section), let index = sections[section].stocks.firstIndex(where: { $0.id == stock }) else { return nil }
        return starts(sections)[section] + title + titleGap + CGFloat(index) * (tile + tileGap)
    }
    /// The section a stock's tile is shown in: with `favoritesFirst` (as the
    /// tray opens, or once stepped or clicked to among the favourites) the
    /// favourites' copy of a favourite, else its group.
    static func section(of stock: String, in sections: [Section], favoritesFirst: Bool = true) -> Int? {
        let has = { (section: Section) in section.stocks.contains { $0.id == stock } }
        if favoritesFirst, let index = sections.firstIndex(where: { $0.favorites && has($0) }) { return index }
        return sections.firstIndex { !$0.favorites && has($0) }
    }
    /// The group in view, underlined in the foot: the last one starting
    /// within 40 pt of the row's left edge.
    static func current(scrolledTo x: CGFloat, starts: [CGFloat]) -> Int {
        starts.lastIndex { $0 <= x + 40 } ?? 0
    }
    /// The scroll offset that centres a tile at `tileX` in a row `visible` wide.
    static func centring(_ tileX: CGFloat, visible: CGFloat, content: CGFloat) -> CGFloat {
        clamp(tileX - (visible - tile) / 2, visible: visible, content: content)
    }
    /// The scroll offset that puts a group 30 pt in from the left, as the foot's index jumps.
    static func jump(to start: CGFloat, visible: CGFloat, content: CGFloat) -> CGFloat {
        clamp(start - 30, visible: visible, content: content)
    }
    static func clamp(_ x: CGFloat, visible: CGFloat, content: CGFloat) -> CGFloat {
        min(max(x, 0), max(content - visible, 0))
    }

    /// The foot's hint: how to try before anything is tried, how to keep or go back after.
    static func hint(before: String?) -> String {
        guard let before else { return "click to try · ← → to step" }
        return "return keeps · esc goes back to \(before)"
    }
    /// While searching the index gives way to the count.
    static func matches(_ count: Int) -> String {
        count == 0 ? "" : "\(count) \(count == 1 ? "match" : "matches") · return tries the first"
    }
    static func noMatch(_ query: String, paper: Bool) -> String {
        "No \(paper ? "papers" : "films") match “\(query.trimmingCharacters(in: .whitespaces))”"
    }
}

/// Where the strip and the status line were: the stocks as tiles of the
/// photograph developed with each, and a foot with the group index, the hint,
/// Search and Done. A click tries a stock and the tray stays open (§4.5).
struct StockTray: View {
    let model: EditorModel
    let visit: TrayVisit
    @State private var scroll = TrayScroll()

    var body: some View {
        let sections = Self.sections(model: model, visit: visit)
        VStack(alignment: .leading, spacing: 0) {
            ZStack(alignment: .topLeading) {
                TrayRow(model: model, visit: visit, sections: sections, scroll: scroll)
                if sections.isEmpty {
                    Text(StockTrayRules.noMatch(visit.query, paper: visit.paper))
                        .font(Theme.serif(16, italic: true)).foregroundStyle(Theme.dim)
                        .padding(.top, 46).padding(.leading, 30)
                }
            }
            .frame(height: Theme.Size.strip)
            TrayFoot(model: model, visit: visit, sections: sections, scroll: scroll)
        }
        .frame(height: Theme.Size.tray)
        .frame(maxWidth: .infinity)
        .background(Theme.film)
        .background(TrayHitArea(model: model))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(visit.paper ? "Papers" : "Films")
    }

    static func sections(model: EditorModel, visit: TrayVisit) -> [StockTrayRules.Section] {
        let paper = visit.paper
        return StockTrayRules.sections(paper ? model.paperGroups : model.filmGroups,
                                       favorites: model.previews.favorites(in: paper ? model.papers : model.films, paper: paper),
                                       query: visit.query)
    }
}

/// The row's scroll view and the group in view, shared by the row and the foot.
@MainActor @Observable final class TrayScroll {
    var group = 0
    @ObservationIgnored weak var view: NSScrollView?

    func scroll(to x: CGFloat, animated: Bool) {
        guard let view else { return }
        let clip = view.contentView
        let origin = NSPoint(x: x, y: clip.bounds.origin.y)
        if animated, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.25
                context.allowsImplicitAnimation = true
                clip.animator().setBoundsOrigin(origin)
            }
        } else {
            clip.setBoundsOrigin(origin)
        }
        view.reflectScrolledClipView(clip)
    }
    var offset: CGFloat { view?.contentView.bounds.origin.x ?? 0 }
    var visible: CGFloat { view?.contentView.bounds.width ?? 0 }
}

/// The tiles in an AppKit scroll view, so a vertical wheel scrolls them
/// sideways and the group in view can be followed.
private struct TrayRow: NSViewRepresentable {
    let model: EditorModel
    let visit: TrayVisit
    let sections: [StockTrayRules.Section]
    let scroll: TrayScroll

    func makeNSView(context: Context) -> TrayScrollView {
        let view = TrayScrollView()
        view.drawsBackground = false
        view.hasHorizontalScroller = false
        view.hasVerticalScroller = false
        view.verticalScrollElasticity = .none
        view.horizontalScrollElasticity = .allowed
        let tiles = NSHostingView(rootView: TrayTiles(model: model, visit: visit))
        tiles.translatesAutoresizingMaskIntoConstraints = false
        view.documentView = tiles
        let clip = view.contentView
        NSLayoutConstraint.activate([
            tiles.leadingAnchor.constraint(equalTo: clip.leadingAnchor),
            tiles.topAnchor.constraint(equalTo: clip.topAnchor),
            tiles.heightAnchor.constraint(equalTo: clip.heightAnchor),
        ])
        clip.postsBoundsChangedNotifications = true
        context.coordinator.observe(clip)
        scroll.view = view
        context.coordinator.update(self)
        // It opens on the current stock.
        DispatchQueue.main.async { context.coordinator.reveal(centring: true, animated: false) }
        return view
    }

    func updateNSView(_ view: TrayScrollView, context: Context) {
        scroll.view = view
        context.coordinator.update(self)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    @MainActor final class Coordinator: NSObject {
        private var row: TrayRow?
        private var stock: String?
        private var query: String?
        private var observer: NSObjectProtocol?

        func observe(_ clip: NSClipView) {
            observer = NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: clip, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.follow() }
            }
        }
        deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }

        func update(_ row: TrayRow) {
            self.row = row
            let current = row.visit.paper ? row.model.paper : row.model.film
            defer { stock = current; query = row.visit.query; follow() }
            guard let stock, let query else { return }
            if query != row.visit.query {
                // Each search starts at the first match; clearing it goes back to the stock.
                if row.visit.query.isEmpty { DispatchQueue.main.async { self.reveal(centring: true, animated: false) } }
                else { row.scroll.scroll(to: 0, animated: false) }
            } else if stock != current {
                // Stepped to (or tried from the search's first match): shown if out of view.
                DispatchQueue.main.async { self.reveal(centring: false, animated: true) }
            }
        }

        /// Shows the current stock's tile, the copy the arrows go on from:
        /// centred, or only if it's out of view.
        func reveal(centring: Bool, animated: Bool) {
            guard let row else { return }
            let sections = row.sections, scroll = row.scroll
            let current = row.visit.paper ? row.model.paper : row.model.film
            let content = StockTrayRules.contentWidth(sections), visible = scroll.visible
            guard let section = StockTrayRules.section(of: current, in: sections, favoritesFirst: row.visit.inFavourites),
                  let x = StockTrayRules.tileX(current, in: section, of: sections) else { return }
            if !centring, x >= scroll.offset, x + StockTrayRules.tile <= scroll.offset + visible { return }
            scroll.scroll(to: StockTrayRules.centring(x, visible: visible, content: content), animated: animated)
        }

        private func follow() {
            guard let row else { return }
            let group = StockTrayRules.current(scrolledTo: row.scroll.offset, starts: StockTrayRules.starts(row.sections))
            if row.scroll.group != group { row.scroll.group = group }
        }
    }
}

/// A vertical wheel scrolls the row sideways; a trackpad's sideways swipe
/// scrolls it as usual.
final class TrayScrollView: NSScrollView {
    override func scrollWheel(with event: NSEvent) {
        guard abs(event.scrollingDeltaY) > abs(event.scrollingDeltaX), let document = documentView else {
            return super.scrollWheel(with: event)
        }
        let step = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 12
        let clip = contentView
        let x = min(max(clip.bounds.origin.x - step, 0), max(document.frame.width - clip.bounds.width, 0))
        clip.setBoundsOrigin(NSPoint(x: x, y: clip.bounds.origin.y))
        reflectScrolledClipView(clip)
    }
}

/// The groups of tiles, each with its name written up its left side.
private struct TrayTiles: View {
    let model: EditorModel
    let visit: TrayVisit

    var body: some View {
        let paper = visit.paper
        let previews = model.previews
        let current = paper ? model.paper : model.film
        let tried = current != visit.beforeID
        HStack(alignment: .top, spacing: StockTrayRules.groupGap) {
            ForEach(StockTray.sections(model: model, visit: visit)) { section in
                HStack(alignment: .top, spacing: StockTrayRules.titleGap) {
                    Text(section.title)
                        .font(Theme.serif(13.5, italic: true)).foregroundStyle(Theme.label)
                        .lineLimit(1).fixedSize()
                        .rotationEffect(.degrees(-90))
                        .frame(width: StockTrayRules.title, height: StockTrayRules.picture)
                        .accessibilityAddTraits(.isHeader)
                    HStack(alignment: .top, spacing: StockTrayRules.tileGap) {
                        ForEach(section.stocks) { stock in
                            if let tile = previews.tile(stock.id, paper: paper) {
                                TrayTile(stock: stock, tile: tile, selected: stock.id == current, before: tried && stock.id == visit.beforeID,
                                         favorite: previews.isFavorite(stock.id, paper: paper),
                                         toggleFavorite: { previews.toggleFavorite(stock.id, paper: paper) },
                                         choose: { visit.inFavourites = section.favorites; model.tryStock(stock.id) })
                            }
                        }
                    }
                }
                .accessibilityElement(children: .contain)
                .accessibilityLabel(section.title)
            }
        }
        .padding(.horizontal, StockTrayRules.inset)
        .padding(.top, 9)
        .frame(maxHeight: .infinity, alignment: .top)
        .onAppear { setShown(true) }
        .onDisappear { setShown(false) }
    }

    private func setShown(_ shown: Bool) {
        if visit.paper { model.previews.paperShown = shown } else { model.previews.filmShown = shown }
        model.pumpPreviews()
    }
}

/// A stock's tile: the photograph developed with it, and its name. The one
/// being tried is ringed in pencil and its name is in `paper` 600; the one
/// the tray opened on says "before" once another is tried.
private struct TrayTile: View {
    let stock: Stock
    let tile: PreviewTile
    let selected: Bool
    let before: Bool
    let favorite: Bool
    let toggleFavorite: () -> Void
    let choose: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: choose) {
            VStack(alignment: .leading, spacing: 4) {
                // Not developed yet: a blank; an older edit of this photograph: dimmed.
                Rectangle().fill(Theme.blank)
                    .frame(width: StockTrayRules.tile, height: StockTrayRules.picture)
                    .overlay {
                        if let image = tile.image {
                            Image(decorative: image, scale: 2).resizable().aspectRatio(contentMode: .fill)
                                .frame(width: StockTrayRules.tile, height: StockTrayRules.picture)
                                .opacity(tile.dimmed ? 0.55 : 1)
                        }
                    }
                    .clipped()
                    .overlay {
                        if selected {
                            PencilRing().stroke(Theme.accent, style: StrokeStyle(lineWidth: PencilRing.lineWidth, lineCap: .round))
                                .padding(EdgeInsets(top: -6, leading: -7, bottom: -1, trailing: -7))
                                .rotationEffect(.degrees(-1))
                                .allowsHitTesting(false)
                        }
                    }
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(stock.name)
                        .font(Theme.serif(14, weight: selected ? 600 : 400))
                        .foregroundStyle(selected ? Theme.paper : before || hovering ? Theme.label : Theme.dim)
                        .lineLimit(1).truncationMode(.tail)
                    if before {
                        Text("before").font(Theme.serif(13, italic: true)).foregroundStyle(Theme.dim).fixedSize()
                    }
                }
                .frame(width: StockTrayRules.tile, alignment: .leading)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusable(false)
        .help(stock.name)
        .accessibilityLabel(stock.name)
        .accessibilityValue(selected ? (before ? "" : "Trying") : before ? "Before" : "")
        .accessibilityAddTraits(selected ? .isSelected : [])
        // Over the picture's corner, outside the tile's own button so a
        // click stars rather than tries. Shown on a favourite, or while the
        // pointer is over the tile.
        .overlay(alignment: .topTrailing) {
            if favorite || hovering {
                Button(action: toggleFavorite) {
                    StarShape()
                        .fill(favorite ? Theme.accent : .clear)
                        .overlay {
                            StarShape().stroke(favorite ? Theme.accent : Theme.paper,
                                               style: StrokeStyle(lineWidth: favorite ? 1.2 : 1.4, lineJoin: .round))
                        }
                        .frame(width: 15, height: 15)
                        .shadow(color: .black.opacity(0.8), radius: 1.5)
                        .frame(width: 20, height: 20)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .focusable(false)
                .padding(5)
                .help(StockTrayRules.starHelp(favourite: favorite))
                .accessibilityLabel(StockTrayRules.starLabel(stock.name, favourite: favorite))
            }
        }
        .onHover { hovering = $0 }
    }
}

/// The favourite's star, from the mockup's 20 × 20 path.
struct StarShape: Shape {
    func path(in rect: CGRect) -> Path {
        let points: [(CGFloat, CGFloat)] = [(10, 2.6), (12.2, 7.5), (17.4, 8), (13.5, 11.5), (14.6, 16.7),
                                            (10, 14), (5.4, 16.7), (6.5, 11.5), (2.6, 8), (7.8, 7.5)]
        // The path's points span 2.6…17.4 of its 20 pt box; drawn to fill `rect`.
        let scale = min(rect.width, rect.height) / 14.8
        var path = Path()
        for (index, point) in points.enumerated() {
            let p = CGPoint(x: rect.minX + (point.0 - 2.6) * scale, y: rect.minY + (point.1 - 2.6) * scale)
            if index == 0 { path.move(to: p) } else { path.addLine(to: p) }
        }
        path.closeSubpath()
        return path
    }
}

/// The tray's foot: the group index (the count while searching), the hint,
/// Search (films only) and Done.
private struct TrayFoot: View {
    let model: EditorModel
    let visit: TrayVisit
    let sections: [StockTrayRules.Section]
    let scroll: TrayScroll
    @State private var searching = false

    var body: some View {
        let current = visit.paper ? model.paper : model.film
        let before = current == visit.beforeID ? nil : (visit.paper ? model.papers : model.films).first { $0.id == visit.beforeID }?.name
        HStack(alignment: .center, spacing: 12) {
            if visit.query.trimmingCharacters(in: .whitespaces).isEmpty {
                index
            } else {
                Text(StockTrayRules.matches(sections.reduce(0) { $0 + $1.stocks.count }))
                    .font(Theme.serif(13, italic: true)).foregroundStyle(Theme.label).lineLimit(1)
            }
            Spacer(minLength: 12)
            Text(StockTrayRules.hint(before: before))
                .font(Theme.serif(13, italic: true)).foregroundStyle(Theme.dim).lineLimit(1)
                .truncationMode(.middle)
                .layoutPriority(-1)
            if !visit.paper {
                TraySearchField(visit: visit, editing: $searching, submit: submit, cancel: cancel)
                    .frame(width: 130, height: 18)
                    .padding(.bottom, 1)
                    .overlay(alignment: .bottom) {
                        if searching {
                            PencilStroke().pencil().frame(height: 5).offset(y: 3).allowsHitTesting(false)
                        } else {
                            Rectangle().fill(Theme.line).frame(height: 1)
                        }
                    }
                    .padding(.leading, 6)
            }
            Button { model.endTray(keep: true) } label: {
                Text("Done").font(Theme.serif(14, weight: 600)).foregroundStyle(Theme.paper)
            }
            .buttonStyle(.plain)
            .focusable(false)
            .padding(.leading, 10)
            .help("Keep this \(visit.paper ? "paper" : "film") (Return)")
        }
        .padding(.leading, 18).padding(.trailing, 18).padding(.bottom, 2)
        .frame(height: Theme.Size.statusLine)
    }

    /// The groups as words; the one in view underlined in pencil. A click jumps to it.
    private var index: some View {
        HStack(alignment: .firstTextBaseline, spacing: 14) {
            ForEach(Array(sections.enumerated()), id: \.offset) { offset, section in
                IndexWord(title: section.title, selected: offset == scroll.group) {
                    let starts = StockTrayRules.starts(sections)
                    scroll.scroll(to: StockTrayRules.jump(to: starts[offset], visible: scroll.visible,
                                                          content: StockTrayRules.contentWidth(sections)), animated: true)
                }
            }
        }
        .fixedSize()
    }

    /// Return in Search tries the first match; with nothing typed it keeps.
    private func submit() {
        guard !visit.query.trimmingCharacters(in: .whitespaces).isEmpty else { return model.endTray(keep: true) }
        if let first = StockGroup.matching(visit.query, in: model.filmGroups).first?.stocks.first { model.tryStock(first.id) }
    }
    /// Esc in Search clears it; in an empty Search it goes back, as anywhere in the tray.
    private func cancel() {
        if visit.query.isEmpty { model.endTray(keep: false) } else { visit.query = "" }
    }
}

private struct IndexWord: View {
    let title: String
    let selected: Bool
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(Theme.serif(13, italic: true))
                .foregroundStyle(selected ? Theme.paper : hovering ? Theme.label : Theme.dim)
                .padding(.bottom, 3)
                .overlay(alignment: .bottom) {
                    if selected { PencilStroke().pencil().frame(height: 5).allowsHitTesting(false) }
                }
        }
        .buttonStyle(.plain)
        .focusable(false)
        .onHover { hovering = $0 }
        .accessibilityLabel("Show \(title)")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// Search, as a word on a rule: no bezel, no focus ring, a `paper` caret.
private struct TraySearchField: NSViewRepresentable {
    let visit: TrayVisit
    @Binding var editing: Bool
    let submit: () -> Void
    let cancel: () -> Void

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.isBordered = false
        field.isBezeled = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.cell?.isScrollable = true
        field.cell?.wraps = false
        let font = Theme.serifFont(13.5, italic: true) as NSFont
        field.font = font
        field.textColor = Theme.NS.paper
        field.placeholderAttributedString = NSAttributedString(string: "Search films", attributes: [.font: font, .foregroundColor: Theme.NS.dim])
        field.setAccessibilityLabel("Search films")
        field.delegate = context.coordinator
        field.stringValue = visit.query
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != visit.query { field.stringValue = visit.query }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    @MainActor final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: TraySearchField
        init(_ parent: TraySearchField) { self.parent = parent }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            if parent.visit.query != field.stringValue { parent.visit.query = field.stringValue }
        }
        func controlTextDidBeginEditing(_ notification: Notification) {
            if let editor = (notification.object as? NSTextField)?.currentEditor() as? NSTextView { InkTextField.style(editor) }
            parent.editing = true
        }
        func controlTextDidEndEditing(_ notification: Notification) { parent.editing = false }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.insertNewline(_:)): parent.submit(); return true
            case #selector(NSResponder.cancelOperation(_:)): parent.cancel(); return true
            default: return false
            }
        }
    }
}

/// Marks a view as part of the tray, or a name that opens it: a click
/// inside doesn't close the tray (`EditorModel.trayHitAreas`).
struct TrayHitArea: NSViewRepresentable {
    let model: EditorModel
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        model.trayHitAreas.add(view)
        return view
    }
    func updateNSView(_ view: NSView, context: Context) { model.trayHitAreas.add(view) }
}
