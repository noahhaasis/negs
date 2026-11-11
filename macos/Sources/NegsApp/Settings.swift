import AppKit
import SwiftUI

// Settings in the T2 · Label look, as "B · Index" in design/proposals/r19-settings.html:
// the title band over the inspector's tabs, here Photographs, Speed and Upkeep,
// each a short page of the inspector's rows. The window fits its page's height.

/// Settings' pages, in the order of their tabs.
enum SettingsTab: String, CaseIterable {
    case photographs = "Photographs", speed = "Speed", upkeep = "Upkeep"
}

/// The Settings window: Newsreader's title in the 38 pt band over the tabs,
/// no toolbar, and the keyboard focus box rather than the system's ring.
@MainActor final class SettingsWindowController: NSWindowController {
    static let width: CGFloat = 440
    private let band = TitleBand()
    private let host: NSHostingView<SettingsView>

    init(updates: UpdateController, model: EditorModel, removeAppData: @escaping () -> Void) {
        host = NSHostingView(rootView: SettingsView(updates: updates, model: model, removeAppData: removeAppData))
        host.sizingOptions = []
        host.safeAreaRegions = []
        host.identifier = InkKeyLoop.inspectorIdentifier
        let window = SettingsWindow(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: 480),
                                    styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: true)
        window.title = "Settings"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        // As in the editing window: an empty compact toolbar puts the
        // traffic lights in the middle of the 38 pt band.
        let toolbar = NSToolbar(identifier: "settings-title")
        toolbar.allowsUserCustomization = false
        if #available(macOS 15, *) { toolbar.allowsDisplayModeCustomization = false }
        window.toolbar = toolbar
        window.toolbarStyle = .unifiedCompact
        window.backgroundColor = Theme.NS.panel
        window.isReleasedWhenClosed = false
        window.isRestorable = false
        band.title = "Settings"
        let frame = EditorFrameView(band: band, content: host)
        frame.layer?.backgroundColor = Theme.NS.panel.cgColor
        frame.addSubview(host)
        frame.addSubview(band)
        window.contentView = frame
        super.init(window: window)
        host.rootView.resize = { [weak self] height in self?.fit(contentHeight: height) }
        window.center()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    /// Sizes the window to the page under the band, keeping its top edge
    /// where it is, as the system's Settings windows do between panes.
    private func fit(contentHeight: CGFloat) {
        guard let window else { return }
        let content = NSSize(width: Self.width, height: ceil(contentHeight) + Theme.Size.titleBar)
        var frame = window.frameRect(forContentRect: NSRect(origin: .zero, size: content))
        guard abs(frame.height - window.frame.height) > 0.5 else { return }
        frame.origin = NSPoint(x: window.frame.minX, y: window.frame.maxY - frame.height)
        window.setFrame(frame, display: true, animate: window.isVisible)
    }
}

/// Notes keyboard use for the focus box, and Tab from nothing focused
/// moves to the page's first control (⇧Tab its last), as in the inspector.
@MainActor final class SettingsWindow: NSWindow {
    /// Opens with nothing focused: AppKit would give the first tab the keys,
    /// and with them the focus box, before anyone pressed Tab.
    override func makeKeyAndOrderFront(_ sender: Any?) {
        let opening = !isVisible
        super.makeKeyAndOrderFront(sender)
        if opening { makeFirstResponder(nil) }
    }

    override func sendEvent(_ event: NSEvent) {
        InputMode.shared.note(event)
        if event.type == .keyDown, event.keyCode == 48, event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
           !(firstResponder is NSText), (firstResponder as? NSView).flatMap(InkKeyLoop.root(of:)) == nil,
           let root = (contentView as? EditorFrameView)?.content,
           InkKeyLoop.advance(in: root, from: nil, forward: !event.modifierFlags.contains(.shift)) { return }
        super.sendEvent(event)
    }
}

