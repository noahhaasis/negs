import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// What the inspector column shows beside a photograph.
enum InspectorPane: String {
    case adjustments, presets
}

/// The Presets pane (§4.12): the presets in the app's folder as tiles in two
/// columns, each the open photograph developed through it (unless Settings
/// turns previews off), its name, and its film and paper. A click applies
/// one; the one applied last has the pencil ring. Tiles drag to sort, and
/// preset files dropped on the pane are imported. New and Import… are in
/// the pane header; the folder is in the File menu.
struct PresetInspector: View {
    let model: EditorModel
    /// Preset files over the pane: the pencil ring round it.
    @State private var dropTargeted = false
    /// The order while a tile is dragged, before it's dropped.
    @State private var dragOrder: [URL]?
    @State private var dragged: URL?
    /// Each tile's frame in the pane, to find the one under a drag.
    @State private var frames: [URL: CGRect] = [:]

    static let space = "presets"

    var body: some View {
        VStack(spacing: 0) {
            InkRule()
            if model.presets.isEmpty {
                PresetsEmpty(model: model)
            } else {
                tiles
                PresetsFoot(dropping: dropTargeted)
            }
        }
        .coordinateSpace(name: Self.space)
        .onPreferenceChange(TileFrames.self) { frames = $0 }
        .onDrop(of: [.fileURL, .utf8PlainText], delegate: PresetsDrop(model: model, frames: frames, targeted: $dropTargeted,
                                                                      order: $dragOrder, dragged: $dragged))
        .overlay {
            if dropTargeted {
                PencilRing(radii: PresetsEmpty.dropRing).pencil(width: PencilRing.lineWidth)
                    .padding(9)
                    .allowsHitTesting(false)
            }
        }
        .task { await model.refreshPresets() }
        .disabled(!model.canChangePresets)
        // One named group, which also keeps its rules out of VoiceOver.
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Presets")
    }

    /// The presets in the order shown: as dragged, while a tile is.
    private var shown: [PresetFile] {
        guard let dragOrder else { return model.presets }
        return dragOrder.compactMap { url in model.presets.first { $0.url == url } }
    }

    private var tiles: some View {
        ScrollViewReader { scroller in
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: PresetTileRules.columnGap, alignment: .top), count: 2),
                          alignment: .leading, spacing: PresetTileRules.rowGap) {
                    ForEach(shown) { preset in
                        PresetTile(model: model, preset: preset, status: model.presetStatus[preset],
                                   tile: model.previews.enabled ? model.previews.tile(preset: preset.url) : nil,
                                   pictures: model.previews.enabled,
                                   applied: model.appliedPreset == preset.url, renaming: model.renamingPreset == preset.url)
                            .id(preset.url)
                            .background(GeometryReader { proxy in
                                Color.clear.preference(key: TileFrames.self, value: [preset.url: proxy.frame(in: .named(Self.space))])
                            })
                            .onDrag {
                                dragged = preset.url
                                return NSItemProvider(object: PresetsDrop.marker(preset.url) as NSString)
                            }
                    }
                }
                .padding(EdgeInsets(top: 18, leading: Theme.Size.gutter, bottom: 22, trailing: Theme.Size.gutter))
            }
            .scrollIndicators(.never)
            .onChange(of: model.renamingPreset) { _, url in
                if let url { withAnimation { scroller.scrollTo(url) } }
            }
        }
        .onAppear { setShown(true) }
        .onDisappear { setShown(false) }
    }

    /// Thumbnails are made only while the tiles are on screen.
    private func setShown(_ shown: Bool) {
        model.previews.presetsShown = shown
        model.pumpPreviews()
    }
}

enum PresetTileRules {
    /// Gaps between tiles: 14 across, 20 down (`presets.css`).
    static let columnGap: CGFloat = 14, rowGap: CGFloat = 20

    /// The film's line under the name, in the label's words.
    static func film(_ status: PresetStatus?, films: [Stock]) -> String? {
        guard case .valid(let film, _, _) = status else { return nil }
        return films.first { $0.id == film }?.name ?? film
    }

