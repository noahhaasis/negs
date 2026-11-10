import AppKit
import SwiftUI

/// AppKit runs the application, menus, window, and toolbar; the canvas and
/// inspector inside the window are SwiftUI. A SwiftUI scene with the same
/// toolbar and inspector took about twice as long to show the window
/// (docs/native-macos-ui.md, Startup).
@main enum NegsMain {
    static func main() {
        MainActor.assumeIsolated {
            let app = NSApplication.shared
            let delegate = ApplicationDelegate()
            app.delegate = delegate
            withExtendedLifetime(delegate) { app.run() }
        }
    }
}

@MainActor final class ApplicationDelegate: NSObject, NSApplicationDelegate {
    let model = EditorModel()
    let updates = UpdateController()
    private let sessions = SessionStore()
    private var menu: MainMenu?
    private var editor: EditorWindowController?
    private var terminating = false
    private var removingAppData = false
    private var appDataRemovalFinished = false

    func applicationWillFinishLaunching(_ notification: Notification) {
        // The helper starts while AppKit builds the window, and the cached
        // catalog fills in the inspector's first frame. With that catalog the
        // session restores before the window is built, so its saved frame and
        // adjustments are in the window's first frames; the main actor's tasks
        // only run after AppKit's launch passes. Without a cached catalog, or
        // if the session isn't read within 0.1 s, it restores once the helper
        // answers and reports any error then. Benchmarks and explicit
        // launches must not reopen a previous photograph.
        #if SPEKTRAFILM_BENCHMARK
        LaunchBenchmark.begin(model)
        #endif
        // Newsreader is read on another thread while AppKit starts.
        Theme.Fonts.prepare()
        model.launch()
        let benchmark = ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] != nil
        var store: SessionStore? = benchmark ? nil : sessions
        #if SPEKTRAFILM_BENCHMARK
        if let path = ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_SESSION"] {
            store = SessionStore(file: URL(fileURLWithPath: path))
        }
        #endif
        var restoration: Task<EditorSession?, Error>?
        // The last photograph's sidecar is read with the session, a few kilobytes more.
        let sidecars = model.sidecarLookup
        if let store, Self.pathArgument != nil || !(Self.readSession(store, sidecars: sidecars, timeout: 0.1).map(model.restoreAtLaunch) ?? false) {
            restoration = Task { try await store.load(sidecars: sidecars) }
        }
        Task { await model.start(restoration: restoration) }
        menu = MainMenu(model: model, updates: updates) { [weak self] in self?.removeAllAppData() }
        NSApp.mainMenu = menu?.menu
        editor = EditorWindowController(model: model)
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        // The saved frame has usually been read while AppKit built the
        // window; waiting briefly puts it in the window's first frame. A
        // Finder open may have undone the restore that titled the window.
        model.finishLaunching(waitingUpTo: 0.15)
        editor?.refresh()
        editor?.showWindow(nil)
        #if SPEKTRAFILM_BENCHMARK
        LaunchBenchmark.windowShown()
        #endif
        NSApp.activate(ignoringOtherApps: true)
        // Finder opens arrive through application(_:open:); a path argument is for `swift run`.
        if let path = Self.pathArgument { model.open(URL(fileURLWithPath: path)) }
        #if SPEKTRAFILM_BENCHMARK
        InteractionBenchmark.start(model)
        GridBenchmark.start(model)
        #endif
        updates.schedule(afterStartupOf: model)
        offerBatchResume()
    }

    /// An export the last session didn't finish is offered once launch has
    /// settled, as updates are: never on the launch path.
    private func offerBatchResume() {
        guard ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] == nil else { return }
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(3))
            while let self, !Task.isCancelled {
                let model = self.model
                if model.ready && (model.source == nil || model.previewCurrent || model.renderFailed) || model.error != nil {
                    return model.offerBatchResume(in: self.editor?.window)
                }
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }
    /// The session, read on another thread and waited for at most `timeout`,
    /// so a photograph on an unresponsive volume never holds up launch. Nil
    /// if there's none, it can't be read, or it takes longer.
    private static func readSession(_ store: SessionStore, sidecars: SidecarLookup?, timeout: TimeInterval) -> EditorSession? {
        final class Result: @unchecked Sendable { var session: EditorSession? }
        let result = Result(), done = DispatchSemaphore(value: 0)
        DispatchQueue.global(qos: .userInitiated).async {
            result.session = try? store.read(sidecars: sidecars)
            done.signal()
        }
        // The semaphore orders the write before this read.
        return done.wait(timeout: .now() + timeout) == .success ? result.session : nil
    }
    private static var pathArgument: String? {
        CommandLine.arguments.dropFirst().first { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }
    }
    func application(_ application: NSApplication, open urls: [URL]) {
        if let url = urls.first(where: \.isFileURL) { model.open(url) }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // Cleanup intentionally discards the session. Never recreate it here.
        if removingAppData { return appDataRemovalFinished ? .terminateNow : .terminateCancel }
        guard !terminating else { return .terminateLater }
        if model.saving || (updates.installing && model.exporting) { return .terminateCancel }
        if model.batchRunning, let remaining = model.batch?.journal.remaining, !updates.installing {
            // Kept in the journal: the next launch offers to resume.
            let alert = NSAlert()
            alert.messageText = "Export \(remaining == 1 ? "the remaining photograph" : "\(remaining) remaining photographs") next time?"
            alert.informativeText = "Quitting stops the export. Files already exported stay; Negs offers to export the rest when it opens again."
            alert.addButton(withTitle: "Quit")
            alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        }
        if model.exporting {
            let alert = NSAlert()
            alert.messageText = "An export is still in progress."
            alert.informativeText = "Quitting cancels it. Any existing file at the destination is left unchanged."
            alert.addButton(withTitle: "Continue Exporting")
            alert.addButton(withTitle: "Quit")
            guard alert.runModal() == .alertSecondButtonReturn else { return .terminateCancel }
        }
        // Save once at quit/relaunch, not on slider changes or the rendering path.
        // A failed write cancels termination instead of silently losing the
        // session. The photograph's edits go to its sidecar first; one that
        // can't be written was already reported and doesn't keep Negs open.
        guard let session = model.session,
              ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] == nil else { return .terminateNow }
        terminating = true
        Task {
            do {
                await model.flushSidecars()
                try await sessions.save(session)
                await model.flushPreviewCache()
                sender.reply(toApplicationShouldTerminate: true)
            } catch {
                terminating = false
                sender.reply(toApplicationShouldTerminate: false)
                model.error = "Could not save the editing session. Negs stayed open. \(error.localizedDescription)"
            }
        }
        return .terminateLater
    }

    private func removeAllAppData() {
        guard !removingAppData, !terminating, !model.saving, !model.exporting, !model.batchRunning, !updates.installing, !updates.sessionInProgress else { return }
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Remove all Negs app data and quit?"
        alert.informativeText = "This removes your settings, presets, saved editing session, recent-file history, and caches. Your current adjustments will be discarded. Original photographs and exported images are kept.\n\nNegs remains installed. You can move it to the Trash after it quits."
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Remove App Data and Quit")
        guard alert.runModal() == .alertSecondButtonReturn else { return }
        removingAppData = true
        updates.stopForAppDataRemoval()
        editor?.prepareForAppDataRemoval()
        menu?.prepareForAppDataRemoval()
        model.shutdown()
        Task {
            await model.flushPreviewCache()
            // Wait for an in-flight startup load before deleting session files.
            await sessions.finishPendingIO()
            let locations = AppDataLocations.current
            while true {
                NSDocumentController.shared.clearRecentDocuments(nil)
                var failures = await Task.detached(priority: .userInitiated) { AppDataCleaner.removeFiles(at: locations) }.value
                if !AppDataCleaner.removePreferences(domain: locations.preferencesDomain) {
                    failures.append("The saved preferences could not be cleared.")
                }
                if failures.isEmpty { break }
                let failed = NSAlert()
                failed.alertStyle = .warning
                failed.messageText = "Some app data could not be removed."
                failed.informativeText = failures.joined(separator: "\n\n")
                failed.addButton(withTitle: "Retry")
                failed.addButton(withTitle: "Quit")
                if failed.runModal() != .alertFirstButtonReturn { break }
            }
            appDataRemovalFinished = true
            NSApp.terminate(nil)
        }
    }
    func applicationWillTerminate(_ notification: Notification) { updates.stop(); model.shutdown() }
}

