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
        // catalog fills in the inspector's first frame. Session I/O overlaps
        // the catalog request, never window layout. Benchmarks and explicit
        // launches must not reopen a previous photograph.
        model.launch()
        let benchmark = ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] != nil
        var restoration: Task<EditorSession?, Error>? = benchmark ? nil : Task { try await sessions.load() }
        #if SPEKTRAFILM_BENCHMARK
        if let path = ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_SESSION"] {
            let store = SessionStore(file: URL(fileURLWithPath: path))
            restoration = Task { try await store.load() }
        }
        #endif
        Task { await model.start(restoration: restoration) }
        menu = MainMenu(model: model, updates: updates) { [weak self] in self?.removeAllAppData() }
        NSApp.mainMenu = menu?.menu
        editor = EditorWindowController(model: model)
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        editor?.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
        // Finder opens arrive through application(_:open:); a path argument is for `swift run`.
        if let path = CommandLine.arguments.dropFirst().first(where: { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }) {
            model.open(URL(fileURLWithPath: path))
        }
        #if SPEKTRAFILM_BENCHMARK
        InteractionBenchmark.start(model)
        #endif
        updates.schedule(afterStartupOf: model)
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
        if model.exporting {
            let alert = NSAlert()
            alert.messageText = "An export is still in progress."
            alert.informativeText = "Quitting cancels it. Any existing file at the destination is left unchanged."
            alert.addButton(withTitle: "Continue Exporting")
            alert.addButton(withTitle: "Quit")
            guard alert.runModal() == .alertSecondButtonReturn else { return .terminateCancel }
        }
        // Save once at quit/relaunch, not on slider changes or the rendering path.
        // A failed write cancels termination instead of silently losing the edit.
        guard let session = model.session,
              ProcessInfo.processInfo.environment["SPEKTRAFILM_BENCHMARK_PHOTO"] == nil else { return .terminateNow }
        terminating = true
        Task {
            do {
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
        guard !removingAppData, !terminating, !model.saving, !model.exporting, !updates.installing, !updates.sessionInProgress else { return }
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
        VStack(spacing: 0) {
            CanvasArea(model: model)
                .dropDestination(for: URL.self) { urls, _ in
                    guard let url = urls.first, url.isFileURL else { return false }
                    model.open(url); return true
                } isTargeted: { dropTargeted = $0 }
                .overlay {
                    if dropTargeted { RoundedRectangle(cornerRadius: 12).strokeBorder(.tint, lineWidth: 3).padding(8).allowsHitTesting(false) }
                }
            if model.filmstripVisible, let source = model.source {
                Divider()
                PhotoBrowser(model: model, directory: source.deletingLastPathComponent())
            }
            if model.source != nil || model.exporting || (!model.status.isEmpty && model.status != "Starting…") {
                Divider()
                StatusBar(model: model)
            }
        }
        .alert("Negs", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            Button("OK", role: .cancel) { model.error = nil }
        } message: { Text(model.error ?? "") }
    }
}

/// The inspector column: the adjustments, or the crop panel while cropping.
struct InspectorPanel: View {
    let model: EditorModel
    /// False until the window is on screen; see EditorWindowController.
    var built = true

    var body: some View {
        if !built { Color.clear }
        else if let session = model.cropSession { CropInspector(model: model, session: session) }
        else if model.inspectorPane == .presets { PresetInspector(model: model) }
        else { AdjustmentInspector(model: model) }
    }
}

/// Reads only the preview state, so edits don't re-evaluate the canvas.
struct CanvasArea: View {
    let model: EditorModel

    var body: some View {
        ZStack {
            Color(nsColor: .underPageBackgroundColor)
            if let session = model.cropSession, let image = model.image {
                CropEditor(session: session, image: image)
            } else if let image = model.image {
                canvas(model.original ?? image, size: model.imageSize, rotation: model.imageRotation)
            } else if let placeholder = model.placeholder {
                canvas(placeholder, size: model.placeholderSize, rotation: model.imageRotation)
            } else if model.source == nil {
                EmptyState(model: model)
            }
        }
        .overlay(alignment: .top) { ActivityBadge(model: model) }
    }

    private func canvas(_ image: AnyObject, size: CGSize, rotation: Int) -> some View {
        PreviewCanvas(image: image, size: size, rotation: rotation, crop: model.crop, border: model.border.shown(for: model.crop.size(of: size)), filmTexture: model.filmTexture,
                      filmStrip: model.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage, scale: model.canvasScale)
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
            if model.source != nil { OriginalButton(model: model) }
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
