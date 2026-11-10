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
    /// The quit questions are open in the alert sheet.
    private var askingToQuit = false
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
                    return model.offerBatchResume()
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
        if model.saving || (updates.installing && model.exporting) || askingToQuit { return .terminateCancel }
        // A question already open is answered first.
        if model.alerts.shown != nil { NSSound.beep(); return .terminateCancel }
        let questions = quitQuestions()
        guard !questions.isEmpty else { return saveAndTerminate(sender) }
        askingToQuit = true
        Task {
            for (alert, quit) in questions where await model.ask(alert).button != quit {
                askingToQuit = false
                return sender.reply(toApplicationShouldTerminate: false)
            }
            askingToQuit = false
            switch saveAndTerminate(sender) {
            case .terminateNow: sender.reply(toApplicationShouldTerminate: true)
            case .terminateCancel: sender.reply(toApplicationShouldTerminate: false)
            default: break
            }
        }
        return .terminateLater
    }

    /// What quitting now would stop, as questions in the alert sheet, each
    /// with the button that quits.
    private func quitQuestions() -> [(AppAlert, quit: Int)] {
        var questions: [(AppAlert, quit: Int)] = []
        if model.batchRunning, let remaining = model.batch?.journal.remaining, !updates.installing {
            // Kept in the journal: the next launch offers to resume.
            questions.append((AppAlert(
                title: "Export \(remaining == 1 ? "the remaining photograph" : "\(remaining) remaining photographs") next time?",
                detail: "Quitting stops the export. Files already exported stay; Negs offers to export the rest when it opens again.",
                buttons: [.init(title: "Quit"), .init(title: "Cancel", role: .cancel)]), 0))
        }
        if model.exporting {
            questions.append((AppAlert(
                title: "An export is still in progress.",
                detail: "Quitting cancels it. Any existing file at the destination is left unchanged.",
                buttons: [.init(title: "Continue Exporting", role: .cancel), .init(title: "Quit")]), 1))
        }
        return questions
    }

    /// Save once at quit/relaunch, not on slider changes or the rendering path.
    /// A failed write cancels termination instead of silently losing the
    /// session. The photograph's edits go to its sidecar first; one that
    /// can't be written was already reported and doesn't keep Negs open.
    private func saveAndTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
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
        Task {
            let answer = await model.ask(AppAlert(
                title: "Remove all Negs app data and quit?",
                detail: "This removes your settings, presets, saved editing session, recent-file history, and caches. Your current adjustments will be discarded. Original photographs and exported images are kept.\n\nNegs remains installed. You can move it to the Trash after it quits.",
                buttons: [.init(title: "Cancel", role: .cancel), .init(title: "Remove App Data and Quit", role: .destructive)]))
            guard answer.button == 1 else { return }
            // Anything started while it was asking keeps the data.
            guard !removingAppData, !terminating, !model.saving, !model.exporting, !model.batchRunning, !updates.installing, !updates.sessionInProgress else { return }
            removingAppData = true
            updates.stopForAppDataRemoval()
            editor?.prepareForAppDataRemoval()
            menu?.prepareForAppDataRemoval()
            model.shutdown()
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
                // The window is gone, so the sheet stands on its own.
                let retry = await model.ask(AppAlert(title: "Some app data could not be removed.", detail: failures.joined(separator: "\n\n"),
                                                     buttons: [.init(title: "Retry"), .init(title: "Quit", role: .cancel)]))
                if retry.button != 0 { break }
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
            // The grid's sheet covers it (`GridSheet`), so coming back to
            // the same photograph is instant and keeps its zoom.
            CanvasArea(model: model, dropping: dropTargeted)
            // The tray takes the strip's and the status line's place (§4.5).
            if let tray = model.tray, !grid {
                StockTray(model: model, visit: tray)
            } else if !dropTargeted {
                VStack(spacing: 0) {
                    // It stays under the grid's sheet, which folds back into it.
                    if model.filmstripVisible, model.source != nil {
                        PhotoBrowser(model: model)
                    }
                    // None in the empty window (§4.14).
                    if model.source != nil || model.exporting || model.batch != nil {
                        // No rule: strip and status line are 186 pt, the
                        // tray's height, so the photograph doesn't move (§2.3).
                        StatusBar(model: model)
                    }
                }
                // The leader rises from the strip's right end, or from the
                // status line's top edge with the strip hidden (§11).
                .overlay(alignment: .topTrailing) {
                    if model.source != nil, !grid, !model.cropping {
                        StripLeader(model: model).alignmentGuide(.top) { $0[.bottom] }
                    }
                }
            }
        }
        // A photograph or a folder dropped anywhere on the column opens.
        .dropDestination(for: URL.self) { urls, _ in
            guard let url = urls.first, url.isFileURL else { return false }
            model.open(url); return true
        } isTargeted: { dropTargeted = $0 }
        // Under the grid's sheet, hidden from VoiceOver too.
        .accessibilityHidden(grid)
        .modifier(LibraryNoteTimer(model: model))
        // The open photograph's folder is the library, listed once the
        // window is up and kept current while it stays open.
        .task(id: model.source?.deletingLastPathComponent()) {
            guard let folder = model.source?.deletingLastPathComponent() else { return }
            await model.library.watch(folder)
        }
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
        // Under the grid's sheet (§4.6).
        .accessibilityHidden(model.mode == .grid)
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
    /// While a file is dragged over the window, the photograph, caption and
    /// label step aside for a line round the print's place (§4.14).
    var dropping = false
    @State private var geometry = CanvasGeometry()
    /// The pointer is over the photograph, which brightens the trim marks.
    @State private var overPrint = false

    /// Above the photograph, for the caption; below it, for the label's two rows (§2.3).
    static let captionBand: CGFloat = 54
    static let labelBand = PrintLabelLayout.band

    var body: some View {
        ZStack {
            ViewerBackground.canvas
            if let session = model.cropSession, let image = model.image {
                CropEditor(model: model, session: session, image: image)
            } else if model.source == nil, model.image == nil, model.standIn == nil, model.placeholder == nil {
                EmptyCanvas(model: model, dropping: dropping)
            } else {
                let failure = model.developFailure
                VStack(spacing: 0) {
                    Color.clear.frame(height: Self.captionBand)
                    // Couldn't develop: the last develop, faded and greyed.
                    photograph
                        .grayscale(failure == nil ? 0 : 1)
                        .opacity(dropping ? 0 : failure == nil ? 1 : 0.12)
                    Color.clear.frame(height: Self.labelBand)
                }
                .overlay(alignment: .topLeading) {
                    if !dropping, model.image != nil || model.standIn != nil || model.placeholder != nil {
                        GeometryReader { proxy in marks(in: proxy.size) }
                    }
                }
                if dropping {
                    EmptyCanvas(model: model, dropping: true, print: shownPrint)
                } else if let failure {
                    DevelopFailure(name: model.sourceItem.map { model.library.name(of: $0) } ?? "The photograph",
                                   reason: failure, retry: model.retryDevelop)
                        .padding(.top, Self.captionBand).padding(.bottom, Self.labelBand)
                }
            }
        }
        .overlay(alignment: .topLeading) { developTag }
        .onContinuousHover { phase in
            let over: Bool
            if case .active(let point) = phase, let print = shownPrint { over = print.contains(point) } else { over = false }
            if over != overPrint { overPrint = over }
        }
    }

    /// The print's place on the canvas, once it's laid out.
    private var shownPrint: CGRect? {
        geometry.print.width > 0 ? geometry.print.offsetBy(dx: 0, dy: Self.captionBand) : nil
    }

    /// Developing or Updating, at the top centre of the photograph's visible part.
    @ViewBuilder private var developTag: some View {
        let title = DevelopTagTitle.title(source: model.source != nil, failed: model.renderFailed,
                                          developed: model.image != nil, slow: model.showsActivity)
        ZStack {
            if let title, !dropping, !model.cropping {
                GeometryReader { proxy in
                    let canvas = CGRect(origin: .zero, size: proxy.size)
                    let visible = shownPrint.map { $0.intersection(canvas) } ?? CGRect(x: 0, y: Self.captionBand, width: proxy.size.width, height: 0)
                    DevelopTag(title: title)
                        .frame(width: max(visible.width, 1))
                        .offset(x: visible.minX, y: visible.minY + 16)
                }
                .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.18), value: title)
        .allowsHitTesting(false)
    }

    @ViewBuilder private var photograph: some View {
        if model.original == nil, let trial = model.trialImage {
            // A tried stock's tile, enlarged at once until its develop is in (§5.1).
            let size = Self.standInSize(trial, photo: model.photoSize)
            PreviewCanvas(image: trial, size: size, rotation: 0, crop: CropSettings(), border: model.border.shown(for: size),
                          filmTexture: model.filmTexture, filmStrip: model.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage,
                          scale: model.canvasScale, geometry: geometry, doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) })
        } else if let image = model.image {
            canvas(model.original ?? image, size: model.imageSize, rotation: model.imageRotation)
        } else if let standIn = model.standIn {
            // Finished already: drawn as it is, the size the photograph will be.
            PreviewCanvas(image: standIn, size: Self.standInSize(standIn, photo: model.placeholderSize), rotation: 0, crop: CropSettings(),
                          border: nil, filmTexture: nil, filmStrip: nil, zoomCommand: model.zoomCommand, fit: model.fitImage,
                          scale: model.canvasScale, geometry: geometry, doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) })
        } else if let placeholder = model.placeholder {
            canvas(placeholder, size: model.placeholderSize, rotation: model.imageRotation)
        } else {
            Color.clear
        }
    }

    /// The caption 10 pt over the print and the label under it, kept within
    /// the canvas's margins; zoomed in, they stay in their bands.
    @ViewBuilder private func marks(in size: CGSize) -> some View {
        let print = geometry.print.offsetBy(dx: 0, dy: Self.captionBand)
        if print.width > 0 {
            let margin = ImageScrollView.fitInset
            let left = max(print.minX, margin)
            let right = min(print.maxX, size.width - margin)
            let width = max(right - left, 1)
            let portrait = !geometry.zoomed && print.height > print.width
            PhotoCaption(facts: model.photoFacts, fileName: model.sourceItem.map { model.library.name(of: $0) } ?? "", portrait: portrait)
                .frame(width: width, height: 40, alignment: .bottom)
                .offset(x: left, y: print.minY - 10 - 40)
            PrintLabel(model: model, photoWidth: width, canvasWidth: max(size.width - margin - left, width))
                .offset(x: left, y: print.maxY)
            // Not zoomed in: the corners are off the canvas then (§4.3).
            if !geometry.zoomed, model.image != nil, model.developFailure == nil {
                CropTrimMarks(print: print, lit: overPrint, crop: model.beginCrop)
            }
        }
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
                      filmStrip: model.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage, scale: model.canvasScale, geometry: geometry,
                      doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) })
    }
}