/// The window's main column: the photograph, the drop target, and the status bar.
struct EditorCanvas: View {
    let model: EditorModel
    @State private var dropTargeted = false

    var body: some View {
        let grid = model.mode == .grid
        VStack(spacing: 0) {
            ZStack {
                // The canvas stays under the grid, so coming back to the
                // same photograph is instant and keeps its zoom.
                CanvasArea(model: model).accessibilityHidden(grid)
                if grid { LibraryGrid(model: model) }
            }
                .dropDestination(for: URL.self) { urls, _ in
                    guard let url = urls.first, url.isFileURL else { return false }
                    model.open(url); return true
                } isTargeted: { dropTargeted = $0 }
                .overlay {
                    if dropTargeted { RoundedRectangle(cornerRadius: 12).strokeBorder(.tint, lineWidth: 3).padding(8).allowsHitTesting(false) }
                }
                .overlay { LibraryNoteView(model: model) }
            // The grid takes the strip's place; it comes back as it was.
            if model.filmstripVisible, model.source != nil, !grid {
                Divider()
                PhotoBrowser(model: model)
            }
            if model.source != nil || model.exporting || (!model.status.isEmpty && model.status != "Starting…") {
                Divider()
                StatusBar(model: model)
            }
        }
        .alert("Negs", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            Button("OK", role: .cancel) { model.error = nil }
        } message: { Text(model.error ?? "") }
        // The open photograph's folder is the library, listed once the
        // window is up and kept current while it stays open.
        .task(id: model.source?.deletingLastPathComponent()) {
            guard let folder = model.source?.deletingLastPathComponent() else { return }
            await model.library.watch(folder)
        }
    }
}

