import AppKit
import SwiftUI

/// The folder as a grid, for culling. Like the film strip it shows the
/// embedded previews, so it never develops a RAW, and the photographs as
/// filtered. Clicks and the arrow keys move the current photograph without
/// opening it; leaving the grid opens it.
struct LibraryGrid: View {
    let model: EditorModel
    /// The rubber band, in the grid's coordinates, and the selection it adds to.
    @State private var band: CGRect?
    @State private var bandBase: Set<URL> = []
    /// From appearing until the columns have held still for a second.
    @State private var arriving = false
    /// Laid out at the width the grid will have once the inspector has slid
    /// away, so its columns don't change and its rows don't move as it opens.
    @State private var settled = false
    @FocusState private var focused: Bool
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        let library = model.library
        let shown = library.filtering ? [] : model.shownPhotos
        let selection = model.selection
        GeometryReader { geometry in
            let width = settled ? geometry.size.width : max(geometry.size.width, model.fullCanvasWidth())
            let metrics = GridMetrics(width: width, cell: model.gridCellSize)
            let size = PhotoThumbnails.Size.grid(points: metrics.cell, scale: displayScale)
            ScrollViewReader { proxy in
                ScrollView(.vertical) {
                    ZStack(alignment: .topLeading) {
                        // Empty space: a click selects only the current
                        // photograph, a drag draws a rubber band.
                        Color.clear
                            .frame(width: width, height: max(metrics.height(shown.count), geometry.size.height))
                            .contentShape(Rectangle())
                            .gesture(bandGesture(metrics, shown))
                            .onTapGesture {
                                focused = true
                                if NSEvent.modifierFlags.isDisjoint(with: [.command, .shift]) { model.deselectAll() }
                            }
                        LazyVStack(alignment: .leading, spacing: GridMetrics.spacing) {
                            ForEach(0..<metrics.rows(shown.count), id: \.self) { row in
                                HStack(spacing: GridMetrics.spacing) {
                                    ForEach(shown[row * metrics.columns..<min((row + 1) * metrics.columns, shown.count)], id: \.url) { file in
                                        GridCell(file: file, metrics: metrics, size: size, current: file.url == selection.current,
                                                 selected: selection.selected.contains(file.url), summary: library.summary(for: file.url),
                                                 readOnly: library.readOnly, edited: model.editedThumbnail(for: file.url)) { visible in
                                            model.thumbnailShown(file, visible)
                                        } click: {
                                            focused = true
                                            let modifiers = NSEvent.modifierFlags
                                            model.click(file.url, command: modifiers.contains(.command), shift: modifiers.contains(.shift))
                                        } edit: {
                                            model.click(file.url)
                                            model.editCurrent()
                                        }
                                        .contextMenu { LibraryContextMenu(model: model, item: file.url) }
                                    }
                                }
                                .id(row)
                            }
                        }
                        .padding(.leading, metrics.inset)
                        .padding(.vertical, GridMetrics.padding)
                        if let band {
                            Rectangle()
                                .fill(Color.accentColor.opacity(0.12))
                                .overlay(Rectangle().strokeBorder(Color.accentColor.opacity(0.8), lineWidth: 1))
                                .frame(width: band.width, height: band.height)
                                .offset(x: band.minX, y: band.minY)
                                .allowsHitTesting(false)
                        }
                    }
                    // Wider while the inspector slides away: it uncovers the right.
                    .frame(width: geometry.size.width, alignment: .topLeading)
                }
                .onAppear {
                    model.gridColumns = metrics.columns
                    model.gridThumbnailSize = size
                    // Opens on the current photograph.
                    arriving = true
                    if let row = currentRow(shown, metrics) { proxy.scrollTo(row, anchor: .center) }
                }
                .onChange(of: metrics.columns) { _, columns in
                    model.gridColumns = columns
                    // The grid opens while the inspector slides away, so it
                    // gains columns and its rows move; the current photograph
                    // stays in the middle until they settle.
                    if arriving, let row = currentRow(shown, metrics) { proxy.scrollTo(row, anchor: .center) }
                }
                .onChange(of: size) { _, size in model.gridThumbnailSize = size; model.prefetchThumbnails() }
                .task(id: metrics.columns) {
                    try? await Task.sleep(for: .seconds(1))
                    if !Task.isCancelled { arriving = false; settled = true }
                }
                .onChange(of: selection.current) { _, _ in
                    guard band == nil, let row = currentRow(shown, metrics) else { return }
                    proxy.scrollTo(row)
                }
                .onChange(of: library.filtering) { _, filtering in
                    if !filtering, let row = currentRow(model.shownPhotos, metrics) { proxy.scrollTo(row, anchor: .center) }
                }
            }
        }
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear { focused = true }
        .onKeyPress(keys: [.leftArrow, .rightArrow, .upArrow, .downArrow], phases: [.down, .repeat]) { press in
            guard press.modifiers.isDisjoint(with: [.command, .option, .control]) else { return .ignored }
            let columns = max(model.gridColumns, 1)
            let offset = switch press.key { case .leftArrow: -1; case .rightArrow: 1; case .upArrow: -columns; default: columns }
            model.move(by: offset, extending: press.modifiers.contains(.shift))
            return .handled
        }
        .background(Color(nsColor: .controlBackgroundColor))
        .overlay {
            if library.loading || library.filtering { ProgressView() }
            else if library.failed { Text("This folder couldn’t be read.").foregroundStyle(.secondary) }
            else if library.files.isEmpty { Text("No supported photographs in this folder.").foregroundStyle(.secondary) }
            else if shown.isEmpty { Text("No photographs match the filter.").foregroundStyle(.secondary) }
        }
        .accessibilityLabel("Photographs")
    }

    private func currentRow(_ shown: [PhotoFile], _ metrics: GridMetrics) -> Int? {
        guard let current = model.selection.current, let index = shown.firstIndex(where: { $0.url == current }) else { return nil }
        return index / metrics.columns
    }

    /// A drag from empty space selects the photographs it touches; with ⌘
    /// or ⇧ they're added to the selection.
    private func bandGesture(_ metrics: GridMetrics, _ shown: [PhotoFile]) -> some Gesture {
        DragGesture(minimumDistance: 3)
            .onChanged { value in
                if band == nil {
                    focused = true
                    bandBase = NSEvent.modifierFlags.isDisjoint(with: [.command, .shift]) ? [] : model.selection.selected
                }
                let rect = CGRect(x: min(value.startLocation.x, value.location.x), y: min(value.startLocation.y, value.location.y),
                                  width: abs(value.location.x - value.startLocation.x), height: abs(value.location.y - value.startLocation.y))
                band = rect
                model.band(metrics.indices(in: rect, count: shown.count).map { shown[$0].url }, adding: bandBase)
            }
            .onEnded { _ in band = nil; bandBase = [] }
    }
}