struct SettingsView: View {
    @ObservedObject var updates: UpdateController
    var model: EditorModel
    var removeAppData: () -> Void
    /// Called with the page's height, for the window to fit it.
    var resize: (CGFloat) -> Void = { _ in }
    @AppStorage("settingsTab") private var tab = SettingsTab.photographs
    @AppStorage(ViewerSheet.key) private var sheet = ViewerSheet.darkroom
    @State private var thumbnailBytes: Int?
    @State private var emptyingThumbnails = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 24) {
                ForEach(SettingsTab.allCases, id: \.self) { item in
                    TabWord(title: item.rawValue, chosen: tab == item) { tab = item }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Theme.Size.gutter)
            .padding(.top, 8)
            .frame(height: Theme.Size.tabRow, alignment: .top)
            .overlay(alignment: .bottom) { InkRule() }
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Settings tabs")
            switch tab {
            case .photographs: photographs
            case .speed: speed
            case .upkeep: upkeep
            }
        }
        .frame(width: SettingsWindowController.width, alignment: .top)
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGFloat.self, of: \.size.height) { resize($0) }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Theme.panel)
        .environment(\.colorScheme, .dark)
    }

    // MARK: Photographs: what a new photograph starts with, and what it lies on

    @ViewBuilder private var photographs: some View {
        let slide = model.films.first { $0.id == model.newPhotoFilm }?.positive ?? false
        InkSection(title: "New photographs") {
            ChoiceRow(title: "Film", items: Self.items(StockGroup.films(model.listedFilms(keeping: [model.newPhotoFilm]), papers: model.papers)),
                      selection: model.newPhotoFilm) { model.setNewPhotoFilm($0) }
                .inkDisabled(model.films.isEmpty)
            if slide {
                // A slide is scanned, without a paper: struck through, as under the photograph.
                StruckRow(title: "Paper", value: model.papers.first { $0.id == model.newPhotoPaper }?.name ?? model.newPhotoPaper)
                HintText("not printed — scanning the film")
            } else {
                ChoiceRow(title: "Paper", items: Self.items(model.paperGroups), selection: model.newPhotoPaper) { model.setNewPhotoPaper($0) }
                    .inkDisabled(model.papers.isEmpty)
            }
            HintText("A photo you haven’t edited opens with this film and paper, and its thumbnail shows them. Photos you’ve edited keep their own.")
            TickRow(title: "Match to camera brightness", isOn: Binding(
                get: { model.autoPlaceNewImages }, set: { model.autoPlaceNewImages = $0 }))
            HintText("New photographs start with Prep › Match camera brightness on: a RAW develops about as bright as the camera's own preview, a JPEG or TIFF as the file itself.")
        }
        InkSection(title: "Films") {
            ChoiceRow(title: "Fotufilm films", items: FotufilmFilms.allCases.map { .option($0.title, tag: $0.rawValue) },
                      selection: model.fotufilmFilms.rawValue) { model.fotufilmFilms = FotufilmFilms(rawValue: $0) ?? .all }
            HintText("Films ported from Fotufilm. From datasheets hides the eight it models from sample photographs rather than the manufacturer’s data. Photos already using a hidden film keep it.")
        }
        InkSection(title: "Changing film") {
            TickRow(title: "Keep paper when changing film", isOn: Binding(
                get: { model.previews.keepPaper }, set: { model.setKeepPaper($0) }))
            HintText("Leave the paper as it is when you choose another film, instead of switching to the paper made for that film.")
        }
        InkSection(title: "Edits") {
            TickRow(title: "Save ratings and edits next to photos", isOn: Binding(
                get: { model.sidecarsEnabled }, set: { model.sidecarsEnabled = $0 }))
            HintText(model.sidecarsEnabled
                     ? "Each photo remembers its own edits in a small file beside it, such as IMG_0412.CR3.negs, which travels with the folder. A photo you haven’t edited starts with the film and paper above, default Prep and Lab settings and no border; the Image Size and output colour stay."
                     : "Ratings and edits won’t be saved. Reopening a photo starts from the defaults.")
        }
        InkSection(title: "Viewer") {
            ChoiceRow(title: "Sheet", items: ViewerSheet.allCases.map { .option($0.title, tag: $0.rawValue) },
                      selection: sheet.rawValue) { sheet = ViewerSheet(rawValue: $0) ?? .darkroom }
            HintText("The colour behind the photograph. The caption and the label under it change their ink to suit; the panels stay dark.")
        }
    }

    // MARK: Speed

    @ViewBuilder private var speed: some View {
        InkSection(title: "Previews") {
            TickRow(title: "Film, paper and preset previews", isOn: Binding(
                get: { model.previews.enabled }, set: { model.setPreviews($0) }))
            HintText("Show each film, paper and preset on your photograph. Turn off to use simple menus and lists on slower computers.")
            TickRow(title: "Limit preview to 12 MP", isOn: Binding(
                get: { model.limitsPreview }, set: { model.limitsPreview = $0 }))
            HintText("Develops the preview faster on large photographs. Export keeps the full Image Size.")
        }
        InkSection(title: "Export") {
            TickRow(title: "Use less memory for export", isOn: Binding(
                get: { model.lowMemoryExport }, set: { model.lowMemoryExport = $0 }))
            HintText("Develops the grain one colour at a time. Same result, less memory, longer export.")
        }
    }

    // MARK: Upkeep: updates, the thumbnail cache, app data

    @ViewBuilder private var upkeep: some View {
        InkSection(title: "Updates") {
            if updates.configured {
                TickRow(title: "Automatically check for updates", isOn: Binding(
                    get: { updates.automaticChecks }, set: { updates.setAutomaticChecks($0) }))
                HintText("Updates are installed only when you choose to restart Negs.")
                ButtonRow {
                    if let lastCheck = updates.lastCheck {
                        FigureText(lead: "Last checked", figure: lastCheck.formatted(date: .abbreviated, time: .shortened))
                    }
                } button: {
                    Button("Check for Updates…", action: updates.check).buttonStyle(OutlinedButtonStyle())
                        .inkDisabled(!updates.canCheck)
                }
                if let error = updates.configurationError { HintText(error, warning: true) }
            } else {
                ReadoutRow(title: "Updates are not configured for this build.", value: "")
                HintText("Install a release build to receive updates.")
            }
        }
        InkSection(title: "Thumbnails") {
            ButtonRow {
                FigureText(figure: thumbnailBytes.map { ByteCountFormatter.string(fromByteCount: Int64($0), countStyle: .file) } ?? "…", trail: "of 1 GB")
            } button: {
                Button("Empty Thumbnail Cache") {
                    Task {
                        emptyingThumbnails = true
                        await model.emptyThumbnailCache()
                        thumbnailBytes = await model.thumbnailCacheSize()
                        emptyingThumbnails = false
                    }
                }
                .buttonStyle(OutlinedButtonStyle())
                .inkDisabled(emptyingThumbnails || thumbnailBytes == 0)
            }
            HintText("The strip and grid show each photograph as developed; the least recently shown are removed first. Emptied, the open folder’s are developed again in the background.")
        }
        .task { thumbnailBytes = await model.thumbnailCacheSize() }
        InkSection(title: "App data") {
            ButtonRow {
                EmptyView()
            } button: {
                Button("Remove All App Data and Quit…", role: .destructive, action: removeAppData).buttonStyle(OutlinedButtonStyle())
                    .inkDisabled(model.saving || model.exporting || updates.installing || updates.sessionInProgress)
            }
            if model.saving || model.exporting {
                HintText("Wait for the current save or export to finish before removing app data.", reason: true)
            } else if updates.sessionInProgress || updates.installing {
                HintText("Finish or close the update dialog before removing app data.", reason: true)
            }
            HintText("Removes settings, the saved editing session, and caches. Your original photographs and exported images are kept.")
        }
    }

    /// A stock menu's items, one section per group.
    static func items(_ groups: [StockGroup]) -> [ChoiceItem] {
        groups.flatMap { group in [ChoiceItem.header(group.title)] + group.stocks.map { .option($0.name, tag: $0.id) } }
    }
}

