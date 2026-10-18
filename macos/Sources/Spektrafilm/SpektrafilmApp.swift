import AppKit
import SwiftUI

@MainActor final class ApplicationDelegate: NSObject, NSApplicationDelegate {
    let model = EditorModel()

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Start the engine while AppKit builds the window.
        Task { await model.start() }
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        // Finder opens arrive through onOpenURL; a path argument is for `swift run`.
        if let path = CommandLine.arguments.dropFirst().first(where: { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }) {
            model.open(URL(fileURLWithPath: path))
        }
        #if SPEKTRAFILM_BENCHMARK
        InteractionBenchmark.start(model)
        #endif
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard model.exporting else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = "An export is still in progress."
        alert.informativeText = "Quitting cancels it. Any existing file at the destination is left unchanged."
        alert.addButton(withTitle: "Continue Exporting")
        alert.addButton(withTitle: "Quit")
        return alert.runModal() == .alertSecondButtonReturn ? .terminateNow : .terminateCancel
    }
    func applicationWillTerminate(_ notification: Notification) { model.shutdown() }
}

@main struct SpektrafilmApp: App {
    @NSApplicationDelegateAdaptor(ApplicationDelegate.self) private var delegate

    var body: some Scene {
        let model = delegate.model
        Window("Spektrafilm", id: "editor") {
            EditorWindow(model: model)
        }
        .defaultSize(width: 1320, height: 860)
        .windowToolbarStyle(.unified)
        .commands { EditorCommands(model: model) }
    }
}

struct EditorCommands: Commands {
    let model: EditorModel

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("Open Photograph…", action: model.openPanel).keyboardShortcut("o")
            Menu("Open Recent") {
                ForEach(model.recentFiles, id: \.self) { url in
                    Button(url.lastPathComponent) { model.open(url) }
                }
            }.disabled(model.recentFiles.isEmpty)
        }
        CommandGroup(replacing: .saveItem) {
            Button("Save Preview…") { model.savePanel(export: false) }.keyboardShortcut("s").disabled(!model.canSave)
            Button("Export Photograph…") { model.savePanel(export: true) }.keyboardShortcut("e", modifiers: [.command, .shift]).disabled(model.source == nil || !model.ready || model.exporting)
        }
        CommandGroup(after: .toolbar) {
            Toggle("Show Adjustments", isOn: Bindable(model).inspectorVisible).keyboardShortcut("i", modifiers: [.command, .option])
            Divider()
            Button("Fit Photograph") { model.zoom(fit: true) }.keyboardShortcut("0").disabled(model.source == nil)
            Button("Actual Size") { model.zoom(fit: false) }.keyboardShortcut("1").disabled(model.source == nil)
            Button("Rotate Clockwise", action: model.rotate).keyboardShortcut("r").disabled(model.source == nil)
            Divider()
            Button("Reset All Adjustments", action: model.reset).keyboardShortcut("r", modifiers: [.command, .shift]).disabled(!model.ready)
        }
    }
}

struct EditorWindow: View {
    @Bindable var model: EditorModel
    @Environment(\.undoManager) private var undoManager
    @State private var dropTargeted = false

    var body: some View {
        VStack(spacing: 0) {
            CanvasArea(model: model)
                .dropDestination(for: URL.self) { urls, _ in
                    guard let url = urls.first, url.isFileURL else { return false }
                    model.open(url); return true
                } isTargeted: { dropTargeted = $0 }
                .overlay {
                    if dropTargeted { RoundedRectangle(cornerRadius: 12).strokeBorder(.tint, lineWidth: 3).padding(8).allowsHitTesting(false) }
                }
            if model.source != nil || model.exporting {
                Divider()
                StatusBar(model: model)
            }
        }
        // Fully specified frames answer AppKit's window-size questions without
        // measuring the content, which would lay out every inspector row per edit.
        .frame(minWidth: 680, idealWidth: 980, maxWidth: .infinity, minHeight: 500, idealHeight: 860, maxHeight: .infinity)
        .inspector(isPresented: $model.inspectorVisible) {
            AdjustmentInspector(model: model)
                .frame(minWidth: 0, idealWidth: 340, maxWidth: .infinity, minHeight: 0, idealHeight: 860, maxHeight: .infinity)
                .inspectorColumnWidth(min: 300, ideal: 340, max: 440)
        }
        .navigationTitle(model.source?.lastPathComponent ?? "Spektrafilm")
        .navigationSubtitle(model.source == nil || !model.ready ? "" : model.value("io.scan_film").bool ? model.filmName : "\(model.filmName) → \(model.paperName)")
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button(action: model.openPanel) { Label("Open", systemImage: "folder") }.help("Open photograph (⌘O)")
            }
            ToolbarItemGroup(placement: .principal) {
                ControlGroup {
                    Button { model.zoom(fit: true) } label: { Label("Fit", systemImage: "arrow.down.right.and.arrow.up.left") }
                        .help("Fit photograph (⌘0)")
                    Button { model.zoom(fit: false) } label: { Label("Actual Size", systemImage: "1.magnifyingglass") }
                        .help("Actual size (⌘1)")
                } label: { Label("Zoom", systemImage: "magnifyingglass") }
                .disabled(model.source == nil)
                Button(action: model.rotate) { Label("Rotate", systemImage: "rotate.right") }
                    .help("Rotate clockwise (⌘R)").disabled(model.source == nil)
            }
            ToolbarItemGroup(placement: .primaryAction) {
                Button { model.savePanel(export: true) } label: { Label("Export", systemImage: "square.and.arrow.down") }
                    .help("Export photograph (⇧⌘E)").disabled(model.source == nil || !model.ready || model.exporting)
                Button { model.inspectorVisible.toggle() } label: { Label("Adjustments", systemImage: "slider.horizontal.3") }
                    .help("Show or hide adjustments (⌥⌘I)")
            }
        }
        .onAppear { model.undoManager = undoManager }
        .onChange(of: undoManager) { _, manager in model.undoManager = manager }
        .onOpenURL { url in model.open(url) }
        .alert("Spektrafilm", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            Button("OK", role: .cancel) { model.error = nil }
        } message: { Text(model.error ?? "") }
    }
}

