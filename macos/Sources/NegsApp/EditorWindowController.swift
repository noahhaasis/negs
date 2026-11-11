import AppKit
import Observation
import SwiftUI

/// End field editing even when the clicked view doesn't take keyboard focus.
@MainActor final class EditorWindow: NSWindow {
    override func sendEvent(_ event: NSEvent) {
        InputMode.shared.note(event)
        if [.leftMouseDown, .rightMouseDown, .otherMouseDown].contains(event.type),
           let editor = firstResponder as? NSTextView, editor.isFieldEditor,
           let field = editor.delegate as? NSTextField,
           !field.bounds.contains(field.convert(event.locationInWindow, from: nil)) {
            // Commit before dispatch so the clicked control sees the typed value.
            makeFirstResponder(nil)
        }
        super.sendEvent(event)
    }
}

/// The columns' split view. Its 1 pt divider is a warm `line` rule, not the
/// system's black, so the inspector has an edge against the sheet and the
/// strip; the panel draws the same rule along its top (round 25 A).
final class EdgeSplitView: NSSplitView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        isVertical = true
        dividerStyle = .thin
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var dividerColor: NSColor { Theme.NS.line }
}

/// The editing window: the title band over the canvas and a trailing
/// inspector column, with no toolbar (§3). The window, title band and
/// columns are AppKit; the canvas and inspector contents are SwiftUI. They
/// follow the model through Observation, like the views.
@MainActor final class EditorWindowController: NSWindowController, NSWindowDelegate, NSMenuItemValidation, NSPopoverDelegate {
    private let model: EditorModel
    private let inspector: NSSplitViewItem
    private let panel: NSHostingController<InspectorPanel>
    /// The window's content: the title band over the columns.
    let editorFrame: EditorFrame
    private var collapsing: NSKeyValueObservation?
    private var appearing: NSObjectProtocol?
    private var resigning: NSObjectProtocol?
    private var becomingKey: NSObjectProtocol?
    private var keyMonitor: Any?
    private var mouseMonitor: Any?
    private var menuTracking: NSObjectProtocol?
    /// The split view's container, which the grid's sheet covers.
    private let root: NSView
    /// The canvas column's hosting view.
    private let canvasView: NSView
    /// The grid's sheet, its container and its flight, made the first time
    /// the grid opens (§4.6).
    private var sheet: (host: NSHostingView<GridSheet>, container: SheetContainer, flight: SheetFlight)?
    private var sheetShown = false
    /// The alert sheet's window and the dimming, made the first time one shows.
    private var alertPresenter: AlertPresenter?
    /// The narrowest canvas, and the narrowest the content can be with the
    /// inspector shown beside it. The split view can't keep both columns at
    /// their minimums in a narrower window, so the window enforces the sum.
    private let canvasMinimum: CGFloat
    private let columnsMinimum: CGFloat
    private static let minimumHeight: CGFloat = 560
    /// The key held to show the original, so its release is found even if
    /// a modifier (⌥⇧ on some layouts) was let go first.
    private var originalKey: UInt16?
    /// The key held to turn the photograph upside down, likewise.
    private var turnKey: UInt16?
    /// The batch export popover.
    private var batchPopover: NSPopover?