    /// The paper's line: "printed on" and the paper, or that the film is
    /// scanned. The first part is italic.
    static func paper(_ status: PresetStatus?, papers: [Stock]) -> (lead: String, name: String)? {
        guard case .valid(_, let paper, let scan) = status else { return nil }
        if scan { return ("not printed · film scan", "") }
        return ("printed on", papers.first { $0.id == paper }?.name ?? paper)
    }

    /// What the foot says: what a click and a drag do, or what letting go of
    /// the files over the pane will do.
    static func foot(dropping: Bool) -> String {
        dropping ? "drop to import" : "click to apply · drag to sort · drop files to import"
    }
}

/// Each tile's frame in the pane's coordinates.
private struct TileFrames: PreferenceKey {
    static let defaultValue: [URL: CGRect] = [:]
    static func reduce(value: inout [URL: CGRect], nextValue: () -> [URL: CGRect]) {
        value.merge(nextValue()) { $1 }
    }
}

/// Drops on the pane: a tile being dragged moves to where it's held, and
/// lands there; preset files are imported before the tile they're over,
/// or at the end.
struct PresetsDrop: DropDelegate {
    let model: EditorModel
    let frames: [URL: CGRect]
    @Binding var targeted: Bool
    @Binding var order: [URL]?
    @Binding var dragged: URL?

    /// What a dragged tile carries: its file, marked as this pane's own.
    static func marker(_ preset: URL) -> String { "negs-preset:" + preset.absoluteString }
    static func preset(in marker: String) -> URL? {
        guard marker.hasPrefix("negs-preset:") else { return nil }
        return URL(string: String(marker.dropFirst("negs-preset:".count)))
    }

    private func files(_ info: DropInfo) -> Bool { info.hasItemsConforming(to: [.fileURL]) }

    /// The tile under `point`, if any.
    static func tile(at point: CGPoint, in frames: [URL: CGRect]) -> URL? {
        frames.first { $0.value.insetBy(dx: -PresetTileRules.columnGap / 2, dy: -PresetTileRules.rowGap / 2).contains(point) }?.key
    }

    func validateDrop(info: DropInfo) -> Bool {
        model.canChangePresets && (files(info) || dragged != nil)
    }

    func dropEntered(info: DropInfo) {
        if files(info) { targeted = true }
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        guard model.canChangePresets else { return DropProposal(operation: .forbidden) }
        if files(info) { targeted = true; return DropProposal(operation: .copy) }
        guard let dragged else { return DropProposal(operation: .forbidden) }
        var next = order ?? model.presets.map(\.url)
        if let over = Self.tile(at: info.location, in: frames), over != dragged,
           let from = next.firstIndex(of: dragged), let to = next.firstIndex(of: over) {
            next.move(fromOffsets: [from], toOffset: to > from ? to + 1 : to)
            withAnimation(.easeOut(duration: 0.15)) { order = next }
        }
        return DropProposal(operation: .move)
    }

    func dropExited(info: DropInfo) {
        targeted = false
        // A tile taken out of the pane goes back where it was.
        if order != nil { withAnimation(.easeOut(duration: 0.15)) { order = nil } }
    }

    func performDrop(info: DropInfo) -> Bool {
        targeted = false
        if files(info) {
            let position = Self.tile(at: info.location, in: frames).flatMap { url in model.presets.firstIndex { $0.url == url } }
            let providers = info.itemProviders(for: [.fileURL])
            Task { @MainActor in
                let urls = await Self.fileURLs(providers)
                model.importPresets(urls, at: position)
            }
            return true
        }
        defer { order = nil; dragged = nil }
        guard let dragged, let order, let from = model.presets.firstIndex(where: { $0.url == dragged }),
              let to = order.firstIndex(of: dragged) else { return false }
        model.movePresets(from: [from], to: to > from ? to + 1 : to)
        return true
    }

    private static func fileURLs(_ providers: [NSItemProvider]) async -> [URL] {
        var urls: [URL] = []
        for provider in providers {
            let url: URL? = await withCheckedContinuation { continuation in
                _ = provider.loadObject(ofClass: URL.self) { url, _ in continuation.resume(returning: url) }
            }
            if let url, url.isFileURL { urls.append(url) }
        }
        return urls
    }
}

