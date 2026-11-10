import AppKit
import SwiftUI

/// The menu bar. The standard items go to the responder chain; the app's own
/// commands go to the model, and are enabled and titled from its state when a
/// menu opens.
@MainActor final class MainMenu: NSObject, NSMenuItemValidation, NSMenuDelegate {
    let menu = NSMenu()
    private let model: EditorModel
    private let updates: UpdateController
    private let removeAppData: () -> Void
    private let recent = NSMenu(title: "Open Recent")
    private var settings: SettingsWindowController?
    private var removingAppData = false

    init(model: EditorModel, updates: UpdateController, removeAppData: @escaping () -> Void) {
        self.model = model
        self.updates = updates
        self.removeAppData = removeAppData
        super.init()
        let name = ProcessInfo.processInfo.processName == "NegsApp" ? "Negs" : ProcessInfo.processInfo.processName

        let app = submenu(name)
        app.addItem(withTitle: "About \(name)", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        command(app, "Check for Updates…", #selector(checkForUpdates), "")
        app.addItem(.separator())
        command(app, "Settings…", #selector(showSettings), ",")
        app.addItem(.separator())
        let services = NSMenu(title: "Services")
        app.addItem(withTitle: "Services", action: nil, keyEquivalent: "").submenu = services
        NSApp.servicesMenu = services
        app.addItem(.separator())
        app.addItem(withTitle: "Hide \(name)", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        app.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h").keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Quit \(name)", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

        let file = submenu("File")
        command(file, "Open…", #selector(openPhotograph), "o")
        // The action only lets validation disable the submenu while it's empty.
        let recentItem = file.addItem(withTitle: "Open Recent", action: #selector(showRecent), keyEquivalent: "")
        recentItem.target = self
        recentItem.submenu = recent
        recent.delegate = self
        file.addItem(.separator())
        file.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        file.addItem(.separator())
        command(file, "Save Preview…", #selector(savePreview), "s")
        command(file, "Export Photograph…", #selector(exportPhotograph), "e", [.command, .shift])
        command(file, "Export LUT…", #selector(exportLUT), "l", [.command, .shift])
        file.addItem(.separator())
        command(file, "New Preset", #selector(newPreset), "s", [.command, .shift])
        command(file, "Import Presets…", #selector(importPresets), "o", [.command, .shift])
        command(file, "Show Presets Folder", #selector(showPresetsFolder), "")

        let edit = submenu("Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z").keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        // Without a text field or view taking them, ⌘C and ⌘V copy and paste
        // the photograph's edits: `EditorWindowController.copy(_:)`.
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Delete", action: #selector(NSText.delete(_:)), keyEquivalent: "")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        // The window selects photographs when no text field takes it.
        edit.addItem(withTitle: "Deselect All", action: #selector(EditorWindowController.deselectAll(_:)), keyEquivalent: "a")
            .keyEquivalentModifierMask = [.command, .option]

        let view = submenu("View")
        // Plain keys, as in Lightroom, behind the same check as the rating keys.
        command(view, "Edit Photograph", #selector(editPhotograph), "e", [])
        hidden(view, #selector(editPhotograph), "\r")
        command(view, "Show Grid", #selector(showGrid), "g", [])
        // Esc shows the grid too: `EditorWindowController.escapeToGrid`.
        command(view, "Larger Thumbnails", #selector(largerThumbnails), "+")
        hidden(view, #selector(largerThumbnails), "=", .command)
        command(view, "Smaller Thumbnails", #selector(smallerThumbnails), "-")
        view.addItem(.separator())
        // A plain key too: the photograph with only its caption and label.
        // Esc leaves it first: `EditorWindowController.escapeToGrid`.
        command(view, "Image Only", #selector(toggleImageOnly), "f", [])
        command(view, "Hide Film Strip", #selector(toggleFilmstrip), "f", [.command, .option])
        command(view, "Show Adjustments", #selector(toggleAdjustments), "i", [.command, .option])
        command(view, "Show Presets", #selector(togglePresets), "p", [.command, .option])
        // Every control in every module, as the Lab's Advanced controls switch did.
        command(view, "Show All Controls", #selector(toggleAllControls), "")
        view.addItem(.separator())
        command(view, "Fit Photograph", #selector(fitPhotograph), "0")
        command(view, "Actual Size", #selector(actualSize), "1")
        // Rotate lives in cropping (§4.10); from here it starts cropping.
        command(view, "Rotate Clockwise", #selector(rotate), "r")
        command(view, "Rotate Counterclockwise", #selector(rotateLeft), "r", [.command, .option])
        command(view, "Crop", #selector(crop), "k")
        view.addItem(.separator())
        // ⇧⌘R is Photo › Reset Edits.
        command(view, "Reset All Adjustments", #selector(resetAdjustments), "")
        view.addItem(.separator())

        // Plain keys, as in Lightroom and Capture One. Disabled while text is
        // typed, so a number typed into a field never rates the photograph.
        let photo = submenu("Photo")
        let rating = NSMenu(title: "Rating")
        photo.addItem(withTitle: "Rating", action: nil, keyEquivalent: "").submenu = rating
        for stars in 0...5 {
            let item = rating.addItem(withTitle: stars == 0 ? "None" : String(repeating: "★", count: stars),
                                      action: #selector(rate(_:)), keyEquivalent: "\(stars)")
            item.keyEquivalentModifierMask = []; item.tag = stars; item.target = self
            if stars == 0 { rating.addItem(.separator()) }
        }
        rating.addItem(.separator())
        command(rating, "Raise Rating", #selector(raiseRating), "]", [])
        command(rating, "Lower Rating", #selector(lowerRating), "[", [])
        let flag = NSMenu(title: "Flag")
        photo.addItem(withTitle: "Flag", action: nil, keyEquivalent: "").submenu = flag
        command(flag, "Pick", #selector(pick), "p", [])
        command(flag, "Reject", #selector(reject), "x", [])
        command(flag, "Unflagged", #selector(unflag), "u", [])
        photo.addItem(.separator())
        command(photo, "Copy Edits", #selector(copyEdits), "c", [.command, .shift])
        command(photo, "Copy Edits…", #selector(copyEditsChoosing), "c", [.command, .shift, .option])
        command(photo, "Paste Edits", #selector(pasteEdits), "v", [.command, .shift])
        command(photo, "Paste from Previous", #selector(pasteFromPrevious), "v", [.command, .shift, .option])
        photo.addItem(.separator())
        command(photo, "Duplicate", #selector(duplicate), "d")
        command(photo, "Reset Edits", #selector(resetEdits), "r", [.command, .shift])
        photo.addItem(.separator())
        // ⌫ asks first; holding ⌘ shows ⌘⌫, which doesn't. Both go to the Trash.
        command(photo, "Move to Trash…", #selector(moveToTrash), "\u{8}", [])
        command(photo, "Move to Trash", #selector(moveToTrashNow), "\u{8}", .command)
        photo.items.last?.isAlternate = true
        command(photo, "Delete Rejected Photographs…", #selector(deleteRejected), "")
        photo.addItem(.separator())
        command(photo, "Advance After Rating", #selector(toggleAdvance), "")
        photo.addItem(.separator())
        command(photo, "Clear Filter", #selector(clearFilter), "l", [.command, .option])

        let window = submenu("Window")
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        window.addItem(.separator())
        window.addItem(withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        NSApp.windowsMenu = window

        NSApp.helpMenu = submenu("Help")
    }

    private func submenu(_ title: String) -> NSMenu {
        let submenu = NSMenu(title: title)
        menu.addItem(withTitle: title, action: nil, keyEquivalent: "").submenu = submenu
        return submenu
    }

    private func command(_ menu: NSMenu, _ title: String, _ action: Selector, _ key: String, _ modifiers: NSEvent.ModifierFlags = .command) {
        let item = menu.addItem(withTitle: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = self
    }

    /// A second key for the command above it, not listed in the menu.
    private func hidden(_ menu: NSMenu, _ action: Selector, _ key: String, _ modifiers: NSEvent.ModifierFlags = []) {
        let item = menu.addItem(withTitle: "", action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = self
        item.isHidden = true
        item.allowsKeyEquivalentWhenHidden = true
    }

    // MARK: Commands

    @objc private func checkForUpdates() { updates.check() }
    @objc private func showSettings() {
        if settings == nil {
            settings = SettingsWindowController(updates: updates, model: model, removeAppData: removeAppData)
        }
        settings?.window?.makeKeyAndOrderFront(nil)
    }
    func prepareForAppDataRemoval() { removingAppData = true; settings?.window?.orderOut(nil) }
    @objc private func openPhotograph() { model.openPanel() }
    @objc private func showRecent() {}
    @objc private func openRecent(_ item: NSMenuItem) { if let url = item.representedObject as? URL { model.open(url) } }
    @objc private func savePreview() { model.savePanel(export: false) }
    @objc private func exportPhotograph() { model.exportPanel() }
    @objc private func exportLUT() { model.exportLUTPanel() }
    @objc private func toggleImageOnly() { model.toggleImageOnly() }
    @objc private func toggleFilmstrip() { model.toggleFilmstrip() }
    @objc private func toggleAdjustments() { model.togglePane(.adjustments) }
    @objc private func togglePresets() { model.togglePane(.presets) }
    @objc private func newPreset() { model.newPreset() }
    @objc private func importPresets() { model.importPresetsPanel() }
    @objc private func showPresetsFolder() { model.showPresetsFolder() }
    @objc private func toggleAllControls() { AdjustmentInspector.toggleAllControls() }
    @objc private func fitPhotograph() { model.zoom(fit: true) }
    @objc private func actualSize() { model.zoom(fit: false) }
    @objc private func rotate() { model.turn(clockwise: true) }
    @objc private func rotateLeft() { model.turn(clockwise: false) }
    @objc private func crop() { model.cropping ? model.endCrop(apply: true) : model.beginCrop() }
    @objc private func resetAdjustments() { model.reset() }
    @objc private func rate(_ item: NSMenuItem) { model.setRating(item.tag) }
    @objc private func raiseRating() { model.changeRating(by: 1) }
    @objc private func lowerRating() { model.changeRating(by: -1) }
    @objc private func pick() { model.setFlag(.pick) }
    @objc private func reject() { model.setFlag(.reject) }
    @objc private func unflag() { model.setFlag(nil) }
    @objc private func toggleAdvance() { model.advanceAfterRating.toggle() }
    @objc private func clearFilter() { model.library.filter = LibraryFilter() }
    @objc private func editPhotograph() { model.editCurrent() }
    @objc private func copyEdits() { model.copyEdits() }
    @objc private func pasteEdits() { model.pasteEdits() }
    @objc private func pasteFromPrevious() { model.pasteFromPrevious() }
    @objc private func duplicate() { model.duplicate() }
    @objc private func resetEdits() { model.resetEdits() }
    @objc private func moveToTrash() { model.requestDelete(asking: true) }
    @objc private func moveToTrashNow() { model.requestDelete(asking: false) }
    @objc private func deleteRejected() { model.requestDelete(model.rejected, asking: true) }

    /// Copy Edits…: the groups to copy, in the alert sheet; the choice is remembered.
    @objc private func copyEditsChoosing() {
        guard NSApp.keyWindow is EditorWindow else { return }
        model.copyEditsChoosing()
    }
    @objc private func showGrid() { model.showGrid() }
    @objc private func largerThumbnails() { model.changeGridCellSize(larger: true) }
    @objc private func smallerThumbnails() { model.changeGridCellSize(larger: false) }

    /// Rating and flag keys act on the photograph in the editing window,
    /// and not while a text field there (or any other window) has the keys.
    private var rating: Bool { !model.ratingTargets.isEmpty && Self.takesRatingKeys(NSApp.keyWindow) }
    /// On when every photograph a rating command acts on passes `test`,
    /// mixed when some do.
    private func state(_ test: (LibrarySummary) -> Bool) -> NSControl.StateValue {
        let summaries = model.ratingTargets.map { model.library.summary(for: $0) }
        let passing = summaries.filter(test).count
        return passing == 0 ? .off : passing == summaries.count ? .on : .mixed
    }
    /// ⌫ also deletes the selected preset while the presets list has the keys.
    static func takesDeleteKey(_ window: NSWindow?) -> Bool {
        guard takesRatingKeys(window) else { return false }
        var view = window?.firstResponder as? NSView
        while let next = view {
            // A list, or a focused control that deletes its own (a preset tile).
            if next is NSTableView { return false }
            if let anchor = next as? FocusAnchorView, anchor.delete != nil { return false }
            view = next.superview
        }
        return true
    }
    /// The same check as holding \ for the original.
    static func takesRatingKeys(_ window: NSWindow?) -> Bool {
        guard let window, window is EditorWindow else { return false }
        return !(window.firstResponder is NSText)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        // The alert sheet is answered first, as a system alert's would be.
        if removingAppData || model.alert != nil { return false }
        let photo = model.source != nil
        // The grid shows no photograph to adjust, zoom or crop.
        let editing = photo && model.mode == .photo
        switch item.action {
        case #selector(checkForUpdates): return updates.configured && updates.canCheck
        case #selector(showRecent): return !model.recentFiles.isEmpty
        case #selector(savePreview): return model.canSave && model.mode == .photo
        case #selector(exportPhotograph):
            item.title = model.exportTitle
            return model.canExport && !model.cropping
        case #selector(exportLUT): return model.canExportLUT && model.mode == .photo
        case #selector(toggleImageOnly):
            item.state = model.imageOnly ? .on : .off
            // In the grid it opens the current photograph in Image Only.
            let shown = model.mode == .grid ? model.selection.current != nil : photo
            return Self.takesRatingKeys(NSApp.keyWindow) && (model.imageOnly || (shown && !model.cropping))
        case #selector(toggleFilmstrip):
            item.title = model.stripShown ? "Hide Film Strip" : "Show Film Strip"
            return model.mode == .photo
        case #selector(toggleAdjustments):
            item.state = model.inspectorShown && model.inspectorPane == .adjustments ? .on : .off; return model.mode == .photo
        case #selector(togglePresets):
            item.state = model.inspectorShown && model.inspectorPane == .presets ? .on : .off; return model.mode == .photo
        case #selector(newPreset): return model.canChangePresets && model.mode == .photo
        case #selector(importPresets): return model.canChangePresets
        case #selector(showPresetsFolder): return true
        case #selector(toggleAllControls):
            item.state = UserDefaults.standard.bool(forKey: AdjustmentInspector.allControlsKey) ? .on : .off
            return model.mode == .photo
        case #selector(fitPhotograph), #selector(actualSize): return editing && !model.cropping
        case #selector(rotate), #selector(rotateLeft): return model.cropping || (editing && model.image != nil)
        case #selector(crop):
            item.title = model.cropping ? "Done Cropping" : "Crop"
            return model.cropping || (model.image != nil && model.mode == .photo)
        case #selector(resetAdjustments): return model.hasCatalog && !model.cropping && model.mode == .photo
        case #selector(editPhotograph): return model.mode == .grid && model.selection.current != nil && Self.takesRatingKeys(NSApp.keyWindow)
        case #selector(showGrid): return editing && !model.cropping && Self.takesRatingKeys(NSApp.keyWindow)
        case #selector(largerThumbnails): return model.mode == .grid && model.gridCellSize < SheetLayout.sizes.upperBound
        case #selector(smallerThumbnails): return model.mode == .grid && model.gridCellSize > SheetLayout.sizes.lowerBound
        case #selector(copyEdits), #selector(copyEditsChoosing):
            return model.canCopyEdits && Self.takesRatingKeys(NSApp.keyWindow)
        case #selector(pasteEdits):
            return model.canPasteEdits && Self.takesRatingKeys(NSApp.keyWindow)
        case #selector(pasteFromPrevious):
            return model.previousEdits != nil && model.canChangeEdits && !model.editTargets.isEmpty && Self.takesRatingKeys(NSApp.keyWindow)
        case #selector(duplicate), #selector(resetEdits):
            return model.canChangeEdits && !model.editTargets.isEmpty && Self.takesRatingKeys(NSApp.keyWindow)
        case #selector(moveToTrash), #selector(moveToTrashNow):
            let count = model.deleteTargets.count
            let title = count > 1 ? "Move \(count) Photographs to Trash" : "Move to Trash"
            item.title = item.action == #selector(moveToTrash) ? title + "…" : title
            return model.canDelete && count > 0 && Self.takesDeleteKey(NSApp.keyWindow)
        case #selector(deleteRejected): return model.canDelete && !model.rejected.isEmpty
        case #selector(rate(_:)):
            item.state = state { $0.rating == item.tag }; return rating
        case #selector(raiseRating), #selector(lowerRating): return rating
        case #selector(pick): item.state = state { $0.flag == .pick }; return rating
        case #selector(reject): item.state = state { $0.flag == .reject }; return rating
        case #selector(unflag): item.state = state { $0.flag == nil }; return rating
        case #selector(toggleAdvance): item.state = model.advanceAfterRating ? .on : .off; return true
        case #selector(clearFilter): return model.library.filter.active
        default: return true
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        for url in model.recentFiles {
            let item = menu.addItem(withTitle: url.lastPathComponent, action: #selector(openRecent), keyEquivalent: "")
            item.target = self; item.representedObject = url; item.toolTip = url.path
        }
    }
}