    init(model: EditorModel) {
        self.model = model
        // The split view sizes both columns, so SwiftUI never measures their
        // contents for the window's size limits.
        let canvas = NSHostingController(rootView: EditorCanvas(model: model))
        canvas.sizingOptions = []
        // The columns start under the title band; the title bar's last
        // 2 pt over them isn't theirs to avoid.
        canvas.safeAreaRegions = []
        // The adjustments take about a third of the time to the first frame,
        // so the column starts empty and fills in once the window is on screen.
        panel = NSHostingController(rootView: InspectorPanel(model: model, built: false))
        panel.sizingOptions = []
        panel.safeAreaRegions = []
        panel.view.frame.size = NSSize(width: 340, height: 860)
        panel.view.identifier = InkKeyLoop.inspectorIdentifier
        let canvasItem = NSSplitViewItem(viewController: canvas)
        // With the inspector at its narrowest, the smallest window is 980 wide (§7).
        canvasItem.minimumThickness = 640
        // A plain column, square and flush: an inspector item floats as a
        // rounded card on macOS 26 once there's no toolbar above it.
        inspector = NSSplitViewItem(viewController: panel)
        inspector.canCollapse = true
        // The canvas takes a window's resizing; the inspector keeps its width.
        inspector.holdingPriority = .init(260)
        // The rows keep a usable slider track down to 340 (§2.3).
        inspector.minimumThickness = Theme.Size.inspectorSmallest
        inspector.maximumThickness = 440
        inspector.isCollapsed = !model.inspectorVisible
        let split = NSSplitViewController()
        split.splitView = EdgeSplitView()
        split.addSplitViewItem(canvasItem)
        split.addSplitViewItem(inspector)
        split.splitView.autosaveName = "editor"
        root = split.view
        canvasView = canvas.view
        canvasMinimum = canvasItem.minimumThickness
        columnsMinimum = canvasItem.minimumThickness + inspector.minimumThickness + split.splitView.dividerThickness

        let window = EditorWindow(contentRect: NSRect(x: 0, y: 0, width: 1320, height: 860),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: true)
        window.title = "Negs"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        // An empty compact toolbar only sets the title bar's height, so the
        // traffic lights sit in the middle of the 38 pt band (§3).
        let toolbar = NSToolbar(identifier: "editor-title")
        toolbar.allowsUserCustomization = false
        if #available(macOS 15, *) { toolbar.allowsDisplayModeCustomization = false }
        window.toolbar = toolbar
        window.toolbarStyle = .unifiedCompact
        window.backgroundColor = Theme.NS.bg
        window.isRestorable = false
        // Place the window before adding its content, so the first layout is at the final size.
        if !window.setFrameUsingName("editor") { window.center() }
        // A saved frame from before the minimum, or from a smaller screen,
        // grows to it.
        Self.fitMinimum(of: window, width: inspector.isCollapsed ? canvasMinimum : columnsMinimum)
        editorFrame = EditorFrame(split: split, size: window.contentRect(forFrameRect: window.frame).size)
        window.contentViewController = editorFrame
        window.setFrameAutosaveName("editor")
        super.init(window: window)
        shouldCascadeWindows = false
        window.delegate = self

        model.undoManager = window.undoManager
        model.presentBatch = { [weak self] view in self?.showBatchPopover(below: view) }
        // Dragging the inspector closed hides the adjustments, like ⌥⌘I.
        collapsing = inspector.observe(\.isCollapsed) { [weak self] item, _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                // A hidden inspector lets the window narrow to the canvas alone.
                if let window = self.window {
                    Self.fitMinimum(of: window, width: item.isCollapsed ? self.canvasMinimum : self.columnsMinimum)
                }
                // The grid collapses it without hiding the adjustments, and
                // Image Only without changing them.
                guard self.model.mode == .photo, self.model.inspectorShown == item.isCollapsed else { return }
                // Dragged open in Image Only, which ends it.
                if !item.isCollapsed { self.model.leaveImageOnly() }
                self.model.inspectorVisible = !item.isCollapsed
            }
        }
        appearing = NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.windowChangedOcclusion() }
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            MainActor.assumeIsolated { self?.handleKey(event) == true } ? nil : event
        }
        // A click outside the tray keeps the stock being tried (§5.1); so
        // does opening a menu, so Edit › Undo undoes the whole visit.
        menuTracking = NotificationCenter.default.addObserver(forName: NSMenu.didBeginTrackingNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.model.endTray(keep: true) }
        }
        mouseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            MainActor.assumeIsolated { self?.clickOutsideTray(event) }
            return event
        }
        resigning = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.releaseHeldKeys() }
        }
        // File events on network shares get lost, so the folder is listed
        // again whenever the window comes back.
        becomingKey = NotificationCenter.default.addObserver(forName: NSWindow.didBecomeKeyNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.model.library.refreshNow() }
        }
        observe()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    /// Prevent window closure during data removal from restoring preferences.
    func prepareForAppDataRemoval() {
        window?.setFrameAutosaveName("")
        editorFrame.split.splitView.autosaveName = nil
        window?.orderOut(nil)
    }

    // MARK: State

    private func windowChangedOcclusion() {
        guard let window, window.occlusionState.contains(.visible), let appearing else { return }
        NotificationCenter.default.removeObserver(appearing)
        self.appearing = nil
        panel.rootView = InspectorPanel(model: model)
    }

    /// While the alert sheet shows, Return answers it with its default
    /// button and Esc with its Cancel one, before anything else takes them
    /// (§5.2). Tab and Space move and press within it; the window's other
    /// keys wait, and its menus are off (`MainMenu.validateMenuItem`).
    private func alertKeys(_ event: NSEvent) -> Bool {
        guard event.window === window, model.alert != nil, alertPresenter?.onWindow == true else { return false }
        guard event.type == .keyDown, !event.modifierFlags.contains(.command) else { return false }
        switch event.keyCode {
        case 36, 76: model.confirmAlert(); return true
        case 53: model.dismissAlert(); return true
        case 48, 49: return false
        default: return true
        }
    }

    /// The window's own keys, before the menus and the views see them. True
    /// if the event was handled.
    func handleKey(_ event: NSEvent) -> Bool {
        alertKeys(event) || holdOriginal(event) || holdUpsideDown(event) || trayKeys(event) || cropKeys(event)
            || enterInspector(event) || escapeToGrid(event) || arrowsToStep(event)
    }

    /// A key let go in another window never reaches this one, so held keys
    /// end when it stops being the key window.
    func releaseHeldKeys() {
        if originalKey != nil { originalKey = nil; model.holdOriginal(false) }
        if turnKey != nil { turnKey = nil; model.holdUpsideDown(false) }
    }

    /// Holding \ shows the original, as in Lightroom, unless text is being
    /// typed. True if the event was handled.
    private func holdOriginal(_ event: NSEvent) -> Bool {
        if event.type == .keyUp {
            guard event.keyCode == originalKey else { return false }
            originalKey = nil; model.holdOriginal(false)
            return true
        }
        guard event.window === window, model.mode == .photo, event.characters == "\\",
              event.modifierFlags.isDisjoint(with: [.command, .control]),
              !(window?.firstResponder is NSText) else { return false }
        if !event.isARepeat { originalKey = event.keyCode; model.holdOriginal(true) }
        return true
    }

    /// Holding T turns the photograph upside down, and letting go turns it
    /// back, like holding \ for the original: only the canvas turns. Not
    /// while text is typed or a crop drawn.
    private func holdUpsideDown(_ event: NSEvent) -> Bool {
        if event.type == .keyUp {
            guard event.keyCode == turnKey else { return false }
            turnKey = nil; model.holdUpsideDown(false)
            return true
        }
        guard event.window === window, model.mode == .photo, model.source != nil, !model.cropping,
              event.charactersIgnoringModifiers?.lowercased() == "t",
              event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
              !(window?.firstResponder is NSText) else { return false }
        if !event.isARepeat { turnKey = event.keyCode; model.holdUpsideDown(true) }
        return true
    }

    /// Tab from outside the inspector (nothing, the canvas, the strip) moves
    /// to its first control, ⇧Tab to its last; inside it the controls step
    /// through themselves (`InkKeyLoop`). Text fields keep Tab.
    private func enterInspector(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.keyCode == 48, event.window === window, model.mode == .photo,
              event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
              !inspector.isCollapsed, !(window?.firstResponder is NSText) else { return false }
        let root = panel.view
        if let current = window?.firstResponder as? NSView, InkKeyLoop.root(of: current) != nil { return false }
        return InkKeyLoop.advance(in: root, from: nil, forward: !event.modifierFlags.contains(.shift))
    }

    /// While the tray is open, Esc goes back to the stock from before it
    /// opened, Return keeps the one being tried, and ← → step through the
    /// stocks rather than the photographs (§5.1, §5.2). Typing keeps its keys.
    private func trayKeys(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.window === window, model.tray != nil else { return false }
        // A command keeps the stock being tried first, then runs in an event
        // of its own, so it's an undo step of its own (and ⌘Z undoes the
        // whole visit).
        if event.modifierFlags.contains(.command), !(window?.firstResponder is NSText) {
            model.endTray(keep: true)
            NSApp.postEvent(event, atStart: true)
            return true
        }
        guard event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
              !(window?.firstResponder is NSText) else { return false }
        switch event.keyCode {
        case 53: model.endTray(keep: false)
        case 36, 76: model.endTray(keep: true)
        case 123, 124: model.stepTray(by: event.keyCode == 123 ? -1 : 1)
        default: return false
        }
        return true
    }

    /// While cropping, Return applies the crop and Esc cancels it (§5.2),
    /// unless a field is being typed in: it keeps its own.
    private func cropKeys(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.window === window, let session = model.cropSession, !session.typing,
              event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
              !(window?.firstResponder is NSText) else { return false }
        switch event.keyCode {
        case 36, 76: model.endCrop(apply: true)
        case 53: model.endCrop(apply: false)
        default: return false
        }
        return true
    }

    private func clickOutsideTray(_ event: NSEvent) {
        guard model.tray != nil, event.window === window else { return }
        for view in model.trayHitAreas.allObjects where view.window === window && !view.isHiddenOrHasHiddenAncestor {
            if view.bounds.contains(view.convert(event.locationInWindow, from: nil)) { return }
        }
        model.endTray(keep: true)
    }

    /// Esc in the photograph view leaves Image Only, then opens the grid; in
    /// the grid it folds it back (§5.2). Not while text is being typed or a
    /// crop drawn (whose Cancel takes it). The canvas's hosting view claims
    /// Escape before the menu would see it, so it's caught here.
    private func escapeToGrid(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.keyCode == 53, event.window === window,
              event.modifierFlags.isDisjoint(with: [.command, .control, .option, .shift]),
              model.source != nil, !model.cropping, MainMenu.takesRatingKeys(window) else { return false }
        if model.mode == .grid { model.editCurrent() } else if model.imageOnly { model.leaveImageOnly() } else { model.showGrid() }
        return true
    }

    /// ← and → in the photograph view open the previous and next photograph
    /// wherever the keys are, not only once the strip has been clicked; with
    /// ⇧ they select a range, as in the strip. The inspector's controls and
    /// text keep their arrows, and the tray its (`trayKeys`).
    private func arrowsToStep(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.keyCode == 123 || event.keyCode == 124, event.window === window,
              model.mode == .photo, model.source != nil, !model.cropping,
              event.modifierFlags.isDisjoint(with: [.command, .control, .option]),
              !(window?.firstResponder is NSText) else { return false }
        if let focused = window?.firstResponder as? NSView, InkKeyLoop.root(of: focused) != nil { return false }
        model.move(by: event.keyCode == 123 ? -1 : 1, extending: event.modifierFlags.contains(.shift))
        return true
    }

    /// Applies the model now rather than on the next observation pass, which
    /// at launch runs only after the window is on screen.
    func refresh() {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0
            update()
        }
    }

    private func observe() {
        withObservationTracking { update() } onChange: { [weak self] in
            Task { @MainActor in self?.observe() }
        }
    }

    private func update() {
        guard let window else { return }
        let grid = model.mode == .grid
        // The file name, in the grid too; "Negs" with no photograph (§3).
        let title = model.sourceItem.map { model.library.name(of: $0) } ?? "Negs"
        if window.title != title { window.title = title }
        editorFrame.band.title = title
        showSheet(grid)
        let alert = model.alert
        if alert != nil, alertPresenter == nil { alertPresenter = AlertPresenter(model: model, window: window, root: root) }
        alertPresenter?.show(alert)
        // The grid's sheet covers the inspector, which stays as it was under
        // it; Image Only hides it.
        let inspectorShown = model.inspectorShown
        if inspector.isCollapsed == inspectorShown {
            // Make room before the inspector slides in, so neither column is
            // squeezed below its minimum.
            if inspectorShown { Self.fitMinimum(of: window, width: columnsMinimum) }
            inspector.animator().isCollapsed = !inspectorShown
        }
    }

    /// The sheet over the whole window below the title bar, with the
    /// canvas and the inspector under it; it grows out of the strip and
    /// folds back into it (`SheetFlight`). Keys go to it while it shows.
    private func showSheet(_ grid: Bool) {
        guard grid != sheetShown else { return }
        sheetShown = grid
        guard let sheet = grid ? makeSheet() : self.sheet else { model.sheetFolding = false; return }
        if grid {
            sheet.flight.open()
            window?.makeFirstResponder(sheet.host)
        } else {
            if let responder = window?.firstResponder as? NSView, responder.isDescendant(of: sheet.container) {
                window?.makeFirstResponder(nil)
            }
            sheet.flight.fold()
        }
    }

    private func makeSheet() -> (host: NSHostingView<GridSheet>, container: SheetContainer, flight: SheetFlight) {
        if let sheet { return sheet }
        let container = SheetContainer(frame: root.bounds)
        container.autoresizingMask = [.width, .height]
        container.wantsLayer = true
        container.isHidden = true
        let host = NSHostingView(rootView: GridSheet(model: model))
        host.sizingOptions = []
        host.safeAreaRegions = []
        host.frame = container.bounds
        host.autoresizingMask = [.width, .height]
        container.addSubview(host)
        root.addSubview(container, positioned: .above, relativeTo: nil)
        let flight = SheetFlight(model: model, root: root, canvas: canvasView, container: container, sheet: host)
        sheet = (host, container, flight)
        return (host, container, flight)
    }

    /// In full screen the title bar's empty toolbar hides with the menu bar
    /// and comes down with it. Left showing, it was a grey bar across the
    /// top: the system draws a full-screen toolbar on its own material,
    /// whatever the window's title bar looks like.
    func window(_ window: NSWindow, willUseFullScreenPresentationOptions proposedOptions: NSApplication.PresentationOptions = []) -> NSApplication.PresentationOptions {
        proposedOptions.union(.autoHideToolbar)
    }

    /// Sets the window's minimum content size and grows the window to it when
    /// it's smaller, keeping the top-left corner in place and the window on
    /// its screen. Full screen already fills the screen.
    private static func fitMinimum(of window: NSWindow, width: CGFloat) {
        let minimum = NSSize(width: width, height: minimumHeight)
        window.contentMinSize = minimum
        guard !window.styleMask.contains(.fullScreen) else { return }
        let content = window.contentRect(forFrameRect: window.frame)
        guard content.width < minimum.width || content.height < minimum.height else { return }
        let height = max(content.height, minimum.height)
        var frame = window.frameRect(forContentRect: NSRect(x: content.minX, y: content.maxY - height,
                                                            width: max(content.width, minimum.width), height: height))
        if let visible = (window.screen ?? NSScreen.main)?.visibleFrame {
            // Slide left or down rather than off the screen's edge.
            frame.origin.x = max(min(frame.origin.x, visible.maxX - frame.width), visible.minX)
            frame.origin.y = max(min(frame.origin.y, visible.maxY - frame.height), visible.minY)
        }
        window.setFrame(frame, display: window.isVisible)
    }

    // MARK: Batch

    /// The batch's popover over a view: the status line's Show.
    private func showBatchPopover(below view: NSView) {
        guard let run = model.batch else { return }
        if let batchPopover, batchPopover.isShown { return batchPopover.performClose(nil) }
        showBatchPopover(run) { $0.show(relativeTo: view.bounds, of: view, preferredEdge: .maxY) }
    }

    private func showBatchPopover(_ run: BatchRun, show: (NSPopover) -> Void) {
        let content = NSHostingController(rootView: BatchProgressView(run: run, retry: { [weak self] in self?.model.retryBatch() },
                                                                      showInFinder: { [weak self] in self?.model.showBatchInFinder() }))
        content.sizingOptions = .preferredContentSize
        let popover = NSPopover()
        popover.behavior = .transient
        popover.appearance = NSAppearance(named: .darkAqua)
        popover.contentViewController = content
        popover.delegate = self
        batchPopover = popover
        model.batchPopoverShown = true
        show(popover)
        Self.paint(popover)
    }

    /// Fills a popover's frame, its arrow too, with the panel's colour in
    /// place of the system's frosted glass (round 16).
    static func paint(_ popover: NSPopover) {
        guard let frame = popover.contentViewController?.view.window?.contentView?.superview,
              !frame.subviews.contains(where: { $0.identifier == panelFill }) else { return }
        let fill = NSView(frame: frame.bounds)
        fill.identifier = panelFill
        fill.wantsLayer = true
        fill.layer?.backgroundColor = Theme.NS.panel.cgColor
        fill.autoresizingMask = [.width, .height]
        frame.addSubview(fill, positioned: .below, relativeTo: nil)
    }
    static let panelFill = NSUserInterfaceItemIdentifier("negs.popover.panel")

    func popoverDidClose(_ notification: Notification) {
        guard (notification.object as? NSPopover) === batchPopover else { return }
        batchPopover = nil
        model.batchPopoverShown = false
        model.clearBatch()
    }

    // MARK: Selection

    // Edit › Select All and Deselect All reach here through the responder
    // chain only when no text field takes them first.
    override func selectAll(_ sender: Any?) { model.selectAll() }
    @objc func deselectAll(_ sender: Any?) { model.deselectAll() }

    /// The photographs are selected where they're shown: the grid, or the strip.
    private var selecting: Bool {
        model.source != nil && !model.cropping && (model.mode == .grid || model.stripShown)
    }

    // MARK: Edits

    // Edit › Copy and Paste (⌘C, ⌘V) copy and paste the photograph's edits,
    // like Photo › Copy Edits and Paste Edits (⇧⌘C, ⇧⌘V). They reach here,
    // the end of the responder chain, only when no text field or view takes
    // them first, so typed text keeps its own copy and paste.
    @objc func copy(_ sender: Any?) { model.copyEdits() }
    @objc func paste(_ sender: Any?) { model.pasteEdits() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(selectAll(_:)), #selector(deselectAll(_:)): selecting && model.alert == nil
        // Not while a sheet or another window has the keys, as for Photo › Copy Edits.
        case #selector(copy(_:)): model.canCopyEdits && MainMenu.takesRatingKeys(NSApp.keyWindow)
        case #selector(paste(_:)): model.canPasteEdits && MainMenu.takesRatingKeys(NSApp.keyWindow)
        default: true
        }
    }
}