/// What a rating or flag key did ("★★★", "Rejected"), briefly over the
/// canvas. There is no permanent rating widget, so the canvas stays clean.
struct LibraryNoteView: View {
    let model: EditorModel

    var body: some View {
        ZStack {
            if let note = model.libraryNote {
                VStack(spacing: 4) {
                    Text(note.text).font(.title2.weight(.semibold))
                    if note.count > 1 { Text("\(note.count) photographs").font(.callout) }
                    if let detail = note.detail { Text(detail).font(.callout).foregroundStyle(.secondary) }
                }
                .padding(.horizontal, 22)
                .padding(.vertical, 14)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
                .transition(.opacity)
                .task(id: note.serial) {
                    try? await Task.sleep(for: .seconds(note.detail == nil ? 1 : 2.5))
                    guard !Task.isCancelled else { return }
                    withAnimation(.easeOut(duration: 0.25)) { model.hideLibraryNote(note.serial) }
                }
                .accessibilityElement(children: .combine)
            }
        }
        .allowsHitTesting(false)
    }
}

/// The inspector column: the adjustments, or the crop panel while cropping.
struct InspectorPanel: View {
    let model: EditorModel
    /// False until the window is on screen; see EditorWindowController.
    var built = true

    var body: some View {
        ZStack(alignment: .top) {
            Theme.panel
            if !built { Color.clear }
            else if let session = model.cropSession { CropInspector(model: model, session: session) }
            else {
                VStack(spacing: 0) {
                    PaneHeader(model: model)
                    if model.inspectorPane == .presets { PresetInspector(model: model) }
                    // The sections aren't built until a photograph opens.
                    else if model.source == nil { InspectorEmptyLine() }
                    else { AdjustmentInspector(model: model) }
                }
            }
        }
        .tint(Theme.paper)
    }
}

/// The panel with no photograph: one line instead of its tabs (§4.14).
struct InspectorEmptyLine: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            InkRule()
            Text("The film, the paper and every setting appear here once a photograph is open.")
                .font(Theme.serif(14, italic: true)).foregroundStyle(Theme.dim)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, Theme.Size.gutter)
                .padding(.top, 18)
            Spacer(minLength: 0)
        }
    }
}

