import AppKit
import Observation
import SwiftUI

/// End field editing even when the clicked view doesn't take keyboard focus.
@MainActor final class EditorWindow: NSWindow {
    override func sendEvent(_ event: NSEvent) {
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

/// The editing window: the canvas and a trailing inspector column, with no
/// toolbar: everything's in the menus. The window and columns are AppKit; the canvas
/// and inspector contents are SwiftUI. They follow the model through
/// Observation, like the views.
@MainActor final class EditorWindowController: NSWindowController {
    private let model: EditorModel
    private let inspector: NSSplitViewItem
    private let canvasItem: NSSplitViewItem
    private let panel: NSHostingController<InspectorPanel>
    private var collapsing: NSKeyValueObservation?
    private var appearing: NSObjectProtocol?
    private var resigning: NSObjectProtocol?
    private var keyMonitor: Any?
    private var fullScreenObservers: [NSObjectProtocol] = []
    /// The key held to show the original, so its release is found even if
    /// a modifier (⌥⇧ on some layouts) was let go first.
    private var originalKey: UInt16?
    private var wasImageOnly = false

    init(model: EditorModel) {
        self.model = model
        // The split view sizes both columns, so SwiftUI never measures their
        // contents for the window's size limits.
        let canvas = NSHostingController(rootView: EditorCanvas(model: model))
        canvas.sizingOptions = []
        // The adjustments take about a third of the time to the first frame,
        // so the column starts empty and fills in once the window is on screen.
        panel = NSHostingController(rootView: InspectorPanel(model: model, built: false))
        panel.sizingOptions = []
        panel.view.frame.size = NSSize(width: 340, height: 860)
        canvasItem = NSSplitViewItem(viewController: canvas)
        canvasItem.minimumThickness = 680
        inspector = NSSplitViewItem(inspectorWithViewController: panel)
        // Wide enough for the six workflow tabs and their labels.
        inspector.minimumThickness = 340
        inspector.maximumThickness = 440
        inspector.isCollapsed = !model.inspectorVisible
        // Below the title bar, across the window.
        inspector.allowsFullHeightLayout = false
        let split = NSSplitViewController()
        split.addSplitViewItem(canvasItem)
        split.addSplitViewItem(inspector)
        split.splitView.autosaveName = "editor"

        let window = EditorWindow(contentRect: NSRect(x: 0, y: 0, width: 1320, height: 860),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: true)
        window.title = "Negs"
        // Black from edge to edge, the title bar too.
        NSApp.appearance = NSAppearance(named: .darkAqua)
        window.appearance = NSAppearance(named: .darkAqua)
        window.backgroundColor = .black
        window.titlebarAppearsTransparent = true
        window.isRestorable = false
        // Place the window before adding its content, so the first layout is at the final size.
        if !window.setFrameUsingName("editor") { window.center() }
        // The split view cannot keep both columns at their minimums in a
        // narrower window, so the window itself enforces their sum.
        let minimum = NSSize(width: 680 + 340 + split.splitView.dividerThickness, height: 560)
        window.contentMinSize = minimum
        let restored = window.contentRect(forFrameRect: window.frame).size
        if restored.width < minimum.width || restored.height < minimum.height {
            window.setContentSize(NSSize(width: max(restored.width, minimum.width),
                                         height: max(restored.height, minimum.height)))
        }
        split.view.frame = NSRect(origin: .zero, size: window.contentRect(forFrameRect: window.frame).size)
        window.contentViewController = split
        window.setFrameAutosaveName("editor")
        super.init(window: window)
        shouldCascadeWindows = false

        model.undoManager = window.undoManager
        // Dragging the inspector closed hides the adjustments, like the menu command.
        collapsing = inspector.observe(\.isCollapsed) { [weak self] item, _ in
            MainActor.assumeIsolated {
                // F collapses it without changing what comes back.
                guard let self, !self.model.imageOnly, self.model.inspectorVisible == item.isCollapsed else { return }
                self.model.inspectorVisible = !item.isCollapsed
            }
        }
        appearing = NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.windowChangedOcclusion() }
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            MainActor.assumeIsolated {
                guard let self else { return false }
                return self.holdOriginal(event) || self.togglePins(event) || self.toggleGridKey(event) || self.toggleImageOnlyKey(event) || self.stepPhotographKey(event)
            } ? nil : event
        }
        // The title bar hides in full screen, so the status bar carries the title there.
        fullScreenObservers = [(NSWindow.willEnterFullScreenNotification, true), (NSWindow.willExitFullScreenNotification, false)].map { name, entering in
            NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.model.windowFullScreen = entering
                }
            }
        }
        resigning = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.originalKey != nil else { return }
                self.originalKey = nil; self.model.holdOriginal(false)
            }
        }
        observe()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    /// Prevent window closure during data removal from restoring preferences.
    func prepareForAppDataRemoval() {
        window?.setFrameAutosaveName("")
        (window?.contentViewController as? NSSplitViewController)?.splitView.autosaveName = nil
        window?.orderOut(nil)
    }

    // MARK: State

    private func windowChangedOcclusion() {
        guard let window, window.occlusionState.contains(.visible), let appearing else { return }
        NotificationCenter.default.removeObserver(appearing)
        self.appearing = nil
        panel.rootView = InspectorPanel(model: model)
    }

    /// Holding \ shows the original, as in Lightroom, unless text is being
    /// typed. True if the event was handled.
    /// Whether a key press is the bare G that swaps between the grid and the edit view.
    static func togglesGrid(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, firstResponder: NSResponder?) -> Bool {
        keyCode == 5 && modifiers.isDisjoint(with: [.shift, .control, .option, .command]) && !(firstResponder is NSText)
    }

    private func toggleGridKey(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.window === window, model.source != nil, !model.cropping,
              Self.togglesGrid(keyCode: event.keyCode, modifiers: event.modifierFlags, firstResponder: window?.firstResponder)
        else { return false }
        if !event.isARepeat { model.toggleGrid() }
        return true
    }

    /// How far a key press moves through the folder: a bare left or right arrow,
    /// unless a field or slider has the keyboard and needs it.
    static func photographStep(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, firstResponder: NSResponder?) -> Int? {
        guard modifiers.isDisjoint(with: [.shift, .control, .option, .command]),
              !(firstResponder is NSText), !(firstResponder is NSSlider) else { return nil }
        return keyCode == 123 ? -1 : keyCode == 124 ? 1 : nil
    }

    private func stepPhotographKey(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.window === window, model.source != nil, !model.cropping, !model.gridVisible,
              let step = Self.photographStep(keyCode: event.keyCode, modifiers: event.modifierFlags, firstResponder: window?.firstResponder)
        else { return false }
        model.openNeighbour(step)
        return true
    }

    /// Whether a key press is the bare F that shows only the photograph.
    static func togglesImageOnly(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, firstResponder: NSResponder?) -> Bool {
        keyCode == 3 && modifiers.isDisjoint(with: [.shift, .control, .option, .command]) && !(firstResponder is NSText)
    }

    private func toggleImageOnlyKey(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.window === window, model.source != nil, !model.cropping, !model.gridVisible,
              Self.togglesImageOnly(keyCode: event.keyCode, modifiers: event.modifierFlags, firstResponder: window?.firstResponder)
        else { return false }
        if !event.isARepeat { model.toggleImageOnly() }
        return true
    }

    /// Whether a key press is the bare Tab that shows or hides the pins. It stands
    /// down while a field is being edited, so Tab still moves between fields.
    static func togglesPins(keyCode: UInt16, modifiers: NSEvent.ModifierFlags, firstResponder: NSResponder?) -> Bool {
        keyCode == 48 && modifiers.isDisjoint(with: [.shift, .control, .option, .command]) && !(firstResponder is NSText)
    }

    /// Handled here rather than by the menu alone, because Tab otherwise moves
    /// the keyboard focus before a menu shortcut sees it. True if it was Tab.
    private func togglePins(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.window === window, model.source != nil, !model.cropping,
              Self.togglesPins(keyCode: event.keyCode, modifiers: event.modifierFlags, firstResponder: window?.firstResponder)
        else { return false }
        if !event.isARepeat { model.pinsVisible.toggle() }
        return true
    }

    private func holdOriginal(_ event: NSEvent) -> Bool {
        if event.type == .keyUp {
            guard event.keyCode == originalKey else { return false }
            originalKey = nil; model.holdOriginal(false)
            return true
        }
        guard event.window === window, event.characters == "\\",
              event.modifierFlags.isDisjoint(with: [.command, .control]),
              !(window?.firstResponder is NSText) else { return false }
        if !event.isARepeat { originalKey = event.keyCode; model.holdOriginal(true) }
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
        let photo = model.source != nil
        window.title = model.source?.lastPathComponent ?? "Negs"
        window.subtitle = !photo || !model.ready ? "" : model.value("io.scan_film").bool ? model.filmName : "\(model.filmName) → \(model.paperName)"
        let imageOnly = model.imageOnly
        // Full screen has no title bar to stay below, so the column runs to the top.
        inspector.allowsFullHeightLayout = model.windowFullScreen
        window.titleVisibility = model.windowFullScreen || imageOnly ? .hidden : .visible
        for button in [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton] {
            window.standardWindowButton(button)?.isHidden = imageOnly
        }
        let collapsed = !model.inspectorVisible || imageOnly
        if inspector.isCollapsed != collapsed {
            // Straight there for F, so the photograph doesn't slide about.
            if imageOnly != wasImageOnly { inspector.isCollapsed = collapsed } else { inspector.animator().isCollapsed = collapsed }
        }
        wasImageOnly = imageOnly
    }
}