/// Reads only the preview state, so edits don't re-evaluate the canvas.
struct CanvasArea: View {
    let model: EditorModel

    var body: some View {
        ZStack {
            Color(nsColor: .underPageBackgroundColor)
            if let image = model.image {
                canvas(image, size: model.imageSize, rotation: model.imageRotation)
            } else if let placeholder = model.placeholder {
                canvas(placeholder, size: model.placeholderSize, rotation: 0)
            } else if model.source == nil {
                EmptyState(model: model)
            }
        }
        .overlay(alignment: .top) { ActivityBadge(model: model) }
    }

    private func canvas(_ image: AnyObject, size: CGSize, rotation: Int) -> some View {
        PreviewCanvas(image: image, size: size, rotation: rotation, border: model.border.shown(for: size), filmTexture: model.filmTexture,
                      filmStrip: model.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage)
    }
}

struct ActivityBadge: View {
    let model: EditorModel

    private var title: String? {
        guard model.source != nil, !model.renderFailed else { return nil }
        if model.image == nil { return "Developing" }
        return model.showsActivity ? "Updating" : nil
    }

    var body: some View {
        ZStack {
            if let title {
                Label(title, systemImage: "camera.aperture")
                    .symbolEffect(.pulse, options: .repeating)
                    .font(.callout.weight(.medium))
                    .padding(.horizontal, 12).padding(.vertical, 6)
                    .background(.regularMaterial, in: Capsule())
                    .overlay(Capsule().strokeBorder(.separator))
                    .padding(.top, 12)
                    .transition(.opacity.combined(with: .scale(scale: 0.96)))
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
        .animation(.easeInOut(duration: 0.18), value: title)
        .allowsHitTesting(false)
    }
}

struct EmptyState: View {
    let model: EditorModel

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "photo.on.rectangle.angled").font(.system(size: 54, weight: .light)).foregroundStyle(.secondary)
            Text("Open a Photograph").font(.title2.weight(.semibold))
            Text("Explore film stocks, shape the print, and make it your own.").foregroundStyle(.secondary)
            Button("Open Photograph…", action: model.openPanel).buttonStyle(.borderedProminent).controlSize(.large).padding(.top, 4)
            Text("Or drop a RAW, TIFF, PNG, or JPEG file here").font(.caption).foregroundStyle(.tertiary)
            if !model.recentFiles.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Recent").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    ForEach(model.recentFiles.prefix(5), id: \.self) { url in
                        Button { model.open(url) } label: { Label(url.lastPathComponent, systemImage: "photo") }
                            .buttonStyle(.link).help(url.path)
                    }
                }.padding(.top, 18)
            }
        }
        .multilineTextAlignment(.center)
        .padding(30)
    }
}

struct StatusBar: View {
    let model: EditorModel

    var body: some View {
        HStack(spacing: 8) {
            Text(model.status).lineLimit(1).textSelection(.enabled)
            Spacer(minLength: 12)
            if model.exporting {
                ProgressView().controlSize(.mini)
                TimelineView(.periodic(from: model.exportStarted, by: 1)) { context in
                    Text("Exporting · \(Int(context.date.timeIntervalSince(model.exportStarted))) s").monospacedDigit()
                }
                Button("Cancel", action: model.cancelExport).controlSize(.small)
            }
        }
        .font(.caption).foregroundStyle(.secondary)
        .padding(.horizontal, 14).frame(height: 28)
        .background(.bar)
        .accessibilityElement(children: .contain)
    }
}
