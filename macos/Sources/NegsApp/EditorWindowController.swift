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

/// The editing window: a unified toolbar over the canvas and a trailing
/// inspector column. The window, toolbar, and columns are AppKit; the canvas
/// and inspector contents are SwiftUI. They follow the model through
/// Observation, like the views.
@MainActor final class EditorWindowController: NSWindowController, NSToolbarDelegate, NSMenuDelegate {
    private let model: EditorModel
    private let inspector: NSSplitViewItem
    private let panel: NSHostingController<InspectorPanel>
    private var items: [NSToolbarItem.Identifier: NSToolbarItem] = [:]
    private lazy var cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancelCrop))
    private lazy var doneButton = NSButton(title: "Done", target: self, action: #selector(applyCrop))
    private var collapsing: NSKeyValueObservation?
    private var appearing: NSObjectProtocol?
    private var resigning: NSObjectProtocol?
    private var becomingKey: NSObjectProtocol?
    private var keyMonitor: Any?
    /// The narrowest canvas, and the narrowest the content can be with the
    /// inspector shown beside it. The split view can't keep both columns at
    /// their minimums in a narrower window, so the window enforces the sum.
    private let canvasMinimum: CGFloat
    private let columnsMinimum: CGFloat
    private static let minimumHeight: CGFloat = 560
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
        canvasMinimum = canvasItem.minimumThickness
        columnsMinimum = canvasItem.minimumThickness + inspector.minimumThickness + split.splitView.dividerThickness

        let window = EditorWindow(contentRect: NSRect(x: 0, y: 0, width: 1320, height: 860),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: true)
        window.title = "Negs"
        window.toolbarStyle = .unified
        window.isRestorable = false
        // Place the window before adding its content, so the first layout is at the final size.
        if !window.setFrameUsingName("editor") { window.center() }
        // A saved frame from before the minimum, or from a smaller screen,
        // grows to it.
        Self.fitMinimum(of: window, width: inspector.isCollapsed ? canvasMinimum : columnsMinimum)
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
                guard let self else { return }
                // A hidden inspector lets the window narrow to the canvas alone.
                if let window = self.window {
                    Self.fitMinimum(of: window, width: item.isCollapsed ? self.canvasMinimum : self.columnsMinimum)
                }
                guard self.model.inspectorVisible == item.isCollapsed else { return }
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
        showCropActions(model.cropping)
        if let item = items[.filmstrip] {
            item.toolTip = model.filmstripVisible ? "Hide film strip (⌥⌘F)" : "Show film strip (⌥⌘F)"
            let icon = symbol("rectangle.bottomthird.inset.filled", "Film Strip")
            item.image = model.filmstripVisible
                ? icon.withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [.controlAccentColor])) : icon
        }
        if let item = items[.filter] {
            // Filled while a filter is on, so a half-empty strip is never a mystery.
            let on = model.library.filter.active
            item.image = symbol(on ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle", "Filter")
            item.toolTip = on ? "Filter: \(Self.describe(model.library.filter))" : "Filter photographs by rating and flag"
            item.isEnabled = photo
        }
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
        setEnabled(.export, photo && model.ready && !model.exporting && !model.saving)
        // The accent color marks the panel the inspector column shows.
        for (id, pane, name, label) in [(NSToolbarItem.Identifier.adjustments, InspectorPane.adjustments, "slider.horizontal.3", "Adjustments"),
                                        (.presets, .presets, "square.stack", "Presets")] {
            let icon = symbol(name, label)
            items[id]?.image = model.inspectorVisible && model.inspectorPane == pane
                ? icon.withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [.controlAccentColor])) : icon
        }
        // A ratio field being edited keeps its own Return and Escape.
        if let session = model.cropSession {
            cancelButton.keyEquivalent = session.typing ? "" : "\u{1b}"
            doneButton.keyEquivalent = session.typing ? "" : "\r"
        }
        if inspector.isCollapsed == model.inspectorVisible {
            // Make room before the inspector slides in, so neither column is
            // squeezed below its minimum.
            if model.inspectorVisible { Self.fitMinimum(of: window, width: columnsMinimum) }
            inspector.animator().isCollapsed = !model.inspectorVisible
        }
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
        [.open, .filmstrip, .filter, .flexibleSpace, .zoom, .edit, .flexibleSpace, .inspectorTrackingSeparator, .flexibleSpace, .export, .presets, .adjustments]
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
        case .filmstrip:
            item = button(id, "Film Strip", "rectangle.bottomthird.inset.filled", "Show or hide film strip (⌥⌘F)", #selector(toggleFilmstrip))
        case .filter:
            let menuItem = NSMenuToolbarItem(itemIdentifier: id)
            menuItem.label = "Filter"
            menuItem.image = symbol("line.3.horizontal.decrease.circle", "Filter")
            menuItem.showsIndicator = false
            // Filled in when it opens, from the filter as it is then.
            let menu = NSMenu(title: "Filter")
            menu.delegate = self
            menuItem.menu = menu
            item = menuItem
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
        case .presets:
            item = button(id, "Presets", "square.stack", "Show or hide presets (⌥⌘P)", #selector(togglePresets))
        case .adjustments:
            item = button(id, "Adjustments", "slider.horizontal.3", "Show or hide adjustments (⌥⌘I)", #selector(toggleAdjustments))
        case .cancelCrop:
            item = NSToolbarItem(itemIdentifier: id)
            item.view = cancelButton; item.label = "Cancel"; item.toolTip = "Discard the crop (Esc)"
        case .doneCrop:
            item = NSToolbarItem(itemIdentifier: id)
            item.view = doneButton; item.label = "Done"; item.toolTip = "Apply the crop (Return)"
            // The confirming action is filled with the accent color. The style
            // needs the macOS 26 SDK, which release CI's Xcode 16 lacks.
            #if compiler(>=6.2)
            if #available(macOS 26, *) { item.style = .prominent } else { doneButton.bezelColor = .controlAccentColor }
            #else
            doneButton.bezelColor = .controlAccentColor
            #endif
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

    // MARK: Filter

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let filter = model.library.filter
        func add(_ title: String, _ action: Selector, on: Bool, tag: Int = 0) {
            let item = menu.addItem(withTitle: title, action: action, keyEquivalent: "")
            item.target = self; item.tag = tag; item.state = on ? .on : .off
        }
        // A pull-down toolbar menu takes its first item as the button's title
        // and doesn't list it.
        menu.addItem(withTitle: "Filter", action: nil, keyEquivalent: "")
        menu.addItem(.sectionHeader(title: "Rating"))
        add("Any Rating", #selector(filterRating(_:)), on: filter.rating == .any, tag: -1)
        for stars in 1...5 {
            add(stars == 5 ? "★★★★★" : "≥ " + String(repeating: "★", count: stars), #selector(filterRating(_:)), on: filter.rating == .atLeast(stars), tag: stars)
        }
        add("Unrated Only", #selector(filterRating(_:)), on: filter.rating == .unrated, tag: 0)
        menu.addItem(.separator())
        menu.addItem(.sectionHeader(title: "Flags"))
        for (index, (flag, title)) in Self.flagChoices.enumerated() {
            add(title, #selector(filterFlag(_:)), on: filter.flags.contains(flag), tag: index)
        }
        menu.addItem(.separator())
        add("Edited Only", #selector(filterEdited), on: filter.editedOnly)
        menu.addItem(.separator())
        let clear = menu.addItem(withTitle: "Clear Filter", action: filter.active ? #selector(clearFilter) : nil, keyEquivalent: "l")
        clear.keyEquivalentModifierMask = [.command, .option]; clear.target = self
    }

    @objc private func filterRating(_ item: NSMenuItem) {
        model.library.filter.rating = switch item.tag { case -1: .any; case 0: .unrated; default: .atLeast(item.tag) }
    }
    @objc private func filterFlag(_ item: NSMenuItem) {
        let flag = Self.flagChoices[item.tag].flag
        if model.library.filter.flags.contains(flag) { model.library.filter.flags.remove(flag) } else { model.library.filter.flags.insert(flag) }
    }
    private static let flagChoices: [(flag: LibraryFilter.Flag, title: String)] = [(.pick, "Picks"), (.unflagged, "Unflagged"), (.reject, "Rejected")]
    @objc private func filterEdited() { model.library.filter.editedOnly.toggle() }
    @objc private func clearFilter() { model.library.filter = LibraryFilter() }

    static func describe(_ filter: LibraryFilter) -> String {
        var parts: [String] = []
        switch filter.rating {
        case .any: break
        case .atLeast(let stars): parts.append(stars == 5 ? "★★★★★" : "≥ " + String(repeating: "★", count: stars))
        case .unrated: parts.append("unrated")
        }
        let names = flagChoices.filter { filter.flags.contains($0.flag) }.map { $0.title.lowercased() }
        if !names.isEmpty { parts.append(names.joined(separator: " or ")) }
        if filter.editedOnly { parts.append("edited") }
        return parts.joined(separator: ", ")
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
    @objc private func toggleFilmstrip() { model.filmstripVisible.toggle() }
    @objc private func toggleAdjustments() { model.togglePane(.adjustments) }
    @objc private func togglePresets() { model.togglePane(.presets) }
}

private extension NSToolbarItem.Identifier {
    static let filmstrip = Self("filmstrip"), filter = Self("filter")
    static let open = Self("open"), zoom = Self("zoom"), edit = Self("edit")
    static let export = Self("export"), adjustments = Self("adjustments"), presets = Self("presets"), cancelCrop = Self("cancelCrop"), doneCrop = Self("doneCrop")
}
