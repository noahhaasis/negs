import AppKit
import SwiftUI

/// The grid (§4.6): a black sheet over the whole window below the title bar,
/// with the inspector under it. A sentence at the top says what's shown and
/// holds the filter; the photographs are packed in rows at their own
/// proportions, without captions; the foot counts them and sizes the rows.
/// Like the strip it shows embedded previews or developed thumbnails, so it
/// never develops a RAW. A click opens a photograph and folds the grid;
/// ⌘- and ⇧-clicks, the arrow keys and the rubber band select without
/// opening, for the context menu.
struct GridSheet: View {
    let model: EditorModel

    var body: some View {
        // Built only while it shows, so the photograph view pays nothing for it.
        if model.mode == .grid {
            VStack(spacing: 0) {
                SheetSentence(model: model)
                SheetGrid(model: model)
                SheetFoot(model: model)
            }
            .background(Theme.film)
            .overlay(alignment: .topTrailing) { SheetHandle(model: model) }
            .environment(\.colorScheme, .dark)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("All photographs")
        }
    }
}

// MARK: The sentence

/// "Showing *all* 12 photographs", or when filtered "Showing 4 of 12 ·
/// rated *★★★ or more*, *picks*, *edited* ×". Each underlined word opens the
/// filter menu; × clears the filter. The filter lives only here (round 4).
private struct SheetSentence: View {
    let model: EditorModel

    var body: some View {
        let filter = model.library.filter
        let total = model.library.items.count
        HStack(spacing: 0) {
            if filter.active {
                Text("Showing \(model.shownPhotos.count) of \(total) · ")
                ForEach(Array(SheetWords.words(for: filter).enumerated()), id: \.offset) { index, part in
                    if index > 0 { Text(", ") }
                    if !part.lead.isEmpty { Text(part.lead) }
                    FilterWord(model: model, word: part.word, on: true)
                }
                Button { model.library.filter = LibraryFilter() } label: { ClearMark() }
                    .buttonStyle(.plain)
                    .padding(.leading, 8)
                    .help("Clear Filter (⌥⌘L)")
                    .accessibilityLabel("Clear Filter")
            } else {
                Text("Showing ")
                FilterWord(model: model, word: "all", on: false)
                Text(" " + SheetWords.photographs(total))
            }
        }
        .font(Theme.serif(17, italic: true))
        .foregroundStyle(Theme.dim)
        .lineLimit(1)
        .padding(.horizontal, 22)
        .frame(maxWidth: .infinity, minHeight: 46, maxHeight: 46, alignment: .leading)
    }
}

/// The sheet's words, worked out apart from the views so they can be tested.
enum SheetWords {
    struct Part: Equatable {
        /// Words before the underlined one ("rated ").
        let lead: String
        let word: String
    }

    static func photographs(_ count: Int) -> String { count == 1 ? "1 photograph" : "\(count) photographs" }

    /// What the filter lets through, as the sentence says it.
    static func words(for filter: LibraryFilter) -> [Part] {
        var parts: [Part] = []
        switch filter.rating {
        case .any: break
        case .atLeast(5): parts.append(Part(lead: "rated ", word: "★★★★★"))
        case .atLeast(let stars): parts.append(Part(lead: "rated ", word: String(repeating: "★", count: stars) + " or more"))
        case .unrated: parts.append(Part(lead: "", word: "unrated"))
        }
        let flags = FilterMenu.flagChoices.filter { filter.flags.contains($0.flag) }.map { $0.title.lowercased() }
        if !flags.isEmpty { parts.append(Part(lead: "", word: flags.joined(separator: " or "))) }
        if filter.editedOnly { parts.append(Part(lead: "", word: "edited")) }
        return parts
    }

    /// The foot's count: "12 photographs · 1 selected", or "4 of 12 photographs".
    static func count(shown: Int, total: Int, selected: Int, filtered: Bool) -> String {
        if filtered { return "\(shown) of \(photographs(total))" }
        return selected > 0 ? "\(photographs(total)) · \(selected) selected" : photographs(total)
    }
}

