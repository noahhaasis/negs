import AppKit
import SwiftUI

/// Where things sit in the grid view: four columns of photographs on black, each
/// with its camera details above and below it.
enum PhotoGridLayout {
    static let columns = 4
    static let margin: CGFloat = 24, columnGap: CGFloat = 24, rowGap: CGFloat = 8
    /// A line of details, and its gap from the photograph.
    static let lineHeight: CGFloat = 16, lineGap: CGFloat = 4

    static func cellWidth(available: CGFloat) -> CGFloat {
        max(((available - margin * 2 - columnGap * CGFloat(columns - 1)) / CGFloat(columns)).rounded(.down), 1)
    }
    /// Photographs fit a 3:2 box, so rows line up whatever their shape.
    static func boxHeight(cellWidth: CGFloat) -> CGFloat { (cellWidth * 2 / 3).rounded() }
    static func cellHeight(cellWidth: CGFloat) -> CGFloat { boxHeight(cellWidth: cellWidth) + (lineHeight + lineGap) * 2 }

    /// A photograph of `image`'s shape, fitted and centred in `box`.
    static func photoRect(image: CGSize, box: CGSize) -> CGRect {
        guard image.width > 0, image.height > 0 else { return CGRect(origin: .zero, size: box) }
        let scale = min(box.width / image.width, box.height / image.height)
        let size = CGSize(width: image.width * scale, height: image.height * scale)
        return CGRect(x: (box.width - size.width) / 2, y: (box.height - size.height) / 2, width: size.width, height: size.height)
    }

    /// The middle of the photographs on screen, which develop first.
    static func middle(of visible: Set<Int>) -> Int? {
        let sorted = visible.sorted()
        return sorted.isEmpty ? nil : sorted[sorted.count / 2]
    }
}

/// The grid view: every photograph in the folder with its final edit.
struct PhotoGrid: View {
    let model: EditorModel
    let directory: URL
    @State private var listing = FolderListing()
    /// Not observed: which cells are up only steers the developer.
    @State private var visible = VisibleCells()
    /// Up for good once the frames around where it opens are ready.
    @State private var revealed = false

    private var shown: [PhotoFile] { PhotoFolder.shown(listing.files, rawOnly: model.filmstripRawOnly) }

    var body: some View {
        GeometryReader { geometry in
            let width = PhotoGridLayout.cellWidth(available: geometry.size.width)
            ScrollViewReader { reader in
                ScrollView(.vertical) {
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(width), spacing: PhotoGridLayout.columnGap), count: PhotoGridLayout.columns),
                              spacing: PhotoGridLayout.rowGap) {
                        ForEach(Array(shown.enumerated()), id: \.element.url) { index, file in
                            PhotoGridCell(file: file, width: width, developed: model.rollDeveloper?.frames[file.url]) {
                                model.leaveGrid(opening: file.url)
                            }
                            .id(file.url)
                            .onAppear { visible.indices.insert(index); steer() }
                            .onDisappear { visible.indices.remove(index); steer() }
                        }
                    }
                    .padding(PhotoGridLayout.margin)
                }
                // Nothing shows until what's on screen is ready to draw.
                .opacity(revealed ? 1 : 0)
                .task(id: directory) {
                    await listing.watch(directory) {
                        model.rollDeveloper?.show(files: shown, centre: shown.firstIndex { $0.url == model.source } ?? 0)
                        // Starts at the open photograph; nothing moves it after that.
                        if let source = model.source {
                            DispatchQueue.main.async { reader.scrollTo(source, anchor: .center) }
                        }
                    }
                }
            }
        }
        .background(Color.black)
        .overlay {
            Group {
                if listing.loading { ProgressView().controlSize(.small) }
                else if !revealed, let roll = model.rollDeveloper, roll.nearTotal > 0 {
                    VStack(spacing: 8) {
                        ProgressView(value: Double(roll.nearFinished), total: Double(roll.nearTotal))
                            .frame(width: 180)
                            .tint(EdgePrint.amber)
                        Text("Developing · \(roll.nearFinished) of \(roll.nearTotal)")
                            .font(Font(EdgePrint.font(size: 14)))
                            .monospacedDigit()
                    }
                }
                else if listing.failed { Text("This folder couldn’t be read.") }
                else if shown.isEmpty { Text(model.filmstripRawOnly ? "No RAW files in this folder." : "No supported photographs in this folder.") }
            }
            .foregroundStyle(Color(nsColor: EdgePrint.colour))
        }
        .accessibilityLabel("Grid")
        .onChange(of: shown) { _, files in model.rollDeveloper?.show(files: files) }
        .onChange(of: model.rollDeveloper?.nearReady ?? false, initial: true) { _, ready in if ready { revealed = true } }
        .onChange(of: model.rollDeveloper == nil, initial: true) { _, none in if none { revealed = true } }
        // The open photograph's edit, or the folder's look, once changes pause.
        .onChange(of: model.state) { _, _ in visible.refresh(model) }
        .onChange(of: model.sameLookForRoll) { _, _ in model.rollDeveloper?.invalidateAll() }
    }

    private func steer() {
        if let middle = PhotoGridLayout.middle(of: visible.indices) { model.rollDeveloper?.setCentre(middle) }
    }
}

