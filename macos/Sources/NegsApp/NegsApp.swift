import AppKit
import SwiftUI

/// AppKit runs the application, menus, window, and title bar; the canvas and
/// inspector inside the window are SwiftUI. A SwiftUI scene with the old
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
        // Which Lightroom is installed, and its copies' cleanup: off the
        // main actor, after launch has settled (ticket 48).
        model.refreshLightroom(delay: .seconds(2))
    }
    func applicationDidBecomeActive(_ notification: Notification) {
        // Not at launch: that look is already scheduled.
        if model.lightroomDetected != nil { model.refreshLightroom() }
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
        // Compare has no strip: its height goes to the photographs.
        let comparing = model.comparison != nil && !grid
        VStack(spacing: 0) {
            // The grid's sheet covers it (`GridSheet`), so coming back to
            // the same photograph is instant and keeps its zoom.
            if comparing {
                CompareCanvas(model: model)
            } else {
                CanvasArea(model: model, dropping: dropTargeted)
            }
            // The tray takes the strip's and the status line's place (§4.5),
            // in Image Only too, which comes back when it closes.
            if let tray = model.tray, !grid {
                StockTray(model: model, visit: tray)
            } else if !dropTargeted, !model.imageOnly {
                VStack(spacing: 0) {
                    // It stays under the grid's sheet, which folds back into it.
                    if model.stripShown, model.source != nil, !comparing {
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
                // status line's top edge with the strip hidden (§11). It
                // stands on that edge, over the canvas: an `alignmentGuide`
                // to its bottom was ignored and left it covering the frame.
                .overlay(alignment: .topTrailing) {
                    if model.source != nil, !grid, !model.cropping, !comparing {
                        StripLeader(model: model).offset(y: -StripLeader.raised)
                    }
                }
            }
        }
        // Where the strip's frames report their places (`SheetFlight`).
        .coordinateSpace(.named(ThumbnailPlaces.space))
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
                // Compare against the original, with the original active:
                // the panel dims, as nothing on it applies.
                let original = model.comparison?.originalActive == true
                VStack(spacing: 0) {
                    PaneHeader(model: model)
                    if let comparison = model.comparison { CompareEditingNote(model: model, comparison: comparison) }
                    Group {
                        if model.inspectorPane == .presets { PresetInspector(model: model) }
                        // The sections aren't built until a photograph opens.
                        else if model.source == nil { InspectorEmptyLine() }
                        else { AdjustmentInspector(model: model) }
                    }
                    .opacity(original ? 0.45 : 1)
                    .disabled(original)
                }
            }
        }
        // With the divider's rule (`EdgeSplitView`), outlines the column
        // where it meets the title band (round 25 A).
        .overlay(alignment: .top) { Theme.line.frame(height: 1).allowsHitTesting(false) }
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
    /// Settings › Photographs › Viewer.
    @AppStorage(ViewerSheet.key) private var sheet = ViewerSheet.theme

    /// Above the photograph, for the caption; below it, for the label's two rows (§2.3).
    static let captionBand: CGFloat = 54
    static let labelBand = PrintLabelLayout.band

    var body: some View {
        ZStack {
            sheet.color
            if let session = model.cropSession, let image = model.image {
                CropEditor(model: model, session: session, image: image)
            } else if model.source == nil, model.image == nil, model.standIn == nil, model.placeholder == nil {
                EmptyCanvas(model: model, dropping: dropping)
            } else {
                let preview = previewCanvas
                let failure = model.developFailure
                if failure == nil, !dropping { lightTable(border: preview?.border) }
                VStack(spacing: 0) {
                    Color.clear.frame(height: Self.captionBand)
                    // Couldn't develop: the last develop, faded and greyed.
                    Group {
                        if let preview {
                            // Keep the same scroll view when a stock tile replaces the develop.
                            preview
                        } else if let standIn = model.standIn {
                            // Already cut and turned. The develop starts with its own zoom command.
                            PreviewCanvas(image: standIn, size: Self.standInSize(standIn, photo: model.placeholderSize), rotation: 0, crop: CropSettings(),
                                          border: nil, filmTexture: nil, filmStrip: nil, zoomCommand: model.zoomCommand, fit: model.fitImage,
                                          scale: model.canvasScale, geometry: geometry, upsideDown: model.upsideDown,
                                          doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) })
                        } else {
                            Color.clear
                        }
                    }
                    .grayscale(failure == nil ? 0 : 1)
                    .opacity(dropping ? 0 : failure == nil ? 1 : 0.12)
                    Color.clear.frame(height: Self.labelBand)
                }
                .overlay(alignment: .topLeading) {
                    if !dropping, model.image != nil || model.standIn != nil || model.placeholder != nil {
                        GeometryReader { proxy in marks(in: proxy.size) }
                    }
                }
                if model.splitting, !dropping, failure == nil, let print = shownPrint {
                    SplitLine(model: model, print: print, zoomed: geometry.zoomed)
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
        .environment(\.sheetInk, sheet.ink)
        // On its own dark plate, so it reads on any sheet.
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

    /// A light under the print when its border's edge sinks into the sheet
    /// (`LightTable`). Fitted only: zoomed in, the print has no edges on screen.
    @ViewBuilder private func lightTable(border: BorderSettings?) -> some View {
        if !geometry.zoomed, let print = shownPrint,
           let light = LightTable.light(sheet: sheet.hex, edge: LightTable.edge(of: border)) {
            LightTableGlow(print: print, color: light)
        }
    }

    /// Developing, Updating or Loading original, at the top centre of the photograph's visible part.
    @ViewBuilder private var developTag: some View {
        let title = DevelopTagTitle.title(source: model.source != nil, failed: model.renderFailed,
                                          developed: model.image != nil, slow: model.showsActivity,
                                          loadingOriginal: model.loadingOriginal,
                                          original: model.originalPhrase)
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

    private var previewCanvas: PreviewCanvas? {
        if model.original == nil, let trial = model.trialImage {
            // The tile is already cut and straightened. Lay it out in the
            // exact crop of the develop, even if the tile's pixels round differently.
            let size = model.image != nil ? model.imageSize : model.placeholderSize
            if size != .zero { return canvas(trial, size: size, crop: model.crop, cut: true) }
            return canvas(trial, size: Self.standInSize(trial, photo: .zero), crop: CropSettings(), cut: true)
        } else if let image = model.image {
            return canvas(model.original ?? image, size: model.imageSize, crop: model.crop)
        } else if model.standIn == nil, let placeholder = model.placeholder {
            return canvas(placeholder, size: model.placeholderSize, crop: model.crop)
        }
        return nil
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
            // Nor in Image Only, which crops nothing.
            if !geometry.zoomed, !model.imageOnly, model.image != nil, model.developFailure == nil {
                CropTrimMarks(model: model, print: print, lit: overPrint, crop: model.beginCrop)
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

    private func canvas(_ image: AnyObject, size: CGSize, crop: CropSettings, cut: Bool = false) -> PreviewCanvas {
        // Picking the base shows the whole frame, unstraightened but still flipped
        // and turned: the rebate is usually cropped off.
        let whole = !cut && model.negativePick == .base
        return PreviewCanvas(image: image, size: size, rotation: model.imageRotation, flip: model.imageFlip,
                             crop: whole ? CropSettings() : crop, cut: cut,
                             border: whole ? nil : model.border.shown(for: crop.size(of: size)), filmTexture: model.filmTexture,
                             filmStrip: model.filmStrip, zoomCommand: model.zoomCommand, fit: model.fitImage, scale: model.canvasScale, geometry: geometry,
                             upsideDown: model.upsideDown, settled: !cut && (model.original != nil || model.showsSettledFrame),
                             doubleClick: { [model] in model.doubleClickPhotograph(zoomed: $0) },
                             pick: cut || model.negativePick == nil ? nil : { [model] in model.pickedNegativePoint($0) },
                             split: cut || model.original != nil ? nil : model.splitOriginal.map { ($0, model.splitFraction) })
    }
}

/// Original ⌄ › Split line (ticket 46): the line over the photograph, the
/// original left of it, "Original" and "Developed" at the print's top
/// corners. Fitted, a drag anywhere on the print moves it; zoomed in, its
/// knob does, and a drag elsewhere pans. It stays put on screen while the
/// photograph pans or zooms: it's a fraction of the canvas's width.
struct SplitLine: View {
    let model: EditorModel
    /// The print's place on the canvas.
    let print: CGRect
    let zoomed: Bool
    @Environment(\.sheetInk) private var ink

    var body: some View {
        GeometryReader { proxy in
            let width = max(proxy.size.width, 1)
            let x = width * model.splitFraction
            ZStack(alignment: .topLeading) {
                if !zoomed {
                    Color.clear.contentShape(Rectangle())
                        .frame(width: print.width, height: print.height)
                        .offset(x: print.minX, y: print.minY)
                        // In the line's space, as the knob is: outside `.offset` the
                        // local space already counts the print's left margin.
                        .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("splitLine")).onChanged { drag in
                            model.splitFraction = min(max(drag.location.x / width, 0), 1)
                        })
                        .onHover { inside in if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() } }
                }
                if x >= print.minX, x <= print.maxX {
                    Rectangle().fill(ink.paper).frame(width: 1.5, height: print.height)
                        .offset(x: x - 0.75, y: print.minY)
                        .allowsHitTesting(false)
                    Text("‹›").font(Theme.serif(14, weight: 600)).tracking(3).foregroundStyle(ink.paper)
                        .frame(width: 32, height: 32)
                        .background(Circle().fill(ink.sheet.opacity(0.85)))
                        .overlay(Circle().strokeBorder(ink.paper, lineWidth: 1.5))
                        .contentShape(Circle())
                        .offset(x: x - 16, y: print.midY - 16)
                        .gesture(DragGesture(coordinateSpace: .named("splitLine")).onChanged { drag in
                            model.splitFraction = min(max(drag.location.x / width, 0), 1)
                        })
                        .allowsHitTesting(zoomed)
                        .accessibilityLabel("Split line")
                }
                tag("Original").offset(x: print.minX + 10, y: print.minY + 10)
                tag("Developed").frame(width: print.width - 20, alignment: .trailing).offset(x: print.minX + 10, y: print.minY + 10)
            }
            .coordinateSpace(.named("splitLine"))
        }
    }

    private func tag(_ text: String) -> some View {
        Text(text).font(Theme.ui(11)).foregroundStyle(Theme.onPhoto)
            .padding(.horizontal, 6).padding(.vertical, 2)
            .background(RoundedRectangle(cornerRadius: 3).fill(Color.black.opacity(0.55)))
            .allowsHitTesting(false)
    }
}