/// An underlined word with a ⌄ that opens the filter menu under it. While
/// a filter is on, the pencil is under it instead of the plain line.
private struct FilterWord: View {
    let model: EditorModel
    let word: String
    let on: Bool
    @State private var hovered = false
    @State private var anchor = MenuAnchor()

    var body: some View {
        Button {
            FilterMenu(model: model).popUp(below: anchor.view)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                Text(word)
                    .font(Theme.serif(17))
                    .foregroundStyle(Theme.paper)
                    .overlay(alignment: .bottom) {
                        if on {
                            PencilStroke().pencil().frame(height: 5).offset(y: 5)
                        } else {
                            Rectangle().fill(hovered ? Theme.dim : Theme.track).frame(height: 1).offset(y: 3)
                        }
                    }
                Text("⌄").font(Theme.ui(12)).foregroundStyle(Theme.dim).offset(y: -2)
            }
            .background(AnchorView(anchor: anchor))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help("Filter photographs by rating and flag")
        .accessibilityLabel("Filter: \(word)")
    }
}

private struct ClearMark: View {
    @State private var hovered = false
    var body: some View {
        Text("×").font(Theme.ui(13)).foregroundStyle(hovered ? Theme.paper : Theme.dim).onHover { hovered = $0 }
    }
}

/// The filter menu, a native menu with today's items: Rating, Flags,
/// Edited Only and Clear Filter (⌥⌘L).
@MainActor final class FilterMenu: NSObject {
    private let model: EditorModel
    init(model: EditorModel) { self.model = model }

    nonisolated static let flagChoices: [(flag: LibraryFilter.Flag, title: String)] = [(.pick, "Picks"), (.unflagged, "Unflagged"), (.reject, "Rejected")]

    /// Opens it under `view` and returns once it closes.
    func popUp(below view: NSView?) {
        guard let view else { return }
        let menu = NSMenu(title: "Filter")
        menu.appearance = NSAppearance(named: .darkAqua)
        fill(menu)
        let y = view.isFlipped ? view.bounds.maxY + 6 : view.bounds.minY - 6
        menu.popUp(positioning: nil, at: NSPoint(x: view.bounds.minX - 14, y: y), in: view)
    }