/// One preset: the photograph developed through it at 3:2, its name in
/// Newsreader 15, and its film and paper on two lines, each cut rather than
/// wrapped. One that can't be read says so in place of the picture.
private struct PresetTile: View {
    let model: EditorModel
    let preset: PresetFile
    let status: PresetStatus?
    /// The thumbnail; nil until it's made.
    let tile: PreviewTile?
    /// Settings can turn the pictures off.
    let pictures: Bool
    let applied: Bool
    let renaming: Bool
    @State private var hovering = false
    /// The name as it's typed, so the field and its pencil grow with it.
    @State private var draft = ""

    private var invalid: String? {
        if case .invalid(let reason) = status { return reason }
        return nil
    }
    private var valid: Bool { if case .valid = status { true } else { false } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if pictures { picture }
            caption.padding(.top, pictures ? 6 : 0)
        }
        .contentShape(Rectangle())
        .onTapGesture { if !renaming { apply() } }
        .onHover { hovering = $0 }
        .inkFocus(activate: apply, delete: { model.deletePreset(preset) })
        .contextMenu {
            Button("Apply", action: apply).disabled(!valid)
            Button("Rename") { model.renamingPreset = preset.url }
            Button("Export…") { model.exportPresetPanel(preset) }
            Divider()
            Button("Delete…") { model.deletePreset(preset) }
        }
        .help(invalid ?? preset.name)
        .accessibilityElement(children: renaming ? .contain : .ignore)
        .accessibilityLabel(preset.name)
        .accessibilityValue(accessibilityValue)
        .accessibilityAddTraits(applied ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction(.default, apply)
        .accessibilityAction(named: "Rename") { model.renamingPreset = preset.url }
        .accessibilityAction(named: "Delete") { model.deletePreset(preset) }
    }

    private func apply() {
        guard valid else { return }
        model.applyPreset(preset)
    }

    private var picture: some View {
        Theme.blank
            .aspectRatio(3 / 2, contentMode: .fit)
            .overlay {
                if let image = tile?.image, invalid == nil {
                    Image(decorative: image, scale: 2).resizable().aspectRatio(contentMode: .fill)
                        .opacity(tile?.dimmed == true ? 0.55 : 1)
                } else if invalid != nil {
                    Text("can’t be read").font(Theme.serif(13, italic: true)).foregroundStyle(Theme.dim)
                }
            }
            .clipped()
            .overlay {
                if applied {
                    PencilRing().pencil(width: PencilRing.lineWidth)
                        .padding(EdgeInsets(top: -6, leading: -7, bottom: -6, trailing: -7))
                        .rotationEffect(.degrees(-1))
                        .allowsHitTesting(false)
                }
            }
    }

