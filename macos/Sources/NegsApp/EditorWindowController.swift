import AppKit
import Observation
import SwiftUI

/// The editing window: a unified toolbar over the canvas and a trailing
/// inspector column. The window, toolbar, and columns are AppKit; the canvas
/// and inspector contents are SwiftUI. They follow the model through
/// Observation, like the views.
@MainActor final class EditorWindowController: NSWindowController, NSToolbarDelegate {
    private let model: EditorModel
    private let inspector: NSSplitViewItem
    private let panel: NSHostingController<InspectorPanel>
    private var items: [NSToolbarItem.Identifier: NSToolbarItem] = [:]
    private lazy var cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancelCrop))
    private lazy var doneButton = NSButton(title: "Done", target: self, action: #selector(applyCrop))
    private var collapsing: NSKeyValueObservation?
    private var appearing: NSObjectProtocol?
    private var resigning: NSObjectProtocol?
    private var keyMonitor: Any?
    /// The key held to show the original, so its release is found even if
    /// a modifier (⌥⇧ on some layouts) was let go first.
    private var originalKey: UInt16?

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
        let canvasItem = NSSplitViewItem(viewController: canvas)
        canvasItem.minimumThickness = 680
        inspector = NSSplitViewItem(inspectorWithViewController: panel)
        inspector.minimumThickness = 300
        inspector.maximumThickness = 440
        inspector.isCollapsed = !model.inspectorVisible
        let split = NSSplitViewController()
        split.addSplitViewItem(canvasItem)
        split.addSplitViewItem(inspector)
        split.splitView.autosaveName = "editor"

        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1320, height: 860),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: true)
        window.title = "Negs"
        window.toolbarStyle = .unified
        window.isRestorable = false
        // Place the window before adding its content, so the first layout is at the final size.
        if !window.setFrameUsingName("editor") { window.center() }
        split.view.frame = NSRect(origin: .zero, size: window.contentRect(forFrameRect: window.frame).size)
        window.contentViewController = split
        window.setFrameAutosaveName("editor")
        super.init(window: window)
        shouldCascadeWindows = false

        let toolbar = NSToolbar(identifier: "editor")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.centeredItemIdentifiers = [.zoom, .edit]
        window.toolbar = toolbar
        model.undoManager = window.undoManager
        // Dragging the inspector closed hides the adjustments, like the toolbar button.
        collapsing = inspector.observe(\.isCollapsed) { [weak self] item, _ in
            MainActor.assumeIsolated {
                guard let self, self.model.inspectorVisible == item.isCollapsed else { return }
                self.model.inspectorVisible = !item.isCollapsed
            }
        }
        appearing = NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.windowChangedOcclusion() }
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            MainActor.assumeIsolated { self?.holdOriginal(event) ?? false } ? nil : event
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

    // MARK: State

    private func windowChangedOcclusion() {
        guard let window, window.occlusionState.contains(.visible), let appearing else { return }
        NotificationCenter.default.removeObserver(appearing)
        self.appearing = nil
        panel.rootView = InspectorPanel(model: model)
    }

    /// Holding \ shows the original, as in Lightroom, unless text is being
    /// typed. True if the event was handled.
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
        showCropActions(model.cropping)
        setEnabled(.zoom, photo && !model.cropping)
        if let edit = items[.edit] as? NSToolbarItemGroup {
            edit.isEnabled = photo
            edit.subitems[0].isEnabled = photo
            edit.subitems[1].isEnabled = model.image != nil
            let crop = edit.subitems[1]
            crop.toolTip = model.cropping ? "Apply the crop (⌘K)" : "Crop the photograph (⌘K)"
            // The accent color marks crop mode as on.
            crop.image = model.cropping
                ? symbol("crop", "Crop").withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [.controlAccentColor]))
                : symbol("crop", "Crop")
        }
        setEnabled(.export, photo && model.ready && !model.exporting)
        // A ratio field being edited keeps its own Return and Escape.
        if let session = model.cropSession {
            cancelButton.keyEquivalent = session.typing ? "" : "\u{1b}"
            doneButton.keyEquivalent = session.typing ? "" : "\r"
        }
        if inspector.isCollapsed == model.inspectorVisible { inspector.animator().isCollapsed = !model.inspectorVisible }
    }

    private func setEnabled(_ id: NSToolbarItem.Identifier, _ enabled: Bool) {
        guard let item = items[id] else { return }
        item.isEnabled = enabled
        (item as? NSToolbarItemGroup)?.subitems.forEach { $0.isEnabled = enabled }
    }

    /// While cropping, Cancel and Done take the place of Export.
    private func showCropActions(_ cropping: Bool) {
        guard let toolbar = window?.toolbar else { return }
        let ids = toolbar.items.map(\.itemIdentifier)
        if cropping, let i = ids.firstIndex(of: .export) {
            toolbar.removeItem(at: i)
            toolbar.insertItem(withItemIdentifier: .doneCrop, at: i)
            toolbar.insertItem(withItemIdentifier: .cancelCrop, at: i)
        } else if !cropping, let i = ids.firstIndex(of: .cancelCrop) {
            toolbar.removeItem(at: i + 1)
            toolbar.removeItem(at: i)
            toolbar.insertItem(withItemIdentifier: .export, at: i)
        }
    }

    // MARK: Toolbar

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.open, .flexibleSpace, .zoom, .edit, .flexibleSpace, .inspectorTrackingSeparator, .flexibleSpace, .export, .adjustments]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar) + [.cancelCrop, .doneCrop]
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item: NSToolbarItem
        switch id {
        case .open:
            item = button(id, "Open", "folder", "Open photograph (⌘O)", #selector(openPhotograph))
            item.isNavigational = true
        case .zoom:
            let group = NSToolbarItemGroup(itemIdentifier: id)
            group.subitems = [button(.init("fit"), "Fit", "arrow.down.right.and.arrow.up.left", "Fit photograph (⌘0)", #selector(fit)),
                              button(.init("actualSize"), "Actual Size", "1.magnifyingglass", "Actual size (⌘1)", #selector(actualSize))]
            group.label = "Zoom"
            item = group
        case .edit:
            // Grouped items share one capsule, without the dividers of a segmented group.
            let group = NSToolbarItemGroup(itemIdentifier: id)
            group.subitems = [button(.init("rotate"), "Rotate", "rotate.right", "Rotate clockwise (⌘R)", #selector(rotateClockwise)),
                              button(.init("crop"), "Crop", "crop", "Crop the photograph (⌘K)", #selector(toggleCrop))]
            group.label = "Edit"
            item = group
        case .export:
            item = button(id, "Export", "square.and.arrow.down", "Export photograph (⇧⌘E)", #selector(export))
        case .adjustments:
            item = button(id, "Adjustments", "slider.horizontal.3", "Show or hide adjustments (⌥⌘I)", #selector(toggleAdjustments))
        case .cancelCrop:
            item = NSToolbarItem(itemIdentifier: id)
            item.view = cancelButton; item.label = "Cancel"; item.toolTip = "Discard the crop (Esc)"
        case .doneCrop:
            item = NSToolbarItem(itemIdentifier: id)
            item.view = doneButton; item.label = "Done"; item.toolTip = "Apply the crop (Return)"
            // The confirming action is filled with the accent color.
            if #available(macOS 26, *) { item.style = .prominent } else { doneButton.bezelColor = .controlAccentColor }
        default:
            return nil
        }
        // Enabled from the model's state in update(), not by validation.
        item.autovalidates = false
        (item as? NSToolbarItemGroup)?.subitems.forEach { $0.autovalidates = false }
        items[id] = item
        return item
    }

    private func button(_ id: NSToolbarItem.Identifier, _ label: String, _ symbolName: String, _ help: String, _ action: Selector) -> NSToolbarItem {
        let item = NSToolbarItem(itemIdentifier: id)
        item.label = label
        item.image = symbol(symbolName, label)
        item.toolTip = help
        item.isBordered = true
        item.target = self
        item.action = action
        return item
    }

    private func symbol(_ name: String, _ description: String) -> NSImage {
        NSImage(systemSymbolName: name, accessibilityDescription: description) ?? NSImage()
    }

    // MARK: Actions

    @objc private func openPhotograph() { model.openPanel() }
    @objc private func fit() { model.zoom(fit: true) }
    @objc private func actualSize() { model.zoom(fit: false) }
    @objc private func rotateClockwise() { model.rotate() }
    @objc private func toggleCrop() { model.cropping ? model.endCrop(apply: true) : model.beginCrop() }
    @objc private func cancelCrop() { model.endCrop(apply: false) }
    @objc private func applyCrop() { model.endCrop(apply: true) }
    @objc private func export() { model.savePanel(export: true) }
    @objc private func toggleAdjustments() { model.inspectorVisible.toggle() }
}

private extension NSToolbarItem.Identifier {
    static let open = Self("open"), zoom = Self("zoom"), edit = Self("edit")
    static let export = Self("export"), adjustments = Self("adjustments"), cancelCrop = Self("cancelCrop"), doneCrop = Self("doneCrop")
}