    /// Fills `menu` with the filter as it is now.
    func fill(_ menu: NSMenu) {
        let filter = model.library.filter
        func add(_ title: String, _ action: Selector, on: Bool, tag: Int = 0) {
            let item = menu.addItem(withTitle: title, action: action, keyEquivalent: "")
            item.target = self; item.tag = tag; item.state = on ? .on : .off
        }
        menu.addItem(.sectionHeader(title: "Rating"))
        add("Any Rating", #selector(rating(_:)), on: filter.rating == .any, tag: -1)
        for stars in 1...5 {
            add(stars == 5 ? "★★★★★" : "≥ " + String(repeating: "★", count: stars), #selector(rating(_:)), on: filter.rating == .atLeast(stars), tag: stars)
        }
        add("Unrated Only", #selector(rating(_:)), on: filter.rating == .unrated, tag: 0)
        menu.addItem(.separator())
        menu.addItem(.sectionHeader(title: "Flags"))
        for (index, (flag, title)) in Self.flagChoices.enumerated() {
            add(title, #selector(flag(_:)), on: filter.flags.contains(flag), tag: index)
        }
        menu.addItem(.separator())
        add("Edited Only", #selector(edited), on: filter.editedOnly)
        menu.addItem(.separator())
        let clear = menu.addItem(withTitle: "Clear Filter", action: filter.active ? #selector(clear) : nil, keyEquivalent: "l")
        clear.keyEquivalentModifierMask = [.command, .option]; clear.target = self
    }

    @objc private func rating(_ item: NSMenuItem) {
        model.library.filter.rating = switch item.tag { case -1: .any; case 0: .unrated; default: .atLeast(item.tag) }
    }
    @objc private func flag(_ item: NSMenuItem) {
        let flag = Self.flagChoices[item.tag].flag
        if model.library.filter.flags.contains(flag) { model.library.filter.flags.remove(flag) } else { model.library.filter.flags.insert(flag) }
    }
    @objc private func edited() { model.library.filter.editedOnly.toggle() }
    @objc private func clear() { model.library.filter = LibraryFilter() }
}

// MARK: The grid

private struct SheetGrid: View {
    let model: EditorModel
    /// The rubber band, in the grid's coordinates, and the selection it adds to.
    @State private var band: CGRect?
    @State private var bandBase: Set<URL> = []
    @FocusState private var focused: Bool
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        let library = model.library
        let shown = library.filtering ? [] : model.shownPhotos
        let ratios = model.aspects.ratios
        let target = CGFloat(model.gridCellSize)
        // Landscape pictures are about 1.5 × the row height across.
        let size = PhotoThumbnails.Size.grid(points: target * 1.5, scale: displayScale)
        GeometryReader { geometry in
            let layout = SheetLayout(ratios: shown.map { ratios[$0.url] ?? PhotoAspects.fallback }, width: geometry.size.width, target: target)
            grid(layout: layout, shown: shown, size: size, viewport: geometry.size)
        }
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear { focused = true }
        .onKeyPress(keys: [.leftArrow, .rightArrow, .upArrow, .downArrow], phases: [.down, .repeat]) { press in
            guard press.modifiers.isDisjoint(with: [.command, .option, .control]) else { return .ignored }
            let extending = press.modifiers.contains(.shift)
            switch press.key {
            case .leftArrow: model.move(by: -1, extending: extending)
            case .rightArrow: model.move(by: 1, extending: extending)
            case .upArrow: model.moveRow(by: -1, extending: extending)
            default: model.moveRow(by: 1, extending: extending)
            }
            return .handled
        }
        .overlay {
            if library.loading || library.filtering { ProgressView() }
            else if library.failed { SheetNote(text: "This folder couldn’t be read.") }
            else if library.files.isEmpty { SheetNote(text: "No supported photographs in this folder.") }
            else if shown.isEmpty { SheetNote(text: "No photographs match the filter.") }
        }
        .accessibilityLabel("Photographs")
    }

    private func grid(layout: SheetLayout, shown: [PhotoFile], size: PhotoThumbnails.Size, viewport: CGSize) -> some View {
        let selection = model.selection
        let currentRow = selection.current.flatMap { current in shown.firstIndex { $0.url == current } }.flatMap(layout.row(of:))
        return ScrollViewReader { proxy in
            ScrollView(.vertical) {
                ZStack(alignment: .topLeading) {
                    // Empty space: a click selects only the current
                    // photograph, a drag draws a rubber band.
                    Color.clear
                        .frame(width: viewport.width, height: max(layout.height, viewport.height))
                        .contentShape(Rectangle())
                        .gesture(bandGesture(layout, shown))
                        .onTapGesture {
                            focused = true
                            if NSEvent.modifierFlags.isDisjoint(with: [.command, .shift]) { model.deselectAll() }
                        }
                    LazyVStack(alignment: .leading, spacing: SheetLayout.gap) {
                        ForEach(layout.rows.indices, id: \.self) { index in
                            row(layout.rows[index], layout: layout, shown: shown, size: size)
                                // The ring reaches into the gaps round its row.
                                .zIndex(index == currentRow ? 1 : 0)
                                .id(index)
                        }
                    }
                    .padding(.leading, SheetLayout.side)
                    .padding(.top, SheetLayout.top)
                    .padding(.bottom, SheetLayout.bottom)
                    if let band {
                        Rectangle()
                            .fill(Theme.paper.opacity(0.06))
                            .overlay(Rectangle().strokeBorder(Theme.label.opacity(0.7), lineWidth: 1))
                            .frame(width: band.width, height: band.height)
                            .offset(x: band.minX, y: band.minY)
                            .allowsHitTesting(false)
                    }
                }
            }
            .scrollIndicators(.automatic)
            .onAppear {
                model.gridLayout = layout
                model.gridThumbnailSize = size
                // Opens on the current photograph.
                if let currentRow { proxy.scrollTo(currentRow, anchor: .center) }
            }
            .onChange(of: layout) { _, layout in model.gridLayout = layout }
            .onChange(of: size) { _, size in model.gridThumbnailSize = size; model.prefetchThumbnails() }
            .onChange(of: selection.current) { _, _ in
                guard band == nil, let currentRow else { return }
                proxy.scrollTo(currentRow)
            }
            .onChange(of: model.library.filtering) { _, filtering in
                if !filtering, let currentRow { proxy.scrollTo(currentRow, anchor: .center) }
            }
        }
    }

    private func row(_ row: SheetLayout.Row, layout: SheetLayout, shown: [PhotoFile], size: PhotoThumbnails.Size) -> some View {
        let selection = model.selection, library = model.library
        return HStack(spacing: SheetLayout.gap) {
            ForEach(row.range.map { (index: $0, file: shown[$0]) }, id: \.file.url) { cell in
                let url = cell.file.url
                SheetCell(file: cell.file, size: layout.frames[cell.index].size, thumbnail: size,
                          ringed: url == selection.current || selection.selected.contains(url),
                          current: url == selection.current, summary: library.summary(for: url),
                          readOnly: library.readOnly, edited: model.editedThumbnail(for: url)) { visible in
                    model.thumbnailShown(cell.file, visible)
                } loaded: { image in
                    model.aspects.note(url, image: image)
                } click: {
                    focused = true
                    let modifiers = NSEvent.modifierFlags
                    let command = modifiers.contains(.command), shift = modifiers.contains(.shift)
                    model.click(url, command: command, shift: shift)
                    // A plain click opens it and folds the grid.
                    if !command && !shift { model.editCurrent() }
                }
                .contextMenu { LibraryContextMenu(model: model, item: url) }
            }
        }
        .frame(height: row.height)
    }

    /// A drag from empty space selects the photographs it touches; with ⌘
    /// or ⇧ they're added to the selection.
    private func bandGesture(_ layout: SheetLayout, _ shown: [PhotoFile]) -> some Gesture {
        DragGesture(minimumDistance: 3)
            .onChanged { value in
                if band == nil {
                    focused = true
                    bandBase = NSEvent.modifierFlags.isDisjoint(with: [.command, .shift]) ? [] : model.selection.selected
                }
                let rect = CGRect(x: min(value.startLocation.x, value.location.x), y: min(value.startLocation.y, value.location.y),
                                  width: abs(value.location.x - value.startLocation.x), height: abs(value.location.y - value.startLocation.y))
                band = rect
                model.band(layout.indices(in: rect).filter { shown.indices.contains($0) }.map { shown[$0].url }, adding: bandBase)
            }
            .onEnded { _ in band = nil; bandBase = [] }
    }
}

private struct SheetNote: View {
    let text: String
    var body: some View { Text(text).font(Theme.serif(16, italic: true)).foregroundStyle(Theme.dim) }
}

/// One thumbnail: the picture filling its cell, the marks, and the pencil
/// ring round the current photograph and each one selected.
private struct SheetCell: View {
    let file: PhotoFile
    let size: CGSize
    let thumbnail: PhotoThumbnails.Size
    let ringed: Bool
    let current: Bool
    let summary: LibrarySummary
    let readOnly: Bool
    let edited: EditedThumbnail?
    /// On and off screen: what's shown is developed first.
    let shown: (Bool) -> Void
    let loaded: (CGImage) -> Void
    let click: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    private struct Load: Hashable { let file: PhotoFile; let size: PhotoThumbnails.Size; let edited: EditedThumbnail? }

    var body: some View {
        ZStack {
            if let image {
                Image(decorative: image, scale: 2).resizable().aspectRatio(contentMode: .fill)
            } else {
                Theme.blank
            }
        }
        .frame(width: size.width, height: size.height)
        .clipped()
        .opacity(summary.flag == .reject ? PhotoMarks.rejectOpacity(inStrip: false, current: current) : 1)
        .overlay { PhotoMarks(summary: summary) }
        .overlay {
            if ringed {
                PencilRing()
                    .stroke(Theme.accent, lineWidth: PencilRing.lineWidth)
                    .padding(.horizontal, -PencilRing.outset.width)
                    .padding(.vertical, -PencilRing.outset.height)
                    .rotationEffect(PencilRing.tilt)
                    .allowsHitTesting(false)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: click)
        .help(PhotoMarks.help(file.name, summary: summary, readOnly: readOnly))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(PhotoMarks.accessibilityLabel(file.name, summary: summary))
        .accessibilityAddTraits(ringed ? [.isButton, .isSelected] : [.isButton])
        .accessibilityAction { click() }
        .onAppear { visible = true; shown(true) }
        .onDisappear { visible = false; image = nil; shown(false) }
        .task(id: visible ? Load(file: file, size: thumbnail, edited: edited) : nil) {
            guard visible else { return }
            let file = file, thumbnail = thumbnail, edited = edited
            let request = Task.detached(priority: .utility) {
                await PhotoThumbnails.shared.image(for: file, size: thumbnail, look: edited?.look, edited: edited?.store)
            }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            // A new size keeps the last one shown until it arrives.
            if let result { image = result; loaded(result) }
        }
    }
}

// MARK: The foot and the handle

/// "12 photographs · 1 selected" on the left, and the rows' Size on the right.
private struct SheetFoot: View {
    let model: EditorModel

    var body: some View {
        HStack(spacing: 16) {
            Text(SheetWords.count(shown: model.shownPhotos.count, total: model.library.items.count,
                                  selected: model.selection.selected.count, filtered: model.library.filter.active))
            Spacer(minLength: 0)
            HStack(spacing: 14) {
                Text("Size")
                InkSlider(title: "Size", value: Binding(get: { model.gridCellSize }, set: { model.gridCellSize = $0.rounded() }),
                          range: SheetLayout.sizes, step: 1, neutral: SheetLayout.sizes.lowerBound, changed: false, hovered: false,
                          editing: { _ in }, reset: {}, toolTip: "Thumbnail size (⌘+ and ⌘−)", showsNeutral: false)
                    .frame(width: 96, height: 20)
            }
        }
        .font(Theme.ui(11))
        .foregroundStyle(Theme.dim)
        .padding(.horizontal, 18)
        .frame(height: Theme.Size.statusLine)
    }
}

/// The grid's handle: just the arrow, pointing down, at the top right, 14 pt
/// from the edge and 7 below the title bar. A click (or E, Return or Esc)
/// folds the grid back to the photograph.
private struct SheetHandle: View {
    let model: EditorModel
    @State private var hovered = false

    var body: some View {
        Button { model.editCurrent() } label: {
            // A 7 pt chevron with 1.5 pt strokes, turned to point down.
            LeaderArrow(down: true)
                .stroke(Theme.dim, style: StrokeStyle(lineWidth: 1.5, lineCap: .square))
                .frame(width: 9.9, height: 4.95)
                .offset(x: 0.5, y: hovered ? -0.5 : -3.5)
                .frame(width: 36, height: 32)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { over in withAnimation(.easeOut(duration: 0.3)) { hovered = over } }
        .padding(.trailing, 14)
        .padding(.top, 7)
        .help("Edit Photograph (E)")
        .accessibilityLabel("Edit Photograph")
    }
}