private struct GridCell: View {
    let file: PhotoFile
    let metrics: GridMetrics
    let size: PhotoThumbnails.Size
    /// The photograph leaving the grid opens, outlined thickly; others
    /// selected, thinly.
    let current: Bool
    let selected: Bool
    let summary: LibrarySummary
    let readOnly: Bool
    let edited: EditedThumbnail?
    /// On and off screen: what's shown is developed first.
    let shown: (Bool) -> Void
    let click: () -> Void
    let edit: () -> Void
    @State private var image: CGImage?
    @State private var visible = false

    private struct Load: Hashable { let file: PhotoFile; let size: PhotoThumbnails.Size; let edited: EditedThumbnail? }

    var body: some View {
        VStack(spacing: 4) {
            ZStack {
                RoundedRectangle(cornerRadius: 6).fill(selected ? Color.accentColor.opacity(0.12) : .primary.opacity(0.04))
                Group {
                    if let image {
                        Image(decorative: image, scale: 2).resizable().scaledToFit().padding(5)
                    } else {
                        Image(systemName: "photo").font(.title2).foregroundStyle(.tertiary)
                    }
                }
                // Rejects are greyed; their name stays readable.
                .opacity(summary.flag == .reject ? 0.4 : 1)
            }
            .frame(width: metrics.cell, height: metrics.imageHeight)
            .overlay { PhotoMarks(summary: summary) }
            .overlay {
                RoundedRectangle(cornerRadius: 6)
                    .strokeBorder(current || selected ? Color.accentColor : .clear, lineWidth: current ? 3 : 1.5)
            }
            Text(file.name)
                .font(.caption)
                .lineLimit(1)
                .truncationMode(.middle)
                .foregroundStyle(current || selected ? .primary : .secondary)
                .frame(width: metrics.cell)
        }
        .frame(width: metrics.cell, height: metrics.rowHeight, alignment: .top)
        .contentShape(Rectangle())
        .onTapGesture(count: 2, perform: edit)
        .simultaneousGesture(TapGesture().onEnded(click))
        .help(PhotoMarks.help(file.name, summary: summary, readOnly: readOnly))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(PhotoMarks.accessibilityLabel(file.name, summary: summary))
        .accessibilityAddTraits(current || selected ? [.isButton, .isSelected] : [.isButton])
        .accessibilityAction(named: "Edit", edit)
        .accessibilityAction { click() }
        .onAppear { visible = true; shown(true) }
        .onDisappear { visible = false; image = nil; shown(false) }
        .task(id: visible ? Load(file: file, size: size, edited: edited) : nil) {
            guard visible else { return }
            let file = file, size = size, edited = edited
            let request = Task.detached(priority: .utility) {
                await PhotoThumbnails.shared.image(for: file, size: size, look: edited?.look, edited: edited?.store)
            }
            let result = await withTaskCancellationHandler {
                await request.value
            } onCancel: { request.cancel() }
            guard !Task.isCancelled else { return }
            // A new size keeps the last one shown until it arrives.
            if let result { image = result }
        }
    }
}