/// Reads only the preview state, so edits don't re-evaluate the canvas.
struct CanvasArea: View {
    let model: EditorModel
    /// Settings › Custom viewer background.
    @AppStorage(ViewerBackground.customKey) private var customBackground = false
    @AppStorage(ViewerBackground.colorKey) private var backgroundHex = ViewerBackground.defaultHex

    var body: some View {
        ZStack {
            ViewerBackground.canvas(custom: customBackground, hex: backgroundHex)
            if let session = model.cropSession, let image = model.image {
                CropEditor(session: session, image: image)
            } else if let image = model.image {
                canvas(model.original ?? image, size: model.imageSize, rotation: model.imageRotation)
            } else if let standIn = model.standIn {
                // Finished already: drawn as it is, the size the photograph will be.
                PreviewCanvas(image: standIn, size: Self.standInSize(standIn, photo: model.placeholderSize), rotation: 0, crop: CropSettings(),
                              border: nil, filmTexture: nil, filmStrip: nil, zoomCommand: model.zoomCommand, fit: model.fitImage,
                              scale: model.canvasScale, doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) })
            } else if let placeholder = model.placeholder {
                canvas(placeholder, size: model.placeholderSize, rotation: model.imageRotation)
            } else if model.source == nil {
                EmptyState(model: model)
            }
        }
        .overlay(alignment: .top) { ActivityBadge(model: model) }
    }

    /// The stand-in's layout size: its own shape (fitted, only that shows),
    /// with the long side of the photograph's placeholder once that's read,
    /// else 2048, as a RAW's.
    static func standInSize(_ image: CGImage, photo: CGSize) -> CGSize {
        let long = max(photo.width, photo.height) > 0 ? max(photo.width, photo.height) : 2048
        let scale = long / CGFloat(max(image.width, image.height, 1))
        return CGSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
    }

    private func canvas(_ image: AnyObject, size: CGSize, rotation: Int) -> some View {
        PreviewCanvas(image: image, size: size, rotation: rotation, crop: model.crop, border: model.border.shown(for: model.crop.size(of: size)), filmTexture: model.filmTexture,
                      filmStrip: model.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage, scale: model.canvasScale,
                      doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) })
    }
}

struct ActivityBadge: View {
    let model: EditorModel

    private var title: String? {
        if model.original != nil { return "Original" }
        if model.holdingOriginal { return "Loading Original" }
        guard model.source != nil, !model.renderFailed else { return nil }
        if model.image == nil { return "Developing" }
        return model.showsActivity ? "Updating" : nil
    }

    var body: some View {
        ZStack {
            if let title {
                let shown = model.original != nil
                Label(title, systemImage: model.holdingOriginal ? "square.split.diagonal.fill" : "camera.aperture")
                    .symbolEffect(.pulse, options: .repeating, isActive: !shown)
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
            if model.source != nil, model.mode == .photo { OriginalButton(model: model) }
        }
        .font(.caption).foregroundStyle(.secondary)
        .padding(.horizontal, 14).frame(height: 28)
        .background(.bar)
        .accessibilityElement(children: .contain)
    }
}

/// Press and hold to compare with the photograph before developing, like
/// Lightroom's Show Original. The decode starts when the pointer arrives.
struct OriginalButton: View {
    let model: EditorModel
    @State private var pressed = false

    var body: some View {
        let enabled = model.canShowOriginal
        Image(systemName: "square.split.diagonal.fill")
            .font(.system(size: 13))
            .foregroundStyle(model.original != nil ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
            .frame(width: 24, height: 20)
            .contentShape(Rectangle())
            .opacity(enabled ? 1 : 0.4)
            .onHover { if $0 { model.prepareOriginal() } }
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { _ in if !pressed { pressed = true; model.holdOriginal(true) } }
                .onEnded { _ in pressed = false; model.holdOriginal(false) })
            .allowsHitTesting(enabled)
            .help("Hold to show the original (\\)")
            .accessibilityElement()
            .accessibilityLabel("Show Original")
            .accessibilityAddTraits(.isButton)
            .accessibilityValue(model.original != nil ? "On" : "Off")
            // VoiceOver can't hold, so activating it toggles.
            .accessibilityAction { model.holdOriginal(!model.holdingOriginal) }
    }
}