@MainActor private final class VisibleCells {
    var indices: Set<Int> = []
    private var refreshing: Task<Void, Never>?

    func refresh(_ model: EditorModel) {
        refreshing?.cancel()
        refreshing = Task { [weak model] in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled, let model, let source = model.source else { return }
            if model.sameLookForRoll { model.rollDeveloper?.invalidateAll() } else { model.rollDeveloper?.invalidate(source) }
        }
    }
}

/// One photograph, with its details in amber above and below, starting at its left edge.
private struct PhotoGridCell: View {
    let file: PhotoFile
    let width: CGFloat
    /// The final edit, once it's been developed. Nothing shows before that.
    let developed: CGImage?
    let open: () -> Void
    @State private var exif: PhotoExif?
    @State private var visible = false

    var body: some View {
        let box = CGSize(width: width, height: PhotoGridLayout.boxHeight(cellWidth: width))
        let image = developed
        let rect = PhotoGridLayout.photoRect(image: image.map { CGSize(width: $0.width, height: $0.height) } ?? CGSize(width: 3, height: 2), box: box)
        VStack(alignment: .leading, spacing: PhotoGridLayout.lineGap) {
            line(image == nil ? nil : exif?.topLine, left: rect.minX, width: width)
            ZStack(alignment: .topLeading) {
                if let image {
                    Image(decorative: image, scale: 1)
                        .resizable()
                        .frame(width: rect.width, height: rect.height)
                        .offset(x: rect.minX, y: rect.minY)
                }
                DoubleClickArea(open: open)
                    .frame(width: rect.width, height: rect.height)
                    .offset(x: rect.minX, y: rect.minY)
            }
            .frame(width: box.width, height: box.height, alignment: .topLeading)
            line(image == nil ? nil : exif?.bottomLine, left: rect.minX, width: width)
        }
        .frame(width: width, height: PhotoGridLayout.cellHeight(cellWidth: width), alignment: .topLeading)
        .accessibilityElement()
        .accessibilityLabel(file.name)
        .accessibilityAddTraits(.isButton)
        .accessibilityAction(.default, open)
        .onAppear { visible = true }
        .onDisappear { visible = false }
        .task(id: visible ? file : nil) {
            guard visible else { return }
            let read = await Task.detached(priority: .utility) { PhotoExif.read(file.url) }.value
            if !Task.isCancelled { exif = read }
        }
    }

    private func line(_ text: String?, left: CGFloat, width: CGFloat) -> some View {
        Text(text ?? "")
            .font(Font(EdgePrint.font(size: 12)))
            .foregroundStyle(Color(nsColor: EdgePrint.colour))
            .lineLimit(1)
            .truncationMode(.tail)
            .padding(.leading, left)
            .frame(width: width, height: PhotoGridLayout.lineHeight, alignment: .leading)
    }
}

/// Opens on a double-click, in a window that isn't active too. Scrolling carries
/// on to the scroll view around it.
struct DoubleClickArea: NSViewRepresentable {
    let open: () -> Void
    func makeNSView(context: Context) -> Area { Area() }
    func updateNSView(_ view: Area, context: Context) { view.open = open }

    final class Area: NSView {
        var open: (() -> Void)?
        override func isAccessibilityElement() -> Bool { false }
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        override func mouseDown(with event: NSEvent) {}
        override func mouseUp(with event: NSEvent) { if event.clickCount == 2 { open?() } }
    }
}