/// A row whose value can't be chosen now: struck through in pencil, in `dim`.
private struct StruckRow: View {
    let title: String
    let value: String

    var body: some View {
        HStack(spacing: 8) {
            RowLabel(title: title)
            Spacer(minLength: 8)
            Text(value).font(Theme.serif(14.5)).foregroundStyle(Theme.dim).lineLimit(1)
                .overlay { PencilStroke().pencil().frame(height: 5).offset(y: 1) }
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(height: Theme.Size.row)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityValue("\(value), not printed, scanning the film")
    }
}

/// Something to read at the row's start and an outlined button at its end.
private struct ButtonRow<Lead: View, Trailing: View>: View {
    @ViewBuilder let lead: Lead
    @ViewBuilder let button: Trailing

    var body: some View {
        HStack(spacing: 8) {
            lead
            Spacer(minLength: 8)
            button
        }
        .padding(.horizontal, Theme.Size.gutter)
        .frame(minHeight: Theme.Size.row)
    }
}

/// A figure in the serif, with words before or after it in `dim` italic:
/// "*Last checked* 10 Nov 2026, 21:14", "4,9 MB *of 1 GB*".
private struct FigureText: View {
    var lead: String? = nil
    let figure: String
    var trail: String? = nil

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 5) {
            if let lead { Text(lead).font(Theme.serif(14.5, italic: true)).foregroundStyle(Theme.dim) }
            Text(figure).font(Theme.serif(14.5)).foregroundStyle(Theme.paper)
            if let trail { Text(trail).font(Theme.serif(14.5, italic: true)).foregroundStyle(Theme.dim) }
        }
        .lineLimit(1)
        .accessibilityElement(children: .combine)
    }
}