    private var caption: some View {
        VStack(alignment: .leading, spacing: 1) {
            if renaming {
                NameField(name: preset.name, draft: $draft) { model.renamePreset(preset, to: $0) } cancel: { model.cancelRename(preset) } refuse: { name, problem in
                    model.tell(AppAlert(title: "“\(name)” can’t be used as a preset name.", detail: problem))
                }
                .namingUnderline()
            } else {
                Text(preset.name)
                    .font(Theme.serif(15, weight: applied ? 600 : 400))
                    .foregroundStyle(invalid != nil ? Theme.dim : applied || hovering ? Theme.paper : Theme.label)
                    .lineLimit(1).truncationMode(.tail)
            }
            Group {
                if let invalid {
                    Text(invalid).fixedSize(horizontal: false, vertical: true)
                } else if let film = PresetTileRules.film(status, films: model.films),
                          let paper = PresetTileRules.paper(status, papers: model.papers) {
                    Text(film).lineLimit(1).truncationMode(.tail)
                    (Text(paper.lead).font(Theme.serif(12.5, italic: true)) + Text(paper.name.isEmpty ? "" : " " + paper.name))
                        .lineLimit(1).truncationMode(.tail)
                } else {
                    // Not checked yet: the lines keep their place.
                    Text(" ")
                    Text(" ")
                }
            }
            .font(Theme.serif(12.5))
            .foregroundStyle(Theme.dim)
            .lineSpacing(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var accessibilityValue: String {
        if let invalid { return "Can’t be read. " + invalid }
        guard let film = PresetTileRules.film(status, films: model.films), let paper = PresetTileRules.paper(status, papers: model.papers)
        else { return "" }
        let print = paper.name.isEmpty ? paper.lead : "\(paper.lead) \(paper.name)"
        return (applied ? "Applied last. " : "") + "\(film), \(print)"
    }
}

/// No presets: what they are and how to make some, and Import Presets….
private struct PresetsEmpty: View {
    let model: EditorModel
    /// The loose ring round the pane while files are over it (`presets.css`).
    static let dropRing = PencilRing.Radii(horizontal: (18, 26, 16, 24), vertical: (22, 14, 26, 16))

    var body: some View {
        VStack(spacing: 10) {
            Text("No presets yet")
                .font(Theme.serif(21, weight: 500)).foregroundStyle(Theme.paper)
            // The bundled serif has no synthetic italic: each part takes its own font.
            (Text("Save this photograph’s film, paper and Lab settings with ").font(Theme.serif(14, italic: true))
             + Text("New").font(Theme.serif(14)).foregroundColor(Theme.label)
             + Text(", or import presets made in Negs or spektrafilm. You can also drop them here.").font(Theme.serif(14, italic: true)))
                .foregroundStyle(Theme.dim)
                .lineSpacing(3)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            Button("Import Presets…", action: model.importPresetsPanel)
                .buttonStyle(OutlinedButtonStyle())
                .padding(.top, 6)
                .help("Import presets (⇧⌘O)")
        }
        .padding(EdgeInsets(top: 0, leading: 44, bottom: 40, trailing: 44))
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// "click to apply · drag to sort · drop files to import", 34 tall under a rule.
private struct PresetsFoot: View {
    let dropping: Bool

    var body: some View {
        Text(PresetTileRules.foot(dropping: dropping))
            .font(Theme.serif(13, italic: true))
            .foregroundStyle(dropping ? Theme.paper : Theme.dim)
            .lineLimit(1)
            .frame(maxWidth: .infinity)
            .frame(height: 34)
            .overlay(alignment: .top) { InkRule() }
    }
}

/// The name being edited, in place of the tile's name: a borderless field
/// underlined in pencil, with a `paper` caret. Return or clicking elsewhere
/// keeps it if it's a valid preset name (an invalid one keeps the old name),
/// Escape cancels. AppKit, like the inspector's value fields.
private struct NameField: NSViewRepresentable {
    let name: String
    /// The text as typed; its width is the field's.
    @Binding var draft: String
    let commit: (String) -> Void
    let cancel: () -> Void
    /// A name that can't be used, and why.
    let refuse: (String, String) -> Void

    func makeNSView(context: Context) -> NSTextField {
        let field = InkTextField(string: name)
        field.isBordered = false
        field.isBezeled = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.font = Theme.serifFont(15) as NSFont
        field.textColor = Theme.NS.paper
        field.cell?.isScrollable = true
        field.cell?.wraps = false
        field.setAccessibilityLabel("Preset name")
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) { context.coordinator.parent = self }
    func makeCoordinator() -> Coordinator { Coordinator(self) }

    /// As wide as the name, so the pencil runs under it alone, up to the tile's width.
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSTextField, context: Context) -> CGSize? {
        let text = (draft.isEmpty ? name : draft) as NSString
        let width = ceil(text.size(withAttributes: [.font: nsView.font as Any]).width) + 8
        return CGSize(width: min(width, proposal.width ?? width), height: ceil(nsView.intrinsicContentSize.height))
    }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: NameField
        private var finished = false
        init(_ parent: NameField) { self.parent = parent }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            parent.draft = field.stringValue
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.cancelOperation(_:)) else { return false }
            finished = true
            parent.cancel()
            return true
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard !finished, let field = notification.object as? NSTextField else { return }
            finished = true
            let name = field.stringValue.trimmingCharacters(in: .whitespaces)
            if let problem = PresetName.problem(name) {
                NSSound.beep()
                parent.commit(parent.name)
                if !name.isEmpty { parent.refuse(name, problem) }
            } else {
                parent.commit(name)
            }
        }
    }
}

extension View {
    /// The pencil under a field being named, 5 pt tall, 3 pt under the text.
    fileprivate func namingUnderline() -> some View {
        padding(.bottom, 3).overlay(alignment: .bottom) {
            PencilStroke().pencil().frame(height: 5).offset(y: 2).allowsHitTesting(false)
        }
    }
}
